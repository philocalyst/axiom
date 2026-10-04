//! The values of a property line, read in order: one reader for every line of the language that is a word and its
//! values (`holds USD`, `grace 5d`, `loan 250_000 USD on 2026-01-01 at 5% over 30y`, `input hours HR`), and one way of
//! saying what is wrong with one.
//!
//! A line that is wrong says which property, where (the value it points at, or where one should follow), what the line
//! needs there and what it found; a line with a form (`loan AMOUNT on DATE at RATE over SPAN [for ASSET]`) shows it as
//! the help, so the forms are also the documentation. A value missing, extra or out of place is said with the line's
//! shape code; a value of the wrong kind or out of range with the code of what it is (`contract-loan-date`), so a
//! mistake keeps the code it has always had.

use axiom_core::{Day, Diagnostic, Id, Loc, Ratio, Span};
use axiom_syntax::{Expr, ExprId, ExprKind, File, Prop as Line};

use crate::book::{Amount, Asset, Commodity, Entity, Place, Purpose, System};
use crate::declare::World;
use crate::errors::{Word, list, suggest};
use crate::scope::Home;
use crate::values::describe;

/// The values of one property line, and how its mistakes are said.
pub(crate) struct Args<'a, 's> {
    pub file: &'a File<'s>,
    pub line: &'a Line<'s>,
    ids: &'a [ExprId],
    next: usize,
    /// The code of a value missing, extra or not the word the line takes there.
    shape: &'static str,
    /// The code of a value of the wrong kind or out of range.
    wrong: &'static str,
    /// The line as it is written when it is right: the help of every mistake.
    form: Option<&'static str>,
}

impl<'a, 's> Args<'a, 's> {
    /// A built-in property's values: a value missing or extra is `property-argument`, of the wrong kind
    /// `property-type`.
    pub fn of(file: &'a File<'s>, line: &'a Line<'s>) -> Args<'a, 's> {
        let ids = &file[line.args];
        Args { file, line, ids, next: 0, shape: "property-argument", wrong: "property-type", form: None }
    }

    /// A line of the form `form`, whose mistakes are said with `code` unless a value says otherwise.
    pub fn shaped(file: &'a File<'s>, line: &'a Line<'s>, code: &'static str, form: &'static str) -> Args<'a, 's> {
        Args { shape: code, wrong: code, form: Some(form), ..Args::of(file, line) }
    }

    /// Reads with `read`, saying a value of the wrong kind or out of range with `code`.
    pub fn with<T>(
        &mut self,
        code: &'static str,
        read: impl FnOnce(&mut Self) -> Result<T, Diagnostic>,
    ) -> Result<T, Diagnostic> {
        let wrong = std::mem::replace(&mut self.wrong, code);
        let read = read(self);
        self.wrong = wrong;
        read
    }

    fn said(&self, code: &'static str, message: String) -> Diagnostic {
        let problem = Diagnostic::error(code, message);
        match self.form {
            Some(form) => problem.help(format!("write `{form}`")),
            None => problem,
        }
    }

    pub fn peek(&self) -> Option<&'a Expr<'s>> {
        self.ids.get(self.next).map(|&id| &self.file.exprs[id])
    }

    /// The next value, which the line needs to be `wanted`.
    pub fn next_id(&mut self, wanted: &str) -> Result<ExprId, Diagnostic> {
        let Some(&id) = self.ids.get(self.next) else {
            let problem = self.said(self.shape, format!("`{}` needs {wanted}", self.line.name.0));
            return Err(problem.label(self.line.loc, format!("{wanted} should follow here")));
        };
        self.next += 1;
        Ok(id)
    }

    /// `expr` is not what the line needs there.
    pub fn wrong(&self, expr: &Expr, wanted: &str) -> Diagnostic {
        let problem = self.said(self.wrong, format!("`{}` needs {wanted}", self.line.name.0));
        problem.label(expr.loc, format!("this is {}", describe(&expr.kind)))
    }

    /// The value at `loc` is of the kind the line needs and not `wanted`: out of range.
    pub fn refuse(&self, loc: Loc, wanted: &str) -> Diagnostic {
        self.refuse_as(self.wrong, loc, wanted)
    }

    /// The value at `loc` is out of range, said with `code` rather than the line's.
    pub fn refuse_as(&self, code: &'static str, loc: Loc, wanted: &str) -> Diagnostic {
        self.said(code, format!("`{}` needs {wanted}", self.line.name.0)).label(loc, format!("this is not {wanted}"))
    }

    /// `what`, at `loc`, is written a second time where the line takes it once.
    pub fn twice(&self, loc: Loc, what: &str) -> Diagnostic {
        self.said(self.wrong, format!("`{}` takes `{what}` once", self.line.name.0)).label(loc, "written again here")
    }

    /// The line has no lines under it.
    pub fn flat(&self) -> Result<(), Diagnostic> {
        match self.file[self.line.lines].first() {
            None => Ok(()),
            Some(nested) => {
                let problem = self.said(self.shape, format!("`{}` takes no lines under it", self.line.name.0));
                Err(problem.label(nested.0.loc, "unexpected"))
            }
        }
    }

    /// The next value, as `pick` reads it out of the expression.
    pub fn arg<T>(&mut self, wanted: &str, pick: impl FnOnce(&Expr<'s>) -> Option<T>) -> Result<T, Diagnostic> {
        let expr = &self.file.exprs[self.next_id(wanted)?];
        pick(expr).ok_or_else(|| self.wrong(expr, wanted))
    }

    /// Nothing is left to read.
    pub fn done(&self) -> Result<(), Diagnostic> {
        let Some(extra) = self.peek() else {
            return Ok(());
        };
        let problem = self.said(self.shape, format!("`{}` takes no more arguments here", self.line.name.0));
        Err(problem.label(extra.loc, "unexpected"))
    }

    /// One of the `allowed` words; one the line does not take is said with the shape's code when the line has a form.
    pub fn word(&mut self, allowed: &[&str]) -> Result<&'s str, Diagnostic> {
        let wanted = if allowed.len() == 1 { format!("`{}`", allowed[0]) } else { format!("one of {}", list(allowed)) };
        let expr = &self.file.exprs[self.next_id(&wanted)?];
        match expr.kind {
            ExprKind::Name(name) if allowed.contains(&name.0) => Ok(name.0),
            ExprKind::Name(name) => {
                let error = self.wrong(expr, &wanted).label(expr.loc, format!("`{}` is not one of them", name.0));
                Err(suggest(error, expr.loc, name.0, allowed.iter().copied()))
            }
            _ => Err(self.wrong(expr, &wanted)),
        }
    }

    /// Whether the next value is the word `word`, read if it is.
    pub fn takes(&mut self, word: &str) -> bool {
        let next = matches!(self.peek().map(|expr| &expr.kind), Some(ExprKind::Name(name)) if name.0 == word);
        self.next += usize::from(next);
        next
    }

    pub fn name(&mut self, wanted: &str) -> Result<Word<'s>, Diagnostic> {
        let name = |expr: &Expr<'s>| match expr.kind {
            ExprKind::Name(name) => Some(Word { text: name.0, loc: expr.loc }),
            _ => None,
        };
        self.arg(wanted, name)
    }

    pub fn day(&mut self) -> Result<Day, Diagnostic> {
        self.arg("a date", |expr| if let ExprKind::Date(day) = expr.kind { Some(day) } else { None })
    }

    pub fn span(&mut self) -> Result<Span, Diagnostic> {
        let span = |expr: &Expr| {
            if let ExprKind::Span(span) = expr.kind { Some(span) } else { None }
        };
        self.arg("a span such as `5d` or `1y6m`", span)
    }

    /// A span of some days or months and none negative, and where it is written.
    pub fn positive_span(&mut self) -> Result<Span, Diagnostic> {
        let loc = self.peek().map(|expr| expr.loc);
        let span = self.span()?;
        match positive(span) {
            true => Ok(span),
            false => Err(self.refuse(loc.unwrap_or(self.line.loc), "a positive span")),
        }
    }

    pub fn text(&mut self) -> Result<&'s str, Diagnostic> {
        self.arg("text in quotes", |expr| if let ExprKind::Str(text) = expr.kind { Some(text.0) } else { None })
    }

    /// A whole number from zero to `max`.
    pub fn count(&mut self, max: u8) -> Result<u8, Diagnostic> {
        self.arg(&format!("a whole number up to {max}"), |expr| match expr.kind {
            ExprKind::Num(number) => {
                number.to_qty(0).ok().and_then(|qty| u8::try_from(qty.0).ok()).filter(|&n| n <= max)
            }
            _ => None,
        })
    }

    pub fn percent(&mut self) -> Result<Ratio, Diagnostic> {
        let percent = |expr: &Expr| match expr.kind {
            ExprKind::Pct(number) => Ratio::percent(number.mantissa.into(), number.scale),
            _ => None,
        };
        self.arg("a percentage such as `5%`", percent)
    }

    /// A percentage that is not negative.
    pub fn rate(&mut self) -> Result<Ratio, Diagnostic> {
        let loc = self.peek().map_or(self.line.loc, |expr| expr.loc);
        let rate = self.percent()?;
        match rate.is_negative() {
            false => Ok(rate),
            true => Err(self.refuse(loc, "a percentage of zero or more")),
        }
    }

    /// A literal amount (`3_000 USD`) in its unit, else in `fallback`, and where it is written.
    pub fn amount(&mut self, world: &World<'s>, fallback: Id<Commodity>) -> Result<(Amount, Loc), Diagnostic> {
        let literal = |expr: &Expr<'s>| match expr.kind {
            ExprKind::Amount(literal) => Some((literal, expr.loc)),
            _ => None,
        };
        let (literal, loc) = self.arg("an amount such as `3_000 USD`", literal)?;
        Ok((world.literal_amount(self.file, literal, Some(fallback))?, loc))
    }

    /// An amount above zero.
    pub fn positive_amount(&mut self, world: &World<'s>, fallback: Id<Commodity>) -> Result<Amount, Diagnostic> {
        let (amount, loc) = self.amount(world, fallback)?;
        match amount.qty.0 > 0 {
            true => Ok(amount),
            false => Err(self.refuse(loc, "an amount above zero")),
        }
    }

    pub fn entity(&mut self, world: &World<'s>, home: Home) -> Result<Id<Entity>, Diagnostic> {
        let word = self.name("an entity")?;
        world.entity(home, word)
    }

    pub fn place(&mut self, world: &World<'s>) -> Result<Id<Place>, Diagnostic> {
        let word = self.name("a place")?;
        world.place(word)
    }

    pub fn system(&mut self, world: &World<'s>) -> Result<Id<System>, Diagnostic> {
        let word = self.name("a system")?;
        world.system(word)
    }

    /// A purpose, and where it is written.
    pub fn purpose(&mut self, world: &World<'s>, home: Home) -> Result<(Id<Purpose>, Loc), Diagnostic> {
        let word = self.name("a purpose")?;
        Ok((world.purpose(home, word)?, word.loc))
    }

    /// A declared asset, and where it is named.
    pub fn asset(&mut self, world: &World<'s>) -> Result<(Id<Asset>, Loc), Diagnostic> {
        let word = self.name("an asset")?;
        match world.book.asset(word.text) {
            Some(asset) => Ok((asset, word.loc)),
            None => Err(world.missing_asset(word)),
        }
    }

    /// One commodity, as a book writes it: a unit, `USD`.
    pub fn unit(&mut self, world: &World<'s>, wanted: &str) -> Result<Id<Commodity>, Diagnostic> {
        let unit = |expr: &Expr<'s>| match expr.kind {
            ExprKind::Unit(symbol) => Some(Word { text: symbol.0, loc: expr.loc }),
            _ => None,
        };
        let word = self.arg(wanted, unit)?;
        world.commodity_of(word)
    }
}

/// Some days or months, and none negative.
fn positive(span: Span) -> bool {
    span.months >= 0 && span.days >= 0 && (span.months > 0 || span.days > 0)
}

#[cfg(test)]
mod tests {
    use axiom_core::Span;

    use super::positive;

    #[test]
    fn a_positive_span_has_no_negative_part_and_is_not_empty() {
        assert!(positive(Span::months(360)));
        assert!(positive(Span::days(30)));
        assert!(!positive(Span::default()));
        assert!(!positive(Span { months: 1, days: -2 }));
        assert!(!positive(Span { months: -1, days: 32 }));
    }
}
