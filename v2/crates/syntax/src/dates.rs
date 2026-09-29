//! Dates as written: in full, or short of what the file's place gives.
//!
//! A file in `journal/2026/03.ax` holds March 2026, so its items may be dated
//! `15`; in `journal/2026.ax` they may be dated `03-15`, and any other date in a
//! file whose place gives the year may leave the year out. The parser completes
//! them here, so the tree holds whole days. That a whole date agrees with its
//! file is for the model to check: it reports a misfiled file once, where the
//! parser could only report every line.

use axiom_core::{Day, Diagnostic, Loc};

use crate::ast::*;
use crate::lex::{Tok, Token};
use crate::malformed::not_a_date;
use crate::parser::{Parse, Parser};

impl<'s> Parser<'s> {
    /// The date an item starts with (or `opening` is followed by): a date, `MM-DD`
    /// or the bare day of the month.
    pub fn item_date(&mut self, what: &str) -> Parse<Day> {
        let token = self.peek();
        let Some(day) = self.day_of_month(token) else { return self.date(what) };
        self.bump();
        self.complete(token.loc, self.place.month, day)
    }

    /// A date, or `MM-DD`.
    pub fn date(&mut self, what: &str) -> Parse<Day> {
        let token = self.peek();
        match token.tok {
            Tok::Date(day) => Ok(self.bump_as(day)),
            Tok::MonthDay(month, day) => {
                self.bump();
                self.complete(token.loc, Some(month), day)
            }
            _ => Err(self.expected("expected-date", what)),
        }
    }

    /// The day that a short date written at `at` means in this file's place.
    fn complete(&mut self, at: Loc, month: Option<u8>, day: u8) -> Parse<Day> {
        let (Some(year), Some(month)) = (self.place.year, month) else {
            return self.fail(short_date(at, self.text(at), self.place));
        };
        let (month, day) = (u32::from(month), u32::from(day));
        Day::from_ymd(year, month, day).ok_or_else(|| self.report(not_a_date(at, self.text(at), (year, month, day))))
    }

    /// The token as a day of the month: one or two digits.
    pub fn day_of_month(&self, token: Token<'s>) -> Option<u8> {
        self.integer_name(token).filter(|digits| digits.len() <= 2)?.parse().ok()
    }

    /// `MM-DD`, a day of every year: one that exists in some year.
    pub fn month_day(&mut self) -> Parse<(u8, u8)> {
        let token = self.peek();
        let Tok::MonthDay(month, day) = token.tok else {
            return Err(self.expected("expected-day", "a month and day, like `04-15`"));
        };
        self.bump();
        // In a leap year, so that `02-29` is a day of some year.
        match Day::from_ymd(2024, month.into(), day.into()) {
            Some(_) => Ok((month, day)),
            None => self.fail(not_a_day(token.loc, self.text(token.loc))),
        }
    }

    /// A day, month or year, or `A..B` from the first day of one to the last of
    /// the other: as first and last day, and where it was written.
    pub fn days(&mut self, code: &'static str, what: &str) -> Parse<(Day, Day, Loc)> {
        let (first, mut last, mut loc) = self.day_bound(code, what)?;
        if self.eat("..").is_some() {
            let (_, end, end_loc) = self.day_bound(code, what)?;
            (last, loc) = (end, loc.to(end_loc));
        }
        if first > last {
            return self.fail(empty_range(loc, self.text(loc), first, last));
        }
        Ok((first, last, loc))
    }

    /// The first and last day of a written date, month or year.
    fn day_bound(&mut self, code: &'static str, what: &str) -> Parse<(Day, Day, Loc)> {
        let token = self.peek();
        let bound = match token.tok {
            Tok::Date(_) | Tok::MonthDay(..) => {
                let day = self.date(what)?;
                return Ok((day, day, token.loc));
            }
            Tok::Month(first) => (first, first.month_end()),
            _ => match self.year(token).and_then(|year| Day::from_ymd(year, 1, 1)) {
                Some(first) => (first, first.year_end()),
                None => return Err(self.expected(code, what)),
            },
        };
        self.bump();
        Ok((bound.0, bound.1, token.loc))
    }
}

/// A short date in a file whose place does not give the rest.
fn short_date(at: Loc, written: &str, place: Place) -> Diagnostic {
    let missing = if place.year.is_some() { "month" } else { "year" };
    Diagnostic::error("short-date", format!("`{written}` leaves out the {missing}, which this file does not give"))
        .label(at, "write the whole date here")
        .note("a date may be short only where its file's folder gives the rest: `15` is enough in `journal/2026/03.ax`")
        .help("write the date in full, like `2026-03-15`")
}

pub(crate) fn not_a_day(loc: Loc, written: &str) -> Diagnostic {
    Diagnostic::error("bad-day", format!("`{written}` is not a day of the month or year"))
        .label(loc, "no such day")
        .help("write a day of the month (`on 15`), `last`, a month and day (`on 04-15`), or a weekday (`on monday`)")
}

/// A range whose end comes before its start, with the bounds swapped as a fix.
pub(crate) fn empty_range(loc: Loc, written: &str, first: Day, last: Day) -> Diagnostic {
    let days = first.0 - last.0;
    let s = if days == 1 { "" } else { "s" };
    let message = format!("this range ends on {last}, {days} day{s} before it starts on {first}");
    let diag = Diagnostic::error("empty-range", message).label(loc, "a range runs from the earlier day to the later");
    match written.split_once("..") {
        Some((start, end)) => diag.fix("swap the bounds", loc, format!("{end}..{start}")),
        None => diag,
    }
}
