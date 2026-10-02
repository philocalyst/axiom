//! Where value rests, and how it leaves: one [`Slot`] per `(place, commodity)`.
//!
//! A place holds a handful of commodities, usually one or two. Each place
//! therefore threads a short chain of slots, sorted by commodity, through one
//! flat vector. A lookup is one index and a hop or two, with no hashing;
//! iteration by place then commodity is the chain order; a new pair costs one
//! push; and cloning is two flat copies.
//!
//! A slot is the public [`Holding`] plus what keeps a sale from scanning it.
//! Lots are kept oldest first, so FIFO takes from the front (past a cursor over
//! the exhausted ones) and LIFO from the back, each in time proportional to the
//! lots it uses. HIFO asks a heap ordered by basis per unit, built when first
//! needed and kept up to date by every change to a lot. Only `prorata`,
//! selectors and ties have to look at every lot.
//!
//! Exhausted lots are left in place while the fold runs and swept out when
//! control returns to the caller, so the holdings a caller sees are always
//! clean.

use std::cmp::Ordering;
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

fn empty_codes() -> FlowCodes {
    let empty = axiom_core::Run::new(Id::new(0), 0);
    FlowCodes { header: empty, local: empty }
}

fn same_codes(left: FlowCodes, right: FlowCodes, pool: &Arena<Sym>) -> bool {
    let in_left = |code: &Sym| pool[left.header].contains(code) || pool[left.local].contains(code);
    let in_right = |code: &Sym| pool[right.header].contains(code) || pool[right.local].contains(code);
    pool[left.header].iter().chain(&pool[left.local]).all(in_right)
        && pool[right.header].iter().chain(&pool[right.local]).all(in_left)
}

/// What makes two parcels interchangeable. Parcels merge exactly when their
/// identities are equal, and relief between candidates with equal identities is
/// never ambiguous: taking any of them is the same as taking any other.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Identity {
    /// Plain value has no acquisition transaction and must not borrow a fake
    /// journal key merely to participate in ambiguity checks.
    Plain { basis: Qty, qty: Qty },
    /// Money: what matters is who it is tied to and how much of each unit is
    /// already accounted for. Where and when it arrived does not matter, so a
    /// 401k's hundreds of zero-basis deferrals are one lot.
    Money { tied: Option<Id<Entity>>, basis: Qty, qty: Qty, part: Option<PartId>, wash_matched: bool },
    /// Anything else: each purchase is its own lot, for selectors and for how
    /// long it has been held.
    Lot {
        acquired: Day,
        held_since: Day,
        wash_matched: bool,
        txn: RuntimeTxn,
        tied: Option<Id<Entity>>,
        part: Option<PartId>,
    },
}

impl PartialEq for Identity {
    fn eq(&self, other: &Identity) -> bool {
        match (*self, *other) {
            (Identity::Plain { basis: ab, qty: aq }, Identity::Plain { basis: bb, qty: bq }) => (ab, aq) == (bb, bq),
            (
                Identity::Money { tied: a, basis: ab, qty: aq, part: ap, wash_matched: aw },
                Identity::Money { tied: b, basis: bb, qty: bq, part: bp, wash_matched: bw },
            ) => {
                // Basis per unit, compared exactly: ab/aq == bb/bq.
                a == b && ap == bp && aw == bw && ab.0 as i128 * bq.0 as i128 == bb.0 as i128 * aq.0 as i128
            }
            (
                Identity::Lot { acquired: a, held_since: ah, wash_matched: aw, txn: at, tied: ap, part: apart },
                Identity::Lot { acquired: b, held_since: bh, wash_matched: bw, txn: bt, tied: bp, part: bpart },
            ) => (a, ah, aw, at, ap, apart) == (b, bh, bw, bt, bp, bpart),
            _ => false,
        }
    }
}

/// `money`: base-currency parcels outside claim places, which are told apart by
/// their basis per unit; every other parcel by the purchase that made it.
pub(crate) fn identity(parcel: &Parcel, money: bool) -> Identity {
    if money {
        Identity::Money {
            tied: parcel.tied,
            basis: parcel.basis,
            qty: parcel.qty,
            part: parcel.part,
            wash_matched: parcel.wash_matched,
        }
    } else {
        Identity::Lot {
            acquired: parcel.acquired,
            held_since: parcel.held_since,
            wash_matched: parcel.wash_matched,
            txn: parcel.txn,
            tied: parcel.tied,
            part: parcel.part,
        }
    }
}

/// Part of the value in flight: what left a place, and what it carries on.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Slice {
    pub qty: Qty,
    /// Basis relieved with it at the source.
    pub basis: Qty,
    pub acquired: Day,
    pub held_since: Day,
    pub wash_matched: bool,
    pub txn: RuntimeTxn,
    pub part: Option<PartId>,
    pub codes: FlowCodes,
    pub tied: Option<Id<Entity>>,
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
    fn new(qty: Qty, basis: Qty, origin: Origin, (acquired, txn): (Day, RuntimeTxn)) -> Slice {
        Slice {
            qty,
            basis,
            acquired,
            held_since: acquired,
            wash_matched: false,
            txn,
            part: None,
            codes: empty_codes(),
            tied: None,
            origin,
            worth: Qty::ZERO,
            carried: Qty::ZERO,
        }
    }

    /// Value that nothing gave up: base currency is at its face, anything else
    /// has no basis of its own.
    pub fn fresh(qty: Qty, is_base: bool, now: (Day, RuntimeTxn)) -> Slice {
        Slice::new(qty, if is_base { qty } else { Qty::ZERO }, Origin::Fresh, now)
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
    identity: Identity,
}

/// What relief is asked for.
pub(crate) struct Request<'a> {
    pub need: Qty,
    /// Base currency outside claim places: plain money has basis at its face,
    /// and acquisition dates do not tell parcels apart.
    pub money: bool,
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

/// What a parcel's tie says about when relief takes it. The variants are in
/// the order relief takes them.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
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

impl Colour {
    const ALL: [Colour; 4] = [Colour::Own, Colour::Permitted, Colour::Free, Colour::Refused];
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

    /// Whether a holding can have parcels of `colour` that may leave, given
    /// whether any of its parcels is tied at all: the colours nobody asked
    /// about are skipped.
    fn possible(&self, colour: Colour, tied: bool) -> bool {
        self.allows(colour)
            && match colour {
                Colour::Free => true,
                Colour::Own => tied && self.spender.is_some(),
                Colour::Permitted => tied && self.permits.iter().any(|&(_, permit)| permit),
                Colour::Refused => tied,
            }
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
    /// How many lots are tied to an entity: only then are there colours to weigh.
    ties: u32,
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

/// A lot as HIFO ranks it: dearest per unit first, then oldest.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Ranked {
    basis: i64,
    qty: i64,
    at: u32,
}

impl Ranked {
    fn of(at: usize, lot: &Parcel) -> Ranked {
        Ranked { basis: lot.basis.0, qty: lot.qty.0, at: at as u32 }
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
        Slot { holding, qty: Qty::ZERO, ties: 0, first: 0, dead: 0, ranked: None, next }
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

    /// Receives a parcel. Money at its face, tied to nothing, is plain; anything
    /// else joins the lots, merging into the interchangeable lot if there is one
    /// (its basis adds; it keeps its own acquisition day) and otherwise taking
    /// its place among the lots, oldest first.
    pub fn land(&mut self, parcel: Parcel, money: bool) {
        self.land_with_codes(parcel, money, &Arena::new());
    }

    /// Lands a parcel and merges only when its pooled code sets are equivalent.
    /// Range positions are handles, so two distinct ranges can name the same
    /// marks; compare their symbols before coalescing their selector identity.
    pub fn land_with_codes(&mut self, parcel: Parcel, money: bool, codes: &Arena<Sym>) {
        if money && parcel.tied.is_none() && parcel.basis == parcel.qty {
            return self.credit(parcel.qty);
        }
        self.qty += parcel.qty;

        let (kind, lots) = (identity(&parcel, money), &mut self.holding.lots);
        // Outside money only a lot acquired on the same day can match.
        let (start, end) = if money {
            (self.first, lots.len())
        } else {
            (
                lots.partition_point(|lot| lot.acquired < parcel.acquired),
                lots.partition_point(|lot| lot.acquired <= parcel.acquired),
            )
        };
        let same = lots[start..end].iter().position(|lot| {
            !lot.qty.is_zero() && identity(lot, money) == kind && same_codes(lot.codes, parcel.codes, codes)
        });
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

    /// Tells the heap that lot `at` has a new quantity and basis. It ranks untied lots only.
    fn revalue(&mut self, at: usize) {
        let lot = &self.holding.lots[at];
        if let Some(ranked) = self.ranked.as_mut().filter(|_| lot.tied.is_none()) {
            ranked.push(Ranked::of(at, lot));
        }
    }

    /// Takes `qty` from part of the holding, and appends what left to `out`.
    fn take(&mut self, source: Source, qty: Qty, req: &Request, out: &mut Relief) {
        let slice = match source {
            Source::Plain => {
                self.holding.plain -= qty;
                // Outside the base currency plain value has no basis of its own.
                Slice::new(qty, if req.money { qty } else { Qty::ZERO }, Origin::Plain, req.now)
            }
            Source::Lot(at) => {
                let lot = &mut self.holding.lots[at];
                let basis = if qty == lot.qty {
                    lot.basis
                } else {
                    lot.basis.share(qty, lot.qty).expect("a part of a basis fits")
                };
                let slice = Slice {
                    tied: lot.tied,
                    part: lot.part,
                    codes: lot.codes,
                    held_since: lot.held_since,
                    wash_matched: lot.wash_matched,
                    ..Slice::new(qty, basis, Origin::Lot, (lot.acquired, lot.txn))
                };
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

    /// Removes `req.need` from the holding, choosing what leaves by the
    /// request's selectors, ties and policy (in that order of precedence), and
    /// says what left. What could not be covered is `out.shortfall`, and comes
    /// out of plain value, so a holding can go negative.
    pub fn relieve(&mut self, req: &Request, out: &mut Relief) {
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
            && match policy {
                None => !self.is_tied(),
                Some(Policy::Hifo) => !req.money,
                Some(policy) => policy != Policy::Prorata,
            };
        if in_order {
            self.relieve_in_order(req, policy, out);
        } else {
            self.relieve_scanning(req, &selection, policy, out);
        }
        self.holding.plain -= out.shortfall;
        self.qty -= req.need;
        self.sweep_ends();
    }

    /// FIFO, LIFO and HIFO (and no policy at all): from the end of the holding
    /// that the policy names, touching only the lots it uses. Ties are colours:
    /// the spender's own lots go first, then lots tied to an entity whose laws
    /// permit the flow, untied ones next, and the rest last.
    fn relieve_in_order(&mut self, req: &Request, policy: Option<Policy>, out: &mut Relief) {
        let takeable = self.qty - self.holding.plain.min(Qty::ZERO);
        let candidates = usize::from(self.holding.plain > Qty::ZERO) + self.live();
        out.ambiguous = policy.is_none() && candidates > 1 && req.need < takeable;
        if out.ambiguous && (req.explain)() {
            self.gather(req.money, &Selection { selectors: &[], codes: req.codes }, &mut out.candidates);
        }
        let mut left = req.need;
        let (lifo, tied) = (policy == Some(Policy::Lifo), self.is_tied());
        for colour in Colour::ALL {
            if left.is_zero() || !req.possible(colour, tied) {
                continue;
            }
            let of_colour = |lot: &Parcel| !tied || req.colour(lot.tied) == colour;
            let plain_here = colour == Colour::Free;
            if plain_here && !lifo {
                self.take_plain(&mut left, req, out);
            }
            match (policy, colour) {
                (Some(Policy::Hifo), Colour::Free) => self.take_dearest(&mut left, req, out),
                (Some(Policy::Hifo), _) => self.take_priciest(&mut left, of_colour, req, out),
                _ => self.take_run(&mut left, lifo, of_colour, req, out),
            }
            if plain_here && lifo {
                self.take_plain(&mut left, req, out);
            }
        }
        out.shortfall = left;
    }

    fn take_plain(&mut self, left: &mut Qty, req: &Request, out: &mut Relief) {
        let qty = self.holding.plain.min(*left);
        if qty > Qty::ZERO {
            self.take(Source::Plain, qty, req, out);
            *left -= qty;
        }
    }

    /// The lots `keep` admits, from the front or from the back, until `left` is covered.
    fn take_run(
        &mut self,
        left: &mut Qty,
        lifo: bool,
        keep: impl Fn(&Parcel) -> bool,
        req: &Request,
        out: &mut Relief,
    ) {
        let live = |lot: &Parcel| !lot.qty.is_zero() && keep(lot);
        let mut at = if lifo { self.holding.lots.len() } else { self.first };
        while !left.is_zero() {
            let lots = &self.holding.lots;
            let found = if lifo {
                lots[..at].iter().rposition(live)
            } else {
                lots[at..].iter().position(live).map(|found| at + found)
            };
            let Some(found) = found else { return };
            let qty = self.holding.lots[found].qty.min(*left);
            self.take(Source::Lot(found), qty, req, out);
            *left -= qty;
            at = if lifo { found } else { found + 1 };
        }
    }

    /// The dearest lots `keep` admits, found by looking at every lot: only for the few tied ones.
    fn take_priciest(&mut self, left: &mut Qty, keep: impl Fn(&Parcel) -> bool, req: &Request, out: &mut Relief) {
        while !left.is_zero() {
            let live = self.holding.lots.iter().enumerate().filter(|(_, lot)| !lot.qty.is_zero() && keep(lot));
            let dearest = live.max_by(|(a, x), (b, y)| Ranked::of(*a, x).cmp(&Ranked::of(*b, y)));
            let Some((at, lot)) = dearest else { return };
            let qty = lot.qty.min(*left);
            self.take(Source::Lot(at), qty, req, out);
            *left -= qty;
        }
    }

    /// Untied lots dearest per unit first: whatever the heap says is on top and still true.
    fn take_dearest(&mut self, left: &mut Qty, req: &Request, out: &mut Relief) {
        while !left.is_zero() {
            let lots = &self.holding.lots;
            let ranked = self.ranked.get_or_insert_with(|| {
                let untied = lots.iter().enumerate().filter(|(_, lot)| !lot.qty.is_zero() && lot.tied.is_none());
                Box::new(untied.map(|(at, lot)| Ranked::of(at, lot)).collect())
            });
            let Some(&top) = ranked.peek() else { return };
            ranked.pop();
            let current = lots.get(top.at as usize).is_some_and(|lot| lot.qty.0 == top.qty && lot.basis.0 == top.basis);
            if current {
                let qty = Qty(top.qty).min(*left);
                self.take(Source::Lot(top.at as usize), qty, req, out);
                *left -= qty;
            }
        }
    }

    /// Everything else: look at every lot, weigh ties, then the policy.
    fn relieve_scanning(&mut self, req: &Request, selection: &Selection, policy: Option<Policy>, out: &mut Relief) {
        let mut candidates = std::mem::take(&mut out.gathered);
        candidates.clear();
        self.gather(req.money, selection, &mut candidates);
        let colour = |c: &Candidate| req.colour(c.tied);
        candidates.retain(|c| req.allows(colour(c)));
        candidates.sort_unstable_by(|a, b| colour(a).cmp(&colour(b)).then_with(|| by_policy(policy, a, b)));

        let mut plan = std::mem::take(&mut out.plan);
        plan.clear();
        let mut rest = candidates.as_slice();
        let mut left = req.need;
        while left > Qty::ZERO && !rest.is_empty() {
            let first = colour(&rest[0]);
            let (group, tail) = rest.split_at(rest.iter().take_while(|c| colour(c) == first).count());
            rest = tail;
            let total: Qty = group.iter().map(|c| c.qty).sum();
            let take = left.min(total);
            if take < total && policy.is_none() && !interchangeable(group) {
                out.ambiguous = true;
                if (req.explain)() {
                    out.candidates.extend_from_slice(group);
                }
            }
            allocate(group, take, policy == Some(Policy::Prorata), &mut plan);
            left -= take;
        }
        out.shortfall = left;
        for &(source, qty) in &plan {
            self.take(source, qty, req, out);
        }
        (out.plan, out.gathered) = (plan, candidates);
    }

    /// The parcels the selection admits, plain money first.
    fn gather(&self, money: bool, selection: &Selection, out: &mut Vec<Candidate>) {
        let plain = self.holding.plain;
        if plain > Qty::ZERO && !selection.constrains() {
            // Plain money has no transaction or acquisition day of its own.
            let basis = if money { plain } else { Qty::ZERO };
            out.push(Candidate {
                source: Source::Plain,
                qty: plain,
                basis,
                acquired: Day::MIN,
                txn: None,
                tied: None,
                identity: if money {
                    Identity::Money { tied: None, basis, qty: plain, part: None, wash_matched: false }
                } else {
                    Identity::Plain { basis, qty: plain }
                },
            });
        }
        let lots = self.holding.lots.iter().enumerate();
        let admitted = lots.filter(|(_, lot)| !lot.qty.is_zero() && selection.admits(lot));
        out.extend(admitted.map(|(at, lot)| Candidate::new(Source::Lot(at), lot, money)));
    }

    /// How much of the holding the selectors admit: what `all` means.
    pub fn admitted(&self, money: bool, selectors: &[Select], codes: &Arena<Sym>) -> Qty {
        let selection = Selection { selectors, codes };
        if !selection.constrains() {
            return self.qty - self.holding.plain.min(Qty::ZERO);
        }
        let mut found = Vec::new();
        self.gather(money, &selection, &mut found);
        found.iter().map(|c| c.qty).sum()
    }

    /// Spreads `delta` of basis over the admitted parcels in proportion to
    /// their quantity. Quantities do not change. Plain money takes a share too,
    /// and stops being plain when it does. `false` if there is nothing to carry
    /// it: no admitted parcel.
    pub fn rebase(&mut self, delta: Qty, selection: &Selection, money: bool, now: (Day, RuntimeTxn)) -> bool {
        let plain = self.holding.plain;
        if plain > Qty::ZERO && !selection.constrains() {
            let basis = if money { plain } else { Qty::ZERO };
            self.holding.plain = Qty::ZERO;
            self.insert(Parcel {
                qty: plain,
                basis,
                acquired: now.0,
                held_since: now.0,
                wash_matched: false,
                txn: now.1,
                part: None,
                codes: empty_codes(),
                tied: None,
            });
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
    pub fn basis(&self, money: bool) -> Qty {
        let plain = if money { self.holding.plain } else { Qty::ZERO };
        plain + self.holding.lots.iter().map(|lot| lot.basis).sum()
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
        let (mut ranges, mut codes) = (ranges.peekable(), codes.peekable());
        let in_range = ranges.peek().is_none() || ranges.any(|days| days.contains(lot.acquired));
        let marked = codes.peek().is_none()
            || codes.any(|code| {
                self.codes[lot.codes.header].contains(&code) || self.codes[lot.codes.local].contains(&code)
            });
        in_range && marked
    }
}

impl Candidate {
    fn new(source: Source, parcel: &Parcel, money: bool) -> Candidate {
        let (qty, basis, acquired, tied) = (parcel.qty, parcel.basis, parcel.acquired, parcel.tied);
        Candidate { source, qty, basis, acquired, txn: Some(parcel.txn), tied, identity: identity(parcel, money) }
    }
}

/// Candidates in the order the policy consumes them. Storage order is oldest
/// first, so FIFO is the identity, and it also orders "no policy", pro-rata
/// (whose order does not matter) and ties in the others.
fn by_policy(policy: Option<Policy>, a: &Candidate, b: &Candidate) -> Ordering {
    match policy {
        Some(Policy::Lifo) => b.source.cmp(&a.source),
        Some(Policy::Hifo) => basis_per_unit(b, a).then(a.source.cmp(&b.source)),
        _ => a.source.cmp(&b.source),
    }
}

/// Compares `a.basis / a.qty` with `b.basis / b.qty` without dividing.
fn basis_per_unit(a: &Candidate, b: &Candidate) -> Ordering {
    (a.basis.0 as i128 * b.qty.0 as i128).cmp(&(b.basis.0 as i128 * a.qty.0 as i128))
}

/// Whether taking any part of the group is the same as taking any other.
fn interchangeable(group: &[Candidate]) -> bool {
    group.iter().all(|c| c.identity == group[0].identity)
}

fn allocate(group: &[Candidate], take: Qty, prorata: bool, plan: &mut Vec<(Source, Qty)>) {
    if prorata {
        let mut shares = Shares::new(take, group.iter().map(|c| c.qty).sum());
        plan.extend(group.iter().map(|c| (c.source, shares.take(c.qty))));
        return;
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
    /// Derived locator for asset parcels. Entries are append-only slot ids;
    /// relief can leave a stale slot entry, which lookup verifies against the
    /// live parcels. This avoids searching unrelated holdings for every part
    /// basis query or adjustment.
    part_slots: axiom_core::Map<PartId, Vec<u32>>,
    /// A lot was exhausted since the last sweep.
    untidy: bool,
}

pub(crate) struct PartBasisAdjustment<'a> {
    holdings: &'a mut Holdings,
    part: PartId,
    delta: Qty,
    whole: Qty,
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

struct ParcelCarryChange {
    order: usize,
    slot: usize,
    parcel: usize,
    quantity: Qty,
    basis: Qty,
    new_basis: Qty,
    held_since: Day,
}

/// A preflighted partial lot carry. Matched slices are split from untouched
/// shares so basis and holding-period tacking apply to exactly the allocated
/// quantity.
pub(crate) struct CarryLotBatchAdjustment<'a> {
    holdings: &'a mut Holdings,
    changes: Vec<ParcelCarryChange>,
}

impl CarryLotBatchAdjustment<'_> {
    pub(crate) fn apply(self) {
        let mut changes = self.changes;
        changes.sort_by(|left, right| {
            right
                .slot
                .cmp(&left.slot)
                .then_with(|| right.parcel.cmp(&left.parcel))
                .then_with(|| left.order.cmp(&right.order))
        });
        for change in changes {
            let slot = &mut self.holdings.slots[change.slot];
            let source = slot.holding.lots[change.parcel];
            let old_piece_basis = change.basis;
            let mut carried = source;
            carried.qty = change.quantity;
            carried.basis = change.new_basis;
            carried.held_since = change.held_since;
            carried.wash_matched = true;
            if change.quantity == source.qty {
                slot.holding.lots[change.parcel] = carried;
            } else {
                slot.holding.lots[change.parcel].qty -= change.quantity;
                slot.holding.lots[change.parcel].basis -= old_piece_basis;
                let at = slot.holding.lots.partition_point(|lot| lot.acquired <= carried.acquired);
                slot.holding.lots.insert(at, carried);
                slot.first = slot.first.min(at);
                slot.ties += u32::from(carried.tied.is_some());
            }
            // The aggregate slot quantity is unchanged by splitting a parcel.
            // Any cached HIFO order points at stale lot positions.
            slot.ranked = None;
        }
    }
}

impl PartBasisAdjustment<'_> {
    /// Applies the prepared basis update while retaining an exclusive borrow
    /// of the indexed holdings store from validation through commit.
    pub(crate) fn apply(self) {
        if self.delta.is_zero() {
            return;
        }
        let magnitude = if self.delta.is_negative() {
            Qty(self.delta.0.checked_neg().expect("prepared magnitude"))
        } else {
            self.delta
        };
        let slots = self.holdings.part_slots.get(&self.part).expect("prepared part index");
        let mut shares = Shares::new(magnitude, self.whole);
        for &slot_id in slots {
            let Some(slot) = self.holdings.slots.get_mut(slot_id as usize) else { continue };
            let mut changed = false;
            for parcel in &mut slot.holding.lots {
                if parcel.part != Some(self.part) || parcel.qty <= Qty::ZERO {
                    continue;
                }
                let weight = if self.delta.is_negative() { parcel.basis } else { parcel.qty };
                let share = shares.take(weight);
                if self.delta.is_negative() {
                    parcel.basis -= share;
                } else {
                    parcel.basis += share;
                }
                changed = true;
            }
            if changed {
                slot.ranked = None;
            }
        }
    }
}

impl Holdings {
    pub fn new(places: usize) -> Holdings {
        Holdings { heads: vec![NONE; places], slots: Vec::new(), part_slots: axiom_core::Map::default(), untidy: false }
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
        &mut self.slots[at as usize]
    }

    /// Records that a live asset part can be held in this stable slot. The
    /// slot ids never move when lot vectors are sorted, split or swept.
    pub(crate) fn index_part_slot(&mut self, place: Id<Place>, unit: Id<Commodity>, part: PartId) {
        let slot = self.slot_index(place, unit).unwrap_or_else(|| {
            self.entry(place, unit);
            self.slot_index(place, unit).expect("entry creates the slot")
        });
        let slots = self.part_slots.entry(part).or_default();
        if !slots.contains(&(slot as u32)) {
            slots.push(slot as u32);
        }
    }

    fn slot_index(&self, place: Id<Place>, unit: Id<Commodity>) -> Option<usize> {
        let mut at = self.heads[place.index()];
        while at != NONE {
            let slot = &self.slots[at as usize];
            match slot.unit.cmp(&unit) {
                std::cmp::Ordering::Less => at = slot.next,
                std::cmp::Ordering::Equal => return Some(at as usize),
                std::cmp::Ordering::Greater => return None,
            }
        }
        None
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

    /// The total basis carried by every live slice of one canonical part.
    pub(crate) fn part_basis(&self, part: PartId) -> Result<Qty, AssetError> {
        let mut found = false;
        self.part_slots
            .get(&part)
            .into_iter()
            .flatten()
            .filter_map(|&slot| self.slots.get(slot as usize))
            .flat_map(|slot| slot.holding.lots.iter())
            .filter(|parcel| parcel.part == Some(part) && parcel.qty > Qty::ZERO)
            .try_fold(Qty::ZERO, |basis, parcel| {
                found = true;
                basis.0.checked_add(parcel.basis.0).map(Qty).ok_or(AssetError::Overflow)
            })
            .and_then(|basis| if found { Ok(basis) } else { Err(AssetError::UnknownPart) })
    }

    /// Applies a basis change to all live slices of a part in proportion to
    /// their current basis (consumption) or quantity (capital carry). Shares
    /// are rounded once across the whole part, so their sum stays exact.
    pub(crate) fn adjust_part_basis(&mut self, part: PartId, delta: Qty) -> Result<(), AssetError> {
        self.prepare_part_basis_adjustment(part, delta)?.apply();
        Ok(())
    }

    /// Preflights every parcel-level share before either the holdings or the
    /// asset-part table is mutated. The returned guard keeps an exclusive
    /// borrow of this store until its infallible `apply` consumes it.
    pub(crate) fn prepare_part_basis_adjustment(
        &mut self,
        part: PartId,
        delta: Qty,
    ) -> Result<PartBasisAdjustment<'_>, AssetError> {
        if delta.is_zero() {
            return Ok(PartBasisAdjustment { holdings: self, part, delta, whole: Qty::ZERO });
        }
        let basis = self.part_basis(part)?;
        let magnitude =
            if delta.is_negative() { delta.0.checked_neg().map(Qty).ok_or(AssetError::Overflow)? } else { delta };
        let slots = self.part_slots.get(&part).ok_or(AssetError::UnknownPart)?;
        let weights = if delta.is_negative() {
            if magnitude > basis {
                return Err(AssetError::ParcelBasisMismatch);
            }
            basis
        } else {
            slots
                .iter()
                .filter_map(|&slot| self.slots.get(slot as usize))
                .flat_map(|slot| slot.holding.lots.iter())
                .filter(|parcel| parcel.part == Some(part) && parcel.qty > Qty::ZERO)
                .try_fold(Qty::ZERO, |qty, parcel| {
                    qty.0.checked_add(parcel.qty.0).map(Qty).ok_or(AssetError::Overflow)
                })?
        };
        if weights.is_zero() {
            return Err(AssetError::UnknownPart);
        }
        if !delta.is_negative() {
            basis.0.checked_add(magnitude.0).ok_or(AssetError::Overflow)?;
        }
        let mut shares = Shares::new(magnitude, weights);
        for &slot_id in slots {
            let Some(slot) = self.slots.get(slot_id as usize) else { continue };
            for parcel in &slot.holding.lots {
                if parcel.part != Some(part) || parcel.qty <= Qty::ZERO {
                    continue;
                }
                let weight = if delta.is_negative() { parcel.basis } else { parcel.qty };
                let share = shares.take(weight);
                if delta.is_negative() {
                    parcel.basis.0.checked_sub(share.0).ok_or(AssetError::Overflow)?;
                } else {
                    parcel.basis.0.checked_add(share.0).ok_or(AssetError::Overflow)?;
                }
            }
        }
        Ok(PartBasisAdjustment { holdings: self, part, delta, whole: weights })
    }

    /// Preflights quantity-scoped carry updates by actual acquisition day.
    /// The guard reserves exact parcel indices and keeps this holdings store
    /// exclusively borrowed until the split, basis adjustment, and holding
    /// period tacking are committed together.
    pub(crate) fn prepare_part_carry_additions(
        &mut self,
        additions: &[CarryLotAddition],
    ) -> Result<CarryLotBatchAdjustment<'_>, AssetError> {
        let mut plans = Vec::new();
        for (addition_index, addition) in additions.iter().enumerate() {
            if addition.quantity <= Qty::ZERO || addition.amount.is_negative() {
                return Err(AssetError::NegativeAmount);
            }
            let slots = self.part_slots.get(&addition.part).ok_or(AssetError::UnknownPart)?;
            let mut whole = Qty::ZERO;
            for &slot_id in slots {
                let slot_index = slot_id as usize;
                let Some(slot) = self.slots.get(slot_index) else { continue };
                for (parcel_index, parcel) in slot.holding.lots.iter().enumerate() {
                    if parcel.part != Some(addition.part)
                        || parcel.acquired != addition.acquired
                        || parcel.wash_matched
                        || parcel.qty <= Qty::ZERO
                    {
                        continue;
                    }
                    let used = plans
                        .iter()
                        .filter(|plan: &&ParcelCarryChange| plan.slot == slot_index && plan.parcel == parcel_index)
                        .try_fold(Qty::ZERO, |sum, plan| {
                            sum.0.checked_add(plan.quantity.0).map(Qty).ok_or(AssetError::Overflow)
                        })?;
                    let available = parcel.qty.0.checked_sub(used.0).ok_or(AssetError::Overflow)?;
                    whole.0 = whole.0.checked_add(available).ok_or(AssetError::Overflow)?;
                }
            }
            if whole < addition.quantity {
                return Err(AssetError::ParcelBasisMismatch);
            }
            let mut quantities = Shares::new(addition.quantity, whole);
            let mut basis_additions = Shares::new(addition.amount, addition.quantity);
            for &slot_id in slots {
                let slot_index = slot_id as usize;
                let Some(slot) = self.slots.get(slot_index) else { continue };
                for (parcel_index, parcel) in slot.holding.lots.iter().enumerate() {
                    if parcel.part != Some(addition.part)
                        || parcel.acquired != addition.acquired
                        || parcel.wash_matched
                        || parcel.qty <= Qty::ZERO
                    {
                        continue;
                    }
                    let used_qty = plans
                        .iter()
                        .filter(|plan: &&ParcelCarryChange| plan.slot == slot_index && plan.parcel == parcel_index)
                        .try_fold(Qty::ZERO, |sum, plan| {
                            sum.0.checked_add(plan.quantity.0).map(Qty).ok_or(AssetError::Overflow)
                        })?;
                    let used_basis = plans
                        .iter()
                        .filter(|plan: &&ParcelCarryChange| plan.slot == slot_index && plan.parcel == parcel_index)
                        .try_fold(Qty::ZERO, |sum, plan| {
                            sum.0.checked_add(plan.basis.0).map(Qty).ok_or(AssetError::Overflow)
                        })?;
                    let available = Qty(parcel.qty.0.checked_sub(used_qty.0).ok_or(AssetError::Overflow)?);
                    let available_basis = Qty(parcel.basis.0.checked_sub(used_basis.0).ok_or(AssetError::Overflow)?);
                    let quantity = quantities.take(available);
                    if quantity.is_zero() {
                        continue;
                    }
                    let basis = if quantity == available {
                        available_basis
                    } else {
                        available_basis.share(quantity, available).ok_or(AssetError::Overflow)?
                    };
                    let amount = basis_additions.take(quantity);
                    let new_basis = Qty(basis.0.checked_add(amount.0).ok_or(AssetError::Overflow)?);
                    plans.push(ParcelCarryChange {
                        order: addition_index,
                        slot: slot_index,
                        parcel: parcel_index,
                        quantity,
                        basis,
                        new_basis,
                        held_since: addition.held_since.min(parcel.held_since),
                    });
                }
            }
        }
        Ok(CarryLotBatchAdjustment { holdings: self, changes: plans })
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

    /// Spreads `delta` of basis over the parcels one holding of `place` can
    /// carry (the first that has any the selection admits). `false` if none can.
    pub fn rebase(
        &mut self,
        place: Id<Place>,
        delta: Qty,
        selection: &Selection,
        money: impl Fn(Id<Commodity>) -> bool,
        now: (Day, RuntimeTxn),
    ) -> bool {
        let mut at = self.heads[place.index()];
        while at != NONE {
            let slot = &mut self.slots[at as usize];
            if slot.rebase(delta, selection, money(slot.unit), now) {
                return true;
            }
            at = slot.next;
        }
        false
    }

    /// A split: every holding of `unit`, in every place, is multiplied by `ratio`.
    pub fn scale(&mut self, unit: Id<Commodity>, ratio: Ratio) {
        self.slots.iter_mut().filter(|slot| slot.unit == unit).for_each(|slot| slot.scale(ratio));
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
    use axiom_core::{Day, Days};

    use super::*;

    fn span(first: i32, last: i32) -> Days {
        Days::new(Day(first), Day(last)).unwrap()
    }

    fn journal(txn: u32) -> RuntimeTxn {
        RuntimeTxn::journal(Id::new(txn)).unwrap()
    }

    fn lot(qty: i64, basis: i64, acquired: i32) -> Parcel {
        Parcel {
            qty: Qty(qty),
            basis: Qty(basis),
            acquired: Day(acquired),
            held_since: Day(acquired),
            wash_matched: false,
            txn: journal(acquired as u32),
            part: None,
            codes: empty_codes(),
            tied: None,
        }
    }

    fn slot_of(unit: u32, plain: i64, lots: &[Parcel], money: bool) -> Slot {
        let mut slot = Slot::new(Id::new(0), Id::new(unit), NONE);
        slot.credit(Qty(plain));
        lots.iter().for_each(|&lot| slot.land(lot, money));
        slot
    }

    struct Ask<'a> {
        money: bool,
        policy: Option<Policy>,
        selectors: &'a [Select],
        permits: &'a [(Id<Entity>, bool)],
        spender: Option<Id<Entity>>,
    }

    const PLAIN: Ask = Ask { money: false, policy: None, selectors: &[], permits: &[], spender: None };

    fn relieve(slot: &mut Slot, need: i64, ask: &Ask) -> Relief {
        let mut relief = Relief::default();
        let codes = Arena::new();
        let (money, policy, selectors, permits) = (ask.money, ask.policy, ask.selectors, ask.permits);
        let (spender, now) = (ask.spender, (Day(1_000), journal(0)));
        let request = Request {
            need: Qty(need),
            money,
            selectors,
            policy,
            codes: &codes,
            permits,
            spender,
            now,
            explain: &|| true,
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
        let mut slot = slot_of(3, 0, &[lot(10, 10, 5), lot(0, 0, 7)], false);
        let relief = relieve(&mut slot, 10, &Ask { policy: Some(Policy::Lifo), ..PLAIN });
        assert_eq!(taken(&relief), [(10, 10)]);
        assert!(slot.holding.lots.is_empty() && slot.dead == 0, "both ends swept, and the count agrees");
    }

    #[test]
    fn lots_merge_by_identity_and_stay_oldest_first() {
        let mut held = Holdings::new(2);
        let (place, unit) = (Id::new(1), Id::new(3));
        let slot = held.entry(place, unit);
        slot.land(lot(5, 50, 20), false);
        slot.land(lot(2, 10, 10), false);
        slot.land(lot(3, 30, 20), false);
        slot.land(Parcel { txn: journal(9), ..lot(1, 10, 20) }, false);
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
            slot.land(Parcel { tied, ..lot(qty, basis, acquired) }, true);
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
        held.entry(Id::new(0), Id::new(2)).land(lot(4, 4, 0), true);
        let order: Vec<_> =
            held.iter().map(|s| (s.place.index(), s.unit.index(), s.plain.0, s.lots.capacity())).collect();
        assert_eq!(order, [(0, 2, 5, 0), (0, 5, 1, 0), (2, 1, 7, 0)]);
    }

    #[test]
    fn prorata_parts_sum_exactly() {
        let mut held = slot_of(1, 0, &[lot(7, 100, 1), lot(11, 250, 2), lot(13, 333, 3)], false);
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
        let base = slot_of(1, 0, &[lot(10, 1_000, 1), lot(10, 3_000, 2), lot(10, 2_000, 3)], false);
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
    fn hifo_follows_a_lot_whose_basis_changed_and_lots_that_arrive() {
        let mut held = slot_of(1, 0, &[lot(10, 1_000, 1), lot(10, 2_000, 2)], false);
        let hifo = Ask { policy: Some(Policy::Hifo), ..PLAIN };
        assert_eq!(taken(&relieve(&mut held, 4, &hifo)), [(4, 800)]);
        held.land(lot(5, 5_000, 3), false);
        assert_eq!(taken(&relieve(&mut held, 6, &hifo)), [(5, 5_000), (1, 200)]);
        assert!(held.rebase(
            Qty(9_000),
            &Selection { selectors: &[], codes: &Arena::new() },
            false,
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
        let mut held = slot_of(1, 0, &[lot(1, 10, 1), lot(1, 10, 2), lot(1, 10, 3), lot(1, 10, 4)], false);
        let (fifo, lifo) = (Ask { policy: Some(Policy::Fifo), ..PLAIN }, Ask { policy: Some(Policy::Lifo), ..PLAIN });
        relieve(&mut held, 2, &fifo);
        relieve(&mut held, 1, &lifo);
        assert_eq!(lots(&held), [(1, 10)]);
        assert_eq!(held.holding.lots.len(), 3, "the back was trimmed; the front is a cursor, not a removal");
        held.land(lot(2, 20, 0), false);
        assert_eq!(
            taken(&relieve(&mut held, 3, &fifo)),
            [(2, 20), (1, 10)],
            "an older lot arrives ahead of the cursor"
        );
    }

    #[test]
    fn only_lots_that_differ_are_ambiguous_without_a_policy() {
        let mut same = slot_of(1, 0, &[lot(10, 1_000, 5)], false);
        assert!(!relieve(&mut same, 6, &PLAIN).ambiguous);
        let purchase = |txn| Parcel { txn: journal(txn), ..lot(10, 1_500, 5) };
        let differ = slot_of(1, 0, &[lot(10, 1_000, 5), purchase(99)], false);
        let relief = relieve(&mut differ.clone(), 6, &PLAIN);
        assert!(relief.ambiguous);
        assert_eq!(relief.candidates.len(), 2);
        assert_eq!(taken(&relief), [(6, 600)], "FIFO carries on");
        assert!(!relieve(&mut differ.clone(), 6, &Ask { policy: Some(Policy::Fifo), ..PLAIN }).ambiguous);
        assert!(!relieve(&mut differ.clone(), 20, &PLAIN).ambiguous, "taking everything leaves no choice");
        let days = slot_of(1, 0, &[lot(10, 1_000, 5), lot(10, 1_000, 6)], false);
        assert!(relieve(&mut days.clone(), 6, &PLAIN).ambiguous, "each purchase is its own lot outside the base");
    }

    #[test]
    fn plain_money_and_a_zero_basis_lot_differ_but_deferrals_do_not() {
        let money = Ask { money: true, ..PLAIN };
        let mixed = slot_of(1, 700, &[lot(300, 0, 1)], true);
        assert!(relieve(&mut mixed.clone(), 100, &money).ambiguous, "after-tax and pre-tax money differ");
        let deferrals = slot_of(1, 0, &[lot(100, 0, 1), lot(50, 0, 30)], true);
        assert!(!relieve(&mut deferrals.clone(), 20, &money).ambiguous, "zero-basis deferrals from any day are alike");
    }

    #[test]
    fn prorata_over_plain_and_a_zero_basis_lot_splits_the_withdrawal() {
        let mut held = slot_of(1, 6_300_00, &[lot(2_200_00, 0, 3)], true);
        let relief = relieve(&mut held, 1_500_00, &Ask { money: true, policy: Some(Policy::Prorata), ..PLAIN });
        assert_eq!(taken(&relief), [(1_111_76, 1_111_76), (388_24, 0)]);
    }

    #[test]
    fn a_sale_beyond_what_is_held_reports_the_shortfall_and_goes_negative() {
        let mut held = slot_of(1, 0, &[lot(7, 700, 1)], false);
        let relief = relieve(&mut held, 10, &PLAIN);
        assert_eq!((relief.shortfall, taken(&relief)), (Qty(3), vec![(7, 700)]));
        assert_eq!((held.qty, held.holding.plain), (Qty(-3), Qty(-3)));
    }

    #[test]
    fn tied_parcels_go_first_only_when_their_laws_permit() {
        let grant = Id::new(4);
        let tied = Parcel { tied: Some(grant), ..lot(5, 5, 9) };
        let held = slot_of(1, 0, &[lot(10, 10, 1), tied], true);
        let first = |permit: bool| {
            let permits = [(grant, permit)];
            relieve(&mut held.clone(), 4, &Ask { money: true, permits: &permits, ..PLAIN }).slices[0].tied
        };
        assert_eq!(first(true), Some(grant));
        assert_eq!(first(false), None);
    }

    #[test]
    fn a_spender_takes_its_own_parcels_then_untied_ones_and_nobody_elses() {
        let (car, trip) = (Id::new(4), Id::new(5));
        let tied = |entity, acquired| Parcel { tied: Some(entity), ..lot(5, 5, acquired) };
        let held = slot_of(1, 0, &[tied(trip, 1), tied(car, 2), lot(10, 10, 3)], true);
        let order = |spender, permits: &[(Id<Entity>, bool)], need| {
            let ask = Ask { money: true, policy: Some(Policy::Fifo), spender, permits, ..PLAIN };
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
        let held = slot_of(1, 0, &[lot(1, 1, 10), lot(2, 2, 20), lot(4, 4, 30)], false);
        let codes = Arena::new();
        let pick = |selectors: &[Select]| held.admitted(false, selectors, &codes).0;
        assert_eq!(pick(&[]), 7);
        assert_eq!(pick(&[Select::Range(span(10, 20))]), 3);
        assert_eq!(pick(&[Select::Range(span(10, 10)), Select::Range(span(30, 30))]), 5);
        assert_eq!(pick(&[Select::Policy(Policy::Lifo)]), 7);
    }

    #[test]
    fn plain_identity_is_reflexive_without_a_fabricated_transaction_key() {
        let mut slot = slot_of(1, 0, &[], false);
        slot.credit(Qty(5));
        let candidates = {
            let mut out = Vec::new();
            slot.gather(false, &Selection { selectors: &[], codes: &Arena::new() }, &mut out);
            out
        };
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].txn, None);
        assert_eq!(candidates[0].identity, candidates[0].identity);
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
        held.land_with_codes(parcel, false, &pool);

        assert_eq!(held.admitted(false, &[Select::Code(header)], &pool), Qty(5));
        assert_eq!(held.admitted(false, &[Select::Code(local)], &pool), Qty(5));
        assert_eq!(held.admitted(false, &[Select::Code(other)], &pool), Qty::ZERO);
    }

    #[test]
    fn moving_a_lot_through_a_slice_preserves_its_pooled_code_identity() {
        let mut names = axiom_core::Interner::default();
        let original = names.intern("original-purchase");
        let mut pool = Arena::new();
        let first = pool.push(original);
        let codes = FlowCodes { header: axiom_core::Run::new(first, 1), local: empty_codes().local };
        let mut parcel = lot(7, 700, 10);
        parcel.codes = codes;
        let mut source = Slot::new(Id::new(0), Id::new(0), NONE);
        source.land_with_codes(parcel, false, &pool);

        let mut relief = Relief::default();
        let request = Request {
            need: Qty(3),
            money: false,
            selectors: &[],
            policy: Some(Policy::Fifo),
            codes: &pool,
            permits: &[],
            spender: None,
            now: (Day(20), journal(20)),
            explain: &|| false,
        };
        source.relieve(&request, &mut relief);
        let slice = relief.slices[0];
        let moved = Parcel {
            qty: slice.qty,
            basis: slice.basis,
            acquired: slice.acquired,
            held_since: slice.held_since,
            wash_matched: slice.wash_matched,
            txn: slice.txn,
            part: slice.part,
            codes: slice.codes,
            tied: slice.tied,
        };
        let mut target = Slot::new(Id::new(1), Id::new(0), NONE);
        target.land_with_codes(moved, false, &pool);

        assert_eq!(target.admitted(false, &[Select::Code(original)], &pool), Qty(3));
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
        let one = FlowCodes { header: axiom_core::Run::new(first, 1), local: empty_codes().local };
        let equal = FlowCodes { header: axiom_core::Run::new(second, 1), local: empty_codes().local };
        let distinct = FlowCodes { header: axiom_core::Run::new(other, 1), local: empty_codes().local };
        let mut slot = Slot::new(Id::new(0), Id::new(0), NONE);
        let mut first_parcel = lot(2, 2, 10);
        first_parcel.codes = one;
        let mut equivalent_parcel = lot(3, 3, 10);
        equivalent_parcel.codes = equal;
        let mut distinct_parcel = lot(1, 1, 10);
        distinct_parcel.codes = distinct;
        slot.land_with_codes(first_parcel, false, &pool);
        slot.land_with_codes(equivalent_parcel, false, &pool);
        slot.land_with_codes(distinct_parcel, false, &pool);

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
        let mut slot = slot_of(1, 0, &[a, b], false);

        assert_eq!(slot.holding.lots.len(), 2, "distinct cost-basis parts remain addressable");
        let mut relief = Relief::default();
        let codes = Arena::new();
        let request = Request {
            need: Qty(2),
            money: false,
            selectors: &[],
            policy: Some(Policy::Fifo),
            codes: &codes,
            permits: &[],
            spender: None,
            now: (Day(20), journal(20)),
            explain: &|| false,
        };
        slot.relieve(&request, &mut relief);
        assert_eq!(relief.slices[0].part, Some(first));

        let slice = relief.slices[0];
        let moved = Parcel {
            qty: slice.qty,
            basis: slice.carried,
            acquired: slice.acquired,
            held_since: slice.held_since,
            wash_matched: slice.wash_matched,
            txn: slice.txn,
            part: slice.part,
            codes: slice.codes,
            tied: slice.tied,
        };
        assert_eq!(moved.part, Some(first), "ordinary transfer carries the part key");
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
        holdings.entry(Id::new(0), Id::new(0)).land(first, false);
        holdings.index_part_slot(Id::new(0), Id::new(0), part);
        holdings.entry(Id::new(1), Id::new(0)).land(second, false);
        holdings.index_part_slot(Id::new(1), Id::new(0), part);
        holdings.entry(Id::new(1), Id::new(0)).land(independent, false);
        holdings.index_part_slot(Id::new(1), Id::new(0), other);

        assert_eq!(holdings.part_basis(part), Ok(Qty(100)));
        holdings.adjust_part_basis(part, Qty(-25)).unwrap();
        assert_eq!(holdings.part_basis(part), Ok(Qty(75)));
        assert_eq!(holdings.part_basis(other), Ok(Qty(80)));
        holdings.adjust_part_basis(part, Qty(20)).unwrap();
        assert_eq!(holdings.part_basis(part), Ok(Qty(95)));
        assert_eq!(holdings.part_basis(other), Ok(Qty(80)));
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
        holdings.entry(Id::new(0), Id::new(0)).land(first, false);
        holdings.index_part_slot(Id::new(0), Id::new(0), part);
        holdings.entry(Id::new(1), Id::new(0)).land(second, false);
        holdings.index_part_slot(Id::new(1), Id::new(0), part);

        assert_eq!(holdings.part_basis(part), Ok(Qty(10)));
        assert_eq!(holdings.prepare_part_basis_adjustment(part, Qty(1)).err(), Some(AssetError::Overflow));
        assert_eq!(holdings.adjust_part_basis(part, Qty(1)), Err(AssetError::Overflow));
        let parcels: Vec<_> = holdings
            .iter()
            .flat_map(|slot| slot.holding.lots.iter())
            .map(|parcel| (parcel.qty, parcel.basis))
            .collect();
        assert_eq!(parcels, [(Qty(i64::MAX), Qty(10)), (Qty(1), Qty::ZERO)]);
        assert_eq!(holdings.part_basis(part), Ok(Qty(10)));
    }

    #[test]
    fn carry_adds_basis_and_tacks_only_the_matched_quantity() {
        let origin = RuntimeTxn::Adjustment { place: Id::new(0), day: Day(20) };
        let part = PartId { origin, ordinal: 0 };
        let mut parcel = lot(5_000, 25_000, 20);
        parcel.part = Some(part);
        let mut holdings = Holdings::new(1);
        holdings.entry(Id::new(0), Id::new(0)).land(parcel, false);
        holdings.index_part_slot(Id::new(0), Id::new(0), part);

        holdings
            .prepare_part_carry_additions(&[CarryLotAddition {
                part,
                acquired: Day(20),
                held_since: Day(1),
                quantity: Qty(2_500),
                amount: Qty(1_000),
            }])
            .unwrap()
            .apply();

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
        assert_eq!(holdings.part_basis(part), Ok(Qty(26_000)));
    }

    #[test]
    fn carry_quantity_preflight_is_atomic_when_the_target_slice_is_too_small() {
        let origin = RuntimeTxn::Adjustment { place: Id::new(0), day: Day(20) };
        let part = PartId { origin, ordinal: 0 };
        let mut parcel = lot(5_000, 25_000, 20);
        parcel.part = Some(part);
        let mut holdings = Holdings::new(1);
        holdings.entry(Id::new(0), Id::new(0)).land(parcel, false);
        holdings.index_part_slot(Id::new(0), Id::new(0), part);

        assert_eq!(
            holdings
                .prepare_part_carry_additions(&[CarryLotAddition {
                    part,
                    acquired: Day(20),
                    held_since: Day(1),
                    quantity: Qty(5_001),
                    amount: Qty(1_000),
                }])
                .err(),
            Some(AssetError::ParcelBasisMismatch)
        );
        let slot = holdings.get(Id::new(0), Id::new(0)).unwrap();
        assert_eq!(slot.holding.lots, [parcel]);
        assert_eq!(holdings.part_basis(part), Ok(Qty(25_000)));
    }

    #[test]
    fn a_split_scales_quantity_and_keeps_basis() {
        let mut held = slot_of(1, 0, &[lot(3, 30, 1), lot(5, 50, 2)], false);
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
        for policy in [Policy::Fifo, Policy::Lifo, Policy::Hifo] {
            let (mut fast, mut slow) =
                (Slot::new(Id::new(0), Id::new(1), NONE), Slot::new(Id::new(0), Id::new(1), NONE));
            for step in 0..600 {
                if roll(3) < 2 {
                    let (day, qty) = (step / 3 + roll(3) as i32, 1 + roll(9) as i64);
                    let tied = (roll(6) == 0).then(|| Id::new(3));
                    let parcel = Parcel { tied, ..lot(qty, qty * (50 + roll(100) as i64), day) };
                    fast.land(parcel, false);
                    slow.land(parcel, false);
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
