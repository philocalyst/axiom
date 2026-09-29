//! `register`: one place's flows, dated, with a running balance.
//!
//! Amounts and balances are in the place's display sign, the way a statement
//! shows them: what a card owes is positive, and a charge adds to it.

use std::borrow::Cow;
use std::collections::BTreeMap;

use axiom_core::{Day, Diagnostic, Id, Qty};
use axiom_engine::{Pad, Run, State};
use axiom_model::{Amount, Book, Commodity, Place};

use crate::history::{Change, Posting, pad_ends};
use crate::lens::Whose;
use crate::places::path;
use crate::resolve;
use crate::table::{code_labels, gap_words};
use crate::{Cell, Column, Report, Row, Section, Style};

pub fn view<'s>(
    book: &Book<'s>,
    run: &Run,
    whose: &Whose,
    place: &str,
    from: Option<Day>,
    to: Option<Day>,
) -> Result<Report<'s>, Diagnostic> {
    let place = resolve::place(book, place)?;
    // A place is somebody's: another owner's register is not part of whose money this is.
    let owner = book.places[place].owner;
    let register = match whose.includes(owner) {
        true => section(book, run, place, from, to),
        false => Section::note_only(format!(
            "{} belongs to {}, whose money this is not.",
            path(book, place),
            book.name(book.entities[owner].path)
        )),
    };
    Ok(Report::new(format!("Register: {}", path(book, place))).with(register))
}

/// The flows touching `place` from `from` to `to` (default: everything up to
/// the run's day), each with the balance after it.
///
/// The running balance counts what is real at the end of the window. Pending,
/// void and returned flows are listed, muted, and leave it alone. A flow into
/// or out of `PLACE.basis` is listed with the change it made to the basis, and
/// leaves the balance alone: no quantity moved.
pub fn section<'s>(book: &Book<'s>, run: &Run, place: Id<Place>, from: Option<Day>, to: Option<Day>) -> Section<'s> {
    let steps = steps(book, run, place, to.unwrap_or(run.today));
    let split = from.map_or(0, |from| steps.partition_point(|step| step.day < from));
    let sign = book.v3_root(place).display_sign();
    let shown = |qty: Qty, unit: Id<Commodity>| Cell::amount(book, Amount::new(Qty(qty.0 * sign), unit));

    let columns = [
        Column::left("Date"),
        Column::left("With"),
        Column::left("Payee"),
        Column::left("Note"),
        Column::right("Amount"),
        Column::right("Balance"),
    ];
    let mut section = Section::new(columns);
    let mut running: BTreeMap<Id<Commodity>, Qty> = BTreeMap::new();
    for step in &steps[..split] {
        if let Change::Moved(moved) = step.change {
            *running.entry(moved.unit).or_default() += step.counted();
        }
    }
    if let Some(from) = from {
        for (&unit, &qty) in running.iter().filter(|(_, qty)| !qty.is_zero()) {
            let cells = [
                Cell::Day(from),
                Cell::text("opening balance"),
                Cell::Blank,
                Cell::Blank,
                Cell::Blank,
                shown(qty, unit),
            ];
            section.push(Row::new(cells).style(Style::Total));
        }
    }
    for step in &steps[split..] {
        let (amount, balance) = match step.change {
            Change::Moved(moved) => {
                let balance = running.entry(moved.unit).or_default();
                *balance += step.counted();
                (shown(moved.qty, moved.unit), shown(*balance, moved.unit))
            }
            Change::Rebased(_) => (Cell::Blank, Cell::Blank),
        };
        let payee = step.source.posting().and_then(|posting| posting.flow.payee);
        let cells = [
            Cell::Day(step.day),
            Cell::text(path(book, step.with)),
            payee.map_or(Cell::Blank, |entity| Cell::text(book.name(book.entities[entity].path))),
            note(book, step).map_or(Cell::Blank, Cell::text),
            amount,
            balance,
        ];
        section.push(Row::new(cells).style(if step.counts { Style::Normal } else { Style::Muted }));
    }

    if section.rows.is_empty() {
        section.note(format!("Nothing touches {} in this window.", path(book, place)));
    }
    if section.rows.iter().any(|row| row.style == Style::Muted) {
        section.note("Muted lines are pending, void or returned: they do not move the balance.");
    }
    section
}

/// One journal flow, or one accepted gap, as seen from a place.
struct Step<'a> {
    day: Day,
    change: Change,
    /// Whether it is real at the end of the window.
    counts: bool,
    /// The other end.
    with: Id<Place>,
    source: Source<'a>,
}

/// What made a step.
enum Source<'a> {
    Flow(Posting<'a>),
    /// A pad, which the journal never wrote: the engine posted it to close the
    /// gap an assertion accepted.
    Gap(&'a Pad),
}

impl<'a> Source<'a> {
    fn posting(&self) -> Option<Posting<'a>> {
        match *self {
            Source::Flow(posting) => Some(posting),
            Source::Gap(_) => None,
        }
    }
}

impl Step<'_> {
    /// What it adds to the running balance.
    fn counted(&self) -> Qty {
        match self.change {
            Change::Moved(moved) if self.counts => moved.qty,
            Change::Moved(_) | Change::Rebased(_) => Qty::ZERO,
        }
    }
}

/// Every step touching `place` up to `cutoff`, in order. A pad, made at the
/// end of its day, follows that day's flows.
fn steps<'a>(book: &'a Book, run: &'a Run, place: Id<Place>, cutoff: Day) -> Vec<Step<'a>> {
    let flows = book.touching[place].iter().flat_map(|&id| {
        let posting = Posting::at(book, run, id);
        posting.changes_at(place).map(move |change| Step {
            day: posting.flow.day,
            change,
            counts: posting.is_real_on(cutoff),
            with: posting.counterparty(place),
            source: Source::Flow(posting),
        })
    });
    let pads = run.pads.iter().flat_map(|pad| {
        let with = if pad.place == place { pad.counter } else { pad.place };
        let here = pad_ends(pad).into_iter().filter(move |&(at, _)| at == place);
        here.map(move |(_, moved)| Step {
            day: pad.day,
            change: Change::Moved(moved),
            counts: true,
            with,
            source: Source::Gap(pad),
        })
    });
    let mut steps: Vec<Step> = flows.chain(pads).filter(|step| step.day <= cutoff).collect();
    steps.sort_by_key(|step| step.day);
    steps
}

/// A change of basis, codes, and settlement, as one line of small print.
fn note(book: &Book, step: &Step) -> Option<Cow<'static, str>> {
    let rebased = match step.change {
        Change::Rebased(by) => Some(format!("basis {}{}", if by.qty.is_negative() { "" } else { "+" }, book.show(by))),
        Change::Moved(_) => None,
    };
    let details: Vec<String> = match step.source {
        Source::Gap(pad) => vec![gap_words(book, pad)],
        Source::Flow(posting) => {
            let status = match posting.posted.state {
                State::Actual | State::Planned => None,
                State::Pending => Some("pending".to_string()),
                State::Void => Some("void".to_string()),
                State::Settled(on) if !step.counts => Some(format!("pending until {on}")),
                State::Settled(on) => Some(format!("settled {on}")),
                State::Returned(on) => Some(format!("returned {on}")),
            };
            code_labels(book, &posting.flow.codes).chain(status).collect()
        }
    };
    let parts: Vec<String> = rebased.into_iter().chain(details).collect();
    (!parts.is_empty()).then(|| parts.join(" · ").into())
}
