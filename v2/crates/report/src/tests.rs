//! Views over a small household built by hand.
//!
//! The book and the run are assembled directly (the model and the engine are
//! other crates), so each view can be held against figures worked out on
//! paper. Only base-currency money is used: pricing is the model's business.

use axiom_core::{Arena, Day, FileId, Groups, Id, Interner, Loc, Qty, Ratio, Span, Sym, Tree};
use axiom_engine::{Cause, Effect, Gain, Holding, Owed, Parcel, Posted, Run, State};
use axiom_model::*;

use crate::{Cell, Query, Report, Row, Section, Style, report};

fn day(y: i32, m: u32, d: u32) -> Day {
    Day::from_ymd(y, m, d).unwrap()
}

/// Line `n` of the imaginary journal: bytes `100n .. 100n + 99`.
fn line(n: u32) -> Loc {
    Loc::new(FileId(0), n * 100, n * 100 + 99)
}

pub(crate) struct Household {
    pub(crate) book: Book<'static>,
    pub(crate) run: Run,
}

impl Household {
    fn place(&self, path: &str) -> Id<Place> {
        let named = |(_, place): &(Id<Place>, &Place)| self.book.name(place.path) == path;
        self.book.places.iter().find(named).map(|(id, _)| id).expect("a place in the fixture")
    }

    fn report(&self, query: Query) -> Report<'static> {
        report(&self.book, &self.run, &query).expect("the query resolves")
    }
}

// ─── The cast ───────────────────────────────────────────────────────────────

const PLACES: [(&str, Class); 17] = [
    ("assets", Class::Asset),
    ("assets/bank", Class::Asset),
    ("assets/bank/checking", Class::Asset),
    ("assets/retirement", Class::Asset),
    ("equity", Class::Equity),
    ("equity/unknown", Class::Equity),
    ("expenses", Class::Expense),
    ("expenses/food", Class::Expense),
    ("expenses/food/groceries", Class::Expense),
    ("expenses/insurance", Class::Expense),
    ("expenses/rent", Class::Expense),
    ("expenses/repairs", Class::Expense),
    ("income", Class::Income),
    ("income/gains", Class::Income),
    ("income/salary", Class::Income),
    ("liabilities", Class::Liability),
    ("liabilities/visa", Class::Liability),
];

/// Everything the book names: one kind, four entities, the places above, one
/// commodity, and two nested jurisdictions.
struct Cast {
    names: Interner<'static>,
    kinds: Tree<Kind>,
    me: Id<Entity>,
    landlord: Id<Entity>,
    irs: Id<Entity>,
    nsf: Id<Entity>,
    entities: Tree<Entity>,
    places: Tree<Place>,
    place_ids: Vec<Id<Place>>,
    usd: Id<Commodity>,
    commodities: Arena<Commodity>,
    us: Id<System>,
    california: Id<System>,
    systems: Tree<System>,
}

impl Cast {
    fn new() -> Cast {
        let mut names = Interner::default();
        let (kinds, kind_ids) = Tree::build(vec![kind(names.intern("thing"))], &[None]).unwrap();
        let thing = kind_ids[0];

        let entities = ["me", "landlord", "irs", "nsf"].map(|name| Entity {
            path: names.intern(name),
            kind: thing,
            via: None,
            restricted: false,
            lives: Box::default(),
            props: Box::default(),
            doc: None,
            loc: None,
        });
        let (entities, ids) = Tree::build(entities.into(), &[None; 4]).unwrap();
        let [me, landlord, irs, nsf] = [ids[0], ids[1], ids[2], ids[3]];

        let parents: Vec<Option<usize>> = PLACES
            .iter()
            .map(|(path, _)| {
                path.rsplit_once('/').and_then(|(parent, _)| PLACES.iter().position(|(other, _)| *other == parent))
            })
            .collect();
        let items = PLACES.iter().map(|&(path, class)| Place {
            path: names.intern(path),
            class,
            kind: thing,
            owner: me,
            holds: None,
            select: None,
            deferred: false,
            liquidity: (path == "assets/retirement").then_some(Span::months(1)),
            opened: None,
            closed: None,
            props: Box::default(),
            doc: None,
            loc: None,
        });
        let (places, place_ids) = Tree::build(items.collect(), &parents).unwrap();

        let mut commodities = Arena::new();
        let usd = commodities.push(Commodity {
            symbol: names.intern("USD"),
            kind: thing,
            scale: 2,
            title: None,
            liquidity: None,
            growth: None,
            props: Box::default(),
            doc: None,
            loc: None,
        });

        let jurisdictions =
            ["us", "us/ca"].map(|path| System { path: names.intern(path), laws: Box::default(), doc: None, loc: None });
        let (systems, system_ids) = Tree::build(jurisdictions.into(), &[None, Some(0)]).unwrap();

        Cast {
            names,
            kinds,
            me,
            landlord,
            irs,
            nsf,
            entities,
            places,
            place_ids,
            usd,
            commodities,
            us: system_ids[0],
            california: system_ids[1],
            systems,
        }
    }

    fn id(&self, path: &str) -> Id<Place> {
        self.place_ids[PLACES.iter().position(|(other, _)| *other == path).unwrap()]
    }
}

fn kind(name: Sym) -> Kind {
    Kind {
        name,
        sort: Sort::Entity,
        system: None,
        restricted: false,
        deferred: false,
        select: None,
        liquidity: None,
        has: Box::default(),
        props: Box::default(),
        laws: Box::default(),
        doc: None,
        loc: None,
    }
}

// ─── What happened ──────────────────────────────────────────────────────────

/// Three months of a household: salary, rent, groceries, a visa card, a year's
/// insurance spread over 2026, a pending repair check, 40 that vanished, and
/// 1,000 put away for retirement.
struct Journal {
    txns: Arena<Txn>,
    flows: Arena<Flow>,
    posted: Vec<Posted>,
}

fn journal(cast: &mut Cast) -> Journal {
    let landlord = Some(cast.landlord);
    // (line, day, from, to, cents, payee, state)
    let rows = [
        (1, day(2026, 1, 1), "assets/bank/checking", "expenses/insurance", 120_000, None, State::Actual),
        (2, day(2026, 1, 15), "income/salary", "assets/bank/checking", 500_000, None, State::Actual),
        (3, day(2026, 1, 16), "assets/bank/checking", "expenses/rent", 180_000, landlord, State::Actual),
        (4, day(2026, 1, 18), "assets/bank/checking", "expenses/food/groceries", 8_420, None, State::Actual),
        (5, day(2026, 2, 15), "income/salary", "assets/bank/checking", 500_000, None, State::Actual),
        (6, day(2026, 2, 16), "assets/bank/checking", "expenses/rent", 180_000, landlord, State::Actual),
        (7, day(2026, 2, 20), "liabilities/visa", "expenses/food/groceries", 12_000, None, State::Actual),
        (8, day(2026, 3, 1), "assets/bank/checking", "expenses/repairs", 35_000, None, State::Pending),
        (9, day(2026, 3, 15), "income/salary", "assets/bank/checking", 500_000, None, State::Actual),
        (10, day(2026, 3, 20), "assets/bank/checking", "equity/unknown", 4_000, None, State::Actual),
        (11, day(2026, 3, 25), "assets/bank/checking", "liabilities/visa", 12_000, None, State::Actual),
        (12, day(2026, 3, 28), "assets/bank/checking", "assets/retirement", 100_000, None, State::Actual),
    ];
    let check = cast.names.intern("#check-1041");
    let mut journal = Journal { txns: Arena::new(), flows: Arena::new(), posted: Vec::new() };
    for (index, &(row, when, from, to, cents, payee, state)) in rows.iter().enumerate() {
        let txn = journal.txns.push(Txn {
            day: when,
            first: Id::new(index as u32),
            len: 1,
            payee,
            codes: Box::default(),
            waive: None,
            doc: None,
            loc: line(row),
        });
        let amount = Amount::new(Qty(cents), cast.usd);
        journal.flows.push(Flow {
            day: when,
            // The insurance is paid for the whole year.
            until: if row == 1 { day(2026, 12, 31) } else { when },
            from: cast.id(from),
            to: cast.id(to),
            out: amount,
            arrive: amount,
            mode: if state == State::Pending { Mode::Pending } else { Mode::Actual },
            infer: Infer::Known,
            txn,
            payee,
            select: Box::default(),
            codes: if state == State::Pending { Box::new([check]) } else { Box::default() },
            loc: line(row),
            waive: None,
        });
        journal.posted.push(Posted { out: Qty(cents), arrive: Qty(cents), state });
    }
    journal
}

/// `every month on 1 from 2026-04-01 until 2026-06-30 checking -> landlord 1_800 USD`
fn rent_plan(cast: &Cast) -> Plan {
    let amount = Amount::new(Qty(180_000), cast.usd);
    let once = Flow {
        day: day(2026, 4, 1),
        until: day(2026, 4, 1),
        from: cast.id("assets/bank/checking"),
        to: cast.id("expenses/rent"),
        out: amount,
        arrive: amount,
        mode: Mode::Planned,
        infer: Infer::Known,
        txn: Id::new(2),
        payee: Some(cast.landlord),
        select: Box::default(),
        codes: Box::default(),
        loc: line(60),
        waive: None,
    };
    Plan {
        every: Span::months(1),
        on: Some(On::MonthDay(1)),
        from: Some(day(2026, 4, 1)),
        until: Some(day(2026, 6, 30)),
        template: Box::new([once]),
        loc: line(60),
    }
}

/// `budget 500 USD monthly` under expenses/food: `warn total(in, month) <= 500 USD`.
fn budget_law(cast: &mut Cast) -> Law {
    let node = |op, ty, first| Node { op, ty, loc: line(80), first: NodeId(first) };
    let limit = Amount::new(Qty(50_000), cast.usd);
    let nodes = vec![
        node(Op::Call(Func::Total(Dir::In, Window::Month), Box::default()), Ty::Amount, 0),
        node(Op::Const(Value::Amount(limit)), Ty::Amount, 1),
        node(Op::Bin(BinOp::Le, NodeId(0), NodeId(1)), Ty::Bool, 0),
    ];
    Law {
        name: cast.names.intern("budget"),
        doc: Some(cast.names.intern("/// Groceries and dining stay under 500 a month.")),
        owner: Owner::Place(cast.id("expenses/food")),
        system: None,
        trigger: Trigger::In,
        steps: Box::new([Step {
            loc: line(80),
            kind: StepKind::Require { cond: NodeId(2), otherwise: None, message: None, warn: true },
        }]),
        nodes: nodes.into(),
        loc: line(80),
    }
}

/// What the laws and the engine recorded.
struct Records {
    laws: Arena<Law>,
    effects: Vec<Effect>,
    holdings: Vec<Holding>,
    gains: Vec<Gain>,
}

fn records(cast: &mut Cast) -> Records {
    let mut laws = Arena::new();
    let budget = laws.push(budget_law(cast));
    let wages = laws.push(Law {
        name: cast.names.intern("wages"),
        doc: None,
        owner: Owner::System(cast.us),
        system: Some(cast.us),
        trigger: Trigger::In,
        steps: Box::default(),
        nodes: Box::default(),
        loc: line(90),
    });
    assert_eq!(budget, Id::new(0), "the rules below refer to the budget by its position");

    // Names are one namespace per person-year: both systems add to `agi`.
    let (me, irs, usd) = (cast.me, cast.irs, cast.usd);
    let owed = |to| Some(Owed { to, due: day(2027, 4, 15) });
    let mut effect = |system, when, name: &'static str, cents, owe, cause| Effect {
        law: wages,
        subject: Subject::Entity(me),
        owner: me,
        system: Some(system),
        day: when,
        name: cast.names.intern(name),
        amount: Amount::new(Qty(cents), usd),
        owe,
        cause,
    };
    let (us, ca) = (cast.us, cast.california);
    let effects = vec![
        effect(us, day(2026, 1, 15), "wages", 500_000, None, Cause::Flow(Id::new(1))),
        effect(us, day(2026, 2, 15), "wages", 500_000, None, Cause::Flow(Id::new(4))),
        effect(us, day(2026, 3, 31), "agi", 1_000_000, None, Cause::Time),
        effect(ca, day(2026, 3, 31), "agi", 20_000, None, Cause::Time),
        effect(us, day(2026, 3, 31), "federal-tax", 90_000, owed(irs), Cause::Time),
        effect(ca, day(2026, 3, 31), "ca-income-tax", 15_000, owed(irs), Cause::Time),
    ];

    let parcel = |cents: i64, basis: i64, tied| Parcel {
        qty: Qty(cents),
        basis: Qty(basis),
        acquired: day(2025, 6, 1),
        txn: Id::new(1),
        tied,
    };
    let holdings = vec![
        Holding {
            place: cast.id("assets/bank/checking"),
            unit: usd,
            plain: Qty(1_374_000),
            lots: vec![parcel(50_000, 50_000, Some(cast.nsf))],
        },
        Holding {
            place: cast.id("assets/retirement"),
            unit: usd,
            plain: Qty(0),
            lots: vec![parcel(1_000_000, 0, None)],
        },
    ];
    let gains = vec![Gain {
        cause: Cause::Time,
        day: day(2026, 2, 10),
        from: cast.id("assets/retirement"),
        to: cast.id("assets/bank/checking"),
        unit: usd,
        qty: Qty(100),
        basis: Qty(20_000),
        proceeds: Qty(50_000),
        acquired: day(2025, 6, 1),
        ambiguous: false,
    }];
    Records { laws, effects, holdings, gains }
}

pub(crate) fn household() -> Household {
    let mut cast = Cast::new();
    let journal = journal(&mut cast);
    let records = records(&mut cast);

    let food = cast.id("expenses/food");
    let budget_rule =
        Rule { law: Id::new(0), subject: Subject::Place(food), from: Day(i32::MIN), until: Day(i32::MAX) };
    let rules = Rules { on_in: Groups::build(cast.places.len(), [(food, budget_rule)]), ..Rules::default() };
    let touching =
        Groups::build(cast.places.len(), journal.flows.iter().flat_map(|(id, flow)| [(flow.from, id), (flow.to, id)]));
    let roots = Roots {
        me: cast.me,
        unknown: cast.id("equity/unknown"),
        asset: Id::new(0),
        liability: Id::new(0),
        income: Id::new(0),
        expense: Id::new(0),
        equity: Id::new(0),
        commodity: Id::new(0),
        entity: Id::new(0),
    };
    let plans = vec![rent_plan(&cast)];
    let book = Book {
        names: cast.names,
        base: cast.usd,
        relaxed: false,
        roots,
        places: cast.places,
        entities: cast.entities,
        kinds: cast.kinds,
        systems: cast.systems,
        commodities: cast.commodities,
        laws: records.laws,
        rules,
        params: Arena::new(),
        schedules: Arena::new(),
        codes: Vec::new(),
        txns: journal.txns,
        flows: journal.flows,
        touching,
        asserts: Vec::new(),
        events: Vec::new(),
        prices: Prices::default(),
        lookup: Default::default(),
        plans,
        syncs: Vec::new(),
    };
    let run = Run {
        today: day(2026, 3, 31),
        posted: journal.posted.into(),
        holdings: records.holdings,
        gains: records.gains,
        effects: records.effects,
        violations: Vec::new(),
        pads: Vec::new(),
        checks: vec![0; 2].into(),
        diagnostics: Vec::new(),
    };
    Household { book, run }
}

// ─── Rendering, so figures can be compared as text ──────────────────────────

fn cell(cell: &Cell) -> String {
    match cell {
        Cell::Blank => String::new(),
        Cell::Text(text) => text.to_string(),
        Cell::Amount { qty, scale, unit } => format!("{} {unit}", qty.show(*scale)),
        Cell::Day(day) => day.to_string(),
        Cell::Percent(ratio) => format!("{}%", Ratio::new(ratio.num() as i128 * 100, ratio.den() as i128).unwrap()),
        Cell::Source(loc) => format!("@{}", loc.start / 100),
    }
}

fn lines(section: &Section) -> Vec<String> {
    let mark = |row: &Row| match row.style {
        Style::Normal => "",
        Style::Total => "=",
        Style::Muted => "~",
        Style::Alert => "!",
    };
    section
        .rows
        .iter()
        .map(|row| {
            let cells: Vec<String> = row.cells.iter().map(cell).collect();
            format!("{}{}{}", mark(row), "  ".repeat(row.depth as usize), cells.join(" | ")).trim_end().to_string()
        })
        .collect()
}

fn show(report: &Report) -> String {
    let mut out = format!("# {}\n", report.title);
    for section in &report.sections {
        out += &format!("##{}\n", section.heading.as_ref().map_or(String::new(), |heading| format!(" {heading}")));
        out += &lines(section).join("\n");
        out += &section.notes.iter().map(|note| format!("\n  note: {note}")).collect::<String>();
        out += "\n";
    }
    out
}

// ─── The views ──────────────────────────────────────────────────────────────

fn table(house: &Household, query: Query) -> String {
    show(&house.report(query))
}

#[test]
fn balance_is_a_tree_with_subtotals_and_net_worth() {
    let house = household();
    // Checking: three salaries in, less the year's insurance, two rents,
    // groceries, 40 that vanished, the card payment and 1,000 put away. The
    // 350 check is only pending. The card nets to nothing and is left out.
    let expected = "\
# Balances at 2026-03-31
##
=assets | 9,955.80 USD
  bank | 8,955.80 USD
    checking | 8,955.80 USD
  retirement | 1,000.00 USD
=equity | -40.00 USD
  unknown | -40.00 USD
=expenses | 5,004.20 USD
  food | 204.20 USD
    groceries | 204.20 USD
  insurance | 1,200.00 USD
  rent | 3,600.00 USD
=income | 15,000.00 USD
  salary | 15,000.00 USD
##
Assets | 9,955.80 USD
Liabilities | 0.00 USD
=Net worth | 9,955.80 USD
";
    assert_eq!(table(&house, Query::Balance { globs: vec![], at: None, value: false, monthly: false }), expected);
}

#[test]
fn a_past_date_and_monthly_columns_read_the_same_flows() {
    let house = household();
    let january = table(
        &house,
        Query::Balance { globs: vec!["checking"], at: Some(day(2026, 1, 20)), value: false, monthly: false },
    );
    assert!(january.contains("checking | 1,915.80 USD"));
    let monthly = house.report(Query::Balance { globs: vec!["checking"], at: None, value: true, monthly: true });
    let titles: Vec<_> = monthly.sections[0].columns.iter().map(|column| column.title.to_string()).collect();
    assert_eq!(titles, ["Place", "2026-01-31", "2026-02-28", "2026-03-31"]);
    assert_eq!(lines(&monthly.sections[0])[2], "    checking | 1,915.80 USD | 5,115.80 USD | 8,955.80 USD");
}

#[test]
fn a_filtered_balance_keeps_context_but_no_net_worth() {
    let house = household();
    let report = house.report(Query::Balance { globs: vec!["checking"], at: None, value: false, monthly: false });
    assert_eq!(lines(&report.sections[0]), ["~assets |", "~  bank |", "    checking | 8,955.80 USD"]);
    assert_eq!(report.sections.len(), 1);
}

#[test]
fn flow_recognizes_a_spread_premium_a_little_each_day() {
    let house = household();
    // 1,200 over 365 days: January's 31 days are 101.92, February's 28 are 92.05,
    // and half-even rounding at each month boundary keeps the year exact.
    let expected = "\
# Income and spending
##
income | 5,000.00 USD | 5,000.00 USD | 5,000.00 USD | 15,000.00 USD
  salary | 5,000.00 USD | 5,000.00 USD | 5,000.00 USD | 15,000.00 USD
~  realized gains ≈ |  | 300.00 USD |  | 300.00 USD
=Total income | 5,000.00 USD | 5,300.00 USD | 5,000.00 USD | 15,300.00 USD
expenses | 1,986.12 USD | 2,012.05 USD | 101.92 USD | 4,100.09 USD
  food | 84.20 USD | 120.00 USD |  | 204.20 USD
    groceries | 84.20 USD | 120.00 USD |  | 204.20 USD
  insurance | 101.92 USD | 92.05 USD | 101.92 USD | 295.89 USD
  rent | 1,800.00 USD | 1,800.00 USD |  | 3,600.00 USD
  unexplained (?) |  |  | 40.00 USD | 40.00 USD
=Total spending | 1,986.12 USD | 2,012.05 USD | 141.92 USD | 4,140.09 USD
=Net | 3,013.88 USD | 3,287.95 USD | 4,858.08 USD | 11,159.91 USD
  note: ≈ Realized gains are derived from the basis of the parcels sold; the journal does not state them.
  note: Flows written over a date range are recognized a little each day across the periods they cover.
";
    assert_eq!(table(&house, Query::Flow { by: Period::Month, from: None, to: None }), expected);
}

#[test]
fn budget_compares_the_month_to_its_limit() {
    let house = household();
    // February: 120 on the card into groceries, against 500.
    let expected = "\
# Budgets for 2026-02-10
##
expenses/food | budget | 2026-02 | 120.00 USD | 500.00 USD | 380.00 USD | 24%
";
    assert_eq!(table(&house, Query::Budget { month: Some(day(2026, 2, 10)) }), expected);
    // Groceries in March: nothing yet.
    assert!(table(&house, Query::Budget { month: None }).contains("0.00 USD | 500.00 USD | 500.00 USD | 0%"));
}

#[test]
fn tax_groups_tallies_by_system_and_lists_obligations_with_their_sources() {
    let house = household();
    let expected = "\
# Taxes 2026 for me
## Counted
=us |  |
  wages | 10,000.00 USD | 2 sources
  agi | 10,200.00 USD | 2 sources
## Owed
=us |  |  |  |
  federal-tax | irs | 2027-04-15 | 900.00 USD | period end
=  us/ca |  |  |  |
    ca-income-tax | irs | 2027-04-15 | 150.00 USD | period end
=Total owed |  |  | 1,050.00 USD |
  note: Trace any line with `axiom why NAME`, or `axiom why FILE:LINE` from its source.
";
    assert_eq!(table(&house, Query::Tax { year: None, entity: None }), expected);
}

#[test]
fn lots_list_parcels_with_basis_gain_and_term() {
    let house = household();
    let expected = "\
# Lots at 2026-03-31
##
assets/bank/checking | 500.00 USD | 500.00 USD | 2025-06-01 | 9m30d | 500.00 USD | 0.00 USD | short | tied to nsf
assets/retirement | 10,000.00 USD | 0.00 USD | 2025-06-01 | 9m30d | 10,000.00 USD | 10,000.00 USD | short |
=Total |  | 500.00 USD |  |  | 10,500.00 USD | 10,000.00 USD |  |
";
    assert_eq!(table(&house, Query::Lots { place: None }), expected);
}

#[test]
fn register_runs_a_balance_and_mutes_the_pending_check() {
    let house = household();
    let checking = house.place("assets/bank/checking");
    let section = crate::register::section(&house.book, &house.run, checking, None, None);
    let rows = lines(&section);
    assert_eq!(rows.len(), 11);
    assert_eq!(rows[6], "~2026-03-01 | expenses/repairs |  | #check-1041 · pending | -350.00 USD | 5,115.80 USD");
    // A window opens with the balance carried in.
    let march = crate::register::section(&house.book, &house.run, checking, Some(day(2026, 3, 1)), None);
    assert_eq!(lines(&march)[0], "=2026-03-01 | opening balance |  |  |  | 5,115.80 USD");
}

#[test]
fn a_code_finds_its_flows_and_a_line_explains_itself() {
    let house = household();
    let code = table(&house, Query::Why { target: "#check-1041" });
    assert!(code.contains("2026-03-01 | assets/bank/checking → expenses/repairs | 350.00 USD | pending | @8"));
    let line = table(&house, Query::Line { loc: line(3) });
    assert!(line.contains("flow: assets/bank/checking → expenses/rent, 1,800.00 USD | @3"));
    let stray = report(&house.book, &house.run, &Query::Why { target: "#check-1014" }).err().expect("no such code");
    assert_eq!(stray.help[0].text, "did you mean `check-1041`?");
}

#[test]
fn the_summary_counts_what_the_book_holds() {
    let house = household();
    let summary = crate::summary(&house.book, &house.run);
    // Seventeen places, five of them class roots.
    assert_eq!((summary.flows, summary.places, summary.unpriced), (12, 12, 0));
    assert_eq!(house.book.show(summary.net_worth).to_string(), "9,955.80 USD");
}

// ─── Views that run the ledger ──────────────────────────────────────────────
//
// These wait for the engine's `Ledger`; run them with `cargo test -- --ignored`.
// The household has no laws that would change plain balances, so whatever
// engine folds it must agree with the arithmetic here.

#[test]
#[ignore = "needs the engine's Ledger"]
fn the_forecast_folds_plans_and_habits_and_reports_an_overdraft() {
    let mut house = household();
    house.run.today = day(2026, 4, 15);
    let outlook = |house: &Household| {
        let report = house.report(Query::Forecast { until: Some(day(2026, 8, 31)), paths: 0 });
        report.sections.iter().map(lines).collect::<Vec<_>>()
    };

    // Rent (a plan, until June) leaves on May 1 and June 1; paychecks (a rhythm
    // in the journal) arrive on the 15th, so the plan's April 1st is behind us.
    let sections = outlook(&house);
    let committed: Vec<&str> = sections[0].iter().map(|row| row.split(" | ").nth(1).unwrap()).collect();
    assert_eq!(
        committed,
        ["8,955.80 USD", "8,955.80 USD", "12,155.80 USD", "15,355.80 USD", "20,355.80 USD", "25,355.80 USD"]
    );
    assert!(
        sections[1][0]
            .starts_with("assets/bank/checking → expenses/rent (landlord) | monthly | 1,800.00 USD | 2026-05-01")
    );
    assert!(
        sections[1][1]
            .starts_with("income/salary → assets/bank/checking | monthly | 5,000.00 USD | 2026-05-15 | seen 3 times")
    );

    // A 20,000 purchase planned for the 2nd of each month overdraws checking.
    let mut big = rent_plan_clone(&house.book.plans[0]);
    big.template[0].to = house.place("expenses/repairs");
    big.template[0].out.qty = Qty(2_000_000);
    big.template[0].arrive.qty = Qty(2_000_000);
    big.on = Some(On::MonthDay(2));
    house.book.plans.push(big);
    let problems = outlook(&house).pop().unwrap();
    assert_eq!(problems, ["!2026-05-02 | assets/bank/checking is overdrawn, down to -29,644.20 USD"]);
}

#[test]
#[ignore = "needs the engine's Ledger"]
fn available_subtracts_what_is_pending_and_lists_what_is_slower() {
    let house = household();
    let report = house.report(Query::Available { at: None });
    let spendable = lines(&report.sections[0]);
    assert_eq!(spendable.last().unwrap(), "=Available to spend | 8,605.80 USD");
    // The retirement account has a month's liquidity and no laws to penalize it.
    assert_eq!(lines(&report.sections[1])[0], "assets/retirement | 1m | 1,000.00 USD |  | 1,000.00 USD");
}

fn rent_plan_clone(plan: &Plan) -> Plan {
    Plan {
        every: plan.every,
        on: plan.on,
        from: plan.from,
        until: plan.until,
        template: plan.template.clone(),
        loc: plan.loc,
    }
}
