//! `why #PURPOSE`: the rules, budget and flows classified by a purpose.

use std::collections::HashMap;

use axiom_core::{Day, Days, Id, Qty, spread};
use axiom_engine::{Headroom, Piece, Run};
use axiom_model::{Book, Budget, Flow, Law, Period, Purpose, PurposeRoot};

use crate::calendar::Periods;
use crate::flow::{Counted, for_each_counted};
use crate::headroom::{current, latest};
use crate::lens::Lens;
use crate::places::path;
use crate::resolve;
use crate::table::year_days;
use crate::{Cell, Column, Report, Row, Section};
use crate::{budget, limits};

/// Resolves a purpose and gathers its rules, budgets, year total and parties.
pub fn report<'s>(lens: Lens<'s, '_, '_, '_>, run: &Run, target: &str) -> Result<Report<'s>, axiom_core::Diagnostic> {
    let book = lens.book();
    let purpose = book.purpose(target).map_err(|_| {
        resolve::nothing_named("purpose", target, book.purposes.values().map(|purpose| book.name(purpose.name)))
    })?;
    let year_window = year_days(run.today.year()).unwrap_or(Days::ALWAYS);
    let cutoff = year_window.last().min(run.today);
    let lens = lens.on(cutoff);
    let laws = governing_laws(book, purpose);
    let (activity, largest) = activity_sections(lens, run, purpose, year_window, cutoff);
    // What the purpose's laws have counted against their limits this year, by the latest reading of each.
    let headroom = current(book, run, year_window.first(), cutoff);
    let governed = |reading: &&Headroom| laws.binary_search(&reading.law).is_ok() && lens.owns_entity(reading.owner);
    let limits = limits::section(book, latest(headroom.iter().filter(governed))).headed("Limits");
    let covers = |budget: &Budget| book.purposes.covers(purpose, budget.purpose);
    let nothing = "No budget is declared for this purpose or its descendants.";
    let budgets = budget::section(lens, run, cutoff, Period::Year, covers, nothing).headed("Budgets");
    Ok(Report::new(format!("Why #{}", book.name(book.purposes[purpose].name)))
        .with(about_section(book, &book.purposes[purpose]))
        .with(super::laws_table(book, &laws))
        .with(limits)
        .with(budgets)
        .with(activity)
        .with(largest))
}

/// The laws of a purpose and of every purpose it is within, each once.
fn governing_laws(book: &Book<'_>, purpose: Id<Purpose>) -> Vec<Id<Law>> {
    let mut laws = Vec::new();
    for ancestor in book.purposes.lineage(purpose) {
        laws.extend(book.purposes[ancestor].laws.iter().copied());
    }
    laws.sort_unstable();
    laws.dedup();
    laws
}

fn about_section<'s>(book: &'s Book<'_>, item: &Purpose) -> Section<'s> {
    let mut about = Section::new([Column::left("Purpose"), Column::left("Value")]).headed("Purpose");
    about.push(Row::new([Cell::Name(book.name(item.name)), Cell::Word(root_name(item.root))]));
    if let Some(doc) = item.doc {
        for line in crate::table::doc_lines(book.name(doc)) {
            about.note(Cell::Said(std::borrow::Cow::Owned(line.to_owned())));
        }
    }
    about
}

/// What the purpose came to this year, and the parties it came to most with.
fn activity_sections<'s>(
    lens: Lens<'s, '_, '_, '_>,
    run: &Run,
    purpose: Id<Purpose>,
    year_window: Days,
    cutoff: Day,
) -> (Section<'s>, Section<'s>) {
    let book = lens.book();
    let item = &book.purposes[purpose];
    let period = Periods::covering(Period::Year, year_window.first(), cutoff);
    let (total, parties, unpriced) = totals(book, run, lens, purpose, period, cutoff);
    let mut activity = Section::new([Column::left("This year"), Column::right("Amount")]).headed("Activity");
    activity.push(Row::new([Cell::Name(book.name(item.name)), Cell::base(book, total)]));
    activity.fact(
        root_fact(item.root),
        Some(book.name(item.name)),
        lens.whose.label(book),
        crate::When::During(period.window(0).days()),
        crate::Money::base(book, total),
    );
    let mut largest = Section::new([Column::left("Party"), Column::right("Amount")]).headed("Largest parties");
    for (name, amount) in parties.into_iter().take(10) {
        largest.push(Row::new([Cell::Name(name), Cell::base(book, amount)]));
    }
    if largest.rows.is_empty() {
        largest.note("No priced flows of this purpose this year.");
    }
    activity.unpriced(unpriced, "flow");
    (activity, largest)
}

fn totals<'s>(
    book: &'s Book<'_>,
    run: &Run,
    lens: Lens<'s, '_, '_, '_>,
    purpose: Id<Purpose>,
    periods: Periods,
    cutoff: Day,
) -> (Qty, Vec<(&'s str, Qty)>, usize) {
    let mut total = Qty::ZERO;
    let mut parties: HashMap<&'s str, Qty> = HashMap::new();
    let mut unpriced = 0;
    let of_purpose =
        |_: &Flow, piece: &Piece| piece.purpose.is_some_and(|counted| book.purposes.covers(purpose, counted.purpose));
    for_each_counted(lens, run, cutoff, of_purpose, |counted| {
        let Counted { flow, amount, recognized, .. } = counted;
        let Some(amount) = amount else {
            unpriced += 1;
            return;
        };
        let first = periods.window(0).days().first().max(recognized.first());
        let Some(happened) = Days::new(first, cutoff.min(recognized.last())) else { return };
        let amount = spread(amount, recognized, happened);
        total += amount;
        let other = if book.places[flow.from].class == axiom_model::Class::Outside { flow.from } else { flow.to };
        let name = flow.payee.map_or_else(|| path(book, other), |entity| book.name(book.entities[entity].path));
        *parties.entry(name).or_default() += amount;
    });
    let mut parties = parties.into_iter().collect::<Vec<_>>();
    parties.sort_by(|(left_name, left), (right_name, right)| {
        i128::from(right.0).abs().cmp(&i128::from(left.0).abs()).then_with(|| left_name.cmp(right_name))
    });
    (total, parties, unpriced)
}

fn root_name(root: PurposeRoot) -> &'static str {
    match root {
        PurposeRoot::Income => "income",
        PurposeRoot::Spending => "spending",
        PurposeRoot::Capital => "capital",
        PurposeRoot::Transfer => "transfer",
    }
}

fn root_fact(root: PurposeRoot) -> &'static str {
    root_name(root)
}
