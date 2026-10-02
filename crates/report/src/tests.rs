//! Views over a small household built by hand.
//!
//! The book and the run are assembled directly (the model and the engine are
//! other crates), so each view can be held against figures worked out on
//! paper. Two people, `me` and `jordan`, belong to one household. Only
//! base-currency money is used except for two sales of VTI, which the gains
//! view needs: pricing is the model's business. Outside parties are not
//! balance-sheet places; income and spending are represented by purposes in
//! source-backed fixtures.

use std::collections::BTreeMap;

use axiom_core::{Arena, Day, Days, Facts, FileId, Groups, Id, Interner, Loc, Qty, Ratio, Severity, Span, Sym, Tree};
use axiom_engine::{Bound, Cause, Effect, Gain, Headroom, Holding, Owed, Parcel, Posted, Run, State};
use axiom_model::Effect as Consequence;
use axiom_model::builtin;
use axiom_model::*;

use crate::lens::Whose;
use crate::why::Found;
use crate::{Cell, FlowBy, Query, Report, Row, Section, Style};

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
    pub(crate) fn place(&self, path: &str) -> Id<Place> {
        let named = |(_, place): &(Id<Place>, &Place)| self.book.name(place.path) == path;
        self.book.places.iter().find(named).map(|(id, _)| id).expect("a place in the fixture")
    }

    /// The entity called `name`: the book's name lookup is the model's, so it is found by hand.
    fn entity(&self, name: &str) -> Id<Entity> {
        self.book.entities.iter().find(|(_, entity)| self.book.name(entity.path) == name).map(|(id, _)| id).unwrap()
    }

    fn report_for<'a>(&'a self, query: Query, whose: Option<&str>) -> Result<Report<'a>, axiom_core::Diagnostic> {
        let whose = whose.map_or_else(Whose::default, |name| Whose::of(&self.book, self.entity(name)));
        let plan = axiom_engine::Plan::new(&self.book);
        crate::views(crate::lens::Lens::new(&plan, &whose, self.run.today), &self.run, &query)
    }

    fn why<'a>(&'a self, found: Found) -> Report<'a> {
        let whose = Whose::default();
        let plan = axiom_engine::Plan::new(&self.book);
        crate::why::explain_with_lens(crate::lens::Lens::new(&plan, &whose, self.run.today), &self.run, found)
    }

    fn report<'a>(&'a self, query: Query) -> Report<'a> {
        self.report_for(query, None).expect("the query resolves")
    }
}

// ─── The cast ───────────────────────────────────────────────────────────────

const PLACES: [(&str, Class); 25] = [
    ("assets", Class::Asset),
    ("assets/bank", Class::Asset),
    ("assets/bank/checking", Class::Asset),
    ("assets/bank/jordan-checking", Class::Asset),
    ("assets/owed", Class::Asset),
    ("assets/owed/clients", Class::Asset),
    ("assets/retirement", Class::Asset),
    ("equity", Class::Outside),
    ("equity/unknown", Class::Outside),
    ("expenses", Class::Outside),
    ("expenses/food", Class::Outside),
    ("expenses/food/groceries", Class::Outside),
    ("expenses/insurance", Class::Outside),
    ("expenses/rent", Class::Outside),
    ("expenses/repairs", Class::Outside),
    ("income", Class::Outside),
    ("income/design", Class::Outside),
    ("income/gains", Class::Outside),
    ("income/jordan-pay", Class::Outside),
    ("income/salary", Class::Outside),
    ("liabilities", Class::Debt),
    ("liabilities/bills", Class::Debt),
    ("liabilities/visa", Class::Debt),
    ("assets/vault", Class::Asset),
    ("assets/vault/coins", Class::Asset),
];

/// Places somebody other than `me` owns.
const JORDAN_OWNS: [&str; 2] = ["assets/bank/jordan-checking", "income/jordan-pay"];

/// Everything the book names: one kind, six entities, the places above, two
/// commodities, and two nested jurisdictions.
struct Cast {
    names: Interner<'static>,
    kinds: Tree<Kind>,
    me: Id<Entity>,
    landlord: Id<Entity>,
    acme: Id<Entity>,
    irs: Id<Entity>,
    nsf: Id<Entity>,
    market: Id<Entity>,
    unknown: Id<Entity>,
    opening: Id<Entity>,
    entities: Tree<Entity>,
    places: Tree<Place>,
    place_ids: Vec<Id<Place>>,
    usd: Id<Commodity>,
    vti: Id<Commodity>,
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

        // Roots take their ids in the order given: `me` and `jordan` belong to the third.
        // No place of the household is the market's.
        let people = ["me", "jordan", "household", "landlord", "acme", "irs", "nsf", "market", "unknown", "opening"];
        let entities = people.map(|name| Entity {
            path: names.intern(name),
            kind: thing,
            place: None,
            owner: None,
            client_of: None,
            owned_by: Box::default(),
            known_as: Box::default(),
            doc: None,
            loc: None,
        });
        let (entities, ids) = Tree::build(entities.into(), &[None; 10]).unwrap();
        let [me, jordan, landlord, acme, irs, nsf, market] = [ids[0], ids[1], ids[3], ids[4], ids[5], ids[6], ids[7]];
        assert_eq!(ids[2], Id::new(2), "the household is the third root");

        let parents: Vec<Option<usize>> = PLACES
            .iter()
            .map(|(path, _)| {
                path.rsplit_once('/').and_then(|(parent, _)| PLACES.iter().position(|(other, _)| *other == parent))
            })
            .collect();
        let items = PLACES.iter().map(|&(path, class)| Place {
            path: names.intern(path),
            class,
            role: if class == Class::Outside { Role::Outside(None) } else { Role::Account { institution: None } },
            kind: thing,
            owner: if JORDAN_OWNS.contains(&path) { jordan } else { me },
            shares: Box::default(),
            known_as: Box::default(),
            doc: None,
            loc: (!path.starts_with("assets/vault")).then(|| line(1)),
        });
        let (places, place_ids) = Tree::build(items.collect(), &parents).unwrap();

        let mut commodities = Arena::new();
        let mut commodity = |symbol: &'static str, scale| {
            commodities.push(Commodity { symbol: names.intern(symbol), kind: thing, scale, doc: None, loc: None })
        };
        let (usd, vti) = (commodity("USD", 2), commodity("VTI", 3));

        let jurisdictions = ["us", "us/ca"].map(|path| System {
            path: names.intern(path),
            laws: Box::default(),
            currency: None,
            rates: None,
            doc: None,
            loc: None,
        });
        let (systems, system_ids) = Tree::build(jurisdictions.into(), &[None, Some(0)]).unwrap();

        Cast {
            names,
            kinds,
            me,
            landlord,
            acme,
            irs,
            nsf,
            market,
            unknown: ids[8],
            opening: ids[9],
            entities,
            places,
            place_ids,
            usd,
            vti,
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
        slots: axiom_core::Run::default(),
        laws: Box::default(),
        doc: None,
        loc: None,
    }
}

// ─── What happened ──────────────────────────────────────────────────────────

/// Three months of a household: salary, rent, groceries, a visa card, a year's
/// insurance spread over 2026, a pending repair check, 40 that vanished, and
/// 1,000 put away for retirement. Jordan is paid into an account of their own,
/// a client owes an invoice that was partly paid, and a repair bill is partly
/// paid.
struct Journal {
    txns: Arena<Txn>,
    flows: Arena<Flow>,
    codes: Arena<Sym>,
    details: Arena<Detail>,
    posted: Vec<Posted>,
}

fn journal(cast: &mut Cast) -> Journal {
    let landlord = Some(cast.landlord);
    let client = Some(cast.acme);
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
        (13, day(2026, 3, 2), "income/design", "assets/owed/clients", 480_000, client, State::Actual),
        (16, day(2026, 3, 5), "liabilities/bills", "expenses/repairs", 120_000, None, State::Actual),
        (14, day(2026, 3, 10), "income/jordan-pay", "assets/bank/jordan-checking", 300_000, None, State::Actual),
        (17, day(2026, 3, 12), "assets/bank/jordan-checking", "liabilities/bills", 50_000, None, State::Actual),
        (9, day(2026, 3, 15), "income/salary", "assets/bank/checking", 500_000, None, State::Actual),
        (10, day(2026, 3, 20), "assets/bank/checking", "equity/unknown", 4_000, None, State::Actual),
        (11, day(2026, 3, 25), "assets/bank/checking", "liabilities/visa", 12_000, None, State::Actual),
        (15, day(2026, 3, 26), "assets/owed/clients", "assets/bank/jordan-checking", 180_000, client, State::Actual),
        (12, day(2026, 3, 28), "assets/bank/checking", "assets/retirement", 100_000, None, State::Actual),
    ];
    let (check, invoice, bill) =
        (cast.names.intern("check-1041"), cast.names.intern("inv-12"), cast.names.intern("bill-7"));
    let mut journal = Journal {
        txns: Arena::new(),
        flows: Arena::new(),
        codes: Arena::new(),
        details: Arena::new(),
        posted: Vec::new(),
    };
    for (index, &(row, when, from, to, cents, payee, state)) in rows.iter().enumerate() {
        let codes: &[Sym] = match row {
            8 => &[check],
            13 | 15 => &[invoice],
            16 | 17 => &[bill],
            _ => &[],
        };
        let code_start = journal.codes.len();
        for &code in codes {
            journal.codes.push(code);
        }
        let header_codes = axiom_core::Run::new(Id::new(code_start as u32), u32::try_from(codes.len()).unwrap());
        let local_codes = axiom_core::Run::new(Id::new((code_start + codes.len()) as u32), 0);
        let due = match row {
            13 => Some(day(2026, 3, 20)),
            16 => Some(day(2026, 4, 4)),
            _ => None,
        };
        let detail = due.map(|due| journal.details.push(Detail { due: Some(due), ..Detail::NONE }));
        let txn = journal.txns.push(Txn {
            day: when,
            flows: axiom_core::Run::new(Id::new(index as u32), 1),
            inputs: axiom_core::Run::new(Id::new(0), 0),
            program: None,
            codes: header_codes,
            waive: None,
            contract: None,
            contract_schedule: None,
            occurrence: None,
            kind: axiom_model::journal::TxnKind::Journal,
            doc: (row == 13).then(|| cast.names.intern("/// The March design invoice.")),
            loc: line(row),
        });
        let amount = Amount::new(Qty(cents), cast.usd);
        let owner = [cast.id(from), cast.id(to)].map(|id| &cast.places[id]);
        let owner = owner.into_iter().find(|place| place.class != Class::Outside).map_or(cast.me, |place| place.owner);
        journal.flows.push(Flow {
            day: when,
            // The insurance is paid for the whole year.
            recognized: Days::new(when, if row == 1 { day(2026, 12, 31) } else { when }).unwrap(),
            from: cast.id(from),
            to: cast.id(to),
            out: amount,
            arrive: amount,
            mode: if state == State::Pending { Mode::Pending } else { Mode::Actual },
            infer: Infer::Known,
            txn,
            payee,
            owner,
            purpose: None,
            description: None,
            origin: Origin::Written,
            select: axiom_core::Run::new(Id::new(0), 0),
            header_codes,
            codes: local_codes,
            loc: line(row),
            waive: None,
            detail,
        });
        journal.posted.push(Posted { out: Qty(cents), arrive: Qty(cents), state });
    }
    journal
}

/// A law over `left op right`, both written amounts: `warn total(in, month) <= 500 USD`.
fn limit_law(cast: &mut Cast, name: &'static str, owner: Owner, op: BinOp, warn: bool) -> Law {
    let node = |op, ty, first| Node { op, ty: Some(ty), loc: line(80), first: NodeId(first) };
    let limit = Amount::new(Qty(50_000), cast.usd);
    let nodes = vec![
        node(Op::Call(Func::Total(Dir::In, Window::Month), Box::default()), Ty::AMOUNT, 0),
        node(Op::Const(Value::Amount(limit)), Ty::AMOUNT, 1),
        node(Op::Bin(op, NodeId(0), NodeId(1)), Ty::Bool, 0),
    ];
    Law {
        name: cast.names.intern(name),
        doc: Some(cast.names.intern(Box::leak(format!("/// The {name} law says what it says.").into_boxed_str()))),
        owner,
        system: None,
        trigger: Trigger::In,
        budget: None,
        overrides: None,
        override_name: None,
        rank: Rank::ZERO,
        steps: Box::new([Step {
            loc: line(80),
            kind: StepKind::Require {
                cond: NodeId(2),
                otherwise: Box::default(),
                message: None,
                severity: if warn { Severity::Warning } else { Severity::Error },
            },
        }]),
        nodes: nodes.into(),
        loc: line(80),
    }
}

/// `on out`, `owe amount * 10% to irs as early-withdrawal`: leaving the retirement account costs a tenth.
fn early_withdrawal(cast: &mut Cast) -> Law {
    let node = |op, ty, first| Node { op, ty: Some(ty), loc: line(85), first: NodeId(first) };
    let nodes = vec![
        node(Op::Var(Var::Amount), Ty::AMOUNT, 0),
        node(Op::Const(Value::Num(Ratio::percent(10, 0).unwrap())), Ty::Num, 1),
        node(Op::Bin(BinOp::Mul, NodeId(0), NodeId(1)), Ty::AMOUNT, 0),
    ];
    let owe =
        Consequence::Owe { amount: NodeId(2), to: cast.irs, due: None, name: cast.names.intern("early-withdrawal") };
    Law {
        name: cast.names.intern("early-withdrawal"),
        doc: None,
        owner: Owner::Place(cast.id("assets/retirement")),
        system: None,
        trigger: Trigger::Out,
        budget: None,
        overrides: None,
        override_name: None,
        rank: Rank::ZERO,
        steps: Box::new([Step { loc: line(85), kind: StepKind::Effect(owe) }]),
        nodes: nodes.into(),
        loc: line(85),
    }
}

/// What the laws and the engine recorded.
struct Records {
    laws: Arena<Law>,
    effects: Vec<Effect>,
    holdings: Vec<Holding>,
    gains: Vec<Gain>,
}

fn records(cast: &mut Cast, journal: &Journal) -> Records {
    let mut laws = Arena::new();
    let food = Owner::Place(cast.id("expenses/food"));
    let budget = laws.push(limit_law(cast, "budget", food, BinOp::Le, true));
    let wages = laws.push(Law {
        name: cast.names.intern("wages"),
        doc: None,
        owner: Owner::System(cast.us),
        system: Some(cast.us),
        trigger: Trigger::In,
        budget: None,
        overrides: None,
        override_name: None,
        rank: Rank::ZERO,
        steps: Box::default(),
        nodes: Arena::new(),
        loc: line(90),
    });
    assert_eq!((budget, wages), (Id::new(0), Id::new(1)), "the rules and readings refer to laws by position");
    let retirement = Owner::Place(cast.id("assets/retirement"));
    laws.push(limit_law(cast, "deferral-limit", retirement, BinOp::Le, false));
    laws.push(limit_law(cast, "overdraft", Owner::Place(cast.id("assets/bank/checking")), BinOp::Ge, true));
    laws.push(early_withdrawal(cast));

    // Names are one namespace per person-year: both systems add to `agi`.
    let (me, irs, usd) = (cast.me, cast.irs, cast.usd);
    let owed = |to| Some(Owed { to, due: day(2027, 4, 15) });
    let mut effect = |system, when, name: &'static str, cents, owe: Option<Owed>, cause| Effect {
        law: wages,
        subject: Subject::Entity(me),
        owner: me,
        system: Some(system),
        day: when,
        name: cast.names.intern(name),
        amount: Amount::new(Qty(cents), usd),
        consequence: owe.map_or(axiom_engine::Consequence::Count, axiom_engine::Consequence::Owe),
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

    let gain = |qty: i64, basis: i64, proceeds: i64, acquired: Day, sold: Day, unit| Gain {
        cause: Cause::Time,
        day: sold,
        from: cast.id("assets/retirement"),
        to: cast.id("assets/bank/checking"),
        unit,
        qty: Qty(qty),
        basis: Qty(basis),
        proceeds: Qty(proceeds),
        acquired,
        ambiguous: false,
    };
    let gains = vec![
        gain(100, 20_000, 50_000, day(2025, 6, 1), day(2026, 2, 10), usd),
        gain(2_000, 60_000, 70_000, day(2025, 12, 1), day(2026, 2, 12), cast.vti),
        gain(5_000, 100_000, 180_000, day(2024, 1, 5), day(2026, 3, 3), cast.vti),
    ];
    Records { laws, effects, holdings: holdings(cast, journal), gains }
}

/// The journal's final state, as the engine leaves it: plain money, except in
/// the places that hold parcels. Sorted by place, then commodity.
fn holdings(cast: &Cast, journal: &Journal) -> Vec<Holding> {
    let mut plain: BTreeMap<Id<Place>, Qty> = BTreeMap::new();
    for (flow, posted) in journal.flows.values().zip(&journal.posted) {
        if posted.state.is_real_on(day(2026, 12, 31)) {
            *plain.entry(flow.from).or_default() -= posted.out;
            *plain.entry(flow.to).or_default() += posted.arrive;
        }
    }
    let parcel = |qty: Qty, basis: i64, acquired: Day, txn: u32, tied| Parcel {
        qty,
        basis: Qty(basis),
        acquired,
        held_since: acquired,
        wash_matched: false,
        txn: RuntimeTxn::journal(Id::new(txn)).unwrap(),
        part: None,
        codes: axiom_model::FlowCodes {
            header: journal.flows[Id::new(txn)].header_codes,
            local: journal.flows[Id::new(txn)].codes,
        },
        tied,
    };
    let mut lots: BTreeMap<Id<Place>, Vec<Parcel>> = BTreeMap::new();
    // 500 in checking is tied to the grant it came from.
    lots.entry(cast.id("assets/bank/checking")).or_default().push(parcel(
        Qty(50_000),
        50_000,
        day(2025, 6, 1),
        0,
        Some(cast.nsf),
    ));
    // The retirement money has no basis.
    let retired = plain[&cast.id("assets/retirement")];
    lots.entry(cast.id("assets/retirement")).or_default().push(parcel(retired, 0, day(2026, 3, 28), 16, None));
    // A claim keeps the transaction that made it.
    let owed = plain[&cast.id("assets/owed/clients")];
    lots.entry(cast.id("assets/owed/clients")).or_default().push(parcel(owed, owed.0, day(2026, 3, 2), 8, None));
    for (place, held) in &lots {
        *plain.get_mut(place).unwrap() -= held.iter().map(|lot| lot.qty).sum();
    }
    plain
        .into_iter()
        .map(|(place, plain)| Holding { place, unit: cast.usd, plain, lots: lots.remove(&place).unwrap_or_default() })
        .filter(|holding| !holding.is_empty())
        .collect()
}

pub(crate) fn household() -> Household {
    let mut cast = Cast::new();
    let journal = journal(&mut cast);
    let records = records(&mut cast, &journal);

    let food = cast.id("expenses/food");
    let budget_rule = Rule { law: Id::new(0), subject: Subject::Place(food), days: Days::ALWAYS };
    // The deferral limit governs the retirement place only until the end of 2025.
    let retirement = cast.id("assets/retirement");
    let ends_2025 = Days::new(Day::MIN, day(2025, 12, 31)).unwrap();
    let lapsed = Rule { law: Id::new(2), subject: Subject::Place(retirement), days: ends_2025 };
    let in_force = Rule { law: Id::new(3), subject: Subject::Place(retirement), days: Days::ALWAYS };
    let penalty = Rule { law: Id::new(4), subject: Subject::Place(retirement), days: Days::ALWAYS };
    let rules = Rules {
        on_in: Groups::build(
            cast.places.len(),
            [(food, budget_rule), (retirement, lapsed), (retirement, in_force)].into_iter(),
        ),
        on_out: Groups::build(cast.places.len(), [(retirement, penalty)].into_iter()),
        ..Rules::default()
    };
    let touching =
        Groups::build(cast.places.len(), journal.flows.iter().flat_map(|(id, flow)| [(flow.from, id), (flow.to, id)]));
    let (purposes, [income, spending, capital, transfer]) = Purpose::roots(&mut cast.names);
    let roots = Roots {
        me: cast.me,
        unknown: cast.unknown,
        opening: cast.opening,
        market: cast.market,
        kinds: KindRoots {
            asset: Id::new(0),
            debt: Id::new(0),
            thing: Id::new(0),
            commodity: Id::new(0),
            measure: Id::new(0),
            entity: Id::new(0),
        },
        purposes: PurposeRoots { income, spending, capital, transfer },
    };
    let law_count = records.laws.len();
    let holders = HolderIndex::new(cast.kinds.len(), cast.places.len(), cast.entities.len(), cast.commodities.len(), 0);
    let mut said = Facts::builder(holders.len());
    said.paint_always(holders.number(cast.id("assets/owed/clients")), builtin::CLAIM, true);
    for household_member in [0, 1] {
        said.paint_always(
            holders.number(Holder::Entity(Id::new(household_member))),
            builtin::MEMBER,
            Id::<Entity>::new(2),
        );
    }
    said.paint_always(holders.number(cast.id("assets/retirement")), builtin::LIQUIDITY, Span::months(1));
    let book = Book {
        names: cast.names,
        text_values: Arena::new(),
        base: cast.usd,
        relaxed: false,
        roots,
        issuer_places: Default::default(),
        places: cast.places,
        entities: cast.entities,
        kinds: cast.kinds,
        schema: Default::default(),
        holders,
        facts: said.freeze(),
        sites: Default::default(),
        purposes,
        systems: cast.systems,
        commodities: cast.commodities,
        assets: Arena::new(),
        contracts: Arena::new(),
        also: Arena::new(),
        laws: records.laws,
        rules,
        budgets: Arena::new(),
        params: Arena::new(),
        schedules: Arena::new(),
        code_rules: Vec::new(),
        codes: journal.codes,
        selectors: Arena::new(),
        details: journal.details,
        patterns: Arena::new(),
        formats: Arena::new(),
        txns: journal.txns,
        journal_programs: Arena::new(),
        written_occurrences: Arena::new(),
        input_values: Arena::new(),
        flows: journal.flows,
        touching,
        asserts: Vec::new(),
        assertion_programs: Arena::new(),
        events: Vec::new(),
        endings: Vec::new(),
        claim_changes: Vec::new(),
        prices: Prices::default(),
        lookup: Default::default(),
        splits: Vec::new(),
        measures: Arena::new(),
        readings: Vec::new(),
        filed: Vec::new(),
        sources: Vec::new(),
    };
    let run = Run {
        today: day(2026, 3, 31),
        horizon: day(2026, 3, 31),
        posted: journal.posted.into(),
        holdings: records.holdings,
        gains: records.gains,
        effects: records.effects,
        violations: Vec::new(),
        headroom: Vec::new(),
        pads: Vec::new(),
        assets: Vec::new(),
        pending_carries: Vec::new(),
        promises: Vec::new(),
        adjustments: Vec::new(),
        checks: vec![0; law_count].into(),
        promised_flows: Box::default(),
        runtime_details: Arena::new(),
        missing_inputs: Box::default(),
        open_claims: Box::default(),
        monitor_complete: true,
        diagnostics: Vec::new(),
    };
    Household { book, run }
}

impl Household {
    /// The same household after the engine has recorded what each limit counted.
    fn with_headroom(mut self) -> Household {
        let (me, jordan) = (self.book.roots.me, self.book.entities.iter().nth(1).unwrap().0);
        let usd = self.book.base;
        let cents = |cents| Amount::new(Qty(cents), usd);
        let reading = |law, place: &str, from: Day, until: Day, counted, limit, owner, warn| Headroom {
            law: Id::new(law),
            step: 0,
            subject: Subject::Place(self.place(place)),
            owner,
            days: Days::new(from, until).unwrap(),
            counted: cents(counted),
            limit: cents(limit),
            day: until,
            warn,
            bound: Bound::Cap,
        };
        let (jan, feb) = ((day(2026, 1, 1), day(2026, 1, 31)), (day(2026, 2, 1), day(2026, 2, 28)));
        let year = (day(2026, 1, 1), day(2026, 12, 31));
        let readings = vec![
            // Groceries this year, month by month, against 500 a month.
            reading(0, "expenses/food", jan.0, jan.1, 8_420, 50_000, me, true),
            reading(0, "expenses/food", feb.0, feb.1, 12_000, 50_000, me, true),
            // A cap whose limit is a parameter lookup: 2,400 of 24,000.
            reading(2, "assets/retirement", year.0, year.1, 240_000, 2_400_000, me, false),
            // The same cap for Jordan, who has used 900 of 18,000.
            reading(2, "assets/bank/jordan-checking", year.0, year.1, 90_000, 1_800_000, jordan, false),
            // A floor: the balance may not go below zero, and stands at 8,955.80.
            Headroom {
                bound: Bound::Floor,
                ..reading(3, "assets/bank/checking", day(2026, 3, 31), day(2026, 3, 31), 0, 895_580, me, true)
            },
            // A yearly budget, over.
            reading(0, "expenses/insurance", year.0, year.1, 120_000, 100_000, me, true),
        ];
        self.run.headroom = readings;
        self
    }
}

// ─── Rendering, so figures can be compared as text ──────────────────────────

pub(crate) fn cell(item: &Cell<'_>) -> String {
    match item {
        Cell::Blank => String::new(),
        Cell::Word(word) => (*word).to_string(),
        Cell::Text(text) => text.to_string(),
        Cell::Name(name) => (*name).to_string(),
        Cell::Code(code) => format!("^{code}"),
        Cell::Purpose(purpose) => format!("#{purpose}"),
        Cell::Said(text) => text.to_string(),
        Cell::Amount { qty, scale, unit } => format!("{} {unit}", qty.show(*scale)),
        Cell::Day(day) => day.to_string(),
        Cell::Span(span) => span.to_string(),
        Cell::Period(days) => format!("{}..{}", days.first(), days.last()),
        Cell::Percent(ratio) => format!("{}%", Ratio::new(ratio.num() as i128 * 100, ratio.den() as i128).unwrap()),
        Cell::Number(ratio) => ratio.to_string(),
        Cell::Count(count, noun) => format!("{count} {noun}"),
        Cell::Trigger(trigger) => format!("{trigger:?}"),
        Cell::Source(loc) => format!("@{}", loc.start / 100),
        Cell::Join(separator, parts) => parts.iter().map(cell).collect::<Vec<_>>().join(separator),
    }
}

pub(crate) fn heading<'a>(section: &'a Section<'_>) -> Option<&'a str> {
    match section.heading.as_ref()? {
        Cell::Word(text) | Cell::Name(text) | Cell::Purpose(text) | Cell::Code(text) => Some(text),
        Cell::Text(text) | Cell::Said(text) => Some(text),
        _ => None,
    }
}

pub(crate) fn lines(section: &Section) -> Vec<String> {
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

pub(crate) fn show(report: &Report) -> String {
    let mut out = format!("# {}\n", cell(&report.title));
    for section in &report.sections {
        out +=
            &format!("##{}\n", section.heading.as_ref().map_or(String::new(), |heading| format!(" {}", cell(heading))));
        out += &lines(section).join("\n");
        out += &section.notes.iter().map(|note| format!("\n  note: {}", cell(note))).collect::<String>();
        out += "\n";
    }
    out
}

fn table(house: &Household, query: Query) -> String {
    show(&house.report(query))
}

fn balance(globs: Vec<&'static str>, at: Option<Day>, value: bool, monthly: bool) -> Query<'static> {
    Query::Balance { globs, at, value, monthly }
}

// ─── Balances, and whose they are ───────────────────────────────────────────

#[test]
fn balance_is_a_tree_with_subtotals_and_net_worth() {
    let house = household();
    // Checking: three salaries in, less the year's insurance, two rents,
    // groceries, 40 that vanished, the card payment and 1,000 put away. The
    // 350 check is only pending. The card nets to nothing and is left out.
    // Jordan's account: 3,000 of pay and 1,800 from the client, less 500 to
    // the bill. The client still owes 3,000; 700 of the 1,200 bill is unpaid.
    let expected = "\
# Balances at 2026-03-31
##
=assets | 17,255.80 USD
  bank | 13,255.80 USD
    checking | 8,955.80 USD
    jordan-checking | 4,300.00 USD
  owed | 3,000.00 USD
    clients | 3,000.00 USD
  retirement | 1,000.00 USD
=liabilities | 700.00 USD
  bills | 700.00 USD
##
Assets | 17,255.80 USD
Liabilities | 700.00 USD
=Net worth | 16,555.80 USD
";
    assert_eq!(table(&house, balance(vec![], None, false, false)), expected);
}

#[test]
fn a_past_date_and_monthly_columns_read_the_same_flows() {
    let house = household();
    let january = table(&house, balance(vec!["checking"], Some(day(2026, 1, 20)), false, false));
    assert!(january.contains("checking | 1,915.80 USD"));
    let monthly = house.report(balance(vec!["checking"], None, true, true));
    let titles: Vec<_> = monthly.sections[0].columns.iter().map(|column| cell(&column.title)).collect();
    assert_eq!(titles, ["Place", "2026-01-31", "2026-02-28", "2026-03-31"]);
    assert_eq!(lines(&monthly.sections[0])[2], "    checking | 1,915.80 USD | 5,115.80 USD | 8,955.80 USD");
}

#[test]
fn the_legacy_report_keeps_the_run_its_caller_supplied() {
    let mut house = household();
    let checking = house.place("assets/bank/checking");
    house.run.holdings.iter_mut().find(|holding| holding.place == checking).unwrap().plain += Qty(100);
    let report = crate::report(&house.book, &house.run, &balance(vec!["checking"], None, false, false), None).unwrap();
    assert!(lines(&report.sections[0]).iter().any(|row| row == "    checking | 8,956.80 USD"));
}

#[test]
fn today_from_the_run_and_from_the_flows_agree() {
    // The run's holdings answer for today; two days are one pass over the flows.
    let house = household();
    let (everyone, today) = (Whose::default(), house.run.today);
    let plan = axiom_engine::Plan::new(&house.book);
    let lens = crate::lens::Lens::new(&plan, &everyone, today);
    let from_holdings = crate::history::Snapshots::of(lens, &house.run, &[today], false);
    let from_flows = crate::history::Snapshots::of(lens, &house.run, &[day(2026, 2, 1), today], false);
    for place in house.book.places.ids() {
        let (held, replayed) =
            (from_holdings.subtree(&house.book, 0, place), from_flows.subtree(&house.book, 1, place));
        assert_eq!(
            held.get(house.book.base),
            replayed.get(house.book.base),
            "{}",
            house.book.name(house.book.places[place].path)
        );
    }
}

#[test]
fn snapshot_storage_scales_with_occupied_owner_pairs() {
    let house = household();
    let days = [day(2026, 1, 31), house.run.today];
    let everyone = Whose::default();
    let plan = axiom_engine::Plan::new(&house.book);
    let lens = crate::lens::Lens::new(&plan, &everyone, house.run.today);
    let all = crate::history::Snapshots::of(lens, &house.run, &days, false);
    let (all_pairs, all_cells) = all.storage_shape();
    let dense_cells = days.len() * house.book.places.len() * house.book.commodities.len();
    assert_eq!(all_cells, days.len() * all_pairs);
    assert!(all_cells < dense_cells / 2, "{all_cells} stored cells versus {dense_cells} dense cells");

    let jordan = Whose::of(&house.book, house.entity("jordan"));
    let jordan_lens = crate::lens::Lens::new(&plan, &jordan, house.run.today);
    let scoped = crate::history::Snapshots::of(jordan_lens, &house.run, &days, false);
    let (jordan_pairs, jordan_cells) = scoped.storage_shape();
    assert_eq!(jordan_cells, days.len() * jordan_pairs);
    assert!(jordan_pairs < all_pairs, "owner scope should omit unowned pairs");
}

#[test]
fn a_filtered_balance_keeps_context_but_no_net_worth() {
    let house = household();
    let report = house.report(balance(vec!["checking"], None, false, false));
    assert_eq!(lines(&report.sections[0])[..3], ["~assets |", "~  bank |", "    checking | 8,955.80 USD"]);
    assert_eq!(report.sections.len(), 1);
}

#[test]
fn a_view_can_be_about_one_person_or_their_household() {
    let house = household();
    let jordan = house.report_for(balance(vec![], None, false, false), Some("jordan")).unwrap();
    assert_eq!(
        lines(&jordan.sections[0]),
        ["=assets | 4,300.00 USD", "  bank | 4,300.00 USD", "    jordan-checking | 4,300.00 USD",]
    );
    // The household is its members: everything either owns.
    let together = house.report_for(balance(vec![], None, false, false), Some("household")).unwrap();
    let everyone = house.report(balance(vec![], None, false, false));
    assert_eq!(lines(&together.sections[0]), lines(&everyone.sections[0]));
    // Spending is scoped the same way.
    let pay = house
        .report_for(Query::Flow { by: FlowBy::Period(Period::Month), from: None, to: None }, Some("jordan"))
        .unwrap();
    assert!(
        pay.sections[0].facts.iter().any(|fact| {
            fact.concept == "unclassified" && fact.entity == "jordan" && fact.value.qty == Qty(350_000)
        })
    );
    let me_pay =
        house.report_for(Query::Flow { by: FlowBy::Period(Period::Month), from: None, to: None }, Some("me")).unwrap();
    assert!(me_pay.sections[0].facts.iter().all(|fact| fact.entity != "jordan"));
    assert!(!lines(&pay.sections[0]).iter().any(|line| line.contains("salary")));
}

// ─── Statements ─────────────────────────────────────────────────────────────

#[test]
fn register_runs_a_balance_and_mutes_the_pending_check() {
    let house = household();
    let checking = house.place("assets/bank/checking");
    let plan = axiom_engine::Plan::new(&house.book);
    let whose = Whose::default();
    let lens = crate::lens::Lens::new(&plan, &whose, house.run.today);
    let section = crate::register::section_for_lens(lens, &house.run, checking, None, None);
    let rows = lines(&section);
    assert_eq!(rows.len(), 11);
    assert_eq!(rows[6], "~2026-03-01 | expenses/repairs |  | ^check-1041 · pending | -350.00 USD | 5,115.80 USD");
    // A window opens with the balance carried in.
    let march = crate::register::section_for_lens(lens, &house.run, checking, Some(day(2026, 3, 1)), None);
    assert_eq!(lines(&march)[0], "=2026-03-01 | opening balance |  |  |  | 5,115.80 USD");
}

#[test]
fn the_register_of_a_liability_reads_the_way_a_statement_does() {
    let house = household();
    let bills = house.place("liabilities/bills");
    let plan = axiom_engine::Plan::new(&house.book);
    let whose = Whose::default();
    let lens = crate::lens::Lens::new(&plan, &whose, house.run.today);
    let section = crate::register::section_for_lens(lens, &house.run, bills, None, None);
    // A bill of 1,200 is owed; 500 paid leaves 700.
    assert_eq!(
        lines(&section),
        [
            "2026-03-05 | expenses/repairs |  | ^bill-7 | 1,200.00 USD | 1,200.00 USD",
            "2026-03-12 | assets/bank/jordan-checking |  | ^bill-7 | -500.00 USD | 700.00 USD"
        ]
    );
}

#[test]
fn tax_groups_tallies_by_system_and_totals_each_jurisdiction() {
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
=  Total us |  |  | 900.00 USD |
=  us/ca |  |  |  |
    ca-income-tax | irs | 2027-04-15 | 150.00 USD | period end
=    Total us/ca |  |  | 150.00 USD |
=Total owed |  |  | 1,050.00 USD |
  note: Trace any line with `axiom why NAME`, or `axiom why FILE:LINE` from its source.
";
    assert_eq!(table(&house, Query::Tax { year: None }), expected);
}

#[test]
fn a_priced_penalty_is_marked_as_one() {
    let mut house = household();
    let penalty = house.run.effects[4].owed().expect("an obligation");
    house.run.effects[4].consequence = axiom_engine::Consequence::Penalty(penalty);
    let owed = table(&house, Query::Tax { year: None });
    assert!(owed.contains("federal-tax (penalty) | irs"));
    assert!(owed.contains("note: A penalty is the price of a violated law"));
}

// ─── Parcels ────────────────────────────────────────────────────────────────

#[test]
fn lots_list_parcels_with_basis_gain_and_term() {
    let house = household();
    // Money has no holding period: a base-currency lot shows no term.
    let expected = "\
# Lots at 2026-03-31
##
assets/bank/checking | 500.00 USD | 500.00 USD | 2025-06-01 | 9m30d | 500.00 USD | 0.00 USD |  | tied to nsf
assets/owed/clients | 3,000.00 USD | 3,000.00 USD | 2026-03-02 | 29d | 3,000.00 USD | 0.00 USD |  | ^inv-12
assets/retirement | 1,000.00 USD | 0.00 USD | 2026-03-28 | 3d | 1,000.00 USD | 1,000.00 USD |  |
=Total |  | 3,500.00 USD |  |  | 4,500.00 USD | 1,000.00 USD |  |
";
    assert_eq!(table(&house, Query::Lots { place: None, at: None }), expected);
}

#[test]
fn gains_are_listed_as_form_8949_does_with_short_and_long_subtotals() {
    let house = household();
    let expected = "\
# Gains realized in 2026
##
2026-02-12 | 2025-12-01 | 2.000 VTI | assets/retirement | 700.00 USD | 600.00 USD | 100.00 USD | short
=Short-term |  |  |  | 700.00 USD | 600.00 USD | 100.00 USD |
2026-03-03 | 2024-01-05 | 5.000 VTI | assets/retirement | 1,800.00 USD | 1,000.00 USD | 800.00 USD | long
=Long-term |  |  |  | 1,800.00 USD | 1,000.00 USD | 800.00 USD |
2026-02-10 | 2025-06-01 | 1.00 USD | assets/retirement | 500.00 USD | 200.00 USD | 300.00 USD |
=Money withdrawn |  |  |  | 500.00 USD | 200.00 USD | 300.00 USD |
=Total |  |  |  | 3,000.00 USD | 1,800.00 USD | 1,200.00 USD |
";
    let text = table(&house, Query::Gains { year: Some(2026) });
    assert!(text.ends_with(expected.split_once("##\n").unwrap().1), "{text}");
    assert!(table(&house, Query::Gains { year: Some(2025) }).contains("Nothing was sold in 2025"));
}

// ─── Limits and budgets ─────────────────────────────────────────────────────

#[test]
fn limits_rank_every_cap_by_how_much_of_it_is_used() {
    let house = household().with_headroom();
    let report = house.report(Query::Limits { year: Some(2026) });
    // Over its limit first, then by share used. The food budget is in its
    // current window, March, where nothing has been spent yet; the overdraft
    // floor of nothing is an invariant, not a limit, and is left out.
    assert_eq!(
        lines(&report.sections[0]),
        [
            "!me | budget on expenses/insurance | 2026 | 1,200.00 USD | 1,000.00 USD | -200.00 USD | 120%",
            "me | deferral-limit on assets/retirement | 2026 | 2,400.00 USD | 24,000.00 USD | 21,600.00 USD | 10%",
            "jordan | deferral-limit on assets/bank/jordan-checking | 2026 | 900.00 USD | 18,000.00 USD | 17,100.00 USD | 5%",
            "me | budget on expenses/food | 2026-03 | 0.00 USD | 500.00 USD | 500.00 USD | 0%",
        ]
    );
}

#[test]
fn limits_are_per_owner_and_per_year() {
    let house = household().with_headroom();
    let jordan = house.report_for(Query::Limits { year: Some(2026) }, Some("jordan")).unwrap();
    assert_eq!(lines(&jordan.sections[0]).len(), 1);
    let none = house.report(Query::Limits { year: Some(2024) });
    assert!(cell(&none.sections[0].notes[0]).contains("No limit was read in 2024"));
}

// ─── Claims ─────────────────────────────────────────────────────────────────

#[test]
fn claims_list_what_is_owed_with_its_age_and_what_is_overdue() {
    let house = household();
    let report = house.report(Query::Claims { at: None });
    // The invoice was 4,800, 1,800 has been paid, and it fell due on the 20th.
    assert_eq!(
        lines(&report.sections[0]),
        [
            "!acme | ^inv-12 · The March design invoice. | 3,000.00 USD | 2026-03-02 | 29d | 2026-03-20 | overdue 11d",
            "=Total |  | 3,000.00 USD |  |  |  |"
        ]
    );
}

#[test]
fn a_bill_is_netted_by_its_code_across_the_flows_that_made_and_settled_it() {
    let house = household();
    let lens_owner = Whose::default();
    let plan = axiom_engine::Plan::new(&house.book);
    let lens = crate::lens::Lens::new(&plan, &lens_owner, day(2026, 3, 31));
    let bills = crate::claims::owed_by_you(lens, &house.run, house.place("liabilities/bills"));
    let [bill] = &bills[..] else { panic!("one bill is open") };
    assert_eq!(
        (bill.left.qty, bill.made, bill.due, bill.mine),
        (Qty(70_000), day(2026, 3, 5), Some(day(2026, 4, 4)), false)
    );
    // Paid in full, it is no longer open.
    let paid = crate::lens::Lens::new(&plan, &lens_owner, day(2026, 3, 5));
    assert_eq!(
        crate::claims::owed_by_you(paid, &house.run, house.place("liabilities/bills"))[0].left.qty,
        Qty(120_000)
    );
}

// ─── Codes, lines and the summary ───────────────────────────────────────────

#[test]
fn a_code_finds_its_flows_and_a_line_explains_itself() {
    let house = household();
    let code = table(&house, Query::Why { target: "^check-1041" });
    assert!(code.contains("2026-03-01 | assets/bank/checking → expenses/repairs | 350.00 USD | pending | @8"));
    let line = table(&house, Query::Line { loc: line(3) });
    assert!(line.contains("flow: assets/bank/checking → expenses/rent, 1,800.00 USD | @3"));
    let stray = house.report_for(Query::Why { target: "^check-1014" }, None).err().expect("no such code");
    assert_eq!(stray.help[0].text, "did you mean `check-1041`?");
}

#[test]
fn the_summary_counts_the_places_that_were_declared_or_used() {
    let house = household();
    let summary = crate::summary(&house.book, &house.run);
    // Twenty-three places are declared or touched; the two vault places are neither.
    assert_eq!((summary.flows, summary.places, summary.unpriced), (17, 23, 0));
    assert_eq!(house.book.show(summary.net_worth).to_string(), "16,555.80 USD");
}

// ─── Why ────────────────────────────────────────────────────────────────────

#[test]
fn why_a_place_puts_its_limits_before_the_laws_and_leaves_out_laws_that_lapsed() {
    let house = household().with_headroom();
    let report = house.why(Found::Place(house.place("assets/retirement")));
    let headings: Vec<_> =
        report.sections.iter().map(|section| section.heading.as_ref().map_or_else(String::new, cell)).collect();
    assert_eq!(headings, ["Composition", "Parcels", "Limits", "Governed by", "Recent flows"]);
    let limits = lines(&report.sections[2]);
    assert_eq!(limits.len(), 1);
    assert!(
        limits[0].contains("deferral-limit on assets/retirement | 2026 | 2,400.00 USD | 24,000.00 USD | 21,600.00 USD")
    );
    // The rule that lapsed at the end of 2025 is not governing: the one in force is a limit, and
    // the law that prices leaving the account is a price.
    assert_eq!(
        lines(&report.sections[3]),
        [
            "=Limits |  |  |",
            "  overdraft | on in | The overdraft law says what it says. | @80",
            "=Prices |  |  |",
            "  early-withdrawal | on out |  | @85"
        ]
    );
    assert!(cell(&report.sections[3].notes[0]).starts_with("1 law not in force today"));
}

#[test]
fn why_an_entity_shows_its_places_ties_and_claims() {
    let house = household();
    let grant = house.why(Found::Entity(house.entity("nsf")));
    let ties = grant.sections.iter().find(|section| heading(section) == Some("Held for it")).unwrap();
    assert_eq!(lines(ties), ["assets/bank/checking | 500.00 USD | 2025-06-01 | @1", "=Remaining | 500.00 USD |  |"]);
    let client = house.why(Found::Entity(house.entity("acme")));
    let claims = client.sections.iter().find(|section| heading(section) == Some("Claims with it")).unwrap();
    assert!(lines(claims)[0].starts_with("!acme | ^inv-12"));
    let jordan = house.why(Found::Entity(house.entity("jordan")));
    // What is held, not what was earned: the statement of income and spending is `flow`.
    assert_eq!(lines(&jordan.sections[0]), ["assets/bank/jordan-checking | 4,300.00 USD"]);
}

#[test]
fn why_a_system_says_what_each_of_its_laws_counted() {
    let house = household();
    let us = house.book.systems.iter().next().unwrap().0;
    let report = house.why(Found::System(us));
    assert_eq!(report.sections[0].notes.iter().map(cell).collect::<Vec<_>>(), ["Nobody in this book lives here."]);
    assert_eq!(
        lines(&report.sections[1]),
        [
            "wages | on in | wages 10,000.00 USD · agi 10,200.00 USD · owes federal-tax 900.00 USD · owes ca-income-tax 150.00 USD | @90"
        ]
    );
}

#[test]
fn several_laws_with_one_name_are_listed_with_where_each_is_written() {
    let house = household();
    let report = house.why(Found::Laws(Box::new([Id::new(0), Id::new(3)])));
    assert_eq!(cell(&report.title), "`budget` is written in 2 places");
    assert_eq!(
        lines(&report.sections[0]),
        [
            "budget | project | @80 | expenses/food and everything beneath it | The budget law says what it says.",
            "overdraft | project | @80 | assets/bank/checking and everything beneath it | The overdraft law says what it says."
        ]
    );
    assert!(cell(&report.sections[0].notes[0]).contains("axiom why FILE:LINE"));
}

#[test]
fn available_subtracts_what_is_pending_and_lists_what_is_slower() {
    let house = household();
    let report = house.report(Query::Available { at: None });
    // In hand: checking 8,955.80 and Jordan's 4,300. Not yours to spend: the pending 350 repair.
    let spendable = lines(&report.sections[0]);
    assert_eq!(
        spendable,
        [
            "Money in hand | 13,255.80 USD",
            "~  assets/bank/checking | 8,955.80 USD",
            "~  assets/bank/jordan-checking | 4,300.00 USD",
            "Pending outflows | -350.00 USD",
            "~  2026-03-01 to expenses/repairs | -350.00 USD",
            "=Available to spend | 12,905.80 USD",
        ]
    );
    // The retirement account has a month's liquidity and a law that prices leaving it: a tenth, and
    // nothing else drives it. The card money in checking is cash and is not a candidate at all.
    let reach = lines(report.sections.last().unwrap());
    assert_eq!(
        reach,
        [
            "assets/retirement | 1m | 1,000.00 USD | 100.00 USD | 900.00 USD | driven by early-withdrawal 100.00 USD",
            "=If everything were drawn today |  | 1,000.00 USD | 100.00 USD | 900.00 USD |"
        ]
    );
}

#[test]
fn available_can_be_about_one_person() {
    let house = household();
    let report = house.report_for(Query::Available { at: None }, Some("jordan")).unwrap();
    assert_eq!(lines(&report.sections[0]).last().unwrap(), "=Available to spend | 4,300.00 USD");
}

#[test]
fn claims_and_registers_are_about_whose_money_they_are() {
    let house = household();
    let claims = house.report_for(Query::Claims { at: None }, Some("jordan")).unwrap();
    assert!(claims.sections[0].rows.is_empty(), "the invoice is the first person's");
    let me = house.entity("me");
    let plan = axiom_engine::Plan::new(&house.book);
    let mine = crate::views(
        crate::lens::Lens::new(&plan, &Whose::of(&house.book, me), house.run.today),
        &house.run,
        &Query::Claims { at: None },
    )
    .unwrap();
    assert_eq!(mine.sections[0].rows.len(), 2);
}

#[test]
fn tax_lines_are_kept_apart_by_person_when_several_have_them() {
    let mut house = household();
    let jordan = house.entity("jordan");
    let mut theirs = house.run.effects[0];
    (theirs.owner, theirs.subject) = (jordan, Subject::Entity(jordan));
    house.run.effects.push(theirs);
    let text = table(&house, Query::Tax { year: None });
    assert!(text.starts_with("# Taxes 2026\n"), "no single person to name: {text}");
    assert!(text.contains("=me · us |  |") && text.contains("=jordan · us |  |"), "{text}");
    // Asked about one person, the title says whose it is.
    let one = show(&house.report_for(Query::Tax { year: None }, Some("jordan")).unwrap());
    assert!(one.starts_with("# Taxes 2026 for jordan\n"), "{one}");
}

#[test]
fn a_window_of_all_time_is_named_and_never_panics() {
    let mut house = household().with_headroom();
    house.run.headroom[0].days = Days::ALWAYS;
    let report = house.report(Query::Limits { year: Some(2026) });
    assert!(lines(&report.sections[0]).iter().any(|row| row.contains("| ever |")));
}
