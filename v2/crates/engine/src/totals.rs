//! Running sums: window totals per subject, and tallies.
//!
//! Both are kept in the base currency, and both follow a flow's *recognition*
//! (the days it belongs to), not the day it moved. A flow recognized over a
//! range counts in each window its days touch, in proportion to the days: the
//! share falling in the current window at once, and the rest as the fold
//! reaches later windows. Windows roll: each remembers the days it covers, and
//! a read from another window finds only what was recognized into it. The fold
//! visits days in order, so nothing is ever recomputed.

use axiom_core::day::days_in_month;
use axiom_core::{Day, Groups, Id, Map, Qty, Sym};
use axiom_model::{Book, Dir, Entity, Func, Law, NodeId, Op, Place, Recognition, Subject, Ty, Window};

use crate::scope::{containing, inside};

/// Whether `day` lies within the inclusive range.
pub(crate) fn has(range: Recognition, day: Day) -> bool {
    (range.from..=range.until).contains(&day)
}

/// The days of the month or year containing `day`.
pub(crate) fn window_of(window: Window, day: Day) -> Recognition {
    let (year, month, date) = day.ymd();
    match window {
        Window::Month => {
            let from = day.add_days(1 - date as i32);
            Recognition { from, until: from.add_days(days_in_month(year, month) as i32 - 1) }
        }
        Window::Year => {
            let (start, end) = (Day::from_ymd(year, 1, 1), Day::from_ymd(year, 12, 31));
            Recognition { from: start.unwrap_or(day), until: end.unwrap_or(day) }
        }
        Window::Ever => Recognition { from: Day(i32::MIN), until: Day(i32::MAX) },
    }
}

/// The part of `amount`, recognized evenly over `over`, that falls in `window`.
/// Parts are differences of rounded running shares, so the parts of a range
/// cut into consecutive windows add up to the amount exactly.
pub(crate) fn share(amount: Qty, over: Recognition, window: Recognition) -> Qty {
    if window.from > window.until {
        return Qty::ZERO;
    }
    if over.is_instant() {
        return if has(window, over.from) { amount } else { Qty::ZERO };
    }
    let days = i64::from((over.until.0 - over.from.0 + 1).max(1));
    let through = |day: Day| {
        let elapsed = i64::from((day.0.saturating_sub(over.from.0) + 1).clamp(0, days as i32));
        amount.share(Qty(elapsed), Qty(days)).unwrap_or(amount)
    };
    through(window.until) - through(window.from.add_days(-1))
}

/// `amount` cut by the calendar years `over` touches: the first day of each
/// year's part, and its share.
pub(crate) fn by_year(amount: Qty, over: Recognition) -> impl Iterator<Item = (Day, Qty)> {
    // A flow that belongs to one day belongs to one year: no calendar to consult.
    let years = if over.is_instant() { 0..=0 } else { over.from.year()..=over.until.year() };
    years.map(move |year| {
        if over.is_instant() {
            return (over.from, amount);
        }
        let whole = window_of(Window::Year, Day::from_ymd(year, 1, 1).unwrap_or(over.from));
        (whole.from.max(over.from), share(amount, over, whole))
    })
}

/// The part of `amount` that `over` recognizes in the year `over` starts in.
pub(crate) fn share_in_first_year(amount: Qty, over: Recognition) -> Qty {
    if over.is_instant() { amount } else { share(amount, over, window_of(Window::Year, over.from)) }
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
    days: Recognition,
    flowed: Flowed,
}

impl Rolling {
    const NEVER: Rolling = Rolling {
        days: Recognition { from: Day(i32::MAX), until: Day(i32::MIN) },
        flowed: Flowed { incoming: Qty::ZERO, outgoing: Qty::ZERO },
    };

    /// The window `days`, holding what earlier flows recognized into it.
    fn of(days: Recognition, ahead: &[Accrual]) -> Rolling {
        let mut flowed = Flowed::default();
        for accrual in ahead {
            *flowed.side(accrual.dir) += share(accrual.amount, accrual.over, days);
        }
        Rolling { days, flowed }
    }
}

/// Value recognized over days that had not come when its flow was counted.
#[derive(Clone, Copy)]
struct Accrual {
    dir: Dir,
    amount: Qty,
    over: Recognition,
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
}

impl Windows {
    const NONE: Windows = Windows {
        month: Rolling::NEVER,
        year: Rolling::NEVER,
        closed: Rolling::NEVER,
        ever: Flowed { incoming: Qty::ZERO, outgoing: Qty::ZERO },
        ahead: Vec::new(),
    };

    /// Moves the current windows on to the ones containing `day`. Once a month
    /// or year, and kept out of line so that its calendar arithmetic is not
    /// worked out ahead of time on every flow.
    #[cold]
    #[inline(never)]
    fn roll(&mut self, day: Day) {
        if !has(self.month.days, day) {
            self.month = Rolling::of(window_of(Window::Month, day), &self.ahead);
        }
        if !has(self.year.days, day) {
            let year = Rolling::of(window_of(Window::Year, day), &self.ahead);
            let old = std::mem::replace(&mut self.year, year);
            self.closed = if old.days.until.add_days(1) == year.days.from { old } else { Rolling::NEVER };
        }
        self.ahead.retain(|accrual| accrual.over.until >= self.month.days.from);
    }

    fn add(&mut self, day: Day, dir: Dir, amount: Qty, over: Recognition) {
        *self.ever.side(dir) += amount;
        if !has(self.month.days, day) || !has(self.year.days, day) {
            self.roll(day);
        }
        // Counted on the day it moved, in the windows that day is in: nearly every flow.
        if over.from == day && over.until == day {
            *self.month.flowed.side(dir) += amount;
            *self.year.flowed.side(dir) += amount;
            return;
        }
        for rolling in [&mut self.month, &mut self.year, &mut self.closed] {
            *rolling.flowed.side(dir) += share(amount, over, rolling.days);
        }
        if over.until > self.month.days.until {
            self.ahead.push(Accrual { dir, amount, over });
        }
    }

    /// What was recognized into the window containing `day`, which may be
    /// the year that just closed.
    fn read(&self, dir: Dir, window: Window, day: Day) -> Qty {
        let rolling = match window {
            Window::Ever => return *{ self.ever }.side(dir),
            Window::Month => &self.month,
            Window::Year if has(self.closed.days, day) => &self.closed,
            Window::Year => &self.year,
        };
        // A window nothing was counted into yet holds only what was recognized ahead of it.
        let mut flowed = if has(rolling.days, day) {
            rolling.flowed
        } else {
            Rolling::of(window_of(window, day), &self.ahead).flowed
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
}

impl Totals {
    pub fn new(book: &Book) -> Totals {
        let n = book.places.len() + book.entities.len();
        let mut totals = Totals {
            places: book.places.len(),
            watched: vec![false; n],
            through: Groups::default(),
            windows: vec![Windows::NONE; n],
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
        (day, over): (Day, Recognition),
        out: Option<Qty>,
        arrive: Option<Qty>,
    ) {
        let sides = [(Dir::Out, from, to, out), (Dir::In, to, from, arrive)];
        for (dir, here, there, value) in sides {
            let Some(value) = value else { continue };
            for &subject in self.through[here].iter().filter(|&&subject| !inside(book, subject, there)) {
                self.windows[slot(self.places, subject)].add(day, dir, value, over);
            }
        }
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

    #[test]
    fn a_range_is_shared_out_by_days_and_the_shares_add_up() {
        let over = Recognition { from: day(2025, 11, 1), until: day(2026, 1, 31) };
        let month = |year, month| window_of(Window::Month, day(year, month, 15));
        let parts = [month(2025, 11), month(2025, 12), month(2026, 1)].map(|window| share(Qty(120_00), over, window).0);
        assert_eq!(parts, [3_913, 4_044, 4_043]);
        assert_eq!(parts.iter().sum::<i64>(), 120_00);
        let years: Vec<_> = by_year(Qty(120_00), over).map(|(from, part)| (from.ymd(), part.0)).collect();
        assert_eq!(years, [((2025, 11, 1), 7_957), ((2026, 1, 1), 4_043)]);
    }

    #[test]
    fn a_window_reads_what_was_recognized_into_it_including_ahead_of_time() {
        let mut windows = Windows::NONE;
        // 71 days: 12 in December, 31 in January, 28 in February.
        let spread = Recognition { from: day(2025, 12, 20), until: day(2026, 2, 28) };
        windows.add(day(2025, 12, 20), Dir::In, Qty(70_00), spread);
        windows.add(day(2025, 12, 21), Dir::In, Qty(5_00), Recognition::on(day(2025, 12, 21)));
        let read = |windows: &Windows, window, d| windows.read(Dir::In, window, d).0;
        assert_eq!(read(&windows, Window::Month, day(2025, 12, 31)), 5_00 + 11_83);
        assert_eq!(
            read(&windows, Window::Month, day(2026, 1, 20)),
            30_56,
            "January is empty so far: only what was recognized ahead of it"
        );
        assert_eq!(read(&windows, Window::Year, day(2026, 3, 1)), 58_17);
        windows.add(day(2026, 1, 5), Dir::In, Qty(1_00), Recognition::on(day(2026, 1, 5)));
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
        windows.add(
            day(2026, 1, 15),
            Dir::Out,
            Qty(3_000_00),
            Recognition { from: day(2025, 1, 1), until: day(2025, 12, 31) },
        );
        assert_eq!(windows.read(Dir::Out, Window::Year, day(2026, 1, 15)), Qty::ZERO);
        assert_eq!(windows.read(Dir::Out, Window::Ever, day(2026, 1, 15)), Qty(3_000_00));
    }
}
