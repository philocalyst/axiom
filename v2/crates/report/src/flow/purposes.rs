//! `flow` for a book with purposes: income, spending and capital by the
//! purpose tree, or by the parties the money went to, and beneath them what was
//! measured (hours, miles) by purpose and owner.
//!
//! Which flows count is decided by who bears them (`Flow::owner`), so a shared
//! flow shows each owner's share under that owner and the whole under everyone.
//! Direction comes from the ends: value from outside the owners is coming in,
//! value to outside is going out, and a flow's purpose says what kind of thing
//! it was. A flow that says no purpose is grouped by what it says of itself.

use std::collections::HashMap;
use std::hash::Hash;

use axiom_core::{Day, Days, Id, Qty, Sym, spread};
use axiom_model::{Action, Amount, Book, Class, Commodity, Entity, Object, Place, Purpose, PurposeRoot};

use super::{Grid, Group, add_into, is_zero, net_row, row, table};
use crate::calendar::Periods;
use crate::history::{Posting, postings};
use crate::lens::{Lens, Priced};
use crate::places::path;
use crate::{Cell, Money, Report, Row, Section, Style, When};

pub fn report<'s>(lens: Lens<'_, 's>, periods: Periods, group: Group, cutoff: Day) -> Report<'s> {
    let statement = Statement::compile(lens, periods, cutoff);
    let report = Report::new("Income and spending").with(statement.section(lens, group));
    match measured(lens, &periods, cutoff) {
        Some(measured) => report.with(measured),
        None => report,
    }
}

/// Who the other end of a flow is: its payee, else the place outside.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Party {
    Entity(Id<Entity>),
    Place(Id<Place>),
}

impl Party {
    fn label<'s>(self, book: &Book<'s>) -> &'s str {
        match self {
            Party::Entity(entity) => book.name(book.entities[entity].path),
            Party::Place(place) => path(book, place),
        }
    }
}

/// Figures per period under some key.
type ByPeriod<K> = HashMap<K, Vec<Qty>>;

fn bump<K: Hash + Eq>(map: &mut ByPeriod<K>, key: K, periods: usize, period: usize, qty: Qty) {
    map.entry(key).or_insert_with(|| vec![Qty::ZERO; periods])[period] += qty;
}

struct Statement {
    periods: Periods,
    /// Recognition stops here: nothing after the window's end has happened.
    cutoff: Day,
    /// Every flow that says a purpose, as its root reads it: income and
    /// spending both come out positive.
    grid: Grid,
    /// What a purpose was of (`improvement` of the condo): a breakdown of the
    /// grid's rows, not more of them.
    objects: ByPeriod<(Id<Purpose>, Object)>,
    /// Flows that say no purpose, by side and what they say of themselves.
    loose: ByPeriod<(PurposeRoot, Option<Sym>)>,
    /// Every flow by side and party.
    parties: ByPeriod<(PurposeRoot, Party)>,
    /// Realized gains per period, derived from the run's parcels.
    gains: Vec<Qty>,
    spread_seen: bool,
    unpriced: Priced,
}

impl Statement {
    fn compile(lens: Lens, periods: Periods, cutoff: Day) -> Statement {
        let (book, run) = (lens.book, lens.run);
        let mut statement = Statement {
            periods,
            cutoff,
            grid: Grid::new(book.purposes.len(), periods.len()),
            objects: ByPeriod::new(),
            loose: ByPeriod::new(),
            parties: ByPeriod::new(),
            gains: vec![Qty::ZERO; periods.len()],
            spread_seen: false,
            unpriced: Priced::default(),
        };
        for posting in postings(book, run).filter(|posting| posting.is_real_on(cutoff)) {
            statement.record(lens, &posting);
        }
        for gain in run.gains.iter().filter(|gain| gain.day <= cutoff && lens.owns(gain.from)) {
            for period in periods.overlapping(gain.day, gain.day) {
                statement.gains[period] += gain.gain();
            }
        }
        statement
    }

    /// Books a flow under its purpose and party, in each period it is
    /// recognized over.
    fn record(&mut self, lens: Lens, posting: &Posting) {
        let (book, flow) = (lens.book, posting.flow);
        if !lens.whose.includes(flow.owner) {
            return;
        }
        let outside = |place: Id<Place>| book.places[place].class == Class::Outside;
        let (inbound, outbound) = (outside(flow.from) && !outside(flow.to), outside(flow.to) && !outside(flow.from));
        let purposed = flow.purpose.map(|purposed| (purposed, book.purposes[purposed.purpose].root));
        let root = match purposed {
            Some((_, root)) => root,
            None if inbound => PurposeRoot::Income,
            None if outbound => PurposeRoot::Spending,
            None => return,
        };
        let worth = if inbound { posting.arrive_in_base(lens) } else { posting.out_in_base(lens) };
        let Some(worth) = self.unpriced.add(worth) else { return };
        // Income comes in and the rest goes out; the other way round is a reversal (a refund).
        let counts = root == PurposeRoot::Transfer || (root == PurposeRoot::Income) == inbound;
        let recognized = if counts { worth } else { -worth };
        self.spread_seen |= flow.recognized.last() > flow.day;
        let party = flow.payee.map_or(Party::Place(if inbound { flow.from } else { flow.to }), Party::Entity);
        let over = flow.recognized;
        for period in self.periods.overlapping(over.first(), over.last()) {
            let window = self.periods.window(period).days();
            let Some(happened) = Days::new(window.first(), window.last().min(self.cutoff)) else { continue };
            let part = spread(recognized, over, happened);
            let n = self.periods.len();
            bump(&mut self.parties, (root, party), n, period, part);
            match purposed {
                Some((purposed, _)) => {
                    self.grid.add(purposed.purpose, period, part);
                    if let Some(of) = purposed.of {
                        bump(&mut self.objects, (purposed.purpose, of), n, period, part);
                    }
                }
                None => bump(&mut self.loose, (root, flow.description), n, period, part),
            }
        }
    }

    fn section<'s>(&self, lens: Lens<'_, 's>, group: Group) -> Section<'s> {
        let book = lens.book;
        let mut section = table(if group == Group::Party { "Party" } else { "Purpose" }, &self.periods);
        let side = |section: &mut Section<'s>, root| match group {
            Group::Purpose => self.by_purpose(lens, section, root),
            Group::Party => self.by_party(lens, section, root),
        };
        let income = side(&mut section, PurposeRoot::Income);
        let spending = side(&mut section, PurposeRoot::Spending);
        let net: Vec<Qty> = income.iter().zip(&spending).map(|(&earned, &spent)| earned - spent).collect();
        section.push(net_row(book, &net));
        // What joined things the owners keep, and what only passed through them, is not net.
        side(&mut section, PurposeRoot::Capital);
        side(&mut section, PurposeRoot::Transfer);
        if !is_zero(&self.gains) {
            section.note(
                "≈ Realized gains are derived from the basis of the parcels sold; the journal does not state them.",
            );
        }
        if self.spread_seen {
            section.note(
                "Flows written over a date range are recognized a little each day across the periods they cover.",
            );
        }
        section.unpriced(self.unpriced.missing(), "flow");
        section
    }

    /// The rows of one root's purposes and, after them, the flows that say no
    /// purpose, by what they say. Returns the side's total.
    fn by_purpose<'s>(&self, lens: Lens<'_, 's>, section: &mut Section<'s>, root: PurposeRoot) -> Vec<Qty> {
        let book = lens.book;
        let heads: Vec<Id<Purpose>> = book.purposes.roots().filter(|&head| book.purposes[head].root == root).collect();
        let mut loose: Vec<_> = self.loose.iter().filter(|((side, _), _)| *side == root).collect();
        // Described flows by what they say; the rest last.
        loose.sort_by_key(|((_, said), _)| (said.is_none(), said.map(|said| book.name(said))));
        // The first head takes the flows that say no purpose.
        let mut unsaid = vec![Qty::ZERO; self.periods.len()];
        loose.iter().for_each(|(_, values)| add_into(&mut unsaid, values));
        let mut total = unsaid.clone();
        heads.iter().for_each(|&head| add_into(&mut total, &self.grid.subtree(&book.purposes, head)));
        if is_zero(&total) {
            return total;
        }
        for (nth, &head) in heads.iter().enumerate() {
            for purpose in book.purposes.subtree(head) {
                let mut values = self.grid.subtree(&book.purposes, purpose);
                if purpose == head && nth == 0 {
                    add_into(&mut values, &unsaid);
                }
                if is_zero(&values) {
                    continue;
                }
                let (name, depth) = (book.name(book.purposes[purpose].name), book.purposes.depth(purpose) as usize);
                let style = if purpose == head { Style::Total } else { Style::Normal };
                self.emit(lens, section, root, Cell::Name(name), depth, &values, style, Some(name));
                let mut of: Vec<_> = self.objects.iter().filter(|((owner, _), _)| *owner == purpose).collect();
                of.sort_by_key(|((_, object), _)| object_name(book, *object));
                for ((_, object), values) in of {
                    let label = ["of".into(), Cell::Name(object_name(book, *object))].into();
                    self.emit(lens, section, root, label, depth + 1, values, Style::Muted, None);
                }
            }
        }
        for ((_, said), values) in loose {
            let label = said.map_or("unclassified".into(), |said| Cell::Text(book.name(said)));
            let of = Some(said.map_or("unclassified", |said| book.name(said)));
            self.emit(lens, section, root, label, 1, values, Style::Normal, of);
        }
        total
    }

    /// One side's parties, the biggest first, under the side's total.
    fn by_party<'s>(&self, lens: Lens<'_, 's>, section: &mut Section<'s>, root: PurposeRoot) -> Vec<Qty> {
        let book = lens.book;
        let mut parties: Vec<_> = self.parties.iter().filter(|((side, _), _)| *side == root).collect();
        let size = |values: &[Qty]| -values.iter().map(|qty| qty.0.abs()).sum::<i64>();
        parties.sort_by_key(|((_, party), values)| (size(values), party.label(book)));
        let mut total = vec![Qty::ZERO; self.periods.len()];
        parties.iter().for_each(|(_, values)| add_into(&mut total, values));
        if is_zero(&total) {
            return total;
        }
        let head = book.purposes.roots().find(|&head| book.purposes[head].root == root);
        let title = head.map_or("", |head| book.name(book.purposes[head].name));
        self.emit(lens, section, root, Cell::Name(title), 0, &total, Style::Total, None);
        for ((_, party), values) in parties {
            let name = party.label(book);
            self.emit(lens, section, root, Cell::Name(name), 1, values, Style::Normal, Some(name));
        }
        total
    }

    /// A row of the statement, and each of its figures as a fact when it has a subject.
    #[allow(clippy::too_many_arguments)]
    fn emit<'s>(
        &self,
        lens: Lens<'_, 's>,
        section: &mut Section<'s>,
        root: PurposeRoot,
        label: Cell<'s>,
        depth: usize,
        values: &[Qty],
        style: Style,
        of: Option<&'s str>,
    ) {
        let book = lens.book;
        let concept = match root {
            PurposeRoot::Income => "income",
            PurposeRoot::Spending => "spending",
            PurposeRoot::Capital => "capital",
            PurposeRoot::Transfer => "transfer",
        };
        for (index, &qty) in values.iter().enumerate().filter(|(_, qty)| !qty.is_zero()).filter(|_| of.is_some()) {
            let during = When::During(self.periods.window(index).days());
            section.fact(concept, of, lens.whose.label(book), during, Money::base(book, qty));
        }
        section.push(row(book, label, depth, values, style));
    }
}

fn object_name<'s>(book: &Book<'s>, object: Object) -> &'s str {
    match object {
        Object::Asset(asset) => book.name(book.assets[asset].name),
        Object::Place(place) => path(book, place),
        Object::Entity(entity) => book.name(book.entities[entity].path),
    }
}

/// Work done and things used, by purpose and owner, in their own units.
fn measured<'s>(lens: Lens<'_, 's>, periods: &Periods, cutoff: Day) -> Option<Section<'s>> {
    let book = lens.book;
    // Used rather than worked, last.
    type Key = (Option<Id<Purpose>>, Id<Entity>, Id<Commodity>, bool);
    let mut rows: ByPeriod<Key> = ByPeriod::new();
    for measure in book.measures.values().filter(|measure| measure.day <= cutoff && lens.whose.includes(measure.owner))
    {
        let key = (
            measure.purpose.map(|purposed| purposed.purpose),
            measure.owner,
            measure.quantity.unit,
            measure.action == Action::Use,
        );
        for period in periods.overlapping(measure.day, measure.day) {
            bump(&mut rows, key, periods.len(), period, measure.quantity.qty);
        }
    }
    if rows.is_empty() {
        return None;
    }
    let mut section = table("Purpose", periods).headed("Measured");
    let mut rows: Vec<_> = rows.into_iter().collect();
    rows.sort_by_key(|&((purpose, owner, unit, used), _)| {
        (purpose.map_or(usize::MAX, |purpose| purpose.index()), owner.index(), unit.index(), used)
    });
    for ((purpose, owner, unit, used), values) in rows {
        let concept = if used { "used" } else { "worked" };
        let name = purpose.map_or("unclassified", |purpose| book.name(book.purposes[purpose].name));
        let whose = book.name(book.entities[owner].path);
        let amount = |qty: Qty| Cell::amount(book, Amount::new(qty, unit));
        let cells = values.iter().map(|&qty| if qty.is_zero() { Cell::Blank } else { amount(qty) });
        let total = (values.len() > 1).then(|| amount(values.iter().copied().sum()));
        let label = [Cell::Name(name), "for".into(), Cell::Name(whose)].into();
        section.push(Row::new(std::iter::once(label).chain(cells).chain(total)));
        for (index, &qty) in values.iter().enumerate().filter(|(_, qty)| !qty.is_zero()) {
            let during = When::During(periods.window(index).days());
            section.fact(concept, Some(name), whose, during, Money::of(book, Amount::new(qty, unit)));
        }
    }
    Some(section)
}
