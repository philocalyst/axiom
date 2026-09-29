//! A finite expression algebra. Iteration can only range over existing values.

use crate::model::Expr;
use crate::{Number, Value};
use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Fault {
    Missing(String),
    Conflict(String),
    Incomplete(String),
}

pub(crate) struct Budget {
    remaining: usize,
}

impl Budget {
    pub(crate) fn new(limit: usize) -> Self {
        Self { remaining: limit }
    }

    pub(crate) fn tick(&mut self) -> Result<()> {
        self.remaining = self
            .remaining
            .checked_sub(1)
            .ok_or_else(|| Fault::Incomplete("expression work limit reached".into()))?;
        Ok(())
    }
}

type Env = BTreeMap<String, Value>;
type Relations = BTreeMap<String, Vec<Value>>;
type Result<T> = std::result::Result<T, Fault>;

#[derive(Clone, Copy)]
enum Execution {
    Borrowed,
    #[cfg(test)]
    Owning,
}

impl Execution {
    fn borrows(self) -> bool {
        matches!(self, Self::Borrowed)
    }
}

/// An iteration binding shadows its parent without copying any existing
/// binding or the bound record. Results still become ordinary owned Values.
enum Scope<'a> {
    Root(&'a Env),
    Binding {
        parent: &'a Scope<'a>,
        name: &'a str,
        value: &'a Value,
    },
}

impl Scope<'_> {
    fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Self::Root(env) => env.get(key),
            Self::Binding { name, value, .. } if *name == key => Some(value),
            Self::Binding { parent, .. } => parent.get(key),
        }
    }

    #[cfg(test)]
    fn owned(&self) -> Env {
        match self {
            Self::Root(env) => (*env).clone(),
            Self::Binding {
                parent,
                name,
                value,
            } => {
                let mut env = parent.owned();
                env.insert((*name).to_owned(), (*value).clone());
                env
            }
        }
    }
}

/// Filtered relations keep just references; mapping owns only its projected
/// values. Stages remain eager so faults, short circuits and work charges keep
/// their original ordering. No general lazy-query engine is needed.
enum Sequence<'a> {
    Slice(&'a [Value]),
    Selected(Vec<&'a Value>),
    Owned(Vec<Value>),
}

impl Sequence<'_> {
    fn len(&self) -> usize {
        match self {
            Self::Slice(v) => v.len(),
            Self::Selected(v) => v.len(),
            Self::Owned(v) => v.len(),
        }
    }

    fn borrowed(&self) -> bool {
        !matches!(self, Self::Owned(_))
    }

    fn into_values(self) -> Vec<Value> {
        match self {
            Self::Owned(values) => values,
            values => values.into_iter().map(Cow::into_owned).collect(),
        }
    }
}

enum Items<'a> {
    Slice(std::slice::Iter<'a, Value>),
    Selected(std::vec::IntoIter<&'a Value>),
    Owned(std::vec::IntoIter<Value>),
}

impl<'a> Iterator for Items<'a> {
    type Item = Cow<'a, Value>;
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Slice(v) => v.next().map(Cow::Borrowed),
            Self::Selected(v) => v.next().map(Cow::Borrowed),
            Self::Owned(v) => v.next().map(Cow::Owned),
        }
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        match self {
            Self::Slice(v) => v.size_hint(),
            Self::Selected(v) => v.size_hint(),
            Self::Owned(v) => v.size_hint(),
        }
    }
}

impl DoubleEndedIterator for Items<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        match self {
            Self::Slice(v) => v.next_back().map(Cow::Borrowed),
            Self::Selected(v) => v.next_back().map(Cow::Borrowed),
            Self::Owned(v) => v.next_back().map(Cow::Owned),
        }
    }
}

impl<'a> IntoIterator for Sequence<'a> {
    type Item = Cow<'a, Value>;
    type IntoIter = Items<'a>;
    fn into_iter(self) -> Self::IntoIter {
        match self {
            Self::Slice(v) => Items::Slice(v.iter()),
            Self::Selected(v) => Items::Selected(v.into_iter()),
            Self::Owned(v) => Items::Owned(v.into_iter()),
        }
    }
}

fn conflict(message: impl Into<String>) -> Fault {
    Fault::Conflict(message.into())
}

/// A diagnostic literal, bounded by Unicode scalar count (including the final
/// ellipsis), 512 characters and eight container levels. Text is escaped as it
/// is visited; neither strings nor complete containers are rendered first.
pub(crate) fn preview(value: &Value, max_chars: usize) -> String {
    use std::fmt::{self, Write};

    struct Preview {
        text: String,
        remaining: usize,
        truncated: bool,
    }

    impl Write for Preview {
        fn write_str(&mut self, text: &str) -> fmt::Result {
            for ch in text.chars() {
                if self.remaining == 0 {
                    self.truncated = true;
                    return Err(fmt::Error);
                }
                self.text.push(ch);
                self.remaining -= 1;
            }
            Ok(())
        }
    }

    fn quoted(output: &mut Preview, text: &str) -> fmt::Result {
        output.write_char('"')?;
        for ch in text.chars() {
            match ch {
                '"' => output.write_str("\\\"")?,
                '\\' => output.write_str("\\\\")?,
                '\n' => output.write_str("\\n")?,
                '\r' => output.write_str("\\r")?,
                '\t' => output.write_str("\\t")?,
                '\u{08}' => output.write_str("\\b")?,
                '\u{0c}' => output.write_str("\\f")?,
                ch if ch.is_control() || matches!(ch, '\u{2028}' | '\u{2029}') => {
                    write!(output, "\\u{:04x}", ch as u32)?;
                }
                ch => output.write_char(ch)?,
            }
        }
        output.write_char('"')
    }

    fn render(output: &mut Preview, value: &Value, depth: usize) -> fmt::Result {
        // Stop before invoking scalar formatting when the writer is already
        // full. Number's own formatter remains protected by kernel digit limits.
        if output.remaining == 0 {
            output.truncated = true;
            return Err(fmt::Error);
        }
        match value {
            Value::Number(number) => write!(output, "{number}"),
            Value::Quantity(number, unit) => write!(output, "{number} {unit}"),
            Value::Date(date) => write!(output, "{date}"),
            Value::Text(text) => quoted(output, text),
            Value::Bool(value) => output.write_str(if *value { "true" } else { "false" }),
            Value::Ref(reference) => write!(output, "@{reference}"),
            Value::Hole(name) => write!(output, "?{name}"),
            Value::List(values) => {
                if depth == 8 && !values.is_empty() {
                    return output.write_str("[…]");
                }
                output.write_char('[')?;
                for (index, value) in values.iter().enumerate() {
                    if index != 0 {
                        output.write_str(", ")?;
                    }
                    render(output, value, depth + 1)?;
                }
                output.write_char(']')
            }
            Value::Record(fields) => {
                if depth == 8 && !fields.is_empty() {
                    return output.write_str("{…}");
                }
                output.write_char('{')?;
                for (index, (name, value)) in fields.iter().enumerate() {
                    if index != 0 {
                        output.write_str(", ")?;
                    }
                    quoted(output, name)?;
                    output.write_str(": ")?;
                    render(output, value, depth + 1)?;
                }
                output.write_char('}')
            }
        }
    }

    let max_chars = max_chars.min(512);
    if max_chars == 0 {
        return String::new();
    }
    let mut output = Preview {
        text: String::with_capacity(max_chars),
        remaining: max_chars,
        truncated: false,
    };
    let _ = render(&mut output, value, 0);
    if output.truncated {
        output.text.pop();
        output.text.push('…');
    }
    output.text
}

fn ready(value: Value) -> Result<Value> {
    match value {
        Value::Hole(name) => Err(Fault::Missing(format!("resolve ?{name}"))),
        value => Ok(value),
    }
}

fn ready_ref(value: &Value) -> Result<&Value> {
    match value {
        Value::Hole(name) => Err(Fault::Missing(format!("resolve ?{name}"))),
        value => Ok(value),
    }
}

fn read_name<'a>(env: &'a Scope<'_>, name: &str) -> Result<&'a Value> {
    let mut path = name.split('.');
    let root = path.next().unwrap_or_default();
    let mut value = env
        .get(root)
        .ok_or_else(|| conflict(format!("unknown binding {root}")))?;
    for key in path {
        value = field_ref(value, key)?;
    }
    Ok(value)
}

fn field_ref<'a>(value: &'a Value, key: &str) -> Result<&'a Value> {
    match ready_ref(value)? {
        Value::Record(fields) => fields
            .get(key)
            .ok_or_else(|| conflict(format!("record has no field {key}"))),
        _ => Err(conflict(format!("cannot read {key} from a non-record"))),
    }
}

fn resolved(value: &Value, budget: &mut Budget) -> Result<()> {
    budget.tick()?;
    match value {
        Value::Hole(name) => return Err(Fault::Missing(format!("resolve ?{name}"))),
        Value::List(values) => {
            for value in values {
                resolved(value, budget)?;
            }
        }
        Value::Record(fields) => {
            for value in fields.values() {
                resolved(value, budget)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn boolean(value: Value) -> Result<bool> {
    match ready(value)? {
        Value::Bool(value) => Ok(value),
        _ => Err(conflict("expected a boolean")),
    }
}

fn list(value: Value) -> Result<Vec<Value>> {
    match ready(value)? {
        Value::List(values) => Ok(values),
        _ => Err(conflict("expected a list")),
    }
}

fn text(value: Value) -> Result<String> {
    match ready(value)? {
        Value::Text(value) => Ok(value),
        _ => Err(conflict("expected text")),
    }
}

fn arity(name: &str, args: &[Expr], count: usize) -> Result<()> {
    if args.len() == count {
        Ok(())
    } else {
        Err(conflict(format!(
            "{name} expects {count} arguments, got {}",
            args.len()
        )))
    }
}

fn compare(left: &Value, right: &Value) -> Result<Ordering> {
    match (left, right) {
        (Value::Hole(name), _) | (_, Value::Hole(name)) => {
            Err(Fault::Missing(format!("resolve ?{name}")))
        }
        (Value::Number(a), Value::Number(b)) => Ok(a.cmp(b)),
        (Value::Quantity(a, u), Value::Quantity(b, v)) if u == v => Ok(a.cmp(b)),
        (Value::Date(a), Value::Date(b)) => Ok(a.cmp(b)),
        (Value::Text(a), Value::Text(b)) | (Value::Ref(a), Value::Ref(b)) => Ok(a.cmp(b)),
        (Value::Bool(a), Value::Bool(b)) => Ok(a.cmp(b)),
        _ => Err(conflict("comparison requires compatible types and units")),
    }
}

fn arithmetic(op: &str, a: Value, b: Value) -> Result<Value> {
    let a = ready(a)?;
    let b = ready(b)?;
    let calculate = |a: &Number, b: &Number| {
        match op {
            "add" => a.add(b),
            "sub" => a.sub(b),
            "mul" => a.mul(b),
            "div" => a.div(b),
            _ => unreachable!("arithmetic operator dispatched by evaluator"),
        }
        .map_err(conflict)
    };
    match (op, a, b) {
        (_, Value::Number(a), Value::Number(b)) => Ok(Value::Number(calculate(&a, &b)?)),
        ("add" | "sub", Value::Quantity(a, u), Value::Quantity(b, v)) if u == v => {
            Ok(Value::Quantity(calculate(&a, &b)?, u))
        }
        ("add" | "sub", Value::Quantity(a, u), Value::Number(b)) if b.is_zero() => {
            Ok(Value::Quantity(calculate(&a, &b)?, u))
        }
        ("add" | "sub", Value::Number(a), Value::Quantity(b, u)) if a.is_zero() => {
            Ok(Value::Quantity(calculate(&a, &b)?, u))
        }
        ("mul" | "div", Value::Quantity(a, u), Value::Number(b)) => {
            Ok(Value::Quantity(calculate(&a, &b)?, u))
        }
        ("mul", Value::Number(a), Value::Quantity(b, u)) => {
            Ok(Value::Quantity(calculate(&a, &b)?, u))
        }
        ("div", Value::Quantity(a, u), Value::Quantity(b, v)) if u == v => {
            Ok(Value::Number(calculate(&a, &b)?))
        }
        _ => Err(conflict(format!("{op} requires compatible dimensions"))),
    }
}

// Unlike a comparison callback passed to slice::sort_by, this can stop work
// immediately when the budget expires or keys are incompatible.
fn sort_values(
    mut values: Vec<(Value, Value)>,
    budget: &mut Budget,
) -> Result<Vec<(Value, Value)>> {
    budget.tick()?;
    if values.len() < 2 {
        return Ok(values);
    }
    let right = values.split_off(values.len() / 2);
    let mut left = sort_values(values, budget)?.into_iter().peekable();
    let mut right = sort_values(right, budget)?.into_iter().peekable();
    let mut sorted = Vec::with_capacity(left.len() + right.len());
    while let (Some((a, _)), Some((b, _))) = (left.peek(), right.peek()) {
        budget.tick()?;
        let next = if compare(a, b)?.is_gt() {
            right.next()
        } else {
            left.next()
        };
        if let Some(next) = next {
            sorted.push(next);
        }
    }
    for value in left.chain(right) {
        budget.tick()?;
        sorted.push(value);
    }
    Ok(sorted)
}

pub(crate) fn eval(
    expr: &Expr,
    env: &Env,
    relations: &Relations,
    budget: &mut Budget,
) -> Result<Value> {
    eval_at(
        expr,
        &Scope::Root(env),
        relations,
        budget,
        0,
        Execution::Borrowed,
    )
}

fn enter(budget: &mut Budget, depth: usize) -> Result<()> {
    budget.tick()?;
    if depth > 96 {
        return Err(Fault::Incomplete("expression nesting limit reached".into()));
    }
    Ok(())
}

fn relation<'a>(args: &[Expr], relations: &'a Relations) -> Result<&'a [Value]> {
    arity("rows", args, 1)?;
    let name = match &args[0] {
        Expr::Name(name) | Expr::Literal(Value::Text(name)) => name,
        _ => return Err(conflict("rows requires a static schema name")),
    };
    relations
        .get(name)
        .map(Vec::as_slice)
        .ok_or_else(|| conflict(format!("unknown relation {name}")))
}

fn binding(args: &[Expr]) -> Result<&str> {
    let Expr::Name(binding) = &args[1] else {
        return Err(conflict("iteration requires a binding name"));
    };
    if binding.contains('.') {
        return Err(conflict("iteration binding must be a simple name"));
    }
    Ok(binding)
}

#[cfg(test)]
fn owning_environment(env: &Scope<'_>, execution: Execution) -> Option<Env> {
    if execution.borrows() {
        None
    } else {
        Some(env.owned())
    }
}

#[cfg(not(test))]
fn owning_environment(_: &Scope<'_>, _: Execution) -> Option<Env> {
    None
}

#[allow(clippy::too_many_arguments)]
fn eval_bound(
    expr: &Expr,
    env: &Scope<'_>,
    relations: &Relations,
    budget: &mut Budget,
    depth: usize,
    execution: Execution,
    name: &str,
    value: &Value,
    copied: &mut Option<Env>,
) -> Result<Value> {
    if let Some(copied) = copied {
        copied.insert(name.to_owned(), value.clone());
        eval_at(
            expr,
            &Scope::Root(copied),
            relations,
            budget,
            depth,
            execution,
        )
    } else {
        eval_at(
            expr,
            &Scope::Binding {
                parent: env,
                name,
                value,
            },
            relations,
            budget,
            depth,
            execution,
        )
    }
}

fn sequence<'a>(
    expr: &'a Expr,
    env: &Scope<'_>,
    relations: &'a Relations,
    budget: &mut Budget,
    depth: usize,
    execution: Execution,
) -> Result<Sequence<'a>> {
    if execution.borrows() {
        match expr {
            Expr::Literal(Value::List(values)) => {
                enter(budget, depth)?;
                return Ok(Sequence::Slice(values));
            }
            Expr::Call(name, args) if name == "rows" => {
                enter(budget, depth)?;
                let rows = relation(args, relations)?;
                for _ in rows {
                    budget.tick()?;
                }
                return Ok(Sequence::Slice(rows));
            }
            Expr::Call(name, args) if name == "filter" || name == "map" => {
                enter(budget, depth)?;
                return transform(name, args, env, relations, budget, depth, execution);
            }
            _ => {}
        }
    }
    list(eval_at(expr, env, relations, budget, depth, execution)?).map(Sequence::Owned)
}

fn transform<'a>(
    name: &str,
    args: &'a [Expr],
    env: &Scope<'_>,
    relations: &'a Relations,
    budget: &mut Budget,
    depth: usize,
    execution: Execution,
) -> Result<Sequence<'a>> {
    arity(name, args, 3)?;
    let values = sequence(&args[0], env, relations, budget, depth + 1, execution)?;
    let binding = binding(args)?;
    let borrowed = values.borrowed();
    let mut copied = owning_environment(env, execution);
    let mut selected = Vec::new();
    let mut result = Vec::new();
    for value in values {
        budget.tick()?;
        let mapped = eval_bound(
            &args[2],
            env,
            relations,
            budget,
            depth + 1,
            execution,
            binding,
            &value,
            &mut copied,
        )?;
        if name == "map" {
            result.push(mapped);
        } else if boolean(mapped)? {
            match value {
                Cow::Borrowed(value) => selected.push(value),
                Cow::Owned(value) => result.push(value),
            }
        }
    }
    Ok(if name == "filter" && borrowed {
        Sequence::Selected(selected)
    } else {
        Sequence::Owned(result)
    })
}

fn operand<'a>(
    expr: &'a Expr,
    env: &'a Scope<'_>,
    relations: &Relations,
    budget: &mut Budget,
    depth: usize,
    execution: Execution,
) -> Result<Cow<'a, Value>> {
    if execution.borrows() {
        match expr {
            Expr::Literal(value) => {
                enter(budget, depth)?;
                return Ok(Cow::Borrowed(value));
            }
            Expr::Name(name) => {
                enter(budget, depth)?;
                return read_name(env, name).map(Cow::Borrowed);
            }
            _ => {}
        }
    }
    eval_at(expr, env, relations, budget, depth, execution).map(Cow::Owned)
}

fn eval_at(
    expr: &Expr,
    env: &Scope<'_>,
    relations: &Relations,
    budget: &mut Budget,
    depth: usize,
    execution: Execution,
) -> Result<Value> {
    enter(budget, depth)?;
    let (name, args) = match expr {
        Expr::Literal(value) => return Ok(value.clone()),
        Expr::Name(name) => return read_name(env, name).cloned(),
        Expr::Call(name, args) => (name.as_str(), args),
    };
    let eval = |expr: &Expr, budget: &mut Budget| {
        eval_at(expr, env, relations, budget, depth + 1, execution)
    };
    match name {
        "if" => {
            arity(name, args, 3)?;
            eval(
                &args[if boolean(eval(&args[0], budget)?)? {
                    1
                } else {
                    2
                }],
                budget,
            )
        }
        "and" | "or" => {
            if args.is_empty() {
                return Err(conflict(format!("{name} needs at least one argument")));
            }
            // False/true can decide a conjunction/disjunction even when another term is unknown.
            let mut unknown = None;
            for arg in args {
                match eval(arg, budget).and_then(boolean) {
                    Ok(value) if value == (name == "or") => return Ok(Value::Bool(value)),
                    Ok(_) => {}
                    Err(error @ Fault::Missing(_)) => {
                        unknown.get_or_insert(error);
                    }
                    Err(error) => return Err(error),
                }
            }
            match unknown {
                Some(error) => Err(error),
                None => Ok(Value::Bool(name == "and")),
            }
        }
        "not" => {
            arity(name, args, 1)?;
            Ok(Value::Bool(!boolean(eval(&args[0], budget)?)?))
        }
        "rows" => {
            let rows = relation(args, relations)?;
            for _ in rows {
                budget.tick()?;
            }
            Ok(Value::List(rows.to_vec()))
        }
        "filter" | "map" => transform(name, args, env, relations, budget, depth, execution)
            .map(|values| Value::List(values.into_values())),
        "all" | "any" | "sort" => {
            arity(name, args, 3)?;
            let values = sequence(&args[0], env, relations, budget, depth + 1, execution)?;
            let binding = binding(args)?;
            let mut copied = owning_environment(env, execution);
            let mut keyed = Vec::new();
            let mut unknown = None;
            for value in values {
                budget.tick()?;
                let mapped = match eval_bound(
                    &args[2],
                    env,
                    relations,
                    budget,
                    depth + 1,
                    execution,
                    binding,
                    &value,
                    &mut copied,
                ) {
                    Ok(value) => value,
                    Err(error @ Fault::Missing(_)) if name == "all" || name == "any" => {
                        unknown.get_or_insert(error);
                        continue;
                    }
                    Err(error) => return Err(error),
                };
                if name == "all" || name == "any" {
                    match boolean(mapped) {
                        Ok(value) if value == (name == "any") => return Ok(Value::Bool(value)),
                        Ok(_) => {}
                        Err(error @ Fault::Missing(_)) => {
                            unknown.get_or_insert(error);
                        }
                        Err(error) => return Err(error),
                    }
                    continue;
                }
                keyed.push((ready(mapped)?, value.into_owned()));
            }
            match name {
                "all" | "any" => match unknown {
                    Some(error) => Err(error),
                    None => Ok(Value::Bool(name == "all")),
                },
                "sort" => {
                    if let Some((first, _)) = keyed.first() {
                        for (key, _) in &keyed {
                            budget.tick()?;
                            compare(first, key)?;
                        }
                    }
                    let keyed = sort_values(keyed, budget)?;
                    Ok(Value::List(
                        keyed.into_iter().map(|(_, value)| value).collect(),
                    ))
                }
                _ => unreachable!(),
            }
        }
        "add" | "sub" | "mul" | "div" => {
            arity(name, args, 2)?;
            arithmetic(name, eval(&args[0], budget)?, eval(&args[1], budget)?)
        }
        "eq" | "ne" | "lt" | "le" | "gt" | "ge" => {
            arity(name, args, 2)?;
            let left = operand(&args[0], env, relations, budget, depth + 1, execution)?;
            ready_ref(&left)?;
            let right = operand(&args[1], env, relations, budget, depth + 1, execution)?;
            ready_ref(&right)?;
            resolved(&left, budget)?;
            resolved(&right, budget)?;
            let value = if name == "eq" || name == "ne" {
                let equal = *left == *right;
                if name == "eq" { equal } else { !equal }
            } else {
                let ordering = compare(&left, &right)?;
                match name {
                    "lt" => ordering.is_lt(),
                    "le" => !ordering.is_gt(),
                    "gt" => ordering.is_gt(),
                    "ge" => !ordering.is_lt(),
                    _ => unreachable!(),
                }
            };
            Ok(Value::Bool(value))
        }
        "ref" => {
            arity(name, args, 1)?;
            let record = operand(&args[0], env, relations, budget, depth + 1, execution)?;
            let value = ready_ref(field_ref(&record, "id")?)?;
            if !matches!(value, Value::Ref(_)) {
                return Err(conflict("record id must be an explicit reference"));
            }
            Ok(value.clone())
        }
        "get" => {
            arity(name, args, 2)?;
            let key = match &args[1] {
                Expr::Name(name) => Cow::Borrowed(name.as_str()),
                expr => Cow::Owned(text(eval(expr, budget)?)?),
            };
            let record = operand(&args[0], env, relations, budget, depth + 1, execution)?;
            field_ref(&record, &key).cloned()
        }
        "first" | "last" | "count" | "sum" => {
            arity(name, args, 1)?;
            let values = sequence(&args[0], env, relations, budget, depth + 1, execution)?;
            match name {
                "count" => Ok(Value::Number(
                    values.len().to_string().parse().map_err(conflict)?,
                )),
                "sum" => {
                    let mut sum = Value::Number("0".parse().map_err(conflict)?);
                    for value in values {
                        budget.tick()?;
                        sum = arithmetic("add", sum, value.into_owned())?;
                    }
                    Ok(sum)
                }
                _ => {
                    let value = if name == "first" {
                        values.into_iter().next()
                    } else {
                        values.into_iter().next_back()
                    };
                    value
                        .map(Cow::into_owned)
                        .ok_or_else(|| Fault::Missing(format!("{name} requires a nonempty list")))
                }
            }
        }
        "contains" => {
            arity(name, args, 2)?;
            let values = sequence(&args[0], env, relations, budget, depth + 1, execution)?;
            let target = ready(eval(&args[1], budget)?)?;
            resolved(&target, budget)?;
            let mut unknown = None;
            for value in values {
                match resolved(&value, budget) {
                    Ok(()) if *value == target => return Ok(Value::Bool(true)),
                    Ok(()) => {}
                    Err(error @ Fault::Missing(_)) => {
                        unknown.get_or_insert(error);
                    }
                    Err(error) => return Err(error),
                }
            }
            if let Some(error) = unknown {
                return Err(error);
            }
            Ok(Value::Bool(false))
        }
        "concat" => {
            let mut values = Vec::new();
            for arg in args {
                for value in sequence(arg, env, relations, budget, depth + 1, execution)? {
                    budget.tick()?;
                    values.push(value.into_owned());
                }
            }
            Ok(Value::List(values))
        }
        "list" => args
            .iter()
            .map(|arg| eval(arg, budget))
            .collect::<Result<Vec<_>>>()
            .map(Value::List),
        "pairs" => {
            arity(name, args, 1)?;
            let values = list(eval(&args[0], budget)?)?;
            let mut pairs = Vec::with_capacity(values.len().saturating_sub(1));
            for pair in values.windows(2) {
                budget.tick()?;
                pairs.push(Value::List(pair.to_vec()));
            }
            Ok(Value::List(pairs))
        }
        "at" => {
            arity(name, args, 2)?;
            let values = sequence(&args[0], env, relations, budget, depth + 1, execution)?;
            let Value::Number(index) = ready(eval(&args[1], budget)?)? else {
                return Err(conflict("list index must be a nonnegative integer"));
            };
            let index = index
                .to_string()
                .parse::<usize>()
                .map_err(|_| conflict("list index must be a nonnegative integer"))?;
            values
                .into_iter()
                .nth(index)
                .map(Cow::into_owned)
                .ok_or_else(|| Fault::Missing(format!("list has no item at index {index}")))
        }
        "quantity" => {
            arity(name, args, 2)?;
            let Value::Number(number) = ready(eval(&args[0], budget)?)? else {
                return Err(conflict("quantity requires a number"));
            };
            let unit = text(eval(&args[1], budget)?)?;
            if !crate::value::valid_unit(&unit) {
                return Err(conflict("invalid quantity unit"));
            }
            Ok(Value::Quantity(number, unit))
        }
        "unit" | "number" => {
            arity(name, args, 1)?;
            let Value::Quantity(number, unit) = ready(eval(&args[0], budget)?)? else {
                return Err(conflict(format!("{name} requires a quantity")));
            };
            Ok(if name == "unit" {
                Value::Text(unit)
            } else {
                Value::Number(number)
            })
        }
        _ => Err(conflict(format!("unknown operator {name}"))),
    }
}

#[cfg(test)]
#[path = "../lab/src/alloc.rs"]
mod measured_allocator;

#[cfg(test)]
mod tests {
    use super::*;

    #[global_allocator]
    static ALLOCATOR: measured_allocator::Counting = measured_allocator::Counting;

    fn run(source: &str) -> Result<Value> {
        eval(
            &Expr::parse(source).unwrap(),
            &Env::new(),
            &Relations::new(),
            &mut Budget::new(1000),
        )
    }

    #[test]
    fn arithmetic_preserves_dimensions_and_exactness() {
        assert_eq!(
            run("(div (quantity 1 \"USD\") 3)").unwrap().to_string(),
            "1/3 USD"
        );
        assert!(run("(add (quantity 1 \"USD\") (quantity 1 \"EUR\"))").is_err());
        assert!(run("(div 1 0)").is_err());
        assert_eq!(
            run("(add (sum []) (quantity 3 \"USD\"))")
                .unwrap()
                .to_string(),
            "3 USD"
        );
    }

    #[test]
    fn missing_values_are_not_silently_filtered() {
        assert!(matches!(run("(eq ?choice @a)"), Err(Fault::Missing(_))));
        assert_eq!(
            run("(and (eq ?choice @a) false)").unwrap(),
            Value::Bool(false)
        );
        assert!(matches!(run("(first [])"), Err(Fault::Missing(_))));
    }

    #[test]
    fn work_is_bounded_and_branches_are_lazy() {
        assert_eq!(
            run("(if true 1 (div 1 0))").unwrap(),
            Value::Number("1".parse().unwrap())
        );
        assert!(matches!(
            eval(
                &Expr::parse("(add 1 2)").unwrap(),
                &Env::new(),
                &Relations::new(),
                &mut Budget::new(1)
            ),
            Err(Fault::Incomplete(_))
        ));
    }

    #[test]
    fn diagnostic_previews_escape_text_and_bound_unicode_output() {
        let text = Value::Text("a\n\"b\\c".into());
        assert_eq!(preview(&text, 64), "\"a\\n\\\"b\\\\c\"");
        assert_eq!(
            preview(&Value::Text("\u{0085}\u{2028}\u{2029}".into()), 64),
            "\"\\u0085\\u2028\\u2029\""
        );
        assert_eq!(preview(&Value::Bool(true), 4), "true");
        assert_eq!(preview(&Value::Bool(true), 3), "tr…");
        assert_eq!(preview(&Value::Bool(true), 1), "…");
        assert_eq!(preview(&Value::Bool(true), 0), "");

        let large = Value::Text("é🙂".repeat(50_000));
        for limit in 0..20 {
            let rendered = preview(&large, limit);
            assert!(rendered.chars().count() <= limit);
            if limit != 0 {
                assert!(rendered.ends_with('…'));
            }
        }
        assert_eq!(preview(&large, usize::MAX).chars().count(), 512);

        let record = Value::parse("{amount: 1/3 USD, target: @sale/one}").unwrap();
        assert_eq!(
            preview(&record, 80),
            "{\"amount\": 1/3 USD, \"target\": @sale/one}"
        );
        assert_eq!(preview(&Value::Hole("lot".into()), 10), "?lot");
    }

    #[test]
    fn diagnostic_previews_stop_at_container_depth_and_length_bounds() {
        let mut nested = Value::Bool(true);
        for _ in 0..128 {
            nested = Value::List(vec![nested]);
        }
        let rendered = preview(&nested, 128);
        assert!(rendered.contains("[…]"));
        assert!(rendered.len() < 32);

        let values = Value::List(
            (0..10_000)
                .map(|_| Value::Text("a long text".into()))
                .collect(),
        );
        assert_eq!(preview(&values, 12).chars().count(), 12);
        assert!(preview(&values, 12).ends_with('…'));
        let number = Value::Number("1".repeat(4096).parse().unwrap());
        assert_eq!(preview(&number, 16), "111111111111111…");
    }

    fn evaluate_mode(
        expr: &Expr,
        env: &Env,
        relations: &Relations,
        limit: usize,
        execution: Execution,
    ) -> (Result<Value>, usize) {
        let mut budget = Budget::new(limit);
        let result = eval_at(
            expr,
            &Scope::Root(env),
            relations,
            &mut budget,
            0,
            execution,
        );
        (result, budget.remaining)
    }

    fn fixture(seed: usize) -> (Env, Relations) {
        let clean = seed.is_multiple_of(2);
        let count = if seed == 0 { 0 } else { seed % 13 + 3 };
        let mut rng = seed as u64 + 7919;
        let mut rows = Vec::new();
        for i in 0..count {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            let account = if !clean && rng.is_multiple_of(7) {
                Value::Hole("account".into())
            } else {
                Value::Text(format!("account{}", i % 3))
            };
            let amount = if !clean && rng.is_multiple_of(11) {
                Value::Hole("amount".into())
            } else if !clean && rng.is_multiple_of(5) {
                Value::Number("0".parse().unwrap())
            } else {
                Value::Quantity(
                    format!("{}/3", i as i64 - 4).parse().unwrap(),
                    if !clean && rng.is_multiple_of(3) {
                        "EUR"
                    } else {
                        "USD"
                    }
                    .into(),
                )
            };
            let active = if !clean && rng.is_multiple_of(13) {
                Value::Hole("active".into())
            } else if !clean && rng.is_multiple_of(17) {
                Value::Text("not a bool".into())
            } else {
                Value::Bool(i % 3 != 0)
            };
            let mut fields = BTreeMap::from([
                ("id".into(), Value::Ref(format!("line{i}"))),
                ("account".into(), account),
                ("amount".into(), amount),
                ("active".into(), active),
                (
                    "nested".into(),
                    Value::Record(BTreeMap::from([(
                        "value".into(),
                        if !clean && rng.is_multiple_of(2) {
                            Value::Hole("nested".into())
                        } else {
                            Value::Bool(true)
                        },
                    )])),
                ),
            ]);
            if !clean && rng.is_multiple_of(19) {
                fields.remove("account");
            }
            rows.push(if !clean && rng.is_multiple_of(23) {
                Value::Hole("row".into())
            } else if !clean && rng.is_multiple_of(29) {
                Value::Bool(false)
            } else {
                Value::Record(fields)
            });
        }
        let env = BTreeMap::from([
            (
                "s".into(),
                Value::Record(BTreeMap::from([
                    ("account".into(), Value::Text("account1".into())),
                    (
                        "amount".into(),
                        Value::Quantity("1/3".parse().unwrap(), "USD".into()),
                    ),
                ])),
            ),
            ("x".into(), Value::Text("outer binding".into())),
            ("items".into(), Value::List(rows.clone())),
        ]);
        (
            env,
            BTreeMap::from([("fact".into(), rows), ("empty".into(), Vec::new())]),
        )
    }

    fn compare_every_budget(expr: &Expr, env: &Env, relations: &Relations) {
        let (_, remaining) = evaluate_mode(expr, env, relations, 100_000, Execution::Owning);
        let used = 100_000 - remaining;
        for limit in 0..=used + 2 {
            let old = evaluate_mode(expr, env, relations, limit, Execution::Owning);
            let new = evaluate_mode(expr, env, relations, limit, Execution::Borrowed);
            assert_eq!(new, old, "execution differs for {expr:?} at budget {limit}");
        }
    }

    #[test]
    fn borrowed_sequences_match_owning_values_faults_order_and_every_budget() {
        let expressions = [
            "(rows fact)",
            "(rows absent)",
            "(rows (if true \"fact\" \"empty\"))",
            "(filter (rows fact) x (eq x.account s.account))",
            "(map (filter (rows fact) x (and (eq x.account s.account) x.active)) x x.amount)",
            "(sum (map (filter (rows fact) x (and (eq x.account s.account) x.active)) x x.amount))",
            "(count (filter (filter (rows fact) x (eq x.account s.account)) x x.active))",
            "(first (filter (rows fact) x x.active))",
            "(last (map (rows fact) x (ref x)))",
            "(all (rows fact) x x.active)",
            "(any (rows fact) x x.active)",
            "(all (map (rows fact) x x.nested) x (eq x x))",
            "(any (filter (rows fact) x (eq x.account s.account)) x x.active)",
            "(sort (filter (rows fact) x x.active) x x.amount)",
            "(map (sort (rows fact) x x.amount) x (get x account))",
            "(contains (map (rows fact) x x.nested) {value: true})",
            "(contains (rows fact) ?target)",
            "(concat (filter (rows fact) x x.active) (rows empty))",
            "(at (filter (rows fact) x x.active) 1)",
            "(at (rows fact) -1)",
            "(pairs (map (rows fact) x x.amount))",
            "(sum (map items x x.amount))",
            "(map (rows fact) x (list x.account (map [1, 2] x (add x 1)) x.account))",
            "(map (rows fact) x (count (map [1, 2] y (get x account))))",
            "(all [?pending, false] x x)",
            "(any [?pending, true] x x)",
            "(first [])",
            "(sum [])",
            "(at [1, 2] 1/2)",
            "(filter (rows empty) x.path true)",
            "(filter (rows absent) x.path true)",
        ];
        for seed in 0..20 {
            let (env, relations) = fixture(seed);
            for source in expressions {
                compare_every_budget(&Expr::parse(source).unwrap(), &env, &relations);
            }
        }
        // Model parsing already limits authored expressions to depth 64. Test
        // the interpreter's additional guard directly without consuming a
        // test runner's small debug stack with an unparseable expression.
        for limit in 0..3 {
            let expr = Expr::Literal(Value::Bool(true));
            let mut old = Budget::new(limit);
            let mut new = Budget::new(limit);
            assert_eq!(
                eval_at(
                    &expr,
                    &Scope::Root(&Env::new()),
                    &Relations::new(),
                    &mut old,
                    97,
                    Execution::Owning
                ),
                eval_at(
                    &expr,
                    &Scope::Root(&Env::new()),
                    &Relations::new(),
                    &mut new,
                    97,
                    Execution::Borrowed
                )
            );
            assert_eq!(old.remaining, new.remaining);
        }
    }

    #[test]
    fn eager_query_stages_preserve_fault_precedence() {
        let rows = vec![
            Value::parse("{active: true, amount: 1 USD}").unwrap(),
            Value::parse("{active: true, amount: 1 EUR}").unwrap(),
            Value::parse("{active: false, amount: 1 USD}").unwrap(),
        ];
        let relations = BTreeMap::from([("fact".into(), rows)]);
        let source = "(sum (map (rows fact) x (if x.active x.amount (div 1 0))))";
        let expr = Expr::parse(source).unwrap();
        compare_every_budget(&expr, &Env::new(), &relations);
        assert!(
            matches!(eval(&expr, &Env::new(), &relations, &mut Budget::new(1000)),
            Err(Fault::Conflict(message)) if message.contains("division by zero"))
        );
        let expr =
            Expr::parse("(first (filter (rows fact) x (if x.active true (eq ?late 1))))").unwrap();
        compare_every_budget(&expr, &Env::new(), &relations);
        assert!(matches!(
            eval(&expr, &Env::new(), &relations, &mut Budget::new(1000)),
            Err(Fault::Missing(_))
        ));
    }

    #[test]
    #[ignore = "production interpreter timing/allocation probe; run explicitly with --test-threads=1"]
    fn production_borrowing_benchmark() {
        use std::hint::black_box;
        use std::time::Instant;
        for n in [10_000, 100_000] {
            let rows = (0..n)
                .map(|i| {
                    Value::Record(BTreeMap::from([
                        ("id".into(), Value::Ref(format!("line{i}"))),
                        (
                            "account".into(),
                            Value::Text(format!("account{:02}", i % 64)),
                        ),
                        (
                            "amount".into(),
                            Value::Quantity(
                                format!("{}/100", i % 100_000).parse().unwrap(),
                                "USD".into(),
                            ),
                        ),
                        ("active".into(), Value::Bool(i % 13 != 0)),
                        (
                            "memo".into(),
                            Value::Text("a realistic repeated ledger description".into()),
                        ),
                        ("date".into(), Value::Date("2026-09-26".parse().unwrap())),
                    ]))
                })
                .collect::<Vec<_>>();
            let relations = BTreeMap::from([("fact".into(), rows)]);
            let env = BTreeMap::from([(
                "s".into(),
                Value::Record(BTreeMap::from([(
                    "account".into(),
                    Value::Text("account07".into()),
                )])),
            )]);
            let expr = Expr::parse("(sum (map (filter (rows fact) x (and (eq x.account s.account) x.active)) x x.amount))").unwrap();
            let expected = evaluate_mode(&expr, &env, &relations, 10_000_000, Execution::Owning);
            for execution in [Execution::Owning, Execution::Borrowed] {
                measured_allocator::start(0);
                let measured = evaluate_mode(&expr, &env, &relations, 10_000_000, execution);
                let allocations = measured_allocator::stop();
                assert_eq!(measured, expected);
                drop(measured);
                let mut best = f64::INFINITY;
                for _ in 0..3 {
                    let start = Instant::now();
                    let actual = black_box(evaluate_mode(
                        black_box(&expr),
                        black_box(&env),
                        black_box(&relations),
                        10_000_000,
                        execution,
                    ));
                    let millis = start.elapsed().as_secs_f64() * 1000.0;
                    assert_eq!(actual, expected);
                    best = best.min(millis);
                }
                println!(
                    "production-interpreter,{},{n},{best:.3},{},{},{},{},{:?}",
                    if execution.borrows() {
                        "borrowed"
                    } else {
                        "owning-reference"
                    },
                    allocations.calls,
                    allocations.bytes,
                    allocations.live,
                    allocations.peak,
                    expected
                );
            }
        }
    }
}
