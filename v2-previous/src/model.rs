use crate::infer::{common as common_types, unify as unify_types, value_type};
use crate::{Diagnostic, Id, Span, Value, syntax};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Type {
    Any,
    Number,
    Quantity,
    Date,
    Text,
    Bool,
    Ref(String),
    List(Box<Type>),
    Record(BTreeMap<String, Type>),
}

impl Type {
    pub fn validate(&self, value: &Value) -> Result<(), String> {
        value.validate()?;
        self.validate_inner(value, 0)
    }

    fn validate_inner(&self, value: &Value, depth: usize) -> Result<(), String> {
        if depth > 64 {
            return Err("type nesting limit reached".into());
        }
        if matches!(value, Value::Hole(_)) {
            return Ok(());
        }
        match (self, value) {
            (Self::Any, _) => Ok(()),
            (Self::Number, Value::Number(_))
            | (Self::Quantity, Value::Quantity(_, _))
            | (Self::Date, Value::Date(_))
            | (Self::Text, Value::Text(_))
            | (Self::Bool, Value::Bool(_))
            | (Self::Ref(_), Value::Ref(_)) => Ok(()),
            (Self::List(item), Value::List(values)) => {
                for value in values {
                    item.validate_inner(value, depth + 1)?;
                }
                Ok(())
            }
            (Self::Record(fields), Value::Record(values)) => {
                if fields.len() != values.len() {
                    return Err("record fields do not match the declared shape".into());
                }
                for (name, ty) in fields {
                    let value = values
                        .get(name)
                        .ok_or_else(|| format!("record is missing field `{name}`"))?;
                    ty.validate_inner(value, depth + 1)
                        .map_err(|error| format!("field `{name}`: {error}"))?;
                }
                Ok(())
            }
            _ => Err(format!("expected {self}, found {value}")),
        }
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Any => f.write_str("any"),
            Self::Number => f.write_str("number"),
            Self::Quantity => f.write_str("quantity"),
            Self::Date => f.write_str("date"),
            Self::Text => f.write_str("text"),
            Self::Bool => f.write_str("bool"),
            Self::Ref(name) => write!(f, "ref:{name}"),
            Self::List(item) => write!(f, "list<{item}>"),
            Self::Record(fields) => {
                f.write_str("record{")?;
                for (index, (name, ty)) in fields.iter().enumerate() {
                    if index != 0 {
                        f.write_str(",")?;
                    }
                    write!(f, "{name}:{ty}")?;
                }
                f.write_str("}")
            }
        }
    }
}

fn package_name(document: &syntax::Document) -> Result<String, Vec<Diagnostic>> {
    let declarations: Vec<_> = document
        .blocks
        .iter()
        .filter(|block| block.head == "package")
        .collect();
    if declarations.len() != 1 {
        return Err(vec![diag(
            1,
            "each package source must declare exactly one `package`",
        )]);
    }
    let declaration = declarations[0];
    let name = declaration.args.trim();
    if !valid_qualified_name(name) || !declaration.fields.is_empty() {
        return Err(vec![diag(
            declaration.span.line,
            "invalid package declaration",
        )]);
    }
    Ok(name.to_owned())
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Schema {
    pub name: String,
    pub fields: BTreeMap<String, Type>,
    pub authored: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct Pattern {
    name: String,
    capture: String,
    date_field: Option<String>,
    fields: BTreeMap<String, PatternValue>,
    #[serde(skip)]
    span: Span,
    #[serde(skip)]
    field_spans: BTreeMap<String, Span>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
enum PatternValue {
    Capture(String),
    Quantity(Box<PatternValue>, String),
    Reference(String),
    Default(Value),
    OpenList(Box<PatternValue>),
    List(Vec<PatternValue>),
    Record(BTreeMap<String, PatternValue>),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Expr {
    Literal(Value),
    Name(String),
    Call(String, Vec<Expr>),
}

impl Expr {
    pub fn parse(source: &str) -> Result<Self, String> {
        if source.len() > 64 * 1024 {
            return Err("expression exceeds the size limit".into());
        }
        if let Ok(value @ Value::Quantity(_, _)) = Value::parse(source.trim()) {
            return Ok(Expr::Literal(value));
        }
        let mut parser = ExprParser::new(source);
        let expr = parser.one()?;
        parser.space();
        if parser.at != source.len() {
            return Err("unexpected text after expression".into());
        }
        Ok(expr)
    }

    fn pair(source: &str) -> Result<(Self, Self), String> {
        if source.len() > 64 * 1024 {
            return Err("expression exceeds the size limit".into());
        }
        let mut parser = ExprParser::new(source);
        let first = parser.one()?;
        let second = parser.one()?;
        parser.space();
        if parser.at != source.len() {
            return Err("expected exactly two expressions".into());
        }
        Ok((first, second))
    }
}

struct ExprParser<'a> {
    source: &'a str,
    at: usize,
    depth: usize,
    nodes: usize,
}

impl<'a> ExprParser<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            at: 0,
            depth: 0,
            nodes: 0,
        }
    }

    fn space(&mut self) {
        while self.source[self.at..]
            .chars()
            .next()
            .is_some_and(char::is_whitespace)
        {
            self.at += self.source[self.at..].chars().next().unwrap().len_utf8();
        }
    }

    fn one(&mut self) -> Result<Expr, String> {
        self.nodes += 1;
        if self.nodes > 10_000 {
            return Err("expression contains too many nodes".into());
        }
        if self.depth > 64 {
            return Err("expression nesting limit reached".into());
        }
        self.space();
        let Some(ch) = self.source[self.at..].chars().next() else {
            return Err("expected expression".into());
        };
        if ch == '(' {
            return self.call();
        }
        let end = match ch {
            '"' => self.quoted_end()?,
            '[' | '{' => self.container_end(ch)?,
            _ => self.atom_end(),
        };
        let atom = &self.source[self.at..end];
        self.at = end;
        if looks_like_name(atom) {
            return Ok(Expr::Name(atom.to_owned()));
        }
        Value::parse(atom).map(Expr::Literal)
    }

    fn call(&mut self) -> Result<Expr, String> {
        self.at += 1;
        self.space();
        let start = self.at;
        while self.source[self.at..]
            .chars()
            .next()
            .is_some_and(is_ident_char)
        {
            self.at += self.source[self.at..].chars().next().unwrap().len_utf8();
        }
        if self.at == start {
            return Err("expected operator after `(`".into());
        }
        let operator = self.source[start..self.at].to_owned();
        let mut args = Vec::new();
        self.depth += 1;
        if self.depth > 64 {
            return Err("expression nesting limit reached".into());
        }
        loop {
            self.space();
            match self.source[self.at..].chars().next() {
                Some(')') => {
                    self.at += 1;
                    break;
                }
                None => return Err("unclosed expression".into()),
                _ => args.push(self.one()?),
            }
        }
        self.depth -= 1;
        Ok(Expr::Call(operator, args))
    }

    fn atom_end(&self) -> usize {
        self.source[self.at..]
            .char_indices()
            .find(|(_, ch)| ch.is_whitespace() || *ch == ')' || *ch == ',' || *ch == ']')
            .map(|(i, _)| self.at + i)
            .unwrap_or(self.source.len())
    }

    fn quoted_end(&self) -> Result<usize, String> {
        let mut escaped = false;
        for (offset, ch) in self.source[self.at + 1..].char_indices() {
            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                continue;
            }
            if ch == '"' {
                return Ok(self.at + 1 + offset + 1);
            }
        }
        Err("unterminated quoted text".into())
    }

    fn container_end(&self, open: char) -> Result<usize, String> {
        let close = if open == '[' { ']' } else { '}' };
        let mut depth = 0usize;
        let mut quoted = false;
        let mut escaped = false;
        for (offset, ch) in self.source[self.at..].char_indices() {
            if quoted {
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '"' {
                    quoted = false;
                }
                continue;
            }
            if ch == '"' {
                quoted = true;
                continue;
            }
            if ch == open {
                depth += 1;
            }
            if ch == close {
                depth -= 1;
                if depth == 0 {
                    return Ok(self.at + offset + ch.len_utf8());
                }
            }
        }
        Err(format!("unclosed `{open}` literal"))
    }
}

fn is_ident_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.')
}

fn valid_binding_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        && value
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_')
}

fn looks_like_name(atom: &str) -> bool {
    let Some(first) = atom.chars().next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_')
        && atom.chars().all(is_ident_char)
        && !matches!(atom, "true" | "false")
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Instruction {
    Let(String, Expr),
    Choose(String, Expr, Expr),
    Require(Expr),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub name: String,
    pub source: String,
    pub binding: String,
    pub steps: Vec<Instruction>,
    pub output: String,
    pub fields: BTreeMap<String, Expr>,
    pub book: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Model {
    pub(crate) packages: BTreeMap<String, String>,
    pub(crate) schemas: BTreeMap<String, Schema>,
    patterns: BTreeMap<String, Pattern>,
    pub(crate) rules: Vec<Rule>,
    pub(crate) books: BTreeMap<String, Vec<String>>,
    schema_packages: BTreeMap<String, String>,
    rule_packages: BTreeMap<String, String>,
    book_packages: BTreeMap<String, String>,
    package_deps: BTreeMap<String, BTreeSet<String>>,
    #[serde(skip)]
    rule_spans: BTreeMap<String, Span>,
    #[serde(skip)]
    rule_step_spans: BTreeMap<String, Vec<Span>>,
    #[serde(skip)]
    package_indices: BTreeMap<String, usize>,
    #[serde(skip)]
    definition_ids: BTreeMap<String, Id>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Row {
    pub id: String,
    pub schema: String,
    pub fields: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    pub id: String,
    pub target: String,
    pub value: Value,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TypedDocument {
    pub name: String,
    pub rows: Vec<Row>,
    pub decisions: Vec<Decision>,
    pub locations: BTreeMap<String, Span>,
    #[serde(default)]
    pub field_locations: BTreeMap<String, Span>,
    pub inferred: BTreeMap<String, Value>,
}

impl Model {
    pub fn compile(sources: &[String]) -> Result<Self, Vec<Diagnostic>> {
        let mut diagnostics = Vec::new();
        let mut packages = BTreeMap::new();
        let mut parsed = BTreeMap::new();
        let mut package_indices = BTreeMap::new();
        for (source_index, source) in sources.iter().enumerate() {
            match syntax::parse(source) {
                Ok(document) => {
                    let package = match package_name(&document) {
                        Ok(package) => package,
                        Err(errors) => {
                            diagnostics.extend(source_diagnostics(errors, source_index));
                            continue;
                        }
                    };
                    if packages.insert(package.clone(), source.clone()).is_some() {
                        diagnostics.push(
                            diag(1, format!("duplicate package `{package}`"))
                                .in_source(source_index),
                        );
                    } else {
                        package_indices.insert(package.clone(), source_index);
                        parsed.insert(package, document);
                    }
                }
                Err(errors) => diagnostics.extend(source_diagnostics(errors, source_index)),
            }
        }
        if !diagnostics.is_empty() {
            return Err(diagnostics);
        }
        if packages.is_empty() {
            return Err(vec![diag(1, "model needs at least one package source")]);
        }

        let mut deps = BTreeMap::<String, BTreeSet<String>>::new();
        for (package, document) in &parsed {
            let diagnostic_start = diagnostics.len();
            let mut names = BTreeSet::new();
            for block in &document.blocks {
                if block.head == "use" {
                    let dependency = block.args.trim();
                    if !valid_qualified_name(dependency) || !block.fields.is_empty() {
                        diagnostics.push(diag(block.span.line, "invalid package dependency"));
                    } else if !names.insert(dependency.to_owned()) {
                        diagnostics.push(diag(
                            block.span.line,
                            format!("duplicate dependency `{dependency}`"),
                        ));
                    }
                }
            }
            deps.insert(package.clone(), names);
            if let Some(source_index) = package_indices.get(package) {
                tag_diagnostics(&mut diagnostics, diagnostic_start, *source_index);
            }
        }
        for (package, names) in &deps {
            let diagnostic_start = diagnostics.len();
            for name in names {
                if !packages.contains_key(name) {
                    diagnostics.push(diag(
                        1,
                        format!("package `{package}` depends on missing package `{name}`"),
                    ));
                }
            }
            if let Some(source_index) = package_indices.get(package) {
                tag_diagnostics(&mut diagnostics, diagnostic_start, *source_index);
            }
        }
        if !diagnostics.is_empty() {
            return Err(diagnostics);
        }
        if let Some(cycle) = dependency_cycle(&deps) {
            let source_index = cycle
                .first()
                .and_then(|package| package_indices.get(package))
                .copied();
            let mut diagnostic = diag(
                1,
                format!("package dependency cycle: {}", cycle.join(" -> ")),
            );
            if let Some(source_index) = source_index {
                diagnostic.source_index = Some(source_index);
            }
            diagnostics.push(diagnostic);
            return Err(diagnostics);
        }
        let mut schemas = BTreeMap::new();
        let mut patterns = BTreeMap::new();
        let mut schema_packages = BTreeMap::new();
        let mut book_defs = Vec::new();
        let mut rule_defs = Vec::new();
        for (package, document) in &parsed {
            let diagnostic_start = diagnostics.len();
            for block in &document.blocks {
                match block.head.as_str() {
                    "package" | "use" => {}
                    "rule" => rule_defs.push((package.clone(), block.clone())),
                    "book" => book_defs.push((package.clone(), block.clone())),
                    "form" | "fact" => diagnostics.push(diag(
                        block.span.line,
                        "authored schema declarations are removed; use an ordinary entry pattern",
                    )),
                    _ => match compile_pattern(block) {
                        Ok((pattern, schema)) => {
                            let schema_name = schema.name.clone();
                            if patterns.insert(schema_name.clone(), pattern).is_some()
                                || schemas.insert(schema_name.clone(), schema).is_some()
                            {
                                diagnostics.push(diag(
                                    block.span.line,
                                    format!("duplicate entry pattern `{schema_name}`"),
                                ));
                            } else {
                                schema_packages.insert(schema_name, package.clone());
                            }
                        }
                        Err(errors) => diagnostics.extend(errors),
                    },
                }
            }
            if let Some(source_index) = package_indices.get(package) {
                tag_diagnostics(&mut diagnostics, diagnostic_start, *source_index);
            }
        }
        if !diagnostics.is_empty() {
            return Err(diagnostics);
        }

        for (schema_name, pattern) in &patterns {
            for field in pattern.fields.values() {
                for target in pattern_reference_targets(field) {
                    if !patterns.contains_key(&target) {
                        diagnostics.push(
                            diag(
                                1,
                                format!(
                                    "pattern `{schema_name}` refers to unknown pattern `{target}`"
                                ),
                            )
                            .in_source(package_indices[&schema_packages[schema_name]]),
                        );
                    } else if !package_can_see(
                        &schema_packages[schema_name],
                        &schema_packages[&target],
                        &deps,
                    ) {
                        diagnostics.push(
                            diag(
                                1,
                                format!(
                                    "package `{}` must import `{}` to reference schema `{target}`",
                                    schema_packages[schema_name], schema_packages[&target]
                                ),
                            )
                            .in_source(package_indices[&schema_packages[schema_name]]),
                        );
                    }
                }
            }
        }
        if !diagnostics.is_empty() {
            return Err(diagnostics);
        }

        let mut books = BTreeMap::new();
        let mut book_packages = BTreeMap::new();
        for (package, block) in book_defs {
            let diagnostic_start = diagnostics.len();
            let source_index = package_indices.get(&package).copied();
            let name = block.args.trim();
            if !valid_qualified_name(name) {
                let mut diagnostic = diag(block.span.line, "book needs one valid name");
                if let Some(source_index) = source_index {
                    diagnostic.source_index = Some(source_index);
                }
                diagnostics.push(diagnostic);
                continue;
            }
            let mut includes = Vec::new();
            let mut seen = BTreeSet::new();
            for field in &block.fields {
                if field.name != "include" {
                    diagnostics.push(diag(
                        field.span.line,
                        "book accepts only `include schema` fields",
                    ));
                    continue;
                }
                let included = field.value.trim();
                if !valid_qualified_name(included) {
                    diagnostics.push(diag(field.span.line, "book include needs a relation name"));
                } else if !seen.insert(included.to_owned()) {
                    diagnostics.push(diag(
                        field.span.line,
                        format!("duplicate book include `{included}`"),
                    ));
                } else {
                    includes.push(included.to_owned());
                }
            }
            if books.insert(name.to_owned(), includes).is_some() {
                diagnostics.push(diag(block.span.line, format!("duplicate book `{name}`")));
            } else {
                book_packages.insert(name.to_owned(), package);
            }
            if let Some(source_index) = source_index {
                tag_diagnostics(&mut diagnostics, diagnostic_start, source_index);
            }
        }

        let mut raw_rules = BTreeMap::new();
        let mut rule_packages = BTreeMap::new();
        for (package, block) in rule_defs {
            let diagnostic_start = diagnostics.len();
            let source_index = package_indices.get(&package).copied();
            let name = block.args.trim();
            if !valid_qualified_name(name) {
                let mut diagnostic = diag(block.span.line, "rule needs one valid name");
                if let Some(source_index) = source_index {
                    diagnostic.source_index = Some(source_index);
                }
                diagnostics.push(diagnostic);
                continue;
            }
            if raw_rules.insert(name.to_owned(), block.clone()).is_some() {
                diagnostics.push(diag(block.span.line, format!("duplicate rule `{name}`")));
            } else {
                rule_packages.insert(name.to_owned(), package);
            }
            if let Some(source_index) = source_index {
                tag_diagnostics(&mut diagnostics, diagnostic_start, source_index);
            }
        }
        if !diagnostics.is_empty() {
            return Err(diagnostics);
        }

        // Output names are relations too, so consumers may refer to them before
        // their producers are compiled. Their fields are inferred below.
        for (rule_name, block) in &raw_rules {
            if let Some(output) = block
                .fields
                .iter()
                .find(|field| field.name == "emit")
                .map(|field| field.value.trim())
                && valid_qualified_name(output)
                && !schemas.contains_key(output)
            {
                schemas.insert(
                    output.to_owned(),
                    Schema {
                        name: output.to_owned(),
                        fields: BTreeMap::new(),
                        authored: false,
                    },
                );
                schema_packages.insert(output.to_owned(), rule_packages[rule_name].clone());
            }
        }

        let mut rules_by_name = BTreeMap::new();
        for (name, block) in &raw_rules {
            match compile_rule(name, block, &schemas, &books) {
                Ok(rule) => {
                    rules_by_name.insert(name.clone(), rule);
                }
                Err(errors) => {
                    let source_index = rule_packages
                        .get(name)
                        .and_then(|package| package_indices.get(package))
                        .copied();
                    diagnostics.extend(errors.into_iter().map(|mut diagnostic| {
                        if let Some(source_index) = source_index {
                            diagnostic.source_index = Some(source_index);
                        }
                        diagnostic
                    }));
                }
            }
        }
        if !diagnostics.is_empty() {
            return Err(diagnostics);
        }

        let mut rules_by_output = BTreeMap::<String, Vec<String>>::new();
        for (name, rule) in &rules_by_name {
            if patterns.contains_key(&rule.output) {
                diagnostics.push(
                    diag(
                        1,
                        format!(
                            "rule `{name}` output `{}` conflicts with an entry pattern",
                            rule.output
                        ),
                    )
                    .in_source(package_indices[&rule_packages[name]]),
                );
            }
            if !valid_qualified_name(&rule.output) {
                diagnostics.push(
                    diag(1, format!("rule `{name}` has an invalid output relation"))
                        .in_source(package_indices[&rule_packages[name]]),
                );
            }
            rules_by_output
                .entry(rule.output.clone())
                .or_default()
                .push(name.clone());
        }
        for (schema, producers) in &rules_by_output {
            let scopes: BTreeSet<_> = producers
                .iter()
                .map(|name| rules_by_name[name].book.as_deref())
                .collect();
            if scopes.len() > 1 {
                diagnostics.push(
                    diag(
                        1,
                        format!("rules producing `{schema}` must belong to the same world or book"),
                    )
                    .in_source(package_indices[&rule_packages[&producers[0]]]),
                );
            }
        }
        for (output, producers) in &rules_by_output {
            let Some(first_name) = producers.first() else {
                continue;
            };
            let first = &rules_by_name[first_name];
            let expected: BTreeSet<_> = first.fields.keys().cloned().collect();
            if expected.is_empty() {
                diagnostics.push(
                    diag(
                        raw_rules.get(first_name).map_or(1, |block| block.span.line),
                        format!("emitted relation `{output}` must set at least one field"),
                    )
                    .in_source(package_indices[&rule_packages[first_name]]),
                );
            }
            for producer in producers.iter().skip(1) {
                let actual: BTreeSet<_> = rules_by_name[producer].fields.keys().cloned().collect();
                if actual != expected {
                    diagnostics.push(
                        diag(
                            raw_rules.get(producer).map_or(1, |block| block.span.line),
                            format!("all producers of `{output}` must set the same fields"),
                        )
                        .in_source(package_indices[&rule_packages[producer]]),
                    );
                }
            }
            let fields = expected
                .iter()
                .map(|field| (field.clone(), Type::Any))
                .collect();
            schemas.insert(
                output.clone(),
                Schema {
                    name: output.clone(),
                    fields,
                    authored: false,
                },
            );
            let owner = rule_packages.get(first_name).cloned().unwrap_or_default();
            schema_packages.insert(output.clone(), owner);
        }
        for (book, included) in &books {
            let package = &book_packages[book];
            for schema in included {
                if !schemas.contains_key(schema) {
                    diagnostics.push(
                        diag(
                            1,
                            format!("book `{book}` includes unknown relation `{schema}`"),
                        )
                        .in_source(package_indices[&book_packages[book]]),
                    );
                } else if !package_can_see(package, &schema_packages[schema], &deps) {
                    diagnostics.push(diag(
                        1,
                        format!(
                            "package `{package}` must import `{}` to include relation `{schema}`",
                            schema_packages[schema]
                        ),
                    ).in_source(package_indices[package]));
                }
            }
        }
        for (name, rule) in &rules_by_name {
            if rule.book.is_some()
                && rule
                    .book
                    .as_ref()
                    .is_some_and(|book| !books.contains_key(book))
            {
                diagnostics.push(
                    diag(1, format!("rule `{name}` names an unknown book"))
                        .in_source(package_indices[&rule_packages[name]]),
                );
            }
            if let Some(book) = &rule.book
                && books
                    .get(book)
                    .is_some_and(|included| !included.contains(&rule.output))
            {
                diagnostics.push(
                    diag(
                        1,
                        format!(
                            "book `{book}` does not include output relation `{}`",
                            rule.output
                        ),
                    )
                    .in_source(package_indices[&rule_packages[name]]),
                );
            }
        }
        for (name, rule) in &rules_by_name {
            let package = &rule_packages[name];
            let line = raw_rules.get(name).map_or(1, |block| block.span.line);
            let mut relations = BTreeSet::from([rule.source.clone(), rule.output.clone()]);
            for step in &rule.steps {
                match step {
                    Instruction::Let(_, expr) | Instruction::Require(expr) => {
                        collect_rows(expr, &mut relations)
                    }
                    Instruction::Choose(_, selector, candidates) => {
                        collect_rows(selector, &mut relations);
                        collect_rows(candidates, &mut relations);
                    }
                }
            }
            for expr in rule.fields.values() {
                collect_rows(expr, &mut relations);
            }
            for schema in relations {
                if let Some(owner) = schema_packages.get(&schema)
                    && !package_can_see(package, owner, &deps)
                {
                    diagnostics.push(diag(line, format!("package `{package}` must import `{owner}` to use schema `{schema}` in rule `{name}`"))
                        .in_source(package_indices[package]));
                }
            }
            if let Some(book) = &rule.book
                && let Some(owner) = book_packages.get(book)
                && !package_can_see(package, owner, &deps)
            {
                diagnostics.push(
                    diag(
                        line,
                        format!("package `{package}` must import `{owner}` to use book `{book}`"),
                    )
                    .in_source(package_indices[package]),
                );
            }
        }
        if !diagnostics.is_empty() {
            return Err(diagnostics);
        }

        let mut names = BTreeSet::new();
        for name in schemas
            .keys()
            .chain(rules_by_name.keys())
            .chain(books.keys())
            .chain(packages.keys())
        {
            if !names.insert(name.as_str()) {
                let source_index = schema_packages
                    .get(name)
                    .or_else(|| rule_packages.get(name))
                    .or_else(|| book_packages.get(name))
                    .and_then(|package| package_indices.get(package))
                    .or_else(|| package_indices.get(name))
                    .copied();
                let mut diagnostic = diag(
                    1,
                    format!("definition name `{name}` is ambiguous across model definitions"),
                );
                if let Some(source_index) = source_index {
                    diagnostic.source_index = Some(source_index);
                }
                diagnostics.push(diagnostic);
            }
        }
        if !diagnostics.is_empty() {
            return Err(diagnostics);
        }

        let order = rule_order(
            &rules_by_name,
            &rules_by_output,
            &schemas,
            &raw_rules,
            &rule_packages,
            &package_indices,
            &mut diagnostics,
        );
        if !diagnostics.is_empty() {
            return Err(diagnostics);
        }
        let rules = order
            .into_iter()
            .filter_map(|name| rules_by_name.remove(&name))
            .collect();
        let rules: Vec<Rule> = rules;
        if let Err(errors) = crate::infer::infer_outputs(&mut schemas, &rules, &raw_rules) {
            return Err(errors
                .into_iter()
                .map(|(name, mut error)| {
                    if let Some(package) = rule_packages.get(&name)
                        && let Some(source_index) = package_indices.get(package)
                    {
                        error.source_index = Some(*source_index);
                    }
                    error
                })
                .collect());
        }

        let mut model = Self {
            packages,
            schemas,
            patterns,
            rules,
            books,
            schema_packages,
            rule_packages,
            book_packages,
            package_deps: deps,
            rule_spans: raw_rules
                .iter()
                .map(|(name, block)| (name.clone(), block.span.clone()))
                .collect(),
            rule_step_spans: raw_rules
                .iter()
                .map(|(name, block)| {
                    (
                        name.clone(),
                        block
                            .fields
                            .iter()
                            .filter(|field| {
                                matches!(field.name.as_str(), "let" | "choose" | "require")
                            })
                            .map(|field| field.span.clone())
                            .collect(),
                    )
                })
                .collect(),
            package_indices,
            definition_ids: BTreeMap::new(),
        };
        model.definition_ids = model.compute_definition_ids();
        Ok(model)
    }

    pub fn id(&self) -> Id {
        Id::of("axiom.v2.model", self)
    }

    pub fn definition_id(&self, name: &str) -> Id {
        self.definition_ids.get(name).cloned().unwrap_or_else(|| {
            #[derive(Serialize)]
            struct Missing<'a> {
                name: &'a str,
            }
            Id::of("axiom.v2.definition", &Missing { name })
        })
    }

    fn compute_definition_ids(&self) -> BTreeMap<String, Id> {
        #[derive(Serialize)]
        #[serde(tag = "kind", content = "definition")]
        enum Definition<'a> {
            Schema(SchemaDefinition<'a>),
            Rule(&'a Rule),
            Book(Vec<String>),
            Package(PackageDefinition<'a>),
        }
        #[derive(Serialize)]
        struct PackageDefinition<'a> {
            name: &'a str,
            dependencies: Vec<&'a str>,
            schemas: Vec<&'a Schema>,
            patterns: Vec<&'a Pattern>,
            rules: Vec<&'a Rule>,
            books: Vec<(&'a str, Vec<String>)>,
        }
        #[derive(Serialize)]
        struct SchemaDefinition<'a> {
            shape: &'a Schema,
            pattern: Option<&'a Pattern>,
        }
        let mut ids = BTreeMap::new();
        for (name, schema) in &self.schemas {
            ids.insert(
                name.clone(),
                Id::of(
                    "axiom.v2.definition",
                    &Definition::Schema(SchemaDefinition {
                        shape: schema,
                        pattern: self.patterns.get(name),
                    }),
                ),
            );
        }
        for rule in &self.rules {
            ids.insert(
                rule.name.clone(),
                Id::of("axiom.v2.definition", &Definition::Rule(rule)),
            );
        }
        for (name, schemas) in &self.books {
            let mut included = schemas.clone();
            included.sort();
            ids.insert(
                name.clone(),
                Id::of("axiom.v2.definition", &Definition::Book(included)),
            );
        }
        for name in self.packages.keys() {
            let mut books: Vec<_> = self
                .book_packages
                .iter()
                .filter(|(_, owner)| *owner == name)
                .filter_map(|(book, _)| {
                    self.books.get(book).map(|schemas| {
                        let mut included = schemas.clone();
                        included.sort();
                        (book.as_str(), included)
                    })
                })
                .collect();
            books.sort_by(|left, right| left.0.cmp(right.0));
            let definition = PackageDefinition {
                name,
                dependencies: self
                    .package_deps
                    .get(name)
                    .into_iter()
                    .flatten()
                    .map(String::as_str)
                    .collect(),
                schemas: self
                    .schema_packages
                    .iter()
                    .filter(|(_, owner)| *owner == name)
                    .filter_map(|(schema, _)| self.schemas.get(schema))
                    .collect(),
                patterns: self
                    .schema_packages
                    .iter()
                    .filter(|(_, owner)| *owner == name)
                    .filter_map(|(pattern, _)| self.patterns.get(pattern))
                    .collect(),
                rules: self
                    .rule_packages
                    .iter()
                    .filter(|(_, owner)| *owner == name)
                    .filter_map(|(rule, _)| {
                        self.rules.iter().find(|compiled| &compiled.name == rule)
                    })
                    .collect(),
                books,
            };
            ids.insert(
                name.clone(),
                Id::of("axiom.v2.definition", &Definition::Package(definition)),
            );
        }
        ids
    }

    pub fn schema(&self, name: &str) -> Option<&Schema> {
        self.schemas.get(name)
    }
    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }
    pub fn books(&self) -> &BTreeMap<String, Vec<String>> {
        &self.books
    }
    pub fn schemas(&self) -> &BTreeMap<String, Schema> {
        &self.schemas
    }
    pub fn patterns(&self) -> impl Iterator<Item = &str> {
        self.patterns.keys().map(String::as_str)
    }
    pub fn pattern_location(&self, kind: &str) -> Option<(&str, usize, Span)> {
        let pattern = self.patterns.get(kind)?;
        let package = self.schema_packages.get(kind)?;
        Some((
            package,
            self.package_indices.get(package)?.to_owned(),
            pattern.span,
        ))
    }
    pub fn pattern_field_location(&self, kind: &str, field: &str) -> Option<(&str, usize, Span)> {
        let (package, source_index, pattern_span) = self.pattern_location(kind)?;
        let span = self
            .patterns
            .get(kind)?
            .field_spans
            .get(field)
            .copied()
            .unwrap_or(pattern_span);
        Some((package, source_index, span))
    }
    pub fn inference_inputs(&self, kind: &str, field: &str) -> Vec<&str> {
        let Some(pattern) = self.patterns.get(kind) else {
            return Vec::new();
        };
        let Some(target) = pattern.fields.get(field) else {
            return Vec::new();
        };
        let target_captures = pattern_capture_names(target);
        if target_captures.is_empty() {
            return Vec::new();
        }
        pattern
            .fields
            .iter()
            .filter(|(name, term)| {
                name.as_str() != field && !target_captures.is_disjoint(&pattern_capture_names(term))
            })
            .map(|(name, _)| name.as_str())
            .collect()
    }
    pub fn rule_location(&self, name: &str) -> Option<(&str, &Span)> {
        Some((
            self.rule_packages.get(name)?.as_str(),
            self.rule_spans.get(name)?,
        ))
    }
    pub fn rule_source_index(&self, name: &str) -> Option<usize> {
        self.package_indices
            .get(self.rule_packages.get(name)?)
            .copied()
    }
    pub fn rule_step_spans(&self, name: &str) -> Option<&[Span]> {
        self.rule_step_spans.get(name).map(Vec::as_slice)
    }
    pub fn packages(&self) -> &BTreeMap<String, String> {
        &self.packages
    }

    pub fn elaborate(&self, source: &str) -> Result<TypedDocument, Vec<Diagnostic>> {
        let document = syntax::parse(source).map_err(|errors| source_diagnostics(errors, 0))?;
        let mut errors = Vec::new();
        let mut ledger = None::<String>;
        let mut imports = BTreeSet::new();
        let mut rows = Vec::new();
        let mut locations = BTreeMap::new();
        let mut decisions = Vec::new();
        let mut decision_lines = BTreeMap::new();
        let mut seen_ids = BTreeSet::new();
        let mut inferred = BTreeMap::new();
        let mut field_locations = BTreeMap::new();
        let mut entry_count = 0usize;
        let mut retained_value_budget = 16 * 1024 * 1024;

        for block in &document.blocks {
            match block.head.as_str() {
                "ledger" => {
                    if ledger.is_some() {
                        errors.push(diag(block.span.line, "duplicate ledger declaration"));
                    } else if !valid_identifier(block.args.trim()) || !block.fields.is_empty() {
                        errors.push(diag(block.span.line, "invalid ledger declaration"));
                    } else {
                        ledger = Some(block.args.trim().to_owned());
                    }
                }
                "use" => {
                    let name = block.args.trim();
                    if !self.packages.contains_key(name) {
                        errors.push(diag(
                            block.span.line,
                            format!("package `{name}` is not in the pinned model"),
                        ));
                    } else if !block.fields.is_empty() {
                        errors.push(diag(block.span.line, "package import takes no fields"));
                    } else if !imports.insert(name.to_owned()) {
                        errors.push(diag(
                            block.span.line,
                            format!("duplicate package import `{name}`"),
                        ));
                    }
                }
                "decide" => {
                    let decision_id = block.args.trim();
                    if !valid_identifier(decision_id) || block.fields.len() != 2 {
                        errors.push(diag(
                            block.span.line,
                            "decision needs an id, target, and value",
                        ));
                        continue;
                    }
                    let mut target = None;
                    let mut value = None;
                    let mut field_names = BTreeSet::new();
                    for field in &block.fields {
                        if !field_names.insert(field.name.as_str()) {
                            errors.push(diag(
                                field.span.line,
                                format!("duplicate decision field `{}`", field.name),
                            ));
                            continue;
                        }
                        match field.name.as_str() {
                            "target" => target = Some(field.value.trim().to_owned()),
                            "value" => match Value::parse(field.value.trim()) {
                                Ok(parsed) => value = Some(parsed),
                                Err(error) => errors.push(diag(field.span.line, error)),
                            },
                            _ => errors.push(diag(
                                field.span.line,
                                format!("unknown decision field `{}`", field.name),
                            )),
                        }
                    }
                    if let (Some(target), Some(value)) = (target, value) {
                        decisions.push(Decision {
                            id: decision_id.to_owned(),
                            target,
                            value,
                        });
                        decision_lines.insert(decision_id.to_owned(), block.span.line);
                    }
                }
                _head => {
                    let (pattern_name, id, header_date) = match parse_source_header(block) {
                        Ok(header) => header,
                        Err(error) => {
                            errors.push(diag(block.span.line, error));
                            continue;
                        }
                    };
                    if !self.patterns.contains_key(pattern_name) {
                        if self
                            .schemas
                            .get(pattern_name)
                            .is_some_and(|schema| !schema.authored)
                        {
                            errors.push(diag(
                                block.span.line,
                                format!("derived relation `{pattern_name}` cannot be authored"),
                            ));
                        } else {
                            errors.push(diag(
                                block.span.line,
                                format!("unknown ledger entry pattern `{pattern_name}`"),
                            ));
                        }
                        continue;
                    }
                    if header_date.is_some()
                        && self
                            .patterns
                            .get(pattern_name)
                            .is_some_and(|pattern| pattern.date_field.is_none())
                    {
                        errors.push(diag(
                            block.span.line,
                            format!("entry pattern `{pattern_name}` has no dated header field"),
                        ));
                        continue;
                    }
                    entry_count += 1;
                    let occurrence =
                        id.map_or_else(|| format!("entry/{entry_count}"), str::to_owned);
                    self.elaborate_pattern(
                        block,
                        &occurrence,
                        pattern_name,
                        header_date,
                        &imports,
                        &mut rows,
                        &mut locations,
                        &mut inferred,
                        &mut field_locations,
                        &mut seen_ids,
                        &mut retained_value_budget,
                        &mut errors,
                    );
                }
            }
        }
        if ledger.is_none() {
            errors.push(diag(1, "ledger declaration is required"));
        }
        validate_source_references(
            &rows,
            &self.schemas,
            &self.patterns,
            &locations,
            &mut errors,
        );

        if !errors.is_empty() {
            tag_diagnostics(&mut errors, 0, 0);
            return Err(errors);
        }
        let by_id: BTreeMap<String, (String, BTreeMap<String, Value>)> = rows
            .iter()
            .map(|row| (row.id.clone(), (row.schema.clone(), row.fields.clone())))
            .collect();
        let mut decision_targets = BTreeSet::new();
        let mut decision_ids = BTreeSet::new();
        for decision in &decisions {
            let line = decision_lines.get(&decision.id).copied().unwrap_or(1);
            if !decision_ids.insert(decision.id.clone()) {
                errors.push(diag(
                    line,
                    format!("duplicate decision id `{}`", decision.id),
                ));
                continue;
            }
            if !decision_targets.insert(decision.target.clone()) {
                errors.push(diag(
                    line,
                    format!("duplicate decision target `{}`", decision.target),
                ));
                continue;
            }
            let Some((row_id, field_name)) = decision.target.rsplit_once('.') else {
                errors.push(diag(
                    line,
                    format!(
                        "decision target `{}` must be occurrence.field",
                        decision.target
                    ),
                ));
                continue;
            };
            let Some((row_schema, original_fields)) = by_id.get(row_id) else {
                errors.push(diag(
                    line,
                    format!("decision targets unknown occurrence `{row_id}`"),
                ));
                continue;
            };
            let Some(schema) = self.schemas.get(row_schema) else {
                continue;
            };
            let Some(field_def) = schema.fields.get(field_name) else {
                errors.push(diag(
                    line,
                    format!("decision targets unknown field `{}`", decision.target),
                ));
                continue;
            };
            let Some(current) = original_fields.get(field_name) else {
                continue;
            };
            if !matches!(current, Value::Hole(_)) {
                errors.push(diag(
                    line,
                    format!("decision target `{}` is not a hole", decision.target),
                ));
                continue;
            }
            if let Err(error) = field_def.validate(&decision.value) {
                errors.push(diag(
                    line,
                    format!("decision value for `{}`: {error}", decision.target),
                ));
                continue;
            }
            let refs_before = errors.len();
            validate_value_refs(
                field_def,
                &decision.value,
                &by_id,
                line,
                &format!("decision `{}`", decision.id),
                &mut errors,
            );
            if errors.len() == refs_before
                && let Some(row_mut) = rows.iter_mut().find(|row| row.id == row_id)
            {
                row_mut
                    .fields
                    .insert(field_name.to_owned(), decision.value.clone());
            }
        }
        if !errors.is_empty() {
            tag_diagnostics(&mut errors, 0, 0);
            return Err(errors);
        }
        rows.sort_by(|a, b| a.id.cmp(&b.id));
        decisions.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(TypedDocument {
            name: ledger.unwrap(),
            rows,
            decisions,
            locations,
            field_locations,
            inferred,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn elaborate_pattern(
        &self,
        block: &syntax::Block,
        id: &str,
        pattern_name: &str,
        header_date: Option<crate::Date>,
        imports: &BTreeSet<String>,
        rows: &mut Vec<Row>,
        locations: &mut BTreeMap<String, Span>,
        inferred: &mut BTreeMap<String, Value>,
        field_locations: &mut BTreeMap<String, Span>,
        seen_ids: &mut BTreeSet<String>,
        retained_value_budget: &mut usize,
        errors: &mut Vec<Diagnostic>,
    ) {
        if !valid_identifier(id) {
            errors.push(diag(block.span.line, "occurrence id is invalid"));
            return;
        }
        let Some(schema) = self.schemas.get(pattern_name) else {
            errors.push(diag(
                block.span.line,
                format!("unknown pattern `{pattern_name}`"),
            ));
            return;
        };
        if let Some(package) = self.schema_packages.get(pattern_name)
            && !imports.contains(package)
        {
            errors.push(diag(
                block.span.line,
                format!("pattern `{pattern_name}` requires `use {package}`"),
            ));
            return;
        }
        if !seen_ids.insert(id.to_owned()) {
            errors.push(diag(
                block.span.line,
                format!("duplicate occurrence `{id}`"),
            ));
            return;
        }
        let pattern = &self.patterns[pattern_name];
        let mut values = BTreeMap::new();
        let mut explicit = BTreeSet::new();
        let header_date_supplied = header_date.is_some();
        for field in &block.fields {
            if !schema.fields.contains_key(&field.name) {
                errors.push(diag(
                    field.span.line,
                    format!("unknown field `{}` on `{pattern_name}`", field.name),
                ));
                continue;
            }
            if !explicit.insert(field.name.clone()) {
                errors.push(diag(
                    field.span.line,
                    format!("duplicate field `{}`", field.name),
                ));
                continue;
            }
            match Value::parse(field.value.trim()) {
                Ok(value) => {
                    values.insert(field.name.clone(), value);
                    field_locations.insert(format!("{id}.{}", field.name), field.span);
                }
                Err(error) => errors.push(diag(field.span.line, error)),
            }
        }
        if let (Some(date_field), Some(date)) = (&pattern.date_field, header_date) {
            let value = Value::Date(date);
            if let Some(explicit_value) = values.get(date_field) {
                if explicit_value != &value {
                    errors.push(diag(
                        block.span.line,
                        format!("date header conflicts with `{date_field}` field"),
                    ));
                }
            } else {
                values.insert(date_field.clone(), value);
                explicit.insert(date_field.clone());
                if header_date_supplied {
                    field_locations.insert(format!("{id}.{date_field}"), block.span);
                }
            }
        }
        let mut captures = BTreeMap::new();
        for name in &explicit {
            let (Some(term), Some(value)) = (pattern.fields.get(name), values.get(name)) else {
                continue;
            };
            if let Err(error) = match_pattern_value(term, value, &mut captures) {
                errors.push(diag(block.span.line, format!("field `{name}`: {error}")));
            }
        }
        let mut synthesized = BTreeSet::new();
        for (name, term) in &pattern.fields {
            if let Some(value) = values.get(name).cloned() {
                match complete_pattern_value(term, value, &captures) {
                    Ok((completed, completed_unit)) => {
                        if completed_unit {
                            synthesized.insert(name.clone());
                        }
                        values.insert(name.clone(), completed);
                    }
                    Err(error) => {
                        errors.push(diag(block.span.line, format!("field `{name}`: {error}")))
                    }
                }
            } else {
                match instantiate_pattern_value(term, &captures) {
                    Ok(value) => {
                        values.insert(name.clone(), value);
                        synthesized.insert(name.clone());
                    }
                    Err(error) => {
                        errors.push(diag(block.span.line, format!("field `{name}`: {error}")))
                    }
                }
            }
        }
        for (name, value) in &values {
            let Some(def) = schema.fields.get(name) else {
                continue;
            };
            if let Err(error) = def.validate(value) {
                errors.push(diag(block.span.line, format!("field `{name}`: {error}")));
            }
        }
        for value in values.values() {
            if !value.charge_size(retained_value_budget) {
                errors.push(diag(
                    block.span.line,
                    "elaborated ledger exceeds the 16 MiB retained-value budget",
                ));
                return;
            }
        }
        for name in synthesized {
            if let Some(value) = values.get(&name) {
                if !value.charge_size(retained_value_budget) {
                    errors.push(diag(
                        block.span.line,
                        "elaborated ledger exceeds the 16 MiB retained-value budget",
                    ));
                    return;
                }
                inferred.insert(format!("{id}.{name}"), value.clone());
            }
        }
        rows.push(Row {
            id: id.to_owned(),
            schema: pattern_name.to_owned(),
            fields: values,
        });
        locations.insert(id.to_owned(), block.span);
    }
}

fn compile_pattern(block: &syntax::Block) -> Result<(Pattern, Schema), Vec<Diagnostic>> {
    let mut errors = Vec::new();
    let (name, capture, date_field) = if let Some(date_capture) = block.head.strip_prefix('?') {
        let mut parts = block.args.split_whitespace();
        match (parts.next(), parts.next(), parts.next()) {
            (Some(name), Some(capture), None) if valid_binding_name(date_capture) => {
                (name, capture, Some(date_capture.to_owned()))
            }
            _ => {
                return Err(vec![diag(
                    block.span.line,
                    "dated pattern header must be `?field KIND ?occurrence`",
                )]);
            }
        }
    } else {
        (block.head.as_str(), block.args.trim(), None)
    };
    let Some(capture) = parse_capture(capture) else {
        return Err(vec![diag(
            block.span.line,
            "pattern header must end with one named occurrence hole such as `?buy`",
        )]);
    };
    if !valid_qualified_name(name) {
        return Err(vec![diag(
            block.span.line,
            "pattern needs a valid entry kind",
        )]);
    }
    if matches!(
        name,
        "ledger" | "use" | "decide" | "entry" | "package" | "rule" | "book" | "form" | "fact"
    ) {
        return Err(vec![diag(
            block.span.line,
            format!("`{name}` is reserved for ledger structure"),
        )]);
    }
    let mut fields = BTreeMap::new();
    let mut terms = BTreeMap::new();
    let mut field_spans = BTreeMap::new();
    for field in &block.fields {
        if field.name == "id" || !valid_binding_name(&field.name) {
            errors.push(diag(
                field.span.line,
                format!("invalid pattern field `{}`", field.name),
            ));
            continue;
        }
        match parse_pattern_value(&field.value) {
            Ok(term) => {
                let ty = pattern_type(&term);
                if terms.insert(field.name.clone(), term).is_some() {
                    errors.push(diag(
                        field.span.line,
                        format!("duplicate pattern field `{}`", field.name),
                    ));
                } else {
                    fields.insert(field.name.clone(), ty);
                    field_spans.insert(field.name.clone(), field.span);
                }
            }
            Err(error) => errors.push(diag(field.span.line, error)),
        }
    }
    if let Some(date_field) = &date_field {
        let date_term = PatternValue::Capture(date_field.clone());
        if let Some(existing) = terms.get(date_field) {
            if pattern_type(existing) != Type::Date && pattern_type(existing) != Type::Any {
                errors.push(diag(
                    block.span.line,
                    "dated pattern `date` field must be a date",
                ));
            }
        } else {
            terms.insert(date_field.clone(), date_term);
            fields.insert(date_field.clone(), Type::Date);
        }
        if let Some(definition) = fields.get_mut(date_field) {
            *definition = Type::Date;
        }
        field_spans.entry(date_field.clone()).or_insert(block.span);
    }
    if fields.is_empty() {
        errors.push(diag(
            block.span.line,
            "entry pattern needs at least one field",
        ));
    }
    if errors.is_empty() {
        let pattern = Pattern {
            name: name.to_owned(),
            capture,
            date_field,
            fields: terms,
            span: block.span,
            field_spans,
        };
        let schema = Schema {
            name: name.to_owned(),
            fields,
            authored: true,
        };
        Ok((pattern, schema))
    } else {
        Err(errors)
    }
}

fn parse_source_header(
    block: &syntax::Block,
) -> Result<(&str, Option<&str>, Option<crate::Date>), String> {
    if let Ok(Value::Date(date)) = Value::parse(&block.head) {
        let mut parts = block.args.split_whitespace();
        let Some(pattern) = parts.next() else {
            return Err("date-prefixed entry needs a pattern name".into());
        };
        let id = parts.next();
        if parts.next().is_some()
            || !valid_qualified_name(pattern)
            || id.is_some_and(|id| !valid_identifier(id))
        {
            return Err("expected `DATE KIND [OCCURRENCE]`".into());
        }
        Ok((pattern, id, Some(date)))
    } else {
        let mut parts = block.args.split_whitespace();
        let id = parts.next();
        if parts.next().is_some() || id.is_some_and(|id| !valid_identifier(id)) {
            return Err("expected `KIND [OCCURRENCE]`".into());
        }
        Ok((&block.head, id, None))
    }
}

fn all_refs(value: &Value) -> Vec<String> {
    match value {
        Value::Ref(id) => vec![id.clone()],
        Value::List(values) => values.iter().flat_map(all_refs).collect(),
        Value::Record(fields) => fields.values().flat_map(all_refs).collect(),
        _ => Vec::new(),
    }
}

fn validate_source_references(
    rows: &[Row],
    schemas: &BTreeMap<String, Schema>,
    patterns: &BTreeMap<String, Pattern>,
    locations: &BTreeMap<String, Span>,
    errors: &mut Vec<Diagnostic>,
) {
    let by_id: BTreeMap<_, _> = rows
        .iter()
        .map(|row| (row.id.clone(), (row.schema.clone(), row.fields.clone())))
        .collect();
    for row in rows {
        let Some(schema) = schemas.get(&row.schema) else {
            continue;
        };
        let Some(pattern) = patterns.get(&row.schema) else {
            continue;
        };
        let line = locations.get(&row.id).map_or(1, |span| span.line);
        for (field, value) in &row.fields {
            if let Some(definition) = schema.fields.get(field) {
                validate_value_refs(
                    definition,
                    value,
                    &by_id,
                    line,
                    &format!("`{}.{field}`", row.id),
                    errors,
                );
            }
            if let Some(term) = pattern.fields.get(field) {
                validate_pattern_reference_constraints(
                    term,
                    value,
                    &by_id,
                    line,
                    &format!("`{}.{field}`", row.id),
                    errors,
                );
            }
        }
    }
}

fn validate_pattern_reference_constraints(
    pattern: &PatternValue,
    value: &Value,
    rows: &BTreeMap<String, (String, BTreeMap<String, Value>)>,
    line: usize,
    context: &str,
    errors: &mut Vec<Diagnostic>,
) {
    match (pattern, value) {
        (PatternValue::Reference(expected), Value::Ref(target)) => match rows.get(target) {
            Some((actual, _)) if actual == expected => {}
            Some((actual, _)) => errors.push(diag(
                line,
                format!(
                    "reference `@{target}` in {context} has pattern `{actual}`, expected `{expected}`"
                ),
            )),
            None => errors.push(diag(
                line,
                format!("reference `@{target}` in {context} does not resolve"),
            )),
        },
        (PatternValue::Quantity(amount, _), value) => {
            validate_pattern_reference_constraints(amount, value, rows, line, context, errors);
        }
        (PatternValue::OpenList(item), Value::List(values)) => {
            for value in values {
                validate_pattern_reference_constraints(item, value, rows, line, context, errors);
            }
        }
        (PatternValue::List(items), Value::List(values)) => {
            for (item, value) in items.iter().zip(values) {
                validate_pattern_reference_constraints(item, value, rows, line, context, errors);
            }
        }
        (PatternValue::Record(items), Value::Record(values)) => {
            for (name, item) in items {
                if let Some(value) = values.get(name) {
                    validate_pattern_reference_constraints(
                        item, value, rows, line, context, errors,
                    );
                }
            }
        }
        _ => {}
    }
}

fn validate_value_refs(
    ty: &Type,
    value: &Value,
    rows: &BTreeMap<String, (String, BTreeMap<String, Value>)>,
    line: usize,
    context: &str,
    errors: &mut Vec<Diagnostic>,
) {
    match (ty, value) {
        (Type::Ref(expected), Value::Ref(target)) => match rows.get(target) {
            Some((actual, _)) if actual == expected => {}
            Some((actual, _)) => errors.push(diag(
                line,
                format!("reference `@{target}` in {context} has pattern `{actual}`, expected `{expected}`"),
            )),
            None => errors.push(diag(line, format!("reference `@{target}` in {context} does not resolve"))),
        },
        (Type::List(item), Value::List(values)) => {
            for value in values {
                validate_value_refs(item, value, rows, line, context, errors);
            }
        }
        (Type::Record(types), Value::Record(values)) => {
            for (name, ty) in types {
                if let Some(value) = values.get(name) {
                    validate_value_refs(ty, value, rows, line, context, errors);
                }
            }
        }
        (Type::Any, value) => {
            for target in all_refs(value) {
                if !rows.contains_key(&target) {
                    errors.push(diag(line, format!("reference `@{target}` in {context} does not resolve")));
                }
            }
        }
        _ => {}
    }
}

fn parse_capture(source: &str) -> Option<String> {
    let name = source.strip_prefix('?')?;
    valid_binding_name(name).then(|| name.to_owned())
}

fn parse_pattern_value(source: &str) -> Result<PatternValue, String> {
    let source = source.trim();
    if let Some((amount, unit)) = split_two_atoms(source)
        && parse_capture(unit).is_some()
        && matches!(
            Value::parse(amount),
            Ok(Value::Number(_)) | Ok(Value::Hole(_))
        )
    {
        let amount = parse_pattern_value(amount)?;
        let unit = parse_capture(unit).expect("checked capture");
        return Ok(PatternValue::Quantity(Box::new(amount), unit));
    }
    if let Some(pattern) = source.strip_prefix("@?") {
        if valid_qualified_name(pattern) {
            return Ok(PatternValue::Reference(pattern.to_owned()));
        }
        return Err("reference pattern needs a pattern name after `@?`".into());
    }
    if source == "?" {
        return Err("a hole must have a name".into());
    }
    if source.starts_with('?') {
        return parse_capture(source)
            .map(PatternValue::Capture)
            .ok_or_else(|| "invalid named hole".into());
    }
    if source.starts_with('[') && source.ends_with(']') {
        let inner = &source[1..source.len() - 1];
        if inner.trim().is_empty() {
            return Ok(PatternValue::List(Vec::new()));
        }
        let items = split_commas(inner)?
            .into_iter()
            .map(parse_pattern_value)
            .collect::<Result<Vec<_>, _>>()?;
        return if items.len() == 1 && matches!(items[0], PatternValue::Capture(_)) {
            Ok(PatternValue::OpenList(Box::new(
                items.into_iter().next().unwrap(),
            )))
        } else {
            Ok(PatternValue::List(items))
        };
    }
    if source.starts_with('{') && source.ends_with('}') {
        let inner = &source[1..source.len() - 1];
        if inner.trim().is_empty() {
            return Ok(PatternValue::Record(BTreeMap::new()));
        }
        let mut fields = BTreeMap::new();
        for field in split_commas(inner)? {
            let Some((name, value)) = field.split_once(':') else {
                return Err("record pattern field needs `name:value`".into());
            };
            let name = name.trim();
            if !valid_binding_name(name) || name == "id" {
                return Err(format!("invalid record pattern field `{name}`"));
            }
            if fields
                .insert(name.to_owned(), parse_pattern_value(value)?)
                .is_some()
            {
                return Err(format!("duplicate record pattern field `{name}`"));
            }
        }
        return Ok(PatternValue::Record(fields));
    }
    let value = Value::parse(source)?;
    Ok(pattern_from_value(value))
}

fn split_two_atoms(source: &str) -> Option<(&str, &str)> {
    let mut parts = source.split_whitespace();
    let first = parts.next()?;
    let second = parts.next()?;
    parts.next().is_none().then_some((first, second))
}

fn pattern_from_value(value: Value) -> PatternValue {
    match value {
        Value::Hole(name) => PatternValue::Capture(name),
        Value::List(items) if items.len() == 1 && matches!(items[0], Value::Hole(_)) => {
            PatternValue::OpenList(Box::new(pattern_from_value(
                items.into_iter().next().unwrap(),
            )))
        }
        Value::List(items) => {
            PatternValue::List(items.into_iter().map(pattern_from_value).collect())
        }
        Value::Record(items) => PatternValue::Record(
            items
                .into_iter()
                .map(|(name, value)| (name, pattern_from_value(value)))
                .collect(),
        ),
        value => PatternValue::Default(value),
    }
}

fn pattern_type(pattern: &PatternValue) -> Type {
    match pattern {
        PatternValue::Capture(_) => Type::Any,
        PatternValue::Quantity(_, _) => Type::Quantity,
        PatternValue::Reference(target) => Type::Ref(target.clone()),
        PatternValue::Default(value) => value_type(value),
        PatternValue::OpenList(item) => Type::List(Box::new(pattern_type(item))),
        PatternValue::List(items) => {
            Type::List(Box::new(common_types(items.iter().map(pattern_type))))
        }
        PatternValue::Record(fields) => Type::Record(
            fields
                .iter()
                .map(|(name, value)| (name.clone(), pattern_type(value)))
                .collect(),
        ),
    }
}

fn pattern_capture_names(pattern: &PatternValue) -> BTreeSet<&str> {
    match pattern {
        PatternValue::Capture(name) => BTreeSet::from([name.as_str()]),
        PatternValue::Quantity(amount, unit) => pattern_capture_names(amount)
            .into_iter()
            .chain(std::iter::once(unit.as_str()))
            .collect(),
        PatternValue::Reference(_) | PatternValue::Default(_) | PatternValue::OpenList(_) => {
            BTreeSet::new()
        }
        PatternValue::List(items) => items.iter().flat_map(pattern_capture_names).collect(),
        PatternValue::Record(fields) => fields.values().flat_map(pattern_capture_names).collect(),
    }
}

fn pattern_reference_targets(pattern: &PatternValue) -> Vec<String> {
    match pattern {
        PatternValue::Reference(target) => vec![target.clone()],
        PatternValue::Quantity(amount, _) | PatternValue::OpenList(amount) => {
            pattern_reference_targets(amount)
        }
        PatternValue::List(items) => items.iter().flat_map(pattern_reference_targets).collect(),
        PatternValue::Record(fields) => fields
            .values()
            .flat_map(pattern_reference_targets)
            .collect(),
        PatternValue::Capture(_) | PatternValue::Default(_) => Vec::new(),
    }
}

fn match_pattern_value(
    pattern: &PatternValue,
    value: &Value,
    captures: &mut BTreeMap<String, Value>,
) -> Result<(), String> {
    match (pattern, value) {
        (PatternValue::Capture(_), Value::Hole(_)) => Ok(()),
        (PatternValue::Capture(name), value) => bind_capture(captures, name, value.clone()),
        (PatternValue::Quantity(amount, unit), Value::Quantity(number, actual_unit)) => {
            match_pattern_value(amount, &Value::Number(number.clone()), captures)?;
            bind_capture(captures, unit, Value::Text(actual_unit.clone()))
        }
        (PatternValue::Quantity(amount, _), Value::Number(number)) => {
            match_pattern_value(amount, &Value::Number(number.clone()), captures)
        }
        (PatternValue::Quantity(_, _), Value::Hole(_)) => Ok(()),
        (PatternValue::Reference(_), Value::Ref(_)) => Ok(()),
        (PatternValue::Default(default), value) => pattern_compatible(&value_type(default), value),
        (PatternValue::OpenList(item), Value::List(values)) => {
            for value in values {
                match_pattern_shape(item, value)?;
            }
            Ok(())
        }
        (PatternValue::List(items), Value::List(values)) if items.len() == values.len() => {
            for (item, value) in items.iter().zip(values) {
                match_pattern_value(item, value, captures)?;
            }
            Ok(())
        }
        (PatternValue::Record(items), Value::Record(values)) if items.keys().eq(values.keys()) => {
            for (name, item) in items {
                match_pattern_value(item, &values[name], captures)?;
            }
            Ok(())
        }
        (_, Value::Hole(_)) => Ok(()),
        _ => Err(format!(
            "expected pattern shape {}, found {value}",
            pattern_type(pattern)
        )),
    }
}

fn match_pattern_shape(pattern: &PatternValue, value: &Value) -> Result<(), String> {
    match (pattern, value) {
        (PatternValue::Capture(_), _) | (_, Value::Hole(_)) => Ok(()),
        (PatternValue::Quantity(_, _), Value::Quantity(_, _) | Value::Number(_)) => Ok(()),
        (PatternValue::Reference(_), Value::Ref(_)) => Ok(()),
        (PatternValue::Default(default), value) => pattern_compatible(&value_type(default), value),
        (PatternValue::OpenList(item), Value::List(values)) => {
            for value in values {
                match_pattern_shape(item, value)?;
            }
            Ok(())
        }
        (PatternValue::List(items), Value::List(values)) if items.len() == values.len() => {
            for (item, value) in items.iter().zip(values) {
                match_pattern_shape(item, value)?;
            }
            Ok(())
        }
        (PatternValue::Record(items), Value::Record(values)) if items.keys().eq(values.keys()) => {
            for (name, item) in items {
                match_pattern_shape(item, &values[name])?;
            }
            Ok(())
        }
        _ => Err(format!(
            "expected pattern shape {}, found {value}",
            pattern_type(pattern)
        )),
    }
}

fn pattern_compatible(expected: &Type, value: &Value) -> Result<(), String> {
    let actual = value_type(value);
    if unify_types(expected, &actual).is_ok() {
        Ok(())
    } else {
        Err(format!("expected {expected}, found {value}"))
    }
}

fn bind_capture(
    captures: &mut BTreeMap<String, Value>,
    name: &str,
    value: Value,
) -> Result<(), String> {
    match captures.get(name) {
        Some(previous) if previous != &value => Err(format!(
            "capture `?{name}` conflicts ({previous} versus {value})"
        )),
        Some(_) => Ok(()),
        None => {
            captures.insert(name.to_owned(), value);
            Ok(())
        }
    }
}

fn complete_pattern_value(
    pattern: &PatternValue,
    value: Value,
    captures: &BTreeMap<String, Value>,
) -> Result<(Value, bool), String> {
    match (pattern, value) {
        (PatternValue::Quantity(_amount, unit), Value::Number(number)) => {
            let unit = captures
                .get(unit)
                .and_then(|value| match value {
                    Value::Text(unit) => Some(unit.clone()),
                    _ => None,
                })
                .ok_or_else(|| format!("unit capture `?{unit}` is not determined by this entry"))?;
            Ok((Value::Quantity(number, unit), true))
        }
        (PatternValue::List(patterns), Value::List(values)) if patterns.len() == values.len() => {
            let mut completed = Vec::with_capacity(values.len());
            let mut inferred = false;
            for (pattern, value) in patterns.iter().zip(values) {
                let (value, changed) = complete_pattern_value(pattern, value, captures)?;
                completed.push(value);
                inferred |= changed;
            }
            Ok((Value::List(completed), inferred))
        }
        (PatternValue::Record(patterns), Value::Record(values)) => {
            let mut completed = BTreeMap::new();
            let mut inferred = false;
            for (name, pattern) in patterns {
                let value = values
                    .get(name)
                    .cloned()
                    .ok_or_else(|| format!("record is missing field `{name}`"))?;
                let (value, changed) = complete_pattern_value(pattern, value, captures)?;
                completed.insert(name.clone(), value);
                inferred |= changed;
            }
            Ok((Value::Record(completed), inferred))
        }
        (_, value) => Ok((value, false)),
    }
}

fn instantiate_pattern_value(
    pattern: &PatternValue,
    captures: &BTreeMap<String, Value>,
) -> Result<Value, String> {
    match pattern {
        PatternValue::Capture(name) => Ok(captures
            .get(name)
            .cloned()
            .unwrap_or_else(|| Value::Hole(name.clone()))),
        PatternValue::Quantity(amount, unit) => {
            let amount = match amount.as_ref() {
                PatternValue::Default(Value::Number(number)) => Some(number.clone()),
                PatternValue::Capture(name) => captures.get(name).and_then(|value| match value {
                    Value::Number(number) => Some(number.clone()),
                    _ => None,
                }),
                _ => None,
            };
            let unit_value = captures.get(unit).and_then(|value| match value {
                Value::Text(unit) => Some(unit.clone()),
                _ => None,
            });
            match (amount, unit_value) {
                (Some(amount), Some(unit)) => Ok(Value::Quantity(amount, unit)),
                _ => Ok(Value::Hole(
                    pattern_hole_name(pattern).unwrap_or(unit).to_owned(),
                )),
            }
        }
        PatternValue::Reference(target) => {
            Ok(Value::Hole(format!("ref_{}", target.replace('.', "_"))))
        }
        PatternValue::Default(value) => Ok(value.clone()),
        PatternValue::OpenList(item) => Ok(Value::Hole(
            pattern_hole_name(item).unwrap_or("items").to_owned(),
        )),
        PatternValue::List(items) => Ok(Value::List(
            items
                .iter()
                .map(|item| instantiate_pattern_value(item, captures))
                .collect::<Result<_, _>>()?,
        )),
        PatternValue::Record(fields) => Ok(Value::Record(
            fields
                .iter()
                .map(|(name, item)| Ok((name.clone(), instantiate_pattern_value(item, captures)?)))
                .collect::<Result<_, String>>()?,
        )),
    }
}

fn pattern_hole_name(pattern: &PatternValue) -> Option<&str> {
    match pattern {
        PatternValue::Capture(name) => Some(name),
        PatternValue::Quantity(amount, unit) => pattern_hole_name(amount).or(Some(unit)),
        PatternValue::Reference(name) => Some(name),
        PatternValue::OpenList(item) => pattern_hole_name(item),
        PatternValue::List(items) => items.iter().find_map(pattern_hole_name),
        PatternValue::Record(fields) => fields.values().find_map(pattern_hole_name),
        PatternValue::Default(_) => None,
    }
}

fn split_commas(source: &str) -> Result<Vec<&str>, String> {
    let mut result = Vec::new();
    let mut start = 0;
    let mut depth = 0i32;
    let mut quote = false;
    let mut escaped = false;
    for (index, ch) in source.char_indices() {
        if quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                quote = false;
            }
            continue;
        }
        match ch {
            '"' => quote = true,
            '<' | '{' | '[' | '(' => depth += 1,
            '>' | '}' | ']' | ')' => depth -= 1,
            ',' if depth == 0 => {
                result.push(source[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
        if depth < 0 {
            return Err("unbalanced type delimiters".into());
        }
    }
    if quote || depth != 0 {
        return Err("unbalanced type delimiters".into());
    }
    if start < source.len() || !source.trim().is_empty() {
        result.push(source[start..].trim());
    }
    if result.iter().any(|item| item.is_empty()) {
        return Err("empty record field".into());
    }
    Ok(result)
}

fn compile_rule(
    name: &str,
    block: &syntax::Block,
    schemas: &BTreeMap<String, Schema>,
    books: &BTreeMap<String, Vec<String>>,
) -> Result<Rule, Vec<Diagnostic>> {
    let mut errors = Vec::new();
    let mut binding = None;
    let mut output = None;
    let mut fields = BTreeMap::new();
    let mut steps = Vec::new();
    let mut book = None;
    let mut emitted = false;
    let mut stage = 0u8;
    for field in &block.fields {
        match field.name.as_str() {
            "for" => {
                if binding.is_some() || stage != 0 {
                    errors.push(diag(
                        field.span.line,
                        "`for` must be the first rule field and appear once",
                    ));
                    continue;
                }
                let mut pieces = field.value.split_whitespace();
                match (pieces.next(), pieces.next(), pieces.next()) {
                    (Some(variable), Some(schema), None) if valid_binding_name(variable) => {
                        binding = Some((variable.to_owned(), schema.to_owned()));
                        stage = 1;
                    }
                    _ => errors.push(diag(field.span.line, "expected `for variable schema`")),
                }
            }
            "let" if stage != 1 => errors.push(diag(
                field.span.line,
                "`let` must appear after `for` and before `emit`",
            )),
            "let" => match split_leading(&field.value) {
                Some((variable, expr)) if valid_binding_name(variable) => match Expr::parse(expr) {
                    Ok(expr) => steps.push(Instruction::Let(variable.to_owned(), expr)),
                    Err(error) => errors.push(diag(field.span.line, error)),
                },
                _ => errors.push(diag(field.span.line, "expected `let variable EXPR`")),
            },
            "choose" if stage != 1 => errors.push(diag(
                field.span.line,
                "`choose` must appear after `for` and before `emit`",
            )),
            "choose" => match split_leading(&field.value) {
                Some((variable, rest)) if valid_binding_name(variable) => match Expr::pair(rest) {
                    Ok((selector, candidates)) => steps.push(Instruction::Choose(
                        variable.to_owned(),
                        selector,
                        candidates,
                    )),
                    Err(error) => errors.push(diag(
                        field.span.line,
                        format!("choose needs selector and list expressions: {error}"),
                    )),
                },
                _ => errors.push(diag(
                    field.span.line,
                    "expected `choose variable SELECTOR LIST_EXPR`",
                )),
            },
            "require" if stage != 1 => errors.push(diag(
                field.span.line,
                "`require` must appear after `for` and before `emit`",
            )),
            "require" => match Expr::parse(&field.value) {
                Ok(expr) => steps.push(Instruction::Require(expr)),
                Err(error) => errors.push(diag(field.span.line, error)),
            },
            "emit" => {
                let schema = field.value.trim();
                if stage != 1 || emitted {
                    errors.push(diag(
                        field.span.line,
                        "`emit` must follow the rule steps and appear once",
                    ));
                } else if !valid_qualified_name(schema) {
                    errors.push(diag(field.span.line, "emit needs one valid relation name"));
                } else {
                    emitted = true;
                    stage = 2;
                    output = Some(schema.to_owned());
                }
            }
            "set" => {
                if stage != 2 {
                    errors.push(diag(
                        field.span.line,
                        "`set` must follow `emit` and precede `book`",
                    ));
                    continue;
                }
                match split_leading(&field.value) {
                    Some(("id", _)) => errors.push(diag(
                        field.span.line,
                        "`id` is generated for every derived occurrence and cannot be set",
                    )),
                    Some((field_name, expr)) => match Expr::parse(expr) {
                        Ok(expr) => {
                            if fields.insert(field_name.to_owned(), expr).is_some() {
                                errors.push(diag(
                                    field.span.line,
                                    format!("duplicate output field `{field_name}`"),
                                ));
                            }
                        }
                        Err(error) => errors.push(diag(field.span.line, error)),
                    },
                    None => errors.push(diag(field.span.line, "expected `set field EXPR`")),
                }
            }
            "book" => {
                if book.is_some() {
                    errors.push(diag(field.span.line, "rule may name only one book"));
                } else if stage != 2 {
                    errors.push(diag(
                        field.span.line,
                        "`book` must follow all output fields",
                    ));
                } else if !books.contains_key(field.value.trim()) {
                    errors.push(diag(
                        field.span.line,
                        format!("unknown book `{}`", field.value.trim()),
                    ));
                } else {
                    book = Some(field.value.trim().to_owned());
                    stage = 3;
                }
            }
            other => errors.push(diag(
                field.span.line,
                format!("unknown rule field `{other}`"),
            )),
        }
    }
    let Some((binding_name, source_schema)) = binding else {
        errors.push(diag(
            block.span.line,
            "rule requires exactly one `for variable schema`",
        ));
        return Err(errors);
    };
    let Some(output_name) = output else {
        errors.push(diag(
            block.span.line,
            "rule requires exactly one `emit schema`",
        ));
        return Err(errors);
    };
    if !schemas.contains_key(&source_schema) {
        errors.push(diag(
            block.span.line,
            format!("rule source names unknown relation `{source_schema}`"),
        ));
    }
    if errors.is_empty() {
        Ok(Rule {
            name: name.to_owned(),
            source: source_schema,
            binding: binding_name,
            steps,
            output: output_name,
            fields,
            book,
        })
    } else {
        Err(errors)
    }
}

fn collect_rows(expr: &Expr, output: &mut BTreeSet<String>) {
    if let Expr::Call(op, args) = expr {
        if op == "rows" {
            if let Some(Expr::Name(name)) = args.first() {
                output.insert(name.clone());
            }
            if let Some(Expr::Literal(Value::Text(name))) = args.first() {
                output.insert(name.clone());
            }
        }
        for arg in args {
            collect_rows(arg, output);
        }
    }
}

fn rule_order(
    rules: &BTreeMap<String, Rule>,
    by_output: &BTreeMap<String, Vec<String>>,
    schemas: &BTreeMap<String, Schema>,
    raw_blocks: &BTreeMap<String, syntax::Block>,
    rule_packages: &BTreeMap<String, String>,
    package_indices: &BTreeMap<String, usize>,
    errors: &mut Vec<Diagnostic>,
) -> Vec<String> {
    let mut deps = BTreeMap::<String, BTreeSet<String>>::new();
    for (name, rule) in rules {
        let mut relations = BTreeSet::from([rule.source.clone()]);
        for step in &rule.steps {
            match step {
                Instruction::Let(_, expr) | Instruction::Require(expr) => {
                    collect_rows(expr, &mut relations)
                }
                Instruction::Choose(_, left, right) => {
                    collect_rows(left, &mut relations);
                    collect_rows(right, &mut relations);
                }
            }
        }
        for expr in rule.fields.values() {
            collect_rows(expr, &mut relations);
        }
        let mut prerequisites = BTreeSet::new();
        for relation in relations {
            if !schemas.contains_key(&relation) {
                let line = raw_blocks.get(name).map_or(1, |block| block.span.line);
                errors.push(
                    diag(
                        line,
                        format!("rule `{name}` reads unknown schema `{relation}`"),
                    )
                    .in_source(package_indices[&rule_packages[name]]),
                );
            } else if let Some(producers) = by_output.get(&relation) {
                for prerequisite in producers {
                    if prerequisite == name {
                        let line = raw_blocks.get(name).map_or(1, |block| block.span.line);
                        errors.push(
                            diag(
                                line,
                                format!("rule `{name}` reads its own output `{relation}`"),
                            )
                            .in_source(package_indices[&rule_packages[name]]),
                        );
                        continue;
                    }
                    if let Some(upstream) = rules.get(prerequisite)
                        && upstream.book.is_some()
                        && upstream.book != rule.book
                    {
                        let line = raw_blocks.get(name).map_or(1, |block| block.span.line);
                        errors.push(
                            diag(
                                line,
                                format!("rule `{name}` cannot read `{relation}` from another book"),
                            )
                            .in_source(package_indices[&rule_packages[name]]),
                        );
                    }
                    prerequisites.insert(prerequisite.clone());
                }
            }
        }
        deps.insert(name.clone(), prerequisites);
    }
    let mut result = Vec::new();
    let mut remaining = deps;
    while !remaining.is_empty() {
        let ready = remaining
            .iter()
            .find(|(_, needed)| needed.is_empty())
            .map(|(name, _)| name.clone());
        let Some(name) = ready else {
            let cycle = remaining.keys().cloned().collect::<Vec<_>>();
            errors.push(
                diag(
                    1,
                    format!("recursive rule dependencies: {}", cycle.join(" -> ")),
                )
                .in_source(package_indices[&rule_packages[&cycle[0]]]),
            );
            break;
        };
        remaining.remove(&name);
        for needed in remaining.values_mut() {
            needed.remove(&name);
        }
        result.push(name);
    }
    result
}

fn dependency_cycle(dependencies: &BTreeMap<String, BTreeSet<String>>) -> Option<Vec<String>> {
    fn visit(
        name: &str,
        deps: &BTreeMap<String, BTreeSet<String>>,
        active: &mut Vec<String>,
        done: &mut BTreeSet<String>,
    ) -> Option<Vec<String>> {
        if let Some(index) = active.iter().position(|current| current == name) {
            let mut cycle = active[index..].to_vec();
            cycle.push(name.to_owned());
            return Some(cycle);
        }
        if done.contains(name) {
            return None;
        }
        active.push(name.to_owned());
        for next in deps.get(name).into_iter().flatten() {
            if let Some(cycle) = visit(next, deps, active, done) {
                return Some(cycle);
            }
        }
        active.pop();
        done.insert(name.to_owned());
        None
    }
    let mut done = BTreeSet::new();
    for name in dependencies.keys() {
        if let Some(cycle) = visit(name, dependencies, &mut Vec::new(), &mut done) {
            return Some(cycle);
        }
    }
    None
}

fn package_can_see(
    package: &str,
    dependency: &str,
    deps: &BTreeMap<String, BTreeSet<String>>,
) -> bool {
    if package == dependency {
        return true;
    }
    let mut stack: Vec<_> = deps.get(package).into_iter().flatten().cloned().collect();
    let mut seen = BTreeSet::new();
    while let Some(current) = stack.pop() {
        if current == dependency {
            return true;
        }
        if seen.insert(current.clone()) {
            stack.extend(deps.get(&current).into_iter().flatten().cloned());
        }
    }
    false
}

fn split_leading(source: &str) -> Option<(&str, &str)> {
    let source = source.trim_start();
    let end = source.find(char::is_whitespace)?;
    let first = &source[..end];
    let rest = source[end..].trim_start();
    if first.is_empty() || rest.is_empty() {
        None
    } else {
        Some((first, rest))
    }
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '/' | '.'))
        && value
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn valid_qualified_name(value: &str) -> bool {
    !value.is_empty() && value.split('.').all(valid_identifier)
}

fn diag(line: usize, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new(line, message)
}

fn source_diagnostics(diagnostics: Vec<Diagnostic>, source_index: usize) -> Vec<Diagnostic> {
    diagnostics
        .into_iter()
        .map(|mut diagnostic| {
            diagnostic.source_index = Some(source_index);
            diagnostic
        })
        .collect()
}

fn tag_diagnostics(diagnostics: &mut [Diagnostic], start: usize, source_index: usize) {
    for diagnostic in diagnostics.iter_mut().skip(start) {
        diagnostic.source_index.get_or_insert(source_index);
    }
}

#[cfg(test)]
mod local_tests {
    use super::*;

    #[test]
    fn expression_parser_handles_prefix_calls_and_quotes() {
        assert_eq!(
            Expr::parse("(add s.amount 1)").unwrap(),
            Expr::Call(
                "add".into(),
                vec![
                    Expr::Name("s.amount".into()),
                    Expr::Literal(Value::Number("1".parse().unwrap()))
                ]
            )
        );
        assert!(Expr::parse("(add 1").is_err());
        assert!(Expr::parse("s.amount extra").is_err());
    }
}
