//! How much everything else varies: the spending nobody planned.

use std::collections::BTreeMap;

use axiom_core::{Days, Id, Qty, spread};
use axiom_engine::Run;
use axiom_model::{End, Flow, Period, Place, Purpose, PurposeRoot};

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
        if book.flows.iter().any(|(_, flow)| {
            flow.purpose
                .is_some_and(|purpose| book.purposes[purpose.purpose].root == PurposeRoot::Spending)
        }) {
            return purpose_history(lens, run, explained);
        }
        // v3 bridge: expense places are the only classification on legacy books.
        place_history(lens, run, explained)
    }
}

fn place_history(lens: Lens, run: &Run, explained: impl Fn(&Flow) -> bool) -> Variable {
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

/// Spending history grouped under the first child of the spending root. A
/// recognized flow is spread across the months it belongs to, so a refund
/// offsets its purpose in the month it is recognized.
fn purpose_history(lens: Lens, run: &Run, explained: impl Fn(&Flow) -> bool) -> Variable {
    let book = lens.book;
    let none = Variable { categories: Vec::new(), months: 0 };
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
    let mut categories: BTreeMap<Id<Purpose>, Vec<i64>> = BTreeMap::new();
    for posting in postings(book, run).filter(|posting| posting.is_real_on(last_full_month) && !explained(posting.flow)) {
        let Some((category, amount)) = spending_purpose(lens, posting) else { continue };
        for month in months.overlapping(posting.flow.recognized.first(), posting.flow.recognized.last()) {
            let window = months.window(month).days();
            let Some(happened) = Days::new(window.first(), window.last().min(last_full_month)) else {
                continue;
            };
            let part = spread(amount, posting.flow.recognized, happened);
            categories.entry(category).or_insert_with(|| vec![0; months.len()])[month] += part.0;
        }
    }
    Variable { categories: categories.into_values().collect(), months: months.len() }
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
            let variable = Variable::from_history(Lens::new(book, &whose, run.today), run, |_| false);
            assert_eq!(variable.categories, [vec![10_000; 4]]);
            assert_eq!(variable.months, 4);
        });
    }
}
