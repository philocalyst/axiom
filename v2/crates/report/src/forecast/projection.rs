//! Running the forecast: expected flows, folded through a ledger.
//!
//! The projection is a real fold, not arithmetic beside one. Every expected
//! flow is applied to the ledger in date order, so laws run on the future
//! exactly as they run on the past: a limit that will be crossed in November,
//! or a deadline that will pass, is recorded by the same rules.

use std::collections::BTreeMap;

use axiom_core::{Arena, Day, Id, Qty, Ratio};
use axiom_engine::{Checkpoint, Holding, Ledger, Options, Plan};
use axiom_model::{Book, Class, Commodity, Flow, Place, RuntimeDetail, RuntimeFlow, Value};

use crate::history::Held;
use crate::lens::{Basket, Lens, Liquidity};

/// A cash place that goes below zero.
pub struct Overdraft {
    pub place: Id<Place>,
    pub first: Day,
    pub lowest: Qty,
}

/// What running the expected flows produced.
pub struct Trace<'p, 'b, 's> {
    /// Everything the ledger recorded, history and projection alike.
    pub ledger: Ledger<'p, 'b, 's>,
    /// Money in hand less the debts with no term, at each checkpoint.
    pub liquid: Vec<Qty>,
    /// Everything owned less everything owed at each checkpoint, with
    /// commodities grown by their models.
    pub worth: Vec<Qty>,
    pub overdrafts: Vec<Overdraft>,
}

/// Applies `flows` (in date order) to a ledger standing at `today`, reading
/// the position at each checkpoint. Laws with deadlines fire all the way to
/// the last checkpoint.
pub fn project<'p, 'b, 's>(
    plan: &'p Plan<'b, 's>,
    lens: Lens<'b, 's, '_, '_>,
    today: Day,
    flows: Vec<Flow>,
    checkpoints: &[Day],
) -> Trace<'p, 'b, 's> {
    let details = Arena::default();
    project_runtime(
        plan,
        lens,
        today,
        flows.into_iter().map(RuntimeFlow::source).collect(),
        &details,
        checkpoints,
    )
}

/// Projects typed runtime flows using the detail arena that owns their
/// overrides. The ledger consumes the same representation as contract
/// occurrences and never treats a runtime transaction as a Book index.
pub(crate) fn project_runtime<'p, 'b, 's>(
    plan: &'p Plan<'b, 's>,
    lens: Lens<'b, 's, '_, '_>,
    today: Day,
    flows: Vec<RuntimeFlow>,
    details: &Arena<RuntimeDetail>,
    checkpoints: &[Day],
) -> Trace<'p, 'b, 's> {
    let book = lens.book();
    let horizon = checkpoints.last().copied().unwrap_or(today);
    let mut ledger = plan.start(Options {
        today: horizon,
        relaxed: book.relaxed,
    });
    ledger.advance(today);

    trace_from(ledger, lens, today, flows, details, checkpoints)
}

/// Projects from the view checkpoint paired with `plan` and `lens`.
///
/// The checkpoint is the journal state before its day's closings. Advancing to
/// `today` closes that boundary before the forecast starts, matching
/// [`project`]; expected flows are then judged against the same ledger state
/// without folding the book a second time. `relaxed` is explicit because a
/// client may run a relaxed view even when the serialized book's default is
/// strict.
pub(crate) fn project_from<'p, 'b, 's>(
    plan: &'p Plan<'b, 's>,
    checkpoint: &Checkpoint,
    lens: Lens<'b, 's, '_, '_>,
    today: Day,
    relaxed: bool,
    flows: Vec<Flow>,
    checkpoints: &[Day],
) -> Trace<'p, 'b, 's> {
    let details = Arena::default();
    project_runtime_from(
        plan,
        checkpoint,
        lens,
        today,
        relaxed,
        flows.into_iter().map(RuntimeFlow::source).collect(),
        &details,
        checkpoints,
    )
}

/// Resumes from a prepared checkpoint and folds engine-materialized future
/// occurrences through the same runtime flow path used by the canonical run.
pub(crate) fn project_runtime_from<'p, 'b, 's>(
    plan: &'p Plan<'b, 's>,
    checkpoint: &Checkpoint,
    lens: Lens<'b, 's, '_, '_>,
    today: Day,
    relaxed: bool,
    flows: Vec<RuntimeFlow>,
    details: &Arena<RuntimeDetail>,
    checkpoints: &[Day],
) -> Trace<'p, 'b, 's> {
    debug_assert!(
        checkpoint.day() <= today,
        "projection cannot rewind its view checkpoint"
    );
    let horizon = checkpoints.last().copied().unwrap_or(today);
    let options = Options {
        today: horizon,
        relaxed,
    };
    let mut ledger = plan.resume(checkpoint, options);
    ledger.advance(today);

    trace_from(ledger, lens, today, flows, details, checkpoints)
}

/// The shared forecast fold once a ledger has reached the end of `today`.
fn trace_from<'p, 'b, 's>(
    mut ledger: Ledger<'p, 'b, 's>,
    lens: Lens<'b, 's, '_, '_>,
    today: Day,
    flows: Vec<RuntimeFlow>,
    details: &Arena<RuntimeDetail>,
    checkpoints: &[Day],
) -> Trace<'p, 'b, 's> {
    let mut overdrawn: BTreeMap<Id<Place>, Overdraft> = BTreeMap::new();
    let (mut liquid, mut worth) = (Vec::new(), Vec::new());
    let mut coming = flows.into_iter().peekable();
    for &checkpoint in checkpoints {
        while let Some(flow) = coming.next_if(|flow| flow.flow.day <= checkpoint) {
            let Some(flow) = within_means(lens, &ledger, flow) else {
                continue;
            };
            ledger.apply_runtime(&flow, details);
            note_overdrafts(lens, &ledger, &flow.flow, &mut overdrawn);
        }
        ledger.advance(checkpoint);
        let months = checkpoint.since(today).months;
        let position = |pick: &dyn Fn(&Holding) -> Qty| grown(lens, months, &ledger, pick);
        liquid.push(position(&|holding| in_hand_or_owed(lens, holding)));
        worth.push(position(&|holding| holding.qty()));
    }
    Trace {
        ledger,
        liquid,
        worth,
        overdrafts: overdrawn.into_values().collect(),
    }
}

/// What a holding adds to what can be spent: its free money, less what is
/// owed on debts with no term (a card, a tab). A loan with a term is paid by
/// the payments the projection already makes.
fn in_hand_or_owed(lens: Lens, holding: &Holding) -> Qty {
    let place = &lens.book().places[holding.place];
    let has_term = lens.known().maturity.is_some_and(|name| {
        place
            .props
            .iter()
            .any(|prop| prop.name == name && matches!(prop.value, Value::Day(_)))
    });
    match lens.liquidity(holding.place, holding.unit) {
        Some(Liquidity::Cash) => lens.free(holding),
        _ if place.class == Class::Debt && !has_term => holding.qty(),
        _ => Qty::ZERO,
    }
}

/// What the lens's owners hold on the balance sheet, as `pick` counts it,
/// each commodity priced as a whole at today's prices and grown `months` ahead.
fn grown(lens: Lens, months: i32, ledger: &Ledger, pick: &dyn Fn(&Holding) -> Qty) -> Qty {
    let mut basket = Basket::default();
    let on_sheet = |holding: &&Holding| {
        lens.owns(holding.place)
            && matches!(
                lens.book().places[holding.place].class,
                Class::Asset | Class::Debt
            )
    };
    for holding in ledger.holdings().filter(on_sheet) {
        basket.add(
            holding.unit,
            Held {
                qty: pick(holding),
                booked: Qty::ZERO,
            },
        );
    }
    basket
        .amounts()
        .filter_map(|amount| {
            Some(compound(
                lens.book(),
                amount.unit,
                lens.value(amount)?,
                months,
            ))
        })
        .sum()
}

/// A flow that cannot move more than its ends hold: what leaves an account
/// that is not cash is limited by what it holds, and a payment into a debt by
/// what is owed. `None` when there is nothing to move.
fn within_means(lens: Lens, ledger: &Ledger, mut flow: RuntimeFlow) -> Option<RuntimeFlow> {
    let movement = &flow.flow;
    let (from, to, out_unit, arrive_unit, exchange, amount) = (
        movement.from,
        movement.to,
        movement.out.unit,
        movement.arrive.unit,
        movement.is_exchange(),
        movement.out.qty,
    );
    // Slow holdings and claims cannot move more than they currently hold.
    let held_back = matches!(
        lens.liquidity(from, out_unit),
        Some(Liquidity::Slow(_) | Liquidity::Claim)
    );
    let room = if held_back {
        Some(ledger.balance(from, out_unit))
    } else if lens.book().places[to].class == Class::Debt {
        Some(-ledger.balance(to, arrive_unit))
    } else {
        None
    };
    match room {
        Some(room) if !exchange && room < amount => {
            if room <= Qty::ZERO {
                None
            } else {
                flow.flow.out.qty = room;
                flow.flow.arrive.qty = room;
                Some(flow)
            }
        }
        _ => Some(flow),
    }
}

/// Records where a flow left a cash place below zero.
fn note_overdrafts(
    lens: Lens,
    ledger: &Ledger,
    flow: &Flow,
    overdrawn: &mut BTreeMap<Id<Place>, Overdraft>,
) {
    let book = lens.book();
    let cash = |&place: &Id<Place>| lens.liquidity(place, book.base) == Some(Liquidity::Cash);
    for place in [flow.from, flow.to].into_iter().filter(cash) {
        let balance = ledger.balance(place, book.base);
        if balance.is_negative() {
            let overdraft = overdrawn.entry(place).or_insert(Overdraft {
                place,
                first: flow.day,
                lowest: balance,
            });
            overdraft.lowest = overdraft.lowest.min(balance);
        }
    }
}

/// A commodity that `grows 5% yearly` is priced 5%/12 higher each month
/// ahead, compounding, rounded half to even each month. The base currency is
/// the yardstick and does not grow.
fn compound(book: &Book, unit: Id<Commodity>, value: Qty, months: i32) -> Qty {
    let yearly = book.commodities[unit].growth.filter(|_| unit != book.base);
    let monthly =
        yearly.and_then(|yearly| Ratio::ONE.checked_add(yearly.checked_div(Ratio::int(12))?));
    monthly.map_or(value, |factor| {
        (0..months).fold(value, |worth, _| worth.scale(factor).unwrap_or(worth))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::household;
    use axiom_core::Days;
    use axiom_model::Amount;

    #[test]
    fn growth_compounds_monthly_and_leaves_the_yardstick_alone() {
        let mut house = household();
        let (usd, vti) = (house.book.base, Id::new(1));
        house.book.commodities[vti].growth = Ratio::percent(5, 0);
        // 100,000.00 at 241/240 a month for a year, rounded half-even each month.
        assert_eq!(
            compound(&house.book, vti, Qty(10_000_000), 12),
            Qty(10_511_619)
        );
        assert_eq!(
            compound(&house.book, usd, Qty(10_000_000), 12),
            Qty(10_000_000)
        );
        assert_eq!(
            compound(&house.book, vti, Qty(10_000_000), 0),
            Qty(10_000_000)
        );
    }

    #[test]
    fn projection_resumes_the_supplied_checkpoint_without_refolding() {
        let house = household();
        let plan = Plan::new(&house.book);
        let today = house.run.today;
        let options = Options {
            today,
            relaxed: false,
        };
        let (_, mut view) = plan.run_with_view(options);

        // Add a hypothetical salary to the checkpoint. A fresh fold of the
        // book cannot contain this amount, so the resumed projection must.
        let tomorrow = today.add_days(1);
        let mut salary = house.book.flows[Id::new(0)].clone();
        let checking = house.place("assets/bank/checking");
        salary.day = tomorrow;
        salary.recognized = Days::on(tomorrow);
        salary.from = house.place("income/salary");
        salary.to = checking;
        salary.out = Amount::new(Qty(1_000), house.book.base);
        salary.arrive = salary.out;
        view.apply(&salary);
        let checkpoint = view.checkpoint();

        let whose = crate::lens::Whose::default();
        let lens = Lens::new(&plan, &whose, tomorrow);
        let resumed = project_from(
            &plan,
            &checkpoint,
            lens,
            tomorrow,
            false,
            Vec::new(),
            &[tomorrow],
        );
        let folded = project(&plan, lens, tomorrow, Vec::new(), &[tomorrow]);

        assert_eq!(resumed.liquid[0] - folded.liquid[0], Qty(1_000));
        assert_eq!(resumed.worth[0] - folded.worth[0], Qty(1_000));
    }
}
