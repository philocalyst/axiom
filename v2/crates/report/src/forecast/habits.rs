//! What history says about the future: the flows that recur, and how much
//! everything else varies.

use std::collections::BTreeMap;

use axiom_core::{Day, Id, Qty, Set};
use axiom_engine::Run;
use axiom_model::{Book, Class, Entity, Flow, Period, Place};

use super::recurrence::{Recurrence, detect};
use crate::calendar::Periods;
use crate::history::{Posting, postings};
use crate::places::category;

/// Who pays whom, through whom: what makes two flows the same habit.
type Key = (Id<Place>, Id<Place>, Option<Id<Entity>>);

/// A rhythm found among real flows.
pub struct Habit {
    pub recurrence: Recurrence,
    pub occurrences: usize,
    /// The latest occurrence: projections take their ends and payee from it.
    pub template: Id<Flow>,
}

/// The recurring flows in the journal that no plan already covers.
pub struct Habits {
    pub found: Vec<Habit>,
    /// Every (from, to, payee) that recurs, so variable spending can leave it out.
    keys: Set<Key>,
}

impl Habits {
    /// Groups real transfers by (from, to, payee) and keeps the groups that
    /// have a rhythm. `covered` says which pairs a plan already projects.
    pub fn infer(book: &Book, run: &Run, covered: &Set<(Id<Place>, Id<Place>)>) -> Habits {
        let mut groups: BTreeMap<Key, Vec<(Day, Qty, Id<Flow>)>> = BTreeMap::new();
        let candidates = postings(book, run).filter(|posting| {
            let flow = posting.flow;
            posting.is_real_on(run.today)
                && !flow.is_exchange()
                && flow.from != book.roots.unknown
                && flow.to != book.roots.unknown
        });
        for posting in candidates {
            let flow = posting.flow;
            groups.entry(key(flow)).or_default().push((flow.day, posting.posted.out, posting.id));
        }

        let mut habits = Habits { found: Vec::new(), keys: Set::default() };
        for (key, occurrences) in groups {
            if covered.contains(&(key.0, key.1)) {
                continue;
            }
            let series: Vec<(Day, Qty)> = occurrences.iter().map(|&(day, qty, _)| (day, qty)).collect();
            if let (Some(recurrence), Some(&(_, _, template))) = (detect(&series, run.today), occurrences.last()) {
                habits.keys.insert(key);
                habits.found.push(Habit { recurrence, occurrences: series.len(), template });
            }
        }
        habits
    }

    /// Whether a flow belongs to a habit found here.
    pub fn explains(&self, flow: &Flow) -> bool {
        self.keys.contains(&key(flow))
    }
}

fn key(flow: &Flow) -> Key {
    (flow.from, flow.to, flow.payee)
}

/// What each top-level expense category cost in each full month of history,
/// leaving out the flows that plans and habits already project. This is the
/// variable part of spending, the part the bands bootstrap.
pub struct Variable {
    /// One series per category, one figure per month, oldest first, in base quanta.
    pub categories: Vec<Vec<i64>>,
    pub months: usize,
}

impl Variable {
    /// History runs from the first month anything was spent to the last full
    /// month: the current month is still going, and months before the books
    /// had any spending would only dilute it.
    pub fn from_history(book: &Book, run: &Run, explained: impl Fn(&Flow) -> bool) -> Variable {
        let none = Variable { categories: Vec::new(), months: 0 };
        let first_spent = postings(book, run).find(|posting| !spending(book, posting).is_empty());
        let Some(first) = first_spent.map(|posting| posting.flow.day) else { return none };
        let last_full_month = run.today.month_start().add_days(-1);
        if first > last_full_month {
            return none;
        }

        let months = Periods::covering(Period::Month, first, last_full_month);
        let mut categories: BTreeMap<Id<Place>, Vec<i64>> = BTreeMap::new();
        let real =
            postings(book, run).filter(|posting| posting.is_real_on(last_full_month) && !explained(posting.flow));
        for posting in real {
            let Some(month) = months.index_of(posting.flow.day) else { continue };
            for (place, qty) in spending(book, &posting) {
                categories.entry(category(book, place)).or_insert_with(|| vec![0; months.len()])[month] += qty.0;
            }
        }
        Variable { categories: categories.into_values().collect(), months: months.len() }
    }
}

/// What a flow spent in the expense places it touches, in the base currency:
/// money into one counts, and a refund out of one takes it back.
fn spending(book: &Book, posting: &Posting) -> Vec<(Id<Place>, Qty)> {
    let into = (posting.flow.to, posting.arrive_in_base(book));
    let refunded = (posting.flow.from, posting.out_in_base(book).map(|qty| -qty));
    [into, refunded]
        .into_iter()
        .filter(|&(place, _)| book.places[place].class == Class::Expense)
        .filter_map(|(place, qty)| Some((place, qty?)))
        .collect()
}

#[cfg(test)]
mod tests {
    use axiom_core::Qty;
    use axiom_model::On;

    use super::*;
    use crate::forecast::recurrence::Cadence;
    use crate::tests::household;

    #[test]
    fn the_salary_is_a_rhythm_and_the_rest_of_spending_is_variable() {
        let house = household();
        let habits = Habits::infer(&house.book, &house.run, &Set::default());
        // Three paychecks make a rhythm; two rents do not.
        let [salary] = &habits.found[..] else { panic!("only the salary recurs") };
        assert_eq!(salary.recurrence.cadence, Cadence::Monthly);
        assert_eq!((salary.recurrence.amount, salary.recurrence.on), (Qty(500_000), Some(On::MonthDay(15))));
        assert_eq!(salary.occurrences, 3);

        // January's groceries and February's card spending; the year's insurance
        // in January; rent both months. The pending repair is not real yet.
        let variable = Variable::from_history(&house.book, &house.run, |flow| habits.explains(flow));
        assert_eq!(variable.months, 2);
        assert_eq!(variable.categories, [vec![8_420, 12_000], vec![120_000, 0], vec![180_000, 180_000]]);
    }
}
