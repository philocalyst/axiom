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
use axiom_core::{Day, Days, Groups, Id, Map, Period, Qty, Sym, spread};
use axiom_model::{Book, Dir, Entity, Place, Purpose, Subject, Window};

use crate::facts::{LawFacts, TotalsRead};
use crate::scope::containing;
use crate::state::unordered;

/// `amount` cut by the calendar years `over` touches: the first day of each
/// year's part, and its share.
pub(crate) fn by_year(amount: Qty, over: Days) -> impl Iterator<Item = (Day, Qty)> {
    // A flow that belongs to one day belongs to one year: no calendar to consult.
    let one_day = over.single().map(|day| (day, amount));
    let years = one_day.map_or_else(|| Some(calendar::Window::covering(Period::Year, over)), |_| None);
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
    const NEVER: Rolling =
        Rolling { days: Days::on(Day::MIN), flowed: Flowed { incoming: Qty::ZERO, outgoing: Qty::ZERO } };

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
        ever: Flowed { incoming: Qty::ZERO, outgoing: Qty::ZERO },
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
            self.closed = if old.days.last().add_days(1) == year.days.first() { old } else { Rolling::NEVER };
        }
        self.ahead.retain(|accrual| accrual.over.last() >= self.month.days.first());
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
    purpose_through: Option<Groups<Purpose, Purpose>>,
    places: usize,
    entities: usize,
}

impl Watch {
    pub fn of(book: &Book, laws: &[LawFacts]) -> Watch {
        let (places, entities, assets) = (book.places.len(), book.entities.len(), book.assets.len());
        let mut watched = vec![false; places + entities + assets];
        for rule in book.rules.all() {
            match laws[rule.law.index()].totals {
                TotalsRead::Nothing => {}
                TotalsRead::Subject => watched[slot(places, entities, rule.subject)] = true,
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
                TotalsRead::Subject => watched[slot(places, entities, rule.subject)] = true,
                TotalsRead::Kind => watched[..places].fill(true),
            }
        }
        let mut slots = vec![u32::MAX; watched.len()];
        let mut subjects = Vec::new();
        for (at, &yes) in watched.iter().enumerate() {
            if yes {
                slots[at] = subjects.len() as u32;
                subjects.push(subject_at(places, entities, at));
            }
        }
        let mut within: Vec<_> = (0..places as u32)
            .map(Id::new)
            .flat_map(|place| containing(book, place).map(move |subject| (place, subject)))
            .filter(|&(_, subject)| watched[slot(places, entities, subject)])
            .collect();
        // An asset's totals follow its asset place and its parts. Keep the
        // relation in the plan so posting a flow never scans the asset table.
        for (asset, _) in book.assets.iter() {
            let mut part = Some(asset);
            while let Some(current) = part {
                let subject = Subject::Asset(current);
                if watched[slot(places, entities, subject)] {
                    within.extend(book.places.subtree(book.assets[asset].place).map(|place| (place, subject)));
                }
                part = book.assets[current].part_of;
            }
        }
        within.sort_unstable_by_key(|&(place, subject)| (place, subject_key(places, entities, subject)));
        within.dedup();
        let through = Groups::build(places, within);
        let mut purpose_reads = Set::default();
        for law in book.laws.values() {
            for node in &law.nodes {
                let axiom_model::Op::Call(axiom_model::Func::PurposeTotal { purpose, .. }, _) = &node.op else {
                    continue;
                };
                let purpose = (*purpose).or_else(|| match law.owner {
                    axiom_model::Owner::Purpose(purpose) => Some(purpose),
                    _ => None,
                });
                if let Some(purpose) = purpose {
                    purpose_reads.insert(purpose);
                }
            }
        }
        let purpose_through = if purpose_reads.is_empty() {
            None
        } else {
            let within = book.purposes.ids().flat_map(|actual| {
                book.purposes.lineage(actual).filter(|ancestor| purpose_reads.contains(ancestor)).map(move |ancestor| (actual, ancestor))
            });
            Some(Groups::build(book.purposes.len(), within))
        };
        Watch { through, slots: slots.into(), subjects: subjects.into(), purpose_through, places, entities }
    }

    fn slot(&self, subject: Subject) -> Option<usize> {
        let slot = self.slots[slot(self.places, self.entities, subject)];
        (slot != u32::MAX).then_some(slot as usize)
    }

    pub(crate) fn subjects(&self) -> &[Subject] {
        &self.subjects
    }

    pub(crate) fn reads_purpose(&self, purpose: Id<Purpose>) -> bool {
        self.purpose_through.as_ref().is_some_and(|through| !through[purpose].is_empty())
    }

    /// The watched subjects that contain `here` but not `there`: a flow from
    /// `here` to `there` leaves them, and one from `there` to `here` enters them.
    fn crossed(&self, here: Id<Place>, there: Id<Place>) -> impl Iterator<Item = Subject> + '_ {
        let beyond = &self.through[there];
        self.through[here].iter().copied().filter(move |subject| !beyond.contains(subject))
    }

    /// Whether a flow from `from` to `to` leaves, and whether it enters, any
    /// subject a law reads: only then is its value worth computing.
    pub fn sides(&self, from: Id<Place>, to: Id<Place>) -> (bool, bool) {
        (self.crossed(from, to).next().is_some(), self.crossed(to, from).next().is_some())
    }
}

/// Running flow totals for the subjects a [`Watch`] names.
#[derive(Clone)]
pub(crate) struct Totals {
    windows: Vec<Windows>,
    reaching: Reaching,
    purpose: Map<(Id<Entity>, Id<Purpose>), Windows>,
}

/// What the running totals hold: the windows, which are all the future reads.
impl Hash for Totals {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.windows.hash(state);
        unordered(self.purpose.iter()).hash(state);
    }
}

/// The first day of the next month that some subject enters with value already
/// recognized into it, earliest first. A subject is here once, and again after
/// each month for as long as value reaches on.
#[derive(Clone)]
struct Reaching {
    months: BinaryHeap<Reverse<(Day, u32)>>,
    /// The earliest of them: what every moment of the fold asks about.
    soonest: Day,
}

impl Reaching {
    fn new() -> Reaching {
        Reaching { months: BinaryHeap::new(), soonest: Day::MAX }
    }

    /// Notes that `slot` enters the month that begins on `from`.
    fn push(&mut self, from: Day, slot: u32) {
        self.months.push(Reverse((from, slot)));
        self.soonest = self.soonest.min(from);
    }

    /// The earliest month that begins by `day`, and whose subject it is for.
    fn pop(&mut self, day: Day) -> Option<(Day, u32)> {
        let Reverse(next) = self.months.peek().copied().filter(|&Reverse((from, _))| from <= day)?;
        self.months.pop();
        self.soonest = self.months.peek().map_or(Day::MAX, |&Reverse((from, _))| from);
        Some(next)
    }
}

impl Totals {
    pub fn new(watch: &Watch) -> Totals {
        Totals {
            windows: vec![Windows::NONE; watch.subjects().len()],
            reaching: Reaching::new(),
            purpose: Map::default(),
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
                let Some(at) = watch.slot(subject) else { continue };
                let windows = &mut self.windows[at];
                if windows.add(day, dir, value, over) && !windows.reaching {
                    windows.reaching = true;
                    self.reaching.push(windows.month.days.last().add_days(1), at as u32);
                }
            }
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
        let Some(purpose_through) = watch.purpose_through.as_ref() else { return };
        for &purpose in &purpose_through[actual] {
            self.purpose.entry((owner, purpose)).or_insert_with(|| Windows::NONE.clone()).add(day, dir, amount, over);
        }
    }

    /// The amount of this purpose that entered and left the owner's boundary.
    pub fn read_purpose(&self, owner: Id<Entity>, purpose: Id<Purpose>, window: Window, day: Day) -> (Qty, Qty) {
        self.purpose
            .get(&(owner, purpose))
            .map_or((Qty::ZERO, Qty::ZERO), |windows| {
                (windows.read(Dir::In, window, day), windows.read(Dir::Out, window, day))
            })
    }

    /// Whether some month begins by `day` with value recognized into it ahead of time.
    #[inline]
    pub fn reaches_by(&self, day: Day) -> bool {
        self.reaching.soonest <= day
    }

    /// The next month that begins by `day` with value recognized into it ahead
    /// of time, and the subject it is for. Each month is handed out once, and
    /// the subject comes back for the month after while value still reaches it.
    pub fn reached(&mut self, watch: &Watch, day: Day) -> Option<(Subject, Day)> {
        let (from, at) = self.reaching.pop(day)?;
        let month = Window::Month.around(from);
        let windows = &mut self.windows[at as usize];
        windows.reaching = windows.ahead.iter().any(|accrual| accrual.over.last() > month.last());
        if windows.reaching {
            self.reaching.push(month.last().add_days(1), at);
        }
        Some((watch.subjects[at as usize], from))
    }

    /// What entered or left `subject` in the window containing `day`.
    pub fn read(&self, watch: &Watch, subject: Subject, dir: Dir, window: Window, day: Day) -> Qty {
        watch.slot(subject).map_or(Qty::ZERO, |at| self.windows[at].read(dir, window, day))
    }
}

/// Where a subject's totals are kept: places first, then entities.
fn slot(places: usize, entities: usize, subject: Subject) -> usize {
    match subject {
        Subject::Place(place) => place.index(),
        Subject::Entity(entity) => places + entity.index(),
        Subject::Asset(asset) => places + entities + asset.index(),
    }
}

/// The subject whose totals are kept at `slot`.
fn subject_at(places: usize, entities: usize, slot: usize) -> Subject {
    match slot {
        at if at < places => Subject::Place(Id::new(at as u32)),
        at if at < places + entities => Subject::Entity(Id::new((at - places) as u32)),
        at => Subject::Asset(Id::new((at - places - entities) as u32)),
    }
}

fn subject_key(places: usize, entities: usize, subject: Subject) -> usize {
    slot(places, entities, subject)
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
        self.sums.get(&(owner, year, name)).copied().unwrap_or_default()
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
        let years: Vec<_> = by_year(Qty(120_00), over).map(|(from, part)| (from.ymd(), part.0)).collect();
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
        windows.add(day(2025, 12, 21), Dir::In, Qty(5_00), Days::on(day(2025, 12, 21)));
        let read = |windows: &Windows, window, d| windows.read(Dir::In, window, d).0;
        assert_eq!(read(&windows, Window::Month, day(2025, 12, 31)), 5_00 + 11_83);
        assert_eq!(
            read(&windows, Window::Month, day(2026, 1, 20)),
            30_56,
            "January is empty so far: only what was recognized ahead of it"
        );
        assert_eq!(read(&windows, Window::Year, day(2026, 3, 1)), 58_17);
        windows.add(day(2026, 1, 5), Dir::In, Qty(1_00), Days::on(day(2026, 1, 5)));
        assert_eq!(
            read(&windows, Window::Month, day(2026, 1, 20)),
            31_56,
            "the accrual joined the month that rolled in"
        );
        assert_eq!(read(&windows, Window::Year, day(2025, 12, 31)), 16_83, "the year that just closed stays readable");
        assert_eq!(read(&windows, Window::Year, day(2026, 6, 1)), 59_17);
        assert_eq!(read(&windows, Window::Ever, day(2026, 1, 20)), 76_00);
    }

    #[test]
    fn a_flow_recognized_entirely_in_a_closed_window_counts_in_no_current_one() {
        let mut windows = Windows::NONE;
        windows.add(day(2026, 1, 15), Dir::Out, Qty(3_000_00), days(day(2025, 1, 1), day(2025, 12, 31)));
        assert_eq!(windows.read(Dir::Out, Window::Year, day(2026, 1, 15)), Qty::ZERO);
        assert_eq!(windows.read(Dir::Out, Window::Ever, day(2026, 1, 15)), Qty(3_000_00));
    }
}
