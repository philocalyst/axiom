//! How much other spending varies: the spending contracts and habits do not explain.

use std::collections::BTreeMap;

use axiom_core::{Days, Id, Qty, spread};
use axiom_engine::Run;
use axiom_model::{Flow, Period, Purpose, PurposeRoot};

use crate::calendar::Periods;
use crate::history::{Posting, postings};
use crate::lens::Lens;

/// What each top-level spending purpose cost in each full month of history,
/// leaving out the flows that contracts and habits already project. This is the
/// variable part of spending, the part the bands bootstrap.
pub struct Variable {
    /// Category-major rows in one allocation, each with contiguous monthly
    /// values, in base quanta.
    pub amounts: Vec<i64>,
    /// Category row indices, sorted by the category id for reproducibility.
    pub categories: Vec<usize>,
    pub months: usize,
}

impl Variable {
    /// History runs from the first month anything was spent to the last full
    /// month: the current month is still going, and months before the books
    /// had any spending would only dilute it.
    pub fn from_history(lens: Lens, run: &Run, explained: impl Fn(&Flow) -> bool) -> Variable {
        purpose_history(lens, run, explained)
    }
}

/// Spending history grouped under the first child of the spending root. A
/// recognized flow is spread across the months it belongs to, so a refund
/// offsets its purpose in the month it is recognized.
fn purpose_history(lens: Lens, run: &Run, explained: impl Fn(&Flow) -> bool) -> Variable {
    let book = lens.book;
    let none = Variable { amounts: Vec::new(), categories: Vec::new(), months: 0 };
    let first = postings(book, run)
        .filter(|posting| posting.is_real_on(run.today) && !explained(posting.flow))
        .filter_map(|posting| spending_purpose(lens, posting).map(|_| posting.flow.recognized.first()))
        .min();
    let Some(first) = first else { return none };
    let last_full_month = run.today.month_start().add_days(-1);
    if first > last_full_month {
        return none;
    }

    let months = Periods::covering(Period::Month, first, last_full_month);
    let mut categories: BTreeMap<Id<Purpose>, usize> = BTreeMap::new();
    let mut amounts = Vec::new();
    for posting in postings(book, run).filter(|posting| posting.is_real_on(last_full_month) && !explained(posting.flow)) {
        let Some((category, amount)) = spending_purpose(lens, posting) else { continue };
        for month in months.overlapping(posting.flow.recognized.first(), posting.flow.recognized.last()) {
            let window = months.window(month).days();
            let Some(happened) = Days::new(window.first(), window.last().min(last_full_month)) else {
                continue;
            };
            let part = spread(amount, posting.flow.recognized, happened);
            let index = *categories.entry(category).or_insert_with(|| {
                let index = amounts.len() / months.len();
                amounts.resize((index + 1) * months.len(), 0);
                index
            });
            amounts[index * months.len() + month] += part.0;
        }
    }
    Variable { amounts, categories: categories.into_values().collect(), months: months.len() }
}

/// The first purpose beneath `spending` and this flow's signed amount, if the
/// flow moved value and belongs to the selected owner scope.
fn spending_purpose(lens: Lens, posting: &Posting) -> Option<(Id<Purpose>, Qty)> {
    let book = lens.book;
    let flow = posting.flow;
    if !lens.whose.includes(flow.owner) || !flow.moves_quantity(End::From) {
        return None;
    }
    let purpose = flow.purpose?.purpose;
    let spending = book.roots.purposes.spending;
    if book.purposes[purpose].root != PurposeRoot::Spending {
        return None;
    }
    let mut category = purpose;
    while let Some(parent) = book.purposes.parent(category) {
        if parent == spending {
            break;
        }
        category = parent;
    }
    let amount = crate::flow::movement_in_base(lens, *posting, Some(PurposeRoot::Spending))?;
    Some((category, amount))
}

#[cfg(test)]
mod tests {
    use axiom_core::Day;

    use super::*;
    use crate::lens::Whose;
    use crate::source_tests::with_run;

    #[test]
    fn native_variability_uses_top_level_spending_purposes() {
        let source = "\
base USD
commodity USD
  precision 2
entity me
entity grocer
account checking : asset
purpose food : spending
purpose groceries : food

opening 2026-01-01
  checking 5_000.00 USD

2026-01-10 checking -> grocer 100.00 USD #groceries
2026-02-10 checking -> grocer 100.00 USD #groceries
2026-03-10 checking -> grocer 100.00 USD #groceries
2026-04-10 checking -> grocer 100.00 USD #groceries
";
        with_run(source, Day::from_ymd(2026, 5, 15).unwrap(), |book, run| {
            let whose = Whose::default();
            let plan = axiom_engine::Plan::new(book);
            let variable = Variable::from_history(Lens::new(&plan, &whose, run.today), run, |_| false);
            assert_eq!(variable.categories, [0]);
            assert_eq!(variable.amounts, [10_000; 4]);
            assert_eq!(variable.months, 4);
        });
    }
}
