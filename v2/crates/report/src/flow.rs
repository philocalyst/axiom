//! `flow`: income, spending, capital and transfer, grouped by purpose or party.
//!
//! Income and expense places are read as the change in their balances over
//! each period, priced in the base currency on the day each flow happened.
//! A flow spread over a range is recognized a little each day, so a year's
//! premium lands in every month it covers.

use std::collections::{BTreeMap, HashMap};

use axiom_core::{Day, Days, Id, Qty, Sym, spread};
use axiom_engine::Run;
use axiom_model::{
    Action, Book, Class, Commodity, Entity, Object, Period, Place, Purpose, PurposeRoot,
};

use crate::calendar::Periods;
use crate::history::{Posting, postings};
use crate::lens::{Lens, Whose};
use crate::places::path;
use crate::{Cell, Column, Money, Report, Row, Section, Style, When};

/// How many periods to show when the window is not given.
const DEFAULT_PERIODS: usize = 12;

pub fn view<'s>(
    book: &Book<'s>,
    run: &Run,
    whose: &Whose,
    by: Period,
    from: Option<Day>,
    to: Option<Day>,
) -> Report<'s> {
    let to = to.unwrap_or(run.today);
    view_with_lens(Lens::new(book, whose, to), run, by, from)
}

/// The same statement grouped by the other end of each flow.
pub fn view_by_party<'s>(
    book: &Book<'s>,
    run: &Run,
    whose: &Whose,
    from: Option<Day>,
    to: Option<Day>,
) -> Report<'s> {
    let cutoff = to.unwrap_or(run.today);
    view_by_party_with_lens(Lens::new(book, whose, cutoff), run, from, cutoff)
}

pub(crate) fn view_by_party_with_lens<'s>(
    lens: Lens<'_, 's>,
    run: &Run,
    from: Option<Day>,
    cutoff: Day,
) -> Report<'s> {
    let book = lens.book;
    let periods = match from {
        Some(from) => Periods::covering(Period::Month, from, cutoff),
        None => {
            let first = book
                .flows
                .as_slice()
                .first()
                .map_or(cutoff, |flow| flow.day.min(cutoff));
            Periods::covering(Period::Month, first, cutoff).last(DEFAULT_PERIODS)
        }
    };
    let mut values: HashMap<(PurposeRoot, Party), Vec<Qty>> = HashMap::new();
    let mut unpriced = 0;
    for posting in postings(book, run).filter(|posting| posting.is_real_on(cutoff)) {
        let flow = posting.flow;
        if !lens.whose.includes(flow.owner) || !flow.moves_quantity(axiom_model::End::From) {
            continue;
        }
        let from_outside = book.places[flow.from].class == Class::Outside;
        let to_outside = book.places[flow.to].class == Class::Outside;
        let Some(purpose) = flow.purpose else {
            continue;
        };
        let root = book.purposes[purpose.purpose].root;
        let amount = movement_in_base(lens, posting, Some(root));
        let Some(amount) = amount else {
            unpriced += 1;
            continue;
        };
        let other = if from_outside {
            flow.from
        } else if to_outside {
            flow.to
        } else {
            flow.to
        };
        let party = flow.payee.map_or(Party::Place(other), Party::Entity);
        for period in periods.overlapping(flow.recognized.first(), flow.recognized.last()) {
            let window = periods.window(period).days();
            let Some(happened) = Days::new(window.first(), window.last().min(cutoff)) else {
                continue;
            };
            let part = spread(amount, flow.recognized, happened);
            values
                .entry((root, party))
                .or_insert_with(|| vec![Qty::ZERO; periods.len()])[period] += part;
        }
    }

    let mut section = table("Party", &periods);
    let roots = [
        PurposeRoot::Income,
        PurposeRoot::Spending,
        PurposeRoot::Capital,
        PurposeRoot::Transfer,
    ];
    let mut income = vec![Qty::ZERO; periods.len()];
    let mut spending = vec![Qty::ZERO; periods.len()];
    for root in roots {
        let mut parties: Vec<_> = values
            .iter()
            .filter(|((found, _), _)| *found == root)
            .collect();
        parties.sort_by_key(|((_, party), amounts)| {
            let magnitude = amounts
                .iter()
                .map(|qty| i128::from(qty.0).abs())
                .sum::<i128>();
            (-magnitude, party.label(book))
        });
        let total =
            parties
                .iter()
                .fold(vec![Qty::ZERO; periods.len()], |mut total, (_, amounts)| {
                    add_into(&mut total, amounts);
                    total
                });
        if parties.iter().all(|(_, amounts)| is_zero(amounts)) {
            continue;
        }
        let heading = match root {
            PurposeRoot::Income => "Income",
            PurposeRoot::Spending => "Spending",
            PurposeRoot::Capital => "Capital",
            PurposeRoot::Transfer => "Transfer",
        };
        section.push(row(book, Cell::Word(heading), 0, &total, Style::Total));
        for ((_, party), amounts) in parties {
            let name = party.label(book);
            for (index, &amount) in amounts
                .iter()
                .enumerate()
                .filter(|(_, amount)| !amount.is_zero())
            {
                section.fact(
                    match root {
                        PurposeRoot::Income => "income",
                        PurposeRoot::Spending => "spending",
                        PurposeRoot::Capital => "capital",
                        PurposeRoot::Transfer => "transfer",
                    },
                    Some(name),
                    lens.whose.label(book),
                    When::During(periods.window(index).days()),
                    Money::base(book, amount),
                );
            }
            section.push(row(book, Cell::Name(name), 1, amounts, Style::Normal));
        }
        match root {
            PurposeRoot::Income => add_into(&mut income, &total),
            PurposeRoot::Spending => add_into(&mut spending, &total),
            _ => {}
        }
    }
    let net = income
        .iter()
        .zip(&spending)
        .map(|(&earned, &spent)| earned - spent)
        .collect::<Vec<_>>();
    if !income.iter().all(|qty| qty.is_zero()) || !spending.iter().all(|qty| qty.is_zero()) {
        section.push(net_row(book, &net));
    }
    section.unpriced(unpriced, "flow");
    Report::new("Income and spending").with(section)
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Party {
    Entity(Id<Entity>),
    Place(Id<Place>),
}

fn table<'s>(first: &'static str, periods: &Periods) -> Section<'s> {
    let columns = (0..periods.len()).map(|period| Column::right(periods.title(period)));
    Section::new(std::iter::once(Column::left(first)).chain(columns))
}

fn row<'s>(
    book: &Book<'s>,
    label: Cell<'s>,
    depth: usize,
    values: &[Qty],
    style: Style,
) -> Row<'s> {
    let cells = values.iter().map(|&qty| Cell::base_or_blank(book, qty));
    let total = (values.len() > 1).then(|| Cell::base_or_blank(book, values.iter().copied().sum()));
    Row::new(std::iter::once(label).chain(cells).chain(total))
        .depth(depth)
        .style(style)
}

fn net_row<'s>(book: &Book<'s>, values: &[Qty]) -> Row<'s> {
    let cells = values.iter().map(|&qty| Cell::base(book, qty));
    let total = (values.len() > 1).then(|| Cell::base(book, values.iter().copied().sum()));
    Row::new(std::iter::once(Cell::Word("Net")).chain(cells).chain(total)).style(Style::Total)
}

impl Party {
    fn label<'s>(self, book: &Book<'s>) -> &'s str {
        match self {
            Party::Entity(entity) => book.name(book.entities[entity].path),
            Party::Place(place) => path(book, place),
        }
    }
}

/// Builds an income statement with names resolved by a shared report context.
pub(crate) fn view_with_lens<'s>(
    lens: Lens<'_, 's>,
    run: &Run,
    by: Period,
    from: Option<Day>,
) -> Report<'s> {
    purpose_view(lens, run, by, from)
}

/// Income, spending and capital, grouped by the purpose tree. The matrix is
/// indexed by purpose and period; parent rows are accumulated once, from the
/// leaves up, rather than rescanning every flow for each subtree.
fn purpose_view<'s>(lens: Lens<'_, 's>, run: &Run, by: Period, from: Option<Day>) -> Report<'s> {
    let (book, cutoff) = (lens.book, lens.day);
    let periods = match from {
        Some(from) => Periods::covering(by, from, cutoff),
        None => {
            let first = book
                .flows
                .as_slice()
                .first()
                .map_or(cutoff, |flow| flow.day.min(cutoff));
            Periods::covering(by, first, cutoff).last(DEFAULT_PERIODS)
        }
    };
    let period_count = periods.len();
    let purpose_count = book.purposes.len();
    let mut totals = vec![Qty::ZERO; purpose_count * period_count];
    let mut objects: HashMap<(Id<axiom_model::Purpose>, Object), Vec<Qty>> = HashMap::new();
    let mut descriptions: BTreeMap<Option<axiom_core::Sym>, Vec<Qty>> = BTreeMap::new();
    let mut unclassified = vec![Qty::ZERO; period_count];
    let mut unpriced = 0;
    let mut spread_seen = false;

    for posting in postings(book, run).filter(|posting| posting.is_real_on(cutoff)) {
        let flow = posting.flow;
        if !lens.whose.includes(flow.owner) || !flow.moves_quantity(axiom_model::End::From) {
            continue;
        }
        spread_seen |= flow.recognized.last() > flow.day;
        let Some(purpose) = flow.purpose else {
            let amount = movement_in_base(lens, posting, None);
            let Some(amount) = amount else {
                unpriced += 1;
                continue;
            };
            add_recognized(&mut unclassified, periods, flow.recognized, cutoff, amount);
            let values = descriptions
                .entry(flow.description)
                .or_insert_with(|| vec![Qty::ZERO; period_count]);
            add_recognized(values, periods, flow.recognized, cutoff, amount);
            continue;
        };
        let root = book.purposes[purpose.purpose].root;
        let Some(amount) = movement_in_base(lens, posting, Some(root)) else {
            unpriced += 1;
            continue;
        };
        let values = &mut totals[purpose.purpose.index() * period_count..][..period_count];
        add_recognized(values, periods, flow.recognized, cutoff, amount);
        if let Some(object) = purpose.of {
            let values = objects
                .entry((purpose.purpose, object))
                .or_insert_with(|| vec![Qty::ZERO; period_count]);
            add_recognized(values, periods, flow.recognized, cutoff, amount);
        }
    }

    // Purpose ids are preordered, so reverse traversal adds every child's
    // amount into its parent exactly once.
    for purpose in (0..purpose_count).rev().map(axiom_core::Id::new) {
        if let Some(parent) = book.purposes.parent(purpose) {
            let child_start = purpose.index() * period_count;
            let parent_start = parent.index() * period_count;
            for period in 0..period_count {
                totals[parent_start + period] += totals[child_start + period];
            }
        }
    }

    let columns = (0..period_count).map(|period| Column::right(periods.title(period)));
    let mut section = Section::new(std::iter::once(Column::left("Purpose")).chain(columns));
    let roots = book.purposes.roots().collect::<Vec<_>>();
    for root in roots {
        let root_values = purpose_values(&totals, period_count, root);
        if is_zero(root_values) {
            continue;
        }
        let purpose = &book.purposes[root];
        section.push(period_row(
            book,
            Cell::Name(book.name(purpose.name)),
            0,
            root_values,
            Style::Total,
        ));
        add_purpose_facts(&mut section, lens, periods, root, root_values);

        for id in book.purposes.subtree(root).skip(1) {
            let values = purpose_values(&totals, period_count, id);
            if is_zero(values) {
                continue;
            }
            let purpose = &book.purposes[id];
            let depth = book.purposes.depth(id) as usize;
            section.push(period_row(
                book,
                Cell::Name(book.name(purpose.name)),
                depth,
                values,
                Style::Normal,
            ));
            add_purpose_facts(&mut section, lens, periods, id, values);

            let mut children = objects
                .iter()
                .filter(|((purpose_id, _), amounts)| *purpose_id == id && !is_zero(amounts))
                .map(|((_, object), amounts)| (*object, amounts.as_slice()))
                .collect::<Vec<_>>();
            children.sort_by(|(left, _), (right, _)| {
                object_name(book, *left).cmp(object_name(book, *right))
            });
            for (object, amounts) in children {
                let object_name = object_name(book, object);
                let label = Cell::list(" ", [Cell::Word("of"), Cell::Name(object_name)]);
                section.push(period_row(book, label, depth + 1, amounts, Style::Muted));
                add_object_facts(
                    &mut section,
                    lens,
                    periods,
                    object_name,
                    amounts,
                    purpose.root,
                );
            }
        }
    }

    if !is_zero(&unclassified) {
        section.push(period_row(
            book,
            Cell::Word("Unclassified"),
            0,
            &unclassified,
            Style::Total,
        ));
        add_facts(
            &mut section,
            lens,
            periods,
            "unclassified",
            None,
            &unclassified,
        );
        for (description, values) in descriptions {
            if is_zero(&values) {
                continue;
            }
            let label = description.map_or(Cell::Word("unclassified"), |text| {
                Cell::Name(book.name(text))
            });
            section.push(period_row(book, label, 1, &values, Style::Normal));
            add_facts(
                &mut section,
                lens,
                periods,
                "unclassified",
                description.map(|text| book.name(text)),
                &values,
            );
        }
    }
    section.unpriced(unpriced, "flow");
    if spread_seen {
        section.note("Flows written over a date range are recognized a little each day across the periods they cover.");
    }
    if section.rows.is_empty() {
        section.note("No classified flows in this window.");
    }
    let mut report = Report::new("Income, spending and capital").with(section);
    if let Some(measures) = measure_section(lens, periods, cutoff) {
        report.sections.push(measures);
    }
    report
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum MeasureAction {
    Work,
    Use,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct MeasureKey {
    action: MeasureAction,
    purpose: Option<Id<Purpose>>,
    owner: Id<Entity>,
    unit: Id<Commodity>,
}

/// Events have units rather than money, so they have their own rows and facts.
fn measure_section<'s>(lens: Lens<'_, 's>, periods: Periods, cutoff: Day) -> Option<Section<'s>> {
    let book = lens.book;
    let mut totals: BTreeMap<MeasureKey, Vec<Qty>> = BTreeMap::new();
    for measure in book.measures.iter().map(|(_, measure)| measure) {
        if measure.day > cutoff || !lens.whose.includes(measure.owner) {
            continue;
        }
        let Some(period) = periods.index_of(measure.day) else {
            continue;
        };
        let action = match measure.action {
            Action::Work => MeasureAction::Work,
            Action::Use => MeasureAction::Use,
        };
        let key = MeasureKey {
            action,
            purpose: measure.purpose.map(|purpose| purpose.purpose),
            owner: measure.owner,
            unit: measure.quantity.unit,
        };
        totals
            .entry(key)
            .or_insert_with(|| vec![Qty::ZERO; periods.len()])[period] += measure.quantity.qty;
    }
    if totals.is_empty() {
        return None;
    }

    let period_columns = (0..periods.len()).map(|index| Column::right(periods.title(index)));
    let mut section = Section::new(
        [
            Column::left("Action"),
            Column::left("Purpose"),
            Column::left("Owner"),
            Column::left("Unit"),
        ]
        .into_iter()
        .chain(period_columns)
        .chain((periods.len() > 1).then(|| Column::right("Total"))),
    )
    .headed("Measures");
    for (key, values) in totals {
        let action = match key.action {
            MeasureAction::Work => "work",
            MeasureAction::Use => "use",
        };
        let purpose = key.purpose.map(|id| book.name(book.purposes[id].name));
        let unit = &book.commodities[key.unit];
        let cells = values
            .iter()
            .map(|&qty| Cell::amount(book, axiom_model::Amount::new(qty, key.unit)));
        let total = (periods.len() > 1).then(|| {
            Cell::amount(
                book,
                axiom_model::Amount::new(values.iter().copied().sum(), key.unit),
            )
        });
        section.push(Row::new(
            [
                Cell::Word(action),
                purpose.map_or(Cell::Word("unclassified"), Cell::Purpose),
                Cell::Name(book.name(book.entities[key.owner].path)),
                Cell::Name(book.name(unit.symbol)),
            ]
            .into_iter()
            .chain(cells)
            .chain(total),
        ));
        for (index, &qty) in values.iter().enumerate().filter(|(_, qty)| !qty.is_zero()) {
            section.fact(
                action,
                purpose,
                book.name(book.entities[key.owner].path),
                When::During(periods.window(index).days()),
                Money {
                    qty,
                    scale: unit.scale,
                    unit: book.name(unit.symbol),
                },
            );
        }
    }
    Some(section)
}

pub(crate) fn movement_in_base(
    lens: Lens<'_, '_>,
    posting: Posting<'_>,
    root: Option<PurposeRoot>,
) -> Option<Qty> {
    let flow = posting.flow;
    let from_outside = lens.book.places[flow.from].class == Class::Outside;
    let to_outside = lens.book.places[flow.to].class == Class::Outside;
    let inbound = from_outside && !to_outside;
    let outbound = to_outside && !from_outside;
    let amount = if inbound {
        posting.arrive_in_base(lens)
    } else {
        posting.out_in_base(lens)
    }?;
    let reverses = match root {
        Some(PurposeRoot::Income) => !inbound,
        Some(PurposeRoot::Spending | PurposeRoot::Capital) => inbound,
        Some(PurposeRoot::Transfer) | None => false,
    };
    Some(if reverses { -amount } else { amount })
}

fn add_recognized(
    values: &mut [Qty],
    periods: Periods,
    recognized: axiom_core::Days,
    cutoff: Day,
    amount: Qty,
) {
    if amount.is_zero() {
        return;
    }
    for index in periods.overlapping(recognized.first(), recognized.last()) {
        let window = periods.window(index).days();
        if let Some(happened) = axiom_core::Days::new(window.first(), window.last().min(cutoff)) {
            values[index] += axiom_core::spread(amount, recognized, happened);
        }
    }
}

fn purpose_values(totals: &[Qty], periods: usize, purpose: Id<Purpose>) -> &[Qty] {
    &totals[purpose.index() * periods..][..periods]
}

fn period_row<'s>(
    book: &Book<'s>,
    label: Cell<'s>,
    depth: usize,
    values: &[Qty],
    style: Style,
) -> Row<'s> {
    let cells = values
        .iter()
        .map(|&amount| Cell::base_or_blank(book, amount));
    let total = (values.len() > 1).then(|| Cell::base_or_blank(book, values.iter().copied().sum()));
    Row::new(std::iter::once(label).chain(cells).chain(total))
        .depth(depth)
        .style(style)
}

fn add_purpose_facts<'s>(
    section: &mut Section<'s>,
    lens: Lens<'_, 's>,
    periods: Periods,
    purpose: Id<Purpose>,
    values: &[Qty],
) {
    let book = lens.book;
    let item = &book.purposes[purpose];
    let name = book.name(item.name);
    add_facts(
        section,
        lens,
        periods,
        root_concept(item.root),
        Some(name),
        values,
    );
}

fn add_facts<'s>(
    section: &mut Section<'s>,
    lens: Lens<'_, 's>,
    periods: Periods,
    concept: &'static str,
    of: Option<&str>,
    values: &[Qty],
) {
    for (index, &amount) in values
        .iter()
        .enumerate()
        .filter(|(_, amount)| !amount.is_zero())
    {
        section.fact(
            concept,
            of,
            lens.whose.label(lens.book),
            crate::When::During(periods.window(index).days()),
            crate::Money::base(lens.book, amount),
        );
    }
}

fn add_object_facts<'s>(
    section: &mut Section<'s>,
    lens: Lens<'_, 's>,
    periods: Periods,
    object: &'s str,
    values: &[Qty],
    root: PurposeRoot,
) {
    let concept = root_concept(root);
    let book = lens.book;
    for (index, &amount) in values
        .iter()
        .enumerate()
        .filter(|(_, amount)| !amount.is_zero())
    {
        section.fact(
            concept,
            Some(object),
            lens.whose.label(book),
            crate::When::During(periods.window(index).days()),
            crate::Money::base(book, amount),
        );
    }
}

fn root_concept(root: PurposeRoot) -> &'static str {
    match root {
        PurposeRoot::Income => "income",
        PurposeRoot::Spending => "spending",
        PurposeRoot::Capital => "capital",
        PurposeRoot::Transfer => "transfer",
    }
}

fn object_name<'s>(book: &Book<'s>, object: Object) -> &'s str {
    match object {
        Object::Asset(asset) => book.name(book.assets[asset].name),
        Object::Place(place) => path(book, place),
        Object::Entity(entity) => book.name(book.entities[entity].path),
    }
}

fn add_into(totals: &mut [Qty], values: &[Qty]) {
    for (total, &value) in totals.iter_mut().zip(values) {
        *total += value;
    }
}

fn is_zero(values: &[Qty]) -> bool {
    values.iter().all(|value| value.is_zero())
}
