//! Test books built by hand, so the engine can be exercised without the model
//! lane's `build`.
//!
//! A fixed set of places, entities and commodities; flows, assertions, events,
//! laws and rules are added by the test. Flows must be added in day order (the
//! book's contract), and `book()` assembles the tables the engine reads.

use axiom_core::{Arena, Day, Days, FileId, Groups, Id, Interner, Loc, Qty, Ratio, Run, Severity, Sym, Tree};
use axiom_model::*;

/// The days from `first` to `last`, as day numbers.
pub(crate) fn span(first: i32, last: i32) -> Days {
    Days::new(Day(first), Day(last)).expect("a span that ends no earlier than it begins")
}

pub(crate) struct Fixture {
    pub names: Interner<'static>,
    pub usd: Id<Commodity>,
    pub vti: Id<Commodity>,
    pub me: Id<Entity>,
    pub grant: Id<Entity>,
    pub household: Id<Entity>,
    unknown_entity: Id<Entity>,
    opening_entity: Id<Entity>,
    /// The market, whose place is `market`.
    pub trader: Id<Entity>,
    pub assets: Id<Place>,
    pub checking: Id<Place>,
    pub savings: Id<Place>,
    pub cash: Id<Place>,
    pub brokerage: Id<Place>,
    pub retirement: Id<Place>,
    pub salary: Id<Place>,
    pub grants: Id<Place>,
    pub food: Id<Place>,
    pub equity: Id<Place>,
    pub unknown: Id<Place>,
    pub opening: Id<Place>,
    pub card: Id<Place>,
    /// The market's place: revaluations come from and go to it.
    pub market: Id<Place>,
    pub places: Tree<Place>,
    pub entities: Tree<Entity>,
    commodities: Arena<Commodity>,
    pub flows: Vec<Flow>,
    pub txns: Vec<Txn>,
    codes: Arena<Sym>,
    selectors: Arena<Select>,
    details: Arena<Detail>,
    pub asserts: Vec<Assert>,
    pub events: Vec<Event>,
    pub splits: Vec<Split>,
    pub laws: Arena<Law>,
    pub on_in: Vec<(Id<Place>, Rule)>,
    pub on_out: Vec<(Id<Place>, Rule)>,
    pub on_gain: Vec<(Id<Place>, Rule)>,
    pub always: Vec<(Id<Place>, Rule)>,
    pub on_spend: Vec<(Id<Entity>, Rule)>,
    pub timed: Vec<Rule>,
}

impl Fixture {
    pub fn new() -> Fixture {
        let mut names = Interner::default();
        let kind = Id::new(0);
        let entity = |path: Sym, restricted| Entity {
            path,
            kind,
            purpose: None,
            place: None,
            restricted,
            lives: Box::new([]),
            member: None,
            owner: None,
            client_of: None,
            owned_by: Box::new([]),
            currency: Id::new(0),
            citizen: Box::new([]),
            books: Books::Cash,
            known_as: Box::new([]),
            props: Box::new([]),
            doc: None,
            loc: None,
        };
        let people = vec![
            entity(names.intern("me"), false),
            entity(names.intern("nsf-grant"), true),
            entity(names.intern("household"), false),
            entity(names.intern("market"), false),
            entity(names.intern("unknown"), false),
            entity(names.intern("opening"), false),
        ];
        let (mut entities, ids) = Tree::build(people, &[None; 6]).expect("no cycles");
        let me = ids[0];
        let place = |path: Sym, class| Place {
            path,
            class,
            role: if class == Class::Outside { Role::Outside(None) } else { Role::Account { institution: None } },
            kind,
            owner: me,
            holds: None,
            select: None,
            deferred: false,
            basis: Basis::Cost,
            claim: false,
            liquidity: None,
            opened: None,
            closed: None,
            shares: Box::new([]),
            known_as: Box::new([]),
            props: Box::new([]),
            doc: None,
            loc: None,
        };
        let spec: [(&'static str, Class, Option<usize>); 17] = [
            ("assets", Class::Asset, None),
            ("assets/checking", Class::Asset, Some(0)),
            ("assets/savings", Class::Asset, Some(0)),
            ("assets/cash", Class::Asset, Some(0)),
            ("assets/brokerage", Class::Asset, Some(0)),
            ("assets/retirement", Class::Asset, Some(0)),
            ("income", Class::Outside, None),
            ("income/salary", Class::Outside, Some(6)),
            ("income/grants", Class::Outside, Some(6)),
            ("expenses", Class::Outside, None),
            ("expenses/food", Class::Outside, Some(9)),
            ("equity", Class::Outside, None),
            ("equity/opening", Class::Outside, Some(11)),
            ("equity/unknown", Class::Outside, Some(11)),
            ("liabilities", Class::Debt, None),
            ("liabilities/card", Class::Debt, Some(14)),
            ("income/market", Class::Outside, Some(6)),
        ];
        let items = spec.iter().map(|&(path, class, _)| place(names.intern(path), class)).collect();
        let parents: Vec<_> = spec.iter().map(|&(.., parent)| parent).collect();
        let (mut places, p) = Tree::build(items, &parents).expect("no cycles");
        places[p[5]].deferred = true;
        places[p[5]].basis = Basis::Zero;
        places[p[16]].kind = Id::new(1);
        entities[ids[3]].place = Some(p[16]);
        entities[ids[4]].place = Some(p[13]);
        entities[ids[5]].place = Some(p[12]);
        places[p[13]].owner = ids[4];
        places[p[12]].owner = ids[5];
        let mut commodities = Arena::new();
        let usd = commodities.push(commodity(names.intern("USD"), 2));
        let vti = commodities.push(commodity(names.intern("VTI"), 0));
        Fixture {
            names,
            usd,
            vti,
            me,
            grant: ids[1],
            household: ids[2],
            unknown_entity: ids[4],
            opening_entity: ids[5],
            trader: ids[3],
            assets: p[0],
            checking: p[1],
            savings: p[2],
            cash: p[3],
            brokerage: p[4],
            retirement: p[5],
            salary: p[7],
            grants: p[8],
            food: p[10],
            equity: p[12],
            unknown: p[13],
            opening: p[12],
            card: p[15],
            market: p[16],
            places,
            entities,
            commodities,
            flows: Vec::new(),
            txns: Vec::new(),
            codes: Arena::new(),
            selectors: Arena::new(),
            details: Arena::new(),
            asserts: Vec::new(),
            events: Vec::new(),
            splits: Vec::new(),
            laws: Arena::new(),
            on_in: Vec::new(),
            on_out: Vec::new(),
            on_gain: Vec::new(),
            always: Vec::new(),
            on_spend: Vec::new(),
            timed: Vec::new(),
        }
    }

    pub fn sym(&mut self, text: &'static str) -> Sym {
        self.names.intern(text)
    }

    pub fn usd(&self, cents: i64) -> Amount {
        Amount::new(Qty(cents), self.usd)
    }

    pub fn vti(&self, shares: i64) -> Amount {
        Amount::new(Qty(shares), self.vti)
    }

    /// A USD transfer.
    pub fn flow(&mut self, day: i32, from: Id<Place>, to: Id<Place>, cents: i64) -> Id<Flow> {
        let amount = self.usd(cents);
        self.push(day, from, amount, to, amount, Infer::Known)
    }

    pub fn exchange(&mut self, day: i32, from: Id<Place>, out: Amount, to: Id<Place>, arrive: Amount) -> Id<Flow> {
        self.push(day, from, out, to, arrive, Infer::Known)
    }

    /// Checking buys `shares` of VTI for `cents`.
    pub fn buy(&mut self, day: i32, cents: i64, shares: i64) -> Id<Flow> {
        let (from, to, out, arrive) = (self.checking, self.brokerage, self.usd(cents), self.vti(shares));
        self.exchange(day, from, out, to, arrive)
    }

    /// Checking buys `shares` of VTI for an amount left as `? USD`.
    pub fn unknown_buy(&mut self, day: i32, shares: i64) -> Id<Flow> {
        let id = self.buy(day, 0, shares);
        self.flows[id.index()].infer = Infer::Unknown;
        id
    }

    /// The brokerage sells `shares` of VTI into checking for `cents`.
    pub fn sell(&mut self, day: i32, shares: i64, cents: i64) -> Id<Flow> {
        let (from, to, out, arrive) = (self.brokerage, self.checking, self.vti(shares), self.usd(cents));
        self.exchange(day, from, out, to, arrive)
    }

    /// The brokerage sells everything it holds of VTI for `cents`.
    pub fn sell_all(&mut self, day: i32, cents: i64) -> Id<Flow> {
        let id = self.sell(day, 0, cents);
        self.flows[id.index()].infer = Infer::All;
        id
    }

    /// A USD flow whose `end` is written `= balance`.
    pub fn target_leg(&mut self, day: i32, from: Id<Place>, to: Id<Place>, end: End, balance: i64) -> Id<Flow> {
        let id = self.flow(day, from, to, 0);
        self.flows[id.index()].infer = Infer::Target { end, balance: Qty(balance) };
        id
    }

    /// A transfer of `? USD`.
    pub fn unknown(&mut self, day: i32, from: Id<Place>, to: Id<Place>) -> Id<Flow> {
        let amount = self.usd(0);
        self.push(day, from, amount, to, amount, Infer::Unknown)
    }

    fn push(
        &mut self,
        day: i32,
        from: Id<Place>,
        out: Amount,
        to: Id<Place>,
        arrive: Amount,
        infer: Infer,
    ) -> Id<Flow> {
        assert!(self.flows.last().is_none_or(|last| last.day.0 <= day), "flows are added in day order");
        let id = Id::new(self.flows.len() as u32);
        let loc = Loc::new(FileId(0), id.index() as u32 * 100, id.index() as u32 * 100 + 50);
        let day = Day(day);
        let txn = Txn {
            day,
            flows: Run::new(id, 1),
            inputs: Run::new(Id::new(0), 0),
            program: None,
            codes: Run::new(Id::new(0), 0),
            waive: None,
            contract: None,
            contract_schedule: None,
            occurrence: None,
            kind: axiom_model::journal::TxnKind::Journal,
            doc: None,
            loc,
        };
        self.txns.push(txn);
        let flow = Flow {
            day,
            recognized: Days::on(day),
            from,
            to,
            out,
            arrive,
            mode: Mode::Actual,
            infer,
            txn: Id::new(id.index() as u32),
            payee: None,
            owner: self.me,
            purpose: None,
            description: None,
            origin: Origin::Written,
            select: Run::new(Id::new(0), 0),
            header_codes: Run::new(Id::new(0), 0),
            codes: Run::new(Id::new(0), 0),
            loc,
            waive: None,
            detail: None,
        };
        self.flows.push(flow);
        id
    }

    pub fn assert(&mut self, day: i32, place: Id<Place>, cents: i64) {
        let amount = self.usd(cents);
        let loc =
            Loc::new(FileId(0), 50_000 + self.asserts.len() as u32 * 100, 50_050 + self.asserts.len() as u32 * 100);
        self.asserts.push(Assert {
            day: Day(day),
            place,
            subject: axiom_model::Subject::Place(place),
            amount,
            computed: None,
            gap: Gap::Refused,
            loc,
        });
    }

    /// Marks the last assertion `!`.
    pub fn pad_last(&mut self) {
        let loc = self.asserts.last().expect("an assertion to pad").loc;
        self.asserts.last_mut().expect("an assertion to pad").gap = Gap::Unexplained(Waive { loc, reason: None });
    }

    /// Recognizes a flow over a range of days.
    pub fn recognize(&mut self, id: Id<Flow>, from: i32, until: i32) {
        self.flows[id.index()].recognized = span(from, until);
    }

    /// Gives a flow detail.
    pub fn detail(&mut self, id: Id<Flow>, detail: Detail) {
        let detail = self.details.push(detail);
        self.flows[id.index()].detail = Some(detail);
    }

    pub fn select(&mut self, id: Id<Flow>, selectors: impl IntoIterator<Item = Select>) {
        let first = Id::new(self.selectors.len() as u32);
        let mut len = 0;
        for selector in selectors {
            self.selectors.push(selector);
            len += 1;
        }
        self.flows[id.index()].select = Run::new(first, len);
    }

    /// Makes a flow an `opening` line.
    pub fn opening(&mut self, id: Id<Flow>) {
        self.flows[id.index()].mode = Mode::Opening;
    }

    /// A flow's `!`, on its own leg.
    pub fn waive(&mut self, id: Id<Flow>) {
        let loc = Loc::new(FileId(0), 70_000 + id.index() as u32 * 10, 70_001 + id.index() as u32 * 10);
        self.flows[id.index()].waive = Some(Waive { loc, reason: None });
    }

    /// Asserts a balance of shares of VTI.
    pub fn assert_vti(&mut self, day: i32, place: Id<Place>, shares: i64) {
        let amount = self.vti(shares);
        let loc =
            Loc::new(FileId(0), 55_000 + self.asserts.len() as u32 * 100, 55_050 + self.asserts.len() as u32 * 100);
        self.asserts.push(Assert {
            day: Day(day),
            place,
            subject: axiom_model::Subject::Place(place),
            amount,
            computed: None,
            gap: Gap::Refused,
            loc,
        });
    }

    /// Marks the last assertion `via` a place.
    pub fn via_last(&mut self, counter: Id<Place>) {
        let loc = self.asserts.last().expect("an assertion to book").loc;
        self.asserts.last_mut().expect("an assertion to book").gap = Gap::Via { place: counter, loc };
    }

    /// `DAY UNIT split NEW for OLD`.
    pub fn split(&mut self, day: i32, unit: Id<Commodity>, new: i64, old: i64) {
        let loc = Loc::new(FileId(0), 80_000 + self.splits.len() as u32 * 10, 80_005 + self.splits.len() as u32 * 10);
        self.splits.push(Split {
            day: Day(day),
            unit,
            ratio: Ratio::new(new as i128, old as i128).expect("a ratio"),
            loc,
        });
    }

    /// Makes `me` a member of the household.
    pub fn join_household(&mut self) {
        let (household, me) = (self.household, self.me);
        self.entities[me].member = Some(household);
    }

    /// The flow made a claim, due on `due`, against `payee`.
    pub fn claim(&mut self, id: Id<Flow>, due: i32, payee: Id<Entity>) {
        self.detail(id, Detail { due: Some(Day(due)), ..Detail::default() });
        self.flows[id.index()].payee = Some(payee);
    }

    /// Marks a flow's transaction with `code`, which is what lot selectors read.
    pub fn mark_txn(&mut self, id: Id<Flow>, code: &'static str) {
        let code = self.sym(code);
        let txn = self.flows[id.index()].txn;
        let codes = self.codes.push(code);
        let run = Run::new(codes, 1);
        self.txns[txn.index()].codes = run;
        self.flows[id.index()].header_codes = run;
    }

    /// Marks a flow with `code`.
    pub fn mark(&mut self, id: Id<Flow>, code: &'static str) {
        let code = self.sym(code);
        let start = self.codes.push(code);
        self.flows[id.index()].codes = Run::new(start, 1);
    }

    /// Marks a flow pending under `code`.
    pub fn pending(&mut self, id: Id<Flow>, code: &'static str) {
        self.mark(id, code);
        self.flows[id.index()].mode = Mode::Pending;
    }

    pub fn event(&mut self, day: i32, code: &'static str, state: EventState) {
        let code = self.sym(code);
        self.events.push(Event { day: Day(day), code, state, loc: Loc::new(FileId(0), 60_000, 60_010) });
    }

    pub fn law(&mut self, law: LawBuilder) -> Id<Law> {
        self.laws.push(law.build())
    }

    pub fn property(&mut self, place: Id<Place>, name: Sym, since: i32, value: Value) {
        let mut props = self.places[place].props.to_vec();
        props.push(Prop { name, value, since: Day(since), loc: None });
        props.sort_by_key(|prop| (prop.name, prop.since));
        self.places[place].props = props.into();
    }

    /// A rule that applies for all time.
    pub fn rule(&self, law: Id<Law>, subject: Subject) -> Rule {
        Rule { law, subject, days: Days::ALWAYS }
    }

    pub fn book(mut self) -> Book<'static> {
        let kind_name = self.names.intern("thing");
        let kind = Kind {
            name: kind_name,
            sort: Sort::Place(Class::Asset),
            system: None,
            restricted: false,
            deferred: false,
            basis: None,
            claim: false,
            select: None,
            liquidity: None,
            purpose: None,
            pays: None,
            takes: Box::new([]),
            sales_tax: None,
            shares: Box::new([]),
            has: Box::new([]),
            props: Box::new([]),
            laws: Box::new([]),
            doc: None,
            loc: None,
        };
        let market = Kind { name: self.names.intern("market"), sort: Sort::Place(Class::Outside), ..kind.clone() };
        let (kinds, _) = Tree::build(vec![kind, market], &[None, None]).expect("no cycles");
        let k = Id::new(0);
        let (purposes, [income, spending, capital, transfer]) = Purpose::roots(&mut self.names);
        let roots = Roots {
            me: self.me,
            unknown: self.unknown_entity,
            opening: self.opening_entity,
            market: self.trader,
            kinds: KindRoots { asset: k, debt: k, thing: k, commodity: k, measure: k, entity: k },
            purposes: PurposeRoots { income, spending, capital, transfer },
        };
        let places = self.places.len();
        let ends = |(i, flow): (usize, &Flow)| {
            let id = Id::new(i as u32);
            [Some((flow.from, id)), (flow.to != flow.from).then_some((flow.to, id))]
        };
        let touching = Groups::build(places, self.flows.iter().enumerate().flat_map(ends).flatten());
        let rules = Rules {
            on_in: Groups::build(places, self.on_in.iter().copied()),
            on_out: Groups::build(places, self.on_out.iter().copied()),
            on_gain: Groups::build(places, self.on_gain.iter().copied()),
            always: Groups::build(places, self.always.iter().copied()),
            on_spend: Groups::build(self.entities.len(), self.on_spend.iter().copied()),
            purposes: Groups::default(),
            about: Groups::default(),
            contracts: Groups::default(),
            timed: self.timed,
        };
        let (mut txns, mut flows) = (Arena::new(), Arena::new());
        self.txns.into_iter().for_each(|txn| {
            txns.push(txn);
        });
        self.flows.into_iter().for_each(|flow| {
            flows.push(flow);
        });
        Book {
            names: self.names,
            text_values: Arena::new(),
            base: self.usd,
            relaxed: false,
            roots,
            places: self.places,
            entities: self.entities,
            kinds,
            purposes,
            systems: Tree::default(),
            commodities: self.commodities,
            assets: Arena::new(),
            contracts: Arena::new(),
            also: Arena::new(),
            laws: self.laws,
            rules,
            budgets: Arena::new(),
            params: Arena::new(),
            schedules: Arena::new(),
            code_rules: Vec::new(),
            codes: self.codes,
            selectors: self.selectors,
            details: self.details,
            patterns: Arena::new(),
            formats: Arena::new(),
            txns,
            journal_programs: Arena::new(),
            assertion_programs: Arena::new(),
            written_occurrences: Arena::new(),
            input_values: Arena::new(),
            flows,
            touching,
            asserts: self.asserts,
            endings: Vec::new(),
            claim_changes: Vec::new(),
            events: self.events,
            prices: Prices::default(),
            splits: self.splits,
            measures: Arena::new(),
            readings: Vec::new(),
            filed: Vec::new(),
            sources: Vec::new(),
            lookup: Default::default(),
            issuer_places: Default::default(),
        }
    }
}

fn commodity(symbol: Sym, scale: u8) -> Commodity {
    Commodity {
        symbol,
        kind: Id::new(0),
        scale,
        title: None,
        liquidity: None,
        select: None,
        growth: None,
        props: Box::new([]),
        doc: None,
        loc: None,
    }
}

/// Builds a law's node arena bottom-up, the way the model's compiler does:
/// children before parents, each node remembering where its subtree starts.
pub(crate) struct LawBuilder {
    name: Sym,
    doc: Option<Sym>,
    trigger: Trigger,
    nodes: Vec<Node>,
    steps: Vec<Step>,
}

impl LawBuilder {
    pub fn new(name: Sym, trigger: Trigger) -> LawBuilder {
        LawBuilder { name, doc: None, trigger, nodes: Vec::new(), steps: Vec::new() }
    }

    pub fn doc(mut self, doc: Sym) -> LawBuilder {
        self.doc = Some(doc);
        self
    }

    fn node(&mut self, op: Op, ty: Ty, first: Option<NodeId>) -> NodeId {
        let id = NodeId(self.nodes.len() as u32);
        let loc = Loc::new(FileId(1), id.0 * 10, id.0 * 10 + 5);
        self.nodes.push(Node { op, ty: Some(ty), loc, first: first.unwrap_or(id) });
        id
    }

    pub fn konst(&mut self, value: Value, ty: Ty) -> NodeId {
        self.node(Op::Const(value), ty, None)
    }

    pub fn var(&mut self, var: Var, ty: Ty) -> NodeId {
        self.node(Op::Var(var), ty, None)
    }

    pub fn bin(&mut self, op: BinOp, left: NodeId, right: NodeId, ty: Ty) -> NodeId {
        let first = self.nodes[left.index()].first;
        self.node(Op::Bin(op, left, right), ty, Some(first))
    }

    pub fn if_then_else(&mut self, condition: NodeId, yes: NodeId, no: NodeId, ty: Ty) -> NodeId {
        let first = self.nodes[condition.index()].first;
        self.node(Op::If(condition, yes, no), ty, Some(first))
    }

    /// `left is alternatives…`
    pub fn is(&mut self, left: NodeId, alternatives: &[NodeId]) -> NodeId {
        let first = self.nodes[left.index()].first;
        self.node(Op::Is(left, alternatives.into()), Ty::Bool, Some(first))
    }

    /// `base.field`
    pub fn field(&mut self, base: NodeId, field: Field, ty: Ty) -> NodeId {
        let first = self.nodes[base.index()].first;
        self.node(Op::Field(base, field), ty, Some(first))
    }

    pub fn call(&mut self, func: Func, args: &[NodeId], ty: Ty) -> NodeId {
        let first = args.first().map(|a| self.nodes[a.index()].first);
        self.node(Op::Call(func, args.into()), ty, first)
    }

    /// Makes the law a `by` law that fires on the date `node` computes.
    pub fn by(&mut self, node: NodeId) {
        self.trigger = Trigger::By(node);
    }

    fn step(mut self, kind: StepKind) -> LawBuilder {
        let loc = Loc::new(FileId(1), 500 + self.steps.len() as u32 * 10, 505 + self.steps.len() as u32 * 10);
        self.steps.push(Step { loc, kind });
        self
    }

    pub fn require(self, cond: NodeId, message: Option<Sym>) -> LawBuilder {
        self.step(StepKind::Require { cond, otherwise: Box::default(), message, severity: Severity::Error })
    }

    /// `require cond else owe amount to who as name`
    pub fn require_else_owe(self, cond: NodeId, amount: NodeId, to: Id<Entity>, name: Sym) -> LawBuilder {
        let otherwise = Box::new([Effect::Owe { amount, to, due: None, name }]);
        self.step(StepKind::Require { cond, otherwise, message: None, severity: Severity::Error })
    }

    pub fn warn(self, cond: NodeId) -> LawBuilder {
        self.step(StepKind::Require { cond, otherwise: Box::default(), message: None, severity: Severity::Warning })
    }

    pub fn when(self, cond: NodeId) -> LawBuilder {
        self.step(StepKind::When(cond))
    }

    pub fn unless(self, cond: NodeId) -> LawBuilder {
        self.step(StepKind::Unless(cond))
    }

    pub fn count(self, amount: NodeId, name: Sym) -> LawBuilder {
        self.step(StepKind::Effect(Effect::Count { amount, name }))
    }

    pub fn owe(self, amount: NodeId, to: Id<Entity>, name: Sym) -> LawBuilder {
        self.step(StepKind::Effect(Effect::Owe { amount, to, due: None, name }))
    }

    fn build(self) -> Law {
        Law {
            name: self.name,
            doc: self.doc,
            owner: Owner::Kind(Id::new(0)),
            system: None,
            trigger: self.trigger,
            budget: None,
            overrides: None,
            override_name: None,
            rank: Rank::ZERO,
            steps: self.steps.into(),
            nodes: self.nodes.into(),
            loc: Loc::new(FileId(1), 0, 1000),
        }
    }
}
