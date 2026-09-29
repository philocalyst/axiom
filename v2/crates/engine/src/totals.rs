//! Running sums: window totals per subject, and tallies.
//!
//! Both are kept in the base currency. Windows roll: each remembers the month
//! or year it last saw, and a read from another period finds nothing. The fold
//! visits days in order, so nothing is ever recomputed.

use axiom_core::{Day, Id, Map, Qty, Sym};
use axiom_model::{Book, Dir, Entity, Func, Law, NodeId, Op, Place, Subject, Ty, Window};

use crate::scope::departures;

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

/// One window's sums, and the period they belong to.
#[derive(Clone, Copy)]
struct Rolling {
    period: i32,
    flowed: Flowed,
}

impl Rolling {
    const NEVER: Rolling = Rolling { period: i32::MIN, flowed: Flowed { incoming: Qty::ZERO, outgoing: Qty::ZERO } };

    fn at(&mut self, period: i32) -> &mut Flowed {
        if self.period != period {
            *self = Rolling { period, ..Rolling::NEVER };
        }
        &mut self.flowed
    }

    fn peek(&self, period: i32) -> Flowed {
        if self.period == period { self.flowed } else { Flowed::default() }
    }
}

#[derive(Clone, Copy)]
struct Windows {
    month: Rolling,
    year: Rolling,
    ever: Flowed,
}

/// Which month and year a day belongs to, as comparable numbers.
#[derive(Clone, Copy)]
struct Period {
    month: i32,
    year: i32,
}

impl Period {
    fn of(day: Day) -> Period {
        let (year, month, _) = day.ymd();
        Period { month: year * 12 + month as i32 - 1, year }
    }
}

/// Flow totals for the subjects some law reads: places first (each including
/// its subtree), then entities. A book whose laws never read `total(…)` keeps
/// none, and a subject nobody reads costs nothing.
#[derive(Clone)]
pub(crate) struct Totals {
    places: usize,
    watched: Vec<bool>,
    any_watched: bool,
    windows: Vec<Windows>,
}

impl Totals {
    pub fn new(book: &Book) -> Totals {
        let n = book.places.len() + book.entities.len();
        let none = Windows { month: Rolling::NEVER, year: Rolling::NEVER, ever: Flowed::default() };
        let mut totals =
            Totals { places: book.places.len(), watched: vec![false; n], any_watched: false, windows: vec![none; n] };
        let rules = &book.rules;
        let all = [&rules.on_in, &rules.on_out, &rules.on_gain, &rules.always].into_iter().flat_map(|g| g.values());
        for rule in all.chain(rules.on_spend.values()).chain(&rules.timed) {
            match reads_total(&book.laws[rule.law]) {
                None => {}
                Some(false) => totals.watch(totals.slot(rule.subject)),
                // A kind-wide total reads every place of that kind.
                Some(true) => (0..totals.places).for_each(|slot| totals.watch(slot)),
            }
        }
        totals
    }

    fn watch(&mut self, slot: usize) {
        self.watched[slot] = true;
        self.any_watched = true;
    }

    fn slot(&self, subject: Subject) -> usize {
        match subject {
            Subject::Place(place) => place.index(),
            Subject::Entity(entity) => self.places + entity.index(),
        }
    }

    /// Whether a flow from `from` to `to` leaves, and whether it enters, any
    /// subject a law reads: only then is its value worth computing.
    pub fn watched_sides(&self, book: &Book, from: Id<Place>, to: Id<Place>) -> (bool, bool) {
        if !self.any_watched {
            return (false, false);
        }
        let watches = |here, there| departures(book, here, there).any(|subject| self.watched[self.slot(subject)]);
        (watches(from, to), watches(to, from))
    }

    /// Counts a flow: `out` leaves every watched subject containing `from` but
    /// not `to`, and `arrive` enters every watched subject containing `to` but
    /// not `from`. A side whose value is unknown (`None`) is left out.
    pub fn record(
        &mut self,
        book: &Book,
        (from, to): (Id<Place>, Id<Place>),
        day: Day,
        out: Option<Qty>,
        arrive: Option<Qty>,
    ) {
        let period = Period::of(day);
        let sides = [(Dir::Out, from, to, out), (Dir::In, to, from, arrive)];
        for (dir, here, there, value) in sides {
            let Some(value) = value else { continue };
            for subject in departures(book, here, there) {
                let at = self.slot(subject);
                if !self.watched[at] {
                    continue;
                }
                let windows = &mut self.windows[at];
                *windows.month.at(period.month).side(dir) += value;
                *windows.year.at(period.year).side(dir) += value;
                *windows.ever.side(dir) += value;
            }
        }
    }

    /// What entered or left `subject` in the window containing `day`.
    pub fn read(&self, subject: Subject, dir: Dir, window: Window, day: Day) -> Qty {
        let windows = &self.windows[self.slot(subject)];
        let period = Period::of(day);
        let mut flowed = match window {
            Window::Month => windows.month.peek(period.month),
            Window::Year => windows.year.peek(period.year),
            Window::Ever => windows.ever,
        };
        *flowed.side(dir)
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
