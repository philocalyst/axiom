//! Constants: the literal values written in properties, params and laws.

use axiom_core::{Diagnostic, Id, Loc, Qty, Ratio};
use axiom_syntax::{Expr, ExprId, ExprKind, Exprs, Name};

use crate::args::describe;
use crate::book::{Bracket, Commodity, Schedule};
use crate::errors::article;
use crate::law::{Ty, Value};
use crate::scope::Home;
use crate::survey::code_text;
use crate::world::World;

impl Value {
    /// The static type of a constant. Runtime faults have none.
    pub fn ty(&self) -> Option<Ty> {
        Some(match self {
            Value::Empty => Ty::Empty,
            Value::Bool(_) => Ty::Bool,
            Value::Num(_) => Ty::Num,
            Value::Amount(_) => Ty::Amount,
            Value::Day(_) => Ty::Day,
            Value::Span(_) => Ty::Span,
            Value::Text(_) => Ty::Text,
            Value::Name(_) => Ty::Name,
            Value::Place(_) => Ty::Place,
            Value::Entity(_) => Ty::Entity,
            Value::Kind(_) => Ty::Kind,
            Value::Unit(_) => Ty::Unit,
            Value::Schedule(_) => Ty::Schedule,
            Value::Code(_) => Ty::Code,
            Value::Glob(_) => Ty::Glob,
            Value::Flow => Ty::Flow,
            Value::Fault(_) => return None,
        })
    }
}

/// Whether a value of type `found` may stand where `wanted` is expected:
/// equal types, and `empty`, which is the zero of every amount.
pub(crate) fn fits(wanted: Ty, found: Ty) -> bool {
    wanted == found || matches!((wanted, found), (Ty::Amount, Ty::Empty) | (Ty::Empty, Ty::Amount))
}

impl<'s> World<'s> {
    /// The value and type of `expr` if it is a literal: a number, an amount, a
    /// date, text, a unit, a code. Other expressions are for the caller.
    pub fn literal(&mut self, expr: &Expr<'s>) -> Result<Option<(Value, Ty)>, Diagnostic> {
        let ratio = |dec: axiom_core::Dec| {
            dec.to_ratio().ok_or_else(|| {
                Diagnostic::error("number-range", "this number is too large").label(expr.loc, "out of range")
            })
        };
        Ok(Some(match expr.kind {
            ExprKind::Num(dec) => (Value::Num(ratio(dec)?), Ty::Num),
            ExprKind::Pct(dec) => {
                let percent = Ratio::percent(dec.mantissa.into(), dec.scale);
                let percent = percent.ok_or_else(|| {
                    Diagnostic::error("number-range", "this percentage is too large").label(expr.loc, "out of range")
                })?;
                (Value::Num(percent), Ty::Num)
            }
            ExprKind::Amount(dec, unit) => (Value::Amount(self.amount(dec, unit, expr.loc)?), Ty::Amount),
            ExprKind::Date(day) => (Value::Day(day), Ty::Day),
            ExprKind::Span(span) => (Value::Span(span), Ty::Span),
            ExprKind::Str(text) => (Value::Text(self.book.names.intern(text)), Ty::Text),
            ExprKind::Empty => (Value::Empty, Ty::Empty),
            ExprKind::Unit(symbol) => {
                let unit = self.commodity(Name { text: symbol, loc: expr.loc })?;
                (Value::Unit(unit), Ty::Unit)
            }
            ExprKind::Code(written) => {
                let text = code_text(written);
                let sym = self.book.names.intern(text);
                if axiom_core::glob::is_pattern(text) {
                    (Value::Glob(sym), Ty::Glob)
                } else {
                    (Value::Code(sym), Ty::Code)
                }
            }
            _ => return Ok(None),
        }))
    }

    /// The value and type of `id`, a constant. With `want`, it must fit that
    /// type, and a name is read as the kind of thing `want` says: an entity, a
    /// place, a kind, or a plain word.
    pub fn constant(
        &mut self,
        home: Home,
        exprs: &Exprs<'s>,
        id: ExprId,
        want: Option<Ty>,
    ) -> Result<(Value, Ty), Diagnostic> {
        let expr = &exprs[id];
        let (value, ty) = match self.literal(expr)? {
            Some(found) => found,
            None => self.named(home, exprs, expr, want)?,
        };
        match want {
            Some(want) if !fits(want, ty) => Err(Diagnostic::error(
                "type-mismatch",
                format!("expected {}, but this is {}", article(want.word()), describe(&expr.kind)),
            )
            .label(expr.loc, format!("this is {}", describe(&expr.kind)))),
            _ => Ok((value, ty)),
        }
    }

    fn named(
        &mut self,
        home: Home,
        exprs: &Exprs<'s>,
        expr: &Expr<'s>,
        want: Option<Ty>,
    ) -> Result<(Value, Ty), Diagnostic> {
        let ExprKind::Name(text) = expr.kind else {
            if let ExprKind::Schedule(rows) = &expr.kind {
                return Ok((Value::Schedule(self.schedule(exprs, rows, expr.loc)?), Ty::Schedule));
            }
            return Err(
                Diagnostic::error("not-constant", "expected a constant value here").label(expr.loc, "this is computed")
            );
        };
        let name = Name { text, loc: expr.loc };
        Ok(match want {
            Some(Ty::Entity) => (Value::Entity(self.entity(home, name)?), Ty::Entity),
            Some(Ty::Place) => (Value::Place(self.place(name)?), Ty::Place),
            Some(Ty::Kind) => (Value::Kind(self.kind(home, name)?), Ty::Kind),
            Some(Ty::Bool) if text == "true" || text == "false" => (Value::Bool(text == "true"), Ty::Bool),
            _ => (Value::Name(self.book.names.intern(text)), Ty::Name),
        })
    }

    /// `0 USD 10% | 12_400 USD 12% | …`: marginal brackets, ascending from zero,
    /// all in one commodity.
    pub fn schedule(
        &mut self,
        exprs: &Exprs<'s>,
        rows: &[(ExprId, ExprId)],
        loc: Loc,
    ) -> Result<Id<Schedule>, Diagnostic> {
        let mut unit = None;
        let mut brackets: Vec<Bracket> = Vec::with_capacity(rows.len());
        for &(threshold, rate) in rows {
            let from = self.threshold(&exprs[threshold], &mut unit)?;
            let rate = self.rate(&exprs[rate])?;
            if let Some(rule) = broken_rule(&brackets, from) {
                return Err(Diagnostic::error("schedule-order", format!("a schedule's {rule}"))
                    .label(exprs[threshold].loc, "out of order here"));
            }
            brackets.push(Bracket { from, rate });
        }
        let Some(unit) = unit else {
            return Err(Diagnostic::error("schedule-unit", "a schedule needs a commodity")
                .label(loc, "write the thresholds as amounts"));
        };
        Ok(self.book.schedules.push(Schedule { unit, brackets: brackets.into() }))
    }

    /// Where a bracket starts: `empty`, or an amount in the schedule's commodity.
    fn threshold(&mut self, from: &Expr<'s>, unit: &mut Option<Id<Commodity>>) -> Result<Qty, Diagnostic> {
        match from.kind {
            ExprKind::Empty => Ok(Qty::ZERO),
            ExprKind::Amount(dec, name) => {
                let amount = self.amount(dec, name, from.loc)?;
                if *unit.get_or_insert(amount.unit) != amount.unit {
                    return Err(Diagnostic::error("schedule-unit", "a schedule counts in one commodity")
                        .label(from.loc, format!("this is in {}", name.text)));
                }
                Ok(amount.qty)
            }
            _ => Err(Diagnostic::error("schedule-threshold", "a bracket starts at an amount")
                .label(from.loc, format!("this is {}", describe(&from.kind)))),
        }
    }

    /// The marginal rate of a bracket.
    fn rate(&mut self, rate: &Expr<'s>) -> Result<Ratio, Diagnostic> {
        match self.literal(rate)? {
            Some((Value::Num(ratio), _)) => Ok(ratio),
            _ => Err(Diagnostic::error("schedule-rate", "a bracket's rate is a percentage")
                .label(rate.loc, format!("this is {}", describe(&rate.kind)))),
        }
    }
}

/// The rule a bracket starting at `from` would break, if it breaks one.
fn broken_rule(before: &[Bracket], from: Qty) -> Option<&'static str> {
    match before.last() {
        None if from != Qty::ZERO => Some("first bracket starts at zero"),
        Some(last) if from <= last.from => Some("brackets ascend"),
        _ => None,
    }
}
