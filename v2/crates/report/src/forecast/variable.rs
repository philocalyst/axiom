//! How much everything else varies: the spending nobody planned.

use std::collections::BTreeMap;

use axiom_core::{Id, Qty};
use axiom_engine::Run;
use axiom_model::{End, Flow, Period, Place};

use crate::calendar::Periods;
use crate::history::{Posting, postings};
use crate::lens::Lens;
use crate::places::{Side, category, v3_side};

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
/// money into one counts, and a refund out of one takes it back. A flow whose
/// other end only changed basis paid nothing (depreciation is an expense that
/// nobody paid), so it is not spending.
fn spending<'a>(lens: Lens<'a, '_>, posting: &Posting) -> impl Iterator<Item = (Id<Place>, Qty)> + 'a {
    let flow = posting.flow;
    let paid = flow.moves_quantity(End::From).then(|| posting.arrive_in_base(lens)).flatten();
    let refund = flow.moves_quantity(End::To).then(|| posting.out_in_base(lens).map(|qty| -qty)).flatten();
    [(flow.to, paid), (flow.from, refund)]
        .into_iter()
        .filter(move |&(place, _)| v3_side(lens.book, place) == Some(Side::Spending) && lens.owns(place))
        .filter_map(|(place, qty)| Some((place, qty?)))
}

#[cfg(test)]
mod tests {
    use axiom_core::Day;

    use super::*;
    use crate::lens::Whose;
    use crate::source_tests::with_run;

    /// Groceries cost money each month; the house's depreciation is an expense
    /// in the books that nobody paid, so it is not part of what varies.
    #[test]
    fn depreciation_is_an_expense_nobody_paid_and_is_not_spending() {
        let source = "\
base USD
commodity USD
  precision 2
commodity HOME
  precision 0

account assets/house
account assets/checking
account expenses/groceries
account expenses/depreciation

opening 2026-01-01
  house 1 HOME basis 120_000 USD
  checking 5_000 USD

2026-01-10 checking -> groceries 100 USD
2026-01-31 house.basis -> depreciation 300 USD
2026-02-10 checking -> groceries 100 USD
2026-02-28 house.basis -> depreciation 300 USD
2026-03-10 checking -> groceries 100 USD
2026-04-10 checking -> groceries 100 USD
";
        with_run(source, Day::from_ymd(2026, 5, 15).unwrap(), |book, run| {
            let whose = Whose::default();
            let variable = Variable::from_history(Lens::new(book, &whose, run.today), run, |_| false);
            assert_eq!(variable.categories, [vec![10_000; 4]], "four months of groceries, and no depreciation");
        });
    }
}
