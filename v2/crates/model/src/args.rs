//! Property lines: which words the language defines, and how to read their
//! arguments.
//!
//! `opened 2026-01-01`, `budget 500 USD monthly`, `lives us/ca from 2026-01-01`.
//! Each is a name and a list of primary expressions. [`Args`] reads them one
//! at a time, in order, and says what was expected where a reader gets it wrong.

use axiom_core::diag::closest;
use axiom_core::{Day, Diagnostic, Ratio, Span};
use axiom_syntax::{Expr, ExprId, ExprKind, Exprs, Name, Prop};

use crate::scope::Home;

/// The properties the language defines itself.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Builtin {
    Owner,
    Holds,
    Select,
    Opened,
    Closed,
    Budget,
    Liquidity,
    Via,
    Lives,
    Precision,
    Title,
    Grows,
    Restricted,
    Deferred,
    Has,
}

/// What a property line describes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Target {
    Place,
    Entity,
    Commodity,
    Kind,
}

impl Target {
    pub fn noun(self) -> &'static str {
        match self {
            Target::Place => "account",
            Target::Entity => "entity",
            Target::Commodity => "commodity",
            Target::Kind => "kind",
        }
    }
}

const BUILTINS: [(Builtin, &str, &[Target]); 15] = [
    (Builtin::Owner, "owner", &[Target::Place]),
    (Builtin::Holds, "holds", &[Target::Place]),
    (Builtin::Select, "select", &[Target::Place, Target::Kind]),
    (Builtin::Opened, "opened", &[Target::Place]),
    (Builtin::Closed, "closed", &[Target::Place]),
    (Builtin::Budget, "budget", &[Target::Place]),
    (Builtin::Liquidity, "liquidity", &[Target::Place, Target::Commodity, Target::Kind]),
    (Builtin::Via, "via", &[Target::Entity]),
    (Builtin::Lives, "lives", &[Target::Entity]),
    (Builtin::Precision, "precision", &[Target::Commodity]),
    (Builtin::Title, "name", &[Target::Commodity]),
    (Builtin::Grows, "grows", &[Target::Commodity]),
    (Builtin::Restricted, "restricted", &[Target::Kind]),
    (Builtin::Deferred, "deferred", &[Target::Kind]),
    (Builtin::Has, "has", &[Target::Kind]),
];

/// Words that read a value off a thing in a law (`self.balance`), so a
/// declared property may not take them.
pub(crate) const FIELD_WORDS: [&str; 6] = ["balance", "owner", "kind", "age", "year", "month"];

impl Builtin {
    pub fn parse(word: &str) -> Option<Builtin> {
        BUILTINS.iter().find(|entry| entry.1 == word).map(|entry| entry.0)
    }

    pub fn applies_to(self, target: Target) -> bool {
        self.targets().contains(&target)
    }

    pub fn targets(self) -> &'static [Target] {
        BUILTINS.iter().find(|entry| entry.0 == self).map_or(&[], |entry| entry.2)
    }

    /// The built-in property words for `target`.
    pub fn words(target: Target) -> impl Iterator<Item = &'static str> {
        BUILTINS.iter().filter(move |entry| entry.2.contains(&target)).map(|entry| entry.1)
    }
}

/// What an expression is, for messages: `an amount`, `a date`.
pub(crate) fn describe(kind: &ExprKind) -> &'static str {
    match kind {
        ExprKind::Num(_) => "a number",
        ExprKind::Pct(_) => "a percentage",
        ExprKind::Amount(..) => "an amount",
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

/// `a, b or c`, each in backticks.
pub(crate) fn list(words: &[&str]) -> String {
    match words {
        [] => String::new(),
        [only] => format!("`{only}`"),
        [init @ .., last] => {
            let init: Vec<String> = init.iter().map(|word| format!("`{word}`")).collect();
            format!("{} or `{last}`", init.join(", "))
        }
    }
}

/// The arguments of one property line, read in order.
pub(crate) struct Args<'a, 's> {
    pub exprs: &'a Exprs<'s>,
    pub prop: &'a Prop<'s>,
    pub home: Home,
    next: usize,
}

impl<'a, 's> Args<'a, 's> {
    pub fn new(exprs: &'a Exprs<'s>, home: Home, prop: &'a Prop<'s>) -> Args<'a, 's> {
        Args { exprs, prop, home, next: 0 }
    }

    pub fn peek(&self) -> Option<&'a Expr<'s>> {
        self.prop.args.get(self.next).map(|&id| &self.exprs[id])
    }

    /// The next argument, which the property needs to be `wanted`.
    pub fn next_id(&mut self, wanted: &str) -> Result<ExprId, Diagnostic> {
        let Some(&id) = self.prop.args.get(self.next) else {
            return Err(Diagnostic::error("property-argument", format!("`{}` needs {wanted}", self.prop.name.text))
                .label(self.prop.loc, format!("{wanted} should follow here")));
        };
        self.next += 1;
        Ok(id)
    }

    pub fn take(&mut self, wanted: &str) -> Result<&'a Expr<'s>, Diagnostic> {
        self.next_id(wanted).map(|id| &self.exprs[id])
    }

    /// Leaves the rest unread: someone else has read it.
    pub fn skip(&mut self) {
        self.next = self.prop.args.len();
    }

    /// Fails if arguments remain.
    pub fn done(&self) -> Result<(), Diagnostic> {
        match self.peek() {
            None => Ok(()),
            Some(extra) => Err(Diagnostic::error(
                "property-argument",
                format!("`{}` takes no more arguments here", self.prop.name.text),
            )
            .label(extra.loc, "unexpected")),
        }
    }

    pub fn wrong(&self, expr: &Expr, wanted: &str) -> Diagnostic {
        Diagnostic::error("property-type", format!("`{}` needs {wanted}", self.prop.name.text))
            .label(expr.loc, format!("this is {}", describe(&expr.kind)))
    }

    /// One of the `allowed` words.
    pub fn word(&mut self, allowed: &[&str]) -> Result<&'s str, Diagnostic> {
        let wanted = if allowed.len() == 1 { format!("`{}`", allowed[0]) } else { format!("one of {}", list(allowed)) };
        let expr = self.take(&wanted)?;
        match expr.kind {
            ExprKind::Name(text) if allowed.contains(&text) => Ok(text),
            ExprKind::Name(text) => {
                let mut diagnostic = self.wrong(expr, &wanted).label(expr.loc, format!("`{text}` is not one of them"));
                if let Some(near) = closest(text, allowed.iter().copied()) {
                    diagnostic = diagnostic.fix(format!("did you mean `{near}`?"), expr.loc, near);
                }
                Err(diagnostic)
            }
            _ => Err(self.wrong(expr, &wanted)),
        }
    }

    /// A name, as written.
    pub fn name(&mut self, wanted: &str) -> Result<Name<'s>, Diagnostic> {
        let expr = self.take(wanted)?;
        match expr.kind {
            ExprKind::Name(text) => Ok(Name { text, loc: expr.loc }),
            _ => Err(self.wrong(expr, wanted)),
        }
    }

    pub fn day(&mut self) -> Result<Day, Diagnostic> {
        let expr = self.take("a date")?;
        match expr.kind {
            ExprKind::Date(day) => Ok(day),
            _ => Err(self.wrong(expr, "a date")),
        }
    }

    pub fn span(&mut self) -> Result<Span, Diagnostic> {
        const WANTED: &str = "a span such as `5d` or `1y6m`";
        let expr = self.take(WANTED)?;
        match expr.kind {
            ExprKind::Span(span) => Ok(span),
            _ => Err(self.wrong(expr, WANTED)),
        }
    }

    /// A whole number from zero to `max`.
    pub fn count(&mut self, max: u8) -> Result<u8, Diagnostic> {
        let wanted = format!("a whole number up to {max}");
        let expr = self.take(&wanted)?;
        let whole = match expr.kind {
            ExprKind::Num(number) => number.to_qty(0).ok().and_then(|qty| u8::try_from(qty.0).ok()),
            _ => None,
        };
        whole.filter(|&count| count <= max).ok_or_else(|| self.wrong(expr, &wanted))
    }

    pub fn percent(&mut self) -> Result<Ratio, Diagnostic> {
        const WANTED: &str = "a percentage such as `5%`";
        let expr = self.take(WANTED)?;
        match expr.kind {
            ExprKind::Pct(number) => Ratio::percent(number.mantissa.into(), number.scale).ok_or_else(|| {
                Diagnostic::error("number-range", "this percentage is too large").label(expr.loc, "out of range")
            }),
            _ => Err(self.wrong(expr, WANTED)),
        }
    }

    pub fn text(&mut self) -> Result<&'s str, Diagnostic> {
        let expr = self.take("text in quotes")?;
        match expr.kind {
            ExprKind::Str(text) => Ok(text),
            _ => Err(self.wrong(expr, "text in quotes")),
        }
    }
}
