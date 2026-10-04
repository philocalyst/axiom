//! Relief held to a naive model: every policy, on seeded random landings and reliefs.
//!
//! The model keeps a holding as a plain amount and a list of live lots, oldest first, and nothing else: no cursor, no
//! heap, no sweep, no identity type. It says what a relief takes by listing the candidates, ordering them, and taking in
//! that order: by colour first (the spender's own, then permitted, then untied, then refused), then FIFO the oldest
//! first, LIFO the newest (plain money last), HIFO the dearest per unit (non-money plain first), `exact` the first claim
//! of the size asked and then the oldest, pro rata a rounded running share of every candidate of a colour. The real
//! [`Slot`] and the model are given the same landings, credits and reliefs and must agree on every slice that leaves,
//! the shortfall, and what is left. Lane U's ranking relief (K3e) is held to it; `docs/v5/measure/u/relief_mutants.py`
//! shows it fails on a wrong rank or a wrong take.

use axiom_core::sym::Interner;
use axiom_core::{Arena, Day, Days, Id, Qty, Run, Sym};
use axiom_model::{FlowCodes, Policy, RuntimeTxn, Select};

use crate::Parcel;
use crate::lots::{Held, NONE, Origin, Relief, Request, Slot};

/// The moment of every relief: when, and by what, plain money that leaves is said to have been acquired.
const NOW: i32 = 1_000;
const CASES: u64 = 3_000;

/// A seeded generator (splitmix64): the cases are the same on every run.
struct Seeded(u64);

impl Seeded {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number in `0..n`.
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    /// A number in `low..=high`.
    fn within(&mut self, low: i64, high: i64) -> i64 {
        low + self.below((high - low + 1) as u64) as i64
    }

    fn chance(&mut self, one_in: u64) -> bool {
        self.below(one_in) == 0
    }
}

/// A lot as the model holds it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Lot {
    qty: i64,
    basis: i64,
    day: i32,
    txn: u32,
    tied: Option<u32>,
    code: Option<usize>,
}

/// What left, as the model says it and as the real slice is read back.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Left {
    qty: i64,
    basis: i64,
    day: i32,
    txn: u32,
    tied: Option<u32>,
    lot: bool,
}

/// What may be asked of one relief.
#[derive(Clone, Debug)]
struct Ask {
    need: i64,
    money: bool,
    policy: Option<Policy>,
    picks: Vec<Pick>,
    spender: Option<u32>,
    permits: Vec<(u32, bool)>,
}

/// One selector, as the model reads it.
#[derive(Clone, Copy, Debug)]
enum Pick {
    Days(i32, i32),
    Code(usize),
    Txn(u32),
    Policy(Policy),
}

/// The order relief takes colours in.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Colour {
    Own,
    Permitted,
    Free,
    Refused,
}

/// A candidate: plain money (`at` none) or a lot by its position.
#[derive(Clone, Copy, Debug)]
struct Candidate {
    at: Option<usize>,
    qty: i64,
    basis: i64,
    colour: Colour,
    txn: u32,
}

#[derive(Clone, Default, Debug)]
struct Model {
    plain: i64,
    lots: Vec<Lot>,
}

impl Ask {
    fn constrains(&self) -> bool {
        self.picks.iter().any(|pick| !matches!(pick, Pick::Policy(_)))
    }

    fn policy(&self) -> Option<Policy> {
        let picked = self.picks.iter().find_map(|pick| if let Pick::Policy(p) = pick { Some(*p) } else { None });
        picked.or(self.policy)
    }

    fn colour(&self, tied: Option<u32>) -> Colour {
        match tied {
            None => Colour::Free,
            Some(entity) if Some(entity) == self.spender => Colour::Own,
            Some(entity) if self.permits.contains(&(entity, true)) => Colour::Permitted,
            Some(_) => Colour::Refused,
        }
    }

    /// Every range must not be missed, every code carried, every transaction the one: one of each kind that is given.
    fn admits(&self, lot: &Lot) -> bool {
        let ranges: Vec<_> =
            self.picks.iter().filter_map(|p| if let Pick::Days(a, b) = p { Some((*a, *b)) } else { None }).collect();
        let codes: Vec<_> =
            self.picks.iter().filter_map(|p| if let Pick::Code(c) = p { Some(*c) } else { None }).collect();
        let txns: Vec<_> =
            self.picks.iter().filter_map(|p| if let Pick::Txn(t) = p { Some(*t) } else { None }).collect();
        (ranges.is_empty() || ranges.iter().any(|&(a, b)| a <= lot.day && lot.day <= b))
            && (codes.is_empty() || codes.iter().any(|&c| lot.code == Some(c)))
            && (txns.is_empty() || txns.contains(&lot.txn))
    }
}

/// `qty / whole` of `basis`, as the engine rounds a part of a basis.
fn part(basis: i64, qty: i64, whole: i64) -> i64 {
    Qty(basis).share(Qty(qty), Qty(whole)).expect("a part of a basis fits").0
}

impl Model {
    fn total(&self) -> i64 {
        self.plain + self.lots.iter().map(|lot| lot.qty).sum::<i64>()
    }

    /// Money at its face and tied to nothing is plain; anything else merges into the lot it cannot be told from (for
    /// money: the same tie, code and basis per unit; otherwise the same day, transaction, tie and code), or takes its
    /// place after every lot of its day or earlier.
    fn land(&mut self, lot: Lot, money: bool) {
        if money && lot.tied.is_none() && lot.basis == lot.qty {
            self.plain += lot.qty;
            return;
        }
        let same = |held: &&mut Lot| {
            held.tied == lot.tied
                && held.code == lot.code
                && if money {
                    held.basis as i128 * lot.qty as i128 == lot.basis as i128 * held.qty as i128
                } else {
                    held.day == lot.day && held.txn == lot.txn
                }
        };
        if let Some(held) = self.lots.iter_mut().find(same) {
            held.qty += lot.qty;
            held.basis += lot.basis;
            return;
        }
        let at = self.lots.partition_point(|held| held.day <= lot.day);
        self.lots.insert(at, lot);
    }

    fn candidates(&self, ask: &Ask) -> Vec<Candidate> {
        let mut found = Vec::new();
        if self.plain > 0 && !ask.constrains() {
            let basis = if ask.money { self.plain } else { 0 };
            found.push(Candidate { at: None, qty: self.plain, basis, colour: Colour::Free, txn: 0 });
        }
        for (at, lot) in self.lots.iter().enumerate().filter(|(_, lot)| ask.admits(lot)) {
            let colour = ask.colour(lot.tied);
            found.push(Candidate { at: Some(at), qty: lot.qty, basis: lot.basis, colour, txn: lot.txn });
        }
        found.retain(|c| c.colour != Colour::Refused || ask.spender.is_none());
        found
    }

    /// The candidates of one colour in the order the policy takes them.
    fn ordered(&self, ask: &Ask, mut group: Vec<Candidate>) -> Vec<Candidate> {
        let dearer =
            |a: &Candidate, b: &Candidate| (b.basis as i128 * a.qty as i128).cmp(&(a.basis as i128 * b.qty as i128));
        match ask.policy() {
            Some(Policy::Lifo) => group.reverse(),
            Some(Policy::Hifo) if ask.money => group.sort_by(dearer),
            Some(Policy::Hifo) => {
                let plain_first = |c: &Candidate| c.at.is_some();
                group.sort_by(|a, b| plain_first(a).cmp(&plain_first(b)).then(dearer(a, b)));
            }
            Some(Policy::Exact) if ask.constrains() => {
                // Each candidate's claim is everything of its transaction: the claims of the size asked go first.
                let claim = |c: &Candidate| {
                    group
                        .iter()
                        .filter(|o| o.txn == c.txn && o.at.is_some() == c.at.is_some())
                        .map(|o| o.qty)
                        .sum::<i64>()
                };
                let fits: Vec<bool> = group.iter().map(|c| claim(c) == ask.need).collect();
                let mut keyed: Vec<_> = group.into_iter().zip(fits).collect();
                keyed.sort_by_key(|(_, fits)| !fits);
                group = keyed.into_iter().map(|(c, _)| c).collect();
            }
            Some(Policy::Exact) => {
                // The first run of one transaction's lots that holds exactly what is asked, then plain money, then FIFO.
                let lots: Vec<Candidate> = group.iter().copied().filter(|c| c.at.is_some()).collect();
                let mut run = Vec::new();
                let mut start = 0;
                while start < lots.len() {
                    let txn = self.lots[lots[start].at.unwrap()].txn;
                    let end = start + lots[start..].iter().take_while(|c| c.txn == txn).count();
                    if lots[start..end].iter().map(|c| c.qty).sum::<i64>() == ask.need {
                        run = lots[start..end].to_vec();
                        break;
                    }
                    start = end;
                }
                let rest = group.iter().copied().filter(|c| !run.iter().any(|r| r.at == c.at));
                group = run.iter().copied().chain(rest).collect();
            }
            _ => {}
        }
        group
    }

    /// What leaves, as (candidate, quantity), and the shortfall.
    fn plan(&self, ask: &Ask) -> (Vec<(Candidate, i64)>, i64) {
        let mut candidates = self.candidates(ask);
        candidates.sort_by_key(|c| c.colour);
        let mut plan = Vec::new();
        let mut left = ask.need;
        let mut rest = candidates.as_slice();
        while left > 0 && !rest.is_empty() {
            let split = rest.iter().take_while(|c| c.colour == rest[0].colour).count();
            let (group, tail) = rest.split_at(split);
            rest = tail;
            let total: i64 = group.iter().map(|c| c.qty).sum();
            let take = left.min(total);
            if ask.policy() == Some(Policy::Prorata) {
                let (mut seen, mut paid) = (0, 0);
                for c in group {
                    seen += c.qty;
                    let owed = if seen == total { take } else { part(take, seen, total) };
                    plan.push((*c, owed - paid));
                    paid = owed;
                }
            } else {
                let mut wanted = take;
                for c in self.ordered(ask, group.to_vec()) {
                    let qty = c.qty.min(wanted);
                    plan.push((c, qty));
                    wanted -= qty;
                    if wanted == 0 {
                        break;
                    }
                }
            }
            left -= take;
        }
        (plan, left)
    }

    fn relieve(&mut self, ask: &Ask) -> (Vec<Left>, i64) {
        let (plan, shortfall) = self.plan(ask);
        let mut out = Vec::new();
        for (c, qty) in plan {
            match c.at {
                None => {
                    self.plain -= qty;
                    let basis = if ask.money { qty } else { 0 };
                    out.push(Left { qty, basis, day: NOW, txn: 0, tied: None, lot: false });
                }
                Some(at) => {
                    let lot = &mut self.lots[at];
                    let basis = if qty == lot.qty { lot.basis } else { part(lot.basis, qty, lot.qty) };
                    out.push(Left { qty, basis, day: lot.day, txn: lot.txn, tied: lot.tied, lot: true });
                    lot.qty -= qty;
                    lot.basis -= basis;
                }
            }
        }
        self.lots.retain(|lot| lot.qty != 0);
        self.plain -= shortfall;
        (out, shortfall)
    }

    /// The transactions whose lots are held, and what each holds: a size `exact` can be asked for.
    fn claims(&self) -> Vec<i64> {
        let mut sizes: Vec<(u32, i64)> = Vec::new();
        for lot in &self.lots {
            match sizes.iter_mut().find(|(txn, _)| *txn == lot.txn) {
                Some((_, held)) => *held += lot.qty,
                None => sizes.push((lot.txn, lot.qty)),
            }
        }
        sizes.into_iter().map(|(_, held)| held).collect()
    }
}

/// The real holding, and what it is given in the model's terms.
struct Real {
    slot: Slot,
    pool: Arena<Sym>,
}

impl Real {
    fn new() -> Real {
        let mut names = Interner::default();
        let mut pool = Arena::new();
        for name in ["first", "second"] {
            pool.push(names.intern(name));
        }
        Real { slot: Slot::new(Id::new(0), Id::new(1), NONE), pool }
    }

    fn txn(txn: u32) -> RuntimeTxn {
        RuntimeTxn::journal(Id::new(txn)).expect("a journal transaction")
    }

    fn land(&mut self, lot: Lot, money: bool) {
        let empty = Run::new(Id::new(0), 0);
        let header = lot.code.map_or(empty, |code| Run::new(Id::new(code as u32), 1));
        let parcel = Parcel {
            qty: Qty(lot.qty),
            basis: Qty(lot.basis),
            acquired: Day(lot.day),
            held_since: Day(lot.day),
            wash_matched: false,
            txn: Real::txn(lot.txn),
            part: None,
            codes: FlowCodes { header, local: empty },
            tied: lot.tied.map(Id::new),
        };
        self.slot.land(parcel, if money { Held::Money } else { Held::Lots }, &self.pool);
    }

    fn relieve(&mut self, ask: &Ask) -> (Vec<Left>, i64) {
        let selectors: Vec<Select> = ask
            .picks
            .iter()
            .map(|pick| match *pick {
                Pick::Days(a, b) => Select::Range(Days::new(Day(a), Day(b)).expect("a range")),
                Pick::Code(code) => Select::Code(self.pool[Id::new(code as u32)]),
                Pick::Txn(txn) => Select::Txn(Id::new(txn)),
                Pick::Policy(policy) => Select::Policy(policy),
            })
            .collect();
        let permits: Vec<_> = ask.permits.iter().map(|&(entity, permit)| (Id::new(entity), permit)).collect();
        let request = Request {
            held: if ask.money { Held::Money } else { Held::Lots },
            selectors: &selectors,
            permits: &permits,
            spender: ask.spender.map(Id::new),
            ..Request::of(Qty(ask.need), ask.policy, &self.pool, (Day(NOW), Real::txn(0)))
        };
        let mut relief = Relief::default();
        self.slot.relieve(&request, &mut relief);
        let left = relief.slices.iter().map(|s| Left {
            qty: s.qty.0,
            basis: s.basis.0,
            day: s.acquired.0,
            txn: s.txn.source_txn().map_or(u32::MAX, |txn| txn.index() as u32),
            tied: s.tied.map(|entity| entity.index() as u32),
            lot: s.origin == Origin::Lot,
        });
        (left.collect(), relief.shortfall.0)
    }

    /// What is held, in the model's terms.
    fn held(&self) -> Model {
        let code = |codes: FlowCodes| (codes.header.len() == 1).then(|| codes.header.start().index());
        let lots = self.slot.holding.lots.iter().filter(|lot| !lot.qty.is_zero()).map(|lot| Lot {
            qty: lot.qty.0,
            basis: lot.basis.0,
            day: lot.acquired.0,
            txn: lot.txn.source_txn().map_or(u32::MAX, |txn| txn.index() as u32),
            tied: lot.tied.map(|entity| entity.index() as u32),
            code: code(lot.codes),
        });
        Model { plain: self.slot.holding.plain.0, lots: lots.collect() }
    }
}

/// A parcel of transaction `txn`, landed on `day`.
fn parcel(rng: &mut Seeded, money: bool, txn: u32, day: i32) -> Lot {
    let qty = rng.within(1, 40);
    let basis = if money { [qty, 0, qty / 2, qty * 2][rng.below(4) as usize] } else { rng.within(0, 3_000) };
    let tied = rng.chance(5).then(|| 1 + rng.below(2) as u32);
    let code = rng.chance(3).then(|| rng.below(2) as usize);
    Lot { qty, basis, day, txn, tied, code }
}

fn ask(rng: &mut Seeded, model: &Model, money: bool, txns: u32) -> Ask {
    let policies: &[Option<Policy>] = if money {
        &[None, Some(Policy::Fifo), Some(Policy::Lifo), Some(Policy::Hifo), Some(Policy::Prorata)]
    } else {
        &[None, Some(Policy::Fifo), Some(Policy::Lifo), Some(Policy::Hifo), Some(Policy::Prorata), Some(Policy::Exact)]
    };
    let policy = policies[rng.below(policies.len() as u64) as usize];
    let held = model.total().max(1);
    let claims = model.claims();
    let need = if policy == Some(Policy::Exact) && !claims.is_empty() && rng.chance(2) {
        claims[rng.below(claims.len() as u64) as usize]
    } else if rng.chance(8) {
        held + rng.within(1, 20)
    } else {
        rng.within(1, held)
    };
    let mut picks = Vec::new();
    if rng.chance(5) {
        picks.push(match rng.below(4) {
            0 => {
                let first = rng.within(0, 20) as i32;
                Pick::Days(first, first + rng.within(0, 10) as i32)
            }
            1 => Pick::Code(rng.below(2) as usize),
            2 => Pick::Txn(1 + rng.below(u64::from(txns.max(1))) as u32),
            _ => Pick::Policy([Policy::Fifo, Policy::Lifo, Policy::Hifo, Policy::Prorata][rng.below(4) as usize]),
        });
    }
    let tied = model.lots.iter().any(|lot| lot.tied.is_some());
    let spender = (tied && rng.chance(3)).then(|| 1 + rng.below(2) as u32);
    let permits = if tied { vec![(1, rng.chance(2)), (2, rng.chance(2))] } else { Vec::new() };
    Ask { need: need.max(1), money, policy, picks, spender, permits }
}

#[test]
fn relief_agrees_with_the_model_on_random_holdings_and_requests() {
    let mut reliefs = 0;
    for case in 0..CASES {
        let mut rng = Seeded(case);
        let money = rng.chance(4);
        let (mut model, mut real) = (Model::default(), Real::new());
        let mut txns = 0;
        for step in 0..rng.within(10, 40) {
            match rng.below(10) {
                0..=4 => {
                    txns += 1;
                    let day = rng.within(0, 20) as i32;
                    for _ in 0..rng.within(1, 3) {
                        let lot = parcel(&mut rng, money, txns, day);
                        model.land(lot, money);
                        real.land(lot, money);
                    }
                }
                5 => {
                    let qty = rng.within(1, 30);
                    model.plain += qty;
                    real.slot.credit(Qty(qty));
                }
                _ => {
                    let asked = ask(&mut rng, &model, money, txns);
                    let before = model.clone();
                    let expected = model.relieve(&asked);
                    let got = real.relieve(&asked);
                    reliefs += 1;
                    assert_eq!(got, expected, "case {case} step {step}: {asked:?}\nheld before: {before:?}");
                }
            }
            let held = real.held();
            assert_eq!((held.plain, &held.lots), (model.plain, &model.lots), "case {case} step {step}: what is held");
            assert_eq!(real.slot.qty.0, model.total(), "case {case} step {step}: the slot's own total");
        }
    }
    assert!(reliefs > 10_000, "the cases exercise relief: {reliefs}");
}
