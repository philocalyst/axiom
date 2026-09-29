//! `tax`: what the laws tallied and what they say is owed, for one year and entity.
//!
//! There is no tax engine here. Jurisdictions' laws `count` flows and gains
//! into named tallies and `owe` obligations; this view lays the effects of one
//! entity and year out under the systems that recorded them and keeps each
//! line's source, so `why` can trace it back.

use std::collections::BTreeMap;
use std::iter;

use axiom_core::{Day, Diagnostic, Id, Map, Qty, Sym};
use axiom_engine::{Cause, Effect, Owed, Run};
use axiom_model::{Amount, Book, Commodity, Entity, System};

use crate::resolve;
use crate::table::cause_cell;
use crate::{Cell, Column, Report, Row, Section, Style};

pub fn view<'s>(book: &Book<'s>, run: &Run, year: Option<i32>, entity: Option<&str>) -> Result<Report<'s>, Diagnostic> {
    let year = year.unwrap_or_else(|| run.today.year());
    let entity = match entity {
        Some(text) => resolve::entity(book, text)?,
        None => book.roots.me,
    };
    // An effect belongs to the year of the day it was recorded.
    let effects = run.effects.iter().filter(|effect| effect.owner == entity && effect.day.year() == year);
    let (owed, tallied): (Vec<&Effect>, Vec<&Effect>) = effects.partition(|effect| effect.owe.is_some());

    let mut tallies = tallies(book, &lines(&tallied));
    let mut obligations = obligations(book, &lines(&owed));
    if tallies.rows.is_empty() && obligations.rows.is_empty() {
        tallies.note(
            "Nothing was counted or owed. Laws count and owe only for entities \
             that live under a system that declares them.",
        );
    } else {
        obligations.note("Trace any line with `axiom why NAME`, or `axiom why FILE:LINE` from its source.");
    }
    let title = format!("Taxes {year} for {}", book.name(book.entities[entity].path));
    Ok(Report::new(title).with(tallies).with(obligations))
}

/// Everything counted or owed under one name. A tally is one line on a
/// person's year however many systems add to it; it sits under the first.
struct Line {
    system: Option<Id<System>>,
    name: Sym,
    /// Who is owed, and by when; `None` for a tally.
    owed: Option<Owed>,
    amount: Amount,
    contributions: usize,
    /// The first contribution's cause: the only one when there is just one.
    cause: Cause,
}

/// The effects merged into lines, grouped by system in tree order, and by
/// first appearance within a system. Systems record lines in the order they
/// work through them, so that order reads as the return does.
fn lines(effects: &[&Effect]) -> Vec<Line> {
    let mut lines: Vec<Line> = Vec::new();
    let mut at: Map<(Sym, Option<(Id<Entity>, Day)>, Id<Commodity>), usize> = Map::default();
    for effect in effects {
        let key = (effect.name, effect.owe.map(|owed| (owed.to, owed.due)), effect.amount.unit);
        let index = *at.entry(key).or_insert_with(|| {
            lines.push(Line {
                system: effect.system,
                name: effect.name,
                owed: effect.owe,
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
    lines.sort_by_key(|line| line.system);
    lines
}

fn tallies<'s>(book: &Book<'s>, lines: &[Line]) -> Section<'s> {
    let mut section =
        Section::new([Column::left("Tally"), Column::right("Amount"), Column::left("From")]).headed("Counted");
    grouped(&mut section, book, lines, |line| {
        vec![Cell::text(book.name(line.name)), Cell::amount(book, line.amount), source(book, line)]
    });
    section
}

/// Obligations, with what they come to.
fn obligations<'s>(book: &Book<'s>, lines: &[Line]) -> Section<'s> {
    let columns =
        [Column::left("Owed"), Column::left("To"), Column::left("Due"), Column::right("Amount"), Column::left("From")];
    let mut section = Section::new(columns).headed("Owed");
    grouped(&mut section, book, lines, |line| {
        let (to, due) = line.owed.map_or((Cell::Blank, Cell::Blank), |owed| {
            (Cell::text(book.name(book.entities[owed.to].path)), Cell::Day(owed.due))
        });
        vec![Cell::text(book.name(line.name)), to, due, Cell::amount(book, line.amount), source(book, line)]
    });

    let mut totals: BTreeMap<Id<Commodity>, Qty> = BTreeMap::new();
    for line in lines {
        *totals.entry(line.amount.unit).or_default() += line.amount.qty;
    }
    for (unit, qty) in totals {
        let total = [
            Cell::text("Total owed"),
            Cell::Blank,
            Cell::Blank,
            Cell::amount(book, Amount::new(qty, unit)),
            Cell::Blank,
        ];
        section.push(Row::new(total).style(Style::Total));
    }
    section
}

/// Where a line comes from: the one flow behind it, or how many there are.
fn source<'s>(book: &Book<'s>, line: &Line) -> Cell<'s> {
    match line.contributions {
        1 => cause_cell(book, line.cause),
        many => Cell::text(format!("{many} sources")),
    }
}

/// Lays lines out under a heading row for each system, indented by how deep
/// the system sits in the jurisdiction tree.
fn grouped<'s>(section: &mut Section<'s>, book: &Book<'s>, lines: &[Line], cells: impl Fn(&Line) -> Vec<Cell<'s>>) {
    let others = section.columns.len() - 1;
    let mut current = None;
    for line in lines {
        let depth = line.system.map_or(0, |system| book.systems.depth(system) as usize);
        if current != Some(line.system) {
            current = Some(line.system);
            let heading = line.system.map_or("project", |system| book.name(book.systems[system].path));
            let row = iter::once(Cell::text(heading)).chain((0..others).map(|_| Cell::Blank));
            section.push(Row::new(row).depth(depth).style(Style::Total));
        }
        section.push(Row::new(cells(line)).depth(depth + 1));
    }
}
