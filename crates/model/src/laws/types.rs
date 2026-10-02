//! The typing rules of law expressions: which operands an operator accepts,
//! and what it says when they do not fit.
//!
//! These are functions of types only; they know nothing of names or nodes.
//! The engine implements exactly the combinations accepted here.

use axiom_core::{Diagnostic, Dim, Loc};
use axiom_syntax::BinOp;

use crate::errors::article;
use crate::law::Ty;

/// The type two branches or operands share. `empty` is the zero of every
/// amount, so it joins an amount.
pub(crate) fn unify(a: Ty, b: Ty) -> Option<Ty> {
    match (a, b) {
        _ if a == b => Some(a),
        (amount @ Ty::Amount(_), Ty::Empty) | (Ty::Empty, amount @ Ty::Amount(_)) => Some(amount),
        _ => None,
    }
}

pub(crate) fn is_amount(ty: Ty) -> bool {
    matches!(ty, Ty::Amount(_) | Ty::Empty)
}

fn is_ordered(ty: Ty) -> bool {
    matches!(ty, Ty::Amount(_) | Ty::Empty | Ty::Num | Ty::Day | Ty::Span)
}

/// What `left op right` is, if the operator accepts those operands.
pub(crate) fn binary(op: BinOp, left: Ty, right: Ty) -> Option<Ty> {
    use BinOp::*;
    match op {
        Or | And => (left == Ty::Bool && right == Ty::Bool).then_some(Ty::Bool),
        Eq | Ne => unify(left, right).map(|_| Ty::Bool),
        Lt | Le | Gt | Ge => unify(left, right).filter(|&shared| is_ordered(shared)).map(|_| Ty::Bool),
        UpTo => unify(left, right).filter(|&shared| is_ordered(shared)),
        Add | Sub => match (left, right) {
            (Ty::Num, Ty::Num) => Some(Ty::Num),
            (Ty::Span, Ty::Span) => Some(Ty::Span),
            (Ty::Day, Ty::Span) => Some(Ty::Day),
            (Ty::Day, Ty::Day) if op == Sub => Some(Ty::Span),
            (a, b) if is_amount(a) && is_amount(b) => unify(a, b),
            _ => None,
        },
        Mul => match (left, right) {
            (Ty::Empty, Ty::Num | Ty::Amount(Dim::Number)) | (Ty::Num | Ty::Amount(Dim::Number), Ty::Empty) => {
                Some(Ty::Empty)
            }
            _ => combine(left, right, Dim::mul),
        },
        Div => match (left, right) {
            (Ty::Empty, Ty::Num | Ty::Amount(Dim::Number)) => Some(Ty::Empty),
            _ => combine(left, right, Dim::div),
        },
    }
}

/// Combine numeric and dimensional amount types with the same unit algebra as
/// literals and params. A pure number remains the dedicated `Ty::Num` type.
fn combine(
    left: Ty,
    right: Ty,
    op: fn(
        Dim<axiom_core::Id<crate::book::Commodity>>,
        Dim<axiom_core::Id<crate::book::Commodity>>,
    ) -> Option<Dim<axiom_core::Id<crate::book::Commodity>>>,
) -> Option<Ty> {
    let dimension = |ty| match ty {
        Ty::Num => Some(Dim::Number),
        Ty::Amount(dim) => Some(dim),
        _ => None,
    };
    let result = op(dimension(left)?, dimension(right)?)?;
    Some(match result {
        Dim::Number => Ty::Num,
        dim => Ty::Amount(dim),
    })
}

/// Whether a `-x` is allowed, and what it is.
pub(crate) fn negate(ty: Ty) -> Option<Ty> {
    matches!(ty, Ty::Amount(_) | Ty::Empty | Ty::Num).then_some(ty)
}

/// Whether `left is right` can be asked: a place, entity or commodity against a
/// kind, place, entity or pattern; a kind against a kind; a flow against a code.
pub(crate) fn is_test(left: Ty, alternative: Ty) -> bool {
    match left {
        Ty::Place | Ty::Entity | Ty::Unit => {
            matches!(alternative, Ty::Kind | Ty::Place | Ty::Entity | Ty::Glob | Ty::Unit)
        }
        Ty::Kind => alternative == Ty::Kind,
        Ty::Flow => matches!(alternative, Ty::Code | Ty::Glob),
        Ty::Purpose => alternative == Ty::Purpose,
        _ => false,
    }
}

/// `an amount`, `a date`.
fn a(ty: Ty) -> String {
    article(ty.word())
}

/// Why `left op right` does not type.
pub(crate) fn mismatch(op: BinOp, left: (Ty, Loc), right: (Ty, Loc)) -> Diagnostic {
    let (l, r) = (a(left.0), a(right.0));
    let (message, rule) = match op {
        BinOp::Add => {
            (format!("cannot add {r} to {l}"), "`+` adds two amounts, two numbers or two spans, or a span to a date")
        }
        BinOp::Sub => (
            format!("cannot subtract {r} from {l}"),
            "`-` subtracts two amounts, two numbers or two spans, a span from a date, or a date from a date",
        ),
        BinOp::Mul => (format!("cannot multiply {l} by {r}"), "`*` multiplies two numbers, or an amount by a number"),
        BinOp::Div => (
            format!("cannot divide {l} by {r}"),
            "`/` divides an amount by a number, or by another amount to give a number",
        ),
        BinOp::Eq | BinOp::Ne => (
            format!("cannot compare {l} with {r}"),
            "`==` and `!=` compare two values of one type; `empty` is the zero of any amount",
        ),
        BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
            (format!("cannot compare {l} with {r}"), "`<` and its kin compare two amounts, numbers, dates or spans")
        }
        BinOp::UpTo => (format!("cannot cap {l} with {r}"), "`up to` needs two ordered values of the same unit"),
        BinOp::And | BinOp::Or => (
            format!(
                "`{}` needs true-or-false values, but this side is {}",
                op.symbol(),
                if left.0 == Ty::Bool { &r } else { &l }
            ),
            "`and` and `or` join conditions, such as `amount > 0 USD`",
        ),
    };
    Diagnostic::error("type-mismatch", message)
        .label(right.1, format!("this is {r}"))
        .context(left.1, format!("this is {l}"))
        .note(rule)
}

/// `expected a condition, but this is an amount`
pub(crate) fn expected(what: &str, found: Ty, loc: Loc) -> Diagnostic {
    Diagnostic::error("type-mismatch", format!("expected {what}, but this is {}", a(found)))
        .label(loc, format!("this is {}", a(found)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic_follows_the_rules_of_amounts() {
        assert_eq!(binary(BinOp::Add, Ty::AMOUNT, Ty::Empty), Some(Ty::AMOUNT));
        assert_eq!(binary(BinOp::Mul, Ty::Num, Ty::AMOUNT), Some(Ty::AMOUNT));
        assert_eq!(binary(BinOp::Div, Ty::AMOUNT, Ty::AMOUNT), None);
        let usd = Ty::Amount(Dim::Of(axiom_core::Id::new(0)));
        assert_eq!(binary(BinOp::Div, usd, usd), Some(Ty::Num));
        assert_eq!(binary(BinOp::Sub, Ty::Day, Ty::Day), Some(Ty::Span));
        assert_eq!(binary(BinOp::Add, Ty::Day, Ty::AMOUNT), None);
        assert_eq!(binary(BinOp::Mul, Ty::AMOUNT, Ty::AMOUNT), None);
        assert_eq!(binary(BinOp::Lt, Ty::AMOUNT, Ty::Empty), Some(Ty::Bool));
        assert_eq!(binary(BinOp::Lt, Ty::Place, Ty::Place), None);
    }

    #[test]
    fn a_dynamic_amount_does_not_silently_adopt_a_concrete_unit() {
        type D = Dim<axiom_core::Id<crate::book::Commodity>>;
        let usd = Ty::Amount(D::Of(axiom_core::Id::new(0)));
        let mile = Ty::Amount(D::Of(axiom_core::Id::new(1)));

        assert_eq!(unify(Ty::AMOUNT, usd), None);
        assert_eq!(binary(BinOp::Le, Ty::AMOUNT, usd), None);
        assert_eq!(binary(BinOp::UpTo, usd, Ty::AMOUNT), None);
        assert_eq!(binary(BinOp::Add, Ty::AMOUNT, usd), None);
        assert_eq!(binary(BinOp::Add, usd, mile), None);
    }

    #[test]
    fn arithmetic_tracks_compound_dimensions() {
        type D = Dim<axiom_core::Id<crate::book::Commodity>>;
        // Id is opaque, but its value is never read by the dimension algebra.
        let usd = D::Of(axiom_core::Id::new(0));
        let mile = D::Of(axiom_core::Id::new(1));
        let per_mile = D::Per(axiom_core::Id::new(0), axiom_core::Id::new(1));
        assert_eq!(binary(BinOp::Mul, Ty::Amount(per_mile), Ty::Amount(mile)), Some(Ty::Amount(usd)));
        assert_eq!(binary(BinOp::Div, Ty::Amount(usd), Ty::Amount(per_mile)), Some(Ty::Amount(mile)));
        assert_eq!(binary(BinOp::Add, Ty::Amount(usd), Ty::Amount(mile)), None);
        assert_eq!(binary(BinOp::UpTo, Ty::Amount(usd), Ty::Amount(mile)), None);
        assert_eq!(binary(BinOp::UpTo, Ty::Amount(usd), Ty::Amount(usd)), Some(Ty::Amount(usd)));
    }
}
