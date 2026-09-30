//! `tax`: what the laws tallied and what they say is owed, for one year.
//!
//! There is no tax engine here. Jurisdictions' laws `count` flows and gains
//! into named tallies and `owe` obligations; this view lays the effects of a
//! year out under the systems that recorded them, with what each jurisdiction
//! is owed, and keeps each line's source, so `why` can trace it back.

use std::collections::{BTreeMap, BTreeSet};
use std::iter;

use axiom_core::{Day, Id, Map, Qty, Sym};
use axiom_engine::{Cause, Effect, Owed};
use axiom_model::{Amount, Book, Commodity, Entity, System};

use crate::closings;
use crate::lens::Lens;
use crate::table::{cause_cell, year_days};
use crate::{Cell, Column, Money, Report, Row, Section, Style, When};

pub fn view<'s>(lens: Lens<'_, 's>, year: Option<i32>) -> Report<'s> {
    let (book, run, whose) = (lens.book, lens.run, lens.whose);
    let year = year.unwrap_or_else(|| lens.day.year());
    // An effect belongs to the year of the day it was recorded.
    let effects = run.effects.iter().filter(|effect| whose.includes(effect.owner) && effect.day.year() == year);
    let (owed, tallied): (Vec<&Effect>, Vec<&Effect>) = effects.partition(|effect| effect.owe.is_some());
    let owners: BTreeSet<Id<Entity>> = owed.iter().chain(&tallied).map(|effect| effect.owner).collect();

    let (tallied, owed) = (lines(&tallied), lines(&owed));
    // A closing law has not judged the year until its day: what it owes is
    // missing, not nothing, and what was counted is counted so far.
    let closes = pending_closings(lens, year);
    let (several, during) = (owners.len() > 1, year_days(year).map(When::During));
    let mut tallies = tallies(book, &tallied, several, during);
    let mut obligations = obligations(book, &owed, several, !closes.is_empty(), during);
    if !closes.is_empty() {
        let note = not_closed(year, &closes);
        if obligations.rows.is_empty() {
            // Not an empty table, which would read as owing nothing.
            obligations = Section::note_only(note).headed("Owed");
        } else {
            obligations.note(note);
        }
    }
    if tallies.rows.is_empty() && obligations.rows.is_empty() && closes.is_empty() {
        tallies.note(
            "Nothing was counted or owed. Laws count and owe only for entities that live under a system that declares them.",
        );
    } else {
        let listed = if obligations.rows.is_empty() { &mut tallies } else { &mut obligations };
        listed.note("Trace any line with `axiom why NAME`, or `axiom why FILE:LINE` from its source.");
    }
    if owed.iter().any(|line| line.priced) {
        obligations.note("A penalty is the price of a violated law: it is owed instead of the law failing.");
    }
    let mut title = vec!["Taxes".into(), Cell::year(year)];
    if let [owner] = owners.iter().collect::<Vec<_>>()[..] {
        title.extend(["for".into(), Cell::Name(book.name(book.entities[*owner].path))]);
    }
    Report::new(Cell::Join(" ", title)).with(tallies).with(obligations)
}

/// The days after the run's end on which closing laws written for `whose`
/// will judge `year`.
fn pending_closings(lens: Lens, year: i32) -> Vec<Day> {
    let (book, whose) = (lens.book, lens.whose);
    let mut days = closings::days_for(book, year, |rule| whose.governs(book, rule.subject));
    days.retain(|&day| day > lens.run.horizon);
    days
}

/// Why a year has no return yet: the days its closing laws will judge it.
fn not_closed<'s>(year: i32, closes: &[Day]) -> Cell<'s> {
    let days = Cell::list(" and ", closes.iter().map(|&day| Cell::Day(day)));
    let (verb, rest) = if let [_] = closes {
        ("return closes on", "; what it owes is not figured yet; the tallies are counted so far.")
    } else {
        ("returns close on", "; what they owe is not figured yet; the tallies are counted so far.")
    };
    ["The".into(), Cell::year(year), verb.into(), days, rest.into()].into()
}

/// Everything counted or owed under one name. A tally is one line on a
/// person's year however many systems add to it; it sits under the first.
struct Line {
    owner: Id<Entity>,
    system: Option<Id<System>>,
    name: Sym,
    /// Who is owed, and by when; `None` for a tally.
    owed: Option<Owed>,
    /// The price of a violated law rather than a tax computed from tallies.
    priced: bool,
    amount: Amount,
    contributions: usize,
    /// The first contribution's cause: the only one when there is just one.
    cause: Cause,
}

/// The effects merged into lines, grouped by owner and system in tree order,
/// and by first appearance within a system. Systems record lines in the order
/// they work through them, so that order reads as the return does.
fn lines(effects: &[&Effect]) -> Vec<Line> {
    let mut lines: Vec<Line> = Vec::new();
    let mut at: Map<(Id<Entity>, Sym, Option<(Id<Entity>, Day)>, bool, Id<Commodity>), usize> = Map::default();
    for effect in effects {
        let key =
            (effect.owner, effect.name, effect.owe.map(|owed| (owed.to, owed.due)), effect.priced, effect.amount.unit);
        let index = *at.entry(key).or_insert_with(|| {
            lines.push(Line {
                owner: effect.owner,
                system: effect.system,
                name: effect.name,
                owed: effect.owe,
                priced: effect.priced,
                amount: Amount::zero(effect.amount.unit),
                contributions: 0,
                cause: effect.cause,
            });
            lines.len() - 1
        });
        lines[index].amount.qty += effect.amount.qty;
        lines[index].contributions += 1;
    }
    // Stable: lines of one system keep the order they first appeared in.
    lines.sort_by_key(|line| (line.owner, line.system));
    lines
}

fn tallies<'s>(book: &Book<'s>, lines: &[Line], several: bool, during: Option<When>) -> Section<'s> {
    let mut section =
        Section::new([Column::left("Tally"), Column::right("Amount"), Column::left("From")]).headed("Counted");
    let cells =
        |line: &Line| vec![Cell::Name(book.name(line.name)), Cell::amount(book, line.amount), source(book, line)];
    grouped(&mut section, book, lines, several, cells, |_, _| Vec::new());
    facts(&mut section, book, lines, "tally", during);
    section
}

/// Every line as a fact of its owner's year.
fn facts<'s>(section: &mut Section<'s>, book: &Book<'s>, lines: &[Line], concept: &'static str, during: Option<When>) {
    let Some(during) = during else { return };
    for line in lines {
        let owner = book.name(book.entities[line.owner].path);
        section.fact(concept, Some(book.name(line.name)), owner, during, Money::of(book, line.amount));
    }
}

/// Obligations, and what each jurisdiction is owed. `unfinished`: a law that
/// owes has not judged the year yet, so the totals are what is owed so far.
fn obligations<'s>(
    book: &Book<'s>,
    lines: &[Line],
    several: bool,
    unfinished: bool,
    during: Option<When>,
) -> Section<'s> {
    let columns =
        [Column::left("Owed"), Column::left("To"), Column::left("Due"), Column::right("Amount"), Column::left("From")];
    let mut section = Section::new(columns).headed("Owed");
    let cells = |line: &Line| {
        let (to, due) = line.owed.map_or((Cell::Blank, Cell::Blank), |owed| {
            (Cell::Name(book.name(book.entities[owed.to].path)), Cell::Day(owed.due))
        });
        let name = Cell::Name(book.name(line.name));
        let name = if line.priced { [name, "(penalty)".into()].into() } else { name };
        vec![name, to, due, Cell::amount(book, line.amount), source(book, line)]
    };
    // Each jurisdiction is owed its own total, when there is more than one to tell apart.
    let several_groups = lines.chunk_by(|a, b| (a.owner, a.system) == (b.owner, b.system)).nth(1).is_some();
    let foot = |group: &[Line], jurisdiction: Cell<'s>| {
        if several_groups { totals(book, group, ["Total".into(), jurisdiction].into()) } else { Vec::new() }
    };
    grouped(&mut section, book, lines, several, cells, foot);
    for row in totals(book, lines, if unfinished { "Total owed so far" } else { "Total owed" }.into()) {
        section.push(row);
    }
    facts(&mut section, book, lines, "owed", during);
    section
}

/// A total row per commodity: what `lines` come to.
fn totals<'s>(book: &Book<'s>, lines: &[Line], label: Cell<'s>) -> Vec<Row<'s>> {
    let mut totals: BTreeMap<Id<Commodity>, Qty> = BTreeMap::new();
    for line in lines {
        *totals.entry(line.amount.unit).or_default() += line.amount.qty;
    }
    let row = |(unit, qty): (Id<Commodity>, Qty)| {
        let cells = [label.clone(), Cell::Blank, Cell::Blank, Cell::amount(book, Amount::new(qty, unit)), Cell::Blank];
        Row::new(cells).style(Style::Total)
    };
    totals.into_iter().map(row).collect()
}

/// Where a line comes from: the one flow behind it, or how many there are.
fn source<'s>(book: &Book<'s>, line: &Line) -> Cell<'s> {
    match line.contributions {
        1 => cause_cell(book, line.cause),
        many => Cell::Count(many, "source"),
    }
}

/// Lays lines out under a heading row for each system (and owner, when there
/// are several), indented by how deep the system sits in the jurisdiction tree.
fn grouped<'s>(
    section: &mut Section<'s>,
    book: &Book<'s>,
    lines: &[Line],
    several: bool,
    cells: impl Fn(&Line) -> Vec<Cell<'s>>,
    foot: impl Fn(&[Line], Cell<'s>) -> Vec<Row<'s>>,
) {
    let others = section.columns.len() - 1;
    for group in lines.chunk_by(|a, b| (a.owner, a.system) == (b.owner, b.system)) {
        let first = &group[0];
        let depth = first.system.map_or(0, |system| book.systems.depth(system) as usize);
        let system = first.system.map_or("project".into(), |system| Cell::Name(book.name(book.systems[system].path)));
        let heading = if several {
            Cell::list(" · ", [Cell::Name(book.name(book.entities[first.owner].path)), system.clone()])
        } else {
            system.clone()
        };
        let row = iter::once(heading).chain((0..others).map(|_| Cell::Blank));
        section.push(Row::new(row).depth(depth).style(Style::Total));
        for line in group {
            section.push(Row::new(cells(line)).depth(depth + 1));
        }
        for row in foot(group, system) {
            section.push(row.depth(depth + 1));
        }
    }
}
