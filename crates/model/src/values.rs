//! Constants: the literal values written in properties, params and laws.

use axiom_core::{Diagnostic, Dim, Id, Loc, Qty, Ratio};
use axiom_syntax::{Bracket as WrittenBracket, Expr, ExprId, ExprKind, File, Many};

use crate::book::{Bracket, Commodity, Schedule};
use crate::declare::World;
use crate::errors::{Word, article};
use crate::law::{Ty, Value};
use crate::scope::Home;

impl Value {
    /// The static type of a constant. Runtime faults have none.
    pub fn ty(&self) -> Option<Ty> {
        Some(match self {
            Value::Empty => Ty::Empty,
            Value::Bool(_) => Ty::Bool,
            Value::Num(_) => Ty::Num,
            Value::Amount(amount) => Ty::Amount(Dim::Of(amount.unit)),
            Value::Day(_) => Ty::Day,
            Value::Span(_) => Ty::Span,
            Value::Text(_) => Ty::Text,
            Value::Name(_) => Ty::Name,
            Value::Place(_) => Ty::Place,
            Value::Entity(_) => Ty::Entity,
            Value::Kind(_) => Ty::Kind,
            Value::Unit(_) => Ty::Unit,
            Value::Purpose(..) => Ty::Purpose,
            Value::Asset(_) => Ty::Asset,
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
    wanted == found
        || matches!(
            (wanted, found),
            (Ty::Amount(Dim::Any), Ty::Amount(_)) | (Ty::Amount(_), Ty::Empty) | (Ty::Empty, Ty::Amount(_))
        )
}

/// What an expression is, for messages: `an amount`, `a date`.
pub(crate) fn describe(kind: &ExprKind) -> &'static str {
    match kind {
        ExprKind::Num(_) => "a number",
        ExprKind::Pct(_) => "a percentage",
        ExprKind::Amount(_) => "an amount",
        ExprKind::Date(_) => "a date",
        ExprKind::Span(_) => "a span",
        ExprKind::Str(_) => "text",
        ExprKind::Empty => "`empty`",
        ExprKind::Name(_) => "a name",
        ExprKind::Unit(_) => "a commodity",
        ExprKind::Code(_) => "a code",
        ExprKind::Schedule(_) => "a schedule",
        _ => "an expression",
    }
}

fn out_of_range(what: &str, loc: Loc) -> Diagnostic {
    Diagnostic::error("number-range", format!("this {what} is too large")).label(loc, "out of range")
}

impl<'s> World<'s> {
    /// The value and type of `expr` if it is a literal: a number, an amount, a
    /// date, text, a unit, a code. Other expressions are for the caller.
    pub fn literal(&mut self, home: Home, file: &File<'s>, expr: &Expr<'s>) -> Result<Option<(Value, Ty)>, Diagnostic> {
        Ok(Some(match expr.kind {
            ExprKind::Num(dec) => {
                (Value::Num(dec.to_ratio().ok_or_else(|| out_of_range("number", expr.loc))?), Ty::Num)
            }
            ExprKind::Pct(dec) => {
                let percent = Ratio::percent(dec.mantissa.into(), dec.scale);
                (Value::Num(percent.ok_or_else(|| out_of_range("percentage", expr.loc))?), Ty::Num)
            }
            ExprKind::Amount(amount) => match amount.unit() {
                Some(unit) => {
                    let unit = self.commodity_of(Word::of(file, unit.0))?;
                    (Value::Amount(self.amount(amount.num(), unit, expr.loc)?), Ty::Amount(Dim::Of(unit)))
                }
                None => (Value::Empty, Ty::Empty),
            },
            ExprKind::Date(day) => (Value::Day(day), Ty::Day),
            ExprKind::Month(day) => (Value::Day(day), Ty::Day),
            ExprKind::Fraction(top, bottom) => (
                Value::Num(
                    Ratio::new(i128::from(top), i128::from(bottom))
                        .ok_or_else(|| out_of_range("fraction", expr.loc))?,
                ),
                Ty::Num,
            ),
            ExprKind::Span(span) => (Value::Span(span), Ty::Span),
            ExprKind::Str(text) => (Value::Text(self.book.quoted_text(text.0)), Ty::Text),
            ExprKind::Empty => (Value::Empty, Ty::Empty),
            ExprKind::Unit(symbol) => {
                (Value::Unit(self.commodity_of(Word { text: symbol.0, loc: expr.loc })?), Ty::Unit)
            }
            ExprKind::Purpose(name) => {
                (Value::Purpose(self.purpose(home, Word { text: name.0, loc: expr.loc })?, None), Ty::Purpose)
            }
            ExprKind::Code(code) => {
                let sym = self.book.names.intern(code.name());
                match axiom_core::glob::is_pattern(code.name()) {
                    true => (Value::Glob(sym), Ty::Glob),
                    false => (Value::Code(sym), Ty::Code),
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
        file: &File<'s>,
        id: ExprId,
        want: Option<Ty>,
    ) -> Result<(Value, Ty), Diagnostic> {
        let expr = &file.exprs[id];
        let (value, ty) = match self.literal(home, file, expr)? {
            Some(found) => found,
            None => self.named(home, file, expr, want)?,
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
        file: &File<'s>,
        expr: &Expr<'s>,
        want: Option<Ty>,
    ) -> Result<(Value, Ty), Diagnostic> {
        let ExprKind::Name(name) = expr.kind else {
            if let ExprKind::Schedule(rows) = expr.kind {
                return Ok((Value::Schedule(self.schedule(home, file, rows, expr.loc)?), Ty::Schedule));
            }
            return Err(
                Diagnostic::error("not-constant", "expected a constant value here").label(expr.loc, "this is computed")
            );
        };
        let (text, word) = (name.0, Word { text: name.0, loc: expr.loc });
        Ok(match want {
            Some(Ty::Entity) => (Value::Entity(self.entity(home, word)?), Ty::Entity),
            Some(Ty::Place) => (Value::Place(self.place(word)?), Ty::Place),
            Some(Ty::Kind) => (Value::Kind(self.kind(home, word)?), Ty::Kind),
            Some(Ty::Purpose) => (Value::Purpose(self.purpose(home, word)?, None), Ty::Purpose),
            Some(Ty::Asset) => {
                let Some(sym) = self.book.names.get(text) else {
                    return Err(self.missing_asset(word));
                };
                let Some(&asset) = self.book.lookup.assets.get(&sym) else {
                    return Err(self.missing_asset(word));
                };
                (Value::Asset(asset), Ty::Asset)
            }
            Some(Ty::Bool) if text == "true" || text == "false" => (Value::Bool(text == "true"), Ty::Bool),
            _ => (Value::Name(self.book.names.intern(text)), Ty::Name),
        })
    }

    fn missing_asset(&self, word: Word<'_>) -> Diagnostic {
        let suggestion = axiom_core::diag::closest(
            word.text,
            self.book.assets.iter().map(|(_, asset)| self.book.names.name(asset.name)),
        );
        crate::errors::unknown("unknown-asset", "asset", word, suggestion)
    }

    /// `0 USD 10% | 12_400 USD 12% | …`: marginal brackets, ascending from zero,
    /// all in one commodity.
    pub fn schedule(
        &mut self,
        home: Home,
        file: &File<'s>,
        rows: Many<WrittenBracket>,
        loc: Loc,
    ) -> Result<Id<Schedule>, Diagnostic> {
        let mut unit = None;
        let mut brackets: Vec<Bracket> = Vec::with_capacity(rows.len());
        for row in &file[rows] {
            let (threshold, rate) = (&file.exprs[row.threshold], &file.exprs[row.rate]);
            let from = self.threshold(home, file, threshold, &mut unit)?;
            let rate = self.rate(home, file, rate)?;
            if let Some(rule) = broken_rule(&brackets, from) {
                return Err(Diagnostic::error("schedule-order", format!("a schedule's {rule}"))
                    .label(threshold.loc, "out of order here"));
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
    fn threshold(
        &mut self,
        home: Home,
        file: &File<'s>,
        from: &Expr<'s>,
        unit: &mut Option<Id<Commodity>>,
    ) -> Result<Qty, Diagnostic> {
        match self.literal(home, file, from)? {
            Some((Value::Empty, _)) => Ok(Qty::ZERO),
            Some((Value::Amount(amount), _)) => {
                if *unit.get_or_insert(amount.unit) != amount.unit {
                    let symbol = self.book.name(self.book.commodities[amount.unit].symbol);
                    return Err(Diagnostic::error("schedule-unit", "a schedule counts in one commodity")
                        .label(from.loc, format!("this is in {symbol}")));
                }
                Ok(amount.qty)
            }
            _ => Err(Diagnostic::error("schedule-threshold", "a bracket starts at an amount")
                .label(from.loc, format!("this is {}", describe(&from.kind)))),
        }
    }

    /// The marginal rate of a bracket.
    fn rate(&mut self, home: Home, file: &File<'s>, rate: &Expr<'s>) -> Result<Ratio, Diagnostic> {
        match self.literal(home, file, rate)? {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcard_amount_is_only_a_expected_type() {
        let usd = Ty::Amount(Dim::Of(Id::<Commodity>::new(3)));

        assert!(fits(Ty::AMOUNT, usd));
        assert!(!fits(usd, Ty::AMOUNT));
        assert!(fits(usd, Ty::Empty));
    }
}
