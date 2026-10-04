//! A flow as the fold sees it: oriented, with its quantities solved, on the
//! day it takes effect.
//!
//! Journal flows, applied flows, reversals (a returned deposit runs its flow
//! backwards) and the flows an assertion posts to close a gap all become a
//! `Motion`, so exactly one code path moves value.

use axiom_core::{Day, Days, Id, Loc, Qty, Sym};
use axiom_model::{
    Amount, Assert, Book, Class, Detail, Entity, Flow, FlowCodes, FlowView, Mode, Offspring, Place, Purposed,
    RuntimeTxn, Select, Text, Waive,
};

use crate::Cause;

/// What leaves and what arrives, once every `?`, `=` and `all` is solved.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) struct Amounts {
    pub out: Qty,
    pub arrive: Qty,
}

impl Amounts {
    /// The quantities as written (zero where the source said `?`).
    pub fn written(flow: &Flow) -> Amounts {
        Amounts { out: flow.out.qty, arrive: flow.arrive.qty }
    }
}

/// What a flow does to the parcels it touches.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Moves {
    /// Value crosses from one place to another: relief, realization, arrival.
    Value,
    /// A market raises what an asset is worth: the growth arrives with no basis.
    Growth,
    /// A market lowers it: parcels shrink and leave their basis behind, an
    /// unrealized loss. Nothing is realized, and nothing is spent.
    Loss,
}

impl Moves {
    fn of(book: &Book, from: Id<Place>, to: Id<Place>) -> Moves {
        let market = book.entities[book.roots.market].place;
        match (book.places[from].class, book.places[to].class, Some(from) == market, Some(to) == market) {
            (Class::Outside, Class::Asset, true, _) => Moves::Growth,
            (Class::Asset, Class::Outside, _, true) => Moves::Loss,
            _ => Moves::Value,
        }
    }

    fn reversed(self) -> Moves {
        match self {
            Moves::Growth => Moves::Loss,
            Moves::Loss => Moves::Growth,
            Moves::Value => Moves::Value,
        }
    }
}

/// Which way a flow runs: as it was written, or backwards, because the flow it was is returned.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Course {
    Forward,
    Back,
}

pub(crate) struct Motion<'f> {
    /// The immutable journal metadata, borrowed through the book's pooled view.
    /// Keeping one view avoids copying selector, code, and rare-detail data.
    pub view: Option<FlowView<'f>>,
    pub code_runs: FlowCodes,
    pub cause: Cause,
    pub course: Course,
    pub day: Day,
    pub recognized: Days,
    pub from: Id<Place>,
    pub to: Id<Place>,
    /// The places at the two ends, so nobody looks them up again.
    pub source: &'f Place,
    pub target: &'f Place,
    pub out: Amount,
    pub arrive: Amount,
    pub txn: RuntimeTxn,
    /// Source-order flow index within a journal transaction or materialized
    /// contract occurrence. Asset-part identities use this in addition to the
    /// transaction identity.
    pub flow_ordinal: u32,
    pub owner: Id<Entity>,
    pub payee: Option<Id<Entity>>,
    pub purpose: Option<Purposed>,
    pub description: Option<Text>,
    pub moves: Moves,
    /// An `opening` line: value moves, but no law sees it and no total counts it.
    pub opening: bool,
    pub waive: Option<Waive>,
    pub loc: Loc,
}

impl<'f> Motion<'f> {
    pub fn new(book: &'f Book, flow: &'f Flow, cause: Cause, day: Day, amounts: Amounts) -> Motion<'f> {
        let txn = RuntimeTxn::journal(flow.txn).expect("journal motion cannot use the template transaction sentinel");
        let flow_ordinal = match cause {
            Cause::Flow(id) => book.txns.get(flow.txn).and_then(|txn| txn.offset(id)).unwrap_or(0),
            _ => 0,
        };
        Motion::from_view_at(book, book.flow_view(flow), txn, cause, day, amounts, flow_ordinal)
    }

    pub fn from_view(
        book: &'f Book,
        view: FlowView<'f>,
        txn: RuntimeTxn,
        cause: Cause,
        day: Day,
        amounts: Amounts,
    ) -> Motion<'f> {
        Motion::from_view_at(book, view, txn, cause, day, amounts, 0)
    }

    pub fn from_view_at(
        book: &'f Book,
        view: FlowView<'f>,
        txn: RuntimeTxn,
        cause: Cause,
        day: Day,
        amounts: Amounts,
        flow_ordinal: u32,
    ) -> Motion<'f> {
        let flow = &*view;
        let (source, target) = (&book.places[flow.from], &book.places[flow.to]);
        Motion {
            view: Some(view),
            code_runs: view.code_runs(),
            cause,
            course: Course::Forward,
            day,
            recognized: flow.recognized,
            from: flow.from,
            to: flow.to,
            source,
            target,
            out: Amount::new(amounts.out, flow.out.unit),
            arrive: Amount::new(amounts.arrive, flow.arrive.unit),
            txn,
            flow_ordinal,
            owner: flow.owner,
            payee: flow.payee,
            purpose: flow.purpose,
            description: flow.description,
            moves: Moves::of(book, flow.from, flow.to),
            opening: flow.mode == Mode::Opening,
            waive: flow.waive,
            loc: flow.loc,
        }
    }

    /// A flow a law derived, posting on `day`: a transaction of its own, which says which, and the cause of what it causes.
    pub fn derived(book: &'f Book, id: Id<Offspring>, offspring: &'f Offspring, day: Day) -> Motion<'f> {
        let flow = &offspring.flow;
        let (txn, cause) = (RuntimeTxn::Derived(id), Cause::Derived(id));
        Motion::from_view_at(book, book.flow_view(flow), txn, cause, day, Amounts::written(flow), 0)
    }

    /// What an assertion posts to close a gap: `moved` arrives at the asserted
    /// place from `counter`, or, negative, leaves it for `counter`. The parcels
    /// belong to the transaction that last touched the place.
    pub fn pad(book: &'f Book, assert: &Assert, counter: Id<Place>, moved: Qty, waive: Option<Waive>) -> Motion<'f> {
        let (from, to) = if moved.is_negative() { (assert.place, counter) } else { (counter, assert.place) };
        let (source, target) = (&book.places[from], &book.places[to]);
        let amount = Amount::new(moved.abs(), assert.amount.unit);
        let touching = &book.touching[assert.place];
        let last = touching.partition_point(|&id| book.flows[id].day <= assert.day);
        let txn = last.checked_sub(1).map_or(RuntimeTxn::Adjustment { place: assert.place, day: assert.day }, |at| {
            RuntimeTxn::journal(book.flows[touching[at]].txn)
                .expect("a Book flow cannot use the template transaction sentinel")
        });
        Motion {
            cause: Cause::Time,
            course: Course::Forward,
            day: assert.day,
            recognized: Days::on(assert.day),
            from,
            to,
            source,
            target,
            out: amount,
            arrive: amount,
            txn,
            flow_ordinal: u32::MAX,
            owner: source.owner,
            payee: None,
            purpose: None,
            description: None,
            view: None,
            code_runs: FlowCodes {
                header: axiom_core::Run::new(Id::new(0), 0),
                local: axiom_core::Run::new(Id::new(0), 0),
            },
            moves: Moves::of(book, from, to),
            opening: false,
            waive,
            loc: assert.loc,
        }
    }

    /// The flow as it runs in `course`: as written, or back.
    pub fn running(self, course: Course) -> Motion<'f> {
        match course {
            Course::Forward => self,
            Course::Back => self.reversed(),
        }
    }

    /// The same value moving back: what arrived leaves, and comes home. The
    /// original's lot selectors chose parcels at the other end and mean
    /// nothing here.
    pub fn reversed(&self) -> Motion<'f> {
        let (from, to) = (self.to, self.from);
        let (source, target) = (self.target, self.source);
        let course = Course::Back;
        Motion {
            from,
            to,
            source,
            target,
            out: self.arrive,
            arrive: self.out,
            moves: self.moves.reversed(),
            course,
            ..*self
        }
    }

    /// Whether the laws that watch the flow may derive others from it. A flow run backwards has its derived flows
    /// reversed and derives nothing anew, and a flow the run makes to be consistent with itself (a pad, a claim the
    /// monitor makes: caused by time) or one no law sees (an opening) causes nothing.
    pub fn derives(&self) -> bool {
        self.course == Course::Forward && !self.opening && self.cause != Cause::Time
    }

    pub fn is_exchange(&self) -> bool {
        self.out.unit != self.arrive.unit
    }

    pub fn select(&self) -> &'f [Select] {
        self.view.map_or(&[], FlowView::select)
    }

    pub fn detail(&self) -> &'f Detail {
        self.view.map_or(&Detail::NONE, FlowView::detail)
    }

    pub fn codes(&self) -> impl Iterator<Item = Sym> + 'f {
        self.view.into_iter().flat_map(FlowView::codes)
    }
}
