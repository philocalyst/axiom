//! What the laws of a promise derive from one of its occurrences.
//!
//! A law written in a contract with a `derive` step (an `also` is one) fires once for each group of an occurrence,
//! when the group's flows are made and before any of them posts. The group's header is the flow that fired it, as the
//! template gave it (`amount` is what it was before a leg or an item took from it), and `[end]` selects from what the
//! group made. What the law derives then **joins the group**: a flow of its own is one more flow of the occurrence,
//! and an item is solved against what the header has left, by the same [`solve`] that solved the template's own, so a
//! carved item takes from the header and an added one does not.
//!
//! The derived flows are pushed into the pools every flow of the occurrence goes through, so a forecast, which is
//! the same `post_occurrence`, derives what a kept occurrence does, and everything that reads the occurrence's flows
//! (`register`, `why`, the monitor's oracle) reads them with the rest.

use axiom_core::{Arena, Day, Id};
use axiom_model::{
    Amount, Bear, Book, Commodity, Cut, Derivation, Derived, Expr, Failed, Flow, FlowSide, Infer, Law, LiteralEnv,
    Origin, Remainder, Remaining, RuntimeDetail, RuntimeFlow, Shape, Sign, Watch, solve,
};

use super::{Cx, Pools, TemplateError};
use crate::Cause;
use crate::eval::{self, Context, Env, Occasion, Outcome};
use crate::ledger::Ledger;
use crate::motion::{Amounts, Motion};
use crate::scope::owner_of;

/// A flow a law derived: what the law says it is, how much it came to, and which law said so.
#[derive(Clone, Copy)]
struct Made {
    law: Id<Law>,
    template: Id<Derived>,
    amount: Amount,
}

impl Ledger<'_, '_, '_> {
    /// The laws that derive for this occurrence's contract fire for the group whose flows begin at `first`, whose
    /// header was `given` before the group was solved; what they derive joins the group.
    pub(super) fn derive_group(
        &mut self,
        cx: &Cx<'_>,
        given: Remaining,
        first: usize,
        pools: &mut Pools<'_>,
    ) -> Result<(), TemplateError> {
        let rules = self.plan.book.rules.at(Watch::Occurrence(cx.making.contract));
        if rules.is_empty() {
            return Ok(());
        }
        let mut made = Vec::new();
        for rule in rules {
            made.extend(self.fire_derive(rule.law, cx, given, &pools.flows[first..], pools.details)?);
        }
        self.join(cx, &made, first, pools)
    }

    /// One law fired for the occurrence: what its `derive` steps made, in order.
    fn fire_derive(
        &mut self,
        law: Id<Law>,
        cx: &Cx<'_>,
        given: Remaining,
        group: &[RuntimeFlow],
        details: &Arena<RuntimeDetail>,
    ) -> Result<Vec<Made>, TemplateError> {
        let (plan, book) = (self.plan, self.plan.book);
        let motion = header_motion(book, &group[0], details, cx.making.source_day);
        let occasion = Occasion { amount: Some(given.out), ..Occasion::flow(&motion) };
        let subject = axiom_model::Subject::Contract(cx.making.contract);
        let context = Context::new(subject, owner_of(book, subject), &occasion)
            .for_law(law)
            .with_inputs(cx.making.inputs)
            .with_template_flows(group, details);
        let mut outcomes = std::mem::take(&mut self.scratch.outcomes);
        outcomes.clear();
        let env = Env { plan, world: &self.world };
        eval::run(
            env,
            &book.laws[law],
            &context,
            &mut self.scratch.values,
            &mut self.scratch.budget_values,
            &mut outcomes,
        );
        let made = made_by(self.plan.book, law, &mut outcomes);
        self.scratch.outcomes = outcomes;
        made
    }

    /// What was derived joins the group: the items are solved against what the header has left, and what makes a flow
    /// is pushed after the group's own flows.
    fn join(&mut self, cx: &Cx<'_>, made: &[Made], first: usize, pools: &mut Pools<'_>) -> Result<(), TemplateError> {
        let book = self.plan.book;
        let unit = pools.flows[first].flow.amount_at(cx.group.template.side.end()).unit;
        self.take_items(cx.group.template.side, unit, made, &mut pools.flows[first].flow)?;
        let mut ordinal = pools.flows.last().map_or(0, |flow| flow.ordinal + 1);
        for made in made {
            let derived = &book.derived[made.template];
            if !derived.makes_flow() {
                continue;
            }
            let flow = self.derived_flow(&pools.flows[first].flow, derived, *made);
            pools.flows.push(RuntimeFlow { flow, detail: None, ordinal, txn: cx.making.txn });
            ordinal += 1;
        }
        Ok(())
    }

    /// What the derived items take from the header: the same [`solve`] that solved the template's own items, which
    /// carves a carved item and a `-` item with no purpose out of the header, and leaves an added one on top of it.
    fn take_items(
        &self,
        side: FlowSide,
        unit: Id<Commodity>,
        made: &[Made],
        header: &mut Flow,
    ) -> Result<(), TemplateError> {
        let book = self.plan.book;
        let items = made.iter().filter(|made| matches!(book.derived[made.template].shape, Shape::Item(_)));
        let bears: Vec<Bear> = items
            .clone()
            .map(|made| {
                let derived = &book.derived[made.template];
                let Shape::Item(sign) = derived.shape else { unreachable!("filtered to items") };
                let takes = sign == Sign::Carve || (sign == Sign::Less && derived.purpose.is_none());
                Bear { amount: Cut::Of(Expr::Literal(made.amount)), side, unit, takes }
            })
            .collect();
        let left = Remaining { out: header.out, arrive: header.arrive };
        let solved =
            solve(Some(left), &[], &bears, Remainder::BeforeItems, &mut LiteralEnv).map_err(|failed| match failed {
                Failed::Fault { at, fault } => {
                    let law = items.clone().nth(item_of(at)).map(|made| &book.laws[made.law]);
                    TemplateError::Expression { fault, loc: law.map_or(header.loc, |law| law.loc) }
                }
                Failed::Env(never) => match never {},
                Failed::TwoRests { .. } => TemplateError::InvalidTemplate { loc: header.loc },
            })?;
        if let Some(left) = solved.header {
            (header.out, header.arrive) = (left.out, left.arrive);
        }
        Ok(())
    }

    /// The flow a `derive` makes: along the header's ends (reversed for a `-` item) or the ends it names, the amount
    /// the law came to, and what the line says of it. What it does not say is the header's, as for a leg or an item the
    /// template writes (the contract's party is its payee, whatever end it is paid to). A header carries no waiver, so
    /// there is none to keep.
    fn derived_flow(&self, header: &Flow, derived: &Derived, made: Made) -> Flow {
        let (from, to) = match derived.shape {
            Shape::Flow { from, to } => (from.unwrap_or(header.from), to.unwrap_or(header.to)),
            Shape::Item(Sign::Less) => (header.to, header.from),
            Shape::Item(Sign::Add | Sign::Carve) => (header.from, header.to),
        };
        Flow {
            from,
            to,
            out: made.amount,
            arrive: made.amount,
            infer: Infer::Known,
            owner: derived.owner.unwrap_or(header.owner),
            purpose: derived.purpose.or(header.purpose),
            description: derived.description.or(header.description),
            codes: derived.codes,
            select: derived.select,
            detail: derived.detail,
            waive: derived.waive,
            loc: derived.loc,
            origin: Origin::Derived(Derivation::Law(made.law)),
            ..header.clone()
        }
    }
}

/// The header of a group seen as a flow that has moved, which is what a law that fires for the group reads.
fn header_motion<'a>(
    book: &'a Book,
    header: &'a RuntimeFlow,
    details: &'a Arena<RuntimeDetail>,
    day: Day,
) -> Motion<'a> {
    let cause = Cause::Applied(header.ordinal);
    let (view, amounts) = (book.runtime_flow_view(header, details), Amounts::written(&header.flow));
    Motion::from_view_at(book, view, header.txn, cause, day, amounts, header.ordinal)
}

/// What a law's steps made, in order, or the first thing that went wrong in them.
fn made_by(book: &Book, law: Id<Law>, outcomes: &mut Vec<Outcome>) -> Result<Vec<Made>, TemplateError> {
    outcomes
        .drain(..)
        .map(|outcome| match outcome {
            Outcome::Derive { template, amount, .. } => Ok(Made { law, template, amount }),
            Outcome::Faulted { step, fault } => {
                Err(TemplateError::Expression { fault, loc: book.laws[law].steps[step as usize].loc })
            }
            _ => Err(TemplateError::InvalidTemplate { loc: book.laws[law].loc }),
        })
        .collect()
}

/// Which item of the group a solver failure is about.
fn item_of(at: axiom_model::Line) -> usize {
    match at {
        axiom_model::Line::Item(index) | axiom_model::Line::Leg(index) => index,
        axiom_model::Line::Header(_) => 0,
    }
}

#[cfg(test)]
mod tests {
    use axiom_core::Qty;
    use axiom_model::{Book, Derivation, Origin};

    use crate::source_tests::{day, with_book, with_run};
    use crate::{Ledger, Options, Plan, Run};

    const PRELUDE: &str = "\
base USD
commodity USD
  precision 2
commodity EUR
  precision 2
purpose fee : spending
purpose refund : income
purpose match : transfer
entity lender
entity acme
account checking : asset
account escrow : asset
account k401 : asset
opening 2026-01-01
  checking 10_000.00 USD
";

    /// A monthly payment of 1,000.00 USD from `checking` on the first, with `law` written under it, kept in
    /// January and February.
    fn mortgage(law: &str) -> String {
        format!(
            "{PRELUDE}contract mortgage with lender\n  1_000.00 USD monthly on 1 from checking\n  from 2026-01-01\n  law derived\n    on flow\n{law}2026-01-01 mortgage\n2026-02-01 mortgage\n"
        )
    }

    fn held(book: &Book, run: &Run, place: &str) -> Qty {
        let place = book.place(place).unwrap();
        run.holdings.iter().find(|holding| holding.place == place).map_or(Qty::ZERO, |holding| holding.qty())
    }

    fn after(law: &str) -> (Qty, Qty) {
        with_run(&mortgage(law), day(2026, 3, 1), |book, run| (held(book, run, "checking"), held(book, run, "escrow")))
    }

    #[test]
    fn a_flow_of_its_own_joins_every_occurrence() {
        let (checking, escrow) = after("    derive -> escrow 100.00 USD #match\n");
        assert_eq!(
            (checking, escrow),
            (Qty(10_000_00 - 2 * 1_100_00), Qty(2 * 100_00)),
            "the end it leaves is the header's"
        );
    }

    #[test]
    fn the_derived_flows_are_flows_of_the_occurrence_and_say_which_law_made_them() {
        with_run(&mortgage("    derive -> escrow 100.00 USD #match\n"), day(2026, 3, 1), |book, run| {
            let promise = &run.promises[0];
            let flows = promise.flows.get(&run.promised_flows).unwrap();
            assert_eq!(flows.len(), 2, "the payment, and what the law derived");
            let derived = &flows[1].flow;
            assert_eq!(derived.to, book.place("escrow").unwrap());
            assert!(matches!(derived.origin, Origin::Derived(Derivation::Law(_))));
            assert_eq!(derived.purpose.unwrap().purpose, book.purpose("match").unwrap());
            assert_eq!(flows[1].ordinal, flows[0].ordinal + 1, "it comes after the occurrence's own flows");
        });
    }

    #[test]
    fn an_added_item_comes_on_top_and_a_carved_one_is_taken_from_the_header() {
        let (checking, _) = after("    derive + 5% of amount #fee\n");
        assert_eq!(checking, Qty(10_000_00 - 2 * 1_050_00), "5% more leaves the same end");
        let (checking, _) = after("    derive 5% of amount #fee\n");
        assert_eq!(checking, Qty(10_000_00 - 2 * 1_000_00), "5% of the 1,000.00 is the fee, and the header is 950.00");
        with_run(&mortgage("    derive 5% of amount #fee\n"), day(2026, 3, 1), |_, run| {
            let flows = run.promises[0].flows.get(&run.promised_flows).unwrap();
            let amounts: Vec<_> = flows.iter().map(|flow| flow.flow.out.qty).collect();
            assert_eq!(amounts, [Qty(950_00), Qty(50_00)]);
        });
    }

    #[test]
    fn a_less_item_goes_back_along_the_reversed_ends_and_one_with_no_purpose_only_takes() {
        let (checking, _) = after("    derive - 2% of amount #refund\n");
        assert_eq!(checking, Qty(10_000_00 - 2 * 980_00), "20.00 comes back");
        let (checking, _) = after("    derive - 2% of amount\n");
        assert_eq!(checking, Qty(10_000_00 - 2 * 980_00), "the header is 980.00 and nothing came back");
        with_run(&mortgage("    derive - 2% of amount\n"), day(2026, 3, 1), |_, run| {
            assert_eq!(
                run.promises[0].flows.get(&run.promised_flows).unwrap().len(),
                1,
                "an item with no purpose makes no flow"
            );
        });
    }

    #[test]
    fn a_derived_flow_that_says_no_purpose_is_for_what_its_header_is_and_has_its_payee() {
        let text = format!(
            "{PRELUDE}contract mortgage with lender\n  1_000.00 USD monthly on 1 from checking #fee\n  from 2026-01-01\n  law derived\n    on flow\n    derive -> escrow 100.00 USD\n    derive -> k401 50.00 USD #match\n2026-01-01 mortgage\n"
        );
        with_run(&text, day(2026, 1, 31), |book, run| {
            let flows = run.promises[0].flows.get(&run.promised_flows).unwrap();
            let purposes: Vec<_> = flows.iter().map(|flow| flow.flow.purpose.unwrap().purpose).collect();
            let (fee, matched) = (book.purpose("fee").unwrap(), book.purpose("match").unwrap());
            assert_eq!(purposes, [fee, fee, matched], "the first says none, so it is the payment's");
            let payees: Vec<_> = flows.iter().map(|flow| flow.flow.payee).collect();
            assert_eq!(
                payees,
                [Some(book.entity("lender").unwrap()); 3],
                "the contract's party, whatever it is paid to"
            );
        });
    }

    #[test]
    fn a_derived_flow_keeps_the_headers_description_unless_it_says_its_own() {
        let text = format!(
            "{PRELUDE}contract mortgage with lender\n  1_000.00 USD monthly on 1 from checking #fee \"the payment\"\n  from 2026-01-01\n  law derived\n    on flow\n    derive -> escrow 100.00 USD #match\n    derive -> k401 50.00 USD #match \"its own\"\n2026-01-01 mortgage\n"
        );
        with_run(&text, day(2026, 1, 31), |book, run| {
            let flows = run.promises[0].flows.get(&run.promised_flows).unwrap();
            let says: Vec<_> = flows.iter().map(|flow| flow.flow.description.map(|text| book.text(text))).collect();
            assert_eq!(says, [Some("the payment"), Some("the payment"), Some("its own")]);
        });
    }

    #[test]
    fn a_share_is_carved_from_the_header_and_borne_by_its_entity() {
        let text = format!(
            "{PRELUDE}contract bill with lender\n  1_000.00 USD monthly on 1 from checking #fee\n  from 2026-01-01\n  share 60% for acme\n2026-01-01 bill\n"
        );
        with_run(&text, day(2026, 1, 31), |book, run| {
            let flows = run.promises[0].flows.get(&run.promised_flows).unwrap();
            let (owners, amounts): (Vec<_>, Vec<_>) =
                flows.iter().map(|flow| (flow.flow.owner, flow.flow.out.qty)).unzip();
            let (me, acme) = (book.roots.me, book.entity("acme").unwrap());
            assert_eq!(
                (owners, amounts),
                (vec![me, acme], vec![Qty(400_00), Qty(600_00)]),
                "60% of the 1,000.00 is acme's"
            );
            let fee = book.purpose("fee").unwrap();
            assert_eq!(flows[1].flow.purpose.unwrap().purpose, fee, "of the same purpose");
            assert_eq!(held(book, run, "checking"), Qty(9_000_00), "and nothing more leaves the account");
        });
    }

    #[test]
    fn a_when_that_does_not_hold_derives_nothing() {
        let (_, escrow) = after("    when value(amount, USD) > 5_000.00 USD\n    derive -> escrow 100.00 USD #match\n");
        assert_eq!(escrow, Qty::ZERO);
        let (_, escrow) = after("    when value(amount, USD) > 500.00 USD\n    derive -> escrow 100.00 USD #match\n");
        assert_eq!(escrow, Qty(200_00));
    }

    #[test]
    fn an_amount_that_reads_the_occurrence_sees_the_header_as_given_and_the_flows_the_group_made() {
        // The header is 5,000.00 with 500.00 to the 401(k): a match of 40% of the smaller of that and 10% of the gross.
        let text = format!(
            "{PRELUDE}contract pay with acme\n  5_000.00 USD monthly on 1 into checking\n  from 2026-01-01\n  k401 500.00 USD #match\n  checking ...\n  law match\n    on flow\n    derive acme -> k401 40% of ([k401] up to 10% of amount) #match\n2026-01-01 pay\n"
        );
        with_run(&text, day(2026, 1, 31), |book, run| {
            assert_eq!(held(book, run, "k401"), Qty(500_00 + 200_00), "40% of min(500.00, 500.00)");
            assert_eq!(held(book, run, "checking"), Qty(10_000_00 + 4_500_00));
        });
        let low = text.replace("10% of amount", "5% of amount");
        with_run(&low, day(2026, 1, 31), |book, run| {
            assert_eq!(held(book, run, "k401"), Qty(500_00 + 100_00), "40% of the cap of 250.00");
        });
    }

    #[test]
    fn a_law_fires_for_the_header_and_reads_its_ends_whatever_legs_the_group_has() {
        let text = format!(
            "{PRELUDE}contract pay with acme\n  5_000.00 USD monthly on 1 into checking\n  from 2026-01-01\n  k401 500.00 USD #match\n  checking ...\n  law derived\n    on flow\n    when to is checking\n    derive -> escrow 10.00 USD #match\n2026-01-01 pay\n"
        );
        with_run(&text, day(2026, 1, 31), |book, run| {
            assert_eq!(
                held(book, run, "escrow"),
                Qty(10_00),
                "the header goes to checking; the leg, which goes to k401, is not what fired it"
            );
        });
    }

    #[test]
    fn an_item_derived_for_a_standing_buy_is_in_what_is_spent_not_in_what_is_bought() {
        let text = format!(
            "{PRELUDE}commodity VTI\ncontract invest with lender\n  buy VTI for 500.00 USD monthly on 15 from checking\n  from 2026-01-01\n  law derived\n    on flow\n    derive + 2.00 USD #fee\n2026-01-02 VTI = 100.00 USD\n2026-01-15 invest 5 VTI\n"
        );
        with_run(&text, day(2026, 1, 31), |book, run| {
            let said: Vec<_> = run
                .diagnostics
                .iter()
                .map(|diagnostic| (diagnostic.code.to_string(), diagnostic.message.clone()))
                .collect();
            assert_eq!(
                held(book, run, "checking"),
                Qty(10_000_00 - 500_00 - 2_00),
                "the 5 VTI cost 500.00, and the fee is 2.00 more: {said:?}"
            );
            assert!(
                run.diagnostics.iter().all(|diagnostic| diagnostic.severity != axiom_core::Severity::Error),
                "{:?}",
                run.diagnostics
            );
        });
    }

    #[test]
    fn a_selection_that_finds_nothing_derives_nothing() {
        let text = format!(
            "{PRELUDE}contract pay with acme\n  5_000.00 USD monthly on 1 into checking\n  from 2026-01-01\n  law match\n    on flow\n    derive acme -> k401 40% of ([escrow] up to 10% of amount) #match\n2026-01-01 pay\n"
        );
        with_run(&text, day(2026, 1, 31), |book, run| {
            assert_eq!(held(book, run, "k401"), Qty::ZERO, "nothing went to escrow, so there is nothing to match");
            assert_eq!(run.promises[0].flows.get(&run.promised_flows).unwrap().len(), 1);
        });
    }

    #[test]
    fn a_law_derives_only_for_its_own_contract() {
        let text = format!(
            "{PRELUDE}contract mortgage with lender\n  1_000.00 USD monthly on 1 from checking\n  from 2026-01-01\n  law derived\n    on flow\n    derive -> escrow 100.00 USD #match\ncontract rent with lender\n  300.00 USD monthly on 1 from checking\n  from 2026-01-01\n2026-01-01 mortgage\n2026-01-01 rent\n"
        );
        with_run(&text, day(2026, 1, 31), |book, run| assert_eq!(held(book, run, "escrow"), Qty(100_00)));
    }

    #[test]
    fn a_forecast_derives_what_a_kept_occurrence_does() {
        // January and February are written; the forecast from 02-15 promises March and April, and derives with them.
        with_book(&mortgage("    derive -> escrow 100.00 USD #match\n"), |book| {
            let plan = Plan::new(book);
            let mut ledger: Ledger = plan.start(Options { today: day(2026, 2, 15), relaxed: false });
            ledger.advance(day(2026, 2, 15));
            ledger.reach(day(2026, 4, 30));
            ledger.promise(|_| true);
            ledger.advance(day(2026, 4, 30));
            let planned = ledger.recorded().planned.clone();
            let flows = planned[0].made.unwrap().flows(ledger.recorded().promised_flows).unwrap();
            assert_eq!(planned.len(), 2, "03-01 and 04-01, and nothing more");
            assert_eq!(flows.len(), 2, "the payment and the escrow");
            let (checking, escrow) = (book.place("checking").unwrap(), book.place("escrow").unwrap());
            assert_eq!(ledger.balance(escrow, book.base), Qty(4 * 100_00));
            assert_eq!(ledger.balance(checking, book.base), Qty(10_000_00 - 4 * 1_100_00));
        });
    }

    #[test]
    fn a_derived_amount_in_another_commodity_stops_the_occurrence_and_says_so() {
        let text = mortgage("    derive 5.00 EUR #fee\n");
        with_run(&text, day(2026, 3, 1), |book, run| {
            assert_eq!(held(book, run, "escrow"), Qty::ZERO);
            assert_eq!(held(book, run, "checking"), Qty(10_000_00), "neither occurrence posted");
            assert!(run.diagnostics.iter().any(|diagnostic| diagnostic.code == "contract-occurrence-materialization"));
        });
    }
}
