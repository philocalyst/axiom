//! The days `each year closing MM-DD` laws judge a year.
//!
//! Such a law runs for a year on that day of the next, so what the journal
//! recognizes `for` the year until then counts. Until that day the law has not
//! run: the tallies it reads are counted so far, and what it owes is not
//! figured. Views that look at a year, or run the books forward, ask here when
//! that day is, from the laws themselves.

use axiom_core::Day;
use axiom_model::{Book, Closing, Period, Rule, Trigger};

/// The rules of the book's closing laws, with the day of the year each closes on.
fn rules<'b>(book: &'b Book) -> impl Iterator<Item = (&'b Rule, Closing)> {
    book.rules.timed.iter().filter_map(|rule| match book.laws[rule.law].trigger {
        Trigger::Each(Period::Year, Some(closing)) => Some((rule, closing)),
        _ => None,
    })
}

/// The day to run the books to so that what happens in `day`'s year is judged:
/// the end of that year, or the last day a closing law judges it, if later.
pub fn judged_through(book: &Book, day: Day) -> Day {
    let year_end = day.year_end();
    days_for(book, day.year(), |_| true).last().map_or(year_end, |&closes| closes.max(year_end))
}

/// The first day after `day` on which a closing law judges a year.
pub fn next_after(book: &Book, day: Day) -> Option<Day> {
    // A year is judged in the next one: the year before `day`'s, or its own.
    let years = day.year() - 1..=day.year();
    years.flat_map(|year| days_for(book, year, |_| true)).filter(|&closes| closes > day).min()
}

/// The days on which closing laws judge `year`, earliest first and each once.
/// Only the rules `wanted` picks count, and only if they were in force on some
/// day of that year (a residence that ended before it has nothing to judge).
pub fn days_for(book: &Book, year: i32, wanted: impl Fn(&Rule) -> bool) -> Vec<Day> {
    let (first, last) = (Day::from_ymd(year, 1, 1), Day::from_ymd(year, 12, 31));
    let (Some(first), Some(last)) = (first, last) else { return Vec::new() };
    let in_force = |rule: &Rule| rule.from <= last && first <= rule.until;
    let judged = rules(book).filter(|&(rule, _)| in_force(rule) && wanted(rule));
    let mut days: Vec<Day> =
        judged.filter_map(|(_, closing)| Day::from_ymd(year + 1, closing.month.into(), closing.day.into())).collect();
    days.sort_unstable();
    days.dedup();
    days
}
