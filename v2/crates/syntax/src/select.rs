//! Lot selectors: `[fifo, 2024, 2026-01..2026-06, 2026-01-22, #house]`.

use axiom_core::{Day, Diagnostic, Loc};

use crate::ast::{Name, Policy, Select};
use crate::errors::Delim;
use crate::lex::Tok;
use crate::parser::{Parse, Parser};

const POLICIES: [(&str, Policy); 4] =
    [("fifo", Policy::Fifo), ("lifo", Policy::Lifo), ("hifo", Policy::Hifo), ("prorata", Policy::Prorata)];

impl<'s> Parser<'s> {
    /// `[…]` after a place. Days, months and years become inclusive day ranges.
    pub fn selector(&mut self) -> Parse<Vec<Select<'s>>> {
        let open = self.cursor.bump().loc;
        let mut items = vec![self.select_item()?];
        while self.cursor.eat(Tok::Comma).is_some() {
            items.push(self.select_item()?);
        }
        self.close(open, Delim::Bracket)?;
        Ok(items)
    }

    fn select_item(&mut self) -> Parse<Select<'s>> {
        let token = self.cursor.peek();
        match token.tok {
            Tok::Code(text) => {
                self.cursor.bump();
                Ok(Select::Code(Name { text, loc: token.loc }))
            }
            Tok::Name(_) => {
                let (policy, loc) = self.choose(&POLICIES, "unknown-policy", "lot policy")?;
                Ok(Select::Policy(policy, loc))
            }
            _ => self.days(),
        }
    }

    /// A day, month or year, or `A..B` from the first day of one to the last of
    /// the other.
    fn days(&mut self) -> Parse<Select<'s>> {
        let (first, mut last, mut loc) = self.day_bound()?;
        if self.cursor.eat(Tok::DotDot).is_some() {
            let (_, end, end_loc) = self.day_bound()?;
            (last, loc) = (end, loc.to(end_loc));
        }
        if first > last {
            return self.fail(empty_range(loc));
        }
        Ok(Select::Range(first, last, loc))
    }

    /// The first and last day of a written date, month or year.
    fn day_bound(&mut self) -> Parse<(Day, Day, Loc)> {
        let token = self.cursor.peek();
        let bound = match token.tok {
            Tok::Date(day) => (day, day),
            Tok::Month(first) => (first, first.month_end()),
            _ => match self.year(token).and_then(|year| Day::from_ymd(year, 1, 1)) {
                Some(first) => (first, first.year_end()),
                None => {
                    return Err(self.expected(
                        "expected-selector",
                        "a lot selector: a policy, year, month, date, range or `#code`",
                    ));
                }
            },
        };
        self.cursor.bump();
        Ok((bound.0, bound.1, token.loc))
    }
}

fn empty_range(loc: Loc) -> Diagnostic {
    Diagnostic::error("empty-range", "this range ends before it starts")
        .label(loc, "no day is in it")
        .help("write the earlier bound first")
}
