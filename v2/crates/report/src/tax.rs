//! `tax`: what the laws tallied and what they say is owed, for one year.
//!
//! There is no tax engine here. Jurisdictions' laws `count` flows and gains
//! into named tallies and `owe` obligations; this view lays the effects of a
//! year out under the systems that recorded them, with what each jurisdiction
//! is owed, and keeps each line's source, so `why` can trace it back.

use std::collections::{BTreeMap, BTreeSet};
use std::iter;

use axiom_core::{Day, Id, Map, Qty, Sym};
use axiom_engine::{Cause, Effect, Owed, Run};
use axiom_model::{Amount, Book, Commodity, Entity, System};

use crate::closings;
use crate::lens::Lens;
use crate::table::{cause_cell, plural};
use crate::{Cell, Column, Report, Row, Section, Style};

pub(crate) fn view_with_lens<'s>(
    lens: Lens<'s, '_, '_, '_>,
    run: &Run,
    year: Option<i32>,
) -> Report<'s> {
    let book = lens.book();
    let year = year.unwrap_or_else(|| run.today.year());
    // An effect belongs to the year of the day it was recorded.
    let effects = run
        .effects
        .iter()
        .filter(|effect| lens.owns_entity(effect.owner) && effect.day.year() == year);
    let (owed, tallied): (Vec<&Effect>, Vec<&Effect>) =
        effects.partition(|effect| effect.owed().is_some());
    let owners: BTreeSet<Id<Entity>> = owed
        .iter()
        .chain(&tallied)
        .map(|effect| effect.owner)
        .collect();

    let (tallied, owed) = (lines(&tallied), lines(&owed));
    // A closing law has not judged the year until its day: what it owes is
    // missing, not nothing, and what was counted is counted so far.
    let closes = pending_closings(lens, run, year);
    let several = owners.len() > 1;
    let mut tallies = tallies(book, &tallied, several);
    let mut obligations = obligations(book, &owed, several, !closes.is_empty());
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
            "Nothing was counted or owed. Laws count and owe only for entities \
             that live under a system that declares them.",
        );
    } else {
        let listed = if obligations.rows.is_empty() {
            &mut tallies
        } else {
            &mut obligations
        };
        listed.note(
            "Trace any line with `axiom why NAME`, or `axiom why FILE:LINE` from its source.",
        );
    }
    if owed.iter().any(|line| line.priced) {
        obligations.note(
            "A penalty is the price of a violated law: it is owed instead of the law failing.",
        );
    }
    let title = match owners.iter().collect::<Vec<_>>()[..] {
        [owner] => format!("Taxes {year} for {}", book.name(book.entities[*owner].path)),
        _ => format!("Taxes {year}"),
    };
    Report::new(title).with(tallies).with(obligations)
}

/// The days after the run's end on which closing laws written for `whose`
/// will judge `year`.
fn pending_closings(lens: Lens<'_, '_, '_, '_>, run: &Run, year: i32) -> Vec<Day> {
    let mut days = closings::days_for(lens.book(), year, |rule| lens.governs(rule.subject));
    days.retain(|&day| day > run.horizon);
    days
}

/// Why a year has no return yet: the days its closing laws will judge it.
fn not_closed(year: i32, closes: &[Day]) -> String {
    let days: Vec<String> = closes.iter().map(Day::to_string).collect();
    let days = days.join(" and ");
    match closes {
        [_] => format!(
            "The {year} return closes on {days}; what it owes is not figured yet; the tallies are counted so far."
        ),
        _ => format!(
            "The {year} returns close on {days}; what they owe is not figured yet; the tallies are counted so far."
        ),
    }
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
    let mut at: Map<
        (
            Id<Entity>,
            Sym,
            Option<(Id<Entity>, Day)>,
            bool,
            Id<Commodity>,
        ),
        usize,
    > = Map::default();
    for effect in effects {
        let key = (
            effect.owner,
            effect.name,
            effect.owed().map(|owed| (owed.to, owed.due)),
            effect.is_penalty(),
            effect.amount.unit,
        );
        let index = *at.entry(key).or_insert_with(|| {
            lines.push(Line {
                owner: effect.owner,
                system: effect.system,
                name: effect.name,
                owed: effect.owed(),
                priced: effect.is_penalty(),
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

fn tallies<'s>(book: &'s Book<'_>, lines: &[Line], several: bool) -> Section<'s> {
    let mut section = Section::new([
        Column::left("Tally"),
        Column::right("Amount"),
        Column::left("From"),
    ])
    .headed("Counted");
    let cells = |line: &Line| {
        vec![
            Cell::text(book.name(line.name)),
            Cell::amount(book, line.amount),
            source(book, line),
        ]
    };
    grouped(&mut section, book, lines, several, cells, |_, _| Vec::new());
    section
}

/// Obligations, and what each jurisdiction is owed. `unfinished`: a law that
/// owes has not judged the year yet, so the totals are what is owed so far.
fn obligations<'s>(
    book: &'s Book<'_>,
    lines: &[Line],
    several: bool,
    unfinished: bool,
) -> Section<'s> {
    let columns = [
        Column::left("Owed"),
        Column::left("To"),
        Column::left("Due"),
        Column::right("Amount"),
        Column::left("From"),
    ];
    let mut section = Section::new(columns).headed("Owed");
    let cells = |line: &Line| {
        let (to, due) = line.owed.map_or((Cell::Blank, Cell::Blank), |owed| {
            (
                Cell::text(book.name(book.entities[owed.to].path)),
                Cell::Day(owed.due),
            )
        });
        let name = book.name(line.name);
        let name = if line.priced {
            format!("{name} (penalty)")
        } else {
            name.to_string()
        };
        vec![
            Cell::text(name),
            to,
            due,
            Cell::amount(book, line.amount),
            source(book, line),
        ]
    };
    // Each jurisdiction is owed its own total, when there is more than one to tell apart.
    let several_groups = lines
        .chunk_by(|a, b| (a.owner, a.system) == (b.owner, b.system))
        .nth(1)
        .is_some();
    let foot = |group: &[Line], jurisdiction: &str| {
        if several_groups {
            totals(book, group, &format!("Total {jurisdiction}"))
        } else {
            Vec::new()
        }
    };
    grouped(&mut section, book, lines, several, cells, foot);
    for row in totals(
        book,
        lines,
        if unfinished {
            "Total owed so far"
        } else {
            "Total owed"
        },
    ) {
        section.push(row);
    }
    section
}

/// A total row per commodity: what `lines` come to.
fn totals<'s>(book: &'s Book<'_>, lines: &[Line], label: &str) -> Vec<Row<'s>> {
    let mut totals: BTreeMap<Id<Commodity>, Qty> = BTreeMap::new();
    for line in lines {
        *totals.entry(line.amount.unit).or_default() += line.amount.qty;
    }
    let row = |(unit, qty): (Id<Commodity>, Qty)| {
        let cells = [
            Cell::text(label.to_string()),
            Cell::Blank,
            Cell::Blank,
            Cell::amount(book, Amount::new(qty, unit)),
            Cell::Blank,
        ];
        Row::new(cells).style(Style::Total)
    };
    totals.into_iter().map(row).collect()
}

/// Where a line comes from: the one flow behind it, or how many there are.
fn source<'s>(book: &'s Book<'_>, line: &Line) -> Cell<'s> {
    match line.contributions {
        1 => cause_cell(book, line.cause),
        many => Cell::text(plural(many, "source")),
    }
}

/// Lays lines out under a heading row for each system (and owner, when there
/// are several), indented by how deep the system sits in the jurisdiction tree.
fn grouped<'s>(
    section: &mut Section<'s>,
    book: &'s Book<'_>,
    lines: &[Line],
    several: bool,
    cells: impl Fn(&Line) -> Vec<Cell<'s>>,
    foot: impl Fn(&[Line], &str) -> Vec<Row<'s>>,
) {
    let others = section.columns.len() - 1;
    for group in lines.chunk_by(|a, b| (a.owner, a.system) == (b.owner, b.system)) {
        let first = &group[0];
        let depth = first
            .system
            .map_or(0, |system| book.systems.depth(system) as usize);
        let system = first
            .system
            .map_or("project", |system| book.name(book.systems[system].path));
        let heading = if several {
            format!("{} · {system}", book.name(book.entities[first.owner].path))
        } else {
            system.to_string()
        };
        let row = iter::once(Cell::text(heading)).chain((0..others).map(|_| Cell::Blank));
        section.push(Row::new(row).depth(depth).style(Style::Total));
        for line in group {
            section.push(Row::new(cells(line)).depth(depth + 1));
        }
        for row in foot(group, system) {
            section.push(row.depth(depth + 1));
        }
    }
}
