//! Dates as written: in full, or short of what their context gives.
//!
//! The context of a date is the nearest heading above it (a line holding only
//! `2026` or `2026-03`), else the file's folder: a file in `journal/2026/03.ax`
//! holds March 2026, so its items may be dated `15`; in `journal/2026.ax` they
//! may be dated `03-15`, and any other date in a file whose context gives the
//! year may leave the year out. The parser completes them here, so the tree
//! holds whole days. Nothing checks that a whole date agrees with its context:
//! where a file is kept is a convention, never a law.
//!
//! A short `until` or `due` date needs no context: it is the first such day on
//! or after the day of the line it is written on.
//!
//! What a place gives is [`Folder`], and whoever writes dates into these files
//! asks it, so that no one else decides what a short date means: `heading`
//! reads a heading line, `complete` a date as the parser would, and `shorten`
//! writes one as briefly as the place allows.

use axiom_core::{Day, Diagnostic, FileId, Loc};

use crate::ast::*;
use crate::lex::{Lexer, Punct, Tok, Token};
use crate::malformed::not_a_date;
use crate::parser::{Parse, Parser};

/// The names of the months: the one table of them.
#[rustfmt::skip]
pub const MONTHS: [&str; 12] = [
    "January", "February", "March", "April", "May", "June",
    "July", "August", "September", "October", "November", "December",
];

impl Folder {
    /// The context a heading line gives the lines below it: `2026` is a year and
    /// `2026-03` a month of it. `line` starts at the heading's first byte and may
    /// run on, so that this reads exactly what a pre-scan of a piece finds and no
    /// more; it is `None` for every other line.
    pub fn heading(line: &[u8]) -> Option<Folder> {
        let digits = |from: usize, count: usize| {
            let text = line.get(from..from + count)?;
            text.iter().all(u8::is_ascii_digit).then(|| text.iter().fold(0, |sum, &b| sum * 10 + i32::from(b - b'0')))
        };
        let year = digits(0, 4)?;
        let (month, end) = match line.get(4) {
            Some(b'-') => (Some(digits(5, 2)?), 7),
            _ => (None, 4),
        };
        // Nothing may follow but blanks and a comment, which needs a blank before it.
        let blanks = line[end..].iter().take_while(|&&b| matches!(b, b' ' | b'\t')).count();
        match line[end + blanks..] {
            [] | [b'\n' | b'\r', ..] => {}
            [b'/', b'/', ..] if blanks > 0 => {}
            _ => return None,
        }
        Day::from_ymd(year, month.unwrap_or(1) as u32, 1)?;
        Some(Folder { year: Some(year), month: month.map(|month| month as u8) })
    }

    /// The day a written date means here, read as the parser reads it: `2026-01-15`
    /// whole, `01-15` where the year is known, `15` where the month is too.
    pub fn complete(self, written: &str) -> Option<Day> {
        let mut lexer = Lexer::new(written, FileId::default());
        lexer.load(0, written.len());
        match (lexer.bump().tok, lexer.peek().tok) {
            (Tok::Date(day), Tok::Eol) => Some(day),
            (Tok::MonthDay(month, day), Tok::Eol) => Day::from_ymd(self.year?, month.into(), day.into()),
            (Tok::Number(_), Tok::Eol) if written.len() <= 2 => {
                Day::from_ymd(self.year?, self.month?.into(), written.parse().ok()?)
            }
            _ => None,
        }
    }

    /// `day` as briefly as this context allows: what [`complete`](Folder::complete) reads back.
    pub fn shorten(self, day: Day) -> String {
        let (year, month, of_month) = day.ymd();
        match (self.year == Some(year), self.month.map(u32::from) == Some(month)) {
            (true, true) => format!("{of_month:02}"),
            (true, false) => format!("{month:02}-{of_month:02}"),
            _ => day.to_string(),
        }
    }
}

impl<'s> Parser<'s> {
    /// The date an item starts with (or `opening` is followed by): a date, `MM-DD`
    /// or the bare day of the month.
    pub fn item_date(&mut self, what: &str) -> Parse<Day> {
        let token = self.peek();
        let Some(day) = self.day_of_month(token) else { return self.date(what) };
        self.bump();
        self.complete(token.loc, self.folder.month, day)
    }

    /// A date, or `MM-DD` counted forward: the first such day on or after
    /// `anchor`, the day of the line it is written on. Where there is no such
    /// day, it is completed like any other short date.
    pub fn date_from(&mut self, anchor: Option<Day>, what: &str) -> Parse<Day> {
        let token = self.peek();
        let (Tok::MonthDay(month, day), Some(anchor)) = (token.tok, anchor) else { return self.date(what) };
        self.bump();
        // A day that exists in some year near the anchor's: `02-29` waits for a leap year.
        let (month, day) = (u32::from(month), u32::from(day));
        let year = anchor.year();
        let next = (year..year + 9).find_map(|year| Day::from_ymd(year, month, day).filter(|&found| found >= anchor));
        next.ok_or_else(|| self.report(not_a_date(token.loc, self.text(token.loc), (year, month, day))))
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

    /// The day that a short date written at `at` means in its context.
    fn complete(&mut self, at: Loc, month: Option<u8>, day: u8) -> Parse<Day> {
        let (Some(year), Some(month)) = (self.folder.year, month) else {
            return self.fail(short_date(at, self.text(at), self.folder));
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
        if self.eat(Punct::DotDot).is_some() {
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

/// A short date whose context does not give the rest.
fn short_date(at: Loc, written: &str, folder: Folder) -> Diagnostic {
    let missing = if folder.year.is_some() { "month" } else { "year" };
    Diagnostic::error("short-date", format!("`{written}` leaves out the {missing}, which no heading above it and no folder gives"))
        .label(at, "write the whole date here")
        .note("a date may be short where a heading above it (`2026-03`) or its file's folder gives the rest: `15` is enough in `journal/2026/03.ax`")
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
