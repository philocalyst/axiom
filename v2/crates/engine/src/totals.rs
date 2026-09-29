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

use axiom_core::calendar;
use axiom_core::{Day, Days, Groups, Id, Map, Period, Qty, Sym, spread};
use axiom_model::{Book, Dir, Entity, Func, Law, NodeId, Op, Place, Subject, Ty, Window};

use crate::eval::V3;
use crate::scope::{containing, inside};

/// `amount` cut by the calendar years `over` touches: the first day of each
/// year's part, and its share.
pub(crate) fn by_year(amount: Qty, over: Days) -> impl Iterator<Item = (Day, Qty)> {
    calendar::Window::covering(Period::Year, over).map(move |year| {
        let part = year.days();
        (part.first().max(over.first()), spread(amount, over, part))
    })
}

/// Value that entered and left, over one window.
#[derive(Clone, Copy, Default)]
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
#[derive(Clone, Copy)]
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
#[derive(Clone, Copy)]
struct Accrual {
    dir: Dir,
    amount: Qty,
    over: Days,
}

/// One subject's windows. `closed` is the year that just ended, kept for the
/// laws that close it late.
#[derive(Clone)]
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

/// Flow totals for the subjects some law reads: places first (each including
/// its subtree), then entities. A book whose laws never read `total(…)` keeps
/// none, and a subject nobody reads costs nothing.
#[derive(Clone)]
pub(crate) struct Totals {
    places: usize,
    watched: Vec<bool>,
    /// The watched subjects each place lies within: the only ones a flow at
    /// that place can enter or leave, found once instead of on every flow.
    through: Groups<Place, Subject>,
    windows: Vec<Windows>,
    reaching: Reaching,
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
    pub fn new(book: &Book) -> Totals {
        let n = book.places.len() + book.entities.len();
        let mut totals = Totals {
            places: book.places.len(),
            watched: vec![false; n],
            through: Groups::default(),
            windows: vec![Windows::NONE; n],
            reaching: Reaching::new(),
        };
        let rules = &book.rules;
        let all = [&rules.on_in, &rules.on_out, &rules.on_gain, &rules.always].into_iter().flat_map(|g| g.values());
        for rule in all.chain(rules.on_spend.values()).chain(&rules.timed) {
            match reads_total(&book.laws[rule.law]) {
                None => {}
                Some(false) => totals.watched[slot(totals.places, rule.subject)] = true,
                // A kind-wide total reads every place of that kind.
                Some(true) => totals.watched[..totals.places].fill(true),
            }
        }
        let places = (0..book.places.len() as u32).map(Id::new);
        let within = places.flat_map(|place| containing(book, place).map(move |subject| (place, subject)));
        let watched = within.filter(|&(_, subject)| totals.watched[slot(totals.places, subject)]);
        totals.through = Groups::build(book.places.len(), watched);
        totals
    }

    /// Whether a flow from `from` to `to` leaves, and whether it enters, any
    /// subject a law reads: only then is its value worth computing.
    pub fn watched_sides(&self, book: &Book, from: Id<Place>, to: Id<Place>) -> (bool, bool) {
        let watches = |here, there| self.through[here].iter().any(|&subject| !inside(book, subject, there));
        (watches(from, to), watches(to, from))
    }

    /// Counts a flow moved on `day` and recognized over `over`: `out` leaves
    /// every watched subject containing `from` but not `to`, and `arrive`
    /// enters every watched subject containing `to` but not `from`. A side
    /// whose value is unknown (`None`) is left out.
    pub fn record(
        &mut self,
        book: &Book,
        (from, to): (Id<Place>, Id<Place>),
        (day, over): (Day, Days),
        out: Option<Qty>,
        arrive: Option<Qty>,
    ) {
        let sides = [(Dir::Out, from, to, out), (Dir::In, to, from, arrive)];
        for (dir, here, there, value) in sides {
            let Some(value) = value else { continue };
            for &subject in self.through[here].iter().filter(|&&subject| !inside(book, subject, there)) {
                let at = slot(self.places, subject);
                let windows = &mut self.windows[at];
                if windows.add(day, dir, value, over) && !windows.reaching {
                    windows.reaching = true;
                    self.reaching.push(windows.month.days.last().add_days(1), at as u32);
                }
            }
        }
    }

    /// Whether some month begins by `day` with value recognized into it ahead of time.
    #[inline]
    pub fn reaches_by(&self, day: Day) -> bool {
        self.reaching.soonest <= day
    }

    /// The next month that begins by `day` with value recognized into it ahead
    /// of time, and the subject it is for. Each month is handed out once, and
    /// the subject comes back for the month after while value still reaches it.
    pub fn reached(&mut self, day: Day) -> Option<(Subject, Day)> {
        let (from, at) = self.reaching.pop(day)?;
        let month = Window::Month.around(from);
        let windows = &mut self.windows[at as usize];
        windows.reaching = windows.ahead.iter().any(|accrual| accrual.over.last() > month.last());
        if windows.reaching {
            self.reaching.push(month.last().add_days(1), at);
        }
        Some((subject_at(self.places, at as usize), from))
    }

    /// What entered or left `subject` in the window containing `day`.
    pub fn read(&self, subject: Subject, dir: Dir, window: Window, day: Day) -> Qty {
        self.windows[slot(self.places, subject)].read(dir, window, day)
    }
}

/// Where a subject's totals are kept: places first, then entities.
fn slot(places: usize, subject: Subject) -> usize {
    match subject {
        Subject::Place(place) => place.index(),
        Subject::Entity(entity) => places + entity.index(),
        Subject::Asset(_) => unreachable!("{V3}"),
    }
}

/// The subject whose totals are kept at `slot`.
fn subject_at(places: usize, slot: usize) -> Subject {
    match slot.checked_sub(places) {
        None => Subject::Place(Id::new(slot as u32)),
        Some(entity) => Subject::Entity(Id::new(entity as u32)),
    }
}

/// Whether a law reads window totals, and whether any of its reads is
/// widened to a kind.
fn reads_total(law: &Law) -> Option<bool> {
    let widened = |args: &[NodeId]| args.iter().any(|arg| law.nodes[arg.index()].ty == Ty::Kind);
    let reads = law.nodes.iter().filter_map(|node| match &node.op {
        Op::Call(Func::Total(..), args) => Some(widened(args)),
        _ => None,
    });
    reads.reduce(|a, b| a || b)
}

/// What `count` effects have added up to, keyed by `(owner, year, name)`. A
/// tally is a name in a year for one owner; which system's law counted it is
/// kept on the [`Effect`](crate::Effect) for reports, not in the lookup.
#[derive(Clone, Default)]
pub(crate) struct Tallies {
    sums: Map<(Id<Entity>, i32, Sym), Qty>,
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
