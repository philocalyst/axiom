//! Promises, watched: what a contract says today, and which occurrences are due
//! and not kept. `contracts`, `claims`, `available` and the forecast all read
//! them from here, so a late rent is late in each.

use axiom_core::{Day, Days, Id, Span};
use axiom_model::{Amount, Cadence, Class, Contract, Entity, Flow, Terms};

use crate::forecast::recurrence;
use crate::lens::Lens;
use crate::places::path;
use crate::{Cell, Column, Row, Section, Style};

/// The flow an occurrence is chiefly about: the largest of its template's.
pub fn main_flow(terms: &Terms) -> Option<&Flow> {
    terms.template.iter().max_by_key(|flow| flow.out.qty.abs())
}

/// Whether what the contract promises comes to the owners, rather than leaving them.
pub fn comes_in(lens: Lens, terms: &Terms) -> bool {
    main_flow(terms).is_some_and(|flow| lens.book.places[flow.from].class == Class::Outside)
}

/// A promise due and not kept: what it says, who is to blame, and how late.
pub struct Late {
    pub contract: Id<Contract>,
    pub due: Day,
    /// The party when the money is owed to the owners, else the owner.
    pub blame: Id<Entity>,
    pub coming_in: bool,
    pub amount: Amount,
}

impl Late {
    pub fn days(&self, at: Day) -> Span {
        at.since(self.due)
    }
}

/// Every occurrence due by the lens's day that the journal had not kept by
/// then, for the lens's owners, oldest first.
pub fn late(lens: Lens) -> Vec<Late> {
    let (book, at) = (lens.book, lens.day);
    let unkept = |kept: &Option<(Day, _)>| kept.is_none_or(|(day, _)| day > at);
    let due = lens.run.promises.iter().filter(|promise| promise.due <= at && unkept(&promise.kept));
    let mut late: Vec<Late> = due
        .filter_map(|promise| {
            let contract = &book.contracts[promise.contract];
            let terms = contract.terms_on(promise.due);
            let flow = main_flow(terms).filter(|_| lens.whose.includes(contract.owner))?;
            let coming_in = comes_in(lens, terms);
            let blame = if coming_in { contract.party } else { contract.owner };
            Some(Late { contract: promise.contract, due: promise.due, blame, coming_in, amount: flow.out })
        })
        .collect();
    late.sort_by_key(|late| (late.due, late.contract));
    late
}

/// An occurrence still to come: the contract, the day it falls due, and the
/// terms in force that day.
pub struct Coming<'b> {
    pub contract: Id<Contract>,
    pub due: Day,
    pub terms: &'b Terms,
}

/// What falls due after the lens's day and through `until`, for the lens's
/// owners, soonest first.
pub fn coming<'b>(lens: Lens<'b, '_>, until: Day) -> Vec<Coming<'b>> {
    let Some(ahead) = Days::new(lens.day.add_days(1), until) else { return Vec::new() };
    let owned = lens.book.contracts.iter().filter(|(_, contract)| lens.whose.includes(contract.owner));
    let due = owned.flat_map(|(id, contract)| {
        let occurrence = move |due| Coming { contract: id, due, terms: contract.terms_on(due) };
        contract.due_days(ahead).into_iter().map(occurrence)
    });
    let mut coming: Vec<Coming> = due.collect();
    coming.sort_by_key(|coming| (coming.due, coming.contract));
    coming
}

/// The promises late on the lens's day, as a table: what was promised, by
/// whom, since when, and who is to blame. Only those `keep` picks.
pub fn late_section<'s>(lens: Lens<'_, 's>, heading: &'static str, keep: impl Fn(&Late) -> bool) -> Section<'s> {
    let (book, at) = (lens.book, lens.day);
    let columns = ["Promise", "With", "Due"].map(Column::left).into_iter();
    let mut section =
        Section::new(columns.chain([Column::left("Late"), Column::left("Blames"), Column::right("Amount")]));
    for late in late(lens).iter().filter(|late| keep(late)) {
        let contract = &book.contracts[late.contract];
        let cells = [
            Cell::Name(book.name(contract.name)),
            Cell::Name(book.name(book.entities[contract.party].path)),
            Cell::Day(late.due),
            Cell::Span(late.days(at)),
            Cell::Name(book.name(book.entities[late.blame].path)),
            Cell::amount(book, late.amount),
        ];
        section.push(Row::new(cells).style(Style::Alert));
    }
    section.headed(heading)
}

/// Terms in words: `2,900.00 USD monthly from checking`.
pub fn shape<'s>(lens: Lens<'_, 's>, terms: &Terms) -> Cell<'s> {
    let Some(flow) = main_flow(terms) else { return "waived".into() };
    let cadence = match terms.every {
        Cadence::Every(span) => recurrence::describe(span),
        Cadence::TwiceMonthly => "twice monthly".into(),
    };
    // The holding the money leaves or joins: the end that is not a party's.
    let (word, holding) =
        if lens.book.places[flow.from].class == Class::Outside { ("into", flow.to) } else { ("from", flow.from) };
    [Cell::amount(lens.book, flow.out), cadence, word.into(), Cell::Name(path(lens.book, holding))].into()
}

/// The terms on `day`, and how they are to change: `120.00 USD monthly until
/// 05-31, then 240.00 USD monthly: spring promotion`.
pub fn terms_on<'s>(lens: Lens<'_, 's>, contract: &Contract, day: Day) -> Cell<'s> {
    let now = contract.terms_on(day);
    let mut parts = vec![shape(lens, now)];
    let stretch = contract.terms.within(Days::on(day)).next().map(|(days, _)| days.last());
    if let Some(until) = stretch.filter(|&until| until < Day::MAX) {
        let later = contract.terms_on(until.add_days(1));
        parts.extend(["until".into(), Cell::Day(until), ", then".into(), shape(lens, later)]);
        let why = now.change.or(later.change).and_then(|change| change.description);
        parts.extend(why.into_iter().flat_map(|why| [":".into(), Cell::Text(lens.book.name(why))]));
    }
    Cell::Join(" ", parts)
}
