//! `flow`: income, spending, capital and transfer, grouped by purpose or party.
//!
//! Flows are grouped by their declared purpose and the purpose's root. Amounts
//! are priced in the base currency on the day each flow happened. A flow spread
//! over a range is recognized a little each day, so a year's premium lands in
//! every month it covers.

use std::collections::BTreeMap;

use axiom_core::{Day, Days, Id, Map, Qty};
use axiom_engine::{Counting, Counts, Piece, Posted, Run, Share};
use axiom_model::{
    Action, Amount, Book, Class, Commodity, Dir, Entity, Flow, Object, Period, Place, Purpose, PurposeRoot, Purposed,
};

use crate::calendar::Periods;
use crate::history::all_postings;
use crate::lens::Lens;
use crate::pivot::{Grid, Pivot};
use crate::places::path;
use crate::{Cell, Column, Money, Report, Row, Section, Style, When};

/// How many periods to show when the window is not given.
const DEFAULT_PERIODS: usize = 12;

/// The purpose roots a report by party shows, in the order it shows them.
const ROOTS: [PurposeRoot; 4] =
    [PurposeRoot::Income, PurposeRoot::Spending, PurposeRoot::Capital, PurposeRoot::Transfer];

/// What a root is called as a heading, and as the concept of the facts under it.
fn root_names(root: PurposeRoot) -> (&'static str, &'static str) {
    match root {
        PurposeRoot::Income => ("Income", "income"),
        PurposeRoot::Spending => ("Spending", "spending"),
        PurposeRoot::Capital => ("Capital", "capital"),
        PurposeRoot::Transfer => ("Transfer", "transfer"),
    }
}

/// The periods a flow report covers: from the day asked for, else from the first activity, the last twelve.
fn periods_of(lens: Lens<'_, '_, '_, '_>, by: Period, from: Option<Day>) -> Periods {
    match from {
        Some(from) => Periods::covering(by, from, lens.day),
        None => Periods::covering(by, first_activity(lens, lens.day), lens.day).last(DEFAULT_PERIODS),
    }
}

// ─── By party ───────────────────────────────────────────────────────────────

pub(crate) fn view_by_party_with_lens<'s>(lens: Lens<'s, '_, '_, '_>, run: &Run, from: Option<Day>) -> Report<'s> {
    let periods = periods_of(lens, Period::Month, from);
    let totals = PartyTotals::of(lens, run, periods);
    let mut section = table("Party", &periods);
    let (mut income, mut spending) = (vec![Qty::ZERO; periods.len()], vec![Qty::ZERO; periods.len()]);
    for root in ROOTS {
        let Some(total) = totals.push_root(lens, periods, root, &mut section) else { continue };
        match root {
            PurposeRoot::Income => add_into(&mut income, &total),
            PurposeRoot::Spending => add_into(&mut spending, &total),
            _ => {}
        }
    }
    if !is_zero(&income) || !is_zero(&spending) {
        let net: Vec<Qty> = income.iter().zip(&spending).map(|(&earned, &spent)| earned - spent).collect();
        section.push(net_row(lens.book(), &net));
    }
    section.unpriced(totals.0.unpriced, "flow");
    Report::new("Income and spending").with(section)
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Party {
    Entity(Id<Entity>),
    Place(Id<Place>),
}

impl Party {
    fn label<'s>(self, book: &'s Book<'_>) -> &'s str {
        match self {
            Party::Entity(entity) => book.name(book.entities[entity].path),
            Party::Place(place) => path(book, place),
        }
    }
}

/// What each party moved under each purpose root, by period.
struct PartyTotals(Pivot<(PurposeRoot, Party)>);

impl PartyTotals {
    fn of(lens: Lens<'_, '_, '_, '_>, run: &Run, periods: Periods) -> PartyTotals {
        let book = lens.book();
        let mut pivot = Pivot::new(periods);
        let classified = |_: &Flow, piece: &Piece| piece.purpose.is_some();
        for_each_counted(lens, run, lens.day, classified, |counted| {
            let Counted { flow, purpose: Some(purpose), .. } = counted else { return };
            let Some(amount) = pivot.price(&counted) else { return };
            // The end that is not the book's own: where the money came from, or went.
            let other = if book.places[flow.from].class == Class::Outside { flow.from } else { flow.to };
            let party = flow.payee.map_or(Party::Place(other), Party::Entity);
            pivot.add((book.purposes[purpose.purpose].root, party), &counted, amount, lens.day);
        });
        PartyTotals(pivot)
    }

    /// The rows of one root: its total, then each party, the largest first. The total, or `None` if nothing moved.
    fn push_root<'s>(
        &self,
        lens: Lens<'s, '_, '_, '_>,
        periods: Periods,
        root: PurposeRoot,
        section: &mut Section<'s>,
    ) -> Option<Vec<Qty>> {
        let book = lens.book();
        let rows = self.0.keys().iter().filter(|(found, _)| *found == root);
        let mut parties: Vec<(Party, &[Qty])> =
            rows.map(|&key| (key.1, self.0.amounts(key).expect("a key has a row"))).collect();
        if parties.iter().all(|(_, amounts)| is_zero(amounts)) {
            return None;
        }
        parties.sort_by_key(|(party, amounts)| {
            let magnitude = amounts.iter().map(|qty| i128::from(qty.0).abs()).sum::<i128>();
            (-magnitude, party.label(book))
        });
        let mut total = vec![Qty::ZERO; periods.len()];
        parties.iter().for_each(|(_, amounts)| add_into(&mut total, amounts));
        let (heading, concept) = root_names(root);
        section.push(row(book, Cell::Word(heading), 0, &total, Style::Total));
        for (party, amounts) in parties {
            let name = party.label(book);
            add_facts(section, lens, periods, concept, Some(name), amounts);
            section.push(row(book, Cell::Name(name), 1, amounts, Style::Normal));
        }
        Some(total)
    }
}

fn table<'s>(first: &'static str, periods: &Periods) -> Section<'s> {
    let columns = (0..periods.len()).map(|period| Column::right(periods.title(period)));
    Section::new(
        std::iter::once(Column::left(first)).chain(columns).chain((periods.len() > 1).then(|| Column::right("Total"))),
    )
}

fn row<'s>(book: &'s Book<'_>, label: Cell<'s>, depth: usize, values: &[Qty], style: Style) -> Row<'s> {
    let cells = values.iter().map(|&qty| Cell::base_or_blank(book, qty));
    let total = (values.len() > 1).then(|| Cell::base_or_blank(book, values.iter().copied().sum()));
    Row::new(std::iter::once(label).chain(cells).chain(total)).depth(depth).style(style)
}

fn net_row<'s>(book: &'s Book<'_>, values: &[Qty]) -> Row<'s> {
    let cells = values.iter().map(|&qty| Cell::base(book, qty));
    let total = (values.len() > 1).then(|| Cell::base(book, values.iter().copied().sum()));
    Row::new(std::iter::once(Cell::Word("Net")).chain(cells).chain(total)).style(Style::Total)
}

// ─── By purpose ─────────────────────────────────────────────────────────────

/// Builds an income statement with names resolved by a shared report context.
pub(crate) fn view_with_lens<'s>(lens: Lens<'s, '_, '_, '_>, run: &Run, by: Period, from: Option<Day>) -> Report<'s> {
    let periods = periods_of(lens, by, from);
    let mut totals = PurposeTotals::of(lens, run, periods);
    totals.roll_up(lens.book());
    let mut section = totals.section(lens, periods);
    section.unpriced(totals.0.unpriced, "flow");
    if totals.0.spread {
        section.note("Flows written over a date range are recognized a little each day across the periods they cover.");
    }
    if section.rows.is_empty() {
        section.note("No classified flows in this window.");
    }
    let mut report = Report::new("Income, spending and capital").with(section);
    if let Some(measures) = measure_section(lens, periods) {
        report.sections.push(measures);
    }
    report
}

/// A row of the purpose table: a purpose, what a purpose is of, or what had no purpose (all of it, or by description).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Tally<'s> {
    Purpose(Id<Purpose>),
    Of(Id<Purpose>, Object),
    Unclassified,
    Described(Option<&'s str>),
}

/// What the flows of a window add up to: by purpose, by purpose and the object it is of, and, for those with no purpose, by
/// description. The parents of the purpose tree are accumulated once, from the leaves up, rather than rescanning every flow
/// for each subtree.
struct PurposeTotals<'s>(Pivot<Tally<'s>>);

impl<'s> PurposeTotals<'s> {
    fn of(lens: Lens<'s, '_, '_, '_>, run: &Run, periods: Periods) -> PurposeTotals<'s> {
        let mut totals = PurposeTotals(Pivot::new(periods));
        for_each_counted(lens, run, lens.day, |_, _| true, |counted| totals.add(lens, counted));
        totals
    }

    /// Adds what one posting counts to the purpose it is for, and the object of that, or to those with no purpose.
    fn add(&mut self, lens: Lens<'s, '_, '_, '_>, counted: Counted<'_>) {
        let (book, cutoff) = (lens.book(), lens.day);
        let Some(amount) = self.0.price(&counted) else { return };
        let mut add = |key| self.0.add(key, &counted, amount, cutoff);
        match counted.purpose {
            Some(Purposed { purpose, of, .. }) => {
                add(Tally::Purpose(purpose));
                if let Some(object) = of {
                    add(Tally::Of(purpose, object));
                }
            }
            None => {
                add(Tally::Unclassified);
                add(Tally::Described(counted.flow.description.map(|text| book.text(text))));
            }
        }
    }

    /// Adds every purpose to its parent. Purpose ids are preordered, so going backwards adds each child exactly once.
    fn roll_up(&mut self, book: &Book<'_>) {
        for purpose in (0..book.purposes.len()).rev().map(|index| Id::<Purpose>::new(index as u32)) {
            if let Some(parent) = book.purposes.parent(purpose) {
                self.0.roll_up(Tally::Purpose(purpose), Tally::Purpose(parent));
            }
        }
    }

    /// The purposes that had something, each under its parent, with the objects they are of and what had no purpose.
    fn section(&self, lens: Lens<'s, '_, '_, '_>, periods: Periods) -> Section<'s> {
        let book = lens.book();
        let mut section = table("Purpose", &periods);
        let objects = self.objects(book);
        let mut cursor = ObjectCursor { objects: &objects, next: 0 };
        for root in book.purposes.roots() {
            for id in book.purposes.subtree(root).filter(|&id| self.0.moved(Tally::Purpose(id))) {
                self.push_purpose(lens, periods, &mut section, id, cursor.take(id));
            }
        }
        self.push_unclassified(lens, periods, &mut section);
        section
    }

    /// The objects with something in the window, by purpose and then by name.
    fn objects(&self, book: &Book<'_>) -> Vec<(Id<Purpose>, Object)> {
        let held = |key: &Tally<'_>| self.0.amounts(*key).is_some_and(|amounts| !is_zero(amounts));
        let mut moved: Vec<_> = self
            .0
            .keys()
            .iter()
            .filter_map(|key| match *key {
                Tally::Of(purpose, object) if held(key) => Some((purpose, object)),
                _ => None,
            })
            .collect();
        moved.sort_by(|&(left, left_object), &(right, right_object)| {
            left.cmp(&right).then_with(|| object_name(book, left_object).cmp(object_name(book, right_object)))
        });
        moved
    }

    /// A purpose's row, its facts, and a row below it for each object it is of.
    fn push_purpose(
        &self,
        lens: Lens<'s, '_, '_, '_>,
        periods: Periods,
        section: &mut Section<'s>,
        id: Id<Purpose>,
        objects: &[(Id<Purpose>, Object)],
    ) {
        let book = lens.book();
        let purpose = &book.purposes[id];
        let (name, concept) = (book.name(purpose.name), root_names(purpose.root).1);
        let values = self.0.amounts(Tally::Purpose(id)).expect("a purpose that moved has a row");
        let depth = book.purposes.depth(id) as usize;
        let style = if book.purposes.parent(id).is_none() { Style::Total } else { Style::Normal };
        section.push(row(book, Cell::Name(name), depth, values, style));
        add_facts(section, lens, periods, concept, Some(name), values);
        for &(_, object) in objects {
            let (name, amounts) =
                (object_name(book, object), self.0.amounts(Tally::Of(id, object)).expect("an object that moved"));
            let label = Cell::list(" ", [Cell::Word("of"), Cell::Name(name)]);
            section.push(row(book, label, depth + 1, amounts, Style::Muted));
            add_facts(section, lens, periods, concept, Some(name), amounts);
        }
    }

    /// What no purpose explains: a total row, and a row for each description that moved something, in description order.
    fn push_unclassified(&self, lens: Lens<'s, '_, '_, '_>, periods: Periods, section: &mut Section<'s>) {
        let book = lens.book();
        let zeros = vec![Qty::ZERO; periods.len()];
        let all = self.0.amounts(Tally::Unclassified).unwrap_or(&zeros);
        let mut described: Vec<(Option<&'s str>, &[Qty])> = self
            .0
            .keys()
            .iter()
            .filter_map(|key| match *key {
                Tally::Described(description) if self.0.moved(*key) => Some((description, self.0.amounts(*key)?)),
                _ => None,
            })
            .collect();
        if is_zero(all) && described.is_empty() {
            return;
        }
        described.sort_by_key(|&(description, _)| description);
        section.push(row(book, Cell::Word("Unclassified"), 0, all, Style::Total));
        add_facts(section, lens, periods, "unclassified", None, all);
        for (description, amounts) in described {
            let label = description.map_or(Cell::Word("unclassified"), Cell::text);
            section.push(row(book, label, 1, amounts, Style::Normal));
            add_facts(section, lens, periods, "unclassified", description, amounts);
        }
    }
}

/// Walks the objects of purposes in purpose order, as the purposes are walked in the same order.
struct ObjectCursor<'a> {
    objects: &'a [(Id<Purpose>, Object)],
    next: usize,
}

impl<'a> ObjectCursor<'a> {
    /// The objects of `purpose`, passing those of any purpose before it.
    fn take(&mut self, purpose: Id<Purpose>) -> &'a [(Id<Purpose>, Object)] {
        let rest = &self.objects[self.next..];
        let before = rest.partition_point(|&(of, _)| of < purpose);
        let of = rest[before..].partition_point(|&(found, _)| found == purpose);
        self.next += before + of;
        &rest[before..before + of]
    }
}

// ─── Measures ───────────────────────────────────────────────────────────────

/// What a measure records, as a key that sorts: work before use.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum MeasureAction {
    Work,
    Use,
}

impl MeasureAction {
    fn of(action: Action) -> MeasureAction {
        match action {
            Action::Work => MeasureAction::Work,
            Action::Use => MeasureAction::Use,
        }
    }

    fn word(self) -> &'static str {
        match self {
            MeasureAction::Work => "work",
            MeasureAction::Use => "use",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct MeasureKey {
    action: MeasureAction,
    purpose: Option<Id<Purpose>>,
    owner: Id<Entity>,
    unit: Id<Commodity>,
}

/// Events have units rather than money, so they have their own rows and facts.
fn measure_section<'s>(lens: Lens<'s, '_, '_, '_>, periods: Periods) -> Option<Section<'s>> {
    let book = lens.book();
    // A row in a grid for each owner, purpose and unit, not a vector of its own.
    let mut rows: BTreeMap<MeasureKey, usize> = BTreeMap::new();
    let mut grid = Grid::new(periods.len());
    for measure in book.measures.values().filter(|measure| measure.day <= lens.day && lens.owns_entity(measure.owner)) {
        let Some(period) = periods.index_of(measure.day) else { continue };
        let key = MeasureKey {
            action: MeasureAction::of(measure.action),
            purpose: measure.purpose.map(|purpose| purpose.purpose),
            owner: measure.owner,
            unit: measure.quantity.unit,
        };
        let at = *rows.entry(key).or_insert_with(|| grid.push());
        grid.row_mut(at)[period] += measure.quantity.qty;
    }
    if rows.is_empty() {
        return None;
    }
    let period_columns = (0..periods.len()).map(|index| Column::right(periods.title(index)));
    let mut section = Section::new(
        [Column::left("Action"), Column::left("Purpose"), Column::left("Owner"), Column::left("Unit")]
            .into_iter()
            .chain(period_columns)
            .chain((periods.len() > 1).then(|| Column::right("Total"))),
    )
    .headed("Measures");
    for (key, at) in rows {
        push_measures(&mut section, lens, periods, key, grid.row(at));
    }
    Some(section)
}

/// The row of one kind of measure, and a fact for each period it has some in.
fn push_measures<'s>(
    section: &mut Section<'s>,
    lens: Lens<'s, '_, '_, '_>,
    periods: Periods,
    key: MeasureKey,
    values: &[Qty],
) {
    let book = lens.book();
    let action = key.action.word();
    let purpose = key.purpose.map(|id| book.name(book.purposes[id].name));
    let owner = book.name(book.entities[key.owner].path);
    let unit = &book.commodities[key.unit];
    let cells = values.iter().map(|&qty| Cell::amount(book, Amount::new(qty, key.unit)));
    let total = (periods.len() > 1).then(|| Cell::amount(book, Amount::new(values.iter().copied().sum(), key.unit)));
    section.push(Row::new(
        [
            Cell::Word(action),
            purpose.map_or(Cell::Word("unclassified"), Cell::Purpose),
            Cell::Name(owner),
            Cell::Name(book.name(unit.symbol)),
        ]
        .into_iter()
        .chain(cells)
        .chain(total),
    ));
    for (index, &qty) in values.iter().enumerate().filter(|(_, qty)| !qty.is_zero()) {
        let money = Money { qty, scale: unit.scale, unit: book.name(unit.symbol) };
        section.fact(action, purpose, owner, When::During(periods.window(index).days()), money);
    }
}

/// Carries rounding boundaries across postings at each physical endpoint.
/// This makes statement rows conserve the same cents as the corresponding
/// register balance when many small movements share one place.
#[derive(Default)]
pub(crate) struct MovementShares {
    cumulative: Map<(Id<Place>, Id<Commodity>), Qty>,
}

impl MovementShares {
    /// Allocates the change in a place's signed cumulative balance. Carrying
    /// one signed boundary across both inflows and outflows is important when
    /// a small shared balance crosses zero: separately rounding positive and
    /// negative magnitudes can disagree with the scoped closing balance.
    fn split(&mut self, lens: Lens<'_, '_, '_, '_>, place: Id<Place>, amount: Amount) -> Qty {
        if lens.whose.is_everyone() || amount.qty.is_zero() {
            return amount.qty;
        }
        let cumulative = self.cumulative.entry((place, amount.unit)).or_default();
        let before = lens.place_qty(place, *cumulative);
        *cumulative += amount.qty;
        let after = lens.place_qty(place, *cumulative);
        after - before
    }
}

/// What one posting counts toward one purpose, or what a written-off claim took back, as the run says.
pub(crate) struct Counted<'a> {
    /// The flow it is of: whose payee and description say whom and what it was for.
    pub flow: &'a Flow,
    pub purpose: Option<Purposed>,
    /// The day it counts on: the flow's own, or the day the claim was forgiven.
    pub day: Day,
    pub recognized: Days,
    /// Its value on that day in the base currency, signed by the root of its purpose (income in is positive, spending and
    /// capital out are negative, a transfer or what has no purpose is its size); `None` where a price is missing.
    pub amount: Option<Qty>,
}

/// Calls `each` for everything the postings real on `cutoff` count, in the order of the journal, and then for what the
/// claims written off by then took back, as the lens owns it and `wanted` asks for it. What a posting counts is
/// `recognition`'s rule, the one the fold counts the totals and the laws by, so that what a report says of a purpose is
/// what a limit, a law and the forecast say.
pub(crate) fn for_each_counted<'a>(
    lens: Lens<'a, '_, '_, '_>,
    run: &'a Run,
    cutoff: Day,
    wanted: impl Fn(&Flow, &Piece) -> bool,
    mut each: impl FnMut(Counted<'a>),
) {
    let (book, plan) = (lens.book(), lens.plan());
    let (mut shares, mut pieces) = (MovementShares::default(), Vec::new());
    for posting in all_postings(book, run).filter(|posting| posting.is_real_on(cutoff)) {
        let (flow, posted) = (posting.flow, posting.posted);
        Counting::posted(plan, flow, &posted, posting.settlement).pieces(book, &mut pieces);
        for piece in pieces.iter().filter(|piece| wanted(flow, piece)) {
            let (place, signed) = counted_at(book, flow, &posted, piece);
            if !lens.owns(place) {
                continue;
            }
            let amount = signed.and_then(|signed| priced(lens, flow.day, place, signed, piece, &mut shares));
            each(Counted { flow, purpose: piece.purpose, day: flow.day, recognized: piece.recognized, amount });
        }
    }
    for Forgiven { day, claim, tab, unit, qty } in forgiven_by(run, book, cutoff) {
        let flow = &book.flows[claim];
        Counting::forgiving(plan, flow, day, qty).pieces(book, &mut pieces);
        for piece in pieces.iter().filter(|piece| wanted(flow, piece) && lens.owns(tab)) {
            let Share::Part(taken) = piece.share else { continue };
            let signed = Amount::new(Qty(-taken.0), unit);
            let amount = priced(lens, day, tab, signed, piece, &mut shares);
            each(Counted { flow, purpose: piece.purpose, day, recognized: piece.recognized, amount });
        }
    }
}

/// A line of a claim that was forgiven, all its parcels that were open added up: the fold takes it back once.
struct Forgiven {
    day: Day,
    claim: Id<Flow>,
    tab: Id<Place>,
    unit: Id<Commodity>,
    qty: Qty,
}

/// The claims forgiven by `cutoff`, a line at a time.
fn forgiven_by(run: &Run, book: &Book<'_>, cutoff: Day) -> Vec<Forgiven> {
    let mut lines: Vec<Forgiven> = Vec::new();
    for off in &run.written_off {
        let day = book.claim_changes[off.change as usize].day;
        if day > cutoff {
            continue;
        }
        match lines.iter_mut().find(|line| (line.day, line.claim) == (day, off.claim)) {
            Some(line) => line.qty += off.qty,
            None => lines.push(Forgiven { day, claim: off.claim, tab: off.place, unit: off.unit, qty: off.qty }),
        }
    }
    lines
}

/// Where a piece of a posting counts and what of it: the end the flow's ownership moves money through, and its quantity
/// signed by the way it counts (in is positive); or, for a piece that is a claim settled, the tab that held it. `None` if
/// the quantity cannot be negated.
fn counted_at(book: &Book<'_>, flow: &Flow, posted: &Posted, piece: &Piece) -> (Id<Place>, Option<Amount>) {
    let inbound = book.places[flow.from].class == Class::Outside && book.places[flow.to].class != Class::Outside;
    let (place, dir) = match piece.counts {
        Counts::Claim { tab, dir } => (tab, dir),
        Counts::Flow if inbound => (flow.to, Dir::In),
        Counts::Flow => (flow.from, Dir::Out),
    };
    let side = match dir {
        Dir::In => Amount::new(posted.arrive, flow.arrive.unit),
        Dir::Out => Amount::new(posted.out, flow.out.unit),
    };
    let qty = match piece.share {
        Share::Whole => side.qty,
        Share::Part(qty) => qty,
    };
    let signed = match dir {
        Dir::In => Some(qty),
        Dir::Out => qty.0.checked_neg().map(Qty),
    };
    (place, signed.map(|qty| Amount::new(qty, side.unit)))
}

/// `signed` at `place` in the base currency on `day`, by the root of the piece's purpose. A flow's declared owner is only
/// the primary owner: the physical end it moves through carries the effective ownership shares, so the quantity is scoped
/// there before it is priced. That also keeps a foreign-currency movement's displayed share aligned with the unit posted.
fn priced(
    lens: Lens<'_, '_, '_, '_>,
    day: Day,
    place: Id<Place>,
    signed: Amount,
    piece: &Piece,
    shares: &mut MovementShares,
) -> Option<Qty> {
    let book = lens.book();
    let amount = Amount::new(shares.split(lens, place, signed), signed.unit);
    let value = lens.on(day).value(amount)?;
    match piece.purpose.map(|purpose| book.purposes[purpose.purpose].root) {
        Some(PurposeRoot::Income) => Some(value),
        Some(PurposeRoot::Spending | PurposeRoot::Capital) => Some(Qty(value.0.checked_neg()?)),
        // What passes through counts as the volume it was in either direction, but a claim taken back is not more of it.
        Some(PurposeRoot::Transfer) | None => {
            let volume = value.0.checked_abs()?;
            Some(Qty(if piece.takes_back() { -volume } else { volume }))
        }
    }
}

/// The physical endpoint whose amount is used by income/spending reports.
/// Incoming flows use what reached the owned end; all other flows use what
/// left the source, which is the end `counted_at` counts a flow's own pieces at.
pub(crate) fn movement_place(lens: Lens<'_, '_, '_, '_>, flow: &axiom_model::Flow) -> Id<Place> {
    let book = lens.book();
    if book.places[flow.from].class == Class::Outside && book.places[flow.to].class != Class::Outside {
        flow.to
    } else {
        flow.from
    }
}

/// Applies the same endpoint ownership split to template amounts and other
/// unpriced quantity views.
pub(crate) fn scoped_movement_qty(lens: Lens<'_, '_, '_, '_>, flow: &axiom_model::Flow, qty: Qty) -> Qty {
    lens.place_qty(movement_place(lens, flow), qty)
}

fn first_activity(lens: Lens<'_, '_, '_, '_>, cutoff: Day) -> Day {
    let book = lens.book();
    book.flows
        .iter()
        .filter(|(_, flow)| lens.owns(movement_place(lens, flow)))
        .map(|(_, flow)| flow.day)
        .chain(
            book.measures.iter().filter(|(_, measure)| lens.owns_entity(measure.owner)).map(|(_, measure)| measure.day),
        )
        .filter(|day| *day <= cutoff)
        .min()
        .unwrap_or(cutoff)
}

fn add_facts<'s>(
    section: &mut Section<'s>,
    lens: Lens<'s, '_, '_, '_>,
    periods: Periods,
    concept: &'static str,
    of: Option<&'s str>,
    values: &[Qty],
) {
    for (index, &amount) in values.iter().enumerate().filter(|(_, amount)| !amount.is_zero()) {
        section.fact(
            concept,
            of,
            lens.whose.label(lens.book()),
            crate::When::During(periods.window(index).days()),
            crate::Money::base(lens.book(), amount),
        );
    }
}

pub(crate) fn object_name<'s>(book: &'s Book<'_>, object: Object) -> &'s str {
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
