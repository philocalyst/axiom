//! Folds over hand-built books: the promises the engine makes, end to end.
//!
//! Amounts are written in cents as `1_000_00` (a thousand and no cents), which
//! clippy reads as inconsistent digit grouping.
#![allow(clippy::inconsistent_digit_grouping)]

use axiom_core::{Day, Diagnostic, Disposition, Dim, Days, FileId, Groups, Id, Loc, Qty, Ratio, Severity, Tree};
use axiom_model::*;

use crate::fixture::{Fixture, LawBuilder, span};
use crate::{Bound, Cause, Holding, Options, Owed, Parcel, Plan, Run, State, Verdict, run};

fn options() -> Options {
    Options { today: Day(1000), relaxed: false }
}

fn held(run: &Run, place: Id<Place>, unit: Id<Commodity>) -> Option<&Holding> {
    run.holdings.iter().find(|h| h.place == place && h.unit == unit)
}

fn qty(run: &Run, place: Id<Place>, unit: Id<Commodity>) -> i64 {
    held(run, place, unit).map_or(0, |h| h.qty().0)
}

fn diagnostic<'a>(run: &'a Run, code: &str) -> &'a Diagnostic {
    run.diagnostics
        .iter()
        .find(|d| d.code == code)
        .unwrap_or_else(|| panic!("no `{code}` diagnostic in {:?}", run.diagnostics))
}

#[test]
fn dimensional_prices_apply_the_result_commodity_scale_once() {
    let fixture = Fixture::new();
    let (usd, mile) = (fixture.usd, fixture.vti);
    let book = fixture.book();
    let calc = crate::calc::Calc { book: &book, day: Day(1000) };
    let price = Value::Num(Ratio::new(7, 10).unwrap());
    let rate_ty = Ty::Amount(Dim::Per(usd, mile));
    let distance_ty = Ty::Amount(Dim::Of(mile));
    let money_ty = Ty::Amount(Dim::Of(usd));

    let money = calc.binary_typed(
        BinOp::Mul,
        Value::Amount(Amount::new(Qty(44), mile)),
        price,
        distance_ty,
        rate_ty,
        money_ty,
    );
    assert_eq!(money, Value::Amount(Amount::new(Qty(3_080), usd)));

    let distance = calc.binary_typed(
        BinOp::Div,
        money,
        price,
        money_ty,
        rate_ty,
        distance_ty,
    );
    assert_eq!(distance, Value::Amount(Amount::new(Qty(44), mile)));

    assert_eq!(
        calc.typed_value(Value::Num(Ratio::new(5_840, 100).unwrap()), money_ty),
        Value::Amount(Amount::new(Qty(5_840), usd)),
        "a unit-bearing parameter number becomes quanta in its declared unit"
    );
    assert_eq!(
        calc.typed_value(Value::Amount(Amount::new(Qty(44), mile)), money_ty),
        Value::Fault(Fault::UnitMismatch { found: mile, expected: usd })
    );
}

#[test]
fn cap_shortcut_uses_typed_conversion_when_the_limit_unit_differs() {
    let mut f = Fixture::new();
    let (usd, vti, checking, salary) = (f.usd, f.vti, f.checking, f.salary);
    let mut law = LawBuilder::new(f.sym("quoted-cap"), Trigger::In);
    let total = law.call(Func::Total(Dir::In, Window::Month), &[], Ty::AMOUNT);
    let cap = law.konst(Value::Amount(Amount::new(Qty(100), vti)), Ty::AMOUNT);
    let condition = law.bin(BinOp::Le, total, cap, Ty::Bool);
    let law = f.law(law.require(condition, None));
    let rule = f.rule(law, Subject::Place(checking));
    f.on_in.push((checking, rule));
    f.flow(1, salary, checking, 100);

    let mut book = f.book();
    // Use the same precision as USD so raw quanta happen to match even though
    // the quoted values do not. The shortcut must decline to compare them.
    book.commodities[vti].scale = 2;
    book.prices = Prices::new(vec![Quote {
        unit: usd,
        quote: vti,
        day: Day(1),
        rate: Ratio::int(2),
        implied: false,
        loc: Loc::new(FileId(0), 0, 1),
    }]);
    assert_eq!(book.convert(Amount::new(Qty(100), usd), vti, Day(1)), Some(Amount::new(Qty(200), vti)));
    let run = Plan::new(&book).run(options());

    assert_eq!(run.violations.len(), 1, "1.00 USD is 200 VTI and exceeds the 100 VTI cap");
    let reading = run.headroom.iter().find(|reading| reading.law == law).expect("the cap was read");
    assert_eq!(reading.counted, Amount::new(Qty(100), usd));
    assert_eq!(reading.limit, Amount::new(Qty(100), vti));
}

#[test]
fn a_faulted_computed_assertion_is_not_treated_as_its_zero_placeholder() {
    let mut f = Fixture::new();
    let (checking, usd) = (f.checking, f.usd);
    f.assert(1, checking, 0);
    let mut book = f.book();
    let mut nodes = axiom_core::Arena::new();
    nodes.push(Node {
        op: Op::Const(Value::Amount(Amount::new(Qty::ZERO, usd))),
        ty: None,
        loc: Loc::new(FileId(0), 0, 1),
        first: NodeId(0),
    });
    let program = book.assertion_programs.push(TemplateProgram { nodes });
    book.asserts[0].computed = Some((program, NodeId(0)));

    let run = Plan::new(&book).run(options());

    assert!(run.diagnostics.iter().any(|diagnostic| diagnostic.code == "assertion-expression"));
    assert!(run.diagnostics.iter().all(|diagnostic| diagnostic.code != "assertion"));
    assert!(run.pads.is_empty());
}

#[test]
fn well_known_names_resolve_once_and_are_absent_where_the_book_never_says_them() {
    let book = Fixture::new().book();
    let plan = Plan::new(&book);
    let known = plan.known();
    assert!(known.born.is_none() && known.maturity.is_none() && known.budget.is_none() && known.currency.is_none());
    assert!(plan.kind_places.is_empty(), "a book with no kind-widened totals needs no place index");
    let mut f = Fixture::new();
    let born = f.sym("born");
    let book = f.book();
    let known = Plan::new(&book).known();
    assert_eq!((known.born, known.maturity), (Some(born), None), "only what the book interned");
}

#[test]
fn effective_owners_compose_place_shares_through_nested_entities() {
    let mut f = Fixture::new();
    let (place, me, grant, household) = (f.savings, f.me, f.grant, f.household);
    let share = |entity, rate| Share { entity, rate, measure: None, loc: Loc::default() };
    f.entities[grant].owned_by =
        vec![share(me, Ratio::new(1, 4).unwrap()), share(household, Ratio::new(3, 4).unwrap())].into();
    f.places[place].shares =
        vec![share(me, Ratio::new(3, 5).unwrap()), share(grant, Ratio::new(2, 5).unwrap())].into();

    let book = f.book();
    let plan = Plan::new(&book);
    assert_eq!(
        plan.owners_of(place),
        &[
            crate::OwnerShare { owner: me, share: Ratio::new(7, 10).unwrap() },
            crate::OwnerShare { owner: household, share: Ratio::new(3, 10).unwrap() },
        ]
    );
    assert_eq!(
        plan.owners_of_entity(grant),
        &[
            crate::OwnerShare { owner: me, share: Ratio::new(1, 4).unwrap() },
            crate::OwnerShare { owner: household, share: Ratio::new(3, 4).unwrap() },
        ]
    );
    let positive: Vec<_> = plan.allocate(place, Qty(101)).collect();
    let negative: Vec<_> = plan.allocate(place, Qty(-101)).collect();
    assert_eq!(positive.iter().map(|(_, qty)| *qty).sum::<Qty>(), Qty(101));
    assert_eq!(negative.iter().map(|(_, qty)| *qty).sum::<Qty>(), Qty(-101));
    assert_eq!(positive[0].1, Qty(71), "cumulative owner boundary rounds once");
    assert_eq!(negative[0].1, Qty(-71), "signed allocations use the same boundary");
}

#[test]
fn invalid_entity_ownership_is_reported_without_recursing_or_dropping_into_self() {
    let mut f = Fixture::new();
    let (me, grant) = (f.me, f.grant);
    let share = |entity| Share { entity, rate: Ratio::ONE, measure: None, loc: Loc::default() };
    f.entities[me].owned_by = vec![share(grant)].into();
    f.entities[grant].owned_by = vec![share(me)].into();

    let book = f.book();
    let plan = Plan::new(&book);
    assert!(plan.owners_of_entity(me).is_empty());
    assert!(plan.owners_of_entity(grant).is_empty());
    let run = plan.run(options());
    assert!(run.diagnostics.iter().any(|problem| problem.code == "ownership-cycle"));
}

#[test]
fn runtime_contract_flows_keep_typed_occurrence_identity_in_acquired_lots() {
    let mut f = Fixture::new();
    let (equity, brokerage, savings, vti) = (f.equity, f.brokerage, f.savings, f.vti);
    let source = f.exchange(1, equity, Amount::new(Qty(5), vti), brokerage, Amount::new(Qty(5), vti));
    let mut book = f.book();
    let contract = book.contracts.push(Contract {
        name: book.entities[book.roots.me].path,
        party: book.roots.me,
        owner: book.roots.me,
        purpose: None,
        description: None,
        days: Days::ALWAYS,
        terms: None,
        standing: None,
        buys: None,
        deposit: None,
        deposit_holding: None,
        loan: None,
        matching: None,
        ended: None,
        laws: Box::default(),
        doc: None,
        loc: Loc::default(),
    });
    let mut flow = book.flows[source].clone();
    flow.txn = TEMPLATE_TXN;
    flow.day = Day(2);
    flow.recognized = axiom_core::Days::on(Day(2));
    flow.to = savings;
    flow.out = Amount::new(Qty(7), vti);
    flow.arrive = Amount::new(Qty(7), vti);
    let txn = RuntimeTxn::contract_occurrence(contract, ScheduleKind::Regular, Day(2), 0, None);
    let runtime = RuntimeFlow { flow, detail: None, txn };
    let plan = Plan::new(&book);
    let mut ledger = plan.start(Options { today: Day(3), relaxed: false });
    ledger.apply_runtime(&runtime, &axiom_core::Arena::<RuntimeDetail>::new());
    let run = ledger.finish();

    let holding = held(&run, savings, vti).expect("the runtime flow posted its non-money lot");
    assert_eq!(holding.lots.len(), 1);
    assert_eq!(holding.lots[0].txn, txn);
    assert_eq!(holding.lots[0].qty, Qty(7));
}

#[test]
fn an_empty_book_folds_to_nothing() {
    let book = Fixture::new().book();
    let plan = Plan::new(&book);
    let mut ledger = plan.start(options());
    ledger.advance(Day(500));
    assert_eq!(ledger.day(), Day(500));
    let run = ledger.finish();
    assert!(run.posted.is_empty() && run.holdings.is_empty() && run.diagnostics.is_empty());
}

#[test]
fn unless_suppresses_the_law_only_when_its_exception_holds() {
    let mut f = Fixture::new();
    let retirement = f.retirement;
    let make = |f: &mut Fixture, name: &'static str, unless: bool| {
        let name = f.sym(name);
        let mut builder = LawBuilder::new(name, Trigger::In);
        let exception = builder.konst(Value::Bool(unless), Ty::Bool);
        let failure = builder.konst(Value::Bool(false), Ty::Bool);
        let law = builder.unless(exception).require(failure, None);
        let law = f.law(law);
        f.on_in.push((retirement, f.rule(law, Subject::Place(retirement))));
        law
    };
    let suppressed = make(&mut f, "suppressed", true);
    let active = make(&mut f, "active", false);
    let salary = f.salary;
    f.flow(10, salary, retirement, 100_00);

    let run = run(&f.book(), options());
    assert_eq!(run.checks[suppressed.index()], 0);
    assert_eq!(run.checks[active.index()], 1);
    assert_eq!(run.violations.len(), 1);
    assert_eq!(run.violations[0].law, active);
}

#[test]
fn a_property_uses_the_value_in_force_on_the_day_judged() {
    let mut f = Fixture::new();
    let (retirement, salary) = (f.retirement, f.salary);
    let (name, limit) = (f.sym("limit"), f.sym("limit"));
    f.property(retirement, name, 0, Value::Amount(f.usd(100_00)));
    f.property(retirement, name, 20, Value::Amount(f.usd(200_00)));

    let mut law = LawBuilder::new(f.sym("dated-limit"), Trigger::In);
    let amount = law.var(Var::Amount, Ty::AMOUNT);
    let subject = law.var(Var::Subject, Ty::Place);
    let limit = law.field(subject, Field::Prop(limit), Ty::AMOUNT);
    let condition = law.bin(BinOp::Le, amount, limit, Ty::Bool);
    let law = f.law(law.require(condition, None));
    f.on_in.push((retirement, f.rule(law, Subject::Place(retirement))));
    f.flow(10, salary, retirement, 150_00);
    f.flow(20, salary, retirement, 150_00);

    let run = run(&f.book(), options());
    assert_eq!(run.violations.len(), 1, "the first limit is exceeded; the later value is in force for the second flow");
    assert_eq!(run.violations[0].cause, Cause::Flow(Id::new(0)));
}

#[test]
fn purpose_laws_see_the_owner_purpose_tree_and_description_of_a_flow() {
    let mut f = Fixture::new();
    let (salary, retirement, owner) = (f.salary, f.retirement, f.me);
    let purpose_name = f.sym("retirement-contribution");
    let payroll = f.sym("payroll");
    let law_name = f.sym("purpose-classification");
    let flow = f.flow(10, salary, retirement, 100_00);
    let mut builder = LawBuilder::new(law_name, Trigger::Flow);
    let actual_purpose = builder.var(Var::Purpose, Ty::Purpose);
    let spending = builder.konst(Value::Purpose(Id::new(1), Some(Object::Entity(owner))), Ty::Purpose);
    let belongs = builder.is(actual_purpose, &[spending]);
    let actual_object = builder.field(actual_purpose, Field::Of, Ty::Entity);
    let owner_value = builder.konst(Value::Entity(owner), Ty::Entity);
    let about_owner = builder.bin(BinOp::Eq, actual_object, owner_value, Ty::Bool);
    let actual_description = builder.var(Var::Description, Ty::Text);
    let description = builder.konst(Value::Text(Text::Borrowed(payroll)), Ty::Text);
    let described = builder.bin(BinOp::Eq, actual_description, description, Ty::Bool);
    let with_object = builder.bin(BinOp::And, belongs, about_owner, Ty::Bool);
    let both = builder.bin(BinOp::And, with_object, described, Ty::Bool);
    let law = f.law(builder.require(both, None));
    let purpose = Id::new(2);
    f.laws[law].owner = Owner::Purpose(purpose);
    let rule = Rule { law, subject: Subject::Entity(owner), days: Days::ALWAYS };
    f.flows[flow.index()].purpose =
        Some(Purposed { purpose, of: Some(Object::Entity(owner)), source: Provenance::Written });
    f.flows[flow.index()].description = Some(Text::Borrowed(payroll));
    let mut book = f.book();
    let roots = [
        book.roots.purposes.income,
        book.roots.purposes.spending,
        book.roots.purposes.capital,
        book.roots.purposes.transfer,
    ];
    let names = roots.map(|root| book.purposes[root].name);
    let purposes = vec![
        Purpose { name: names[0], root: PurposeRoot::Income, system: None, of: None, shares: Box::new([]), laws: Box::new([]), doc: None, loc: None },
        Purpose { name: names[1], root: PurposeRoot::Spending, system: None, of: None, shares: Box::new([]), laws: Box::new([]), doc: None, loc: None },
        Purpose { name: purpose_name, root: PurposeRoot::Spending, system: None, of: None, shares: Box::new([]), laws: Box::new([]), doc: None, loc: None },
        Purpose { name: names[2], root: PurposeRoot::Capital, system: None, of: None, shares: Box::new([]), laws: Box::new([]), doc: None, loc: None },
        Purpose { name: names[3], root: PurposeRoot::Transfer, system: None, of: None, shares: Box::new([]), laws: Box::new([]), doc: None, loc: None },
    ];
    let (tree, ids) = Tree::build(purposes, &[None, None, Some(1), None, None]).unwrap();
    book.purposes = tree;
    book.roots.purposes.income = ids[0];
    book.roots.purposes.spending = ids[1];
    book.roots.purposes.capital = ids[3];
    book.roots.purposes.transfer = ids[4];
    book.rules.purposes = Groups::build(book.purposes.len(), [(purpose, rule)]);

    let run = run(&book, options());
    assert_eq!(run.checks[law.index()], 1);
    assert!(run.violations.is_empty());
}

#[test]
fn purpose_rules_use_each_flows_owner_for_scope_and_sparse_totals() {
    let mut f = Fixture::new();
    let (salary, checking, grants, savings, me, grant) =
        (f.salary, f.checking, f.grants, f.savings, f.me, f.grant);
    f.places[savings].owner = grant;
    let from_me = f.flow(10, salary, checking, 80_00);
    let from_grant = f.flow(11, grants, savings, 150_00);
    let purpose = Id::new(2);
    for flow in [from_me, from_grant] {
        f.flows[flow.index()].purpose = Some(Purposed {
            purpose,
            of: None,
            source: Provenance::Written,
        });
    }
    f.flows[from_grant.index()].owner = grant;

    let mut builder = LawBuilder::new(f.sym("owner-budget"), Trigger::Flow);
    let total = builder.call(Func::Total(Dir::In, Window::Year), &[], Ty::AMOUNT);
    let limit = builder.konst(Value::Amount(f.usd(100_00)), Ty::AMOUNT);
    let below_limit = builder.bin(BinOp::Le, total, limit, Ty::Bool);
    let law = f.law(builder.warn(below_limit));
    f.laws[law].owner = Owner::Purpose(purpose);
    let placeholder = Rule { law, subject: Subject::Entity(me), days: Days::ALWAYS };
    let mut book = f.book();
    book.rules.purposes = Groups::build(book.purposes.len(), [(purpose, placeholder)]);

    let run = run(&book, options());
    assert_eq!(run.violations.len(), 1, "only the grant owner's 150.00 total exceeds 100.00");
    assert_eq!(run.violations[0].subject, Subject::Entity(grant));
    assert_eq!(run.violations[0].cause, Cause::Flow(from_grant));
}

#[test]
fn a_purpose_window_rechecks_prepaid_recognition_without_later_flows() {
    let mut f = Fixture::new();
    let (checking, market, me) = (f.checking, f.market, f.me);
    let purpose = Id::new(1);
    let mut builder = LawBuilder::new(f.sym("monthly-purpose-cap"), Trigger::Flow);
    let total = builder.call(
        Func::PurposeTotal { purpose: Some(purpose), window: Window::Month },
        &[],
        Ty::AMOUNT,
    );
    let limit = builder.konst(Value::Amount(f.usd(100_00)), Ty::AMOUNT);
    let within = builder.bin(BinOp::Le, total, limit, Ty::Bool);
    let year_total = builder.call(
        Func::PurposeTotal { purpose: Some(purpose), window: Window::Year },
        &[],
        Ty::AMOUNT,
    );
    let year_limit = builder.konst(Value::Amount(f.usd(500_00)), Ty::AMOUNT);
    let year_within = builder.bin(BinOp::Le, year_total, year_limit, Ty::Bool);
    let both_within = builder.bin(BinOp::And, within, year_within, Ty::Bool);
    let amount = builder.var(Var::Amount, Ty::AMOUNT);
    let empty = builder.konst(Value::Empty, Ty::Empty);
    let no_flow_amount = builder.bin(BinOp::Eq, amount, empty, Ty::Bool);
    let one = builder.konst(Value::Amount(f.usd(1_00)), Ty::AMOUNT);
    let zero = builder.konst(Value::Amount(f.usd(0)), Ty::AMOUNT);
    let invalid_without_flow = builder.bin(BinOp::Div, one, zero, Ty::AMOUNT);
    let bucket_amount = builder.if_then_else(no_flow_amount, invalid_without_flow, amount, Ty::AMOUNT);
    let count = f.sym("recognized-flow-amount");
    let law = f.law(builder.warn(both_within).count(bucket_amount, count));
    f.laws[law].owner = Owner::Purpose(purpose);
    let mut flow_only = LawBuilder::new(f.sym("purpose-flow-only"), Trigger::Flow);
    // Even an orphaned PurposeTotal node is not a window requirement.
    let _unreferenced = flow_only.call(
        Func::PurposeTotal { purpose: Some(purpose), window: Window::Month },
        &[],
        Ty::AMOUNT,
    );
    let amount = flow_only.var(Var::Amount, Ty::AMOUNT);
    let maximum = flow_only.konst(Value::Amount(f.usd(1_000_00)), Ty::AMOUNT);
    let accepted = flow_only.bin(BinOp::Le, amount, maximum, Ty::Bool);
    let flow_only = f.law(flow_only.warn(accepted));
    f.laws[flow_only].owner = Owner::Purpose(purpose);
    let mut prepaid = f.flow(date(2025, 12, 15), checking, market, 300_00);
    f.recognize(prepaid, date(2025, 12, 15), date(2026, 2, 14));
    f.flows[prepaid.index()].purpose = Some(Purposed { purpose, of: None, source: Provenance::Written });

    let mut book = f.book();
    let rule = Rule { law, subject: Subject::Entity(me), days: Days::ALWAYS };
    let flow_rule = Rule { law: flow_only, ..rule };
    book.rules.purposes = Groups::build(book.purposes.len(), [(purpose, rule), (purpose, flow_rule)]);
    let run = run(&book, Options { today: Day(date(2026, 2, 28)), relaxed: false });

    assert_eq!(run.violations.len(), 1, "only January's recognized share exceeds the monthly cap");
    assert_eq!(run.violations[0].day, Day(date(2026, 1, 1)));
    assert_eq!(run.violations[0].cause, Cause::Time, "the limit breaks as the prepaid window opens");
    assert_eq!(run.checks[law.index()], 3, "the flow and both future months are evaluated once, despite reading two windows");
    assert_eq!(run.checks[flow_only.index()], 1, "an amount-only flow law does not run at month or year openings");
    let counted: Vec<_> = run.effects.iter().filter(|effect| effect.law == law).collect();
    assert_eq!(counted.len(), 2, "the recognized span contributes one tally entry per year");
    assert_eq!(counted.iter().map(|effect| effect.amount.qty).sum::<Qty>(), Qty(300_00));
    assert!(counted.iter().all(|effect| effect.cause == Cause::Flow(prepaid)));
    assert!(run.diagnostics.iter().all(|diagnostic| diagnostic.code != "arithmetic"), "the flow-only count expression must not be evaluated at a window opening");
}

#[test]
fn a_credit_card_refund_reverses_spending_purpose_total() {
    let mut f = Fixture::new();
    let (card, food, me) = (f.card, f.food, f.me);
    let purpose = Id::new(2);
    let charge = f.flow(2, card, food, 84_00);
    let refund = f.flow(3, food, card, 40_00);
    for flow in [charge, refund] {
        f.flows[flow.index()].purpose = Some(Purposed {
            purpose,
            of: None,
            source: Provenance::Written,
        });
    }

    let mut law = LawBuilder::new(f.sym("net-spending"), Trigger::Flow);
    let total = law.call(
        Func::PurposeTotal { purpose: Some(purpose), window: Window::Ever },
        &[],
        Ty::AMOUNT,
    );
    let expected = law.konst(Value::Amount(f.usd(44_00)), Ty::AMOUNT);
    let matches = law.bin(BinOp::Eq, total, expected, Ty::Bool);
    let law = f.law(law.require(matches, None));
    f.laws[law].owner = Owner::Purpose(purpose);

    let spending_name = f.sym("card-spending");
    let mut book = f.book();
    let roots = [
        book.roots.purposes.income,
        book.roots.purposes.spending,
        book.roots.purposes.capital,
        book.roots.purposes.transfer,
    ];
    let names = roots.map(|root| book.purposes[root].name);
    let purposes = vec![
        Purpose { name: names[0], root: PurposeRoot::Income, system: None, of: None, shares: Box::new([]), laws: Box::new([]), doc: None, loc: None },
        Purpose { name: names[1], root: PurposeRoot::Spending, system: None, of: None, shares: Box::new([]), laws: Box::new([]), doc: None, loc: None },
        Purpose { name: spending_name, root: PurposeRoot::Spending, system: None, of: None, shares: Box::new([]), laws: Box::new([]), doc: None, loc: None },
        Purpose { name: names[2], root: PurposeRoot::Capital, system: None, of: None, shares: Box::new([]), laws: Box::new([]), doc: None, loc: None },
        Purpose { name: names[3], root: PurposeRoot::Transfer, system: None, of: None, shares: Box::new([]), laws: Box::new([]), doc: None, loc: None },
    ];
    let (tree, ids) = Tree::build(purposes, &[None, None, Some(1), None, None]).unwrap();
    book.purposes = tree;
    book.roots.purposes.income = ids[0];
    book.roots.purposes.spending = ids[1];
    book.roots.purposes.capital = ids[3];
    book.roots.purposes.transfer = ids[4];
    book.rules.purposes = Groups::build(book.purposes.len(), [(purpose, Rule {
        law,
        subject: Subject::Entity(me),
        days: span(3, 3),
    })]);

    let run = run(&book, options());
    assert!(run.violations.is_empty(), "84.00 charge less a 40.00 refund is 44.00");
    assert_eq!(run.checks[law.index()], 1, "only the refund day is checked");
}

#[test]
fn a_promise_is_late_by_the_days_until_it_is_kept_or_the_horizon_if_it_never_is() {
    let promise = |kept: Option<i32>| crate::Promise {
        contract: Id::new(0),
        schedule: ScheduleKind::Regular,
        ordinal: 0,
        due: Day(100),
        kept: kept.map(|day| (Day(day), Id::new(0))),
        waived: false,
        flows: crate::RuntimeRange::default(),
        missing_inputs: crate::RuntimeRange::default(),
    };
    assert_eq!(promise(Some(95)).late(Day(200)), 0, "kept early");
    assert_eq!(promise(Some(103)).late(Day(200)), 3, "kept late");
    assert_eq!(promise(None).late(Day(110)), 10, "still missing");
    assert_eq!(promise(None).late(Day(90)), 0, "not due yet");
}

#[test]
fn plain_money_is_one_integer_per_place() {
    let mut f = Fixture::new();
    let (equity, checking, food, salary, usd) = (f.equity, f.checking, f.food, f.salary, f.usd);
    f.flow(1, equity, checking, 1_000_00);
    f.flow(2, checking, food, 20_00);
    f.flow(3, salary, checking, 500_00);
    let book = f.book();
    let plan = Plan::new(&book);
    assert_eq!(plan.sides().sign(checking), 1);
    assert_eq!(plan.sides().display(equity, Qty(20)), Qty(20), "all Outside places use the same class sign");
    let run = run(&book, options());
    assert_eq!(qty(&run, checking, usd), 1_480_00);
    assert_eq!(qty(&run, food, usd), 20_00);
    assert_eq!(qty(&run, equity, usd), -1_000_00, "sources go negative: balances are inflow minus outflow");
    assert!(run.holdings.iter().all(|h| h.lots.is_empty()));
    assert!(run.diagnostics.is_empty());
}

fn brokerage_book(policy: Option<Policy>) -> (Book<'static>, Fixture) {
    let mut f = Fixture::new();
    let (equity, checking) = (f.equity, f.checking);
    f.flow(1, equity, checking, 10_000_00);
    f.buy(2, 1_000_00, 10);
    f.buy(3, 1_500_00, 10);
    let sale = f.sell(4, 5, 800_00);
    if let Some(policy) = policy {
        f.select(sale, [Select::Policy(policy)]);
    }
    let names = Fixture::new();
    (f.book(), names)
}

#[test]
fn ambiguous_lots_report_candidates_with_gains_and_fall_back_to_fifo() {
    let (book, f) = brokerage_book(None);
    let run = run(&book, options());
    let d = diagnostic(&run, "ambiguous-lots");
    assert_eq!(d.severity, Severity::Error);
    let rows: Vec<_> = d.labels.iter().filter(|l| !l.primary).map(|l| l.text.as_str()).collect();
    assert_eq!(
        rows,
        [
            "acquired 1970-01-03: 10 VTI, basis 1,000.00 USD; from it alone the gain is 300.00 USD",
            "acquired 1970-01-04: 10 VTI, basis 1,500.00 USD; from it alone the gain is 50.00 USD",
        ]
    );
    let gain = run.gains[0];
    assert!(gain.ambiguous);
    assert_eq!((gain.qty, gain.basis, gain.proceeds, gain.acquired), (Qty(5), Qty(500_00), Qty(800_00), Day(2)));
    assert_eq!(gain.gain(), Qty(300_00), "first in, first out");
    drop(f);
}

#[test]
fn a_policy_decides_and_hifo_sells_the_dearest_lot() {
    let (book, _) = brokerage_book(Some(Policy::Hifo));
    let run = run(&book, options());
    assert!(run.diagnostics.is_empty());
    let gain = run.gains[0];
    assert_eq!((gain.basis, gain.gain(), gain.acquired, gain.ambiguous), (Qty(750_00), Qty(50_00), Day(3), false));
    let brokerage = held(&run, book.places.ids().nth(4).unwrap(), Id::new(1)).unwrap();
    let lots: Vec<_> = brokerage.lots.iter().map(|l| (l.qty.0, l.basis.0)).collect();
    assert_eq!(lots, [(10, 1_000_00), (5, 750_00)]);
}

#[test]
fn all_realizes_every_lot_at_its_share_of_the_proceeds() {
    let mut f = Fixture::new();
    let (equity, checking, brokerage, usd, vti) = (f.equity, f.checking, f.brokerage, f.usd, f.vti);
    f.flow(1, equity, checking, 10_000_00);
    f.buy(2, 1_000_00, 10);
    f.buy(3, 1_500_00, 10);
    let sale = f.sell_all(4, 3_000_00);
    let book = f.book();
    let run = run(&book, options());
    assert_eq!(run.posted[sale.index()].out, Qty(20));
    let gains: Vec<_> = run.gains.iter().map(|g| (g.qty.0, g.gain().0)).collect();
    assert_eq!(gains, [(10, 500_00), (10, 0)]);
    assert!(run.diagnostics.is_empty(), "taking everything leaves no choice to make");
    assert_eq!(qty(&run, brokerage, vti), 0);
    assert_eq!(qty(&run, checking, usd), 10_000_00 - 2_500_00 + 3_000_00);
}

#[test]
fn a_sale_of_more_than_is_held_leaves_a_negative_balance() {
    let mut f = Fixture::new();
    let (equity, checking, brokerage, vti) = (f.equity, f.checking, f.brokerage, f.vti);
    f.flow(1, equity, checking, 1_000_00);
    f.buy(2, 700_00, 7);
    f.sell(3, 10, 1_300_00);
    let book = f.book();
    let run = run(&book, options());
    let d = diagnostic(&run, "insufficient-holding");
    assert_eq!(d.message, "assets/brokerage does not hold 10 VTI");
    assert_eq!(qty(&run, brokerage, vti), -3);
    assert_eq!(held(&run, brokerage, vti).unwrap().plain, Qty(-3));
    assert_eq!(run.gains[0].gain(), Qty(210_00), "the seven shares held sold at their share of the proceeds");
}

/// `total(in, year) <= 24_500 USD`, on flows into the retirement account.
fn deferral_limit(f: &mut Fixture) -> Id<Law> {
    let (name, doc) = (
        f.sym("deferral-limit"),
        f.sym(
            "/// Elective deferrals are capped per calendar year.\n///\n/// To fix: ask payroll to lower the deferral.",
        ),
    );
    let limit = f.usd(24_500_00);
    let mut law = LawBuilder::new(name, Trigger::In).doc(doc);
    let total = law.call(Func::Total(Dir::In, Window::Year), &[], Ty::AMOUNT);
    let cap = law.konst(Value::Amount(limit), Ty::AMOUNT);
    let cond = law.bin(BinOp::Le, total, cap, Ty::Bool);
    let law = f.law(law.require(cond, None));
    let (retirement, rule) = (f.retirement, f.rule(law, Subject::Place(f.retirement)));
    f.on_in.push((retirement, rule));
    law
}

#[test]
fn a_failed_law_explains_itself_power_assert_style() {
    let mut f = Fixture::new();
    let (salary, retirement) = (f.salary, f.retirement);
    let law = deferral_limit(&mut f);
    f.flow(10, salary, retirement, 22_700_00);
    let over = f.flow(20, salary, retirement, 2_600_00);
    let book = f.book();
    let run = run(&book, options());

    assert_eq!(run.checks[law.index()], 2);
    assert_eq!(run.violations.len(), 1);
    let violation = run.violations[0];
    assert_eq!((violation.cause, violation.verdict), (Cause::Flow(over), Verdict::Blocks));
    let d = &run.diagnostics[violation.diagnostic as usize];
    assert_eq!((&*d.code, d.severity), ("deferral-limit", Severity::Error), "the code is the law's own name");
    assert_eq!(
        d.message, "assets/retirement: 25,300.00 USD in 1970 against a limit of 24,500.00 USD, over by 800.00 USD",
        "the headline is the accounting fact"
    );
    let primary = d.labels.iter().find(|l| l.primary).unwrap();
    assert_eq!(primary.loc, book.flows[over].loc);
    assert_eq!(primary.text, "this flow: 2,600.00 USD into assets/retirement");
    let others: Vec<_> = d.labels.iter().filter(|l| !l.primary).map(|l| (l.loc.file.0, l.text.as_str())).collect();
    assert_eq!(
        others,
        [(0, "1970-01-11: 22,700.00 USD from income/salary to assets/retirement"), (1, "25,300.00 USD")],
        "the flows that built the count, then the operand of the failing comparison in the law's own source"
    );
    assert_eq!(d.notes, ["Elective deferrals are capped per calendar year."]);
    assert_eq!(d.help[0].text, "at most 1,800.00 USD more can go in this year");
}

#[test]
fn a_tally_bound_names_the_room_left_when_the_law_counted_this_flow() {
    let mut f = Fixture::new();
    let (salary, retirement) = (f.salary, f.retirement);
    let (name, deferrals, message) =
        (f.sym("deferral-limit"), f.sym("elective-deferrals"), f.sym("401(k) deferrals over the yearly limit"));
    let mut law = LawBuilder::new(name, Trigger::In);
    let amount = law.var(Var::Amount, Ty::AMOUNT);
    let counted = law.call(Func::Tally(deferrals), &[], Ty::AMOUNT);
    let cap = law.konst(Value::Amount(f.usd(24_500_00)), Ty::AMOUNT);
    let cond = law.bin(BinOp::Le, counted, cap, Ty::Bool);
    let law = f.law(law.count(amount, deferrals).require(cond, Some(message)));
    let rule = f.rule(law, Subject::Entity(f.me));
    f.on_in.push((retirement, rule));
    f.flow(10, salary, retirement, 22_700_00);
    f.flow(20, salary, retirement, 26_000_00);
    let book = f.book();
    let run = run(&book, options());
    let d = &run.diagnostics[run.violations[0].diagnostic as usize];
    assert_eq!(
        d.message,
        "401(k) deferrals over the yearly limit: 48,700.00 USD in 1970 against a limit of 24,500.00 USD, over by 24,200.00 USD"
    );
    assert_eq!(d.help[0].text, "at most 1,800.00 USD more can count toward `elective-deferrals` this year");
}

#[test]
fn a_waived_transaction_or_a_relaxed_book_demotes_the_error() {
    let mut f = Fixture::new();
    let (salary, retirement) = (f.salary, f.retirement);
    deferral_limit(&mut f);
    let over = f.flow(20, salary, retirement, 25_000_00);
    let loc = f.flows[over.index()].loc;
    f.flows[over.index()].waive = Some(Waive { loc, reason: None });
    let book = f.book();
    let run = run(&book, options());
    assert!(run.violations[0].verdict.is_waived());
    assert_eq!(run.diagnostics[0].severity, Severity::Warning);
    assert!(run.diagnostics[0].labels.iter().any(|l| l.text == "waived here"));

    let mut f = Fixture::new();
    let (salary, retirement) = (f.salary, f.retirement);
    deferral_limit(&mut f);
    f.flow(20, salary, retirement, 25_000_00);
    let book = f.book();
    let relaxed = crate::run(&book, Options { relaxed: true, ..options() });
    assert_eq!(relaxed.diagnostics[0].severity, Severity::Warning);
}

#[test]
fn value_moving_inside_a_subject_does_not_count_toward_its_totals() {
    let mut f = Fixture::new();
    let (equity, salary, checking, savings, assets) = (f.equity, f.salary, f.checking, f.savings, f.assets);
    let (name, cap) = (f.sym("monthly-cap"), f.usd(10_000_00));
    let mut law = LawBuilder::new(name, Trigger::In);
    let total = law.call(Func::Total(Dir::In, Window::Month), &[], Ty::AMOUNT);
    let cap = law.konst(Value::Amount(cap), Ty::AMOUNT);
    let cond = law.bin(BinOp::Le, total, cap, Ty::Bool);
    let law = f.law(law.require(cond, None));
    for place in [checking, savings] {
        let rule = f.rule(law, Subject::Place(assets));
        f.on_in.push((place, rule));
    }
    f.flow(1, equity, checking, 5_000_00);
    f.flow(2, checking, savings, 3_000_00);
    f.flow(3, salary, savings, 4_000_00);
    let last = f.flow(4, salary, checking, 1_500_00);
    let book = f.book();
    let run = run(&book, options());
    assert_eq!(run.checks[law.index()], 3, "the transfer between two places under `assets` is skipped");
    assert_eq!(run.violations.len(), 1);
    assert_eq!(run.violations[0].cause, Cause::Flow(last));
    let d = &run.diagnostics[run.violations[0].diagnostic as usize];
    assert!(d.labels.iter().any(|l| l.text == "10,500.00 USD"), "9,000 already in, 1,500 more: {d:?}");
}

#[test]
fn kind_totals_use_descendants_and_only_the_subject_owners_places() {
    let mut f = Fixture::new();
    let (checking, savings, brokerage, cash, salary, grant, market_place) =
        (f.checking, f.savings, f.brokerage, f.cash, f.salary, f.grant, f.market);
    let bank = Id::new(1);
    let add_limit = |f: &mut Fixture, window, name| {
        let mut law = LawBuilder::new(f.sym(name), Trigger::In);
        let kind = law.konst(Value::Kind(bank), Ty::Kind);
        let total = law.call(Func::Total(Dir::In, window), &[kind], Ty::AMOUNT);
        let limit = law.konst(Value::Amount(f.usd(100_00)), Ty::AMOUNT);
        let condition = law.bin(BinOp::Le, total, limit, Ty::Bool);
        let law = f.law(law.warn(condition));
        f.on_in.push((checking, f.rule(law, Subject::Place(checking))));
        law
    };
    let (month, year) = (
        add_limit(&mut f, Window::Month, "kind-month-limit"),
        add_limit(&mut f, Window::Year, "kind-year-limit"),
    );

    f.flow(date(2025, 1, 1), salary, checking, 40_00);
    f.flow(date(2025, 1, 2), salary, savings, 30_00);
    f.flow(date(2025, 1, 3), salary, brokerage, 25_00);
    f.flow(date(2025, 1, 4), salary, cash, 200_00);
    f.flow(date(2025, 1, 5), salary, savings, 100_00);
    f.flow(date(2025, 1, 6), salary, checking, 20_00);
    f.flow(date(2025, 2, 1), salary, checking, 10_00);

    // Replace the fixture's two unrelated kinds with an asset-kind tree whose
    // bank descendants and unrelated cash place make the widened set explicit.
    let mut book = f.book();
    let root = book.kinds[Id::new(0)].clone();
    let market = book.kinds[Id::new(1)].clone();
    let kind = |name, sort, parent: &Kind| Kind { name, sort, ..parent.clone() };
    let bank_kind = kind(book.names.intern("bank"), Sort::Place(Class::Asset), &root);
    let checking_kind = kind(book.names.intern("checking-kind"), Sort::Place(Class::Asset), &bank_kind);
    let savings_kind = kind(book.names.intern("savings-kind"), Sort::Place(Class::Asset), &bank_kind);
    let brokerage_kind = kind(book.names.intern("brokerage-kind"), Sort::Place(Class::Asset), &bank_kind);
    book.kinds = Tree::build(
        vec![root, bank_kind, checking_kind, savings_kind, brokerage_kind, market],
        &[None, Some(0), Some(1), Some(1), Some(1), None],
    )
    .expect("acyclic kind fixture")
    .0;
    book.places[checking].kind = Id::new(2);
    book.places[savings].kind = Id::new(3);
    book.places[brokerage].kind = Id::new(4);
    book.places[market_place].kind = Id::new(5);
    book.places[savings].owner = grant;

    let plan = Plan::new(&book);
    assert_eq!(plan.kind_places.len(), 1, "repeated reads share one sparse index entry");
    let indexed = plan.kind_places.get(&bank).expect("the total reads its kind");
    let scanned: Vec<_> = book
        .places
        .iter()
        .filter(|(_, place)| book.is_a(place.kind, bank))
        .map(|(place, _)| place)
        .collect();
    assert_eq!(
        indexed.as_ref(),
        scanned,
        "the kind index includes the kind and its descendants"
    );
    drop(plan);

    let run = run(&book, options());
    let rows = |law| {
        run.headroom
            .iter()
            .filter(|reading| reading.law == law)
            .map(|reading| (reading.days.first(), reading.counted.qty))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        rows(month),
        [
            (Day(date(2025, 1, 1)), Qty(85_00)),
            (Day(date(2025, 2, 1)), Qty(10_00)),
        ],
        "the bank parent includes its checking, savings and brokerage descendants; \
         cash and another owner's savings are excluded"
    );
    assert_eq!(
        rows(year),
        [(Day(date(2025, 1, 1)), Qty(95_00))],
        "the annual window includes January and February"
    );
}

#[test]
fn computed_kind_totals_index_every_kind_and_use_the_selected_kind() {
    let mut f = Fixture::new();
    let (checking, savings, brokerage, salary, market) =
        (f.checking, f.savings, f.brokerage, f.salary, f.market);
    let mut law = LawBuilder::new(f.sym("computed-kind-total"), Trigger::In);
    let yes = law.konst(Value::Bool(true), Ty::Bool);
    let target = law.var(Var::To, Ty::Place);
    let target_kind = law.field(target, Field::Kind, Ty::Kind);
    let other_kind = law.konst(Value::Kind(Id::new(5)), Ty::Kind);
    let kind = law.if_then_else(yes, target_kind, other_kind, Ty::Kind);
    let total = law.call(Func::Total(Dir::In, Window::Year), &[kind], Ty::AMOUNT);
    let limit = law.konst(Value::Amount(f.usd(80_00)), Ty::AMOUNT);
    let condition = law.bin(BinOp::Le, total, limit, Ty::Bool);
    let law = f.law(law.warn(condition));
    f.on_in.push((checking, f.rule(law, Subject::Place(checking))));
    f.flow(date(2025, 1, 1), salary, checking, 40_00);
    f.flow(date(2025, 1, 2), salary, savings, 30_00);
    f.flow(date(2025, 1, 2), salary, market, 200_00);
    f.flow(date(2025, 1, 3), salary, checking, 20_00);

    let mut book = f.book();
    let root = book.kinds[Id::new(0)].clone();
    let old_market = book.kinds[Id::new(1)].clone();
    let kind = |name, sort, parent: &Kind| Kind { name, sort, ..parent.clone() };
    let bank = kind(book.names.intern("computed-bank"), Sort::Place(Class::Asset), &root);
    let checking_kind = kind(book.names.intern("computed-checking"), Sort::Place(Class::Asset), &bank);
    let savings_kind = kind(book.names.intern("computed-savings"), Sort::Place(Class::Asset), &bank);
    let brokerage_kind = kind(book.names.intern("computed-brokerage"), Sort::Place(Class::Asset), &bank);
    book.kinds = Tree::build(
        vec![root, bank, checking_kind, savings_kind, brokerage_kind, old_market],
        &[None, Some(0), Some(1), Some(1), Some(1), None],
    )
    .expect("acyclic kind fixture")
    .0;
    book.places[checking].kind = Id::new(2);
    book.places[savings].kind = Id::new(3);
    book.places[brokerage].kind = Id::new(4);
    book.places[market].kind = Id::new(5);
    // The unrelated kind belongs to the same owner, so only the selected kind
    // can keep its large flow out of this total.
    book.places[market].owner = book.roots.me;
    let plan = Plan::new(&book);
    assert_eq!(plan.kind_places.len(), book.kinds.len(), "a computed kind may select every kind");
    for (candidate, _) in book.kinds.iter() {
        let indexed = plan.kind_places.get(&candidate).expect("the dynamic kind fallback indexes all kinds");
        let scanned: Vec<_> = book
            .places
            .iter()
            .filter(|(_, place)| book.is_a(place.kind, candidate))
            .map(|(place, _)| place)
            .collect();
        assert_eq!(indexed.as_ref(), scanned, "candidate kind {candidate:?}");
    }
    drop(plan);

    let run = run(&book, options());
    let [reading] = run.headroom[..] else { panic!("one computed-kind reading: {:?}", run.headroom) };
    assert_eq!((reading.law, reading.counted.qty), (law, Qty(60_00)));
}

#[test]
fn unknown_amounts_are_solved_and_a_failed_assertion_names_the_flows_since() {
    let mut f = Fixture::new();
    let (equity, checking, cash, food) = (f.equity, f.checking, f.cash, f.food);
    f.flow(1, equity, checking, 1_000_00);
    f.assert(2, checking, 1_000_00);
    let atm = f.unknown(3, checking, cash);
    f.flow(4, checking, food, 20_00);
    f.assert(5, checking, 700_00);
    let lunch = f.flow(6, checking, food, 10_00);
    f.assert(7, checking, 600_00);
    let book = f.book();
    let run = run(&book, options());
    assert_eq!(run.posted[atm.index()].out, Qty(280_00));
    let errors: Vec<_> = run.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert_eq!(errors.len(), 1, "{errors:?}");
    let d = errors[0];
    assert_eq!((&*d.code, d.message.as_str()), ("assertion", "assets/checking holds 690.00 USD, not 600.00 USD"));
    assert_eq!(d.labels[0].text, "90.00 USD less than the ledger holds");
    let since: Vec<_> = d.labels.iter().skip(1).map(|l| (l.loc, l.text.as_str())).collect();
    assert_eq!(
        since,
        [(book.flows[lunch].loc, "-10.00 USD to expenses/food")],
        "only the flows after the last assertion that held"
    );
    assert_eq!(d.help[0].text, "record the missing flow", "correcting the books comes before accepting the gap");
    assert_eq!(d.help.last().unwrap().edit.as_ref().unwrap().1, " !");
}

#[test]
fn liabilities_are_asserted_and_solved_in_the_sign_they_are_read() {
    let mut f = Fixture::new();
    let (card, checking, food, cash) = (f.card, f.checking, f.food, f.cash);
    f.flow(1, card, food, 84_00);
    f.assert(2, card, 84_00);
    let advance = f.unknown(3, card, cash);
    f.assert(4, card, 184_00);
    f.flow(5, checking, card, 30_00);
    f.assert(6, card, 154_00);
    f.assert(7, card, 100_00);
    let book = f.book();
    let run = run(&book, options());
    assert_eq!(run.posted[advance.index()].out, Qty(100_00), "100 more owed: the card paid out 100");
    let errors: Vec<_> = run.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].message, "liabilities/card holds 154.00 USD, not 100.00 USD");
    assert_eq!(qty(&run, card, Id::new(0)), -154_00, "a liability's balance is naturally negative");
}

#[test]
fn only_the_unwritten_side_of_an_exchange_is_unknown() {
    let mut f = Fixture::new();
    let (equity, checking) = (f.equity, f.checking);
    f.flow(1, equity, checking, 5_000_00);
    f.assert(2, checking, 5_000_00);
    let purchase = f.unknown_buy(3, 7);
    f.assert(4, checking, 3_000_10);
    let book = f.book();
    let run = run(&book, options());
    let posted = run.posted[purchase.index()];
    assert_eq!(
        (posted.out, posted.arrive),
        (Qty(1_999_90), Qty(7)),
        "the 7 shares were written; the price is what the bank says"
    );
    assert!(run.diagnostics.is_empty(), "{:?}", run.diagnostics);
}

#[test]
fn an_assertion_that_accepts_a_gap_cannot_solve_an_unknown_amount() {
    let mut f = Fixture::new();
    let (equity, checking, cash, food) = (f.equity, f.checking, f.cash, f.food);
    f.flow(1, equity, checking, 1_000_00);
    f.assert(2, checking, 1_000_00);
    let atm = f.unknown(3, checking, cash);
    f.assert(4, checking, 900_00);
    f.flow(5, cash, food, 22_50);
    f.assert(6, cash, 63_00);
    f.pad_last();
    let book = f.book();
    let run = run(&book, options());
    assert_eq!(run.posted[atm.index()].out, Qty(100_00), "the bank's number, not the wallet's, which admits a gap");
    assert_eq!(run.pads[0].amount.qty, Qty(-14_50));
}

#[test]
fn a_padded_assertion_moves_the_gap_from_unknown() {
    let mut f = Fixture::new();
    let (equity, checking, unknown, usd) = (f.equity, f.checking, f.unknown, f.usd);
    f.flow(1, equity, checking, 1_000_00);
    f.assert(2, checking, 900_00);
    f.pad_last();
    f.assert(3, checking, 900_00);
    let book = f.book();
    let run = run(&book, options());
    assert_eq!(run.pads.len(), 1);
    assert_eq!(run.pads[0].amount.qty, Qty(-100_00));
    assert_eq!(qty(&run, checking, usd), 900_00);
    assert_eq!(qty(&run, unknown, usd), 100_00);
    assert_eq!(diagnostic(&run, "pad").severity, Severity::Note);
    assert!(run.diagnostics.iter().all(|d| !d.is_error()), "the second assertion holds after the pad");
}

#[test]
fn a_target_leg_is_whatever_reaches_the_balance() {
    let mut f = Fixture::new();
    let (equity, checking, savings, usd) = (f.equity, f.checking, f.savings, f.usd);
    f.flow(1, equity, checking, 1_000_00);
    let sweep = f.target_leg(2, checking, savings, End::From, 400_00);
    let book = f.book();
    let run = run(&book, options());
    assert_eq!(run.posted[sweep.index()].out, Qty(600_00));
    assert_eq!((qty(&run, checking, usd), qty(&run, savings, usd)), (400_00, 600_00));
}

#[test]
fn deferred_money_has_no_basis_and_a_withdrawal_realizes_all_of_it() {
    let mut f = Fixture::new();
    let (salary, retirement, checking, usd) = (f.salary, f.retirement, f.checking, f.usd);
    let (name, penalty, irs) = (f.sym("early-withdrawal"), f.sym("penalty"), f.grant);
    let mut law = LawBuilder::new(name, Trigger::Gain);
    let tenth = law.konst(Value::Num(Ratio::new(1, 10).unwrap()), Ty::Num);
    let gain = law.var(Var::Gain, Ty::AMOUNT);
    let tax = law.bin(BinOp::Mul, tenth, gain, Ty::AMOUNT);
    let law = f.law(law.owe(tax, irs, penalty));
    let rule = f.rule(law, Subject::Place(retirement));
    f.on_gain.push((retirement, rule));
    f.flow(1, salary, retirement, 10_000_00);
    let withdrawal = f.flow(2, retirement, checking, 4_000_00);
    let book = f.book();
    let run = run(&book, options());

    assert_eq!(run.gains.len(), 1);
    let gain = run.gains[0];
    assert_eq!((gain.basis, gain.proceeds, gain.gain()), (Qty::ZERO, Qty(4_000_00), Qty(4_000_00)));
    assert_eq!(gain.cause, Cause::Flow(withdrawal));
    let effect = run.effects[0];
    assert_eq!((effect.amount.qty, effect.amount.unit, effect.name), (Qty(400_00), usd, penalty));
    assert_eq!(effect.owed(), Some(Owed { to: irs, due: Day(2) }));
    let left = held(&run, retirement, usd).unwrap();
    assert_eq!(
        (left.plain, left.lots.iter().map(|l| (l.qty.0, l.basis.0)).collect::<Vec<_>>()),
        (Qty::ZERO, vec![(6_000_00, 0)])
    );
    assert_eq!(held(&run, checking, usd).unwrap().plain, Qty(4_000_00), "taxed money arrives as plain money");
}

#[test]
fn a_prorata_place_realizes_only_the_lots_share_and_deferrals_merge_into_one_lot() {
    let mut f = Fixture::new();
    let (equity, salary, checking, retirement, usd) = (f.equity, f.salary, f.checking, f.retirement, f.usd);
    f.places[retirement].select = Some(Policy::Prorata);
    f.flow(1, equity, checking, 6_300_00);
    f.flow(2, checking, retirement, 6_300_00);
    f.flow(3, salary, retirement, 1_000_00);
    f.flow(9, salary, retirement, 1_200_00);
    f.flow(10, retirement, checking, 1_500_00);
    let book = f.book();
    let run = run(&book, options());
    assert!(run.diagnostics.is_empty(), "{:?}", run.diagnostics);
    let held = held(&run, retirement, usd).unwrap();
    assert_eq!(held.plain, Qty(6_300_00 - 1_111_76), "after-tax contributions are plain money");
    assert_eq!(held.lots.len(), 1, "two deferrals of zero-basis money are one lot");
    assert_eq!((held.lots[0].qty, held.lots[0].basis), (Qty(2_200_00 - 388_24), Qty::ZERO));
    assert_eq!(run.gains.len(), 1, "plain money never realizes");
    assert_eq!((run.gains[0].qty, run.gains[0].gain()), (Qty(388_24), Qty(388_24)));
}

#[test]
fn a_require_with_an_else_prices_the_violation_instead_of_failing() {
    let mut f = Fixture::new();
    let (salary, retirement, checking, irs) = (f.salary, f.retirement, f.checking, f.grant);
    let (name, penalty) = (f.sym("early-withdrawal"), f.sym("penalty"));
    let mut law = LawBuilder::new(name, Trigger::Gain);
    let gain = law.var(Var::Gain, Ty::AMOUNT);
    let nothing = law.konst(Value::Empty, Ty::Empty);
    let cond = law.bin(BinOp::Le, gain, nothing, Ty::Bool);
    let tenth = law.konst(Value::Num(Ratio::new(1, 10).unwrap()), Ty::Num);
    let again = law.var(Var::Gain, Ty::AMOUNT);
    let tax = law.bin(BinOp::Mul, tenth, again, Ty::AMOUNT);
    let law = f.law(law.require_else_owe(cond, tax, irs, penalty));
    let rule = f.rule(law, Subject::Place(retirement));
    f.on_gain.push((retirement, rule));
    f.flow(1, salary, retirement, 5_000_00);
    f.flow(2, retirement, checking, 1_000_00);
    let book = f.book();
    let run = run(&book, options());
    assert_eq!(run.effects.len(), 1);
    let [effect] = run.effects[..] else { panic!("{:?}", run.effects) };
    assert_eq!((effect.amount.qty, effect.name, effect.is_penalty()), (Qty(100_00), penalty, true));
    let [violation] = run.violations[..] else { panic!("one priced violation: {:?}", run.violations) };
    assert_eq!(violation.verdict, Verdict::Priced { waived: false });
    let d = &run.diagnostics[violation.diagnostic as usize];
    assert_eq!((&*d.code, d.severity, d.disposition), ("early-withdrawal", Severity::Note, Disposition::Priced));
    assert_eq!(d.message, "100.00 USD owed to nsf-grant as penalty, due 1970-01-03", "what is owed, to whom, and by when");
    assert!(run.diagnostics.iter().all(|d| !d.is_error()), "a priced violation is a price, not a failure");
}

#[test]
fn restricted_money_stays_tied_and_is_spent_first_only_where_its_laws_permit() {
    let mut f = Fixture::new();
    let (equity, grants, checking, savings, food, unknown, usd) =
        (f.equity, f.grants, f.checking, f.savings, f.food, f.unknown, f.usd);
    let (nsf, name) = (f.grant, f.sym("grant-purpose"));
    let mut law = LawBuilder::new(name, Trigger::Spend);
    let to = law.var(Var::To, Ty::Place);
    let allowed = law.konst(Value::Place(food), Ty::Place);
    let cond = law.is(to, &[allowed]);
    let law = f.law(law.require(cond, None));
    let rule = f.rule(law, Subject::Entity(nsf));
    f.on_spend.push((nsf, rule));

    f.flow(1, equity, checking, 1_000_00);
    let award = f.flow(2, grants, checking, 5_000_00);
    f.flows[award.index()].payee = Some(nsf);
    f.flow(3, checking, food, 800_00);
    let misuse = f.flow(4, checking, unknown, 1_200_00);
    f.flow(5, checking, savings, 500_00);
    let book = f.book();
    let run = run(&book, options());

    let tied = |amount: i64, txn: u32| Parcel {
        qty: Qty(amount),
        basis: Qty(amount),
        acquired: Day(2),
        txn: RuntimeTxn::journal(Id::new(txn)).unwrap(),
        codes: FlowCodes {
            header: axiom_core::Run::new(Id::new(0), 0),
            local: axiom_core::Run::new(Id::new(0), 0),
        },
        tied: Some(nsf),
    };
    let checking_held = held(&run, checking, usd).unwrap();
    assert_eq!(checking_held.plain, Qty::ZERO);
    assert_eq!(checking_held.lots, [tied(3_500_00, 1)], "800 went on the purpose, then plain 1,000 before tied 200");
    let savings_held = held(&run, savings, usd).unwrap();
    assert_eq!(savings_held.lots, [tied(500_00, 1)], "the tie travels with an internal transfer");
    assert_eq!(run.checks[law.index()], 2, "it fires only for tied money leaving the owner's places");
    assert_eq!(run.violations.len(), 1);
    assert_eq!(run.violations[0].cause, Cause::Flow(misuse));
    assert_eq!(run.violations[0].subject, Subject::Entity(nsf));
}

#[test]
fn a_flow_between_places_that_hold_no_parcels_spends_nothing() {
    let mut f = Fixture::new();
    let (grants, checking, food, unknown, card) = (f.grants, f.checking, f.food, f.unknown, f.card);
    let (nsf, name) = (f.grant, f.sym("grant-purpose"));
    let mut law = LawBuilder::new(name, Trigger::Spend);
    let to = law.var(Var::To, Ty::Place);
    let allowed = law.konst(Value::Place(food), Ty::Place);
    let cond = law.is(to, &[allowed]);
    let law = f.law(law.require(cond, None));
    let rule = f.rule(law, Subject::Entity(nsf));
    f.on_spend.push((nsf, rule));

    let award = f.flow(1, grants, checking, 5_000_00);
    f.flows[award.index()].payee = Some(nsf);
    f.flow(2, checking, unknown, 500_00);
    // The card and the food account hold only a balance: the grant money spent a day before is not spent again.
    f.flow(3, card, food, 100_00);
    let book = f.book();
    let run = run(&book, options());
    assert_eq!(run.checks[law.index()], 1, "only the flow that took tied money out fired it");
    assert_eq!(run.violations.len(), 1);
}

#[test]
fn a_pending_flow_lands_on_its_settlement_day_and_a_typo_gets_a_suggestion() {
    let mut f = Fixture::new();
    let (equity, checking, food, usd) = (f.equity, f.checking, f.food, f.usd);
    f.flow(1, equity, checking, 100_00);
    let check = f.flow(2, checking, food, 50_00);
    f.pending(check, "#c1");
    f.event(5, "#c1", EventState::Settled);
    f.event(6, "#c2", EventState::Void);
    let book = f.book();
    let plan = Plan::new(&book);
    let mut ledger = plan.start(options());
    ledger.advance(Day(4));
    assert_eq!(ledger.balance(checking, usd), Qty(100_00), "pending money has not moved");
    ledger.advance(Day(5));
    assert_eq!(ledger.balance(checking, usd), Qty(50_00));
    let run = ledger.finish();
    assert_eq!(run.posted[check.index()].state, State::Settled(Day(5)));
    let d = diagnostic(&run, "unknown-code");
    assert_eq!(d.help[0].text, "did you mean `#c1`?");
}

/// A monthly `count` law and a `by` law due on 1970-02-10 (day 40), over one flow on day 1.
fn timed_book(assertion_on: Option<i32>) -> (Book<'static>, Id<Law>) {
    let mut f = Fixture::new();
    let (equity, cash, me) = (f.equity, f.cash, f.me);
    let (monthly, deadline, months) = (f.sym("monthly"), f.sym("payoff"), f.sym("months"));
    let one = f.usd(100);
    let mut each = LawBuilder::new(monthly, Trigger::Each(Period::Month, None));
    let amount = each.konst(Value::Amount(one), Ty::AMOUNT);
    let each = f.law(each.count(amount, months));
    let mut by = LawBuilder::new(deadline, Trigger::Always);
    let (y, m, d) = (
        by.konst(Value::Num(Ratio::int(1970)), Ty::Num),
        by.konst(Value::Num(Ratio::int(2)), Ty::Num),
        by.konst(Value::Num(Ratio::int(10)), Ty::Num),
    );
    let date = by.call(Func::Date, &[y, m, d], Ty::Day);
    by.by(date);
    let balance = by.var(Var::Balance, Ty::AMOUNT);
    let nothing = by.konst(Value::Empty, Ty::Empty);
    let cond = by.bin(BinOp::Eq, balance, nothing, Ty::Bool);
    let by = f.law(by.require(cond, None));
    let (each_rule, by_rule) = (f.rule(each, Subject::Entity(me)), f.rule(by, Subject::Place(cash)));
    f.timed.extend([each_rule, by_rule]);
    f.flow(1, equity, cash, 5_00);
    if let Some(day) = assertion_on {
        f.assert(day, cash, 5_00);
    }
    (f.book(), each)
}

#[test]
fn balance_reads_in_the_display_sign_and_a_lasting_condition_is_reported_once() {
    let mut f = Fixture::new();
    let (card, checking, food, equity) = (f.card, f.checking, f.food, f.equity);
    let (name, limit) = (f.sym("card-limit"), f.usd(100_00));
    let mut law = LawBuilder::new(name, Trigger::Always);
    let owed = law.var(Var::Balance, Ty::AMOUNT);
    let cap = law.konst(Value::Amount(limit), Ty::AMOUNT);
    let cond = law.bin(BinOp::Le, owed, cap, Ty::Bool);
    let law = f.law(law.warn(cond));
    let rule = f.rule(law, Subject::Place(card));
    f.always.push((card, rule));
    f.flow(1, equity, checking, 1_000_00);
    f.flow(2, card, food, 84_00);
    let over = f.flow(3, card, food, 30_00);
    f.flow(4, card, food, 5_00);
    f.flow(5, checking, card, 60_00);
    let again = f.flow(6, card, food, 60_00);
    let book = f.book();
    let run = run(&book, options());
    let causes: Vec<_> = run.violations.iter().map(|v| v.cause).collect();
    assert_eq!(
        causes,
        [Cause::Flow(over), Cause::Flow(again)],
        "owed 114, still owed 119 (quiet), paid down to 59, owed 119"
    );
    let d = &run.diagnostics[run.violations[0].diagnostic as usize];
    assert!(d.labels.iter().any(|l| l.text == "114.00 USD"), "what is owed, not the negative balance: {d:?}");
}

#[test]
fn a_returned_flow_is_reversed_on_the_day_of_the_return() {
    let mut f = Fixture::new();
    let (equity, checking, food, usd) = (f.equity, f.checking, f.food, f.usd);
    f.flow(1, equity, checking, 100_00);
    let bounced = f.flow(2, checking, food, 30_00);
    let instant = f.flow(2, checking, food, 5_00);
    f.mark(bounced, "#b1");
    f.mark(instant, "#b2");
    f.event(5, "#b1", EventState::Returned);
    f.event(2, "#b2", EventState::Returned);
    let book = f.book();
    let plan = Plan::new(&book);
    let mut ledger = plan.start(options());
    ledger.advance(Day(4));
    assert_eq!(ledger.balance(checking, usd), Qty(70_00), "the returned flow counted until it was returned");
    ledger.advance(Day(5));
    assert_eq!(ledger.balance(checking, usd), Qty(100_00), "and its reversal put everything back");
    let run = ledger.finish();
    assert_eq!(run.posted[bounced.index()].state, State::Returned(Day(5)));
    assert_eq!(run.posted[instant.index()].state, State::Void, "returned on its own day: it never happened");
    assert_eq!(qty(&run, food, usd), 0);
}

#[test]
fn periods_and_deadlines_fire_as_the_journal_reaches_them_and_never_past_today() {
    let (book, each) = timed_book(None);
    let run = run(&book, Options { today: Day(70), relaxed: false });
    let days: Vec<_> = run.effects.iter().map(|e| (e.day, e.cause)).collect();
    assert_eq!(
        days,
        [(Day(30), Cause::Time), (Day(58), Cause::Time)],
        "January and February closed; March 31 is past today"
    );
    assert_eq!(run.violations.len(), 1);
    assert_eq!((run.violations[0].day, run.violations[0].cause), (Day(40), Cause::Time));
    assert_eq!(run.checks[each.index()], 2);
}

#[test]
fn a_deadline_the_journal_itself_reaches_fires_even_before_today() {
    let (book, _) = timed_book(Some(45));
    let run = run(&book, Options { today: Day(10), relaxed: false });
    assert_eq!(run.violations.len(), 1, "the assertion on day 45 carries the journal past the deadline on day 40");
    assert_eq!(
        run.effects.iter().map(|e| e.day).collect::<Vec<_>>(),
        [Day(30)],
        "and past January's end, but no further"
    );
    let (book, _) = timed_book(None);
    let quiet = crate::run(&book, Options { today: Day(10), relaxed: false });
    assert!(
        quiet.violations.is_empty() && quiet.effects.is_empty(),
        "with nothing after today, the future stays unfired"
    );
}

#[test]
fn tallies_count_what_a_filter_lets_through_and_year_end_laws_read_them() {
    let mut f = Fixture::new();
    let (equity, salary, checking, me, irs) = (f.equity, f.salary, f.checking, f.me, f.grant);
    let (counting, taxing, wages, tax) =
        (f.sym("count-wages"), f.sym("income-tax"), f.sym("wages"), f.sym("federal-tax"));

    let mut count = LawBuilder::new(counting, Trigger::In);
    let (from, source) = (count.var(Var::From, Ty::Place), count.konst(Value::Place(salary), Ty::Place));
    let paid_by_employer = count.is(from, &[source]);
    let amount = count.var(Var::Amount, Ty::AMOUNT);
    let count = f.law(count.when(paid_by_employer).count(amount, wages));
    let rule = f.rule(count, Subject::Entity(me));
    f.on_in.push((checking, rule));

    let mut owe = LawBuilder::new(taxing, Trigger::Each(Period::Year, None));
    let (rate, earned) =
        (owe.konst(Value::Num(Ratio::new(1, 10).unwrap()), Ty::Num), owe.call(Func::Tally(wages), &[], Ty::AMOUNT));
    let due = owe.bin(BinOp::Mul, rate, earned, Ty::AMOUNT);
    let owe = f.law(owe.owe(due, irs, tax));
    let rule = f.rule(owe, Subject::Entity(me));
    f.timed.push(rule);

    f.flow(1, salary, checking, 3_000_00);
    f.flow(2, equity, checking, 1_000_00);
    f.flow(40, salary, checking, 2_000_00);
    let book = f.book();
    let run = run(&book, Options { today: Day(400), relaxed: false });
    let counted: Vec<_> = run.effects.iter().filter(|e| e.name == wages).map(|e| e.amount.qty.0).collect();
    assert_eq!(counted, [3_000_00, 2_000_00], "the equity inflow is filtered out by `when`");
    assert_eq!(run.checks[count.index()], 2);
    let owed = run.effects.iter().find(|e| e.name == tax).unwrap();
    assert_eq!(
        (owed.amount.qty, owed.day, owed.owner),
        (Qty(500_00), Day(364), me),
        "10% of the year's wages, at December 31"
    );
}

#[test]
fn a_warn_law_is_a_warning_and_says_how_much_room_is_left() {
    let mut f = Fixture::new();
    let (checking, food) = (f.checking, f.food);
    let (name, budget) = (f.sym("budget"), f.usd(100_00));
    let mut law = LawBuilder::new(name, Trigger::In);
    let total = law.call(Func::Total(Dir::In, Window::Month), &[], Ty::AMOUNT);
    let cap = law.konst(Value::Amount(budget), Ty::AMOUNT);
    let cond = law.bin(BinOp::Le, total, cap, Ty::Bool);
    let law = f.law(law.warn(cond));
    let rule = f.rule(law, Subject::Place(food));
    f.on_in.push((food, rule));
    f.flow(1, checking, food, 80_00);
    f.flow(2, checking, food, 50_00);
    let book = f.book();
    let run = run(&book, options());
    assert_eq!(run.violations.len(), 1);
    assert_eq!(run.violations[0].verdict, Verdict::Warns);
    let d = &run.diagnostics[run.violations[0].diagnostic as usize];
    assert_eq!(d.severity, Severity::Warning);
    assert_eq!(d.help[0].text, "at most 20.00 USD more can go in this month");
}

#[test]
fn a_fork_reports_only_what_it_causes() {
    let mut f = Fixture::new();
    let (equity, checking, retirement, salary) = (f.equity, f.checking, f.retirement, f.salary);
    f.flow(1, equity, checking, 1_000_00);
    f.buy(2, 500_00, 5);
    f.sell(3, 5, 600_00);
    f.flow(4, salary, retirement, 10_000_00);
    let book = f.book();
    let plan = Plan::new(&book);
    let mut ledger = plan.start(options());
    ledger.advance(Day(5));
    let mut withdrawal = book.flows[Id::new(3)].clone();
    (withdrawal.from, withdrawal.to, withdrawal.day) = (retirement, checking, Day(5));
    (withdrawal.out.qty, withdrawal.arrive.qty) = (Qty(2_000_00), Qty(2_000_00));

    let (mut fork, mut copy) = (ledger.fork(), ledger.clone());
    let (applied, also) = (fork.apply(&withdrawal), copy.apply(&withdrawal));
    assert_eq!((applied.gains, also.gains), (0..1, 1..2), "the copy remembers the journal's sale; the fork does not");
    let (fork, copy) = (fork.finish(), copy.finish());
    assert_eq!((fork.gains.len(), copy.gains.len()), (1, 2));
    assert_eq!(fork.gains[0].gain(), Qty(2_000_00), "pre-tax money: all of it is gain");
    assert_eq!(ledger.finish().gains.len(), 1, "and the original is untouched");
}

#[test]
fn a_clone_diverges_without_touching_the_original() {
    let mut f = Fixture::new();
    let (equity, checking, cash, usd) = (f.equity, f.checking, f.cash, f.usd);
    f.flow(1, equity, checking, 1_000_00);
    f.flow(9, checking, cash, 1_00);
    let book = f.book();
    let plan = Plan::new(&book);
    let mut ledger = plan.start(options());
    ledger.advance(Day(3));
    let mut fork = ledger.clone();
    let mut withdrawal = book.flows[Id::new(0)].clone();
    (withdrawal.from, withdrawal.to, withdrawal.day) = (checking, cash, Day(3));
    (withdrawal.out.qty, withdrawal.arrive.qty) = (Qty(250_00), Qty(250_00));
    let applied = fork.apply(&withdrawal);
    assert!(applied.gains.is_empty() && applied.violations.is_empty());
    assert_eq!((ledger.balance(cash, usd), fork.balance(cash, usd)), (Qty::ZERO, Qty(250_00)));
    fork.advance(Day(20));
    ledger.advance(Day(20));
    assert_eq!((ledger.balance(cash, usd), fork.balance(cash, usd)), (Qty(1_00), Qty(251_00)));
    assert_eq!(
        fork.finish().holdings.iter().filter(|h| h.unit == usd && h.place == checking).map(|h| h.qty().0).sum::<i64>(),
        749_00
    );
}

/// A year's first four months of a checking account: the salary in January
/// and `spent` on food in each of the months after, then `extra` flows.
fn four_months(spent: [i64; 3], extra: &[(Day, bool, i64)]) -> Book<'static> {
    let mut f = Fixture::new();
    let (equity, checking, food, cash) = (f.equity, f.checking, f.food, f.cash);
    f.flow(date(2026, 1, 5), equity, checking, 1_000_00);
    for (month, cents) in (2..=4).zip(spent) {
        f.flow(date(2026, month, 10), checking, food, cents);
        for &(day, out, cents) in extra.iter().filter(|(day, ..)| day.ymd().1 == month) {
            let (from, to) = if out { (checking, cash) } else { (cash, checking) };
            f.flow(day.0, from, to, cents);
        }
    }
    f.book()
}

/// What each place holds, as a fold stands or ends.
fn balances<'a>(holdings: impl Iterator<Item = &'a Holding>) -> Vec<(Id<Place>, i64)> {
    holdings.map(|h| (h.place, h.qty().0)).collect()
}

#[test]
fn a_refold_resumes_from_a_checkpoint_and_stops_where_it_meets_the_old_fold() {
    let end = Day(date(2026, 4, 30));
    let options = Options { today: end, relaxed: false };
    let mut old_ends = Vec::new();
    let original = four_months([100_00, 50_00, 20_00], &[]);
    Plan::new(&original).start(options).advance_by_month(end, |checkpoint| {
        old_ends.push(checkpoint);
        true
    });
    assert_eq!(old_ends.iter().map(|c| c.day().ymd().1).collect::<Vec<_>>(), [1, 2, 3, 4], "one at each month's end");

    // An edit in March that nets out by month's end: 30 out to cash on the 12th, and back on the 25th.
    let edited = four_months([100_00, 50_00, 20_00], &[(Day(date(2026, 3, 12)), true, 30_00), (Day(date(2026, 3, 25)), false, 30_00)]);
    let plan = Plan::new(&edited);
    let before = old_ends.iter().rfind(|c| c.day() < Day(date(2026, 3, 12))).expect("a checkpoint before the edit");
    let mut ledger = plan.resume(before, options);
    let mut seen = Vec::new();
    ledger.advance_by_month(end, |checkpoint| {
        let old = old_ends.iter().find(|old| old.day() == checkpoint.day()).expect("the old fold's month end");
        seen.push(checkpoint.day().ymd().1);
        old.digest() != checkpoint.digest()
    });
    assert_eq!(seen, [3], "March ends as it did: nothing after it needs refolding");
    let mut fresh = plan.start(options);
    fresh.advance(Day(date(2026, 3, 31)));
    assert_eq!(balances(ledger.holdings()), balances(fresh.holdings()), "and it stands where a full fold stands");

    // An edit that changes what March ends with is followed to the end, and ends where a full fold does.
    let changed = four_months([100_00, 60_00, 20_00], &[]);
    let plan = Plan::new(&changed);
    let mut ledger = plan.resume(before, options);
    let mut seen = Vec::new();
    ledger.advance_by_month(end, |checkpoint| {
        let old = old_ends.iter().find(|old| old.day() == checkpoint.day()).expect("the old fold's month end");
        seen.push(checkpoint.day().ymd().1);
        old.digest() != checkpoint.digest()
    });
    assert_eq!(seen, [3, 4], "April differs too, because March did");
    assert_eq!(balances(ledger.finish().holdings.iter()), balances(run(&changed, options).holdings.iter()));
}

#[test]
fn deadlines_beyond_the_horizon_wait_until_the_ledger_is_asked_to_reach_them() {
    let (book, _) = timed_book(None);
    let plan = Plan::new(&book);
    let mut ledger = plan.start(Options { today: Day(30), relaxed: false });
    ledger.advance(Day(70));
    assert!(ledger.recorded().violations.is_empty(), "the payoff date, February 10, is past today");
    ledger.reach(Day(70));
    ledger.advance(Day(70));
    assert_eq!(ledger.recorded().violations.len(), 1);
}

#[test]
fn a_resumed_fold_meets_the_deadlines_still_to_come_and_not_those_already_passed() {
    let (book, each) = timed_book(None);
    let options = Options { today: Day(70), relaxed: false };
    let plan = Plan::new(&book);
    let mut january = None;
    plan.start(options).advance_by_month(Day(30), |checkpoint| {
        january = Some(checkpoint);
        true
    });
    let mut resumed = plan.resume(&january.expect("January's end"), options);
    resumed.advance(Day(70));
    let (resumed, whole) = (resumed.finish(), plan.run(options));
    assert_eq!((resumed.checks[each.index()], whole.checks[each.index()]), (1, 2), "January closed before it");
    assert_eq!((resumed.violations.len(), whole.violations.len()), (1, 1), "the deadline on February 10 came after");
}

#[test]
fn a_view_forks_the_ledger_the_run_stood_at_instead_of_folding_again() {
    let mut f = Fixture::new();
    let (equity, checking, cash, usd) = (f.equity, f.checking, f.cash, f.usd);
    f.flow(date(2026, 1, 5), equity, checking, 1_000_00);
    f.flow(date(2026, 3, 1), checking, cash, 100_00);
    let book = f.book();
    let options = Options { today: Day(date(2026, 2, 1)), relaxed: false };
    let plan = Plan::new(&book);
    let (run, view) = plan.run_with_view(options);
    assert_eq!(held(&run, cash, usd).map(|h| h.qty().0), Some(100_00), "the run goes on to the journal's last fact");
    assert_eq!((view.balance(checking, usd), view.balance(cash, usd)), (Qty(1_000_00), Qty::ZERO), "the view stands at today");
    let mut fork = view.fork();
    fork.advance(Day(date(2026, 3, 31)));
    assert_eq!(fork.balance(cash, usd), Qty(100_00));
    assert_eq!(view.balance(cash, usd), Qty::ZERO, "and forking leaves it alone");

    // Forks run side by side, each borrowing the one plan.
    let template = book.flows[Id::new(1)].clone();
    let moved = axiom_core::par::map_each(&[10_00, 20_00, 30_00], |&cents| {
        let mut flow = template.clone();
        (flow.day, flow.out.qty, flow.arrive.qty) = (Day(date(2026, 2, 1)), Qty(cents), Qty(cents));
        let mut fork = view.fork();
        fork.apply(&flow);
        fork.balance(cash, usd)
    });
    assert_eq!(moved, [Qty(10_00), Qty(20_00), Qty(30_00)]);
}

/// A small deterministic generator: xorshift64*.
struct Dice(u64);

impl Dice {
    fn roll(&mut self, below: u64) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 33) % below
    }
}

/// Whatever the journal says, value is neither created nor destroyed: each
/// commodity's balances sum to what flowed in less what flowed out, basis is
/// carried or relieved but never lost, lots hold positive quantities in a
/// stable order, and no two lots are interchangeable.
#[test]
fn value_is_conserved_over_random_journals() {
    for seed in 1..=8u64 {
        let mut dice = Dice(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let mut f = Fixture::new();
        let (equity, salary, food, usd, vti) = (f.equity, f.salary, f.food, f.usd, f.vti);
        let (checking, savings, cash, brokerage, retirement) =
            (f.checking, f.savings, f.cash, f.brokerage, f.retirement);
        f.places[brokerage].select =
            [None, Some(Policy::Fifo), Some(Policy::Hifo), Some(Policy::Prorata)][seed as usize % 4];
        f.places[retirement].select = Some(Policy::Prorata);
        let pool = [checking, savings, cash, retirement, food];
        f.flow(1, equity, checking, 50_000_00);
        let mut expected = std::collections::BTreeMap::<Id<Commodity>, i64>::new();
        let mut paid = 0;
        for i in 0..3_000 {
            let day = 2 + i / 20;
            match dice.roll(8) {
                0 => {
                    let (shares, cost) = (1 + dice.roll(20) as i64, 100_00 + dice.roll(5_000_00) as i64);
                    f.buy(day, cost, shares);
                    paid += cost;
                    *expected.entry(vti).or_default() += shares;
                    *expected.entry(usd).or_default() -= cost;
                }
                1 | 2 => {
                    let (shares, proceeds) = (1 + dice.roll(25) as i64, 100_00 + dice.roll(6_000_00) as i64);
                    let sale = f.sell(day, shares, proceeds);
                    if dice.roll(3) == 0 {
                        let from = (2 + dice.roll(day as u64) as i32).min(day);
                        f.select(sale, [Select::Range(span(from, day))]);
                    }
                    *expected.entry(vti).or_default() -= shares;
                    *expected.entry(usd).or_default() += proceeds;
                }
                3 => {
                    f.flow(day, salary, retirement, 1_000_00 + dice.roll(500_00) as i64);
                }
                _ => {
                    let (from, to) = (pool[dice.roll(5) as usize], pool[dice.roll(5) as usize]);
                    f.flow(day, from, to, 1_00 + dice.roll(2_000_00) as i64);
                }
            }
        }
        let book = f.book();
        let run = run(&book, Options { today: Day(1_000), relaxed: false });

        let mut totals = std::collections::BTreeMap::<Id<Commodity>, i64>::new();
        for holding in &run.holdings {
            *totals.entry(holding.unit).or_default() += holding.qty().0;
            assert!(
                holding.lots.iter().all(|lot| lot.qty > Qty::ZERO && lot.basis >= Qty::ZERO),
                "seed {seed}: {holding:?}"
            );
            assert!(
                holding.lots.windows(2).all(|pair| pair[0].acquired <= pair[1].acquired),
                "seed {seed}: lots out of order"
            );
            let is_base = holding.unit == usd;
            let alike = |a: &Parcel, b: &Parcel| crate::lots::identity(a, is_base) == crate::lots::identity(b, is_base);
            for (at, lot) in holding.lots.iter().enumerate() {
                assert!(
                    holding.lots[at + 1..].iter().all(|other| !alike(lot, other)),
                    "seed {seed}: unmerged lots in {holding:?}"
                );
            }
        }
        // Every dollar paid for shares is still the basis of a lot or was relieved with a sale.
        let held = held(&run, brokerage, vti).map_or(0, |h| h.lots.iter().map(|lot| lot.basis.0).sum::<i64>());
        let sold: i64 = run.gains.iter().filter(|g| g.unit == vti).map(|g| g.basis.0).sum();
        assert_eq!(paid, held + sold, "seed {seed}: basis leaked");
        // Transfers move value between places; only exchanges change a commodity's total.
        for (unit, want) in expected {
            assert_eq!(totals.get(&unit).copied().unwrap_or(0), want, "seed {seed}: unit {unit:?}");
        }
    }
}

// ─── v3 semantics ───────────────────────────────────────────────────────────

fn date(year: i32, month: u32, day: u32) -> i32 {
    Day::from_ymd(year, month, day).expect("a real date").0
}

fn until(year: i32, month: u32, day: u32) -> Options {
    Options { today: Day(date(year, month, day)), relaxed: false }
}

/// `count 1.00 USD as NAME`, on flows into `place`.
fn ticking(f: &mut Fixture, place: Id<Place>, name: &'static str) -> Id<Law> {
    let (law_name, tally, one) = (f.sym(name), f.sym(name), f.usd(1_00));
    let mut law = LawBuilder::new(law_name, Trigger::In);
    let amount = law.konst(Value::Amount(one), Ty::AMOUNT);
    let law = f.law(law.count(amount, tally));
    let rule = f.rule(law, Subject::Place(place));
    f.on_in.push((place, rule));
    law
}

/// `count amount as NAME`, on flows into `place`, for `subject`.
fn counting(f: &mut Fixture, place: Id<Place>, subject: Subject, name: &'static str) -> Id<Law> {
    let (law_name, tally) = (f.sym(name), f.sym(name));
    let mut law = LawBuilder::new(law_name, Trigger::In);
    let amount = law.var(Var::Amount, Ty::AMOUNT);
    let law = f.law(law.count(amount, tally));
    let rule = f.rule(law, subject);
    f.on_in.push((place, rule));
    law
}

/// `warn total(in, month) <= cap`, on flows into `place`.
fn month_budget(f: &mut Fixture, place: Id<Place>, cap: i64) -> Id<Law> {
    let (name, cap) = (f.sym("budget"), f.usd(cap));
    let mut law = LawBuilder::new(name, Trigger::In);
    let total = law.call(Func::Total(Dir::In, Window::Month), &[], Ty::AMOUNT);
    let cap = law.konst(Value::Amount(cap), Ty::AMOUNT);
    let cond = law.bin(BinOp::Le, total, cap, Ty::Bool);
    let law = f.law(law.warn(cond));
    let rule = f.rule(law, Subject::Place(place));
    f.on_in.push((place, rule));
    law
}

fn lots_of(run: &Run, place: Id<Place>, unit: Id<Commodity>) -> Vec<(i64, i64)> {
    held(run, place, unit).map_or(Vec::new(), |h| h.lots.iter().map(|lot| (lot.qty.0, lot.basis.0)).collect())
}

#[test]
fn a_partial_sale_shortfall_does_not_duplicate_the_missing_lot_slice() {
    let mut f = Fixture::new();
    let (equity, checking, brokerage, usd, vti) = (f.equity, f.checking, f.brokerage, f.usd, f.vti);
    f.flow(1, equity, checking, 1_000_00);
    f.buy(2, 700_00, 7);
    f.sell(3, 10, 2_000_00);

    let run = run(&f.book(), options());

    assert_eq!(run.gains.len(), 1);
    let gain = run.gains[0];
    assert_eq!((gain.qty, gain.basis, gain.proceeds, gain.gain()), (Qty(7), Qty(700_00), Qty(1_400_00), Qty(700_00)));
    assert_eq!(qty(&run, brokerage, vti), -3, "a sale shortfall is posted as an explicit negative holding");
    assert_eq!(qty(&run, checking, usd), 1_000_00 + 2_000_00 - 700_00);
}

#[test]
fn a_transferred_lot_keeps_its_source_code_for_a_later_selector() {
    let mut f = Fixture::new();
    let (equity, checking, brokerage, savings, vti) = (f.equity, f.checking, f.brokerage, f.savings, f.vti);
    f.flow(1, equity, checking, 1_000_00);
    let purchase = f.buy(2, 700_00, 7);
    f.mark_txn(purchase, "#original-purchase");
    let shares = f.vti(7);
    f.exchange(3, brokerage, shares, savings, shares);
    let (one_share, proceeds) = (f.vti(1), f.usd(200_00));
    let sale = f.exchange(4, savings, one_share, checking, proceeds);
    let code = f.sym("#original-purchase");
    f.select(sale, [Select::Code(code)]);

    let run = run(&f.book(), options());

    assert_eq!(run.gains.len(), 1, "the transferred lot still matches its original transaction code");
    assert_eq!((run.gains[0].qty, run.gains[0].basis, run.gains[0].proceeds), (Qty(1), Qty(100_00), Qty(200_00)));
    assert_eq!(lots_of(&run, savings, vti), [(6, 600_00)]);
}

#[test]
fn a_spread_flow_counts_in_each_month_it_touches_as_the_fold_reaches_it() {
    let mut f = Fixture::new();
    let (checking, food) = (f.checking, f.food);
    month_budget(&mut f, food, 30_00);
    // 71 days: 12 fall in December, 31 in January, 28 in February.
    let prepaid = f.flow(date(2025, 12, 20), checking, food, 70_00);
    f.recognize(prepaid, date(2025, 12, 20), date(2026, 2, 28));
    f.flow(date(2026, 1, 5), checking, food, 1_00);
    let book = f.book();
    let run = run(&book, until(2026, 3, 1));
    let [violation] = run.violations[..] else { panic!("one violation: {:?}", run.violations) };
    assert_eq!(violation.day, Day(date(2026, 1, 1)), "nothing in December: only 11.83 of it belongs there");
    assert_eq!(violation.cause, Cause::Time, "January opened already over: no flow crossed the line");
    let d = &run.diagnostics[violation.diagnostic as usize];
    assert_eq!(d.message, "expenses/food: 30.56 USD in 2026-01 against a limit of 30.00 USD, over by 0.56 USD");
    let counted: Vec<_> =
        run.headroom.iter().map(|h| (h.days.first().ymd().1, h.counted.qty.0, h.limit.qty.0)).collect();
    assert_eq!(
        counted,
        [(12, 11_83, 30_00), (1, 31_56, 30_00), (2, 27_61, 30_00)],
        "the last reading of each month's window, February's with no flow in it"
    );
}

#[test]
fn plan_allocates_rolling_totals_only_for_subjects_a_law_reads() {
    let mut f = Fixture::new();
    let food = f.food;
    month_budget(&mut f, food, 30_00);
    let book = f.book();
    let plan = Plan::new(&book);
    assert_eq!(plan.watch.subjects(), &[Subject::Place(food)]);
}

#[test]
fn asset_law_scope_contains_each_parts_place_subtree() {
    let mut f = Fixture::new();
    let (checking, savings, food, usd, me) = (f.checking, f.savings, f.food, f.usd, f.me);
    let house_name = f.sym("house");
    let improvement_name = f.sym("improvement");
    let mut book = f.book();
    let house = book.assets.push(Asset {
        name: house_name,
        kind: book.roots.kinds.thing,
        owner: me,
        place: checking,
        unit: usd,
        part_of: None,
        props: Box::default(),
        doc: None,
        loc: Loc::new(FileId(0), 1, 2),
    });
    book.assets.push(Asset {
        name: improvement_name,
        kind: book.roots.kinds.thing,
        owner: me,
        place: savings,
        unit: usd,
        part_of: Some(At { value: house, loc: Loc::default() }),
        props: Box::default(),
        doc: None,
        loc: Loc::new(FileId(0), 3, 4),
    });
    let plan = Plan::new(&book);
    assert!(plan.inside(Subject::Asset(house), checking));
    assert!(plan.inside(Subject::Asset(house), savings));
    assert!(!plan.inside(Subject::Asset(house), food));
}

#[test]
fn a_flow_recognized_for_last_year_counts_in_last_years_tally_and_a_closing_law_reads_it() {
    let mut f = Fixture::new();
    let (salary, checking, me, irs) = (f.salary, f.checking, f.me, f.grant);
    let (paid, tax, closing) = (f.sym("paid"), f.sym("tax"), f.sym("close-year"));
    counting(&mut f, checking, Subject::Entity(me), "paid");
    let mut law = LawBuilder::new(closing, Trigger::Each(Period::Year, Some(Closing { month: 4, day: 15 })));
    let so_far = law.call(Func::Tally(paid), &[], Ty::AMOUNT);
    let law = f.law(law.owe(so_far, irs, tax));
    let rule = f.rule(law, Subject::Entity(me));
    f.timed.push(rule);
    f.flow(date(2025, 6, 1), salary, checking, 3_000_00);
    let late = f.flow(date(2026, 1, 15), salary, checking, 500_00);
    f.recognize(late, date(2025, 1, 1), date(2025, 12, 31));
    let book = f.book();
    let run = run(&book, until(2026, 12, 31));
    let effects: Vec<_> = run.effects.iter().map(|e| (e.name == tax, e.day, e.amount.qty.0)).collect();
    assert_eq!(
        effects,
        [
            (false, Day(date(2025, 6, 1)), 3_000_00),
            (false, Day(date(2025, 1, 1)), 500_00),
            (true, Day(date(2025, 12, 31)), 3_500_00),
        ],
        "the January payment counts for 2025, and the closing law for 2025 sees it"
    );
    assert_eq!(run.effects[2].owed(), Some(Owed { to: irs, due: Day(date(2026, 4, 15)) }), "due the day the year closes");
    assert_eq!(run.checks[law.index()], 1, "2026 has not closed yet");
}

#[test]
fn a_count_over_a_range_splits_by_days_between_the_years() {
    let mut f = Fixture::new();
    let (salary, checking, me) = (f.salary, f.checking, f.me);
    counting(&mut f, checking, Subject::Entity(me), "paid");
    let plan = f.flow(date(2025, 7, 1), salary, checking, 365_00);
    f.recognize(plan, date(2025, 7, 1), date(2026, 6, 30));
    let book = f.book();
    let run = run(&book, until(2026, 7, 1));
    let parts: Vec<_> = run.effects.iter().map(|e| (e.day, e.amount.qty.0)).collect();
    assert_eq!(parts, [(Day(date(2025, 7, 1)), 184_00), (Day(date(2026, 1, 1)), 181_00)]);
}

#[test]
fn a_year_law_runs_for_a_part_year_residence() {
    let mut f = Fixture::new();
    let (equity, checking, me) = (f.equity, f.checking, f.me);
    let (yearly, years) = (f.sym("yearly"), f.sym("years"));
    let mut law = LawBuilder::new(yearly, Trigger::Each(Period::Year, None));
    let one = f.usd(1_00);
    let amount = law.konst(Value::Amount(one), Ty::AMOUNT);
    let law = f.law(law.count(amount, years));
    let subject = Subject::Entity(me);
    let (in_year, other_year) = (
        Rule { law, subject, days: span(date(2025, 3, 1), date(2025, 6, 30)) },
        Rule { law, subject, days: span(date(2024, 1, 1), date(2024, 12, 31)) },
    );
    f.timed.extend([in_year, other_year]);
    f.flow(date(2025, 2, 1), equity, checking, 10_00);
    let book = f.book();
    let run = run(&book, until(2026, 6, 1));
    assert_eq!(
        run.effects.iter().map(|e| e.day).collect::<Vec<_>>(),
        [Day(date(2025, 12, 31))],
        "it overlaps 2025, though it ended in June"
    );
}

#[test]
fn a_law_that_two_residences_bring_runs_once_for_a_period_and_once_for_a_flow() {
    let mut f = Fixture::new();
    let (equity, checking, me) = (f.equity, f.checking, f.me);
    let (yearly, years) = (f.sym("yearly"), f.sym("years"));
    let mut law = LawBuilder::new(yearly, Trigger::Each(Period::Year, None));
    let one = f.usd(1_00);
    let amount = law.konst(Value::Amount(one), Ty::AMOUNT);
    let law = f.law(law.count(amount, years));
    let subject = Subject::Entity(me);
    // A move within one system: the same law, brought by the residence before and the one after.
    let (before, after) = (
        Rule { law, subject, days: span(date(2024, 1, 1), date(2025, 6, 30)) },
        Rule { law, subject, days: span(date(2025, 7, 1), date(2030, 1, 1)) },
    );
    f.timed.extend([before, after]);
    // And two residences that overlap, so a flow meets the same law twice.
    let paid = counting(&mut f, checking, subject, "paid");
    let overlap = Rule { days: span(date(2025, 3, 1), i32::MAX), ..f.rule(paid, subject) };
    f.on_in.push((checking, overlap));
    f.flow(date(2025, 4, 1), equity, checking, 10_00);
    let book = f.book();
    let run = run(&book, until(2026, 6, 1));
    let closed: Vec<_> = run.effects.iter().filter(|e| e.name == years).map(|e| e.day).collect();
    assert_eq!(closed, [Day(date(2025, 12, 31))], "2025 is one period, whichever residence it touched");
    assert_eq!(run.checks[paid.index()], 1, "one flow, one run of the law");
}

#[test]
fn an_opening_moves_value_but_no_law_sees_it_and_it_starts_no_period() {
    let mut f = Fixture::new();
    let (equity, checking, brokerage, vti, me) = (f.equity, f.checking, f.brokerage, f.vti, f.me);
    let (monthly, months) = (f.sym("monthly"), f.sym("months"));
    let bought = ticking(&mut f, brokerage, "bought");
    let one = f.usd(1_00);
    let mut each = LawBuilder::new(monthly, Trigger::Each(Period::Month, None));
    let amount = each.konst(Value::Amount(one), Ty::AMOUNT);
    let each = f.law(each.count(amount, months));
    let rule = f.rule(each, Subject::Entity(me));
    f.timed.push(rule);
    let (shares, day) = (f.vti(10), date(2024, 12, 31));
    let opening = f.exchange(day, equity, shares, brokerage, shares);
    f.opening(opening);
    f.detail(opening, Detail { basis: Some(Qty(700_00)), since: Some(Day(date(2023, 6, 15))), ..Detail::default() });
    f.flow(date(2025, 3, 1), equity, checking, 1_000_00);
    f.buy(date(2025, 3, 10), 300_00, 1);
    let book = f.book();
    let run = run(&book, until(2025, 5, 31));
    assert_eq!(
        run.effects.iter().filter(|e| e.name == book.names.get("bought").unwrap()).count(),
        1,
        "only the purchase"
    );
    let month_days: Vec<_> =
        run.effects.iter().filter(|e| e.name == book.names.get("months").unwrap()).map(|e| e.day).collect();
    assert_eq!(
        month_days,
        [Day(date(2025, 3, 31)), Day(date(2025, 4, 30)), Day(date(2025, 5, 31))],
        "from the first real fact"
    );
    let lots: Vec<_> =
        held(&run, brokerage, vti).unwrap().lots.iter().map(|l| (l.qty.0, l.basis.0, l.acquired)).collect();
    assert_eq!(lots, [(10, 700_00, Day(date(2023, 6, 15))), (1, 300_00, Day(date(2025, 3, 10)))]);
    assert_eq!(run.checks[bought.index()], 1);
}

#[test]
fn a_split_scales_every_holding_of_the_commodity_and_keeps_basis_and_dates() {
    let mut f = Fixture::new();
    let (equity, checking, brokerage, vti, usd) = (f.equity, f.checking, f.brokerage, f.vti, f.usd);
    f.places[brokerage].select = Some(Policy::Fifo);
    f.flow(1, equity, checking, 10_000_00);
    f.buy(2, 1_000_00, 10);
    f.buy(3, 500_00, 5);
    f.split(5, vti, 2, 1);
    f.sell(5, 5, 300_00);
    let book = f.book();
    let run = run(&book, options());
    assert_eq!(lots_of(&run, brokerage, vti), [(15, 750_00), (10, 500_00)], "20 shares at 50.00 each, five sold");
    let gain = run.gains[0];
    assert_eq!((gain.qty, gain.basis, gain.gain(), gain.acquired), (Qty(5), Qty(250_00), Qty(50_00), Day(2)));
    assert_eq!(qty(&run, checking, usd), 10_000_00 - 1_500_00 + 300_00);
}

#[test]
fn a_stated_basis_and_a_hold_override_what_the_route_says() {
    let mut f = Fixture::new();
    let (salary, retirement, checking, savings, cash, grant, me) =
        (f.salary, f.retirement, f.checking, f.savings, f.cash, f.grant, f.me);
    f.join_household();
    let household = f.household;
    f.flow(1, salary, checking, 2_000_00);
    let gift = f.flow(2, salary, retirement, 6_000_00);
    f.detail(gift, Detail { basis: Some(Qty(6_000_00)), ..Detail::default() });
    f.flow(3, salary, retirement, 100_00);
    let set_aside = f.flow(4, checking, savings, 500_00);
    f.detail(set_aside, Detail { hold: Some(grant), ..Detail::default() });
    let mine = f.flow(5, savings, cash, 200_00);
    f.detail(mine, Detail { hold: Some(me), ..Detail::default() });
    let family = f.flow(6, savings, cash, 100_00);
    f.detail(family, Detail { hold: Some(household), ..Detail::default() });
    let book = f.book();
    let run = run(&book, options());
    let usd = book.base;
    let deferred = held(&run, retirement, usd).unwrap();
    assert_eq!(
        (deferred.plain, deferred.lots.iter().map(|l| (l.qty.0, l.basis.0)).collect::<Vec<_>>()),
        (Qty(6_000_00), vec![(100_00, 0)])
    );
    let tied: Vec<_> = held(&run, savings, usd).unwrap().lots.iter().map(|l| (l.qty.0, l.tied)).collect();
    assert_eq!(tied, [(200_00, Some(grant))], "200 left, still tied to the envelope");
    let wallet = held(&run, cash, usd).unwrap();
    assert_eq!((wallet.plain, wallet.lots.len()), (Qty(300_00), 0), "`for` the owner, or its household, unties");
}

#[test]
fn stated_basis_overrides_the_purchase_price_of_arriving_lots() {
    let mut f = Fixture::new();
    let (checking, brokerage, vti, usd) = (f.checking, f.brokerage, f.vti, f.usd);
    f.flow(1, f.equity, checking, 5_000_00);
    let purchase = f.buy(2, 1_000_00, 10);
    f.detail(purchase, Detail { basis: Some(Qty(1_150_00)), ..Detail::default() });
    let book = f.book();
    let run = run(&book, options());
    assert_eq!(lots_of(&run, brokerage, vti), [(10, 1_150_00)], "the stated acquisition basis replaces the cash price");
    assert!(run.gains.is_empty() && run.diagnostics.is_empty(), "an acquisition does not realize a gain: {:?}", run.diagnostics);
    assert_eq!((qty(&run, checking, usd), qty(&run, brokerage, usd)), (4_000_00, 0));
}

#[test]
fn stated_basis_can_initialize_a_new_asset_holding() {
    let mut f = Fixture::new();
    let (checking, brokerage, vti) = (f.checking, f.brokerage, f.vti);
    let equity = f.equity;
    f.flow(0, equity, checking, 100_00);
    let purchase = f.buy(1, 100_00, 1);
    f.detail(purchase, Detail { basis: Some(Qty(250_00)), ..Detail::default() });
    let book = f.book();
    let run = run(&book, options());
    assert_eq!(lots_of(&run, brokerage, vti), [(1, 250_00)]);
    assert_eq!(qty(&run, checking, book.base), 0);
    assert!(run.diagnostics.is_empty());
}

#[test]
fn claims_stay_apart_by_transaction_and_overdue_ones_are_reported_once_each() {
    let mut f = Fixture::new();
    let (salary, savings, checking, grant, usd) = (f.salary, f.savings, f.checking, f.grant, f.usd);
    f.places[savings].claim = true;
    let first = f.flow(1, salary, savings, 300_00);
    let second = f.flow(2, salary, savings, 300_00);
    f.claim(first, 30, grant);
    f.claim(second, 60, grant);
    f.mark_txn(first, "#inv-1");
    let code = f.sym("#inv-1");
    let partial = f.flow(10, savings, checking, 100_00);
    f.select(partial, [Select::Code(code)]);
    let book = f.book();
    let run = run(&book, Options { today: Day(45), relaxed: false });
    assert_eq!(
        lots_of(&run, savings, usd),
        [(200_00, 200_00), (300_00, 300_00)],
        "two claims, not one balance; the first partly paid"
    );
    let overdue: Vec<_> = run.diagnostics.iter().filter(|d| d.code == "overdue").collect();
    assert_eq!(overdue.len(), 1, "the second is not due until day 60");
    assert_eq!(overdue[0].message, "nsf-grant still owes 200.00 USD, 15 days past its due day 1970-01-31");
    assert_eq!(overdue[0].severity, Severity::Warning);
}

#[test]
fn growth_arrives_without_basis_and_a_loss_leaves_its_basis_with_what_remains() {
    let mut f = Fixture::new();
    let (equity, checking, brokerage, market, vti) = (f.equity, f.checking, f.brokerage, f.market, f.vti);
    f.places[brokerage].select = Some(Policy::Fifo);
    f.flow(1, equity, checking, 5_000_00);
    f.buy(2, 600_00, 6);
    f.buy(3, 400_00, 4);
    let (grown, lost) = (f.vti(2), f.vti(3));
    f.exchange(4, market, grown, brokerage, grown);
    f.exchange(5, brokerage, lost, market, lost);
    let ins = ticking(&mut f, brokerage, "grown");
    let book = f.book();
    let run = run(&book, options());
    assert!(run.gains.is_empty(), "a market moving an asset's worth realizes nothing");
    let lots = lots_of(&run, brokerage, vti);
    assert_eq!(lots.iter().map(|l| l.0).sum::<i64>(), 6 + 4 + 2 - 3);
    assert_eq!(
        lots.iter().map(|l| l.1).sum::<i64>(),
        1_000_00,
        "the basis of what was lost is not lost: it is an unrealized loss"
    );
    // Three of the first lot's 300.00 of basis are shared over 9 shares: 100.00, 133.33 and 66.67 more.
    assert_eq!(lots, [(3, 400_00), (4, 533_33), (2, 66_67)]);
    assert_eq!(run.checks[ins.index()], 3, "`on in` fires for both purchases and for growth");
}

#[test]
fn a_gap_via_a_market_place_is_a_revaluation_and_a_pad_is_a_flow_that_parcels_see() {
    let mut f = Fixture::new();
    let (salary, retirement, market, usd, vti, unknown) = (f.salary, f.retirement, f.market, f.usd, f.vti, f.unknown);
    let ticks = ticking(&mut f, retirement, "ticks");
    f.flow(1, salary, retirement, 10_000_00);
    f.assert(2, retirement, 12_000_00);
    f.via_last(market);
    f.assert_vti(3, retirement, 3);
    f.pad_last();
    let book = f.book();
    let run = run(&book, options());
    assert_eq!(lots_of(&run, retirement, usd), [(12_000_00, 0)], "growth arrives with no basis, in the pre-tax lot");
    assert_eq!(lots_of(&run, retirement, vti), [(3, 0)], "an unexplained gap of shares is a parcel, not a balance");
    assert_eq!(run.pads.iter().map(|p| p.counter).collect::<Vec<_>>(), [market, unknown]);
    assert_eq!(run.checks[ticks.index()], 3, "laws see the flows a gap posts");
    assert!(
        run.diagnostics.iter().all(|d| d.code == "pad"),
        "a `via` says nothing; a `!` says so: {:?}",
        run.diagnostics
    );
    assert!(run.gains.is_empty());
}

#[test]
fn a_waiver_waives_a_priced_violation_and_a_waiver_that_waives_nothing_warns() {
    let mut f = Fixture::new();
    let (salary, retirement, checking, irs) = (f.salary, f.retirement, f.checking, f.grant);
    let (name, penalty) = (f.sym("early-withdrawal"), f.sym("penalty"));
    let mut law = LawBuilder::new(name, Trigger::Gain);
    let gain = law.var(Var::Gain, Ty::AMOUNT);
    let nothing = law.konst(Value::Empty, Ty::Empty);
    let cond = law.bin(BinOp::Le, gain, nothing, Ty::Bool);
    let tenth = law.konst(Value::Num(Ratio::new(1, 10).unwrap()), Ty::Num);
    let again = law.var(Var::Gain, Ty::AMOUNT);
    let tax = law.bin(BinOp::Mul, tenth, again, Ty::AMOUNT);
    let law = f.law(law.require_else_owe(cond, tax, irs, penalty));
    let rule = f.rule(law, Subject::Place(retirement));
    f.on_gain.push((retirement, rule));
    f.flow(1, salary, retirement, 5_000_00);
    let waived = f.flow(2, retirement, checking, 1_000_00);
    f.waive(waived);
    let pointless = f.flow(3, salary, checking, 10_00);
    f.waive(pointless);
    let book = f.book();
    let run = run(&book, options());
    assert!(run.effects.is_empty(), "the penalty is waived, so it is not owed");
    let [violation] = run.violations[..] else { panic!("{:?}", run.violations) };
    assert_eq!(violation.verdict, Verdict::Priced { waived: true });
    let d = &run.diagnostics[violation.diagnostic as usize];
    assert_eq!((d.disposition, d.labels.iter().any(|l| l.text == "waived here")), (Disposition::Waived, true));
    let unused = diagnostic(&run, "unused-waiver");
    assert_eq!((unused.severity, unused.labels[0].loc), (Severity::Warning, book.flows[pointless].waive.unwrap().loc));
    assert_eq!(
        run.diagnostics.iter().filter(|d| d.code == "unused-waiver").count(),
        1,
        "the used waiver is not reported"
    );
}

#[test]
fn every_comparison_keeps_its_last_reading_with_the_sides_of_a_floor_swapped() {
    let mut f = Fixture::new();
    let (equity, checking, food) = (f.equity, f.checking, f.food);
    let name = f.sym("bank");
    let mut law = LawBuilder::new(name, Trigger::Always);
    let balance = law.var(Var::Balance, Ty::AMOUNT);
    let minimum = law.konst(Value::Amount(f.usd(100_00)), Ty::AMOUNT);
    let cond = law.bin(BinOp::Ge, balance, minimum, Ty::Bool);
    let law = f.law(law.warn(cond));
    let rule = f.rule(law, Subject::Place(checking));
    f.always.push((checking, rule));
    f.flow(1, equity, checking, 500_00);
    f.flow(2, checking, food, 200_00);
    let book = f.book();
    let run = run(&book, options());
    let [_, reading] = run.headroom[..] else { panic!("one reading a day: {:?}", run.headroom) };
    assert_eq!(
        (reading.counted.qty, reading.limit.qty, reading.warn, reading.bound),
        (Qty(100_00), Qty(300_00), true, Bound::Floor),
        "room above the floor: limit - counted"
    );
    assert_eq!(
        (reading.days.first(), reading.days.last(), reading.day),
        (Day(2), Day(2), Day(2)),
        "no total or tally: the day itself"
    );
}

#[test]
fn a_floor_of_nothing_is_read_off_the_holdings_and_leaves_a_reading_only_when_it_fails() {
    let mut f = Fixture::new();
    let (equity, checking, food) = (f.equity, f.checking, f.food);
    let name = f.sym("overdraft");
    let mut law = LawBuilder::new(name, Trigger::Always);
    let balance = law.var(Var::Balance, Ty::AMOUNT);
    let nothing = law.konst(Value::Empty, Ty::Empty);
    let cond = law.bin(BinOp::Ge, balance, nothing, Ty::Bool);
    let law = f.law(law.warn(cond));
    let rule = f.rule(law, Subject::Place(checking));
    f.always.push((checking, rule));
    f.flow(1, equity, checking, 500_00);
    f.flow(2, checking, food, 200_00);
    let book = f.book();
    let held = run(&book, options());
    assert!(held.headroom.is_empty() && held.violations.is_empty(), "it held both times, and said nothing");
    assert_eq!(held.checks[law.index()], 2, "and was still counted as checked");

    let mut f = Fixture::new();
    let (equity, checking, food) = (f.equity, f.checking, f.food);
    let name = f.sym("overdraft");
    let mut law = LawBuilder::new(name, Trigger::Always);
    let balance = law.var(Var::Balance, Ty::AMOUNT);
    let nothing = law.konst(Value::Empty, Ty::Empty);
    let cond = law.bin(BinOp::Ge, balance, nothing, Ty::Bool);
    let law = f.law(law.warn(cond));
    let rule = f.rule(law, Subject::Place(checking));
    f.always.push((checking, rule));
    f.flow(1, equity, checking, 500_00);
    f.flow(2, checking, food, 700_00);
    f.flow(3, equity, checking, 400_00);
    let book = f.book();
    let broke = run(&book, options());
    let [violation] = broke.violations[..] else { panic!("{:?}", broke.violations) };
    assert_eq!((violation.day, violation.verdict), (Day(2), Verdict::Warns));
    let [reading] = broke.headroom[..] else { panic!("the failing reading only: {:?}", broke.headroom) };
    assert_eq!((reading.limit.qty, reading.day, reading.bound), (Qty(-200_00), Day(2), Bound::Floor));
}

#[test]
fn a_tight_budget_is_reported_once_per_month_not_once_per_flow() {
    let mut f = Fixture::new();
    let (checking, food) = (f.checking, f.food);
    month_budget(&mut f, food, 10_00);
    for day in 0..59 {
        f.flow(day, checking, food, 1_00);
    }
    let book = f.book();
    let run = run(&book, options());
    assert_eq!(run.violations.len(), 2, "January and February, each at the flow that crossed the line");
    assert_eq!(run.diagnostics.len(), 2);
    assert_eq!(run.violations[0].day, Day(10));
    let last: Vec<_> = run.headroom.iter().map(|h| h.counted.qty.0).collect();
    assert_eq!(last, [31_00, 28_00]);
}

#[test]
fn fields_read_a_places_basis_and_an_amounts_commodity() {
    let mut f = Fixture::new();
    let (equity, salary, checking, brokerage, vti) = (f.equity, f.salary, f.checking, f.brokerage, f.vti);
    let (seen, tally, only_shares) = (f.sym("seen"), f.sym("seen"), f.sym("shares-only"));
    let mut law = LawBuilder::new(seen, Trigger::In);
    let place = law.konst(Value::Place(brokerage), Ty::Place);
    let basis = law.field(place, Field::Basis, Ty::AMOUNT);
    let law = f.law(law.count(basis, tally));
    let rule = f.rule(law, Subject::Entity(f.me));
    f.on_in.push((checking, rule));
    let mut unit = LawBuilder::new(only_shares, Trigger::In);
    let (amount, shares) = (unit.var(Var::Amount, Ty::AMOUNT), unit.konst(Value::Unit(vti), Ty::Unit));
    let unit_of = unit.field(amount, Field::Unit, Ty::Unit);
    let cond = unit.bin(BinOp::Eq, unit_of, shares, Ty::Bool);
    let unit = f.law(unit.require(cond, None));
    let rule = f.rule(unit, Subject::Place(checking));
    f.on_in.push((checking, rule));
    f.flow(1, equity, checking, 1_000_00);
    f.buy(2, 300_00, 3);
    f.flow(3, salary, checking, 50_00);
    let book = f.book();
    let run = run(&book, options());
    let seen: Vec<_> = run.effects.iter().map(|e| e.amount.qty.0).collect();
    assert_eq!(seen, [300_00], "the basis of the parcels, and nothing yet before them");
    assert_eq!(run.violations.len(), 2, "each of the two dollar flows into checking is not in shares");
}

#[test]
fn a_household_is_the_subject_of_what_its_members_own() {
    let mut f = Fixture::new();
    let (salary, checking, savings, household) = (f.salary, f.checking, f.savings, f.household);
    f.join_household();
    counting(&mut f, checking, Subject::Entity(household), "household-in");
    counting(&mut f, savings, Subject::Entity(household), "household-in");
    f.flow(1, salary, checking, 900_00);
    f.flow(2, checking, savings, 400_00);
    let book = f.book();
    let run = run(&book, options());
    let counted: Vec<_> = run.effects.iter().map(|e| (e.owner, e.amount.qty.0)).collect();
    assert_eq!(counted, [(household, 900_00)], "the transfer between two of the household's places is not income");
}

/// Builds a book with `build` and folds it.
fn asserted(build: impl FnOnce(&mut Fixture)) -> (Book<'static>, Run) {
    let mut f = Fixture::new();
    build(&mut f);
    let book = f.book();
    let run = run(&book, options());
    (book, run)
}

fn the_error(run: &Run) -> &Diagnostic {
    let errors: Vec<_> = run.diagnostics.iter().filter(|d| d.code == "assertion").collect();
    assert_eq!(errors.len(), 1, "{errors:?}");
    errors[0]
}

#[test]
fn a_failed_assertion_notices_a_flow_written_backwards() {
    let (_, run) = asserted(|f| {
        let (card, food, checking) = (f.card, f.food, f.checking);
        f.flow(1, card, food, 300_00);
        f.flow(2, card, food, 152_00);
        f.flow(3, card, checking, 200_00);
        f.assert(4, card, 252_00);
    });
    let d = the_error(&run);
    assert_eq!(d.message, "liabilities/card holds 652.00 USD, not 252.00 USD");
    assert_eq!(d.labels[0].text, "400.00 USD less than the ledger holds");
    assert!(
        d.notes
            .iter()
            .any(|n| n == "the gap is exactly twice this flow (2 × 200.00 USD): it is probably written backwards")
    );
    assert_eq!(d.help[0].text, "write it the other way: `assets/checking -> liabilities/card`");
}

#[test]
fn a_failed_assertion_notices_two_swapped_digits_a_wrong_sign_and_a_wrong_commodity() {
    let (_, swapped) = asserted(|f| {
        let (equity, checking) = (f.equity, f.checking);
        f.flow(1, equity, checking, 3_015_80);
        f.assert(2, checking, 3_051_80);
    });
    assert!(
        the_error(&swapped)
            .notes
            .iter()
            .any(|n| n == "3,015.80 USD and 3,051.80 USD differ only by two neighbouring digits swapped")
    );
    assert_eq!(the_error(&swapped).help[0].text, "if the statement says 3,015.80 USD, correct the amount");

    let (_, sign) = asserted(|f| {
        let (equity, checking, food) = (f.equity, f.checking, f.food);
        f.flow(1, equity, checking, 100_00);
        f.flow(2, checking, food, 150_00);
        f.assert(3, checking, 50_00);
    });
    assert!(
        the_error(&sign).notes.iter().any(|n| n.starts_with("the ledger holds -50.00 USD, the opposite of 50.00 USD"))
    );

    let (_, unit) = asserted(|f| {
        let (equity, savings) = (f.equity, f.savings);
        f.flow(1, equity, savings, 915_80);
        f.assert_vti(2, savings, 10);
    });
    assert_eq!(the_error(&unit).message, "assets/savings has never held VTI; it holds 915.80 USD");
    assert_eq!(the_error(&unit).help[0].text, "assert in USD: 915.80 USD");
}

#[test]
fn a_gap_is_carried_and_only_a_change_in_it_is_reported_again() {
    let (_, run) = asserted(|f| {
        let (equity, salary, checking) = (f.equity, f.salary, f.checking);
        f.flow(1, equity, checking, 1_000_00);
        f.assert(2, checking, 1_000_00);
        f.assert(10, checking, 955_00);
        f.assert(20, checking, 955_00);
        f.flow(25, salary, checking, 100_00);
        f.assert(30, checking, 1_055_00);
        f.assert(40, checking, 1_045_00);
    });
    let errors: Vec<_> = run.diagnostics.iter().filter(|d| d.code == "assertion").collect();
    assert_eq!(errors.len(), 2, "the same gap at days 20 and 30 says nothing: {errors:?}");
    assert_eq!(errors[0].labels[0].text, "45.00 USD less than the ledger holds");
    assert_eq!(errors[1].labels[0].text, "another 10.00 USD less than the ledger holds");
    assert_eq!(errors[1].labels.len(), 1, "no flow since the assertion on day 30");
    assert!(errors[1].notes[0].starts_with("the 45.00 USD gap reported at an earlier assertion is carried"));
}

#[test]
fn an_assertion_that_depends_on_an_unsolved_amount_is_not_checked_once_and_never_a_false_gap() {
    let (_, run) = asserted(|f| {
        let (equity, checking, cash, food) = (f.equity, f.checking, f.cash, f.food);
        f.flow(1, equity, checking, 1_000_00);
        f.assert(2, checking, 1_000_00);
        f.unknown(3, checking, cash);
        f.unknown(4, checking, food);
        f.assert(5, checking, 800_00);
        f.assert(6, checking, 800_00);
    });
    let codes: Vec<_> = run.diagnostics.iter().map(|d| &*d.code).collect();
    assert_eq!(codes, ["cannot-infer", "cannot-infer", "unchecked"], "no assertion error, and the note is said once");
    assert!(run.diagnostics[2].help.is_empty(), "it never suggests `!`");
}

/// A million flows through `run`: `cargo test -p axiom-engine --release -- --ignored --nocapture million`.
#[test]
#[ignore = "a benchmark"]
fn a_million_flows() {
    let flows: i64 = std::env::var("BENCH_FLOWS").ok().and_then(|n| n.parse().ok()).unwrap_or(1_000_000);
    for laws in [false, true] {
        let mut f = Fixture::new();
        let (equity, salary, checking, savings, food, retirement, brokerage) =
            (f.equity, f.salary, f.checking, f.savings, f.food, f.retirement, f.brokerage);
        if laws {
            let (bank, budget, wages) = (f.sym("bank"), f.sym("budget"), f.sym("wages"));
            let mut never_overdrawn = LawBuilder::new(bank, Trigger::Always);
            let balance = never_overdrawn.var(Var::Balance, Ty::AMOUNT);
            let nothing = never_overdrawn.konst(Value::Empty, Ty::Empty);
            let cond = never_overdrawn.bin(BinOp::Ge, balance, nothing, Ty::Bool);
            let law = f.law(never_overdrawn.warn(cond));
            let rule = f.rule(law, Subject::Place(checking));
            f.always.push((checking, rule));

            let cap = f.usd(1_000_000_00);
            let mut monthly = LawBuilder::new(budget, Trigger::In);
            let total = monthly.call(Func::Total(Dir::In, Window::Month), &[], Ty::AMOUNT);
            let cap = monthly.konst(Value::Amount(cap), Ty::AMOUNT);
            let cond = monthly.bin(BinOp::Le, total, cap, Ty::Bool);
            let law = f.law(monthly.warn(cond));
            let rule = f.rule(law, Subject::Place(food));
            f.on_in.push((food, rule));

            let mut count = LawBuilder::new(wages, Trigger::In);
            let amount = count.var(Var::Amount, Ty::AMOUNT);
            let law = f.law(count.count(amount, wages));
            let rule = f.rule(law, Subject::Entity(f.me));
            f.on_in.push((checking, rule));
        }
        f.flow(1, equity, checking, 1_000_000_00);
        for i in 0..flows {
            let day = 2 + (i / 300) as i32;
            match i % 10 {
                0 => f.flow(day, salary, checking, 3_000_00),
                1 => f.flow(day, checking, retirement, 500_00),
                2 => f.flow(day, checking, savings, 100_00),
                3 => f.flow(day, savings, checking, 100_00),
                4 if i % 100 == 4 => f.buy(day, 1_000_00, 10),
                5 if i % 100 == 5 => {
                    let sale = f.sell(day, 10, 1_200_00);
                    f.select(sale, [Select::Policy(Policy::Fifo)]);
                    sale
                }
                _ => f.flow(day, checking, food, 20_00 + i % 7),
            };
        }
        let _ = brokerage;
        let book = f.book();
        let options = Options { today: Day(5_000), relaxed: false };
        let started = std::time::Instant::now();
        let plan = Plan::new(&book);
        let mut ledger = plan.start(options);
        let solved = started.elapsed();
        ledger.advance(options.today);
        let folded = started.elapsed();
        let cloned = std::time::Instant::now();
        drop(ledger.clone());
        let clone_ms = cloned.elapsed().as_secs_f64() * 1e3;
        let forking = std::time::Instant::now();
        drop(ledger.fork());
        let fork_ms = forking.elapsed().as_secs_f64() * 1e3;
        let run = ledger.finish();
        let took = started.elapsed();
        println!(
            "{} laws: {:.0} ms ({:.0} solve, {:.0} fold, {:.0} finish), {:.2} million flows/s ({} gains, {} effects, {} diagnostics)",
            if laws { "3" } else { "no" },
            took.as_secs_f64() * 1e3,
            solved.as_secs_f64() * 1e3,
            (folded - solved).as_secs_f64() * 1e3,
            (took - folded).as_secs_f64() * 1e3,
            (flows as f64 + 1.0) / took.as_secs_f64() / 1e6,
            run.gains.len(),
            run.effects.len(),
            run.diagnostics.len(),
        );
        println!("   after the fold: clone {clone_ms:.2} ms, fork {fork_ms:.2} ms");
    }
}
