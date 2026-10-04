//! Where value rests, and how it leaves: one [`Slot`] per `(place, commodity)`.
//!
//! A place holds a handful of commodities, usually one or two. Each place
//! therefore threads a short chain of slots, sorted by commodity, through one
//! flat vector. A lookup is one index and a hop or two, with no hashing;
//! iteration by place then commodity is the chain order; a new pair costs one
//! push; and cloning is two flat copies.
//!
//! A slot is the public [`Holding`] plus what keeps a sale from scanning it.
//! Relief is one ranking and one way of taking: every policy orders what may
//! leave by [`Candidate::rank`] (the tie's colour, what the policy takes first,
//! then basis per unit and position) and takes in that order, all of one
//! before the next, or pro rata across a colour. Sorting every lot for every
//! sale is what that means, and on a position bought 100,000 times it is
//! 150 times slower than not looking: so when nothing is tied and no selector
//! narrows, the first candidates of FIFO, LIFO and HIFO come from where they
//! already are. Lots are kept oldest first, so FIFO takes from the front (past
//! a cursor over the exhausted ones) and LIFO from the back, and HIFO asks a
//! heap in the ranking's order, built when first needed and told of every
//! change to a lot. Everything else (`exact`, `prorata`, selectors, ties) is
//! gathered and sorted.
//!
//! Exhausted lots are left in place while the fold runs and swept out when
//! control returns to the caller, so the holdings a caller sees are always
//! clean.

use std::cmp::{Ordering, Reverse};
use std::collections::BinaryHeap;
use std::hash::{Hash, Hasher};
use std::iter::successors;
use std::ops::{Deref, Range};

use axiom_core::{Arena, Day, Id, Qty, Ratio, Sym};
use axiom_model::{Commodity, Entity, FlowCodes, Place, Policy, RuntimeTxn, Select};

use crate::{AssetError, Holding, Parcel, PartId};

const NONE: u32 = u32::MAX;

/// Sweeping a slot mid-fold costs a pass over its lots, so it waits until
/// half of them are exhausted.
const SWEEP_AT: usize = 32;

fn same_codes(left: FlowCodes, right: FlowCodes, pool: &Arena<Sym>) -> bool {
    let in_left = |code: &Sym| pool[left.header].contains(code) || pool[left.local].contains(code);
    let in_right = |code: &Sym| pool[right.header].contains(code) || pool[right.local].contains(code);
    pool[left.header].iter().chain(&pool[left.local]).all(in_right)
        && pool[right.header].iter().chain(&pool[right.local]).all(in_left)
}

/// What a holding holds, which says what its plain value is worth and when two of its parcels are one: money (base
/// currency outside a claim place) is at its face and told apart by its basis per unit, wherever and whenever it came
/// from; anything else is told apart by the purchase that made it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Held {
    Money,
    Lots,
}

impl Held {
    /// The basis of `qty` of plain value: its face, if it is money; nothing else has basis outside its lots.
    fn plain_basis(self, qty: Qty) -> Qty {
        match self {
            Held::Money => qty,
            Held::Lots => Qty::ZERO,
        }
    }
}

impl Parcel {
    /// Whether `other` cannot be told from this parcel, so that landing it merges the two: the same tie, part and wash-sale
    /// mark and the same codes, and then, for money, the same basis per unit (a 401k's hundreds of zero-basis deferrals
    /// are one lot), for anything else the same purchase. The fields that differ most often are compared first.
    pub(crate) fn is_like(&self, other: &Parcel, held: Held, pool: &Arena<Sym>) -> bool {
        let made = match held {
            Held::Money => self.basis.0 as i128 * other.qty.0 as i128 == other.basis.0 as i128 * self.qty.0 as i128,
            Held::Lots => (self.txn, self.acquired, self.held_since) == (other.txn, other.acquired, other.held_since),
        };
        made && (self.tied, self.part, self.wash_matched) == (other.tied, other.part, other.wash_matched)
            && same_codes(self.codes, other.codes, pool)
    }
}

/// Part of the value in flight: the parcel that left a place (with the basis relieved with it), where it came from, and what
/// the flow decides of it on the way.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Slice {
    pub lot: Parcel,
    pub origin: Origin,
    /// What it fetched, in base-currency quanta: what a sale realizes against.
    pub worth: Qty,
    /// The basis it carries into the target, set once the flow's worth is known.
    pub carried: Qty,
}

/// Where a slice came from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Origin {
    /// Plain money: base currency at its face.
    Plain,
    /// A parcel with an identity of its own.
    Lot,
    /// Nothing gave it: value from an income, equity or liability place, or
    /// what a sale asked for beyond what was held.
    Fresh,
}

impl Slice {
    fn of(lot: Parcel, origin: Origin) -> Slice {
        Slice { lot, origin, worth: Qty::ZERO, carried: Qty::ZERO }
    }

    /// Value that nothing gave up, of the codes `codes`: base currency is at its face, anything else has no basis of its own.
    pub fn fresh(qty: Qty, is_base: bool, now: (Day, RuntimeTxn), codes: FlowCodes) -> Slice {
        Slice::of(Parcel { codes, ..Parcel::new(qty, if is_base { qty } else { Qty::ZERO }, now) }, Origin::Fresh)
    }
}

/// A slice is read as the parcel it carries.
impl Deref for Slice {
    type Target = Parcel;
    fn deref(&self) -> &Parcel {
        &self.lot
    }
}

/// Which part of a holding a piece comes from. Ordered as a holding stores
/// them: plain money, then the lots oldest first.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) enum Source {
    Plain,
    Lot(usize),
}

/// A parcel that could leave.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Candidate {
    pub source: Source,
    pub qty: Qty,
    pub basis: Qty,
    pub acquired: Day,
    /// The transaction that made it; plain money has none.
    pub txn: Option<RuntimeTxn>,
    pub tied: Option<Id<Entity>>,
    /// What the claim it belongs to holds: its own quantity, until `exact` adds up the lines of one transaction.
    claim: Qty,
}

impl Candidate {
    /// Where the candidate stands in the order relief takes, which is one order for every policy: its colour, then what
    /// the policy takes first (for `exact` the claims of the size asked; for HIFO outside money, plain value), then the
    /// heap's order ([`Ranked`]): dearest per unit, then oldest. A policy that does not weigh basis ranks every candidate
    /// the same per unit, so position decides, and LIFO counts positions from the newest.
    fn rank(&self, policy: Option<Policy>, req: &Request) -> (Colour, bool, Reverse<Ranked>) {
        let (plain, at) = match self.source {
            Source::Plain => (true, -1),
            Source::Lot(at) => (false, at as i64),
        };
        let (first, ranked) = match policy {
            Some(Policy::Hifo) => {
                (plain && req.held == Held::Lots, Ranked { basis: self.basis.0, qty: self.qty.0, at })
            }
            Some(Policy::Lifo) => (false, Ranked { at: -at, ..Ranked::SAME }),
            Some(Policy::Exact) => (!plain && self.claim == req.exact, Ranked { at, ..Ranked::SAME }),
            _ => (false, Ranked { at, ..Ranked::SAME }),
        };
        (req.colour(self.tied), !first, Reverse(ranked))
    }
}

/// What relief is asked for.
pub(crate) struct Request<'a> {
    pub need: Qty,
    /// The size of the claim `exact` looks for: `need`, unless the flow is one of several that make one payment.
    pub exact: Qty,
    /// What the place holds of the commodity: money, or lots.
    pub held: Held,
    pub selectors: &'a [Select],
    /// The place's policy; a policy selector on the flow overrides it.
    pub policy: Option<Policy>,
    pub codes: &'a Arena<Sym>,
    /// For each entity a parcel here is tied to: whether its `on spend` laws
    /// permit this flow.
    pub permits: &'a [(Id<Entity>, bool)],
    /// The entity the flow is written out of, whose own parcels leave first.
    pub spender: Option<Id<Entity>>,
    /// The moment, which is when and by what plain money is acquired.
    pub now: (Day, RuntimeTxn),
    /// Whether to list the candidates if the choice turns out to be ambiguous:
    /// asked only then, since it is a lookup.
    pub explain: &'a dyn Fn() -> bool,
}

impl<'a> Request<'a> {
    /// The relief of `need` as the policy says, from a place that is not money and holds nothing tied: what a flow
    /// asks beyond that (a selector, a spender, ties) is written over it.
    pub fn of(need: Qty, policy: Option<Policy>, codes: &'a Arena<Sym>, now: (Day, RuntimeTxn)) -> Request<'a> {
        Request {
            need,
            exact: need,
            held: Held::Lots,
            selectors: &[],
            policy,
            codes,
            permits: &[],
            spender: None,
            now,
            explain: &|| false,
        }
    }
}

/// What a parcel's tie says about when relief takes it. The variants are in
/// the order relief takes them.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
enum Colour {
    /// Tied to the entity the flow is written out of.
    Own,
    /// Tied to an entity whose laws permit the flow.
    Permitted,
    /// Tied to nothing.
    Free,
    /// Tied to an entity whose laws do not permit it: last, and only if nothing
    /// else can pay. A flow written out of an entity spends nobody else's
    /// money, so for it these never leave.
    Refused,
}

impl Request<'_> {
    fn colour(&self, tied: Option<Id<Entity>>) -> Colour {
        match tied {
            None => Colour::Free,
            Some(entity) if Some(entity) == self.spender => Colour::Own,
            Some(entity) if self.permits.contains(&(entity, true)) => Colour::Permitted,
            Some(_) => Colour::Refused,
        }
    }

    /// Whether parcels of `colour` may leave for this request at all.
    fn allows(&self, colour: Colour) -> bool {
        colour != Colour::Refused || self.spender.is_none()
    }
}

/// What relief found. Buffers are reused between requests.
#[derive(Default)]
pub(crate) struct Relief {
    pub slices: Vec<Slice>,
    /// Wanted, not held.
    pub shortfall: Qty,
    /// No rule chose between candidates that differ.
    pub ambiguous: bool,
    /// Those candidates, when asked for.
    pub candidates: Vec<Candidate>,
    plan: Vec<(Source, Qty)>,
    gathered: Vec<Candidate>,
}

/// A holding while the fold runs.
#[derive(Clone)]
pub(crate) struct Slot {
    pub holding: Holding,
    /// `plain` and every lot, kept in step: nobody sums lots to learn a balance.
    pub qty: Qty,
    /// What `qty` was the last time the fold wrote it into the position's history.
    recorded: Qty,
    /// How many lots are tied to an entity: only then are there colours to weigh.
    ties: u32,
    /// What is held is a debt: its lots are negative, the liability they are, and relief is asked in the mirror.
    owes: bool,
    /// Lots before this one are exhausted.
    first: usize,
    /// Exhausted lots not yet swept out.
    dead: usize,
    /// HIFO's order, by position in `holding.lots`. Entries go stale when their
    /// lot changes; a stale entry is discarded when it reaches the top.
    ranked: Option<Box<BinaryHeap<Ranked>>>,
    next: u32,
}

impl Deref for Slot {
    type Target = Holding;
    fn deref(&self) -> &Holding {
        &self.holding
    }
}

/// A candidate as the ranking orders it, greatest first: dearest per unit, then the earliest position. It is the heap's
/// order, and the last key of every policy's.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Ranked {
    basis: i64,
    qty: i64,
    at: i64,
}

impl Ranked {
    /// What a policy that does not weigh basis gives every candidate: one basis per unit.
    const SAME: Ranked = Ranked { basis: 0, qty: 1, at: 0 };

    fn of(at: usize, lot: &Parcel) -> Ranked {
        Ranked { basis: lot.basis.0, qty: lot.qty.0, at: at as i64 }
    }
}

impl Ord for Ranked {
    fn cmp(&self, other: &Ranked) -> Ordering {
        // basis / qty, compared without dividing.
        let by_unit = (self.basis as i128 * other.qty as i128).cmp(&(other.basis as i128 * self.qty as i128));
        by_unit.then(other.at.cmp(&self.at))
    }
}

impl PartialOrd for Ranked {
    fn partial_cmp(&self, other: &Ranked) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Slot {
    fn new(place: Id<Place>, unit: Id<Commodity>, next: u32) -> Slot {
        let holding = Holding { place, unit, plain: Qty::ZERO, lots: Vec::new() };
        Slot {
            holding,
            qty: Qty::ZERO,
            recorded: Qty::ZERO,
            ties: 0,
            owes: false,
            first: 0,
            dead: 0,
            ranked: None,
            next,
        }
    }

    fn live(&self) -> usize {
        self.holding.lots.len() - self.dead
    }

    /// Whether any parcel here is tied to an entity.
    pub fn is_tied(&self) -> bool {
        self.ties > 0
    }

    /// Adds (or, negative, removes) plain value.
    pub fn credit(&mut self, qty: Qty) {
        self.holding.plain += qty;
        self.qty += qty;
    }

    /// Receives a parcel. Money at its face, tied to nothing, is plain; anything else joins the lots, merging into the
    /// one it is like if there is one (its basis adds; the lot keeps its own acquisition day) and otherwise taking its
    /// place among the lots, oldest first. Outside money only a lot acquired the same day can be like it.
    pub fn land(&mut self, parcel: Parcel, held: Held, codes: &Arena<Sym>) {
        if held == Held::Money && parcel.tied.is_none() && parcel.basis == parcel.qty {
            return self.credit(parcel.qty);
        }
        self.qty += parcel.qty;
        let lots = &mut self.holding.lots;
        let (start, end) = match held {
            Held::Money => (self.first, lots.len()),
            Held::Lots => (
                lots.partition_point(|lot| lot.acquired < parcel.acquired),
                lots.partition_point(|lot| lot.acquired <= parcel.acquired),
            ),
        };
        let same = lots[start..end].iter().position(|lot| !lot.qty.is_zero() && lot.is_like(&parcel, held, codes));
        if let Some(found) = same {
            let at = start + found;
            lots[at].qty += parcel.qty;
            lots[at].basis += parcel.basis;
            return self.revalue(at);
        }
        self.ties += u32::from(parcel.tied.is_some());
        self.insert(parcel);
    }

    /// Puts a lot in its place among the lots, oldest first, after any of the same day.
    fn insert(&mut self, parcel: Parcel) {
        // A lot that arrives with no quantity (a rounded share of nothing) is
        // exhausted from the start: `dead` counts every lot of quantity zero.
        self.dead += usize::from(parcel.qty.is_zero());
        let lots = &mut self.holding.lots;
        let at = lots.partition_point(|lot| lot.acquired <= parcel.acquired);
        lots.insert(at, parcel);
        if at + 1 == lots.len() {
            return self.revalue(at);
        }
        // Positions after the new lot moved, and it may be the oldest live one.
        self.first = self.first.min(at);
        self.ranked = None;
    }

    /// Takes `matched` (shares a loss was carried into, which held `basis`) out of lot `at`: the lot becomes them if they are
    /// all of it, and otherwise they land after the lots of their day. The slot's quantity does not change.
    fn split(&mut self, at: usize, matched: Parcel, basis: Qty) {
        self.ranked = None;
        let lot = &mut self.holding.lots[at];
        if matched.qty == lot.qty {
            return *lot = matched;
        }
        (lot.qty, lot.basis) = (lot.qty - matched.qty, lot.basis - basis);
        let to = self.holding.lots.partition_point(|lot| lot.acquired <= matched.acquired);
        self.holding.lots.insert(to, matched);
        self.first = self.first.min(to);
        self.ties += u32::from(matched.tied.is_some());
    }

    /// Tells the heap, if there is one, that lot `at` has a new quantity and basis. A tied lot it is told of is never its
    /// top while it is live: the heap is read only when nothing is tied.
    fn revalue(&mut self, at: usize) {
        if let Some(ranked) = self.ranked.as_mut() {
            ranked.push(Ranked::of(at, &self.holding.lots[at]));
        }
    }

    /// Takes `qty` from part of the holding, and appends what left to `out`.
    fn take(&mut self, source: Source, qty: Qty, req: &Request, out: &mut Relief) {
        let slice = match source {
            Source::Plain => {
                self.holding.plain -= qty;
                Slice::of(Parcel::new(qty, req.held.plain_basis(qty), req.now), Origin::Plain)
            }
            Source::Lot(at) => {
                let lot = &mut self.holding.lots[at];
                let basis = if qty == lot.qty {
                    lot.basis
                } else {
                    lot.basis.share(qty, lot.qty).expect("a part of a basis fits")
                };
                let slice = Slice::of(Parcel { qty, basis, ..*lot }, Origin::Lot);
                lot.qty -= qty;
                lot.basis -= basis;
                if lot.qty.is_zero() {
                    self.dead += 1;
                    self.ties -= u32::from(lot.tied.is_some());
                } else {
                    self.revalue(at);
                }
                slice
            }
        };
        out.slices.push(slice);
    }

    /// What is owed, held as the liability it is: a negative parcel, as a place holds what flowed into it less what flowed out.
    pub fn owe(&mut self, owed: Parcel, codes: &Arena<Sym>) {
        self.owes = true;
        self.land(Parcel { qty: -owed.qty, basis: -owed.basis, ..owed }, Held::Lots, codes);
    }

    /// Puts back a parcel that relief took: a debt's, as it was owed.
    pub fn restore(&mut self, parcel: Parcel, codes: &Arena<Sym>) {
        match self.owes {
            true => self.owe(parcel, codes),
            false => self.land(parcel, Held::Lots, codes),
        }
    }

    /// Turns what is held into what is owed and back: the rules of relief are written for parcels of a positive quantity, so a
    /// debt is relieved as seen in a mirror, where paying it is taking from it, and paying more than is owed is the shortfall that
    /// comes out of plain value, which seen the right way round is the credit it leaves.
    fn mirror(&mut self) {
        self.holding.plain = -self.holding.plain;
        self.qty = -self.qty;
        for lot in &mut self.holding.lots {
            (lot.qty, lot.basis) = (-lot.qty, -lot.basis);
        }
        self.ranked = None;
    }

    /// Removes `req.need` from the holding, choosing what leaves by the
    /// request's selectors, ties and policy (in that order of precedence), and
    /// says what left. What could not be covered is `out.shortfall`, and comes
    /// out of plain value, so a holding can go negative. A debt is paid, not
    /// taken from: the same, in the mirror.
    pub fn relieve(&mut self, req: &Request, out: &mut Relief) {
        if self.owes {
            self.mirror();
            self.relieve_held(req, out);
            return self.mirror();
        }
        self.relieve_held(req, out);
    }

    fn relieve_held(&mut self, req: &Request, out: &mut Relief) {
        out.slices.clear();
        out.candidates.clear();
        out.ambiguous = false;
        out.shortfall = req.need;
        let selection = Selection { selectors: req.selectors, codes: req.codes };
        // Plain money that covers the need: the common case, and no choice to make.
        if self.holding.lots.is_empty() && self.holding.plain >= req.need && !selection.constrains() {
            self.take(Source::Plain, req.need, req, out);
            out.shortfall = Qty::ZERO;
            self.qty -= req.need;
            return;
        }
        let policy = selection.policy().or(req.policy);
        let in_order = !selection.constrains()
            && !self.is_tied()
            && match policy {
                Some(Policy::Hifo) => req.held == Held::Lots,
                Some(Policy::Exact | Policy::Prorata) => false,
                None | Some(Policy::Fifo | Policy::Lifo) => true,
            };
        out.shortfall = match in_order {
            true => self.take_in_order(req, policy, out),
            false => self.take_ranked(req, &selection, policy, out),
        };
        self.holding.plain -= out.shortfall;
        self.qty -= req.need;
        self.sweep_ends();
    }

    /// FIFO, LIFO, HIFO outside money and no policy, from a holding where nothing is tied and no selector narrows what may
    /// leave: the first candidates in the order of the ranking are plain value and then the oldest lot past the cursor, the
    /// newest from the back (plain value last), or the dearest per unit from the heap, so only what leaves is looked at. Says
    /// what was not covered.
    fn take_in_order(&mut self, req: &Request, policy: Option<Policy>, out: &mut Relief) -> Qty {
        let takeable = self.qty - self.holding.plain.min(Qty::ZERO);
        let candidates = usize::from(self.holding.plain > Qty::ZERO) + self.live();
        out.ambiguous = policy.is_none() && candidates > 1 && req.need < takeable;
        if out.ambiguous && (req.explain)() {
            self.gather(req.held, &Selection { selectors: &[], codes: req.codes }, &mut out.candidates);
        }
        let lifo = policy == Some(Policy::Lifo);
        let mut left = if lifo { req.need } else { self.take_plain(req.need, req, out) };
        while !left.is_zero() {
            let Some(at) = self.next(policy) else { break };
            let qty = self.holding.lots[at].qty.min(left);
            self.take(Source::Lot(at), qty, req, out);
            left -= qty;
        }
        if lifo { self.take_plain(left, req, out) } else { left }
    }

    /// Takes what plain value holds of `left`, and says what is still wanted.
    fn take_plain(&mut self, left: Qty, req: &Request, out: &mut Relief) -> Qty {
        let qty = self.holding.plain.min(left);
        if qty <= Qty::ZERO {
            return left;
        }
        self.take(Source::Plain, qty, req, out);
        left - qty
    }

    /// The lot the policy takes next: the dearest per unit, the newest live one, or the oldest past the cursor.
    fn next(&mut self, policy: Option<Policy>) -> Option<usize> {
        let lots = &self.holding.lots;
        match policy {
            Some(Policy::Hifo) => self.dearest(),
            Some(Policy::Lifo) => lots.iter().rposition(|lot| !lot.qty.is_zero()),
            _ => {
                self.first += lots[self.first..].iter().take_while(|lot| lot.qty.is_zero()).count();
                (self.first < lots.len()).then_some(self.first)
            }
        }
    }

    /// The lot of the highest basis per unit, the oldest of those: the heap's top, once the entries its lots outgrew are
    /// discarded. The heap is built when first asked, and every change to a lot pushes the lot as it is now.
    fn dearest(&mut self) -> Option<usize> {
        let lots = &self.holding.lots;
        let ranked = self.ranked.get_or_insert_with(|| {
            let live = lots.iter().enumerate().filter(|(_, lot)| !lot.qty.is_zero());
            Box::new(live.map(|(at, lot)| Ranked::of(at, lot)).collect())
        });
        let current = |top: &Ranked| {
            lots.get(top.at as usize).is_some_and(|lot| (lot.basis.0, lot.qty.0) == (top.basis, top.qty))
        };
        std::iter::from_fn(|| ranked.pop()).find(current).map(|top| top.at as usize)
    }

    /// Every other relief (a selector, a tie, `exact`, pro rata, HIFO of money): the candidates the selectors admit, in the
    /// order of the ranking, taken a colour at a time and each colour [`share`]d. Says what was not covered.
    fn take_ranked(&mut self, req: &Request, selection: &Selection, policy: Option<Policy>, out: &mut Relief) -> Qty {
        let mut ranked = std::mem::take(&mut out.gathered);
        ranked.clear();
        self.gather(req.held, selection, &mut ranked);
        ranked.retain(|c| req.allows(req.colour(c.tied)));
        if policy == Some(Policy::Exact) {
            whole_claims(&mut ranked, |c| req.colour(c.tied));
        }
        ranked.sort_unstable_by_key(|c| c.rank(policy, req));
        let mut plan = std::mem::take(&mut out.plan);
        plan.clear();
        let mut left = req.need;
        for group in ranked.chunk_by(|a, b| req.colour(a.tied) == req.colour(b.tied)) {
            if left.is_zero() {
                break;
            }
            let total: Qty = group.iter().map(|c| c.qty).sum();
            let take = left.min(total);
            if take < total && policy.is_none() && group.len() > 1 {
                out.ambiguous = true;
                if (req.explain)() {
                    out.candidates.extend_from_slice(group);
                }
            }
            share(group, take, policy, &mut plan);
            left -= take;
        }
        for &(source, qty) in &plan {
            self.take(source, qty, req, out);
        }
        (out.plan, out.gathered) = (plan, ranked);
        left
    }

    /// The parcels the selection admits, plain money first.
    fn gather(&self, held: Held, selection: &Selection, out: &mut Vec<Candidate>) {
        let plain = self.holding.plain;
        if plain > Qty::ZERO && !selection.constrains() {
            // Plain money has no transaction or acquisition day of its own.
            let (basis, acquired, txn, tied) = (held.plain_basis(plain), Day::MIN, None, None);
            out.push(Candidate { source: Source::Plain, qty: plain, basis, acquired, txn, tied, claim: plain });
        }
        let lots = self.holding.lots.iter().enumerate();
        let admitted = lots.filter(|(_, lot)| !lot.qty.is_zero() && selection.admits(lot));
        out.extend(admitted.map(|(at, lot)| Candidate::new(Source::Lot(at), lot)));
    }

    /// Whether a lot still held carries `code`: a code a flow writes names a claim here only if one does.
    pub fn carries(&self, code: Sym, pool: &Arena<Sym>) -> bool {
        self.holding.lots[self.first..].iter().any(|lot| !lot.qty.is_zero() && carries(lot, code, pool))
    }

    /// How much of the holding the selectors admit: what `all` means. Of a debt, how much of what is owed they reach.
    pub fn admitted(&self, selectors: &[Select], codes: &Arena<Sym>) -> Qty {
        let selection = Selection { selectors, codes };
        if !selection.constrains() {
            return match self.owes {
                true => self.holding.plain - self.qty,
                false => self.qty - self.holding.plain.min(Qty::ZERO),
            };
        }
        // A selector excludes plain value, which has no day or code to be selected by.
        let admitted: Qty = self.holding.lots.iter().filter(|lot| selection.admits(lot)).map(|lot| lot.qty).sum();
        if self.owes { -admitted } else { admitted }
    }

    /// Spreads `delta` of basis over the admitted parcels in proportion to
    /// their quantity. Quantities do not change. Plain money takes a share too,
    /// and stops being plain when it does. `false` if there is nothing to carry
    /// it: no admitted parcel.
    pub fn rebase(&mut self, delta: Qty, selection: &Selection, held: Held, now: (Day, RuntimeTxn)) -> bool {
        let plain = self.holding.plain;
        if plain > Qty::ZERO && !selection.constrains() {
            let basis = held.plain_basis(plain);
            self.holding.plain = Qty::ZERO;
            self.insert(Parcel::new(plain, basis, now));
        }
        let lots = &mut self.holding.lots[self.first..];
        let whole: Qty = lots.iter().filter(|lot| selection.admits(lot)).map(|lot| lot.qty).sum();
        if whole.is_zero() {
            return false;
        }
        let mut shares = Shares::new(delta, whole);
        for lot in lots.iter_mut().filter(|lot| !lot.qty.is_zero() && selection.admits(lot)) {
            lot.basis += shares.take(lot.qty);
        }
        self.ranked = None;
        true
    }

    /// Multiplies every quantity by `ratio`, keeping basis and acquisition day.
    pub fn scale(&mut self, ratio: Ratio) {
        let grow = |qty: Qty| qty.scale(ratio).unwrap_or(qty);
        self.holding.plain = grow(self.holding.plain);
        for lot in &mut self.holding.lots {
            lot.qty = grow(lot.qty);
        }
        self.dead = self.holding.lots.iter().filter(|lot| lot.qty.is_zero()).count();
        self.ties = self.holding.lots.iter().filter(|lot| !lot.qty.is_zero() && lot.tied.is_some()).count() as u32;
        self.qty = self.holding.plain + self.holding.lots.iter().map(|lot| lot.qty).sum();
        self.ranked = None;
    }

    /// The basis of everything held, in base-currency quanta: plain money is at
    /// face, and nothing else has basis outside its lots.
    pub fn basis(&self, held: Held) -> Qty {
        held.plain_basis(self.holding.plain) + self.holding.lots.iter().map(|lot| lot.basis).sum()
    }

    /// Moves the cursors past exhausted lots at either end.
    fn sweep_ends(&mut self) {
        let lots = &mut self.holding.lots;
        while lots.last().is_some_and(|lot| lot.qty.is_zero()) {
            lots.pop();
            self.dead -= 1;
        }
        self.first = self.first.min(lots.len());
        while self.first < lots.len() && lots[self.first].qty.is_zero() {
            self.first += 1;
        }
        if self.dead >= SWEEP_AT && self.dead * 2 > lots.len() {
            self.tidy();
        }
    }

    /// Removes every exhausted lot.
    fn tidy(&mut self) {
        self.holding.lots.retain(|lot| !lot.qty.is_zero());
        (self.first, self.dead, self.ranked) = (0, 0, None);
    }
}

/// Splits `total` across weights so the parts sum to `total` exactly: each
/// part is the difference of two rounded running shares, so rounding never
/// accumulates and the last part absorbs nothing.
pub(crate) struct Shares {
    total: Qty,
    whole: Qty,
    seen: Qty,
    paid: Qty,
}

impl Shares {
    /// Shares of `total` over weights that add up to `whole`.
    pub fn new(total: Qty, whole: Qty) -> Shares {
        Shares { total, whole, seen: Qty::ZERO, paid: Qty::ZERO }
    }

    /// The part owed to the next weight.
    pub fn take(&mut self, weight: Qty) -> Qty {
        self.seen += weight;
        let owed = match (self.seen == self.whole, self.whole.is_zero()) {
            (true, _) => self.total,
            (_, true) => Qty::ZERO,
            _ => self.total.share(self.seen, self.whole).expect("a share of a total never exceeds it"),
        };
        let part = owed - self.paid;
        self.paid = owed;
        part
    }
}

/// The selectors on a flow, read as a filter: parcels must satisfy one of the
/// ranges (if any) and one of the codes (if any).
pub(crate) struct Selection<'a> {
    pub selectors: &'a [Select],
    pub codes: &'a Arena<Sym>,
}

impl Selection<'_> {
    /// Whether plain money, which has no acquisition day or code, is excluded.
    pub fn constrains(&self) -> bool {
        self.selectors.iter().any(|s| !matches!(s, Select::Policy(_)))
    }

    fn policy(&self) -> Option<Policy> {
        self.selectors.iter().find_map(|s| if let Select::Policy(p) = *s { Some(p) } else { None })
    }

    pub fn admits(&self, lot: &Parcel) -> bool {
        let ranges = self.selectors.iter().filter_map(|s| if let Select::Range(days) = *s { Some(days) } else { None });
        let codes = self.selectors.iter().filter_map(|s| if let Select::Code(c) = *s { Some(c) } else { None });
        let made = self.selectors.iter().filter_map(|s| if let Select::Txn(txn) = *s { Some(txn) } else { None });
        let (mut ranges, mut codes, mut made) = (ranges.peekable(), codes.peekable(), made.peekable());
        let in_range = ranges.peek().is_none() || ranges.any(|days| days.contains(lot.acquired));
        let marked = codes.peek().is_none() || codes.any(|code| carries(lot, code, self.codes));
        let by = made.peek().is_none() || made.any(|txn| lot.txn.source_txn() == Some(txn));
        in_range && marked && by
    }
}

/// Whether the transaction that made `lot` carries `code`, in its header or on its own line.
fn carries(lot: &Parcel, code: Sym, pool: &Arena<Sym>) -> bool {
    pool[lot.codes.header].contains(&code) || pool[lot.codes.local].contains(&code)
}

impl Candidate {
    fn new(source: Source, parcel: &Parcel) -> Candidate {
        let (qty, basis, acquired, tied, txn) =
            (parcel.qty, parcel.basis, parcel.acquired, parcel.tied, Some(parcel.txn));
        Candidate { source, qty, basis, acquired, txn, tied, claim: qty }
    }
}

/// Gives each candidate what its whole claim holds: the candidates of one transaction and one colour, which are the
/// lines of an invoice, add up.
fn whole_claims(candidates: &mut [Candidate], colour: impl Fn(&Candidate) -> Colour) {
    let mut held: axiom_core::Map<(Option<RuntimeTxn>, Colour), Qty> = axiom_core::Map::default();
    for c in candidates.iter() {
        *held.entry((c.txn, colour(c))).or_default() += c.qty;
    }
    for c in candidates.iter_mut() {
        c.claim = held[&(c.txn, colour(c))];
    }
}

/// Plans what leaves a group of one colour, `take` of it in all, in the order of the ranking: all of each before the next,
/// or, pro rata, a share of each by its quantity, the shares adding up to `take` exactly.
fn share(group: &[Candidate], take: Qty, policy: Option<Policy>, plan: &mut Vec<(Source, Qty)>) {
    if policy == Some(Policy::Prorata) {
        let mut shares = Shares::new(take, group.iter().map(|c| c.qty).sum());
        return plan.extend(group.iter().map(|c| (c.source, shares.take(c.qty))));
    }
    let mut left = take;
    for c in group {
        let qty = c.qty.min(left);
        plan.push((c.source, qty));
        left -= qty;
        if left.is_zero() {
            break;
        }
    }
}

/// What is held, by place then commodity: the parcels, not the bookkeeping
/// around them (empty slots, cursors, ranks).
impl Hash for Holdings {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let held = self.iter().map(|slot| &slot.holding).filter(|holding| !holding.is_empty());
        held.for_each(|holding| holding.hash(state));
    }
}

/// Every holding, by place then commodity.
#[derive(Clone)]
pub(crate) struct Holdings {
    /// The first slot of each place's chain.
    heads: Vec<u32>,
    slots: Vec<Slot>,
    /// A lot was exhausted since the last sweep.
    untidy: bool,
    /// The slots handed out to change, or scaled, since the fold last wrote the balances that moved into the histories.
    /// A slot is only ever changed through `entry` and `scale`, so a slot that is not here has the balance it had.
    touched: Vec<u32>,
}

/// A wash-sale allocation to a bounded quantity of one actual acquisition.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CarryLotAddition {
    pub part: PartId,
    pub acquired: Day,
    pub held_since: Day,
    pub quantity: Qty,
    pub amount: Qty,
}

impl Holdings {
    pub fn new(places: usize) -> Holdings {
        Holdings { heads: vec![NONE; places], slots: Vec::new(), untidy: false, touched: Vec::new() }
    }

    fn chain(&self, head: u32) -> impl Iterator<Item = &Slot> {
        let next = |slot: &&Slot| Some(slot.next).filter(|&n| n != NONE).map(|n| &self.slots[n as usize]);
        successors(Some(head).filter(|&h| h != NONE).map(|h| &self.slots[h as usize]), next)
    }

    pub fn get(&self, place: Id<Place>, unit: Id<Commodity>) -> Option<&Slot> {
        self.of(place).find(|slot| slot.unit >= unit).filter(|slot| slot.unit == unit)
    }

    /// The slot, created empty if the place has never held `unit`.
    pub fn entry(&mut self, place: Id<Place>, unit: Id<Commodity>) -> &mut Slot {
        let (mut before, mut at) = (NONE, self.heads[place.index()]);
        while at != NONE && self.slots[at as usize].unit < unit {
            (before, at) = (at, self.slots[at as usize].next);
        }
        if at == NONE || self.slots[at as usize].unit != unit {
            let new = self.slots.len() as u32;
            self.slots.push(Slot::new(place, unit, at));
            match before {
                NONE => self.heads[place.index()] = new,
                _ => self.slots[before as usize].next = new,
            }
            at = new;
        }
        self.touched.push(at);
        &mut self.slots[at as usize]
    }

    /// The slots whose balance has moved since this was last asked, each with the balance it holds now: what the fold
    /// writes into the histories. A slot that was changed and changed back says nothing.
    pub fn moved(&mut self) -> impl Iterator<Item = (u32, Qty)> + '_ {
        let Holdings { touched, slots, .. } = self;
        touched.drain(..).filter_map(|at| {
            let slot = &mut slots[at as usize];
            (std::mem::replace(&mut slot.recorded, slot.qty) != slot.qty).then_some((at, slot.qty))
        })
    }

    /// Which place and commodity each slot holds, in the order the slots were made.
    pub fn positions(&self) -> Vec<crate::histories::Position> {
        let at = |slot: &Slot| crate::histories::Position { place: slot.place, unit: slot.unit };
        self.slots.iter().map(at).collect()
    }

    /// What `place` alone holds of `unit`, in quanta.
    pub fn qty(&self, place: Id<Place>, unit: Id<Commodity>) -> Qty {
        self.get(place, unit).map_or(Qty::ZERO, |slot| slot.qty)
    }

    /// Adds (or, negative, removes) plain value.
    pub fn credit(&mut self, place: Id<Place>, unit: Id<Commodity>, qty: Qty) {
        self.entry(place, unit).credit(qty);
    }

    /// A place's own slots, by commodity.
    pub fn of(&self, place: Id<Place>) -> impl Iterator<Item = &Slot> {
        self.chain(self.heads[place.index()])
    }

    /// Every slot by place, then commodity, empty ones included.
    pub fn iter(&self) -> impl Iterator<Item = &Slot> {
        self.within(0..self.heads.len())
    }

    /// Changes the basis of the live parcels of `part` by `delta` in all, wherever they are (a part's parcels are all of its
    /// commodity, `unit`): shared by their basis when it falls (depreciation) and by their quantity when it rises (an
    /// improvement), so that the shares add up to it exactly. Checks before it writes: some parcel holds the part, it does
    /// not lose more basis than they hold, and no basis overflows.
    pub fn adjust(&mut self, unit: Id<Commodity>, part: PartId, delta: Qty) -> Result<(), AssetError> {
        if delta.is_zero() {
            return Ok(());
        }
        let falls = delta.is_negative();
        let magnitude = delta.0.checked_abs().map(Qty).ok_or(AssetError::Overflow)?;
        let of_part = |lot: &&Parcel| lot.part == Some(part) && lot.qty > Qty::ZERO;
        let weight = |lot: &Parcel| if falls { lot.basis } else { lot.qty };
        let held = self.slots.iter().filter(|slot| slot.unit == unit).flat_map(|slot| slot.holding.lots.iter());
        let sums = held.filter(of_part).try_fold((0, Qty::ZERO, Qty::ZERO), |(found, basis, whole), lot| {
            Some((found + 1, basis.0.checked_add(lot.basis.0).map(Qty)?, whole.0.checked_add(weight(lot).0).map(Qty)?))
        });
        match sums.ok_or(AssetError::Overflow)? {
            (0, ..) => Err(AssetError::UnknownPart),
            (_, basis, _) if falls && magnitude > basis => Err(AssetError::ParcelBasisMismatch),
            (_, basis, _) if basis.0.checked_add(magnitude.0).is_none() => Err(AssetError::Overflow),
            (_, _, whole) => {
                let mut shares = Shares::new(magnitude, whole);
                for slot in self.slots.iter_mut().filter(|slot| slot.unit == unit) {
                    for lot in slot.holding.lots.iter_mut().filter(|lot| lot.part == Some(part) && lot.qty > Qty::ZERO)
                    {
                        let share = shares.take(weight(lot));
                        lot.basis = if falls { lot.basis - share } else { lot.basis + share };
                        slot.ranked = None;
                    }
                }
                Ok(())
            }
        }
    }

    /// A wash sale's loss, carried into the shares that replaced the sold ones: each addition gives `quantity` of the live
    /// parcels of its part bought on its day that no loss was carried into yet, wherever they are, shared by quantity,
    /// `amount` more basis (shared by quantity too) and the sold shares' holding period, and marks them. A parcel matched in
    /// part is split, the matched shares landing after the parcels of their day. Every addition is checked against what
    /// the ones before it took before anything is written.
    pub fn carry(&mut self, unit: Id<Commodity>, additions: &[CarryLotAddition]) -> Result<(), AssetError> {
        // What each matched parcel gives: (slot, lot, addition, the matched shares, the basis they leave with).
        let mut pieces: Vec<(usize, usize, usize, Parcel, Qty)> = Vec::new();
        for (order, add) in additions.iter().enumerate() {
            if add.quantity <= Qty::ZERO || add.amount.is_negative() {
                return Err(AssetError::NegativeAmount);
            }
            let slots = self.slots.iter().enumerate().filter(|(_, slot)| slot.unit == unit);
            let lots =
                slots.flat_map(|(s, slot)| slot.holding.lots.iter().enumerate().map(move |(at, lot)| (s, at, *lot)));
            let bought = |lot: &Parcel| lot.part == Some(add.part) && lot.acquired == add.acquired && !lot.wash_matched;
            let left = |(s, at, lot): (usize, usize, Parcel)| {
                let taken = pieces.iter().filter(|piece| (piece.0, piece.1) == (s, at));
                let (qty, basis) =
                    taken.fold((lot.qty, lot.basis), |(qty, basis), piece| (qty - piece.3.qty, basis - piece.4));
                (s, at, Parcel { qty, basis, ..lot })
            };
            let matched: Vec<_> = lots.filter(|(_, _, lot)| bought(lot) && lot.qty > Qty::ZERO).map(left).collect();
            let whole: Qty = matched.iter().map(|(_, _, lot)| lot.qty).sum();
            if whole < add.quantity {
                return Err(AssetError::ParcelBasisMismatch);
            }
            let (mut quantities, mut amounts) =
                (Shares::new(add.quantity, whole), Shares::new(add.amount, add.quantity));
            for (s, at, lot) in matched {
                let qty = quantities.take(lot.qty);
                if qty.is_zero() {
                    continue;
                }
                let basis = if qty == lot.qty {
                    lot.basis
                } else {
                    lot.basis.share(qty, lot.qty).ok_or(AssetError::Overflow)?
                };
                let added = basis.0.checked_add(amounts.take(qty).0).map(Qty).ok_or(AssetError::Overflow)?;
                let held_since = add.held_since.min(lot.held_since);
                pieces.push((s, at, order, Parcel { qty, basis: added, held_since, wash_matched: true, ..lot }, basis));
            }
        }
        // Splitting from the last lot back keeps the positions of the ones still to split.
        pieces.sort_by_key(|&(s, at, order, ..)| (Reverse(s), Reverse(at), order));
        for (s, at, _, matched, basis) in pieces {
            self.slots[s].split(at, matched, basis);
        }
        Ok(())
    }

    /// The slots of the places whose ids lie in `places`: a subtree.
    pub fn within(&self, places: Range<usize>) -> impl Iterator<Item = &Slot> {
        self.heads[places].iter().flat_map(|&head| self.chain(head))
    }

    /// Relieves `req.need` of `unit` from `place`.
    pub fn relieve(&mut self, place: Id<Place>, unit: Id<Commodity>, req: &Request, out: &mut Relief) {
        let slot = self.entry(place, unit);
        slot.relieve(req, out);
        self.untidy |= slot.dead > 0;
    }

    /// A split: every holding of `unit`, in every place, is multiplied by `ratio`.
    pub fn scale(&mut self, unit: Id<Commodity>, ratio: Ratio) {
        for (at, slot) in self.slots.iter_mut().enumerate().filter(|(_, slot)| slot.unit == unit) {
            slot.scale(ratio);
            self.touched.push(at as u32);
        }
        self.untidy = true;
    }

    /// Sweeps out every exhausted lot: the state callers may look at.
    pub fn tidy(&mut self) {
        if std::mem::take(&mut self.untidy) {
            self.slots.iter_mut().filter(|slot| slot.dead > 0).for_each(Slot::tidy);
        }
    }

    /// The non-empty holdings, by place then commodity.
    pub fn into_sorted(mut self) -> Vec<Holding> {
        self.untidy = true;
        self.tidy();
        let mut all: Vec<Holding> = self.slots.into_iter().map(|slot| slot.holding).filter(|h| !h.is_empty()).collect();
        all.sort_unstable_by_key(|h| (h.place, h.unit));
        all
    }
}

#[cfg(test)]
mod tests {
    mod relief_model;

    use axiom_core::{Day, Days};

    use super::*;

    fn span(first: i32, last: i32) -> Days {
        Days::new(Day(first), Day(last)).unwrap()
    }

    fn journal(txn: u32) -> RuntimeTxn {
        RuntimeTxn::journal(Id::new(txn)).unwrap()
    }

    fn lot(qty: i64, basis: i64, acquired: i32) -> Parcel {
        Parcel::new(Qty(qty), Qty(basis), (Day(acquired), journal(acquired as u32)))
    }

    fn slot_of(unit: u32, plain: i64, lots: &[Parcel], held: Held) -> Slot {
        let mut slot = Slot::new(Id::new(0), Id::new(unit), NONE);
        slot.credit(Qty(plain));
        lots.iter().for_each(|&lot| slot.land(lot, held, &Arena::new()));
        slot
    }

    struct Ask<'a> {
        held: Held,
        policy: Option<Policy>,
        selectors: &'a [Select],
        permits: &'a [(Id<Entity>, bool)],
        spender: Option<Id<Entity>>,
    }

    const PLAIN: Ask = Ask { held: Held::Lots, policy: None, selectors: &[], permits: &[], spender: None };

    fn relieve(slot: &mut Slot, need: i64, ask: &Ask) -> Relief {
        let mut relief = Relief::default();
        let codes = Arena::new();
        let (held, policy, selectors, permits) = (ask.held, ask.policy, ask.selectors, ask.permits);
        let (spender, now) = (ask.spender, (Day(1_000), journal(0)));
        let request = Request {
            held,
            selectors,
            permits,
            spender,
            explain: &|| true,
            ..Request::of(Qty(need), policy, &codes, now)
        };
        slot.relieve(&request, &mut relief);
        relief
    }

    fn taken(relief: &Relief) -> Vec<(i64, i64)> {
        relief.slices.iter().map(|s| (s.qty.0, s.basis.0)).collect()
    }

    fn lots(slot: &Slot) -> Vec<(i64, i64)> {
        slot.holding.lots.iter().filter(|lot| !lot.qty.is_zero()).map(|l| (l.qty.0, l.basis.0)).collect()
    }

    #[test]
    fn a_lot_that_arrives_empty_is_swept_like_any_exhausted_one() {
        let mut slot = slot_of(3, 0, &[lot(10, 10, 5), lot(0, 0, 7)], Held::Lots);
        let relief = relieve(&mut slot, 10, &Ask { policy: Some(Policy::Lifo), ..PLAIN });
        assert_eq!(taken(&relief), [(10, 10)]);
        assert!(slot.holding.lots.is_empty() && slot.dead == 0, "both ends swept, and the count agrees");
    }

    #[test]
    fn lots_merge_by_identity_and_stay_oldest_first() {
        let mut held = Holdings::new(2);
        let (place, unit) = (Id::new(1), Id::new(3));
        let slot = held.entry(place, unit);
        slot.land(lot(5, 50, 20), Held::Lots, &Arena::new());
        slot.land(lot(2, 10, 10), Held::Lots, &Arena::new());
        slot.land(lot(3, 30, 20), Held::Lots, &Arena::new());
        slot.land(Parcel { txn: journal(9), ..lot(1, 10, 20) }, Held::Lots, &Arena::new());
        let lots: Vec<_> = slot.holding.lots.iter().map(|l| (l.acquired.0, l.qty.0, l.basis.0)).collect();
        assert_eq!(lots, [(10, 2, 10), (20, 8, 80), (20, 1, 10)]);
        assert_eq!(held.qty(place, unit), Qty(11));
        assert_eq!(held.qty(Id::new(0), unit), Qty::ZERO);
    }

    #[test]
    fn base_lots_merge_when_tie_and_basis_per_unit_agree() {
        let mut slot = Slot::new(Id::new(0), Id::new(0), NONE);
        let entity = Id::new(9);
        for (qty, basis, acquired, tied) in [
            (100, 0, 5, None),
            (250, 0, 900, None),
            (10, 5, 6, None),
            (20, 10, 7, None),
            (30, 30, 8, Some(entity)),
            (5, 5, 9, Some(entity)),
        ] {
            slot.land(Parcel { tied, ..lot(qty, basis, acquired) }, Held::Money, &Arena::new());
        }
        let lots: Vec<_> =
            slot.holding.lots.iter().map(|l| (l.qty.0, l.basis.0, l.acquired.0, l.tied.is_some())).collect();
        // Zero-basis money is one lot, half-basis money another, tied money a third; each keeps its first day.
        assert_eq!(lots, [(350, 0, 5, false), (30, 15, 6, false), (35, 35, 8, true)]);
    }

    #[test]
    fn plain_money_never_allocates_and_iteration_is_ordered() {
        let mut held = Holdings::new(3);
        held.entry(Id::new(2), Id::new(1)).credit(Qty(7));
        held.entry(Id::new(0), Id::new(5)).credit(Qty(1));
        held.entry(Id::new(0), Id::new(2)).credit(Qty(1));
        held.entry(Id::new(0), Id::new(2)).land(lot(4, 4, 0), Held::Money, &Arena::new());
        let order: Vec<_> =
            held.iter().map(|s| (s.place.index(), s.unit.index(), s.plain.0, s.lots.capacity())).collect();
        assert_eq!(order, [(0, 2, 5, 0), (0, 5, 1, 0), (2, 1, 7, 0)]);
    }

    #[test]
    fn prorata_parts_sum_exactly() {
        let mut held = slot_of(1, 0, &[lot(7, 100, 1), lot(11, 250, 2), lot(13, 333, 3)], Held::Lots);
        let policy = Some(Policy::Prorata);
        let ten = relieve(&mut held.clone(), 10, &Ask { policy, ..PLAIN });
        assert_eq!(ten.slices.iter().map(|p| p.qty.0).sum::<i64>(), 10);
        // Relieving everything relieves every lot's basis exactly.
        let all = relieve(&mut held, 31, &Ask { policy, ..PLAIN });
        assert_eq!(all.slices.iter().map(|p| p.basis.0).sum::<i64>(), 683);
        assert_eq!((all.shortfall, held.qty), (Qty::ZERO, Qty::ZERO));
    }

    #[test]
    fn apportioned_shares_carry_no_rounding_drift() {
        let mut shares = Shares::new(Qty(100), Qty(3));
        let parts: Vec<_> = (0..3).map(|_| shares.take(Qty(1)).0).collect();
        assert_eq!(parts, [33, 34, 33]);
        assert_eq!(parts.iter().sum::<i64>(), 100);
    }

    #[test]
    fn every_policy_takes_from_its_own_end() {
        let base = slot_of(1, 0, &[lot(10, 1_000, 1), lot(10, 3_000, 2), lot(10, 2_000, 3)], Held::Lots);
        for (policy, want) in [
            (Policy::Hifo, vec![(10, 3_000), (5, 1_000)]),
            (Policy::Fifo, vec![(10, 1_000), (5, 1_500)]),
            (Policy::Lifo, vec![(10, 2_000), (5, 1_500)]),
        ] {
            let mut held = base.clone();
            let relief = relieve(&mut held, 15, &Ask { policy: Some(policy), ..PLAIN });
            assert_eq!(taken(&relief), want, "{policy:?}");
            assert_eq!(held.qty, Qty(15));
        }
    }

    #[test]
    fn exact_takes_the_lot_of_exactly_the_size_asked_and_otherwise_the_oldest() {
        let base = slot_of(1, 0, &[lot(300, 300, 1), lot(200, 200, 2), lot(300, 300, 3)], Held::Lots);
        let exact = Ask { policy: Some(Policy::Exact), ..PLAIN };
        let mut held = base.clone();
        let relief = relieve(&mut held, 200, &exact);
        assert_eq!((taken(&relief), relief.ambiguous), (vec![(200, 200)], false), "not the oldest, which is 300");
        assert_eq!(lots(&held), [(300, 300), (300, 300)]);
        let mut held = base.clone();
        assert_eq!(taken(&relieve(&mut held, 300, &exact)), [(300, 300)]);
        assert_eq!(lots(&held), [(200, 200), (300, 300)], "of two lots of 300, the older goes");
        let mut held = base.clone();
        assert_eq!(
            taken(&relieve(&mut held, 100, &exact)),
            [(100, 100)],
            "nothing is exactly 100: the oldest, in part"
        );
        assert_eq!(lots(&held), [(200, 200), (200, 200), (300, 300)]);
    }

    #[test]
    fn exact_takes_a_claim_whose_lines_add_up_to_the_size_asked() {
        // An invoice of two lines, 150 and 250, made by one transaction beside two claims of 300.
        let line = |qty, ordinal| Parcel { part: Some(PartId { origin: journal(2), ordinal }), ..lot(qty, qty, 2) };
        let base = slot_of(1, 0, &[lot(300, 300, 1), line(150, 0), line(250, 1), lot(300, 300, 3)], Held::Lots);
        assert_eq!(base.holding.lots.len(), 4, "the lines stay lots of their own");
        let exact = Ask { policy: Some(Policy::Exact), ..PLAIN };
        let mut held = base.clone();
        assert_eq!(
            taken(&relieve(&mut held, 400, &exact)),
            [(150, 150), (250, 250)],
            "the whole invoice, not 300 and 100"
        );
        assert_eq!(lots(&held), [(300, 300), (300, 300)]);
        let admit_all = [Select::Range(Days::ALWAYS)];
        let scanning = Ask { policy: Some(Policy::Exact), selectors: &admit_all, ..PLAIN };
        let mut held = base.clone();
        assert_eq!(taken(&relieve(&mut held, 400, &scanning)), [(150, 150), (250, 250)]);
        let mut held = base;
        assert_eq!(
            taken(&relieve(&mut held, 150, &exact)),
            [(150, 150)],
            "a line is no claim of its own: 150 is the oldest"
        );
    }

    #[test]
    fn exact_among_the_lots_a_selector_admits_and_among_the_colours_of_a_tie() {
        let entity = Id::new(3);
        let tied = |qty, acquired| Parcel { tied: Some(entity), ..lot(qty, qty, acquired) };
        let mut held = slot_of(1, 0, &[lot(200, 200, 1), tied(200, 2), lot(300, 300, 3)], Held::Lots);
        let exact = Ask { policy: Some(Policy::Exact), spender: Some(entity), ..PLAIN };
        assert_eq!(taken(&relieve(&mut held, 200, &exact)), [(200, 200)]);
        assert_eq!(lots(&held), [(200, 200), (300, 300)], "the spender's own lot of 200 goes before the untied one");
        let mut held = slot_of(1, 0, &[lot(300, 300, 1), lot(200, 200, 2)], Held::Lots);
        let admit_all = [Select::Range(Days::ALWAYS)];
        let scanning = Ask { policy: Some(Policy::Exact), selectors: &admit_all, ..PLAIN };
        assert_eq!(taken(&relieve(&mut held, 200, &scanning)), [(200, 200)]);
    }

    #[test]
    fn hifo_follows_a_lot_whose_basis_changed_and_lots_that_arrive() {
        let mut held = slot_of(1, 0, &[lot(10, 1_000, 1), lot(10, 2_000, 2)], Held::Lots);
        let hifo = Ask { policy: Some(Policy::Hifo), ..PLAIN };
        assert_eq!(taken(&relieve(&mut held, 4, &hifo)), [(4, 800)]);
        held.land(lot(5, 5_000, 3), Held::Lots, &Arena::new());
        assert_eq!(taken(&relieve(&mut held, 6, &hifo)), [(5, 5_000), (1, 200)]);
        assert!(held.rebase(
            Qty(9_000),
            &Selection { selectors: &[], codes: &Arena::new() },
            Held::Lots,
            (Day(9), journal(0))
        ));
        assert_eq!(
            taken(&relieve(&mut held, 1, &hifo)),
            [(1, 800)],
            "the lot that gained basis is now dearer per unit"
        );
    }

    #[test]
    fn exhausted_lots_leave_the_front_and_the_back_without_a_sweep() {
        let mut held = slot_of(1, 0, &[lot(1, 10, 1), lot(1, 10, 2), lot(1, 10, 3), lot(1, 10, 4)], Held::Lots);
        let (fifo, lifo) = (Ask { policy: Some(Policy::Fifo), ..PLAIN }, Ask { policy: Some(Policy::Lifo), ..PLAIN });
        relieve(&mut held, 2, &fifo);
        relieve(&mut held, 1, &lifo);
        assert_eq!(lots(&held), [(1, 10)]);
        assert_eq!(held.holding.lots.len(), 3, "the back was trimmed; the front is a cursor, not a removal");
        held.land(lot(2, 20, 0), Held::Lots, &Arena::new());
        assert_eq!(
            taken(&relieve(&mut held, 3, &fifo)),
            [(2, 20), (1, 10)],
            "an older lot arrives ahead of the cursor"
        );
    }

    #[test]
    fn only_lots_that_differ_are_ambiguous_without_a_policy() {
        let mut same = slot_of(1, 0, &[lot(10, 1_000, 5)], Held::Lots);
        assert!(!relieve(&mut same, 6, &PLAIN).ambiguous);
        let purchase = |txn| Parcel { txn: journal(txn), ..lot(10, 1_500, 5) };
        let differ = slot_of(1, 0, &[lot(10, 1_000, 5), purchase(99)], Held::Lots);
        let relief = relieve(&mut differ.clone(), 6, &PLAIN);
        assert!(relief.ambiguous);
        assert_eq!(relief.candidates.len(), 2);
        assert_eq!(taken(&relief), [(6, 600)], "FIFO carries on");
        assert!(!relieve(&mut differ.clone(), 6, &Ask { policy: Some(Policy::Fifo), ..PLAIN }).ambiguous);
        assert!(!relieve(&mut differ.clone(), 20, &PLAIN).ambiguous, "taking everything leaves no choice");
        let days = slot_of(1, 0, &[lot(10, 1_000, 5), lot(10, 1_000, 6)], Held::Lots);
        assert!(relieve(&mut days.clone(), 6, &PLAIN).ambiguous, "each purchase is its own lot outside the base");
        // The same when every candidate is looked at: a selector, or a tie.
        let (all, only_first) = ([Select::Range(Days::ALWAYS)], [Select::Range(span(5, 5))]);
        assert!(relieve(&mut days.clone(), 6, &Ask { selectors: &all, ..PLAIN }).ambiguous);
        assert!(!relieve(&mut days.clone(), 6, &Ask { selectors: &only_first, ..PLAIN }).ambiguous, "one admitted");
        assert!(!relieve(&mut days.clone(), 20, &Ask { selectors: &all, ..PLAIN }).ambiguous, "all of them");
        let tied = slot_of(1, 0, &[lot(10, 1_000, 5), Parcel { tied: Some(Id::new(2)), ..purchase(99) }], Held::Lots);
        let permits = [(Id::new(2), true)];
        let relief = relieve(&mut tied.clone(), 6, &Ask { permits: &permits, ..PLAIN });
        assert!(!relief.ambiguous, "each colour holds one lot, and the permitted one goes first");
        assert_eq!(taken(&relief), [(6, 900)]);
        let untied_too = slot_of(
            1,
            0,
            &[lot(10, 1_000, 5), purchase(99), Parcel { tied: Some(Id::new(2)), ..lot(1, 1, 7) }],
            Held::Lots,
        );
        assert!(relieve(&mut untied_too.clone(), 6, &PLAIN).ambiguous, "two untied lots differ, beside a tied one");
    }

    #[test]
    fn plain_money_and_a_zero_basis_lot_differ_but_deferrals_do_not() {
        let money = Ask { held: Held::Money, ..PLAIN };
        let mixed = slot_of(1, 700, &[lot(300, 0, 1)], Held::Money);
        assert!(relieve(&mut mixed.clone(), 100, &money).ambiguous, "after-tax and pre-tax money differ");
        let deferrals = slot_of(1, 0, &[lot(100, 0, 1), lot(50, 0, 30)], Held::Money);
        assert!(!relieve(&mut deferrals.clone(), 20, &money).ambiguous, "zero-basis deferrals from any day are alike");
    }

    #[test]
    fn prorata_over_plain_and_a_zero_basis_lot_splits_the_withdrawal() {
        let mut held = slot_of(1, 6_300_00, &[lot(2_200_00, 0, 3)], Held::Money);
        let relief = relieve(&mut held, 1_500_00, &Ask { held: Held::Money, policy: Some(Policy::Prorata), ..PLAIN });
        assert_eq!(taken(&relief), [(1_111_76, 1_111_76), (388_24, 0)]);
    }

    #[test]
    fn a_sale_beyond_what_is_held_reports_the_shortfall_and_goes_negative() {
        let mut held = slot_of(1, 0, &[lot(7, 700, 1)], Held::Lots);
        let relief = relieve(&mut held, 10, &PLAIN);
        assert_eq!((relief.shortfall, taken(&relief)), (Qty(3), vec![(7, 700)]));
        assert_eq!((held.qty, held.holding.plain), (Qty(-3), Qty(-3)));
    }

    #[test]
    fn tied_parcels_go_first_only_when_their_laws_permit() {
        let grant = Id::new(4);
        let tied = Parcel { tied: Some(grant), ..lot(5, 5, 9) };
        let held = slot_of(1, 0, &[lot(10, 10, 1), tied], Held::Money);
        let first = |permit: bool| {
            let permits = [(grant, permit)];
            relieve(&mut held.clone(), 4, &Ask { held: Held::Money, permits: &permits, ..PLAIN }).slices[0].tied
        };
        assert_eq!(first(true), Some(grant));
        assert_eq!(first(false), None);
    }

    #[test]
    fn a_spender_takes_its_own_parcels_then_untied_ones_and_nobody_elses() {
        let (car, trip) = (Id::new(4), Id::new(5));
        let tied = |entity, acquired| Parcel { tied: Some(entity), ..lot(5, 5, acquired) };
        let held = slot_of(1, 0, &[tied(trip, 1), tied(car, 2), lot(10, 10, 3)], Held::Money);
        let order = |spender, permits: &[(Id<Entity>, bool)], need| {
            let ask = Ask { held: Held::Money, policy: Some(Policy::Fifo), spender, permits, ..PLAIN };
            let relief = relieve(&mut held.clone(), need, &ask);
            (relief.slices.iter().map(|s| s.tied).collect::<Vec<_>>(), relief.shortfall.0)
        };
        assert_eq!(order(Some(car), &[], 8), (vec![Some(car), None], 0), "then what is tied to nobody");
        assert_eq!(
            order(Some(car), &[], 20),
            (vec![Some(car), None], 5),
            "and what trip-fund holds is not its to spend"
        );
        let permitted = [(trip, true), (car, true)];
        assert_eq!(order(None, &permitted, 12), (vec![Some(trip), Some(car), None], 0), "no spender: the laws decide");
        assert_eq!(
            order(None, &[], 20),
            (vec![None, Some(trip), Some(car)], 0),
            "and refused money is the last resort"
        );
    }

    #[test]
    fn selectors_intersect_by_kind_and_union_within_one() {
        let held = slot_of(1, 0, &[lot(1, 1, 10), lot(2, 2, 20), lot(4, 4, 30)], Held::Lots);
        let codes = Arena::new();
        let pick = |selectors: &[Select]| held.admitted(selectors, &codes).0;
        assert_eq!(pick(&[]), 7);
        assert_eq!(pick(&[Select::Range(span(10, 20))]), 3);
        assert_eq!(pick(&[Select::Range(span(10, 10)), Select::Range(span(30, 30))]), 5);
        assert_eq!(pick(&[Select::Policy(Policy::Lifo)]), 7);
    }

    #[test]
    fn plain_value_is_one_candidate_without_a_fabricated_transaction_key() {
        let mut slot = slot_of(1, 0, &[], Held::Lots);
        slot.credit(Qty(5));
        let candidates = {
            let mut out = Vec::new();
            slot.gather(Held::Lots, &Selection { selectors: &[], codes: &Arena::new() }, &mut out);
            out
        };
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].txn, None);
        let relief = relieve(&mut slot.clone(), 2, &PLAIN);
        assert!(!relief.ambiguous, "part of plain value alone is no choice");
        let tied = Ask { permits: &[(Id::new(4), true)], ..PLAIN };
        assert!(!relieve(&mut slot, 2, &Ask { selectors: &[Select::Policy(Policy::Prorata)], ..tied }).ambiguous);
    }

    #[test]
    fn code_selectors_use_both_pooled_ranges_for_synthetic_txns() {
        let mut names = axiom_core::Interner::default();
        let (header, local, other) = (names.intern("statement"), names.intern("purchase"), names.intern("other"));
        let mut pool = Arena::new();
        let (header_at, local_at, other_at) = (pool.push(header), pool.push(local), pool.push(other));
        let marks = FlowCodes { header: axiom_core::Run::new(header_at, 1), local: axiom_core::Run::new(local_at, 1) };
        let mut parcel = lot(5, 5, 10);
        parcel.txn = RuntimeTxn::ContractOccurrence {
            contract: Id::new(0),
            schedule: axiom_model::ScheduleKind::Regular,
            day: Day(10),
            ordinal: 999,
            source: None,
        };
        parcel.codes = marks;
        let mut held = Slot::new(Id::new(0), Id::new(0), NONE);
        held.land(parcel, Held::Lots, &pool);

        assert_eq!(held.admitted(&[Select::Code(header)], &pool), Qty(5));
        assert_eq!(held.admitted(&[Select::Code(local)], &pool), Qty(5));
        assert_eq!(held.admitted(&[Select::Code(other)], &pool), Qty::ZERO);
    }

    #[test]
    fn moving_a_lot_through_a_slice_preserves_its_pooled_code_identity() {
        let mut names = axiom_core::Interner::default();
        let original = names.intern("original-purchase");
        let mut pool = Arena::new();
        let first = pool.push(original);
        let codes = FlowCodes { header: axiom_core::Run::new(first, 1), local: FlowCodes::default().local };
        let mut parcel = lot(7, 700, 10);
        parcel.codes = codes;
        let mut source = Slot::new(Id::new(0), Id::new(0), NONE);
        source.land(parcel, Held::Lots, &pool);

        let mut relief = Relief::default();
        let request = Request::of(Qty(3), Some(Policy::Fifo), &pool, (Day(20), journal(20)));
        source.relieve(&request, &mut relief);
        let moved = relief.slices[0].lot;
        let mut target = Slot::new(Id::new(1), Id::new(0), NONE);
        target.land(moved, Held::Lots, &pool);

        assert_eq!(target.admitted(&[Select::Code(original)], &pool), Qty(3));
    }

    #[test]
    fn parcel_merging_compares_code_meaning_not_range_positions() {
        let mut names = axiom_core::Interner::default();
        let mark = names.intern("same");
        let different = names.intern("different");
        let mut pool = Arena::new();
        let first = pool.push(mark);
        let second = pool.push(mark);
        let other = pool.push(different);
        let one = FlowCodes { header: axiom_core::Run::new(first, 1), local: FlowCodes::default().local };
        let equal = FlowCodes { header: axiom_core::Run::new(second, 1), local: FlowCodes::default().local };
        let distinct = FlowCodes { header: axiom_core::Run::new(other, 1), local: FlowCodes::default().local };
        let mut slot = Slot::new(Id::new(0), Id::new(0), NONE);
        let mut first_parcel = lot(2, 2, 10);
        first_parcel.codes = one;
        let mut equivalent_parcel = lot(3, 3, 10);
        equivalent_parcel.codes = equal;
        let mut distinct_parcel = lot(1, 1, 10);
        distinct_parcel.codes = distinct;
        slot.land(first_parcel, Held::Lots, &pool);
        slot.land(equivalent_parcel, Held::Lots, &pool);
        slot.land(distinct_parcel, Held::Lots, &pool);

        assert_eq!(slot.holding.lots.len(), 2);
        assert_eq!(slot.holding.lots[0].qty, Qty(5));
        assert_eq!(slot.holding.lots[1].qty, Qty(1));
    }

    #[test]
    fn asset_part_identity_survives_partial_relief_and_is_not_merged() {
        let origin = RuntimeTxn::Adjustment { place: Id::new(1), day: Day(10) };
        let (first, second) = (PartId { origin, ordinal: 0 }, PartId { origin, ordinal: 1 });
        let mut a = lot(4, 40, 10);
        a.part = Some(first);
        let mut b = a;
        b.part = Some(second);
        let mut slot = slot_of(1, 0, &[a, b], Held::Lots);

        assert_eq!(slot.holding.lots.len(), 2, "distinct cost-basis parts remain addressable");
        let mut relief = Relief::default();
        let codes = Arena::new();
        let request = Request::of(Qty(2), Some(Policy::Fifo), &codes, (Day(20), journal(20)));
        slot.relieve(&request, &mut relief);
        assert_eq!(relief.slices[0].part, Some(first));

        let slice = relief.slices[0];
        let moved = Parcel { basis: slice.carried, ..slice.lot };
        assert_eq!(moved.part, Some(first), "ordinary transfer carries the part key");
    }

    /// What the live parcels of `part` hold of basis, wherever they are.
    fn basis_of(holdings: &Holdings, part: PartId) -> i64 {
        let lots = holdings.iter().flat_map(|slot| slot.holding.lots.iter());
        lots.filter(|lot| lot.part == Some(part) && lot.qty > Qty::ZERO).map(|lot| lot.basis.0).sum()
    }

    #[test]
    fn part_basis_adjustments_span_held_slices_without_changing_other_parts() {
        let origin = RuntimeTxn::Adjustment { place: Id::new(1), day: Day(10) };
        let (part, other) = (PartId { origin, ordinal: 0 }, PartId { origin, ordinal: 1 });
        let mut holdings = Holdings::new(2);
        let mut first = lot(3, 60, 10);
        first.part = Some(part);
        let mut second = lot(2, 40, 10);
        second.part = Some(part);
        let mut independent = lot(1, 80, 10);
        independent.part = Some(other);
        holdings.entry(Id::new(0), Id::new(0)).land(first, Held::Lots, &Arena::new());
        holdings.entry(Id::new(1), Id::new(0)).land(second, Held::Lots, &Arena::new());
        holdings.entry(Id::new(1), Id::new(0)).land(independent, Held::Lots, &Arena::new());

        assert_eq!(basis_of(&holdings, part), 100);
        holdings.adjust(Id::new(0), part, Qty(-25)).unwrap();
        assert_eq!((basis_of(&holdings, part), basis_of(&holdings, other)), (75, 80));
        holdings.adjust(Id::new(0), part, Qty(20)).unwrap();
        assert_eq!((basis_of(&holdings, part), basis_of(&holdings, other)), (95, 80));
        assert_eq!(
            holdings.adjust(Id::new(0), part, Qty(-96)),
            Err(AssetError::ParcelBasisMismatch),
            "no more than held"
        );
        assert_eq!(holdings.adjust(Id::new(1), part, Qty(5)), Err(AssetError::UnknownPart), "another commodity's");
        assert_eq!(basis_of(&holdings, part), 95);
    }

    #[test]
    fn part_basis_preflight_rejects_overflow_without_mutating_any_slice() {
        let origin = RuntimeTxn::Adjustment { place: Id::new(1), day: Day(10) };
        let part = PartId { origin, ordinal: 0 };
        let mut holdings = Holdings::new(2);
        let mut first = lot(i64::MAX, 10, 10);
        first.part = Some(part);
        let mut second = lot(1, 0, 10);
        second.part = Some(part);
        holdings.entry(Id::new(0), Id::new(0)).land(first, Held::Lots, &Arena::new());
        holdings.entry(Id::new(1), Id::new(0)).land(second, Held::Lots, &Arena::new());

        assert_eq!(basis_of(&holdings, part), 10);
        assert_eq!(holdings.adjust(Id::new(0), part, Qty(1)), Err(AssetError::Overflow), "the quantities overflow");
        let parcels: Vec<_> = holdings
            .iter()
            .flat_map(|slot| slot.holding.lots.iter())
            .map(|parcel| (parcel.qty, parcel.basis))
            .collect();
        assert_eq!(parcels, [(Qty(i64::MAX), Qty(10)), (Qty(1), Qty::ZERO)]);
        assert_eq!(basis_of(&holdings, part), 10);
    }

    fn addition(part: PartId, quantity: i64, amount: i64) -> CarryLotAddition {
        CarryLotAddition { part, acquired: Day(20), held_since: Day(1), quantity: Qty(quantity), amount: Qty(amount) }
    }

    #[test]
    fn carry_adds_basis_and_tacks_only_the_matched_quantity() {
        let origin = RuntimeTxn::Adjustment { place: Id::new(0), day: Day(20) };
        let part = PartId { origin, ordinal: 0 };
        let mut parcel = lot(5_000, 25_000, 20);
        parcel.part = Some(part);
        let mut holdings = Holdings::new(1);
        holdings.entry(Id::new(0), Id::new(0)).land(parcel, Held::Lots, &Arena::new());

        holdings.carry(Id::new(0), &[addition(part, 2_500, 1_000)]).unwrap();

        let slot = holdings.get(Id::new(0), Id::new(0)).unwrap();
        let mut lots: Vec<_> = slot
            .holding
            .lots
            .iter()
            .map(|parcel| (parcel.qty.0, parcel.basis.0, parcel.acquired.0, parcel.held_since.0))
            .collect();
        lots.sort_unstable();
        assert_eq!(lots, [(2_500, 12_500, 20, 20), (2_500, 13_500, 20, 1)]);
        assert_eq!(slot.qty, Qty(5_000), "a basis carry does not change physical quantity");
        assert_eq!(basis_of(&holdings, part), 26_000);
    }

    #[test]
    fn carries_into_one_lot_split_it_in_turn_and_the_last_takes_the_rest() {
        let origin = RuntimeTxn::Adjustment { place: Id::new(0), day: Day(20) };
        let part = PartId { origin, ordinal: 0 };
        let parcel = Parcel { part: Some(part), ..lot(10, 1_000, 20) };
        let mut holdings = Holdings::new(1);
        holdings.entry(Id::new(0), Id::new(0)).land(parcel, Held::Lots, &Arena::new());
        holdings.carry(Id::new(0), &[addition(part, 4, 40), addition(part, 6, 60)]).unwrap();
        let slot = holdings.get(Id::new(0), Id::new(0)).unwrap();
        let lots: Vec<_> = slot.holding.lots.iter().map(|lot| (lot.qty.0, lot.basis.0, lot.wash_matched)).collect();
        assert_eq!(lots, [(6, 660, true), (4, 440, true)], "the second takes what the first left, in the lot's place");
        assert_eq!(
            holdings.carry(Id::new(0), &[addition(part, 1, 1)]),
            Err(AssetError::ParcelBasisMismatch),
            "nothing unmatched is left"
        );
    }

    #[test]
    fn carry_quantity_preflight_is_atomic_when_the_target_slice_is_too_small() {
        let origin = RuntimeTxn::Adjustment { place: Id::new(0), day: Day(20) };
        let part = PartId { origin, ordinal: 0 };
        let mut parcel = lot(5_000, 25_000, 20);
        parcel.part = Some(part);
        let mut holdings = Holdings::new(1);
        holdings.entry(Id::new(0), Id::new(0)).land(parcel, Held::Lots, &Arena::new());

        let too_much = [addition(part, 3_000, 500), addition(part, 2_001, 500)];
        assert_eq!(holdings.carry(Id::new(0), &too_much), Err(AssetError::ParcelBasisMismatch));
        let slot = holdings.get(Id::new(0), Id::new(0)).unwrap();
        assert_eq!(slot.holding.lots, [parcel], "the first addition, which fits, wrote nothing either");
        assert_eq!(basis_of(&holdings, part), 25_000);
    }

    #[test]
    fn a_split_scales_quantity_and_keeps_basis() {
        let mut held = slot_of(1, 0, &[lot(3, 30, 1), lot(5, 50, 2)], Held::Lots);
        held.scale(Ratio::new(1, 2).unwrap());
        assert_eq!(lots(&held), [(2, 30), (2, 50)], "1.5 rounds to 2 and 2.5 to 2: half to even");
        assert_eq!(held.qty, Qty(4));
    }

    /// Whatever the policy and however lots come and go, the ordered paths
    /// take exactly what the scanning path does.
    #[test]
    fn the_ordered_paths_agree_with_scanning() {
        let mut dice = 0x9E37_79B9_7F4A_7C15u64;
        let mut roll = |below: u64| {
            dice ^= dice >> 12;
            dice ^= dice << 25;
            dice ^= dice >> 27;
            (dice.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 33) % below
        };
        for policy in [Policy::Fifo, Policy::Lifo, Policy::Hifo, Policy::Exact] {
            let (mut fast, mut slow) =
                (Slot::new(Id::new(0), Id::new(1), NONE), Slot::new(Id::new(0), Id::new(1), NONE));
            for step in 0..600 {
                if roll(3) < 2 {
                    let (day, qty) = (step / 3 + roll(3) as i32, 1 + roll(9) as i64);
                    let tied = (roll(6) == 0).then(|| Id::new(3));
                    // Lots of one day are the lines of one transaction, unless a part id tells them apart.
                    let part = Some(PartId { origin: journal(day as u32), ordinal: roll(3) as u32 });
                    let parcel = Parcel { tied, part, ..lot(qty, qty * (50 + roll(100) as i64), day) };
                    fast.land(parcel, Held::Lots, &Arena::new());
                    slow.land(parcel, Held::Lots, &Arena::new());
                } else {
                    let need = 1 + roll(14) as i64;
                    let (yes, no) = ([(Id::new(3), true)], [(Id::new(3), false)]);
                    let permits: &[(Id<Entity>, bool)] = [&[][..], &yes, &no][roll(3) as usize];
                    let spender = (roll(4) == 0).then(|| Id::new(3));
                    let fast_asks = Ask { policy: Some(policy), permits, spender, ..PLAIN };
                    // A selector that admits everything forces the scanning path.
                    let all = [Select::Range(Days::ALWAYS)];
                    let slow_asks = Ask { policy: Some(policy), selectors: &all, permits, spender, ..PLAIN };
                    let (a, b) = (relieve(&mut fast, need, &fast_asks), relieve(&mut slow, need, &slow_asks));
                    assert_eq!((taken(&a), a.shortfall), (taken(&b), b.shortfall), "{policy:?} step {step}");
                }
                assert_eq!(
                    (fast.qty, lots(&fast), fast.ties),
                    (slow.qty, lots(&slow), slow.ties),
                    "{policy:?} step {step}"
                );
            }
        }
    }
}
