//! Relief: choosing which parcels leave a holding.
//!
//! [`plan`] is a pure function from a holding and a [`Request`] to a
//! [`Relief`]: which parcels, how much of each, the basis that goes with it,
//! what could not be covered, and whether the choice was ambiguous. Nothing is
//! removed. That is what lets the ledger list every candidate with the gain it
//! *would* realize before deciding, and lets a hypothetical withdrawal ask the
//! same question of a clone.
//!
//! The order of precedence (PLAN §10):
//! 1. selectors (`[2024]`, `[#house]`) restrict the candidates;
//! 2. colours: parcels tied to an entity whose `on spend` laws permit the flow
//!    go first, untied parcels next, and forbidden tied parcels last;
//! 3. the policy (selector, else place, else kind chain) orders each colour;
//!    with none, only relief among candidates that are not interchangeable
//!    (different [`Identity`]) is ambiguous.

use std::cmp::Ordering;

use axiom_core::{Arena, Day, Id, Qty};
use axiom_model::{Entity, Policy, Select, Txn};

use crate::holdings::{Identity, identity};
use crate::{Holding, Parcel};

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
    pub tied: Option<Id<Entity>>,
    pub identity: Identity,
}

/// Part of a candidate, chosen to leave.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Piece {
    pub source: Source,
    pub qty: Qty,
    /// Basis relieved with it, base-currency quanta. Pieces of one lot sum to
    /// the lot's basis exactly when they sum to its quantity.
    pub basis: Qty,
}

/// The plan for one request. Buffers are reused between requests.
#[derive(Default)]
pub(crate) struct Relief {
    pub pieces: Vec<Piece>,
    /// Wanted, not held.
    pub shortfall: Qty,
    /// When no rule chose between candidates that differ, all of them.
    pub ambiguous: Vec<Candidate>,
    candidates: Vec<Candidate>,
}

pub(crate) struct Request<'a> {
    pub need: Qty,
    /// Whether the commodity is the base currency: plain money has basis at
    /// face, and acquisition dates do not tell base parcels apart.
    pub is_base: bool,
    pub selectors: &'a [Select],
    /// The place's policy; a policy selector on the flow overrides it.
    pub policy: Option<Policy>,
    pub txns: &'a Arena<Txn>,
    /// For each entity a parcel here is tied to: whether its `on spend` laws
    /// permit this flow.
    pub permits: &'a [(Id<Entity>, bool)],
}

/// Plans the relief of `request.need` from `holding`, writing it to `out`.
pub(crate) fn plan(holding: Option<&Holding>, request: &Request, out: &mut Relief) {
    out.pieces.clear();
    out.ambiguous.clear();
    out.candidates.clear();
    out.shortfall = request.need;
    let Some(holding) = holding else { return };
    let selection = Selection { selectors: request.selectors, txns: request.txns };
    gather(holding, request.is_base, &selection, &mut out.candidates);
    let policy = selection.policy().or(request.policy);
    let colour = |c: &Candidate| rank(c.tied, request.permits);
    out.candidates.sort_unstable_by(|a, b| colour(a).cmp(&colour(b)).then_with(|| by_policy(policy, a, b)));

    let mut rest = out.candidates.as_slice();
    while out.shortfall > Qty::ZERO && !rest.is_empty() {
        let first = colour(&rest[0]);
        let (group, tail) = rest.split_at(rest.iter().take_while(|c| colour(c) == first).count());
        rest = tail;
        let total: Qty = group.iter().map(|c| c.qty).sum();
        let take = out.shortfall.min(total);
        if take < total && policy.is_none() && !interchangeable(group) {
            out.ambiguous.extend_from_slice(group);
        }
        allocate(group, take, policy == Some(Policy::Prorata), &mut out.pieces);
        out.shortfall -= take;
    }
}

/// How much of `holding` the selectors admit: what `all` means.
pub(crate) fn admitted(holding: &Holding, is_base: bool, selectors: &[Select], txns: &Arena<Txn>) -> Qty {
    let mut found = Vec::new();
    gather(holding, is_base, &Selection { selectors, txns }, &mut found);
    found.iter().map(|c| c.qty).sum()
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
struct Selection<'a> {
    selectors: &'a [Select],
    txns: &'a Arena<Txn>,
}

impl Selection<'_> {
    /// Whether plain money, which has no acquisition day or code, is excluded.
    fn constrains(&self) -> bool {
        self.selectors.iter().any(|s| !matches!(s, Select::Policy(_)))
    }

    fn policy(&self) -> Option<Policy> {
        self.selectors.iter().find_map(|s| if let Select::Policy(p) = *s { Some(p) } else { None })
    }

    fn admits(&self, lot: &Parcel) -> bool {
        let ranges =
            self.selectors.iter().filter_map(|s| if let Select::Range(a, b) = *s { Some((a, b)) } else { None });
        let codes = self.selectors.iter().filter_map(|s| if let Select::Code(c) = *s { Some(c) } else { None });
        let (mut ranges, mut codes) = (ranges.peekable(), codes.peekable());
        let in_range = ranges.peek().is_none() || ranges.any(|(from, to)| (from..=to).contains(&lot.acquired));
        let marked = codes.peek().is_none() || {
            let marks = self.txns.get(lot.txn).map_or(&[][..], |txn| &txn.codes);
            codes.any(|code| marks.contains(&code))
        };
        in_range && marked
    }
}

impl Candidate {
    fn new(source: Source, parcel: &Parcel, is_base: bool) -> Candidate {
        let (qty, basis, acquired, tied) = (parcel.qty, parcel.basis, parcel.acquired, parcel.tied);
        Candidate { source, qty, basis, acquired, tied, identity: identity(parcel, is_base) }
    }
}

/// The parcels of `holding` the selection admits, plain money first.
fn gather(holding: &Holding, is_base: bool, selection: &Selection, out: &mut Vec<Candidate>) {
    if holding.plain > Qty::ZERO && !selection.constrains() {
        // Plain money has no transaction or acquisition day of its own.
        let basis = if is_base { holding.plain } else { Qty::ZERO };
        let plain = Parcel { qty: holding.plain, basis, acquired: Day(i32::MIN), txn: Id::new(0), tied: None };
        out.push(Candidate::new(Source::Plain, &plain, is_base));
    }
    let lots = holding.lots.iter().enumerate().filter(|(_, lot)| selection.admits(lot));
    out.extend(lots.map(|(at, lot)| Candidate::new(Source::Lot(at), lot, is_base)));
}

/// 0: tied to an entity that permits this flow; 1: untied; 2: tied to one that
/// does not.
fn rank(tied: Option<Id<Entity>>, permits: &[(Id<Entity>, bool)]) -> u8 {
    match tied {
        None => 1,
        Some(entity) if permits.contains(&(entity, true)) => 0,
        Some(_) => 2,
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

fn allocate(group: &[Candidate], take: Qty, prorata: bool, pieces: &mut Vec<Piece>) {
    let total: Qty = group.iter().map(|c| c.qty).sum();
    if prorata {
        let mut shares = Shares::new(take, total);
        pieces.extend(group.iter().map(|c| piece(c, shares.take(c.qty))));
        return;
    }
    let mut left = take;
    for c in group {
        if left.is_zero() {
            break;
        }
        let qty = c.qty.min(left);
        pieces.push(piece(c, qty));
        left -= qty;
    }
}

fn piece(c: &Candidate, qty: Qty) -> Piece {
    let basis = match c.source {
        // Plain money's basis is its face, or nothing outside the base currency.
        Source::Plain if c.basis == c.qty => qty,
        Source::Plain => Qty::ZERO,
        Source::Lot(_) if qty == c.qty => c.basis,
        Source::Lot(_) => c.basis.share(qty, c.qty).expect("a part of a basis fits"),
    };
    Piece { source: c.source, qty, basis }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A lot bought in the transaction numbered by its day.
    fn lot(qty: i64, basis: i64, acquired: i32) -> Parcel {
        Parcel { qty: Qty(qty), basis: Qty(basis), acquired: Day(acquired), txn: Id::new(acquired as u32), tied: None }
    }

    fn holding(plain: i64, lots: Vec<Parcel>) -> Holding {
        Holding { place: Id::new(0), unit: Id::new(1), plain: Qty(plain), lots }
    }

    fn request(need: i64, policy: Option<Policy>, txns: &Arena<Txn>) -> Request<'_> {
        Request { need: Qty(need), is_base: false, selectors: &[], policy, txns, permits: &[] }
    }

    fn planned(holding: &Holding, need: i64, policy: Option<Policy>) -> Relief {
        let mut relief = Relief::default();
        plan(Some(holding), &request(need, policy, &Arena::new()), &mut relief);
        relief
    }

    fn taken(relief: &Relief) -> Vec<(Source, i64, i64)> {
        relief.pieces.iter().map(|p| (p.source, p.qty.0, p.basis.0)).collect()
    }

    #[test]
    fn prorata_parts_sum_exactly() {
        let held = holding(0, vec![lot(7, 100, 1), lot(11, 250, 2), lot(13, 333, 3)]);
        let relief = planned(&held, 10, Some(Policy::Prorata));
        assert_eq!(relief.pieces.iter().map(|p| p.qty.0).sum::<i64>(), 10);
        assert!(relief.pieces.iter().zip(&held.lots).all(|(p, lot)| p.qty <= lot.qty));
        // Relieving everything relieves every lot's basis exactly.
        let all = planned(&held, 31, Some(Policy::Prorata));
        assert_eq!(all.pieces.iter().map(|p| p.basis.0).sum::<i64>(), 683);
        assert_eq!(all.shortfall, Qty::ZERO);
    }

    #[test]
    fn apportioned_shares_carry_no_rounding_drift() {
        let mut shares = Shares::new(Qty(100), Qty(3));
        let parts: Vec<_> = (0..3).map(|_| shares.take(Qty(1)).0).collect();
        assert_eq!(parts, [33, 34, 33]);
        assert_eq!(parts.iter().sum::<i64>(), 100);
    }

    #[test]
    fn hifo_takes_the_highest_basis_per_unit_first() {
        let held = holding(0, vec![lot(10, 1_000, 1), lot(10, 3_000, 2), lot(10, 2_000, 3)]);
        let relief = planned(&held, 15, Some(Policy::Hifo));
        assert_eq!(taken(&relief), [(Source::Lot(1), 10, 3_000), (Source::Lot(2), 5, 1_000)]);
        let fifo = planned(&held, 15, Some(Policy::Fifo));
        assert_eq!(taken(&fifo), [(Source::Lot(0), 10, 1_000), (Source::Lot(1), 5, 1_500)]);
        let lifo = planned(&held, 15, Some(Policy::Lifo));
        assert_eq!(taken(&lifo), [(Source::Lot(2), 10, 2_000), (Source::Lot(1), 5, 1_500)]);
    }

    #[test]
    fn only_lots_that_differ_are_ambiguous_without_a_policy() {
        let same = holding(0, vec![lot(10, 1_000, 5), lot(4, 400, 5)]);
        assert!(planned(&same, 6, None).ambiguous.is_empty());
        let purchase = |txn| Parcel { txn: Id::new(txn), ..lot(10, 1_500, 5) };
        let differ = holding(0, vec![lot(10, 1_000, 5), purchase(99)]);
        let relief = planned(&differ, 6, None);
        assert_eq!(relief.ambiguous.len(), 2);
        assert_eq!(taken(&relief), [(Source::Lot(0), 6, 600)], "FIFO carries on");
        assert!(planned(&differ, 6, Some(Policy::Fifo)).ambiguous.is_empty());
        assert!(planned(&differ, 20, None).ambiguous.is_empty(), "taking everything leaves no choice");
        let days = holding(0, vec![lot(10, 1_000, 5), lot(10, 1_000, 6)]);
        assert_eq!(planned(&days, 6, None).ambiguous.len(), 2, "each purchase is its own lot outside the base");
    }

    #[test]
    fn base_money_is_interchangeable_when_tie_and_basis_per_unit_agree() {
        let base = |held: &Holding, need| {
            let mut relief = Relief::default();
            plan(Some(held), &Request { is_base: true, ..request(need, None, &Arena::new()) }, &mut relief);
            relief
        };
        let same_ratio = holding(0, vec![lot(100, 0, 1), lot(50, 0, 30)]);
        assert!(base(&same_ratio, 20).ambiguous.is_empty(), "zero-basis deferrals from any day are alike");
        let mixed = holding(700, vec![lot(300, 0, 1)]);
        assert_eq!(base(&mixed, 100).ambiguous.len(), 2, "after-tax plain money and pre-tax money differ");
    }

    #[test]
    fn prorata_over_plain_and_a_zero_basis_lot_splits_the_withdrawal() {
        let held = holding(6_300_00, vec![lot(2_200_00, 0, 3)]);
        let mut relief = Relief::default();
        let txns = Arena::new();
        let request = Request { is_base: true, policy: Some(Policy::Prorata), ..request(1_500_00, None, &txns) };
        plan(Some(&held), &request, &mut relief);
        assert_eq!(taken(&relief), [(Source::Plain, 1_111_76, 1_111_76), (Source::Lot(0), 388_24, 0)]);
    }

    #[test]
    fn a_sale_beyond_what_is_held_reports_the_shortfall() {
        let relief = planned(&holding(0, vec![lot(7, 700, 1)]), 10, None);
        assert_eq!(relief.shortfall, Qty(3));
        assert_eq!(taken(&relief), [(Source::Lot(0), 7, 700)]);
    }

    #[test]
    fn tied_parcels_go_first_only_when_their_laws_permit() {
        let grant = Id::new(4);
        let tied = Parcel { tied: Some(grant), ..lot(5, 5, 9) };
        let held = holding(0, vec![lot(10, 10, 1), tied]);
        let txns = Arena::new();
        let mut relief = Relief::default();
        let mut ask = |permits: &[(Id<Entity>, bool)]| {
            plan(Some(&held), &Request { permits, is_base: true, ..request(4, None, &txns) }, &mut relief);
            relief.pieces[0].source
        };
        assert_eq!(ask(&[(grant, true)]), Source::Lot(1));
        assert_eq!(ask(&[(grant, false)]), Source::Lot(0));
    }

    #[test]
    fn selectors_intersect_by_kind_and_union_within_one() {
        let held = holding(0, vec![lot(1, 1, 10), lot(2, 2, 20), lot(4, 4, 30)]);
        let txns = Arena::new();
        let pick = |selectors: &[Select]| admitted(&held, false, selectors, &txns).0;
        assert_eq!(pick(&[]), 7);
        assert_eq!(pick(&[Select::Range(Day(10), Day(20))]), 3);
        assert_eq!(pick(&[Select::Range(Day(10), Day(10)), Select::Range(Day(30), Day(30))]), 5);
        assert_eq!(pick(&[Select::Policy(Policy::Lifo)]), 7);
    }
}
