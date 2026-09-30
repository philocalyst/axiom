//! The v4 views over Sam, built by hand: a designer with a studio he owns, a
//! rented flat with an office corner, a rented-out condo with a mortgage, a
//! gym whose price changes, a client, a friend who borrows, and a tenant who is
//! late. The model does not build v4 books from source yet, so the book and its
//! run are assembled directly, as `tests` does for the v3 household.
//!
//! Today is 2026-03-05. In it: purposes (`groceries` is `food` is `spending`),
//! contracts whose terms change, an asset with parts, shares of a flat and a
//! phone, a budget that carries, promises kept and late, measures, and a
//! return filed for 2025.

use axiom_core::{
    Arena, Day, Days, FileId, Groups, Id, Interner, Loc, Qty, Ratio, Run as Ids, Span, Sym, Timeline, Tree,
};
use axiom_engine::{Adjustment, AdjustmentKind, AssetState, Cause, Effect, Holding, Parcel, Part, Posted, Promise};
use axiom_engine::{Run, State};
use axiom_model::*;

use crate::tests::{Household, kind};

pub(crate) fn day(y: i32, m: u32, d: u32) -> Day {
    Day::from_ymd(y, m, d).unwrap()
}

/// Line `n` of the imaginary journal.
pub(crate) fn line(n: u32) -> Loc {
    Loc::new(FileId(0), n * 100, n * 100 + 99)
}

const ENTITIES: [&str; 16] = [
    "me",
    "studio",
    "lumen",
    "greystar",
    "mint",
    "halcyon",
    "jo",
    "rocket",
    "irs",
    "chase",
    "fitclub",
    "dana",
    "trader-joes",
    "title-co",
    "bay-plumbing",
    "market",
];

/// Every place, by name, class and owner (`me` unless said). Parties' places
/// come after the owners' own, and the tabs and the assets after them.
const PLACES: [(&str, Class, &str); 27] = [
    ("me", Class::Asset, "me"),
    ("studio", Class::Asset, "studio"),
    ("checking", Class::Asset, "me"),
    ("visa", Class::Debt, "me"),
    ("unknown", Class::Outside, "me"),
    ("opening", Class::Outside, "me"),
    ("lumen", Class::Outside, "me"),
    ("greystar", Class::Outside, "me"),
    ("mint", Class::Outside, "me"),
    ("halcyon", Class::Outside, "studio"),
    ("jo", Class::Outside, "me"),
    ("rocket", Class::Outside, "me"),
    ("irs", Class::Outside, "me"),
    ("fitclub", Class::Outside, "me"),
    ("dana", Class::Outside, "me"),
    ("trader-joes", Class::Outside, "me"),
    ("title-co", Class::Outside, "me"),
    ("bay-plumbing", Class::Outside, "me"),
    ("market", Class::Outside, "me"),
    ("chase", Class::Outside, "me"),
    ("jo-tab", Class::Asset, "me"),
    ("halcyon-tab", Class::Asset, "studio"),
    ("rocket-tab", Class::Debt, "me"),
    ("condo", Class::Asset, "me"),
    ("car", Class::Asset, "me"),
    ("dana-tab", Class::Asset, "me"),
    ("studio-checking", Class::Asset, "studio"),
];

/// Purposes as `(name, parent, root, takes an object)`.
const PURPOSES: [(&str, Option<usize>, PurposeRoot, bool); 17] = [
    ("income", None, PurposeRoot::Income, false),
    ("spending", None, PurposeRoot::Spending, false),
    ("capital", None, PurposeRoot::Capital, false),
    ("wages", Some(0), PurposeRoot::Income, false),
    ("design", Some(0), PurposeRoot::Income, false),
    ("rent-received", Some(0), PurposeRoot::Income, false),
    ("food", Some(1), PurposeRoot::Spending, false),
    ("groceries", Some(6), PurposeRoot::Spending, false),
    ("rent", Some(1), PurposeRoot::Spending, false),
    ("phone", Some(1), PurposeRoot::Spending, false),
    ("interest", Some(1), PurposeRoot::Spending, true),
    ("fitness", Some(1), PurposeRoot::Spending, false),
    ("business-travel", Some(1), PurposeRoot::Spending, false),
    ("improvement", Some(2), PurposeRoot::Capital, true),
    ("purchase", Some(2), PurposeRoot::Capital, true),
    ("repair", Some(1), PurposeRoot::Spending, true),
    ("licensing", Some(0), PurposeRoot::Income, false),
];

/// The contracts, in the order the book keeps them.
const CONTRACTS: [&str; 6] = ["flat", "phone", "job", "lease", "gym", "mortgage"];

/// What the book names, and the journal being written.
struct Sam {
    names: Interner<'static>,
    usd: Id<Commodity>,
    thing: Id<Kind>,
    entities: Vec<Id<Entity>>,
    places: Vec<Id<Place>>,
    purposes: Vec<Id<Purpose>>,
    flows: Arena<Flow>,
    txns: Arena<Txn>,
    posted: Vec<Posted>,
    promises: Vec<Promise>,
}

impl Sam {
    fn entity(&self, name: &str) -> Id<Entity> {
        self.entities[ENTITIES.iter().position(|&other| other == name).expect("an entity of the cast")]
    }

    fn place(&self, name: &str) -> Id<Place> {
        self.places[PLACES.iter().position(|&(other, ..)| other == name).expect("a place of the cast")]
    }

    fn purpose(&self, name: &str) -> Id<Purpose> {
        self.purposes[PURPOSES.iter().position(|&(other, ..)| other == name).expect("a purpose of the cast")]
    }

    /// A word the fixture interned at the start: a description, a doc, a code.
    fn say(&self, text: &str) -> Sym {
        self.names.get(text).expect("a word the fixture interned")
    }

    fn contract(name: &str) -> Id<Contract> {
        Id::new(CONTRACTS.iter().position(|&other| other == name).expect("a contract of the cast") as u32)
    }

    /// A flow of `amount` cents from one place to another, borne by `owner`,
    /// written in the next transaction: with no purpose, description or codes yet.
    fn flow(&self, when: Day, from: &str, to: &str, amount: i64, owner: &str) -> Flow {
        let money = Amount::new(Qty(amount), self.usd);
        Flow {
            day: when,
            recognized: Days::on(when),
            from: self.place(from),
            to: self.place(to),
            out: money,
            arrive: money,
            mode: Mode::Actual,
            infer: Infer::Known,
            txn: Id::new(self.txns.len() as u32),
            payee: None,
            owner: self.entity(owner),
            purpose: None,
            description: None,
            origin: Origin::Written,
            select: Box::default(),
            codes: Box::default(),
            loc: line(self.txns.len() as u32 + 1),
            waive: None,
            detail: None,
        }
    }

    /// `flow` for `purpose`, said by `source`.
    fn for_(&self, mut flow: Flow, purpose: &str, of: Option<Object>, source: Provenance) -> Flow {
        flow.purpose = Some(Purposed { purpose: self.purpose(purpose), of, source });
        flow
    }

    /// Writes these flows as one transaction, and returns where they start.
    fn write(&mut self, flows: Vec<Flow>, contract: Option<&str>, doc: Option<&'static str>) -> Id<Flow> {
        let first = Id::new(self.flows.len() as u32);
        let (when, codes) = (flows[0].day, flows[0].codes.clone());
        let count = flows.len() as u32;
        for flow in flows {
            let state = if flow.mode == Mode::Pending { State::Pending } else { State::Actual };
            self.posted.push(Posted { out: flow.out.qty, arrive: flow.arrive.qty, state });
            self.flows.push(flow);
        }
        let doc = doc.map(|doc| self.names.intern(doc));
        let loc = line(self.txns.len() as u32 + 1);
        let contract = contract.map(Sam::contract);
        self.txns.push(Txn {
            day: when,
            flows: Ids::new(first, count),
            codes,
            waive: None,
            plan: None,
            contract,
            ends: false,
            doc,
            loc,
        });
        first
    }

    /// An occurrence of a contract written on `kept`, for what fell due on `due`.
    fn occurrence(&mut self, contract: &str, due: Day, kept: Day, flows: Vec<Flow>) {
        let txn = Id::new(self.txns.len() as u32);
        let occurrence = Origin::Occurrence(Sam::contract(contract));
        // What the occurrence derives (a share, a principal) keeps saying so.
        let flows = flows.into_iter().map(|flow| match flow.origin {
            Origin::Derived(_) => flow,
            _ => Flow { origin: occurrence, ..flow },
        });
        let flows = flows.collect();
        self.write(flows, Some(contract), None);
        self.promises.push(Promise { contract: Sam::contract(contract), due, kept: Some((kept, txn)) });
    }
}

/// `12%`, as a share of what a contract's flows are, for an owner.
fn share(percent: i128, entity: Id<Entity>) -> Share {
    Share { rate: Ratio::percent(percent, 0).unwrap(), entity, measure: None, loc: line(2) }
}

pub(crate) fn sam() -> Household {
    let mut names = Interner::default();
    for word in CONTRACTS.iter().chain(&WORDS) {
        names.intern(word);
    }
    let (kinds, kind_ids) = Tree::build(vec![kind(names.intern("thing"))], &[None]).unwrap();
    let thing = kind_ids[0];
    let mut commodities = Arena::new();
    let [usd, hr, mi, condo_unit, car_unit] =
        [("USD", 2), ("HR", 1), ("MI", 0), ("CONDO", 0), ("CAR", 0)].map(|(symbol, scale)| {
            commodities.push(Commodity {
                symbol: names.intern(symbol),
                kind: thing,
                scale,
                title: None,
                liquidity: None,
                select: None,
                growth: None,
                props: Box::default(),
                doc: None,
                loc: None,
            })
        });

    let entities = ENTITIES.map(|name| Entity {
        path: names.intern(name),
        kind: thing,
        place: None,
        restricted: false,
        lives: Box::default(),
        member: None,
        owner: (name == "studio").then(|| Id::new(0)),
        client_of: None,
        owned_by: Box::default(),
        currency: usd,
        citizen: Box::default(),
        books: Books::Cash,
        known_as: Box::default(),
        props: Box::default(),
        doc: None,
        loc: None,
    });
    let (mut entity_tree, entity_ids) = Tree::build(entities.into(), &[None; 16]).unwrap();
    let entity = |name: &str| entity_ids[ENTITIES.iter().position(|&other| other == name).unwrap()];

    let places = PLACES.map(|(name, class, owner)| Place {
        path: names.intern(name),
        class,
        role: match name {
            "me" => Role::Holding(entity("me")),
            "studio" => Role::Holding(entity("studio")),
            "checking" | "visa" | "studio-checking" => Role::Account { institution: Some(entity("chase")) },
            "condo" => Role::Asset(Id::new(0)),
            "car" => Role::Asset(Id::new(1)),
            "unknown" | "opening" => Role::Outside(None),
            _ if name.ends_with("-tab") => Role::Tab(entity(name.trim_end_matches("-tab"))),
            _ => Role::Outside(Some(entity(name))),
        },
        kind: thing,
        owner: entity(owner),
        holds: None,
        select: None,
        deferred: false,
        basis: Basis::Cost,
        claim: name.ends_with("-tab") && class == Class::Asset,
        liquidity: None,
        opened: None,
        closed: None,
        shares: Box::default(),
        known_as: Box::default(),
        props: Box::default(),
        doc: None,
        loc: Some(line(1)),
    });
    let (place_tree, place_ids) = Tree::build(places.into(), &[None; 27]).unwrap();
    for (name, id) in ENTITIES.iter().zip(&entity_ids) {
        let at = PLACES.iter().position(|&(other, ..)| other == *name);
        entity_tree[*id].place = at.map(|at| place_ids[at]);
    }

    let purposes = PURPOSES.map(|(name, _, root, takes)| Purpose {
        name: names.intern(name),
        root,
        system: None,
        of: takes.then_some(thing),
        shares: Box::default(),
        laws: Box::default(),
        doc: None,
        loc: None,
    });
    let parents: Vec<Option<usize>> = PURPOSES.iter().map(|&(_, parent, ..)| parent).collect();
    let (purpose_tree, purpose_ids) = Tree::build(purposes.into(), &parents).unwrap();

    let mut sam = Sam {
        names,
        usd,
        thing,
        entities: entity_ids.clone(),
        places: place_ids.clone(),
        purposes: purpose_ids.clone(),
        flows: Arena::new(),
        txns: Arena::new(),
        posted: Vec::new(),
        promises: Vec::new(),
    };
    let (condo, car) = (Id::new(0), Id::new(1));
    let assets: Arena<Asset> = [("condo", "condo", condo_unit), ("car", "car", car_unit)]
        .into_iter()
        .map(|(name, place, unit)| Asset {
            name: sam.names.intern(name),
            kind: thing,
            owner: sam.entity("me"),
            place: sam.place(place),
            unit,
            part_of: None,
            props: Box::default(),
            doc: None,
            loc: line(3),
        })
        .fold(Arena::new(), |mut arena, asset| {
            arena.push(asset);
            arena
        });

    let contracts = contracts(&sam);
    let (purchase, improvement) = journal(&mut sam, condo);
    finish(
        sam,
        entity_tree,
        place_tree,
        purpose_tree,
        kinds,
        commodities,
        assets,
        contracts,
        [hr, mi],
        (car, purchase, improvement),
    )
}

/// What a contract's occurrence is, for the terms that promise it.
fn template(sam: &Sam, from: &str, to: &str, amount: i64, purpose: &str, contract: &str) -> Flow {
    let mut flow = sam.flow(day(2026, 1, 1), from, to, amount, "me");
    flow.mode = Mode::Planned;
    let source = Provenance::Contract(Sam::contract(contract));
    sam.for_(flow, purpose, None, source)
}

fn terms(on: u8, flows: Vec<Flow>, shares: Box<[Share]>, change: Option<Change>) -> Terms {
    Terms {
        every: Cadence::Every(Span::months(1)),
        on: Box::new([On::MonthDay(on)]),
        anchor: day(2026, 1, 1),
        template: flows.into(),
        inputs: Box::default(),
        estimate: false,
        due: None,
        grace: Span::days(15),
        period: None,
        covers: None,
        prorated: false,
        escalation: None,
        shares,
        also: Box::default(),
        rate: None,
        change,
    }
}

/// Flat, phone, job, lease, gym and mortgage: what Sam has promised and been promised.
fn contracts(sam: &Sam) -> Arena<Contract> {
    let (me, studio) = (sam.entity("me"), sam.entity("studio"));
    let change = |from: Day, until: Day, why: &str, n: u32| Change {
        days: Days::new(from, until).unwrap(),
        description: Some(sam.say(why)),
        code: None,
        loc: line(n),
    };
    let forever = Day::MAX;
    let mut arena = Arena::new();
    let mut make = |name: &str, party: &str, terms: Timeline<Terms>, loan: Option<Loan>, from: Day| {
        arena.push(Contract {
            name: sam.say(name),
            party: sam.entity(party),
            owner: me,
            days: Days::new(from, forever).unwrap(),
            terms,
            buys: None,
            deposit: None,
            loan,
            matching: None,
            ended: None,
            laws: Box::default(),
            doc: None,
            loc: line(10),
        })
    };

    let mut flat = Timeline::new(terms(
        1,
        vec![template(sam, "checking", "greystar", 290_000, "rent", "flat")],
        Box::new([share(12, studio)]),
        None,
    ));
    let renewed = terms(
        1,
        vec![template(sam, "checking", "greystar", 305_000, "rent", "flat")],
        Box::new([share(12, studio)]),
        Some(change(day(2026, 7, 1), forever, "renewed at 3,050", 11)),
    );
    flat.paint(Days::new(day(2026, 7, 1), forever).unwrap(), renewed);
    make("flat", "greystar", flat, None, day(2026, 1, 1));

    let phone =
        terms(8, vec![template(sam, "visa", "mint", 4_500, "phone", "phone")], Box::new([share(60, studio)]), None);
    make("phone", "mint", Timeline::new(phone), None, day(2026, 1, 1));
    let job = terms(3, vec![template(sam, "lumen", "checking", 460_000, "wages", "job")], Box::default(), None);
    make("job", "lumen", Timeline::new(job), None, day(2026, 1, 1));
    let lease =
        terms(1, vec![template(sam, "dana", "checking", 235_000, "rent-received", "lease")], Box::default(), None);
    make("lease", "dana", Timeline::new(lease), None, day(2026, 1, 1));

    let regular = terms(1, vec![template(sam, "visa", "fitclub", 24_000, "fitness", "gym")], Box::default(), None);
    let mut gym = Timeline::new(regular.clone());
    let promo = Terms {
        template: vec![template(sam, "visa", "fitclub", 12_000, "fitness", "gym")].into(),
        change: Some(change(day(2026, 3, 1), day(2026, 5, 31), "spring promotion", 12)),
        ..regular
    };
    gym.paint(Days::new(day(2026, 3, 1), day(2026, 5, 31)).unwrap(), promo);
    make("gym", "fitclub", gym, None, day(2026, 1, 1));

    let payment = Terms {
        rate: Some(Ratio::percent(5875, 3).unwrap()),
        ..terms(1, vec![template(sam, "checking", "rocket", 230_290, "interest", "mortgage")], Box::default(), None)
    };
    let loan = Loan {
        principal: Amount::new(Qty(32_000_000), sam.usd),
        on: day(2024, 2, 20),
        term: Span::months(360),
        asset: Some(Id::new(0)),
        debt: sam.place("rocket-tab"),
        resets: None,
        prepay: Prepay::Shortens,
    };
    make("mortgage", "rocket", Timeline::new(payment), Some(loan), day(2026, 3, 1));
    arena
}

/// Words the journal and the laws use, interned before anything borrows the book.
const WORDS: [&str; 14] = [
    "budget",
    "icon set",
    "renewed at 3,050",
    "spring promotion",
    "lent to jo",
    "the flea market",
    "brand refresh",
    "/// The water heater's invoice.",
    "/// The March invoice.",
    "inv-12",
    "wages",
    "tax-withheld",
    "depreciation",
    "us",
];

impl Sam {
    /// `flow`, paid to `payee`, for `purpose` as `source` says.
    fn paid(&self, flow: Flow, payee: &str, purpose: &str, of: Option<Object>, source: Provenance) -> Flow {
        let flow = Flow { payee: Some(self.entity(payee)), ..flow };
        self.for_(flow, purpose, of, source)
    }

    /// `flow` as an owner's share of what was paid, declared by `sharer`.
    fn shared(&self, flow: Flow, sharer: Sharer) -> Flow {
        let derived = Origin::Derived(Derivation::Share(sharer));
        let mut flow = Flow { origin: derived, ..flow };
        flow.purpose = flow.purpose.map(|purposed| Purposed { source: Provenance::Derived, ..purposed });
        flow
    }
}

/// Sam's journal to 2026-03-05: the opening, the condo, three months of the
/// contracts, and the one-offs. Returns the flows that made the condo's parts.
fn journal(sam: &mut Sam, condo: Id<Asset>) -> (Id<Flow>, Id<Flow>) {
    let object = Some(Object::Asset(condo));
    let mut opening = sam.flow(day(2024, 1, 1), "opening", "checking", 50_000_000, "me");
    opening.mode = Mode::Opening;
    sam.write(vec![opening], None, None);
    let bought = sam.flow(day(2024, 2, 20), "checking", "title-co", 40_200_000, "me");
    let bought = sam.paid(bought, "title-co", "purchase", object, Provenance::Written);
    let purchase = sam.write(vec![bought], None, None);

    let [flat, phone, job, lease, gym, mortgage] = CONTRACTS.map(Sam::contract);
    let via = Provenance::Contract;
    for month in 1..=3 {
        // The flat: 2,552 Sam's, and the studio's 348, twelve percent of the rent.
        let rent = sam.flow(day(2026, month, 1), "checking", "greystar", 255_200, "me");
        let rent = sam.paid(rent, "greystar", "rent", None, via(flat));
        let office = sam.flow(day(2026, month, 1), "checking", "greystar", 34_800, "studio");
        let office = sam.shared(sam.paid(office, "greystar", "rent", None, via(flat)), Sharer::Contract(flat));
        sam.occurrence("flat", day(2026, month, 1), day(2026, month, 1), vec![rent, office]);

        let pay = sam.flow(day(2026, month, 3), "lumen", "checking", 460_000, "me");
        let pay = sam.paid(pay, "lumen", "wages", None, via(job));
        sam.occurrence("job", day(2026, month, 3), day(2026, month, 3), vec![pay]);

        let fee = sam.flow(day(2026, month, 1), "visa", "fitclub", if month == 3 { 12_000 } else { 24_000 }, "me");
        let fee = sam.paid(fee, "fitclub", "fitness", None, via(gym));
        sam.occurrence("gym", day(2026, month, 1), day(2026, month, 1), vec![fee]);

        if month < 3 {
            let mine = sam.flow(day(2026, month, 8), "visa", "mint", 1_800, "me");
            let mine = sam.paid(mine, "mint", "phone", None, via(phone));
            let studios = sam.flow(day(2026, month, 8), "visa", "mint", 2_700, "studio");
            let studios = sam.shared(sam.paid(studios, "mint", "phone", None, via(phone)), Sharer::Contract(phone));
            sam.occurrence("phone", day(2026, month, 8), day(2026, month, 8), vec![mine, studios]);
            // Dana pays on the third, two days after the rent falls due; March's is not written.
            let paid = sam.flow(day(2026, month, 3), "dana", "checking", 235_000, "me");
            let paid = sam.paid(paid, "dana", "rent-received", None, via(lease));
            sam.occurrence("lease", day(2026, month, 1), day(2026, month, 3), vec![paid]);
        }
    }
    sam.promises.push(Promise { contract: lease, due: day(2026, 3, 1), kept: None });
    // The mortgage's March payment: 1,527.88 of interest on the condo, and 365.02 of principal.
    let interest = sam.flow(day(2026, 3, 1), "checking", "rocket", 152_788, "me");
    let interest = sam.paid(interest, "rocket", "interest", object, via(mortgage));
    let mut principal = sam.flow(day(2026, 3, 1), "checking", "rocket-tab", 36_502, "me");
    principal.origin = Origin::Derived(Derivation::Principal(mortgage));
    sam.occurrence("mortgage", day(2026, 3, 1), day(2026, 3, 1), vec![interest, principal]);

    let groceries = sam.flow(day(2026, 1, 6), "visa", "trader-joes", 8_420, "me");
    let groceries = sam.paid(groceries, "trader-joes", "groceries", None, Provenance::Party(sam.thing));
    sam.write(vec![groceries], None, None);
    // Lent to Jo, due April 1st; she paid back 200 of it.
    let mut lent = sam.flow(day(2026, 1, 12), "checking", "jo-tab", 60_000, "me");
    lent.payee = Some(sam.entity("jo"));
    lent.description = Some(sam.say("lent to jo"));
    lent.detail = Some(Box::new(Detail { due: Some(day(2026, 4, 1)), ..Detail::default() }));
    sam.write(vec![lent], None, None);
    let mut market = sam.flow(day(2026, 1, 20), "checking", "unknown", 4_000, "me");
    market.description = Some(sam.say("the flea market"));
    sam.write(vec![market], None, None);
    let heater = sam.flow(day(2026, 2, 2), "checking", "bay-plumbing", 148_000, "me");
    let heater = sam.paid(heater, "bay-plumbing", "improvement", object, Provenance::Written);
    let improvement = sam.write(vec![heater], None, Some("/// The water heater's invoice."));
    let mut back = sam.flow(day(2026, 2, 10), "jo-tab", "checking", 20_000, "me");
    back.payee = Some(sam.entity("jo"));
    sam.write(vec![back], None, None);
    // The studio's invoice to Halcyon, which the hours of `inv-12` bill: two
    // items, each for its own purpose.
    let due = Some(Box::new(Detail { due: Some(day(2026, 4, 1)), ..Detail::default() }));
    let item = |amount, purpose, words| {
        let item = sam.flow(day(2026, 3, 2), "halcyon", "halcyon-tab", amount, "studio");
        let item = Flow { description: Some(sam.say(words)), codes: Box::new([sam.say("inv-12")]), ..item };
        Flow { detail: due.clone(), ..sam.paid(item, "halcyon", purpose, None, Provenance::Written) }
    };
    let items = vec![item(300_000, "design", "brand refresh"), item(80_000, "licensing", "icon set")];
    sam.write(items, None, Some("/// The March invoice."));
    (purchase, improvement)
}

/// The book and its run, from what was made.
#[allow(clippy::too_many_arguments)]
fn finish(
    mut sam: Sam,
    entities: Tree<Entity>,
    places: Tree<Place>,
    purposes: Tree<Purpose>,
    kinds: Tree<Kind>,
    commodities: Arena<Commodity>,
    assets: Arena<Asset>,
    contracts: Arena<Contract>,
    [hr, mi]: [Id<Commodity>; 2],
    (car, purchase, improvement): (Id<Asset>, Id<Flow>, Id<Flow>),
) -> Household {
    let (me, studio, usd) = (sam.entity("me"), sam.entity("studio"), sam.usd);
    let food = sam.purpose("food");
    let system =
        System { path: sam.say("us"), laws: Box::default(), currency: None, rates: None, doc: None, loc: None };
    let (systems, system_ids) = Tree::build(vec![system], &[None]).unwrap();
    let us = system_ids[0];

    let law = |name: Sym, owner: Owner, budget: Option<Id<Budget>>| Law {
        name,
        doc: None,
        owner,
        system: None,
        trigger: Trigger::Flow,
        budget,
        overrides: None,
        rank: Rank(0),
        steps: Box::default(),
        nodes: Box::default(),
        loc: line(20),
    };
    let mut laws = Arena::new();
    laws.push(law(sam.say("depreciation"), Owner::Asset(Id::new(0)), None));
    let budget_law = laws.push(law(sam.say("budget"), Owner::Purpose(food), Some(Id::new(0))));
    let wages_law = laws.push(Law { system: Some(us), ..law(sam.say("wages"), Owner::System(us), None) });

    let mut limits = Timeline::new(Limit::Amount(Amount::new(Qty(90_000), usd)));
    limits.paint(Days::new(day(2026, 3, 1), day(2026, 3, 31)).unwrap(), Limit::Amount(Amount::new(Qty(120_000), usd)));
    let mut budgets = Arena::new();
    budgets.push(Budget {
        purpose: food,
        period: Period::Month,
        limits,
        carries: true,
        law: budget_law,
        funded: None,
        loc: line(21),
    });

    let measure = |when, action, subject, qty, unit, purpose: &str, code: Option<&str>| Measure {
        day: when,
        action,
        subject,
        quantity: Amount::new(Qty(qty), unit),
        owner: studio,
        party: Some(sam.entity("halcyon")),
        purpose: Some(Purposed { purpose: sam.purpose(purpose), of: None, source: Provenance::Written }),
        description: None,
        codes: code.map(|code| sam.say(code)).into_iter().collect(),
        loc: line(30),
    };
    let mut measures = Arena::new();
    measures.push(measure(day(2026, 1, 12), Action::Work, Subject::Entity(me), 65, hr, "design", Some("inv-12")));
    measures.push(measure(day(2026, 1, 21), Action::Use, Subject::Asset(car), 44, mi, "business-travel", None));
    measures.push(measure(day(2026, 2, 5), Action::Work, Subject::Entity(me), 30, hr, "design", None));

    let filed = vec![Filed {
        day: day(2026, 2, 20),
        system: us,
        year: 2025,
        owner: me,
        lines: Box::new([
            (sam.say("wages"), Amount::new(Qty(5_400_000), usd), line(40)),
            (sam.say("tax-withheld"), Amount::new(Qty(1_195_200), usd), line(41)),
        ]),
        loc: line(39),
    }];
    let tally = |name: &str, qty: i64| Effect {
        law: wages_law,
        subject: Subject::Entity(me),
        owner: me,
        system: Some(us),
        day: day(2025, 12, 31),
        name: sam.say(name),
        amount: Amount::new(Qty(qty), usd),
        owe: None,
        cause: Cause::Time,
        priced: false,
    };
    let effects = vec![tally("wages", 5_500_000), tally("tax-withheld", 1_195_200)];

    let touching =
        Groups::build(places.len(), sam.flows.iter().flat_map(|(id, flow)| [(flow.from, id), (flow.to, id)]));
    let holdings = holdings(&sam);
    let part = |flow, when, cost, basis| Part { flow, day: when, cost: Qty(cost), basis: Qty(basis) };
    let bought = part(purchase, day(2024, 2, 20), 40_200_000, 40_200_000 - 1_837_273);
    let improved = part(improvement, day(2026, 2, 2), 148_000, 148_000 - 673);
    let assets_state = vec![AssetState { asset: Id::new(0), parts: vec![bought, improved], disposed: None }];
    let consumed = |part, qty| Adjustment {
        day: day(2026, 2, 28),
        law: Id::new(0),
        kind: AdjustmentKind::Consumed { asset: Id::new(0), part },
        amount: Qty(qty),
    };
    let adjustments = vec![consumed(0, 1_837_273), consumed(1, 673)];

    let run = Run {
        today: day(2026, 3, 5),
        horizon: day(2026, 3, 5),
        posted: sam.posted.clone().into(),
        holdings,
        gains: Vec::new(),
        effects,
        violations: Vec::new(),
        headroom: Vec::new(),
        pads: Vec::new(),
        assets: assets_state,
        promises: sam.promises.clone(),
        adjustments,
        checks: vec![0; laws.len()].into(),
        diagnostics: Vec::new(),
    };
    let _ = car;
    let roots = Roots {
        me,
        unknown: sam.place("unknown"),
        opening: sam.place("opening"),
        market: sam.entity("market"),
        asset: Id::new(0),
        debt: Id::new(0),
        thing: Id::new(0),
        commodity: Id::new(0),
        entity: Id::new(0),
        income: sam.purposes[0],
        spending: sam.purposes[1],
        capital: sam.purposes[2],
    };
    let (names, flows, txns) = (std::mem::take(&mut sam.names), sam.flows, sam.txns);
    let book = Book {
        names,
        base: usd,
        relaxed: false,
        roots,
        places,
        entities,
        kinds,
        purposes,
        systems,
        commodities,
        assets,
        contracts,
        also: Arena::new(),
        laws,
        rules: Rules::default(),
        budgets,
        params: Arena::new(),
        schedules: Arena::new(),
        codes: Vec::new(),
        patterns: Arena::new(),
        formats: Arena::new(),
        txns,
        flows,
        touching,
        asserts: Vec::new(),
        events: Vec::new(),
        prices: Prices::default(),
        splits: Vec::new(),
        measures,
        readings: Vec::new(),
        filed,
        plans: Arena::new(),
        sources: Vec::new(),
        lookup: Default::default(),
    };
    Household { book, run }
}

/// What every place holds at the end: what flowed in less what flowed out,
/// with each claim its own parcel, and the mortgage's balance as it stands.
fn holdings(sam: &Sam) -> Vec<Holding> {
    let mut plain: std::collections::BTreeMap<Id<Place>, i64> = std::collections::BTreeMap::new();
    for (flow, posted) in sam.flows.values().zip(&sam.posted) {
        *plain.entry(flow.from).or_default() -= posted.out.0;
        *plain.entry(flow.to).or_default() += posted.arrive.0;
    }
    plain.insert(sam.place("rocket-tab"), -31_097_817);
    // Each claim is a parcel that remembers the transaction that made it.
    let made = |place: Id<Place>| {
        let makes = |txn: &Txn| sam.flows[txn.flows].iter().any(|flow| flow.to == place);
        sam.txns.iter().find(|(_, txn)| makes(txn)).map(|(id, txn)| (id, txn.day))
    };
    let tabs = [sam.place("jo-tab"), sam.place("halcyon-tab")];
    plain
        .into_iter()
        .map(|(place, qty)| {
            let claim = made(place).filter(|_| tabs.contains(&place));
            let parcel = |(txn, acquired)| Parcel { qty: Qty(qty), basis: Qty(qty), acquired, txn, tied: None };
            let lots: Vec<Parcel> = claim.map(parcel).into_iter().collect();
            Holding { place, unit: sam.usd, plain: if lots.is_empty() { Qty(qty) } else { Qty::ZERO }, lots }
        })
        .collect()
}

#[cfg(test)]
mod views {
    use super::*;
    use crate::tests::{Household, lines};
    use crate::{Group, Query};

    /// The rows of the section headed `heading` in a view of Sam.
    fn rows(house: &Household, query: Query, heading: &str) -> Vec<String> {
        let report = house.report(query);
        let section = report.sections.iter().find(|section| section.heading.unwrap_or("") == heading);
        lines(section.unwrap_or_else(|| panic!("no section headed {heading:?}")))
    }

    #[test]
    fn contracts_show_terms_next_due_what_is_late_and_what_a_loan_still_owes() {
        let rows = rows(&sam(), Query::Contracts { at: None }, "");
        assert_eq!(
            rows,
            [
                "flat | greystar | 2,900.00 USD monthly from checking until 2026-06-30, then 3,050.00 USD monthly from checking: renewed at 3,050 | 2026-04-01 | 3 |  |  |",
                "phone | mint | 45.00 USD monthly from visa | 2026-03-08 | 2 |  |  |",
                "job | lumen | 4,600.00 USD monthly into checking | 2026-04-03 | 3 |  |  |",
                "!lease | dana | 2,350.00 USD monthly into checking | 2026-04-01 | 2 | 4d | dana |",
                "gym | fitclub | 120.00 USD monthly from visa until 2026-05-31, then 240.00 USD monthly from visa: spring promotion | 2026-04-01 | 3 |  |  |",
                "mortgage | rocket | 2,302.90 USD monthly from checking | 2026-04-01 | 1 |  |  | 310,978.17 USD",
            ]
        );
    }

    #[test]
    fn a_contract_is_late_only_from_its_due_day_and_only_while_unkept() {
        let sam = sam();
        // The day before March's rent falls due nothing is late, and a month on it is late a month and three days.
        let before = rows(&sam, Query::Contracts { at: Some(day(2026, 2, 28)) }, "");
        assert!(before.iter().all(|row| !row.starts_with('!')), "{before:?}");
        let after = rows(&sam, Query::Contracts { at: Some(day(2026, 4, 4)) }, "");
        assert!(after.iter().any(|row| row.starts_with("!lease") && row.contains("| 1m3d | dana |")), "{after:?}");
    }

    #[test]
    fn a_loan_contract_reports_its_balance_as_a_fact_of_its_owner() {
        let report = sam().report(Query::Contracts { at: None });
        let owed: Vec<_> = report.sections[0].facts.iter().filter(|fact| fact.concept == "owed").collect();
        assert_eq!(owed.len(), 1);
        assert_eq!((owed[0].of, owed[0].entity), (Some("rocket-tab"), "me"));
        assert_eq!(owed[0].value.qty, Qty(31_097_817));
    }

    #[test]
    fn contracts_are_about_whose_they_are() {
        let sam = sam();
        let studio = sam.report_for(Query::Contracts { at: None }, Some("studio")).unwrap();
        assert!(studio.sections[0].rows.is_empty());
        assert_eq!(studio.sections[0].notes.len(), 1);
    }

    #[test]
    fn claims_list_the_promises_that_are_late_and_an_itemized_claim_its_items() {
        let sam = sam();
        assert_eq!(
            rows(&sam, Query::Claims { at: None }, "Late promises"),
            ["!lease | dana | 2026-03-01 | 4d | dana | 2,350.00 USD"]
        );
        let owed = rows(&sam, Query::Claims { at: None }, "Owed to you");
        assert_eq!(owed.len(), 5, "{owed:?}");
        assert!(owed[0].starts_with("jo | ") && owed[0].contains("| 400.00 USD |"), "{owed:?}");
        assert!(owed[1].starts_with("halcyon | #inv-12") && owed[1].contains("| 3,800.00 USD |"), "{owed:?}");
        assert_eq!(&owed[2..4], ["~   | design | 3,000.00 USD |  |  |  |", "~   | licensing | 800.00 USD |  |  |  |"]);
        assert_eq!(owed[4], "=Total |  | 4,200.00 USD |  |  |  |");
    }

    #[test]
    fn available_spends_what_is_promised_and_counts_late_promises_as_coming_in() {
        let sam = sam();
        let spendable = rows(&sam, Query::Available { at: None }, "What you can spend");
        assert_eq!(spendable[0], "Money in hand | 103,987.10 USD");
        assert_eq!(spendable[2], "Due within 30 days | -5,367.90 USD");
        // Each promise that falls due within a month is spoken for, with the day.
        assert_eq!(
            spendable[3..7],
            [
                "~  phone, to mint by 2026-03-08 | -45.00 USD",
                "~  flat, to greystar by 2026-04-01 | -2,900.00 USD",
                "~  gym, to fitclub by 2026-04-01 | -120.00 USD",
                "~  mortgage, to rocket by 2026-04-01 | -2,302.90 USD",
            ]
        );
        assert_eq!(spendable[7], "=Available to spend | 98,619.20 USD");
        assert_eq!(
            rows(&sam, Query::Available { at: None }, "Late promises coming in"),
            ["!lease | dana | 2026-03-01 | 4d | dana | 2,350.00 USD"]
        );
    }

    /// The first section of Sam's `flow` over the first quarter of 2026, for whom (everyone by default).
    fn flow(group: Group, whose: Option<&str>) -> Vec<String> {
        let query = Query::Flow { by: Period::Month, group, from: Some(day(2026, 1, 1)), to: None };
        lines(&sam().report_for(query, whose).unwrap().sections[0])
    }

    #[test]
    fn flow_reads_income_spending_and_capital_from_the_purpose_tree() {
        assert_eq!(
            flow(Group::Purpose, None),
            [
                "=income | 6,950.00 USD | 6,950.00 USD | 8,400.00 USD | 22,300.00 USD",
                "  wages | 4,600.00 USD | 4,600.00 USD | 4,600.00 USD | 13,800.00 USD",
                "  design |  |  | 3,000.00 USD | 3,000.00 USD",
                "  rent-received | 2,350.00 USD | 2,350.00 USD |  | 4,700.00 USD",
                "  licensing |  |  | 800.00 USD | 800.00 USD",
                "=spending | 3,309.20 USD | 3,185.00 USD | 4,547.88 USD | 11,042.08 USD",
                "  food | 84.20 USD |  |  | 84.20 USD",
                "    groceries | 84.20 USD |  |  | 84.20 USD",
                "  rent | 2,900.00 USD | 2,900.00 USD | 2,900.00 USD | 8,700.00 USD",
                "  phone | 45.00 USD | 45.00 USD |  | 90.00 USD",
                "  interest |  |  | 1,527.88 USD | 1,527.88 USD",
                "~    of condo |  |  | 1,527.88 USD | 1,527.88 USD",
                "  fitness | 240.00 USD | 240.00 USD | 120.00 USD | 600.00 USD",
                "  the flea market | 40.00 USD |  |  | 40.00 USD",
                "=Net | 3,640.80 USD | 3,765.00 USD | 3,852.12 USD | 11,257.92 USD",
                "=capital |  | 1,480.00 USD |  | 1,480.00 USD",
                "  improvement |  | 1,480.00 USD |  | 1,480.00 USD",
                "~    of condo |  | 1,480.00 USD |  | 1,480.00 USD",
            ]
        );
    }

    #[test]
    fn a_shared_flow_shows_each_owners_share_under_the_owner_and_the_whole_under_everyone() {
        let rent = |rows: &[String]| rows.iter().find(|row| row.starts_with("  rent |")).cloned().unwrap();
        assert_eq!(
            rent(&flow(Group::Purpose, None)),
            "  rent | 2,900.00 USD | 2,900.00 USD | 2,900.00 USD | 8,700.00 USD"
        );
        assert_eq!(
            rent(&flow(Group::Purpose, Some("me"))),
            "  rent | 2,552.00 USD | 2,552.00 USD | 2,552.00 USD | 7,656.00 USD"
        );
        assert_eq!(
            rent(&flow(Group::Purpose, Some("studio"))),
            "  rent | 348.00 USD | 348.00 USD | 348.00 USD | 1,044.00 USD"
        );
        // The studio bears its invoice's items, each under its own purpose, and none of Sam's pay.
        let studio = flow(Group::Purpose, Some("studio"));
        assert!(studio.iter().any(|row| row.starts_with("  licensing |  |  | 800.00 USD")), "{studio:?}");
        assert!(studio.iter().all(|row| !row.starts_with("  wages")), "{studio:?}");
    }

    #[test]
    fn flow_by_party_lists_who_the_money_went_to_and_came_from_biggest_first() {
        let rows = flow(Group::Party, None);
        let names: Vec<&str> =
            rows.iter().map(|row| row.split(" | ").next().unwrap().trim_start_matches(['=', ' '])).collect();
        assert_eq!(
            names,
            [
                "income",
                "lumen",
                "dana",
                "halcyon",
                "spending",
                "greystar",
                "rocket",
                "fitclub",
                "mint",
                "trader-joes",
                "unknown",
                "Net",
                "capital",
                "bay-plumbing"
            ]
        );
        assert_eq!(rows[1], "  lumen | 4,600.00 USD | 4,600.00 USD | 4,600.00 USD | 13,800.00 USD");
    }

    #[test]
    fn flow_counts_hours_and_miles_by_purpose_and_owner_in_their_own_units() {
        let query = Query::Flow { by: Period::Month, group: Group::Purpose, from: Some(day(2026, 1, 1)), to: None };
        let report = sam().report(query);
        assert_eq!(
            lines(&report.sections[1]),
            ["design for studio | 6.5 HR | 3.0 HR |  | 9.5 HR", "business-travel for studio | 44 MI |  |  | 44 MI"]
        );
        let facts: Vec<_> = report.sections[1].facts.iter().map(|fact| (fact.concept, fact.of, fact.entity)).collect();
        assert_eq!(facts[0], ("worked", Some("design"), "studio"));
        assert!(facts.iter().any(|&(concept, ..)| concept == "used"));
    }

    #[test]
    fn flow_facts_say_what_was_earned_or_spent_of_which_purpose() {
        let query = Query::Flow { by: Period::Month, group: Group::Purpose, from: Some(day(2026, 1, 1)), to: None };
        let report = sam().report_for(query, Some("studio")).unwrap();
        let rent: Vec<_> = report.sections[0].facts.iter().filter(|fact| fact.of == Some("rent")).collect();
        assert_eq!(rent.len(), 3);
        assert!(
            rent.iter()
                .all(|fact| fact.concept == "spending" && fact.entity == "studio" && fact.value.qty == Qty(34_800))
        );
    }
}
