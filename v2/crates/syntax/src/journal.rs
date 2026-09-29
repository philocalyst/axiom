//! Journal items: everything that starts with a date, `opening` blocks, and
//! plans, which are transactions that repeat.

use axiom_core::day::days_in_month;
use axiom_core::{Day, Dec, Diagnostic, Loc, Span};

use crate::ast::*;
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

impl<'s> Parser<'s> {
    /// A line that began with a date. What follows it says which kind of entry
    /// it is: a `#code` is an event, a commodity a price or split, anything
    /// else a flow, an assertion, or an occurrence of a plan.
    pub fn journal_entry(&mut self, line: &mut Line<'s>, date: Day) -> Parse<()> {
        match self.tok() {
            Tok::Code(code) => {
                self.bump();
                let (state, state_loc) = self.choose(&EVENT_STATES, "unknown-event-state", "settlement state")?;
                let header = self.end_header(line)?;
                let id = self.push(Event { date, code, state, state_loc });
                self.emit(&header, ItemKind::Event(id));
                Ok(())
            }
            Tok::Unit(_) => self.price_or_split(line, date),
            _ => self.transaction(line, date),
        }
    }

    /// `[..DATE] FROM`, then what follows it: `=` makes an assertion, the end
    /// of the line an occurrence of a plan, anything else a flow. Reading the
    /// first end before deciding is what lets one pass tell them apart.
    fn transaction(&mut self, line: &mut Line<'s>, date: Day) -> Parse<()> {
        let clauses = self.mark::<Clause>();
        let spread = self.spread(line, date)?;
        let from = self.end()?;
        if !spread && from.amount.is_none() && self.at("=") {
            if let Some(place) = from.place {
                return self.assertion(line, date, place);
            }
        }
        let amount = match from.amount {
            None => Some(None),
            Some(Quantity::Fixed(amount)) => Some(Some(amount)),
            Some(_) => None,
        };
        if let (false, true, Some(place), Some(amount)) = (spread, self.at_eol(), &from.place, amount) {
            if place.select.is_empty() {
                return self.occurrence(line, date, place.name, amount);
            }
        }
        let (mut flow, arrow) = self.flow_head(from, clauses)?;
        let header = self.end_header(line)?;
        self.flow_legs(line, &mut flow, arrow)?;
        let id = self.push(Txn { date, flow });
        self.emit(&header, ItemKind::Txn(id));
        Ok(())
    }

    /// `..DATE` after a transaction's date: it is paid that day and recognized
    /// over the range, which is what the clause `for DATE..DATE` says. Adds
    /// that clause.
    fn spread(&mut self, line: &Line<'s>, date: Day) -> Parse<bool> {
        let Some(dots) = self.eat("..") else { return Ok(false) };
        let last = self.date("the last day of the range, like `2026-12-31`")?;
        let range = self.loc_from(line.body);
        if last < date {
            return self.fail(empty_range(range, self.text(range), date, last));
        }
        self.t.clauses.push(Clause { at: dots.to(range), kind: ClauseKind::For(For::Period(date, last)) });
        Ok(true)
    }

    /// `DATE PLACE = [-]AMOUNT [! [STRING] | via PLACE]`.
    fn assertion(&mut self, line: &mut Line<'s>, date: Day, place: Place<'s>) -> Parse<()> {
        self.bump();
        let amount = self.signed_amount()?;
        let gap = match self.tok() {
            Tok::Punct("!") => Gap::Waived(self.waiver()?),
            Tok::Name("via") => {
                self.bump();
                Gap::Via(self.name("expected-place", "the place the difference goes to, like `income/market`")?)
            }
            _ => Gap::Refused,
        };
        let header = self.end_header(line)?;
        let id = self.push(Assert { date, place, amount, gap });
        self.emit(&header, ItemKind::Assert(id));
        Ok(())
    }

    /// `DATE PLAN [AMOUNT]` with override legs below.
    fn occurrence(&mut self, line: &mut Line<'s>, date: Day, plan: Name<'s>, amount: Option<Amount<'s>>) -> Parse<()> {
        let header = self.end_header(line)?;
        let legs = self.legs(line, |parser, leg_line| parser.leg(leg_line).map(drop))?;
        let id = self.push(Occurrence { date, plan, amount, legs });
        self.emit(&header, ItemKind::Occurrence(id));
        Ok(())
    }

    /// `opening DATE` and its lines `PLACE [SELECTOR] AMOUNT [basis AMOUNT] [since DATE]`.
    pub fn opening(&mut self, line: &mut Line<'s>) -> Parse<()> {
        let date = self.date("the day the balances are stated, like `2024-12-31`")?;
        let header = self.end_header(line)?;
        self.opening = true;
        let lines = self.legs(line, |parser, opening_line| {
            let leg = parser.leg(opening_line)?;
            match parser.get(leg).amount {
                Quantity::Fixed(_) => Ok(()),
                _ => parser.fail(opening_needs_amount(parser.get(leg).loc)),
            }
        });
        self.opening = false;
        let id = self.push(Opening { date, lines: lines? });
        self.emit(&header, ItemKind::Opening(id));
        Ok(())
    }

    /// `DATE UNIT PRICE`, or `DATE UNIT split N for M`.
    fn price_or_split(&mut self, line: &mut Line<'s>, date: Day) -> Parse<()> {
        let unit = self.unit("expected-commodity", "a commodity such as `VTI`")?;
        if self.eat_word("split").is_some() {
            let numerator = self.split_count("the new number of units, like `2` in `split 2 for 1`")?;
            self.expect_word("for", "expected-for", "`for` and the old number of units, like `split 2 for 1`")?;
            let denominator = self.split_count("the old number of units, like `1` in `split 2 for 1`")?;
            let header = self.end_header(line)?;
            let id = self.push(Split { date, unit, numerator, denominator });
            self.emit(&header, ItemKind::Split(id));
            return Ok(());
        }
        let price = self.measured()?;
        let header = self.end_header(line)?;
        let id = self.push(Price { date, unit, price });
        self.emit(&header, ItemKind::Price(id));
        Ok(())
    }

    /// A number of units in a split: a positive number.
    fn split_count(&mut self, what: &str) -> Parse<Dec> {
        let token = self.peek();
        let Tok::Number(count) = token.tok else { return Err(self.expected("expected-number", what)) };
        if count.is_zero() {
            let diag = Diagnostic::error("bad-split", "a split cannot have zero units")
                .label(token.loc, "zero units would destroy every holding");
            return self.fail(diag);
        }
        self.bump();
        Ok(count)
    }

    // ─── Plans ──────────────────────────────────────────────────────────────

    /// `every CADENCE [on DAY] [from DATE] [until DATE|MONTH] FLOW`, where the
    /// bounds may equally follow the flow's tail; `name` is that of a named plan.
    pub fn plan(&mut self, line: &mut Line<'s>, name: Option<Name<'s>>) -> Parse<()> {
        let every = match self.tok() {
            Tok::Span(span) => {
                self.bump();
                span
            }
            _ => self.choose(&CADENCES, "unknown-cadence", "cadence")?.0,
        };
        let mut bounds = Bounds::default();
        self.plan_bounds(&mut bounds)?;
        let from = self.end()?;
        let (mut flow, arrow) = self.flow_head(from, self.mark::<Clause>())?;
        self.plan_bounds(&mut bounds)?;
        let header = self.end_header(line)?;
        self.flow_legs(line, &mut flow, arrow)?;
        let Bounds { on, from, until } = bounds;
        let (on, from, until) = (on.map(|(on, _)| on), from.map(|(day, _)| day), until.map(|(day, _)| day));
        let id = self.push(Plan { name, every, on, from, until, flow });
        self.emit(&header, ItemKind::Plan(id));
        Ok(())
    }

    /// `plan NAME every …`
    pub fn named_plan(&mut self, line: &mut Line<'s>) -> Parse<()> {
        let name = self.name("expected-name", "a plan name")?;
        self.expect_word("every", "expected-every", "`every` and how often the plan happens, like `every 2w`")?;
        self.plan(line, Some(name))
    }

    /// The bounds `on DAY`, `from DATE` and `until DATE|MONTH`, each at most
    /// once and in any order.
    fn plan_bounds(&mut self, bounds: &mut Bounds) -> Parse<()> {
        while let Tok::Name(word @ ("on" | "from" | "until")) = self.tok() {
            let keyword = self.bump().loc;
            let earlier = match word {
                "on" => self.plan_day().map(|on| bounds.on.replace((on, keyword)).map(|(_, first)| first)),
                "from" => self
                    .date("the day the plan starts, like `2026-01-01`")
                    .map(|day| bounds.from.replace((day, keyword)).map(|(_, first)| first)),
                _ => self.until_day().map(|day| bounds.until.replace((day, keyword)).map(|(_, first)| first)),
            };
            if let Some(first) = earlier? {
                return Err(self.duplicate(&format!("`{word}` clause"), keyword, first));
            }
        }
        Ok(())
    }

    /// `DATE`, or `MONTH` meaning that month's last day.
    fn until_day(&mut self) -> Parse<Day> {
        let day = match self.tok() {
            Tok::Date(day) => day,
            Tok::Month(first) => first.month_end(),
            _ => return Err(self.expected("expected-date", "a date or month, like `2027-06`")),
        };
        self.bump();
        Ok(day)
    }

    /// The day within each period: `15`, `04-15`, or `monday`.
    fn plan_day(&mut self) -> Parse<On> {
        let token = self.peek();
        let written = self.text(token.loc);
        match token.tok {
            Tok::Number(_) => {
                self.bump();
                match written.parse::<u8>() {
                    Ok(day @ 1..=31) => Ok(On::MonthDay(day)),
                    _ => self.fail(not_a_day(token.loc, written)),
                }
            }
            Tok::Name(word) => match month_and_day(word) {
                Some((month, day)) if valid_year_day(month, day) => {
                    self.bump();
                    Ok(On::YearDay { month, day })
                }
                Some(_) => self.fail(not_a_day(token.loc, word)),
                None => self.choose(&WEEKDAYS, "unknown-day", "weekday").map(|(weekday, _)| On::Weekday(weekday)),
            },
            _ => Err(self.expected("expected-day", "a day: `15`, `04-15` or a weekday")),
        }
    }
}

/// `04-15` as (4, 15), when the word is two two-digit numbers around a dash.
pub(crate) fn month_and_day(word: &str) -> Option<(u8, u8)> {
    let (month, day) = word.split_once('-')?;
    let two_digits = |part: &str| if part.len() == 2 { part.parse::<u8>().ok() } else { None };
    Some((two_digits(month)?, two_digits(day)?))
}

/// Whether the month has that day in some year: a leap year admits `02-29`.
pub(crate) fn valid_year_day(month: u8, day: u8) -> bool {
    (1..=12).contains(&month) && (1..=days_in_month(2024, month.into())).contains(&day.into())
}

pub(crate) fn not_a_day(loc: Loc, written: &str) -> Diagnostic {
    Diagnostic::error("bad-day", format!("`{written}` is not a day of the month or year"))
        .label(loc, "no such day")
        .help("write a day of the month (`on 15`), a month and day (`on 04-15`), or a weekday (`on monday`)")
}

fn opening_needs_amount(leg: Loc) -> Diagnostic {
    Diagnostic::error("opening-amount", "an opening line says how much a place holds")
        .label(leg, "no amount here")
        .help("write the balance the statement shows: `checking 10_000 USD`")
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
