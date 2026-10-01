//! `register`: one place's flows, dated, with a running balance.
//!
//! Amounts and balances are in the place's display sign, the way a statement
//! shows them: what a card owes is positive, and a charge adds to it.

use std::collections::BTreeMap;

use axiom_core::{Day, Diagnostic, Id, Qty};
use axiom_engine::{Pad, Run, State};
use axiom_model::{
    Amount, Asset, Book, Commodity, Contract, Derivation, Entity, Object, Origin, Place, Role,
    TermsState,
};

use crate::history::{Change, Posting, pad_ends, postings};
use crate::lens::{Lens, Whose};
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
    view_with_lens(
        Lens::new(book, whose, to.unwrap_or(run.today)),
        run,
        place,
        from,
        to,
    )
}

/// Builds a register using owner scope and display signs from the shared lens.
pub(crate) fn view_with_lens<'s>(
    lens: Lens<'_, 's>,
    run: &Run,
    place: &str,
    from: Option<Day>,
    to: Option<Day>,
) -> Result<Report<'s>, Diagnostic> {
    let book = lens.book;
    if let Ok(entity) = book.entity(place) {
        return Ok(entity_view(lens, run, entity, place, from, to));
    }
    if let Some(asset) = book.asset(place) {
        return Ok(asset_register(lens, run, asset, from, to));
    }
    if let Some(contract) = book.contract(place) {
        return Ok(contract_register(lens, run, contract, from, to));
    }
    let place = resolve::place(book, place)?;
    // A place is somebody's: another owner's register is not part of whose money this is.
    let owner = book.places[place].owner;
    let register = match lens.whose.includes(owner) {
        true => section_with_sign(
            book,
            run,
            place,
            from,
            to,
            lens.display_sign(place),
            Some(lens.whose),
        ),
        false => Section::note_only(format!(
            "{} belongs to {}, whose money this is not.",
            path(book, place),
            book.name(book.entities[owner].path)
        )),
    };
    Ok(Report::new(format!("Register: {}", path(book, place))).with(register))
}

/// A party or owner register is a history of everything it touched, including
/// flows it owns that have no account end under its name.
fn entity_view<'s>(
    lens: Lens<'_, 's>,
    run: &Run,
    entity: axiom_core::Id<Entity>,
    target: &str,
    from: Option<Day>,
    to: Option<Day>,
) -> Report<'s> {
    let book = lens.book;
    let cutoff = to.unwrap_or(run.today);
    let is_owner = book.entities[entity]
        .place
        .is_some_and(|place| matches!(book.places[place].role, Role::Holding(_)));
    if is_owner && !lens.whose.includes(entity) {
        return Report::new(format!("Register: {target}")).with(Section::note_only(format!(
            "{} is outside this owner's scope.",
            book.name(book.entities[entity].path)
        )));
    }
    let mut section = Section::new([
        Column::left("Date"),
        Column::left("Purpose"),
        Column::left("Flow"),
        Column::right("Amount"),
        Column::left("Note"),
        Column::left("State"),
        Column::left("From"),
    ]);
    for posting in postings(book, run).filter(|posting| {
        let flow = posting.flow;
        let within = from.is_none_or(|from| flow.day >= from) && flow.day <= cutoff;
        let at_party = flow.payee == Some(entity)
            || [flow.from, flow.to].into_iter().any(|place| {
                let place = &book.places[place];
                place.owner == entity
                    || matches!(place.role, Role::Outside(Some(party)) if party == entity)
            });
        within && lens.whose.includes(flow.owner) && (flow.owner == entity || at_party)
    }) {
        let flow = posting.flow;
        let purpose = flow.purpose.map_or(Cell::Blank, |purpose| {
            Cell::Purpose(book.name(book.purposes[purpose.purpose].name))
        });
        let mut note = book
            .flow_view(flow)
            .codes()
            .map(|code| Cell::Code(book.name(code)))
            .collect::<Vec<_>>();
        if let Some(description) = flow.description {
            note.push(Cell::text(book.name(description)));
        }
        if let Some(doc) = book.txns[flow.txn].doc {
            if let Some(headline) = crate::table::doc_headline(book, Some(doc)) {
                note.push(Cell::text(headline));
            }
        }
        let state = match posting.posted.state {
            State::Actual => Cell::Word("actual"),
            State::Pending => Cell::Word("pending"),
            State::Settled(day) => Cell::text(format!("settled {day}")),
            State::Void => Cell::Word("void"),
            State::Returned(day) => Cell::text(format!("returned {day}")),
            State::Planned => Cell::Word("planned"),
        };
        section.push(Row::new([
            Cell::Day(flow.day),
            purpose,
            Cell::text(crate::places::route(book, flow)),
            Cell::amount(book, posting.out()),
            Cell::list_or_blank(" · ", note),
            state,
            Cell::Source(flow.loc),
        ]));
    }
    if section.rows.is_empty() {
        section.note(format!("No flows touch {target} in this window."));
    }
    Report::new(format!("Register: {target}")).with(section)
}

fn asset_register<'s>(
    lens: Lens<'_, 's>,
    run: &Run,
    asset_id: axiom_core::Id<Asset>,
    from: Option<Day>,
    to: Option<Day>,
) -> Report<'s> {
    let book = lens.book;
    let asset = &book.assets[asset_id];
    let name = book.name(asset.name);
    if !lens.whose.includes(asset.owner) {
        return Report::new(format!("Register: {name}")).with(Section::note_only(format!(
            "{name} belongs to {}, whose money this is not.",
            book.name(book.entities[asset.owner].path)
        )));
    }
    let cutoff = to.unwrap_or(run.today);
    let mut rows = Vec::new();
    for (id, flow) in book.flows.iter().filter(|(_, flow)| {
        in_window(flow.day, from, cutoff)
            && lens.whose.includes(flow.owner)
            && (flow.from == asset.place
                || flow.to == asset.place
                || flow.purpose.is_some_and(|purpose| purpose.of == Some(Object::Asset(asset_id)))
                || matches!(flow.origin, Origin::Derived(Derivation::Disposal(found)) if found == asset_id))
    }) {
        let posting = Posting::at(book, run, id);
        let (activity, style) = match flow.origin {
            Origin::Derived(Derivation::Disposal(_)) => ("disposed", Style::Alert),
            Origin::Derived(_) => ("derived", Style::Muted),
            Origin::Occurrence(_) => ("contract occurrence", Style::Muted),
            Origin::Written => ("flow", Style::Normal),
        };
        let purpose = flow.purpose.map_or(Cell::Blank, |purpose| {
            Cell::Purpose(book.name(book.purposes[purpose.purpose].name))
        });
        rows.push((
            flow.day,
            Row::new([
                Cell::Day(flow.day),
                Cell::Word(activity),
                purpose,
                Cell::text(crate::places::route(book, flow)),
                Cell::amount(book, posting.out()),
                Cell::Source(flow.loc),
            ])
            .style(style),
        ));
    }
    for adjustment in run.adjustments.iter().filter(|adjustment| {
        adjustment.day <= cutoff
            && from.is_none_or(|from| adjustment.day >= from)
            && matches!(adjustment.kind, axiom_engine::AdjustmentKind::Consumed { asset, .. } if asset == asset_id)
    }) {
        rows.push((
            adjustment.day,
            Row::new([
                Cell::Day(adjustment.day),
                Cell::Word("basis consumed"),
                Cell::Name(book.name(book.laws[adjustment.law].name)),
                Cell::Blank,
                Cell::base(book, adjustment.amount),
                Cell::Source(book.laws[adjustment.law].loc),
            ]),
        ));
    }
    rows.sort_by_key(|(day, _)| *day);
    let mut section = Section::new([
        Column::left("Date"),
        Column::left("Activity"),
        Column::left("Purpose or law"),
        Column::left("Flow"),
        Column::right("Amount"),
        Column::left("From"),
    ]);
    section.rows.extend(rows.into_iter().map(|(_, row)| row));
    if section.rows.is_empty() {
        section.note(format!("Nothing happened to {name} in this window."));
    }
    Report::new(format!("Register: {name}")).with(section)
}

fn contract_register<'s>(
    lens: Lens<'_, 's>,
    run: &Run,
    contract_id: axiom_core::Id<Contract>,
    from: Option<Day>,
    to: Option<Day>,
) -> Report<'s> {
    let book = lens.book;
    let contract = &book.contracts[contract_id];
    let name = book.name(contract.name);
    if !lens.whose.includes(contract.owner) {
        return Report::new(format!("Register: {name}")).with(Section::note_only(format!(
            "{name} belongs to {}, whose money this is not.",
            book.name(book.entities[contract.owner].path)
        )));
    }
    let cutoff = to.unwrap_or(run.today);
    let mut rows = Vec::new();
    for (days, terms) in contract.terms.within(contract.days) {
        let day = days.first();
        if !in_window(day, from, cutoff) {
            continue;
        }
        let statement = terms.change.map_or(Cell::Source(contract.loc), |change| {
            Cell::Source(change.loc)
        });
        let activity = match terms.state {
            TermsState::Active => Cell::Word("terms active"),
            TermsState::Waived => Cell::Word("terms waived"),
        };
        rows.push((
            day,
            Row::new([
                Cell::Day(day),
                activity,
                crate::contracts::terms_cell(book, contract, terms),
                Cell::Blank,
                statement,
            ]),
        ));
    }
    for promise in run
        .promises
        .iter()
        .filter(|promise| promise.contract == contract_id && in_window(promise.due, from, cutoff))
    {
        let kept = promise.kept.map(|(day, _)| day);
        let late = promise.late(cutoff);
        let mut row = Row::new([
            Cell::Day(promise.due),
            Cell::Word(if late > 0 { "late promise" } else { "due" }),
            kept.map_or(Cell::Blank, Cell::Day),
            Cell::Blank,
            kept.map_or(Cell::Source(contract.loc), |day| Cell::Day(day)),
        ]);
        if late > 0 {
            row = row.style(Style::Alert);
        }
        rows.push((promise.due, row));
    }
    for (id, flow) in book.flows.iter().filter(|(_, flow)| {
        in_window(flow.day, from, cutoff)
            && lens.whose.includes(flow.owner)
            && contract_flow(flow.origin, contract_id)
    }) {
        let posting = Posting::at(book, run, id);
        let activity = match flow.origin {
            Origin::Occurrence(_) => "occurrence",
            Origin::Derived(Derivation::Interest(_)) => "interest",
            Origin::Derived(Derivation::Principal(_)) => "principal",
            Origin::Derived(Derivation::Claim(_)) => "claim",
            Origin::Derived(Derivation::Otherwise(_)) => "late fee",
            Origin::Derived(Derivation::Refund(_)) => "refund",
            Origin::Derived(_) => "derived",
            Origin::Written => "flow",
        };
        let purpose = flow.purpose.map_or(Cell::Blank, |purpose| {
            Cell::Purpose(book.name(book.purposes[purpose.purpose].name))
        });
        rows.push((
            flow.day,
            Row::new([
                Cell::Day(flow.day),
                Cell::Word(activity),
                purpose,
                Cell::text(crate::places::route(book, flow)),
                Cell::list(
                    " ",
                    [Cell::amount(book, posting.out()), Cell::Source(flow.loc)],
                ),
            ]),
        ));
    }
    rows.sort_by_key(|(day, _)| *day);
    let mut section = Section::new([
        Column::left("Date"),
        Column::left("Activity"),
        Column::left("Terms or purpose"),
        Column::left("Flow or kept"),
        Column::left("Amount or source"),
    ]);
    section.rows.extend(rows.into_iter().map(|(_, row)| row));
    if section.rows.is_empty() {
        section.note(format!("Nothing happened under {name} in this window."));
    }
    Report::new(format!("Register: {name}")).with(section)
}

fn contract_flow(origin: Origin, contract: axiom_core::Id<Contract>) -> bool {
    match origin {
        Origin::Occurrence(found) => found == contract,
        Origin::Derived(
            Derivation::Interest(found)
            | Derivation::Principal(found)
            | Derivation::Claim(found)
            | Derivation::Otherwise(found)
            | Derivation::Refund(found),
        ) => found == contract,
        _ => false,
    }
}

fn in_window(day: Day, from: Option<Day>, cutoff: Day) -> bool {
    day <= cutoff && from.is_none_or(|from| day >= from)
}

/// The flows touching `place` from `from` to `to` (default: everything up to
/// the run's day), each with the balance after it.
///
/// The running balance counts what is real at the end of the window. Pending,
/// void and returned flows are listed, muted, and leave it alone. A flow into
/// or out of `PLACE.basis` is listed with the change it made to the basis, and
/// leaves the balance alone: no quantity moved.
pub fn section<'s>(
    book: &Book<'s>,
    run: &Run,
    place: Id<Place>,
    from: Option<Day>,
    to: Option<Day>,
) -> Section<'s> {
    section_with_sign(
        book,
        run,
        place,
        from,
        to,
        book.places[place].class.display_sign(),
        None,
    )
}

/// Builds a register for a place within an owner's view.
pub(crate) fn section_for<'s>(
    book: &'s Book<'s>,
    run: &Run,
    place: Id<Place>,
    from: Option<Day>,
    to: Option<Day>,
    whose: &Whose,
) -> Section<'s> {
    section_with_sign(
        book,
        run,
        place,
        from,
        to,
        book.places[place].class.display_sign(),
        Some(whose),
    )
}

fn section_with_sign<'s>(
    book: &Book<'s>,
    run: &Run,
    place: Id<Place>,
    from: Option<Day>,
    to: Option<Day>,
    sign: i64,
    whose: Option<&Whose>,
) -> Section<'s> {
    let steps = steps(book, run, place, to.unwrap_or(run.today), whose);
    let split = from.map_or(0, |from| steps.partition_point(|step| step.day < from));
    let shown =
        |qty: Qty, unit: Id<Commodity>| Cell::amount(book, Amount::new(Qty(qty.0 * sign), unit));

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
            payee.map_or(Cell::Blank, |entity| {
                Cell::text(book.name(book.entities[entity].path))
            }),
            note(book, step).unwrap_or(Cell::Blank),
            amount,
            balance,
        ];
        section.push(Row::new(cells).style(if step.counts {
            Style::Normal
        } else {
            Style::Muted
        }));
    }

    if section.rows.is_empty() {
        section.note(format!(
            "Nothing touches {} in this window.",
            path(book, place)
        ));
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
fn steps<'a>(
    book: &'a Book,
    run: &'a Run,
    place: Id<Place>,
    cutoff: Day,
    whose: Option<&Whose>,
) -> Vec<Step<'a>> {
    let flows = book.touching[place].iter().flat_map(|&id| {
        let posting = Posting::at(book, run, id);
        let in_scope = whose.is_none_or(|whose| whose.includes(posting.flow.owner));
        posting
            .changes_at(place)
            .filter(move |_| in_scope)
            .map(move |change| Step {
                day: posting.flow.day,
                change,
                counts: posting.is_real_on(cutoff),
                with: posting.counterparty(place),
                source: Source::Flow(posting),
            })
    });
    let pads = run.pads.iter().flat_map(|pad| {
        let in_scope = whose.is_none_or(|whose| whose.governs(book, pad.subject));
        let with = if pad.place == place {
            pad.counter
        } else {
            pad.place
        };
        let here = pad_ends(pad)
            .into_iter()
            .filter(move |&(at, _)| at == place && in_scope);
        here.map(move |(_, moved)| Step {
            day: pad.day,
            change: Change::Moved(moved),
            counts: true,
            with,
            source: Source::Gap(pad),
        })
    });
    let mut steps: Vec<Step> = flows
        .chain(pads)
        .filter(|step| step.day <= cutoff)
        .collect();
    steps.sort_by_key(|step| step.day);
    steps
}

/// A change of basis, codes, and settlement, as one line of small print.
fn note<'s>(book: &Book<'s>, step: &Step<'_>) -> Option<Cell<'s>> {
    let rebased = match step.change {
        Change::Rebased(by) => Some(Cell::text(format!(
            "basis {}{}",
            if by.qty.is_negative() { "" } else { "+" },
            book.show(by)
        ))),
        Change::Moved(_) => None,
    };
    let details: Vec<Cell<'s>> = match step.source {
        Source::Gap(pad) => vec![Cell::text(gap_words(book, pad))],
        Source::Flow(posting) => {
            let status = match posting.posted.state {
                State::Actual | State::Planned => None,
                State::Pending => Some(Cell::Word("pending")),
                State::Void => Some(Cell::Word("void")),
                State::Settled(on) if !step.counts => {
                    Some(Cell::text(format!("pending until {on}")))
                }
                State::Settled(on) => Some(Cell::text(format!("settled {on}"))),
                State::Returned(on) => Some(Cell::text(format!("returned {on}"))),
            };
            let flow = book.flow_view(posting.flow);
            code_labels(book, flow.codes()).chain(status).collect()
        }
    };
    let parts = rebased.into_iter().chain(details).collect::<Vec<_>>();
    (!parts.is_empty()).then(|| Cell::list(" · ", parts))
}
