//! Journal items: everything that starts with a date, and plans, which are
//! transactions that repeat.

use axiom_core::day::days_in_month;
use axiom_core::{Day, Diagnostic, Loc, Span};

use crate::ast::{Assert, Event, EventState, Item, ItemKind, Name, On, PlaceRef, Plan, Price, Txn};
use crate::lex::Tok;
use crate::lines::Line;
use crate::parser::{Parse, Parser};

const EVENT_STATES: [(&str, EventState); 3] =
    [("settled", EventState::Settled), ("void", EventState::Void), ("returned", EventState::Returned)];

const CADENCES: [(&str, Span); 5] = [
    ("day", Span::days(1)),
    ("week", Span::days(7)),
    ("month", Span::months(1)),
    ("quarter", Span::months(3)),
    ("year", Span::months(12)),
];

const WEEKDAYS: [(&str, u8); 7] =
    [("monday", 0), ("tuesday", 1), ("wednesday", 2), ("thursday", 3), ("friday", 4), ("saturday", 5), ("sunday", 6)];

/// The optional clauses of a plan header, each with where it was written so a
/// repeat can point back at it.
#[derive(Default)]
struct Bounds {
    on: Option<(On, Loc)>,
    from: Option<(Day, Loc)>,
    until: Option<(Day, Loc)>,
}

/// Fills `slot`, or gives the location of the earlier clause that already did.
fn fill<T>(slot: &mut Option<(T, Loc)>, value: T, loc: Loc) -> Result<(), Loc> {
    match slot {
        Some((_, first)) => Err(*first),
        None => {
            *slot = Some((value, loc));
            Ok(())
        }
    }
}

impl<'s> Parser<'s> {
    /// A line that began with a date. What follows it says which kind of entry
    /// it is: a `#code` is an event, a commodity a price, anything else a flow
    /// or an assertion.
    pub fn journal_entry(&mut self, line: &mut Line<'s>, date: Day) -> Parse<Item<'s>> {
        let token = self.cursor.peek();
        match token.tok {
            Tok::Code(text) => {
                self.cursor.bump();
                self.event(line, date, Name { text, loc: token.loc })
            }
            Tok::Unit(_) => self.price(line, date),
            _ => self.transaction_or_assertion(line, date),
        }
    }

    pub fn date(&mut self, what: &str) -> Parse<Day> {
        let token = self.cursor.peek();
        match token.tok {
            Tok::Date(day) => {
                self.cursor.bump();
                Ok(day)
            }
            _ => Err(self.expected("expected-date", what)),
        }
    }

    fn transaction_or_assertion(&mut self, line: &mut Line<'s>, date: Day) -> Parse<Item<'s>> {
        let until = match self.cursor.eat(Tok::DotDot) {
            Some(_) => Some(self.date("the last day of the range, like `2026-12-31`")?),
            None => None,
        };
        let from = self.side()?;
        // `PLACE = AMOUNT` is an assertion. Reading the place first is what
        // lets one pass tell it from a flow.
        let could_assert = until.is_none() && from.amount.is_none() && matches!(self.cursor.peek().tok, Tok::Eq);
        if could_assert && let Some(place) = from.place {
            return self.assertion(line, date, place);
        }
        let mut flow = self.flow_head(from)?;
        let header = self.end_header(line)?;
        self.flow_legs(line, &mut flow)?;
        Ok(header.item(ItemKind::Txn(Txn { date, until, flow })))
    }

    fn assertion(&mut self, line: &mut Line<'s>, date: Day, place: PlaceRef<'s>) -> Parse<Item<'s>> {
        self.cursor.bump();
        let amount = self.amount()?;
        let waive = self.waiver()?;
        let header = self.end_header(line)?;
        Ok(header.item(ItemKind::Assert(Assert { date, place, amount, waive })))
    }

    fn event(&mut self, line: &mut Line<'s>, date: Day, code: Name<'s>) -> Parse<Item<'s>> {
        let (state, state_loc) = self.choose(&EVENT_STATES, "unknown-event-state", "settlement state")?;
        let header = self.end_header(line)?;
        Ok(header.item(ItemKind::Event(Event { date, code, state, state_loc })))
    }

    fn price(&mut self, line: &mut Line<'s>, date: Day) -> Parse<Item<'s>> {
        let unit = self.unit("expected-commodity", "a commodity such as `VTI`")?;
        let price = self.measured()?;
        let header = self.end_header(line)?;
        Ok(header.item(ItemKind::Price(Price { date, unit, price })))
    }

    /// `every CADENCE [on DAY] [from DATE] [until DATE|MONTH] FLOW`, where the
    /// bounds may equally follow the flow's tail.
    pub fn plan(&mut self, line: &mut Line<'s>) -> Parse<Item<'s>> {
        let every = self.cadence()?;
        let mut bounds = Bounds::default();
        self.plan_bounds(&mut bounds)?;
        let from = self.side()?;
        let mut flow = self.flow_head(from)?;
        self.plan_bounds(&mut bounds)?;
        let header = self.end_header(line)?;
        self.flow_legs(line, &mut flow)?;
        let Bounds { on, from, until } = bounds;
        let (on, from, until) = (on.map(|(on, _)| on), from.map(|(day, _)| day), until.map(|(day, _)| day));
        Ok(header.item(ItemKind::Plan(Plan { every, on, from, until, flow })))
    }

    fn cadence(&mut self) -> Parse<Span> {
        if let Tok::Span(span) = self.cursor.peek().tok {
            self.cursor.bump();
            return Ok(span);
        }
        let (span, _) = self.choose(&CADENCES, "unknown-cadence", "cadence")?;
        Ok(span)
    }

    fn plan_bounds(&mut self, bounds: &mut Bounds) -> Parse<()> {
        while let Tok::Name(word @ ("on" | "from" | "until")) = self.cursor.peek().tok {
            let keyword = self.cursor.bump().loc;
            let earlier = match word {
                "on" => {
                    let on = self.plan_day()?;
                    fill(&mut bounds.on, on, keyword)
                }
                "from" => {
                    let day = self.date("the day the plan starts, like `2026-01-01`")?;
                    fill(&mut bounds.from, day, keyword)
                }
                _ => {
                    let day = self.until_day()?;
                    fill(&mut bounds.until, day, keyword)
                }
            };
            if let Err(first) = earlier {
                return Err(self.duplicate(&format!("`{word}` clause"), keyword, first));
            }
        }
        Ok(())
    }

    /// `DATE`, or `MONTH` meaning that month's last day.
    fn until_day(&mut self) -> Parse<Day> {
        let token = self.cursor.peek();
        let day = match token.tok {
            Tok::Date(day) => day,
            Tok::Month(first) => first.month_end(),
            _ => return Err(self.expected("expected-date", "a date or month, like `2027-06`")),
        };
        self.cursor.bump();
        Ok(day)
    }

    /// The day within each period: `15`, `04-15`, or `monday`.
    fn plan_day(&mut self) -> Parse<On> {
        let token = self.cursor.peek();
        match token.tok {
            Tok::Number(_) => {
                self.cursor.bump();
                match self.text(token.loc).parse::<u8>() {
                    Ok(day @ 1..=31) => Ok(On::MonthDay(day)),
                    _ => self.fail(not_a_day(token.loc, self.text(token.loc))),
                }
            }
            Tok::Name(word) => match month_and_day(word) {
                Some((month, day)) => {
                    self.cursor.bump();
                    self.day_of_year(token.loc, month, day)
                }
                None => self.choose(&WEEKDAYS, "unknown-day", "weekday").map(|(weekday, _)| On::Weekday(weekday)),
            },
            _ => Err(self.expected("expected-day", "a day: `15`, `04-15` or a weekday")),
        }
    }

    fn day_of_year(&mut self, loc: Loc, month: u8, day: u8) -> Parse<On> {
        // Any year will do for the check; a leap year admits `02-29`.
        let valid = (1..=12).contains(&month) && (1..=days_in_month(2024, month.into())).contains(&day.into());
        if valid { Ok(On::YearDay { month, day }) } else { self.fail(not_a_day(loc, self.text(loc))) }
    }
}

/// `04-15` as (4, 15), when the word is two two-digit numbers around a dash.
fn month_and_day(word: &str) -> Option<(u8, u8)> {
    let (month, day) = word.split_once('-')?;
    Some((two_digits(month)?, two_digits(day)?))
}

fn two_digits(part: &str) -> Option<u8> {
    if part.len() != 2 || !part.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    part.parse().ok()
}

fn not_a_day(loc: Loc, written: &str) -> Diagnostic {
    Diagnostic::error("bad-day", format!("`{written}` is not a day of the month or year"))
        .label(loc, "no such day")
        .help("write a day of the month (`on 15`), a month and day (`on 04-15`), or a weekday (`on monday`)")
}
