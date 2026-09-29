//! Infer result shapes from the same expressions that compute their values.

use crate::model::{Expr, Instruction, Rule, Schema, Type};
use crate::{Block, Diagnostic, Span, Value};
use std::collections::BTreeMap;

type Shapes = BTreeMap<String, Schema>;
type Scope = BTreeMap<String, Type>;
type Result<T> = std::result::Result<T, String>;

/// Rules are already in dependency order. Each relation is the compatible union
/// of its producers; no authored output declaration is necessary.
pub(crate) fn infer_outputs(
    shapes: &mut Shapes,
    rules: &[Rule],
    locations: &BTreeMap<String, Block>,
) -> std::result::Result<(), Vec<(String, Diagnostic)>> {
    let mut errors = Vec::new();
    for rule in rules {
        if let Err((site, message)) = infer_rule(shapes, rule) {
            let mut diagnostic = Diagnostic::new(0, format!("rule {}: {message}", rule.name));
            if let Some(block) = locations.get(&rule.name) {
                diagnostic = diagnostic.with_span(site.span(block));
            }
            errors.push((rule.name.clone(), diagnostic));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

enum Site {
    Header,
    Step(usize),
    Field(String),
}

impl Site {
    fn span(&self, block: &Block) -> Span {
        let field = match self {
            Self::Header => None,
            Self::Step(index) => block
                .fields
                .iter()
                .filter(|field| matches!(field.name.as_str(), "let" | "choose" | "require"))
                .nth(*index),
            Self::Field(name) => block.fields.iter().find(|field| {
                field.name == "set" && field.value.split_whitespace().next() == Some(name)
            }),
        };
        field.map_or(block.span, |field| field.span)
    }
}

fn infer_rule(shapes: &mut Shapes, rule: &Rule) -> std::result::Result<(), (Site, String)> {
    let source = row_type(shapes, &rule.source).map_err(|error| (Site::Header, error))?;
    let mut scope = BTreeMap::from([(rule.binding.clone(), source)]);
    for (index, instruction) in rule.steps.iter().enumerate() {
        let result = (|| {
            match instruction {
                Instruction::Let(name, expr) => {
                    let ty = infer(expr, &scope, shapes)?;
                    bind(&mut scope, name, ty)?;
                }
                Instruction::Choose(name, selector, candidates) => {
                    let selected = infer(selector, &scope, shapes)?;
                    if !matches!(selected, Type::Any | Type::Ref(_)) {
                        return Err(format!(
                            "choice selector must be a reference, found {selected}"
                        ));
                    }
                    let ty = item(infer(candidates, &scope, shapes)?)?;
                    bind(&mut scope, name, ty)?;
                }
                Instruction::Require(expr) => {
                    expect(infer(expr, &scope, shapes)?, &Type::Bool)?;
                }
            }
            Ok(())
        })();
        result.map_err(|error| (Site::Step(index), error))?;
    }
    let inferred = rule
        .fields
        .iter()
        .map(|(name, expr)| {
            infer(expr, &scope, shapes)
                .map(|ty| (name.clone(), ty))
                .map_err(|error| (Site::Field(name.clone()), error))
        })
        .collect::<std::result::Result<BTreeMap<_, _>, _>>()?;
    let output = shapes
        .get_mut(&rule.output)
        .ok_or_else(|| (Site::Header, "missing inferred output relation".into()))?;
    for (name, ty) in inferred {
        let field = output.fields.get_mut(&name).ok_or_else(|| {
            (
                Site::Field(name.clone()),
                "inconsistent producer fields".into(),
            )
        })?;
        *field = unify(field, &ty).map_err(|error| {
            (
                Site::Field(name.clone()),
                format!(
                    "{}.{} has incompatible producer types: {error}",
                    rule.output, name
                ),
            )
        })?;
    }
    Ok(())
}

fn bind(scope: &mut Scope, name: &str, ty: Type) -> Result<()> {
    if !name
        .bytes()
        .next()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err(format!("`{name}` is not a simple binding name"));
    }
    if scope.contains_key(name) {
        return Err(format!("binding `{name}` shadows an existing binding"));
    }
    scope.insert(name.to_owned(), ty);
    Ok(())
}

fn row_type(shapes: &Shapes, name: &str) -> Result<Type> {
    let shape = shapes
        .get(name)
        .ok_or_else(|| format!("unknown relation {name}"))?;
    let mut fields = shape.fields.clone();
    fields.insert("id".into(), Type::Ref(name.into()));
    Ok(Type::Record(fields))
}

pub(crate) fn unify(a: &Type, b: &Type) -> Result<Type> {
    match (a, b) {
        (Type::Any, other) | (other, Type::Any) => Ok(other.clone()),
        (Type::List(a), Type::List(b)) => Ok(Type::List(Box::new(unify(a, b)?))),
        (Type::Record(a), Type::Record(b)) if a.keys().eq(b.keys()) => a
            .iter()
            .map(|(name, ty)| unify(ty, &b[name]).map(|ty| (name.clone(), ty)))
            .collect::<Result<BTreeMap<_, _>>>()
            .map(Type::Record),
        (a, b) if a == b => Ok(a.clone()),
        _ => Err(format!("{a} and {b}")),
    }
}

fn expect(actual: Type, expected: &Type) -> Result<()> {
    unify(&actual, expected)
        .map(|_| ())
        .map_err(|_| format!("expected {expected}, found {actual}"))
}

fn item(ty: Type) -> Result<Type> {
    match ty {
        Type::List(ty) => Ok(*ty),
        Type::Any => Ok(Type::Any),
        other => Err(format!("expected list, found {other}")),
    }
}

fn numeric(ty: Type) -> Result<Type> {
    match ty {
        Type::Any | Type::Number | Type::Quantity => Ok(ty),
        other => Err(format!("expected a number or quantity, found {other}")),
    }
}

fn field(ty: Type, name: &str) -> Result<Type> {
    match ty {
        Type::Record(fields) => fields
            .get(name)
            .cloned()
            .ok_or_else(|| format!("unknown field {name}")),
        Type::Any => Ok(Type::Any),
        other => Err(format!("cannot read field {name} from {other}")),
    }
}

pub(crate) fn value_type(value: &Value) -> Type {
    match value {
        Value::Number(_) => Type::Number,
        Value::Quantity(..) => Type::Quantity,
        Value::Date(_) => Type::Date,
        Value::Text(_) => Type::Text,
        Value::Bool(_) => Type::Bool,
        Value::Ref(_) => Type::Any,
        Value::Hole(_) => Type::Any,
        Value::Record(fields) => Type::Record(
            fields
                .iter()
                .map(|(name, value)| (name.clone(), value_type(value)))
                .collect(),
        ),
        Value::List(values) => Type::List(Box::new(common(values.iter().map(value_type)))),
    }
}

// A heterogeneous value remains a valid list; individual operations decide
// whether they can consume its members. Once heterogeneous, it stays unknown.
pub(crate) fn common(mut types: impl Iterator<Item = Type>) -> Type {
    types
        .try_fold(Type::Any, |a, b| unify(&a, &b))
        .unwrap_or(Type::Any)
}

fn static_name(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Name(name) | Expr::Literal(Value::Text(name)) => Some(name),
        _ => None,
    }
}

fn infer(expr: &Expr, scope: &Scope, shapes: &Shapes) -> Result<Type> {
    let (op, args) = match expr {
        Expr::Literal(value) => return Ok(value_type(value)),
        Expr::Name(name) => {
            let mut parts = name.split('.');
            let root = parts.next().unwrap_or_default();
            let mut ty = scope
                .get(root)
                .cloned()
                .ok_or_else(|| format!("unbound name {root}"))?;
            for name in parts {
                ty = field(ty, name)?;
            }
            return Ok(ty);
        }
        Expr::Call(op, args) => (op.as_str(), args),
    };
    let arg = |index: usize| {
        args.get(index)
            .ok_or_else(|| format!("{op} is missing argument {}", index + 1))
    };
    let ty = |index| infer(arg(index)?, scope, shapes);
    let list_of = |ty| Type::List(Box::new(ty));
    let expected = match op {
        "not" | "ref" | "first" | "last" | "count" | "sum" | "rows" | "unit" | "number"
        | "pairs" => 1..=1,
        "add" | "sub" | "mul" | "div" | "eq" | "ne" | "lt" | "le" | "gt" | "ge" | "get"
        | "contains" | "quantity" | "at" => 2..=2,
        "if" | "filter" | "map" | "all" | "any" | "sort" => 3..=3,
        "and" | "or" => 1..=usize::MAX,
        "list" | "concat" => 0..=usize::MAX,
        _ => return Err(format!("unknown expression operator `{op}`")),
    };
    if !expected.contains(&args.len()) {
        let count = if expected.start() == expected.end() {
            expected.start().to_string()
        } else {
            format!("at least {}", expected.start())
        };
        return Err(format!(
            "`{op}` expects {count} arguments, found {}",
            args.len()
        ));
    }
    match op {
        "rows" => row_type(
            shapes,
            static_name(arg(0)?).ok_or("rows requires a relation name")?,
        )
        .map(list_of),
        "get" => match static_name(arg(1)?) {
            Some(name) => field(ty(0)?, name),
            None => {
                expect(ty(1)?, &Type::Text)?;
                Ok(Type::Any)
            }
        },
        "ref" => field(ty(0)?, "id"),
        "filter" | "map" | "all" | "any" | "sort" => {
            let input = item(ty(0)?)?;
            let Expr::Name(name) = arg(1)? else {
                return Err("collection operator needs a binding name".into());
            };
            let mut inner = scope.clone();
            bind(&mut inner, name, input.clone())?;
            let body = infer(arg(2)?, &inner, shapes)?;
            match op {
                "map" => Ok(list_of(body)),
                "sort" => Ok(list_of(input)),
                _ => {
                    expect(body, &Type::Bool)?;
                    Ok(if op == "filter" {
                        list_of(input)
                    } else {
                        Type::Bool
                    })
                }
            }
        }
        "first" | "last" => item(ty(0)?),
        "sum" => numeric(item(ty(0)?)?),
        "at" => {
            expect(ty(1)?, &Type::Number)?;
            item(ty(0)?)
        }
        "pairs" => item(ty(0)?).map(|ty| list_of(list_of(ty))),
        "count" => {
            item(ty(0)?)?;
            Ok(Type::Number)
        }
        "list" => args
            .iter()
            .map(|arg| infer(arg, scope, shapes))
            .collect::<Result<Vec<_>>>()
            .map(|types| list_of(common(types.into_iter()))),
        "concat" => args
            .iter()
            .map(|arg| item(infer(arg, scope, shapes)?))
            .collect::<Result<Vec<_>>>()
            .map(|types| list_of(common(types.into_iter()))),
        "if" => {
            expect(ty(0)?, &Type::Bool)?;
            unify(&ty(1)?, &ty(2)?)
        }
        "and" | "or" | "not" => {
            for arg in args {
                expect(infer(arg, scope, shapes)?, &Type::Bool)?;
            }
            Ok(Type::Bool)
        }
        "contains" => {
            item(ty(0)?)?;
            ty(1)?;
            Ok(Type::Bool)
        }
        "eq" | "ne" | "lt" | "le" | "gt" | "ge" => {
            // Comparisons can intentionally inspect differently typed values;
            // the runtime carries exact unknown and dimensional outcomes.
            for arg in args {
                infer(arg, scope, shapes)?;
            }
            Ok(Type::Bool)
        }
        "quantity" => {
            expect(ty(0)?, &Type::Number)?;
            expect(ty(1)?, &Type::Text)?;
            Ok(Type::Quantity)
        }
        "number" => {
            expect(ty(0)?, &Type::Quantity)?;
            Ok(Type::Number)
        }
        "unit" => {
            expect(ty(0)?, &Type::Quantity)?;
            Ok(Type::Text)
        }
        "add" | "sub" => {
            let a = numeric(ty(0)?)?;
            let b = numeric(ty(1)?)?;
            match (&a, &b) {
                (Type::Quantity, Type::Number) | (Type::Number, Type::Quantity) => {
                    Ok(Type::Quantity)
                } // zero checked at runtime
                _ => unify(&a, &b),
            }
        }
        "mul" | "div" => {
            let a = numeric(ty(0)?)?;
            let b = numeric(ty(1)?)?;
            Ok(match (&a, &b) {
                (Type::Quantity, Type::Quantity) if op == "div" => Type::Number,
                (Type::Quantity, Type::Number) => Type::Quantity,
                (Type::Number, Type::Quantity) if op == "mul" => Type::Quantity,
                (Type::Number, Type::Number) => Type::Number,
                (Type::Any, _) | (_, Type::Any) => Type::Any,
                _ => return Err(format!("`{op}` cannot combine {a} and {b}")),
            })
        }
        _ => Err(format!("unknown operator {op}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_fields_are_not_lexical_variables() {
        let scope = BTreeMap::from([
            (
                "row".into(),
                Type::Record(BTreeMap::from([("date".into(), Type::Date)])),
            ),
            ("date".into(), Type::Bool),
        ]);
        assert_eq!(
            infer(
                &Expr::parse("(get row date)").unwrap(),
                &scope,
                &Shapes::new()
            ),
            Ok(Type::Date)
        );
        assert!(
            infer(
                &Expr::parse("row.misspelled").unwrap(),
                &scope,
                &Shapes::new()
            )
            .is_err()
        );
    }

    #[test]
    fn static_validation_rejects_wrong_arity_and_shadowing() {
        for source in [
            "(add 1)",
            "(not true false)",
            "(if true 1)",
            "(unknown 1)",
            "(map (list 1) a.b true)",
        ] {
            assert!(
                infer(&Expr::parse(source).unwrap(), &Scope::new(), &Shapes::new()).is_err(),
                "{source}"
            );
        }
        let scope = BTreeMap::from([("x".into(), Type::Number)]);
        assert!(
            infer(
                &Expr::parse("(map (list 1) x x)").unwrap(),
                &scope,
                &Shapes::new()
            )
            .is_err()
        );
    }

    #[test]
    fn incompatible_reference_targets_do_not_become_wildcards() {
        assert!(unify(&Type::Ref("a".into()), &Type::Ref("b".into())).is_err());
        assert_eq!(
            common([Type::Number, Type::Text, Type::Number].into_iter()),
            Type::Any
        );
    }
}
