//! `why #PURPOSE`: the rules, budget and flows classified by a purpose.

use std::collections::HashMap;

use axiom_core::{Day, Days, Id, Qty, spread};
use axiom_engine::{Headroom, Run};
use axiom_model::{Amount, Book, Limit, Period, Purpose, PurposeRoot};

use crate::calendar::Periods;
use crate::headroom::{current, latest, room, window_words};
use crate::history::postings;
use crate::lens::{Lens, Whose};
use crate::places::path;
use crate::resolve;
use crate::table::year_days;
use crate::{Cell, Column, Report, Row, Section, Style};

/// Resolves a purpose and gathers its rules, budgets, year total and parties.
pub fn report<'s>(
    book: &'s Book<'_>,
    run: &Run,
    whose: &Whose,
    target: &str,
) -> Result<Report<'s>, axiom_core::Diagnostic> {
    let purpose = book.purpose(target).map_err(|_| {
        resolve::nothing_named(
            "purpose",
            target,
            book.purposes
                .values()
                .map(|purpose| book.name(purpose.name)),
        )
    })?;
    let year = run.today.year();
    let year_window = year_days(year).unwrap_or(Days::ALWAYS);
    let cutoff = year_window.last().min(run.today);
    let period = Periods::covering(Period::Year, year_window.first(), cutoff);
    let lens = Lens::new(book, whose, cutoff);

    let mut laws = Vec::new();
    for ancestor in book.purposes.lineage(purpose) {
        laws.extend(book.purposes[ancestor].laws.iter().copied());
    }
    laws.sort_unstable();
    laws.dedup();

    let mut about =
        Section::new([Column::left("Purpose"), Column::left("Value")]).headed("Purpose");
    let item = &book.purposes[purpose];
    about.push(Row::new([
        Cell::Name(book.name(item.name)),
        Cell::Word(root_name(item.root)),
    ]));
    if let Some(doc) = item.doc {
        for line in crate::table::doc_lines(book.name(doc)) {
            about.note(line);
        }
    }

    let all_headroom = current(book, run, year_window.first(), cutoff);
    let governing = laws
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    let readings = latest(
        all_headroom
            .iter()
            .filter(|reading| governing.contains(&reading.law) && whose.includes(reading.owner)),
    );
    let mut limits = Section::new([
        Column::left("Law"),
        Column::left("Window"),
        Column::right("Counted"),
        Column::right("Limit"),
        Column::right("Left"),
    ])
    .headed("Headroom");
    for reading in readings {
        limits.push(headroom_row(book, reading));
    }
    if limits.rows.is_empty() {
        limits.note("No headroom has been recorded for this purpose this year.");
    }

    let budgets = budget_section(book, run, whose, purpose, year_window);
    let (total, parties, unpriced) = totals(book, run, lens, purpose, period, cutoff);
    let mut activity =
        Section::new([Column::left("This year"), Column::right("Amount")]).headed("Activity");
    activity.push(Row::new([
        Cell::Name(book.name(item.name)),
        Cell::base(book, total),
    ]));
    activity.fact(
        root_fact(item.root),
        Some(book.name(item.name)),
        whose.label(book),
        crate::When::During(period.window(0).days()),
        crate::Money::base(book, total),
    );
    let mut largest =
        Section::new([Column::left("Party"), Column::right("Amount")]).headed("Largest parties");
    for (name, amount) in parties.into_iter().take(10) {
        largest.push(Row::new([Cell::Name(name), Cell::base(book, amount)]));
    }
    if largest.rows.is_empty() {
        largest.note("No priced flows of this purpose this year.");
    }
    activity.unpriced(unpriced, "flow");

    Ok(Report::new(format!("Why #{}", book.name(item.name)))
        .with(about)
        .with(super::laws_table(book, &laws))
        .with(limits)
        .with(budgets)
        .with(activity)
        .with(largest))
}

fn budget_section<'s>(
    book: &'s Book<'_>,
    run: &Run,
    whose: &Whose,
    purpose: Id<Purpose>,
    days: Days,
) -> Section<'s> {
    let mut section = Section::new([
        Column::left("Purpose"),
        Column::left("Window"),
        Column::left("Limit"),
        Column::left("Carry"),
        Column::right("Counted"),
        Column::right("Left"),
    ])
    .headed("Budgets");
    let readings = current(book, run, days.first(), days.last());
    let budgets = book
        .budgets
        .values()
        .filter(|budget| book.purposes.covers(purpose, budget.purpose));
    for budget in budgets {
        for (stretch, limit) in budget.limits.within(days) {
            section.push(Row::new([
                Cell::Name(book.name(book.purposes[budget.purpose].name)),
                Cell::Period(stretch),
                budget_limit(book, *limit),
                Cell::Word(if budget.carries {
                    "carries"
                } else {
                    "within window"
                }),
                Cell::Blank,
                Cell::Blank,
            ]));
        }
        let mut matching = readings
            .iter()
            .filter(|reading| reading.law == budget.law && whose.includes(reading.owner))
            .collect::<Vec<_>>();
        matching.sort_by_key(|reading| reading.days.first());
        for reading in matching {
            section.push(Row::new([
                Cell::Name(book.name(book.purposes[budget.purpose].name)),
                Cell::text(window_words(reading)),
                budget_limit(book, *budget.limits.at(reading.day)),
                Cell::Word(if budget.carries {
                    "carries"
                } else {
                    "within window"
                }),
                Cell::amount(book, reading.counted),
                Cell::amount(book, Amount::new(room(reading), reading.limit.unit)),
            ]));
        }
    }
    if section.rows.is_empty() {
        section.note("No budget is declared for this purpose or its descendants.");
    }
    section
}

fn budget_limit<'s>(book: &'s Book<'_>, limit: Limit) -> Cell<'s> {
    match limit {
        Limit::Amount(amount) => Cell::amount(book, amount),
        Limit::Share { rate, of } => Cell::list(
            " ",
            [
                Cell::Percent(rate),
                Cell::Word("of"),
                Cell::Purpose(book.name(book.purposes[of].name)),
            ],
        ),
    }
}

fn headroom_row<'s>(book: &'s Book<'_>, reading: &Headroom) -> Row<'s> {
    Row::new([
        Cell::Name(book.name(book.laws[reading.law].name)),
        Cell::text(window_words(reading)),
        Cell::amount(book, reading.counted),
        Cell::amount(book, reading.limit),
        Cell::amount(book, Amount::new(room(reading), reading.limit.unit)),
    ])
    .style(if room(reading).is_negative() {
        Style::Alert
    } else {
        Style::Normal
    })
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
    let wanted_root = book.purposes[purpose].root;
    for posting in postings(book, run).filter(|posting| posting.is_real_on(cutoff)) {
        let flow = posting.flow;
        let Some(purpose_on_flow) = flow.purpose else {
            continue;
        };
        if !book.purposes.covers(purpose, purpose_on_flow.purpose)
            || !lens.whose.includes(flow.owner)
        {
            continue;
        }
        let Some(amount) = super::super::flow::movement_in_base(lens, posting, Some(wanted_root))
        else {
            unpriced += 1;
            continue;
        };
        let Some(happened) = Days::new(
            periods
                .window(0)
                .days()
                .first()
                .max(flow.recognized.first()),
            cutoff.min(flow.recognized.last()),
        ) else {
            continue;
        };
        let amount = spread(amount, flow.recognized, happened);
        total += amount;
        let other = if book.places[flow.from].class == axiom_model::Class::Outside {
            flow.from
        } else {
            flow.to
        };
        let name = flow.payee.map_or_else(
            || path(book, other),
            |entity| book.name(book.entities[entity].path),
        );
        *parties.entry(name).or_default() += amount;
    }
    let mut parties = parties.into_iter().collect::<Vec<_>>();
    parties.sort_by(|(left_name, left), (right_name, right)| {
        i128::from(right.0)
            .abs()
            .cmp(&i128::from(left.0).abs())
            .then_with(|| left_name.cmp(right_name))
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
