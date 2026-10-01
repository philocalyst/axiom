//! Running sums: window totals per subject, and tallies.
//!
//! Both are kept in the base currency, and both follow a flow's *recognition*
//! (the days it belongs to), not the day it moved. A flow recognized over a
//! range counts in each window its days touch, in proportion to the days: the
//! share falling in the current window at once, and the rest as the fold
//! reaches later windows. Windows roll: each remembers the days it covers, and
//! a read from another window finds only what was recognized into it. The fold
//! visits days in order, so nothing is ever recomputed.

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::hash::{Hash, Hasher};

use axiom_core::calendar;
use axiom_core::{Day, Days, Groups, Id, Map, Period, Qty, Set, Sym, spread};
use axiom_model::{Book, Dir, Entity, Fault, Place, Purpose, Subject, Window};

use crate::facts::{LawFacts, TotalsRead};
use crate::scope::containing;
use crate::state::unordered;

/// `amount` cut by the calendar years `over` touches: the first day of each
/// year's part, and its share.
pub(crate) fn by_year(amount: Qty, over: Days) -> impl Iterator<Item = (Day, Qty)> {
    // A flow that belongs to one day belongs to one year: no calendar to consult.
    let one_day = over.single().map(|day| (day, amount));
    let years = one_day.map_or_else(
        || Some(calendar::Window::covering(Period::Year, over)),
        |_| None,
    );
    let cut = years.into_iter().flatten().map(move |year| {
        let part = year.days();
        (part.first().max(over.first()), spread(amount, over, part))
    });
    one_day.into_iter().chain(cut)
}

/// Value that entered and left, over one window.
#[derive(Clone, Copy, Default, Hash)]
struct Flowed {
    incoming: Qty,
    outgoing: Qty,
}

impl Flowed {
    fn side(&mut self, dir: Dir) -> &mut Qty {
        match dir {
            Dir::In => &mut self.incoming,
            Dir::Out => &mut self.outgoing,
        }
    }
}

/// One window's sums, and the days they belong to.
#[derive(Clone, Copy, Hash)]
struct Rolling {
    days: Days,
    flowed: Flowed,
}

impl Rolling {
    /// The window before the first: no day a flow can be on is in it, so the
    /// first flow rolls the subject into a real one.
    const NEVER: Rolling = Rolling {
        days: Days::on(Day::MIN),
        flowed: Flowed {
            incoming: Qty::ZERO,
            outgoing: Qty::ZERO,
        },
    };

    /// The window `days`, holding what earlier flows recognized into it.
    fn of(days: Days, ahead: &[Accrual]) -> Rolling {
        let mut flowed = Flowed::default();
        for accrual in ahead {
            *flowed.side(accrual.dir) += spread(accrual.amount, accrual.over, days);
        }
        Rolling { days, flowed }
    }
}

/// Value recognized over days that had not come when its flow was counted.
#[derive(Clone, Copy, Hash)]
struct Accrual {
    dir: Dir,
    amount: Qty,
    over: Days,
}

/// One subject's windows. `closed` is the year that just ended, kept for the
/// laws that close it late.
#[derive(Clone, Hash)]
struct Windows {
    month: Rolling,
    year: Rolling,
    closed: Rolling,
    ever: Flowed,
    ahead: Vec<Accrual>,
    /// The subject waits in [`Reaching`] for the next month, which value recognized ahead of time has reached.
    reaching: bool,
}

impl Windows {
    const NONE: Windows = Windows {
        month: Rolling::NEVER,
        year: Rolling::NEVER,
        closed: Rolling::NEVER,
        ever: Flowed {
            incoming: Qty::ZERO,
            outgoing: Qty::ZERO,
        },
        ahead: Vec::new(),
        reaching: false,
    };

    /// Moves the current windows on to the ones containing `day`. Once a month
    /// or year, and kept out of line so that its calendar arithmetic is not
    /// worked out ahead of time on every flow.
    #[cold]
    #[inline(never)]
    fn roll(&mut self, day: Day) {
        if !self.month.days.contains(day) {
            self.month = Rolling::of(Window::Month.around(day), &self.ahead);
        }
        if !self.year.days.contains(day) {
            let year = Rolling::of(Window::Year.around(day), &self.ahead);
            let old = std::mem::replace(&mut self.year, year);
            self.closed = if old.days.last().add_days(1) == year.days.first() {
                old
            } else {
                Rolling::NEVER
            };
        }
        self.ahead
            .retain(|accrual| accrual.over.last() >= self.month.days.first());
    }

    /// Counts a flow. Returns whether some of it was recognized after the
    /// month it moved in, and so is now waiting for the windows ahead.
    fn add(&mut self, day: Day, dir: Dir, amount: Qty, over: Days) -> bool {
        *self.ever.side(dir) += amount;
        if !self.month.days.contains(day) || !self.year.days.contains(day) {
            self.roll(day);
        }
        // Counted on the day it moved, in the windows that day is in: nearly every flow.
        if over.single() == Some(day) {
            *self.month.flowed.side(dir) += amount;
            *self.year.flowed.side(dir) += amount;
            return false;
        }
        for rolling in [&mut self.month, &mut self.year, &mut self.closed] {
            *rolling.flowed.side(dir) += spread(amount, over, rolling.days);
        }
        let ahead = over.last() > self.month.days.last();
        if ahead {
            self.ahead.push(Accrual { dir, amount, over });
        }
        ahead
    }

    /// What was recognized into the window containing `day`, which may be
    /// the year that just closed.
    fn read(&self, dir: Dir, window: Window, day: Day) -> Qty {
        let rolling = match window {
            Window::Ever => return *{ self.ever }.side(dir),
            Window::Month => &self.month,
            Window::Year if self.closed.days.contains(day) => &self.closed,
            Window::Year => &self.year,
        };
        // A window nothing was counted into yet holds only what was recognized ahead of it.
        let mut flowed = if rolling.days.contains(day) {
            rolling.flowed
        } else {
            Rolling::of(window.around(day), &self.ahead).flowed
        };
        *flowed.side(dir)
    }
}

/// The subjects some law reads flow totals of, and for each place the ones it
/// lies within: fixed by the book's laws, so worked out once, in the plan. A
/// book whose laws never read `total(…)` watches nothing, and a subject nobody
/// reads costs nothing.
pub(crate) struct Watch {
    /// The watched subjects each place lies within: the only ones a flow at
    /// that place can enter or leave.
    through: Groups<Place, Subject>,
    /// Sparse index into `Totals::windows`; most subjects have no law that
    /// reads a flow total and need no rolling state.
    slots: Box<[u32]>,
    subjects: Box<[Subject]>,
    /// The ancestors of flow purposes that some law reads.
    purpose_through: Option<Groups<Purpose, Id<Purpose>>>,
    /// The budget-relevant purpose ancestors whose recognized history is
    /// retained for carry calculations.
    budget_through: Option<Groups<Purpose, Id<Purpose>>>,
    /// Slots whose history a computed budget `total(in|out, …)` may read.
    /// Empty unless a budget formula has a non-purpose total expression.
    budget_total_slots: Box<[bool]>,
    places: usize,
    entities: usize,
    assets: usize,
}

impl Watch {
    pub fn of(book: &Book, laws: &[LawFacts]) -> Watch {
        let (places, entities, assets, contracts) = (
            book.places.len(),
            book.entities.len(),
            book.assets.len(),
            book.contracts.len(),
        );
        let budget_reads_total = book.laws.values().any(|law| {
            law.budget.is_some()
                && law.nodes.values().any(|node| {
                    matches!(node.op, axiom_model::Op::Call(axiom_model::Func::Total(..), _))
                })
        });
        let mut watched = vec![false; places + entities + assets + contracts];
        for rule in book.rules.all() {
            match laws[rule.law.index()].totals {
                TotalsRead::Nothing => {}
                TotalsRead::Subject => {
                    watched[slot(places, entities, assets, rule.subject)] = true;
                }
                // A kind-wide total reads every place of that kind.
                TotalsRead::Kind => watched[..places].fill(true),
            }
        }
        // Purpose rules are evaluated once for each flow owner at run time;
        // their stored subject is only a placeholder. Reserve owner slots for
        // each such read so a non-placeholder owner's total is never missing.
        for rule in book.rules.purposes.values() {
            match laws[rule.law.index()].totals {
                TotalsRead::Nothing => {}
                TotalsRead::Subject => watched[places..places + entities].fill(true),
                TotalsRead::Kind => watched[..places].fill(true),
            }
        }
        for rule in book.rules.about.values() {
            match laws[rule.law.index()].totals {
                TotalsRead::Nothing => {}
                TotalsRead::Subject => {
                    watched[slot(places, entities, assets, rule.subject)] = true;
                }
                TotalsRead::Kind => watched[..places].fill(true),
            }
        }
        let mut slots = vec![u32::MAX; watched.len()];
        let mut subjects = Vec::new();
        for (at, &yes) in watched.iter().enumerate() {
            if yes {
                slots[at] = subjects.len() as u32;
                subjects.push(subject_at(places, entities, assets, at));
            }
        }
        let budget_total_slots = subjects
            .iter()
            .map(|subject| budget_reads_total && matches!(subject, Subject::Place(_) | Subject::Entity(_)))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let mut within: Vec<_> = (0..places as u32)
            .map(Id::new)
            .flat_map(|place| containing(book, place).map(move |subject| (place, subject)))
            .filter(|&(_, subject)| watched[slot(places, entities, assets, subject)])
            .collect();
        // An asset's totals follow its asset place and its parts. Keep the
        // relation in the plan so posting a flow never scans the asset table.
        for (asset, _) in book.assets.iter() {
            let mut part = Some(asset);
            while let Some(current) = part {
                let subject = Subject::Asset(current);
                if watched[slot(places, entities, assets, subject)] {
                    within.extend(
                        book.places
                            .subtree(book.assets[asset].place)
                            .map(|place| (place, subject)),
                    );
                }
                part = book.assets[current].part_of.map(|part| part.value);
            }
        }
        within.sort_unstable_by_key(|&(place, subject)| {
            (place, subject_key(places, entities, assets, subject))
        });
        within.dedup();
        let through = Groups::build(places, within.iter().copied());
        drop(within);
        let mut purpose_reads = Set::default();
        let mut budget_purposes = Set::default();
        for (_, budget) in book.budgets.iter() {
            budget_purposes.insert(budget.purpose);
            for (_, terms) in budget.terms.within(Days::ALWAYS) {
                if let axiom_model::Limit::Share { of, .. } = terms.limit {
                    budget_purposes.insert(of);
                }
            }
        }
        purpose_reads.extend(budget_purposes.iter().copied());
        for law in book.laws.values() {
            for node in law.nodes.values() {
                let axiom_model::Op::Call(axiom_model::Func::PurposeTotal { purpose, .. }, _) =
                    &node.op
                else {
                    continue;
                };
                let purpose = purpose.or_else(|| match law.owner {
                    axiom_model::Owner::Purpose(purpose) => Some(purpose),
                    _ => None,
                });
                if let Some(purpose) = purpose {
                    purpose_reads.insert(purpose);
                    if law.budget.is_some() {
                        budget_purposes.insert(purpose);
                    }
                }
            }
        }
        let purpose_through = if purpose_reads.is_empty() {
            None
        } else {
            let within = book.purposes.ids().flat_map(|actual| {
                book.purposes
                    .lineage(actual)
                    .filter(|ancestor| purpose_reads.contains(ancestor))
                    .map(move |ancestor| (actual, ancestor))
            });
            Some(Groups::build(book.purposes.len(), within))
        };
        let budget_through = if budget_purposes.is_empty() {
            None
        } else {
            let within = book.purposes.ids().flat_map(|actual| {
                book.purposes
                    .lineage(actual)
                    .filter(|ancestor| budget_purposes.contains(ancestor))
                    .map(move |ancestor| (actual, ancestor))
            });
            Some(Groups::build(book.purposes.len(), within))
        };
        Watch {
            through,
            slots: slots.into(),
            subjects: subjects.into(),
            purpose_through,
            budget_through,
            budget_total_slots,
            places,
            entities,
            assets,
        }
    }

    fn slot(&self, subject: Subject) -> Option<usize> {
        let slot = *self
            .slots
            .get(slot(self.places, self.entities, self.assets, subject))?;
        (slot != u32::MAX).then_some(slot as usize)
    }

    pub(crate) fn subjects(&self) -> &[Subject] {
        &self.subjects
    }

    fn stores_budget_total(&self, at: usize) -> bool {
        self.budget_total_slots.get(at).copied().unwrap_or(false)
    }

    pub(crate) fn reads_purpose(&self, purpose: Id<Purpose>) -> bool {
        self.purpose_through
            .as_ref()
            .is_some_and(|through| !through[purpose].is_empty())
    }

    /// The watched subjects that contain `here` but not `there`: a flow from
    /// `here` to `there` leaves them, and one from `there` to `here` enters them.
    fn crossed(&self, here: Id<Place>, there: Id<Place>) -> impl Iterator<Item = Subject> + '_ {
        let beyond = &self.through[there];
        self.through[here]
            .iter()
            .copied()
            .filter(move |subject| !beyond.contains(subject))
    }

    /// Whether a flow from `from` to `to` leaves, and whether it enters, any
    /// subject a law reads: only then is its value worth computing.
    pub fn sides(&self, from: Id<Place>, to: Id<Place>) -> (bool, bool) {
        (
            self.crossed(from, to).next().is_some(),
            self.crossed(to, from).next().is_some(),
        )
    }
}

/// Running flow totals for the subjects a [`Watch`] names.
#[derive(Clone)]
pub(crate) struct Totals {
    windows: Vec<Windows>,
    reaching: Reaching,
    purpose: Map<(Id<Entity>, Id<Purpose>), Windows>,
    /// Recognition facts needed only by budgets that carry room or overspend
    /// across calendar windows. Ordinary purpose totals remain rolling-only.
    budget_history: Map<(Id<Entity>, Id<Purpose>), History>,
    budget_total_history: Map<(Subject, Dir), History>,
}

/// Sparse recognition history. Ordinary single-day facts arrive in journal
/// order and are folded into compact block-prefix summaries. Ranged and
/// out-of-order facts stay on a retained slow path; neither requires a
/// per-flow scratch Vec.
const HISTORY_BLOCK: usize = 64;

#[derive(Clone, Default, Hash)]
struct History {
    days: Vec<DayFact>,
    blocks: Vec<BlockPrefix>,
    slow: Vec<RecognitionFact>,
    overflowed: bool,
}

#[derive(Clone, Copy, Hash)]
struct DayFact {
    day: Day,
    incoming: i64,
    outgoing: i64,
}

#[derive(Clone, Copy, Hash)]
struct BlockPrefix {
    incoming: i128,
    outgoing: i128,
}

#[derive(Clone, Copy, Hash)]
struct RecognitionFact {
    over: Days,
    dir: Dir,
    amount: Qty,
}

impl History {
    fn record(&mut self, over: Days, dir: Dir, amount: Qty) {
        let Some(day) = over.single() else {
            self.slow.push(RecognitionFact { over, dir, amount });
            return;
        };
        let Some(last_index) = self.days.len().checked_sub(1) else {
            let mut fact = DayFact { day, incoming: 0, outgoing: 0 };
            *fact.side_mut(dir) = amount.0;
            self.days.push(fact);
            return;
        };
        let last_day = self.days[last_index].day;
        if day < last_day {
            self.slow.push(RecognitionFact { over, dir, amount });
            return;
        }
        if day == last_day {
            let Some(next) = self.days[last_index].value(dir).checked_add(amount.0) else {
                self.slow.push(RecognitionFact { over, dir, amount });
                return;
            };
            *self.days[last_index].side_mut(dir) = next;
            if (last_index + 1) % HISTORY_BLOCK == 0 {
                let Some(prefix) = self.blocks.last_mut() else {
                    self.overflowed = true;
                    return;
                };
                let Some(next) = prefix.value(dir).checked_add(i128::from(amount.0)) else {
                    self.overflowed = true;
                    return;
                };
                *prefix.side_mut(dir) = next;
            }
            return;
        }

        let mut fact = DayFact { day, incoming: 0, outgoing: 0 };
        *fact.side_mut(dir) = amount.0;
        self.days.push(fact);
        if self.days.len() % HISTORY_BLOCK == 0 {
            let previous = self.blocks.last().copied().unwrap_or(BlockPrefix { incoming: 0, outgoing: 0 });
            let first = self.days.len() - HISTORY_BLOCK;
            let mut block = BlockPrefix { incoming: 0, outgoing: 0 };
            for fact in &self.days[first..] {
                let Some(incoming) = block.incoming.checked_add(i128::from(fact.incoming)) else {
                    self.overflowed = true;
                    return;
                };
                let Some(outgoing) = block.outgoing.checked_add(i128::from(fact.outgoing)) else {
                    self.overflowed = true;
                    return;
                };
                block.incoming = incoming;
                block.outgoing = outgoing;
            }
            let Some(incoming) = previous.incoming.checked_add(block.incoming) else {
                self.overflowed = true;
                return;
            };
            let Some(outgoing) = previous.outgoing.checked_add(block.outgoing) else {
                self.overflowed = true;
                return;
            };
            self.blocks.push(BlockPrefix { incoming, outgoing });
        }
    }

    /// Prefix through the first `count` chronological day buckets. A fixed
    /// block edge scan keeps retained storage near one fact per date while
    /// range reads remain O(log n) in history length.
    fn prefix(&self, count: usize) -> Result<(i128, i128), Fault> {
        let complete = count / HISTORY_BLOCK;
        let mut sums = complete
            .checked_sub(1)
            .and_then(|index| self.blocks.get(index))
            .map_or((0, 0), |prefix| (prefix.incoming, prefix.outgoing));
        let edge_start = complete * HISTORY_BLOCK;
        for fact in &self.days[edge_start..count] {
            sums.0 = sums.0.checked_add(i128::from(fact.incoming)).ok_or(Fault::Overflow)?;
            sums.1 = sums.1.checked_add(i128::from(fact.outgoing)).ok_or(Fault::Overflow)?;
        }
        Ok(sums)
    }

    fn read(&self, span: Days) -> Result<(Qty, Qty), Fault> {
        if self.overflowed {
            return Err(Fault::Overflow);
        }
        let start = self.days.partition_point(|fact| fact.day < span.first());
        let end = self.days.partition_point(|fact| fact.day <= span.last());
        let upper = self.prefix(end)?;
        let lower = self.prefix(start)?;
        let mut incoming = upper.0.checked_sub(lower.0).ok_or(Fault::Overflow)?;
        let mut outgoing = upper.1.checked_sub(lower.1).ok_or(Fault::Overflow)?;
        for fact in &self.slow {
            let Some(overlap) = fact.over.intersect(span) else { continue };
            let amount = i128::from(spread(fact.amount, fact.over, overlap).0);
            let total = match fact.dir {
                Dir::In => &mut incoming,
                Dir::Out => &mut outgoing,
            };
            *total = total.checked_add(amount).ok_or(Fault::Overflow)?;
        }
        Ok((qty(incoming)?, qty(outgoing)?))
    }
}

fn qty(value: i128) -> Result<Qty, Fault> {
    i64::try_from(value).map(Qty).map_err(|_| Fault::Overflow)
}

impl DayFact {
    fn value(&self, dir: Dir) -> &i64 {
        match dir {
            Dir::In => &self.incoming,
            Dir::Out => &self.outgoing,
        }
    }

    fn side_mut(&mut self, dir: Dir) -> &mut i64 {
        match dir {
            Dir::In => &mut self.incoming,
            Dir::Out => &mut self.outgoing,
        }
    }
}

impl BlockPrefix {
    fn value(&self, dir: Dir) -> &i128 {
        match dir {
            Dir::In => &self.incoming,
            Dir::Out => &self.outgoing,
        }
    }

    fn side_mut(&mut self, dir: Dir) -> &mut i128 {
        match dir {
            Dir::In => &mut self.incoming,
            Dir::Out => &mut self.outgoing,
        }
    }
}

/// What the running totals hold: the windows, which are all the future reads.
impl Hash for Totals {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.windows.hash(state);
        unordered(self.purpose.iter()).hash(state);
        unordered(self.budget_history.iter()).hash(state);
        unordered(self.budget_total_history.iter()).hash(state);
    }
}

/// The first day of the next month that some subject enters with value already
/// recognized into it, earliest first. A subject is here once, and again after
/// each month for as long as value reaches on.
#[derive(Clone)]
struct Reaching {
    months: BinaryHeap<Reverse<(Day, ReachKey)>>,
    /// The earliest of them: what every moment of the fold asks about.
    soonest: Day,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ReachKey {
    Subject(u32),
    Purpose(u32, u32),
}

pub(crate) enum Reached {
    Subject(Subject, Day),
    Purpose(Id<Entity>, Id<Purpose>, Day),
}

impl Reaching {
    fn new() -> Reaching {
        Reaching {
            months: BinaryHeap::new(),
            soonest: Day::MAX,
        }
    }

    /// Notes that one sparse subject or purpose enters the month beginning on `from`.
    fn push(&mut self, from: Day, key: ReachKey) {
        self.months.push(Reverse((from, key)));
        self.soonest = self.soonest.min(from);
    }

    /// The earliest month that begins by `day`, and whose subject it is for.
    fn pop(&mut self, day: Day) -> Option<(Day, ReachKey)> {
        let Reverse(next) = self
            .months
            .peek()
            .copied()
            .filter(|&Reverse((from, _))| from <= day)?;
        self.months.pop();
        self.soonest = self
            .months
            .peek()
            .map_or(Day::MAX, |&Reverse((from, _))| from);
        Some(next)
    }
}

impl Totals {
    pub fn new(watch: &Watch) -> Totals {
        Totals {
            windows: vec![Windows::NONE; watch.subjects().len()],
            reaching: Reaching::new(),
            purpose: Map::default(),
            budget_history: Map::default(),
            budget_total_history: Map::default(),
        }
    }

    /// Counts a flow moved on `day` and recognized over `over`: `out` leaves
    /// every watched subject containing `from` but not `to`, and `arrive`
    /// enters every watched subject containing `to` but not `from`. A side
    /// whose value is unknown (`None`) is left out.
    pub fn record(
        &mut self,
        watch: &Watch,
        (from, to): (Id<Place>, Id<Place>),
        (day, over): (Day, Days),
        out: Option<Qty>,
        arrive: Option<Qty>,
    ) {
        let sides = [(Dir::Out, from, to, out), (Dir::In, to, from, arrive)];
        for (dir, here, there, value) in sides {
            let Some(value) = value else { continue };
            for subject in watch.crossed(here, there) {
                let Some(at) = watch.slot(subject) else {
                    continue;
                };
                if watch.stores_budget_total(at) && !value.is_zero() {
                    self.budget_total_history
                        .entry((subject, dir))
                        .or_default()
                        .record(over, dir, value);
                }
                let windows = &mut self.windows[at];
                if windows.add(day, dir, value, over) && !windows.reaching {
                    windows.reaching = true;
                    self.reaching.push(
                        windows.month.days.last().add_days(1),
                        ReachKey::Subject(at as u32),
                    );
                }
            }
        }
    }

    /// Counts one contract occurrence flow for its governing contract. Unlike
    /// entity totals, this window contains only that contract's movements.
    pub fn record_contract(
        &mut self,
        watch: &Watch,
        contract: axiom_core::Id<axiom_model::Contract>,
        day: Day,
        over: Days,
        dir: Dir,
        amount: Qty,
    ) {
        let Some(at) = watch.slot(Subject::Contract(contract)) else {
            return;
        };
        self.record_slot(at, day, over, dir, amount);
    }

    fn record_slot(&mut self, at: usize, day: Day, over: Days, dir: Dir, amount: Qty) {
        if amount.is_zero() {
            return;
        }
        let windows = &mut self.windows[at];
        if windows.add(day, dir, amount, over) && !windows.reaching {
            windows.reaching = true;
            self.reaching.push(
                windows.month.days.last().add_days(1),
                ReachKey::Subject(at as u32),
            );
        }
    }

    /// Counts a recognized flow for the purposes some law reads. A child
    /// purpose contributes to each requested ancestor, while an unrelated
    /// purpose allocates and updates no rolling state.
    pub fn record_purpose(
        &mut self,
        watch: &Watch,
        owner: Id<Entity>,
        actual: Id<Purpose>,
        (day, over): (Day, Days),
        dir: Dir,
        amount: Qty,
    ) {
        if amount.is_zero() {
            return;
        }
        let Some(purpose_through) = watch.purpose_through.as_ref() else {
            return;
        };
        for &purpose in &purpose_through[actual] {
            let from = {
                let windows = self
                    .purpose
                    .entry((owner, purpose))
                    .or_insert_with(|| Windows::NONE.clone());
                if windows.add(day, dir, amount, over) && !windows.reaching {
                    windows.reaching = true;
                    Some(windows.month.days.last().add_days(1))
                } else {
                    None
                }
            };
            if let Some(from) = from {
                self.reaching.push(
                    from,
                    ReachKey::Purpose(owner.index() as u32, purpose.index() as u32),
                );
            }
        }
        if let Some(budget_through) = watch.budget_through.as_ref() {
            for &purpose in &budget_through[actual] {
                self.budget_history
                    .entry((owner, purpose))
                    .or_default()
                    .record(over, dir, amount);
            }
        }
    }

    /// The amount of this purpose that entered and left the owner's boundary.
    pub fn read_purpose(
        &self,
        owner: Id<Entity>,
        purpose: Id<Purpose>,
        window: Window,
        day: Day,
    ) -> (Qty, Qty) {
        self.purpose
            .get(&(owner, purpose))
            .map_or((Qty::ZERO, Qty::ZERO), |windows| {
                (
                    windows.read(Dir::In, window, day),
                    windows.read(Dir::Out, window, day),
                )
            })
    }

    /// Recognized purpose movement intersecting an arbitrary budget span.
    /// This history is stored only for purposes named by a budget's own limit
    /// or by a share limit, so normal books keep the rolling-only footprint.
    pub fn read_purpose_between(
        &self,
        owner: Id<Entity>,
        purpose: Id<Purpose>,
        span: Days,
    ) -> Result<(Qty, Qty), Fault> {
        self.budget_history
            .get(&(owner, purpose))
            .map_or(Ok((Qty::ZERO, Qty::ZERO)), |history| history.read(span))
    }

    /// Flow through a single watched subject over a historical span. This is
    /// retained only for a computed budget formula that reads entity totals.
    pub fn read_subject_between(&self, subject: Subject, dir: Dir, span: Days) -> Result<Qty, Fault> {
        self.budget_total_history
            .get(&(subject, dir))
            .map_or(Ok(Qty::ZERO), |history| history.read(span).map(|(incoming, outgoing)| {
                match dir {
                    Dir::In => incoming,
                    Dir::Out => outgoing,
                }
            }))
    }

    /// Whether some month begins by `day` with value recognized into it ahead of time.
    #[inline]
    pub fn reaches_by(&self, day: Day) -> bool {
        self.reaching.soonest <= day
    }

    /// The next month that begins by `day` with value recognized into it ahead
    /// of time, and the subject it is for. Each month is handed out once, and
    /// the subject comes back for the month after while value still reaches it.
    pub fn reached(&mut self, watch: &Watch, day: Day) -> Option<Reached> {
        let (from, key) = self.reaching.pop(day)?;
        let month = Window::Month.around(from);
        match key {
            ReachKey::Subject(at) => {
                let windows = &mut self.windows[at as usize];
                windows.reaching = windows
                    .ahead
                    .iter()
                    .any(|accrual| accrual.over.last() > month.last());
                if windows.reaching {
                    self.reaching.push(month.last().add_days(1), key);
                }
                Some(Reached::Subject(watch.subjects[at as usize], from))
            }
            ReachKey::Purpose(owner, purpose) => {
                let (owner, purpose) = (Id::new(owner), Id::new(purpose));
                let windows = self.purpose.get_mut(&(owner, purpose))?;
                windows.reaching = windows
                    .ahead
                    .iter()
                    .any(|accrual| accrual.over.last() > month.last());
                if windows.reaching {
                    self.reaching.push(month.last().add_days(1), key);
                }
                Some(Reached::Purpose(owner, purpose, from))
            }
        }
    }

    /// What entered or left `subject` in the window containing `day`.
    pub fn read(&self, watch: &Watch, subject: Subject, dir: Dir, window: Window, day: Day) -> Qty {
        watch
            .slot(subject)
            .map_or(Qty::ZERO, |at| self.windows[at].read(dir, window, day))
    }
}

/// Where a subject's totals are kept: places first, then entities.
fn slot(places: usize, entities: usize, assets: usize, subject: Subject) -> usize {
    match subject {
        Subject::Place(place) => place.index(),
        Subject::Entity(entity) => places + entity.index(),
        Subject::Asset(asset) => places + entities + asset.index(),
        Subject::Contract(contract) => places + entities + assets + contract.index(),
    }
}

/// The subject whose totals are kept at `slot`.
fn subject_at(places: usize, entities: usize, assets: usize, slot: usize) -> Subject {
    match slot {
        at if at < places => Subject::Place(Id::new(at as u32)),
        at if at < places + entities => Subject::Entity(Id::new((at - places) as u32)),
        at if at < places + entities + assets => {
            Subject::Asset(Id::new((at - places - entities) as u32))
        }
        at => Subject::Contract(Id::new((at - places - entities - assets) as u32)),
    }
}

fn subject_key(places: usize, entities: usize, assets: usize, subject: Subject) -> usize {
    slot(places, entities, assets, subject)
}

/// What `count` effects have added up to, keyed by `(owner, year, name)`. A
/// tally is a name in a year for one owner; which system's law counted it is
/// kept on the [`Effect`](crate::Effect) for reports, not in the lookup.
#[derive(Clone, Default)]
pub(crate) struct Tallies {
    sums: Map<(Id<Entity>, i32, Sym), Qty>,
}

impl Hash for Tallies {
    fn hash<H: Hasher>(&self, state: &mut H) {
        unordered(self.sums.iter().filter(|(_, qty)| !qty.is_zero())).hash(state);
    }
}

impl Tallies {
    pub fn add(&mut self, owner: Id<Entity>, year: i32, name: Sym, qty: Qty) {
        *self.sums.entry((owner, year, name)).or_default() += qty;
    }

    pub fn read(&self, owner: Id<Entity>, year: i32, name: Sym) -> Qty {
        self.sums
            .get(&(owner, year, name))
            .copied()
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(year: i32, month: u32, day: u32) -> Day {
        Day::from_ymd(year, month, day).unwrap()
    }

    fn days(first: Day, last: Day) -> Days {
        Days::new(first, last).unwrap()
    }

    #[test]
    fn a_range_is_cut_by_the_years_it_touches() {
        let over = days(day(2025, 11, 1), day(2026, 1, 31));
        let years: Vec<_> = by_year(Qty(120_00), over)
            .map(|(from, part)| (from.ymd(), part.0))
            .collect();
        assert_eq!(years, [((2025, 11, 1), 7_957), ((2026, 1, 1), 4_043)]);
        let once: Vec<_> = by_year(Qty(120_00), Days::on(day(2026, 3, 9))).collect();
        assert_eq!(once, [(day(2026, 3, 9), Qty(120_00))]);
    }

    #[test]
    fn a_window_reads_what_was_recognized_into_it_including_ahead_of_time() {
        let mut windows = Windows::NONE;
        // 71 days: 12 in December, 31 in January, 28 in February.
        let prepaid = days(day(2025, 12, 20), day(2026, 2, 28));
        windows.add(day(2025, 12, 20), Dir::In, Qty(70_00), prepaid);
        windows.add(
            day(2025, 12, 21),
            Dir::In,
            Qty(5_00),
            Days::on(day(2025, 12, 21)),
        );
        let read = |windows: &Windows, window, d| windows.read(Dir::In, window, d).0;
        assert_eq!(
            read(&windows, Window::Month, day(2025, 12, 31)),
            5_00 + 11_83
        );
        assert_eq!(
            read(&windows, Window::Month, day(2026, 1, 20)),
            30_56,
            "January is empty so far: only what was recognized ahead of it"
        );
        assert_eq!(read(&windows, Window::Year, day(2026, 3, 1)), 58_17);
        windows.add(
            day(2026, 1, 5),
            Dir::In,
            Qty(1_00),
            Days::on(day(2026, 1, 5)),
        );
        assert_eq!(
            read(&windows, Window::Month, day(2026, 1, 20)),
            31_56,
            "the accrual joined the month that rolled in"
        );
        assert_eq!(
            read(&windows, Window::Year, day(2025, 12, 31)),
            16_83,
            "the year that just closed stays readable"
        );
        assert_eq!(read(&windows, Window::Year, day(2026, 6, 1)), 59_17);
        assert_eq!(read(&windows, Window::Ever, day(2026, 1, 20)), 76_00);
    }

    #[test]
    fn a_flow_recognized_entirely_in_a_closed_window_counts_in_no_current_one() {
        let mut windows = Windows::NONE;
        windows.add(
            day(2026, 1, 15),
            Dir::Out,
            Qty(3_000_00),
            days(day(2025, 1, 1), day(2025, 12, 31)),
        );
        assert_eq!(
            windows.read(Dir::Out, Window::Year, day(2026, 1, 15)),
            Qty::ZERO
        );
        assert_eq!(
            windows.read(Dir::Out, Window::Ever, day(2026, 1, 15)),
            Qty(3_000_00)
        );
    }

    #[test]
    fn indexed_budget_ranges_match_the_linear_recognition_scan() {
        let origin = day(2025, 1, 1);
        let mut history = History::default();
        let mut facts = Vec::new();
        // Deliberately add records in a permuted order. Single-day entries
        // include refunds; ranged records exercise the slow path and exact
        // calendar spread rounding.
        for step in 0..2_000_i32 {
            let offset = if step > 13 && step % 23 == 0 { step - 13 } else { step };
            let first = Day(origin.0 + offset);
            let is_range = step % 7 == 0;
            let over = if is_range {
                days(first, Day(first.0 + 1 + step % 5))
            } else {
                Days::on(first)
            };
            let dir = if step % 3 == 0 { Dir::In } else { Dir::Out };
            let magnitude = i64::from((step % 97) + 1) * 37;
            let amount = Qty(if step % 11 == 0 { -magnitude } else { magnitude });
            history.record(over, dir, amount);
            facts.push(RecognitionFact { over, dir, amount });
        }

        for offset in (0..2_000_i32).step_by(19) {
            let first = Day(origin.0 + offset);
            let last = Day((origin.0 + offset + 143).min(origin.0 + 2_005));
            let span = days(first, last);
            let mut incoming = 0_i128;
            let mut outgoing = 0_i128;
            for fact in &facts {
                let Some(overlap) = fact.over.intersect(span) else { continue };
                let amount = i128::from(spread(fact.amount, fact.over, overlap).0);
                match fact.dir {
                    Dir::In => incoming += amount,
                    Dir::Out => outgoing += amount,
                }
            }
            assert_eq!(history.read(span), Ok((qty(incoming).unwrap(), qty(outgoing).unwrap())), "all facts span={span:?}");
        }
    }

    #[test]
    fn indexed_history_checks_the_final_base_amount_range() {
        let target = day(2025, 1, 2);
        let mut history = History::default();
        history.record(Days::on(target), Dir::Out, Qty(i64::MAX));
        history.record(Days::on(target), Dir::Out, Qty(1));
        assert_eq!(history.read(Days::on(target)), Err(Fault::Overflow));

        let mut cancel = History::default();
        cancel.record(Days::on(target), Dir::Out, Qty(i64::MAX));
        cancel.record(Days::on(target), Dir::Out, Qty(1));
        cancel.record(Days::on(target), Dir::Out, Qty(-1));
        assert_eq!(cancel.read(Days::on(target)), Ok((Qty::ZERO, Qty(i64::MAX))));
    }

    /// Manual 1M-fact scale check for the retained tree and repeated range
    /// query path. Run with `cargo test -p axiom-engine budget_history_million -- --ignored --nocapture`.
    #[test]
    #[ignore = "manual million-record budget-history scale check"]
    fn budget_history_million_fact_scale_check() {
        let started = std::time::Instant::now();
        let mut history = History::default();
        for index in 0..1_000_000_i32 {
            history.record(Days::on(Day(index)), Dir::Out, Qty(1));
        }
        let built = started.elapsed();

        let facts: Vec<_> = (0..1_000_000_i32)
            .map(|index| RecognitionFact { over: Days::on(Day(index)), dir: Dir::Out, amount: Qty(1) })
            .collect();
        let index_bytes = history.days.capacity() * std::mem::size_of::<DayFact>()
            + history.blocks.capacity() * std::mem::size_of::<BlockPrefix>()
            + history.slow.capacity() * std::mem::size_of::<RecognitionFact>();
        let linear_bytes = facts.capacity() * std::mem::size_of::<RecognitionFact>();

        let index_started = std::time::Instant::now();
        let mut index_checksum = 0_i128;
        for query in 0..100_i32 {
            let first = Day((query * 7_919) % 899_900);
            let span = days(first, Day(first.0 + 99));
            index_checksum += i128::from(history.read(span).unwrap().1.0);
        }
        let index_100 = index_started.elapsed();
        let linear_started = std::time::Instant::now();
        let mut linear_checksum = 0_i128;
        for query in 0..100_i32 {
            let first = Day((query * 7_919) % 899_900);
            let span = days(first, Day(first.0 + 99));
            for fact in &facts {
                if let Some(overlap) = fact.over.intersect(span) {
                    linear_checksum += i128::from(spread(fact.amount, fact.over, overlap).0);
                }
            }
        }
        let linear_100 = linear_started.elapsed();

        let query_started = std::time::Instant::now();
        let mut checksum = 0_i128;
        for start in (0..900_000_i32).step_by(10) {
            let sum = history.read(days(Day(start), Day(start + 99))).unwrap().1;
            checksum += i128::from(sum.0);
        }
        let queried = query_started.elapsed();
        eprintln!(
            "facts=1000000 index_retained_capacity_bytes={index_bytes} linear_fact_capacity_bytes={linear_bytes} build={built:?} indexed_100_queries={index_100:?} linear_100_queries={linear_100:?} indexed_90000_queries={queried:?}"
        );
        assert_eq!(index_checksum, linear_checksum);
        assert_eq!(checksum, 9_000_000);
    }
}
