//! How much everything else varies: the spending nobody planned.

use std::collections::BTreeMap;

use axiom_core::{Id, Qty};
use axiom_engine::Run;
use axiom_model::{Class, Flow, Period, Place};

use crate::calendar::Periods;
use crate::history::{Posting, postings};
use crate::lens::Lens;
use crate::places::category;

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
    pub fn from_history(lens: Lens, run: &Run, explained: impl Fn(&Flow) -> bool) -> Variable {
        let book = lens.book;
        let none = Variable { categories: Vec::new(), months: 0 };
        let first_spent = postings(book, run).find(|posting| spending(lens, posting).next().is_some());
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
            for (place, qty) in spending(lens, &posting) {
                categories.entry(category(book, place)).or_insert_with(|| vec![0; months.len()])[month] += qty.0;
            }
        }
        Variable { categories: categories.into_values().collect(), months: months.len() }
    }
}

/// What a flow spent in the expense places it touches, in the base currency:
/// money into one counts, and a refund out of one takes it back.
fn spending<'a>(lens: Lens<'a, '_>, posting: &Posting) -> impl Iterator<Item = (Id<Place>, Qty)> + 'a {
    let into = (posting.flow.to, posting.arrive_in_base(lens));
    let refunded = (posting.flow.from, posting.out_in_base(lens).map(|qty| -qty));
    [into, refunded]
        .into_iter()
        .filter(move |&(place, _)| lens.book.places[place].class == Class::Expense && lens.owns(place))
        .filter_map(|(place, qty)| Some((place, qty?)))
}
