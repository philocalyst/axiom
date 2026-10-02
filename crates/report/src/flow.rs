//! `flow`: income, spending, capital and transfer, grouped by purpose or party.
//!
//! Flows are grouped by their declared purpose and the purpose's root. Amounts
//! are priced in the base currency on the day each flow happened. A flow spread
//! over a range is recognized a little each day, so a year's premium lands in
//! every month it covers.

use std::collections::BTreeMap;

use axiom_core::{Day, Days, Id, Map, Qty, spread};
use axiom_engine::Run;
use axiom_model::{Action, Amount, Book, Class, Commodity, Entity, Object, Period, Place, Purpose, PurposeRoot};

use crate::calendar::Periods;
use crate::history::{Posting, postings};
use crate::lens::Lens;
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

/// A table of quantities with a row of one value per period for each of whatever is being tallied: one flat vector,
/// so that a row costs no allocation of its own.
struct Grid {
    width: usize,
    rows: usize,
    cells: Vec<Qty>,
}

impl Grid {
    fn new(width: usize) -> Grid {
        Grid { width, rows: 0, cells: Vec::new() }
    }

    /// A table of `rows` rows of zeros.
    fn zeros(width: usize, rows: usize) -> Grid {
        Grid { width, rows, cells: vec![Qty::ZERO; width * rows] }
    }

    /// Adds a row of zeros, and says which it is.
    fn push(&mut self) -> usize {
        self.cells.resize(self.cells.len() + self.width, Qty::ZERO);
        self.rows += 1;
        self.rows - 1
    }

    fn row(&self, at: usize) -> &[Qty] {
        &self.cells[at * self.width..][..self.width]
    }

    fn row_mut(&mut self, at: usize) -> &mut [Qty] {
        &mut self.cells[at * self.width..][..self.width]
    }

    /// Adds the row `from` into the row `into`.
    fn add_row(&mut self, from: usize, into: usize) {
        for period in 0..self.width {
            let value = self.cells[from * self.width + period];
            self.cells[into * self.width + period] += value;
        }
    }
}

/// The postings that stand on the lens's day and moved money its owner holds.
fn owned_postings<'a>(lens: Lens<'a, '_, '_, '_>, run: &'a Run) -> impl Iterator<Item = Posting<'a>> {
    postings(lens.book(), run)
        .filter(move |posting| posting.is_real_on(lens.day) && lens.owns(movement_place(lens, posting.flow)))
}

/// The periods a flow report covers: from the day asked for, else from the first activity, the last twelve.
fn periods_of(lens: Lens<'_, '_, '_, '_>, by: Period, from: Option<Day>) -> Periods {
    match from {
        Some(from) => Periods::covering(by, from, lens.day),
        None => Periods::covering(by, first_activity(lens, lens.day), lens.day).last(DEFAULT_PERIODS),
    }
}

/// Spreads `amount` over the periods `recognized` touches, as far as `cutoff`, and says what each period gets.
fn spread_over(periods: Periods, recognized: Days, cutoff: Day, amount: Qty, mut each: impl FnMut(usize, Qty)) {
    for index in periods.overlapping(recognized.first(), recognized.last()) {
        let window = periods.window(index).days();
        if let Some(happened) = Days::new(window.first(), window.last().min(cutoff)) {
            each(index, spread(amount, recognized, happened));
        }
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
    section.unpriced(totals.unpriced, "flow");
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
struct PartyTotals {
    /// The row of `grid` that holds a party's amounts under a root.
    rows: Map<(PurposeRoot, Party), usize>,
    grid: Grid,
    /// Flows that could not be priced in the base currency.
    unpriced: usize,
}

impl PartyTotals {
    fn of(lens: Lens<'_, '_, '_, '_>, run: &Run, periods: Periods) -> PartyTotals {
        let book = lens.book();
        let mut totals = PartyTotals { rows: Map::default(), grid: Grid::new(periods.len()), unpriced: 0 };
        let mut shares = MovementShares::default();
        for posting in owned_postings(lens, run) {
            let flow = posting.flow;
            let Some(purpose) = flow.purpose else { continue };
            let root = book.purposes[purpose.purpose].root;
            let Some(amount) = movement_in_base_with(lens, posting, Some(root), &mut shares) else {
                totals.unpriced += 1;
                continue;
            };
            // The end that is not the book's own: where the money came from, or went.
            let other = if book.places[flow.from].class == Class::Outside { flow.from } else { flow.to };
            let party = flow.payee.map_or(Party::Place(other), Party::Entity);
            let mut row = None;
            spread_over(periods, flow.recognized, lens.day, amount, |period, part| {
                let at =
                    *row.get_or_insert_with(|| *totals.rows.entry((root, party)).or_insert_with(|| totals.grid.push()));
                totals.grid.row_mut(at)[period] += part;
            });
        }
        totals
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
        let mut parties: Vec<(Party, &[Qty])> = self
            .rows
            .iter()
            .filter(|((found, _), _)| *found == root)
            .map(|(&(_, party), &at)| (party, self.grid.row(at)))
            .collect();
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
    section.unpriced(totals.unpriced, "flow");
    if totals.spread {
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

/// What the flows of a window add up to: by purpose, by purpose and the object it is of, and, for those with no
/// purpose, by description. Each is a grid of one row per period, and the parents of the purpose tree are
/// accumulated once, from the leaves up, rather than rescanning every flow for each subtree.
struct PurposeTotals<'s> {
    /// One row per purpose.
    purposes: Grid,
    /// Whether a purpose, or any below it, had a flow that moved something in the window.
    active: Vec<bool>,
    objects: Objects,
    unclassified: Unclassified<'s>,
    unpriced: usize,
    /// Some flow is recognized over a range of days, not on one.
    spread: bool,
}

impl<'s> PurposeTotals<'s> {
    fn of(lens: Lens<'s, '_, '_, '_>, run: &Run, periods: Periods) -> PurposeTotals<'s> {
        let (width, purposes) = (periods.len(), lens.book().purposes.len());
        let mut totals = PurposeTotals {
            purposes: Grid::zeros(width, purposes),
            active: vec![false; purposes],
            objects: Objects { keys: Vec::new(), rows: Map::default(), grid: Grid::new(width) },
            unclassified: Unclassified {
                all: vec![Qty::ZERO; width],
                rows: BTreeMap::new(),
                grid: Grid::new(width),
                active: Vec::new(),
            },
            unpriced: 0,
            spread: false,
        };
        let mut shares = MovementShares::default();
        for posting in owned_postings(lens, run) {
            totals.add(lens, periods, posting, &mut shares);
        }
        totals
    }

    /// Adds one posting to the purpose it is for, and the object of that, or to those with no purpose.
    fn add(&mut self, lens: Lens<'s, '_, '_, '_>, periods: Periods, posting: Posting<'_>, shares: &mut MovementShares) {
        let (book, flow) = (lens.book(), posting.flow);
        self.spread |= flow.recognized.last() > flow.day;
        let root = flow.purpose.map(|purpose| book.purposes[purpose.purpose].root);
        let Some(amount) = movement_in_base_with(lens, posting, root, shares) else {
            self.unpriced += 1;
            return;
        };
        let touches = periods.overlapping(flow.recognized.first(), flow.recognized.last()).next().is_some();
        let moved = touches && !amount.is_zero();
        let spreading = |row: &mut [Qty]| add_recognized(row, periods, flow.recognized, lens.day, amount);
        match flow.purpose {
            Some(purpose) => {
                spreading(self.purposes.row_mut(purpose.purpose.index()));
                self.active[purpose.purpose.index()] |= moved;
                if let Some(object) = purpose.of {
                    let at = self.objects.row(purpose.purpose, object);
                    spreading(self.objects.grid.row_mut(at));
                }
            }
            None => {
                spreading(&mut self.unclassified.all);
                let at = self.unclassified.row(flow.description.map(|text| book.text(text)));
                spreading(self.unclassified.grid.row_mut(at));
                self.unclassified.active[at] |= moved;
            }
        }
    }

    /// Adds every purpose to its parent. Purpose ids are preordered, so going backwards adds each child exactly once.
    fn roll_up(&mut self, book: &Book<'_>) {
        for purpose in (0..book.purposes.len()).rev().map(|index| Id::<Purpose>::new(index as u32)) {
            let Some(parent) = book.purposes.parent(purpose) else { continue };
            self.purposes.add_row(purpose.index(), parent.index());
            self.active[parent.index()] |= self.active[purpose.index()];
        }
    }

    /// The purposes that had something, each under its parent, with the objects they are of and what had no purpose.
    fn section(&self, lens: Lens<'s, '_, '_, '_>, periods: Periods) -> Section<'s> {
        let book = lens.book();
        let mut section = table("Purpose", &periods);
        let objects = self.objects.moved(book);
        let mut cursor = ObjectCursor { objects: &objects, next: 0 };
        for root in book.purposes.roots() {
            for id in book.purposes.subtree(root).filter(|id| self.active[id.index()]) {
                self.push_purpose(lens, periods, &mut section, id, cursor.take(id));
            }
        }
        self.unclassified.push_rows(lens, periods, &mut section);
        section
    }

    /// A purpose's row, its facts, and a row below it for each object it is of.
    fn push_purpose(
        &self,
        lens: Lens<'s, '_, '_, '_>,
        periods: Periods,
        section: &mut Section<'s>,
        id: Id<Purpose>,
        objects: &[(Id<Purpose>, Object, usize)],
    ) {
        let book = lens.book();
        let purpose = &book.purposes[id];
        let (name, concept) = (book.name(purpose.name), root_names(purpose.root).1);
        let values = self.purposes.row(id.index());
        let depth = book.purposes.depth(id) as usize;
        let style = if book.purposes.parent(id).is_none() { Style::Total } else { Style::Normal };
        section.push(row(book, Cell::Name(name), depth, values, style));
        add_facts(section, lens, periods, concept, Some(name), values);
        for &(_, object, at) in objects {
            let (name, amounts) = (object_name(book, object), self.objects.grid.row(at));
            let label = Cell::list(" ", [Cell::Word("of"), Cell::Name(name)]);
            section.push(row(book, label, depth + 1, amounts, Style::Muted));
            add_facts(section, lens, periods, concept, Some(name), amounts);
        }
    }
}

/// What flows of a purpose that are of some object add up to: a row for each purpose and object.
struct Objects {
    /// Each row's purpose and object, in the order the rows were made.
    keys: Vec<(Id<Purpose>, Object)>,
    rows: Map<(Id<Purpose>, Object), usize>,
    grid: Grid,
}

impl Objects {
    /// The row for what `purpose` does to `object`, made when it is first asked for.
    fn row(&mut self, purpose: Id<Purpose>, object: Object) -> usize {
        *self.rows.entry((purpose, object)).or_insert_with(|| {
            self.keys.push((purpose, object));
            self.grid.push()
        })
    }

    /// The objects with something in the window, by purpose and then by name: purpose, object and row.
    fn moved(&self, book: &Book<'_>) -> Vec<(Id<Purpose>, Object, usize)> {
        let mut moved: Vec<_> = self
            .keys
            .iter()
            .enumerate()
            .filter(|&(at, _)| !is_zero(self.grid.row(at)))
            .map(|(at, &(purpose, object))| (purpose, object, at))
            .collect();
        moved.sort_by(|&(left, left_object, _), &(right, right_object, _)| {
            left.cmp(&right).then_with(|| object_name(book, left_object).cmp(object_name(book, right_object)))
        });
        moved
    }
}

/// Walks the objects of purposes in purpose order, as the purposes are walked in the same order.
struct ObjectCursor<'a> {
    objects: &'a [(Id<Purpose>, Object, usize)],
    next: usize,
}

impl<'a> ObjectCursor<'a> {
    /// The objects of `purpose`, passing those of any purpose before it.
    fn take(&mut self, purpose: Id<Purpose>) -> &'a [(Id<Purpose>, Object, usize)] {
        let rest = &self.objects[self.next..];
        let before = rest.partition_point(|&(of, ..)| of < purpose);
        let of = rest[before..].partition_point(|&(found, ..)| found == purpose);
        self.next += before + of;
        &rest[before..before + of]
    }
}

/// What flows without a purpose add up to, in all and by description.
struct Unclassified<'s> {
    all: Vec<Qty>,
    /// The row of `grid` for each description (or none), in description order.
    rows: BTreeMap<Option<&'s str>, usize>,
    grid: Grid,
    /// Whether a row had a flow that moved something in the window.
    active: Vec<bool>,
}

impl<'s> Unclassified<'s> {
    fn row(&mut self, description: Option<&'s str>) -> usize {
        *self.rows.entry(description).or_insert_with(|| {
            self.active.push(false);
            self.grid.push()
        })
    }

    /// What no purpose explains: a total row, and a row for each description that moved something.
    fn push_rows(&self, lens: Lens<'s, '_, '_, '_>, periods: Periods, section: &mut Section<'s>) {
        if is_zero(&self.all) && !self.active.contains(&true) {
            return;
        }
        let book = lens.book();
        section.push(row(book, Cell::Word("Unclassified"), 0, &self.all, Style::Total));
        add_facts(section, lens, periods, "unclassified", None, &self.all);
        for (&description, &at) in self.rows.iter().filter(|&(_, &at)| self.active[at]) {
            let label = description.map_or(Cell::Word("unclassified"), Cell::text);
            section.push(row(book, label, 1, self.grid.row(at), Style::Normal));
            add_facts(section, lens, periods, "unclassified", description, self.grid.row(at));
        }
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

pub(crate) fn movement_in_base(
    lens: Lens<'_, '_, '_, '_>,
    posting: Posting<'_>,
    root: Option<PurposeRoot>,
) -> Option<Qty> {
    movement_in_base_with(lens, posting, root, &mut MovementShares::default())
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

pub(crate) fn movement_in_base_with(
    lens: Lens<'_, '_, '_, '_>,
    posting: Posting<'_>,
    root: Option<PurposeRoot>,
    shares: &mut MovementShares,
) -> Option<Qty> {
    let flow = posting.flow;
    let from_outside = lens.book().places[flow.from].class == Class::Outside;
    let to_outside = lens.book().places[flow.to].class == Class::Outside;
    let inbound = from_outside && !to_outside;
    let (place, amount, signed_qty) = if inbound {
        let amount = posting.arrive();
        (flow.to, amount, amount.qty)
    } else {
        let amount = posting.out();
        let signed = Qty(amount.qty.0.checked_neg()?);
        (flow.from, amount, signed)
    };
    // A flow's declared owner is only the primary owner. The physical end it
    // moves through carries the effective ownership shares, so scope the
    // quantity there before pricing it. This also keeps a foreign-currency
    // movement's displayed share aligned with the unit actually posted.
    let amount = axiom_model::Amount::new(
        shares.split(lens, place, axiom_model::Amount::new(signed_qty, amount.unit)),
        amount.unit,
    );
    let amount = lens.on(flow.day).value(amount)?;
    match root {
        Some(PurposeRoot::Income) => Some(amount),
        Some(PurposeRoot::Spending | PurposeRoot::Capital) => Some(Qty(amount.0.checked_neg()?)),
        Some(PurposeRoot::Transfer) | None => Some(Qty(amount.0.checked_abs()?)),
    }
}

/// The physical endpoint whose amount is used by income/spending reports.
/// Incoming flows use what reached the owned end; all other flows use what
/// left the source, matching `movement_in_base`'s direction convention.
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

fn add_recognized(values: &mut [Qty], periods: Periods, recognized: Days, cutoff: Day, amount: Qty) {
    if !amount.is_zero() {
        spread_over(periods, recognized, cutoff, amount, |index, part| values[index] += part);
    }
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
