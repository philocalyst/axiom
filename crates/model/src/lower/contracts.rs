//! S5 contract declarations are lowered in two passes. Names and ids exist
//! before any template expression compiles, so a template may mention a
//! contract declared later in the project.

mod lines;
mod relator;

use axiom_core::{Day, Days, Diagnostic, Id, Loc, Map, Ratio, Run, Sym, Timeline};
use axiom_syntax as ast;
use axiom_syntax::{ClauseKind, Direction, ExprKind, Name};

use super::infer::classify;
use super::tail::{Reach, resolve_object, written_purpose, written_waive};
use super::{compile_roots, contract_roots, inputs};
use crate::book::{
    Amount, Asset, At, Book, Cadence, Commodity, Contract, Deadline, Entity, Input, Loan, Place, Role, Share, Terms,
    Text,
};
use crate::collect::Collected;
use crate::declare::World;
use crate::errors::{Reported, Word};
use crate::journal::{Flow, Infer, Mode, Object, Origin, Program, Provenance, Purposed, Select, TEMPLATE_TXN, Waive};
use crate::law::{Owner, Ty};
use crate::laws::{Placement, Positions};
use crate::problem::{self, Noun};
use crate::promise::Blame;
use crate::resolve::End;
use crate::scope::Home;
use crate::sources::Site;
use crate::split::{Cut, Expr, FlowSide, Header, Item, Leg, Part, Promised, Quantity, Says, Sign};

/// A contract as written, with the id reserved for it and its name.
#[derive(Clone, Copy)]
struct WrittenContract<'a, 's> {
    site: &'a Site<'a, 's>,
    node: &'a ast::Contract<'s>,
    id: Id<Contract>,
    name: Sym,
    loc: Loc,
}

impl<'a, 's> WrittenContract<'a, 's> {
    fn file(&self) -> &'a ast::File<'s> {
        &self.site.source.file
    }

    fn home(&self) -> Home {
        self.site.home
    }
}

/// Reserves every contract id before compiling any contract body. Terms and
/// their computed roots are then compiled against the complete contract
/// namespace, while occurrences are handled by [`super::record`].
pub(crate) fn contracts<'a, 's>(world: &mut World<'s>, collected: &Collected<'a, 's>, diags: &mut Vec<Diagnostic>) {
    let mut written = Vec::new();
    for contract in &collected.contracts {
        let (site, node, loc) = (contract.site, contract.node, contract.item.loc);
        let name = world.book.names.intern(node.name.0);
        if let Some(first) = world.book.lookup.contracts.get(&name).copied() {
            let (word, first) = (Word::of(contract.file(), node.name.0), Some(world.book.contracts[first].loc));
            diags.push(problem::duplicate(Noun::Contract, word, first));
            continue;
        }
        let id = world.book.contracts.push(empty_contract(name, loc, world.book.roots.me));
        world.book.lookup.contracts.insert(name, id);
        written.push(WrittenContract { site, node, id, name, loc });
    }
    for written in written.iter().copied() {
        if let Some(end) = loan_endpoint(world, written) {
            world.contract_endpoints.insert(written.name, end);
        }
    }

    for written in written.iter().copied() {
        if let Some(contract) = lower_contract(world, written, diags) {
            world.book.contracts[written.id] = contract;
        }
    }

    // Nested contract laws follow their parent contract's declaration, after
    // its id exists and before the shared law index is finalized.
    for written in written.iter().copied() {
        let file = written.file();
        let placement = Placement { file, home: written.home(), owner: Owner::Contract(written.id), subject: Ty::Flow };
        let mut laws = Vec::new();
        for also in &file[written.node.alsos] {
            laws.extend(crate::laws::compile_also(world, diags, &placement, also, Positions::NONE));
        }
        laws.extend(relator::legs(world, collected, written, diags));
        for law in &file[written.node.laws] {
            laws.extend(crate::laws::compile_native(world, diags, &placement, law));
        }
        world.book.contracts[written.id].laws = laws.into_boxed_slice();
    }
}

fn empty_contract(name: Sym, loc: Loc, me: axiom_core::Id<Entity>) -> Contract {
    Contract {
        name,
        kind: None,
        fillers: Box::default(),
        party: me,
        owner: me,
        purpose: None,
        description: None,
        area: None,
        days: Days::ALWAYS,
        terms: None,
        standing: None,
        waived: Timeline::new(None),
        buys: None,
        deposit: None,
        deposit_holding: None,
        loan: None,
        rates: Vec::new(),
        ended: None,
        laws: Box::default(),
        doc: None,
        loc,
    }
}

/// What a contract says before its schedules are read: who it is with, when, how much space it is about, what it
/// is for, and the inputs its templates take.
struct Facts {
    party: Id<Entity>,
    days: Days,
    area: Option<Amount>,
    purpose: Option<At<Purposed>>,
    description: Option<Text>,
    inputs: Box<[Input]>,
}

/// The entity a contract is with: the one written after `with`, else the one the contract's own name says.
fn contract_party<'s>(
    world: &World<'s>,
    written: WrittenContract<'_, 's>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Id<Entity>> {
    let name = written.node.party.unwrap_or(written.node.name);
    world.entity(written.home(), Word::of(written.file(), name.0)).or_report(diags)
}

/// The owner of a contract: whoever owns the place its schedule is paid from or into, else the book's owner.
fn contract_owner<'s>(
    world: &World<'s>,
    written: WrittenContract<'_, 's>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Id<Entity>> {
    match written.node.schedule.or(written.node.standing) {
        Some(schedule) => schedule_owner(world, written.home(), written.file(), Some(schedule), diags),
        None => Some(world.book.roots.me),
    }
}

/// The debt tab a loan contract's name stands for, asked for before any template is lowered: a template may name a loan
/// that is declared after it, or the loan it is part of. It is the tab `contract_loan` asks for later, for the same party
/// and owner resolved the same way, so that asks again for what exists; what is wrong with the header, a lender who is
/// also the borrower among it, is said then.
fn loan_endpoint<'a, 's>(world: &mut World<'s>, written: WrittenContract<'a, 's>) -> Option<End> {
    let loan = written.file()[written.node.props].iter().find(|prop| prop.name.0 == "loan")?;
    // What is wrong with the header is said when the contract is lowered, so what this finds out is not.
    let said = &mut Vec::new();
    let party = contract_party(world, written, said)?;
    let owner = contract_owner(world, written, said).filter(|&owner| owner != party)?;
    let loans = world.book.roots.kinds.debt;
    Some(End { place: world.tab(party, owner, loans, loan.loc), entity: Some(party) })
}

fn contract_facts<'a, 's>(
    world: &mut World<'s>,
    written: WrittenContract<'a, 's>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Facts> {
    let (node, file) = (written.node, written.file());
    let party = contract_party(world, written, diags)?;
    let days = lines::days(file, node.props).or_report(diags)?;
    let area = lines::area(world, file, node.props).or_report(diags)?;
    let purpose = contract_purpose(world, written, diags)?;
    let description = node.description.map(|description| world.book.quoted_text(description.0));
    let inputs = inputs(world, file, node.props, diags);
    Some(Facts { party, days, area, purpose, description, inputs })
}

fn lower_contract<'a, 's>(
    world: &mut World<'s>,
    written: WrittenContract<'a, 's>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Contract> {
    let (node, file, home) = (written.node, written.file(), written.home());
    let Facts { party, days, area, purpose, description, inputs: contract_inputs } =
        contract_facts(world, written, diags)?;
    let anchor = days.first();
    let roots = contract_roots(file, node);
    let compile = |world: &mut World<'s>, roots: &_, diags: &mut Vec<Diagnostic>| {
        compile_roots(world, file, home, Ty::Flow, written.name, &contract_inputs, roots, diags)
    };
    let (regular, standing) = (compile(world, &roots.regular, diags), compile(world, &roots.standing, diags));

    let owner = contract_owner(world, written, diags)?;
    let default_holding = node
        .schedule
        .or(node.standing)
        .and_then(|schedule| schedule.terms.holding.map(|holding| (holding.name, schedule.at)));
    let deposit = lines::deposit(world, written, Keeping { owner, default_holding }).or_report(diags)?;
    let loan = contract_loan(world, written, party, owner, diags)?;
    let relation = relator::relation(world, written, diags);
    let mut contract = empty_contract(written.name, written.site.source.file.loc(node.name.0), owner);
    if let Some((kind, fillers)) = relation {
        (contract.kind, contract.fillers) = (Some(kind), fillers);
    }
    contract.party = party;
    contract.owner = owner;
    contract.days = days;
    contract.purpose = purpose;
    contract.description = description;
    contract.area = area;
    if let Some((amount, holding)) = deposit {
        contract.deposit = Some(amount);
        contract.deposit_holding = Some(holding);
    }
    contract.loan = loan.map(|(loan, _)| loan);
    contract.doc = node_doc(world, written.site, written.loc);
    contract.loc = written.loc;
    contract.buys = node.standing.and_then(|schedule| match schedule.terms.payment {
        Some(ast::Payment::Buy { unit, .. }) => resolve_commodity(world, file, unit, diags),
        _ => None,
    });

    let cx = TermsCx { written, file, inputs: &contract_inputs, anchor, party, purpose, description, area, loan };
    if let (Some(schedule), Some((program, ids))) = (node.schedule, regular) {
        contract.terms = Some(lower_terms(world, &cx, schedule, program, ids, diags)?);
    }
    if let (Some(schedule), Some((program, ids))) = (node.standing, standing) {
        contract.standing = Some(lower_terms(world, &cx, schedule, program, ids, diags)?);
    }
    for share in lines::shares(world, &cx, diags) {
        if !bears(&world.book, share.entity) {
            diags.push(share_for_a_party(&world.book, &share));
        }
        crate::laws::push_share(world, Owner::Contract(written.id), home, &share);
    }
    owed_by_party(world, &contract);
    Some(contract)
}

/// A party that is to pay the owner by a deadline can fail to, and what it then owes is a claim, held in the tab the owner
/// keeps with that party. The fold finds the tab by the two of them, so a contract that can make one asks for it here.
fn owed_by_party(world: &mut World<'_>, contract: &Contract) {
    let deadline = |terms: &Terms| terms.due.is_some() && terms.blame() == Blame::Party;
    if contract.terms.iter().chain(&contract.standing).any(deadline) {
        let claims = world.book.roots.kinds.claim;
        world.tab(contract.party, contract.owner, claims, contract.loc);
    }
}

/// The purpose a contract is written for, with the object it is of: the purpose is the contract's, and nothing
/// at all comes of a contract whose purpose names none.
fn contract_purpose<'a, 's>(
    world: &mut World<'s>,
    written: WrittenContract<'a, 's>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Option<At<Purposed>>> {
    let Some(purpose) = written.node.purpose else {
        return Some(None);
    };
    let (file, home) = (written.file(), written.home());
    let word = Word::of(file, purpose.name.0);
    let id = world.purpose(home, word).or_report(diags)?;
    let of = purpose.of.and_then(|object| resolve_object(world, home, file, object, Reach::Parties, diags));
    Some(Some(At { value: Purposed { purpose: id, of, source: Provenance::Contract(written.id) }, loc: word.loc }))
}

fn contract_loan<'s>(
    world: &mut World<'s>,
    contract: WrittenContract<'_, 's>,
    party: Id<Entity>,
    owner: Id<Entity>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Option<(Loan, Ratio)>> {
    let Some(line) = lines::loan(world, contract).or_report(diags)? else {
        return Some(None);
    };
    if party == owner {
        let lender = world.book.name(world.book.entities[party].path);
        diags.push(
            Diagnostic::error("contract-loan-party", format!("`{lender}` cannot be both the lender and the borrower"))
                .label(line.loc, "a loan is with someone other than the owner of the account it is paid from")
                .help("write the lender after `with`, as in `contract mortgage with bank`"),
        );
        return None;
    }
    let debt = world.tab(party, owner, world.book.roots.kinds.debt, line.loc);
    let lines::LoanLine { principal, on, rate, term, asset, resets, prepay, .. } = line;
    Some(Some((Loan { principal, on, term, asset, debt, resets, prepay }, rate)))
}

/// What the terms of one contract are lowered against: the contract, and what of it has been lowered already.
struct TermsCx<'a, 's> {
    written: WrittenContract<'a, 's>,
    file: &'a ast::File<'s>,
    inputs: &'a [Input],
    anchor: Day,
    party: Id<Entity>,
    purpose: Option<At<Purposed>>,
    description: Option<Text>,
    area: Option<Amount>,
    /// The loan the contract is, and the yearly rate it was made at.
    loan: Option<(Loan, Ratio)>,
}

/// The header flow of a schedule, with what the legs and the items under it are made against.
struct HeaderCx {
    flow: Flow,
    out: Quantity,
    arrive: Quantity,
    /// The end the legs are paid from, which is the header's.
    from: Id<Place>,
    from_party: Option<Id<Entity>>,
    side: FlowSide,
    owner: Id<Entity>,
    unit: Id<Commodity>,
}

fn lower_terms<'a, 's>(
    world: &mut World<'s>,
    cx: &TermsCx<'a, 's>,
    schedule: ast::Schedule<'s>,
    program: Program,
    roots: Map<ast::ExprId, crate::law::NodeId>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Terms> {
    let (file, home, node) = (cx.file, cx.written.site.home, cx.written.node);
    let mut header = template_header(world, cx, schedule, &roots, diags)?;
    let interest = split_loan_payment(world, cx, &mut header);
    let mut legs = template_legs(world, cx, &header, node.body.legs, &roots, diags)?;
    legs.splice(0..0, interest);
    let lower = |world: &mut World<'s>, item, diags: &mut Vec<Diagnostic>| {
        lower_header_item(world, cx, &header, &roots, item, diags)
    };
    let items: Vec<_> = file[node.body.items].iter().filter_map(|item| lower(world, item, diags)).collect();
    let template = Promised {
        header: Header { flow: header.flow.clone(), out: header.out, arrive: header.arrive },
        side: header.side,
        legs: legs.into_boxed_slice(),
        items: items.into_boxed_slice(),
    };
    let due = node.deadline.as_ref().map(|deadline| Deadline {
        after: deadline.span,
        otherwise: deadline.otherwise.as_ref().and_then(|item| lower(world, item, diags)),
    });
    let grace = lines::grace(file, node.props).or_report(diags)?;
    Some(Terms {
        every: written_cadence(schedule.terms.cadence),
        on: file[schedule.terms.on].to_vec().into_boxed_slice(),
        template: Box::new([template]),
        program,
        inputs: cx.inputs.to_vec().into_boxed_slice(),
        estimate: schedule.terms.about,
        due,
        grace,
        period: lines::period(file, node.props).or_report(diags).flatten(),
        covers: lines::covers(file, node.props).or_report(diags).flatten(),
        prorated: has_property(file, node.props, "prorated"),
        escalation: lines::escalation(world, home, file, node.props, diags),
        rate: cx.loan.map(|(_, rate)| rate),
    })
}

/// The cadence a schedule is written with, as the promise holds it.
fn written_cadence(cadence: ast::Cadence) -> Cadence {
    match cadence {
        ast::Cadence::Every(span) => Cadence::Every(span),
        ast::Cadence::TwiceMonthly => Cadence::TwiceMonthly,
    }
}

/// The flow a schedule promises as a whole: between the holding and the party, in the direction written, with
/// the purpose its ends and the contract give it.
fn template_header<'a, 's>(
    world: &mut World<'s>,
    cx: &TermsCx<'a, 's>,
    schedule: ast::Schedule<'s>,
    roots: &Map<ast::ExprId, crate::law::NodeId>,
    diags: &mut Vec<Diagnostic>,
) -> Option<HeaderCx> {
    let (file, party) = (cx.file, cx.party);
    let hold = schedule.terms.holding?;
    let holding = resolve_endpoint(world, cx.written.site.home, file, hold.name, diags)?;
    let Some(party_place) = world.book.entities[party].place else {
        diags.push(
            Diagnostic::error("contract-party-place", "the contract party has no usable place")
                .label(schedule.at, "the party's flow endpoint cannot be resolved"),
        );
        return None;
    };
    let (from, to, side) = match hold.direction {
        Direction::From => (holding, party_place, FlowSide::Arrive),
        Direction::Into => (party_place, holding, FlowSide::Out),
    };
    let owner = world.book.places[holding].owner;
    let ScheduleAmount { quantity, amount, buys } = schedule_amount(world, file, schedule, roots, diags)?;
    let mut flow = cx.flow(from, to, amount, owner, schedule.at);
    let (from_party, to_party) = match hold.direction {
        Direction::From => (None, Some(party)),
        Direction::Into => (Some(party), None),
    };
    let purpose = cx.purpose.map(|at| at.value);
    let (from_end, to_end) = (End { place: from, entity: from_party }, End { place: to, entity: to_party });
    flow.purpose = classify(world, from_end, to_end, purpose, schedule.at, diags).ok()?;
    let arrive = buys.map_or(quantity, Quantity::Unknown);
    Some(HeaderCx { flow, out: quantity, arrive, from, from_party, side, owner, unit: amount.unit })
}

/// A loan's payment is a split (LANGUAGE §7): the payment leaves the owner's holding, the interest of it goes to the lender
/// (`#interest`, `of` the asset the loan is `for`), and what the interest leaves, the principal, goes to the debt tab
/// (`#principal`). The header keeps what its leg leaves, so the two add up to the payment on every payment. Returns the
/// interest's leg, and makes the header the principal's; nothing for a schedule that says its own amount, one that is paid
/// into the owner's holding, or a contract that is no loan.
fn split_loan_payment(world: &World<'_>, cx: &TermsCx<'_, '_>, header: &mut HeaderCx) -> Option<Leg<Flow>> {
    let (loan, _) = cx.loan.filter(|_| matches!(header.out, Quantity::Derived) && header.side == FlowSide::Arrive)?;
    let lender = world.book.entities[cx.party].place?;
    let purposed = |name, of| {
        let purpose = world.book.purpose(name).ok()?;
        Some(Purposed { purpose, of, source: Provenance::Derived })
    };
    let unit = loan.principal.unit;
    header.unit = unit;
    (header.flow.out, header.flow.arrive) = (Amount::zero(unit), Amount::zero(unit));
    let interest =
        Flow { to: lender, purpose: purposed("interest", loan.asset.map(Object::Asset)), ..header.flow.clone() };
    (header.flow.to, header.flow.purpose) = (loan.debt, purposed("principal", None));
    Some(Leg { flow: interest, part: Part::Of(Quantity::Interest) })
}

/// A promised split leg names the recipient. The source end of the scheduled header is kept and that portion is
/// sent to the named endpoint: an employer's paycheck leg is `lumen -> retirement`, and an owner payment leg is
/// `checking -> escrow`.
fn template_legs<'a, 's>(
    world: &mut World<'s>,
    cx: &TermsCx<'a, 's>,
    header: &HeaderCx,
    legs: ast::Many<ast::Leg<'s>>,
    roots: &Map<ast::ExprId, crate::law::NodeId>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Vec<Leg<Flow>>> {
    let (file, home) = (cx.file, cx.written.site.home);
    let mut lowered = Vec::new();
    for leg in &file[legs] {
        let to = resolve_endpoint(world, home, file, leg.end.name, diags)?;
        let (part, amount) = template_quantity(world, file, leg.amount, roots, header.unit, diags)?;
        let mut flow = cx.flow(header.from, to, amount, header.owner, leg.loc);
        let tail = lower_term_tail(world, home, file, leg.tail, diags);
        flow.codes = tail.codes;
        flow.select = tail.select;
        flow.waive = tail.waive;
        let written = tail.purpose.or(cx.purpose).map(|at| at.value);
        let ends = (End { place: header.from, entity: header.from_party }, End { place: to, entity: None });
        flow.purpose = classify(world, ends.0, ends.1, written, leg.loc, diags).ok()?;
        flow.description = tail.description.or(flow.description);
        lowered.push(Leg { flow, part });
    }
    Some(lowered)
}

/// What a schedule pays: how the header says it, in what amount, and, for a standing buy, the commodity bought.
struct ScheduleAmount {
    quantity: Quantity,
    amount: Amount,
    buys: Option<Id<Commodity>>,
}

fn schedule_amount<'s>(
    world: &World<'s>,
    file: &ast::File<'s>,
    schedule: ast::Schedule<'s>,
    roots: &Map<ast::ExprId, crate::law::NodeId>,
    diags: &mut Vec<Diagnostic>,
) -> Option<ScheduleAmount> {
    let (quantity, amount, buys) = match schedule.terms.payment {
        Some(ast::Payment::Fixed(amount)) => {
            let expr = template_amount(world, file, amount, roots, world.book.base, diags)?;
            (Quantity::Amount(expr), Quantity::Amount(expr).stand_in(world.book.base), None)
        }
        Some(ast::Payment::Buy { unit, spend }) => {
            let buy_unit = resolve_commodity(world, file, unit, diags)?;
            let expr = template_amount(world, file, spend, roots, world.book.base, diags)?;
            (Quantity::Amount(expr), Quantity::Amount(expr).stand_in(world.book.base), Some(buy_unit))
        }
        None => (Quantity::Derived, Amount::zero(world.book.base), None),
    };
    Some(ScheduleAmount { quantity, amount, buys })
}

impl TermsCx<'_, '_> {
    /// A flow the contract promises between two places: the party is the payee, and the purpose and description
    /// the contract gives its flows are the flow's own until a line says otherwise.
    fn flow(&self, from: Id<Place>, to: Id<Place>, amount: Amount, owner: Id<Entity>, loc: Loc) -> Flow {
        let day = self.anchor;
        Flow {
            day,
            recognized: Days::on(day),
            from,
            to,
            out: amount,
            arrive: amount,
            mode: Mode::Planned,
            infer: Infer::Known,
            txn: TEMPLATE_TXN,
            payee: Some(self.party),
            owner,
            purpose: self.purpose.map(|at| at.value),
            description: self.description,
            origin: Origin::Written,
            select: Run::new(Id::new(0), 0),
            header_codes: Run::new(Id::new(0), 0),
            codes: Run::new(Id::new(0), 0),
            loc,
            waive: None,
            detail: None,
        }
    }
}

/// What a leg of a promise takes of its header, and the amount its flow carries until the fold has read it.
fn template_quantity<'s>(
    world: &World<'s>,
    file: &ast::File<'s>,
    quantity: ast::Quantity<'s>,
    roots: &Map<ast::ExprId, crate::law::NodeId>,
    fallback: Id<Commodity>,
    diags: &mut Vec<Diagnostic>,
) -> Option<(Part, Amount)> {
    if let ast::Quantity::Amount(ast::Amount::Computed(expr)) = quantity
        && let ExprKind::Pct(percent) = file.exprs[expr].kind
    {
        let Some(rate) = Ratio::percent(percent.mantissa as i128, percent.scale) else {
            diags.push(
                Diagnostic::error("contract-percentage", "this percentage cannot be represented")
                    .label(file.exprs[expr].loc, "use a smaller percentage"),
            );
            return None;
        };
        if rate.is_negative() || rate > Ratio::ONE {
            diags.push(
                Diagnostic::error("contract-percentage-range", "a split percentage must be between 0% and 100%")
                    .label(file.exprs[expr].loc, "this share would exceed the parent amount"),
            );
            return None;
        }
        return Some((Part::Share(rate), Amount::zero(fallback)));
    }
    let written = match quantity {
        ast::Quantity::Amount(amount) => {
            Quantity::Amount(template_amount(world, file, amount, roots, fallback, diags)?)
        }
        ast::Quantity::Pending(amount) => {
            Quantity::Pending(template_amount(world, file, amount, roots, fallback, diags)?)
        }
        ast::Quantity::Target(amount) => {
            Quantity::Target(template_amount(world, file, amount, roots, fallback, diags)?)
        }
        ast::Quantity::Unknown(unit) => Quantity::Unknown(resolve_commodity(world, file, unit, diags)?),
        ast::Quantity::All(unit) => {
            let unit = match unit {
                Some(unit) => Some(resolve_commodity(world, file, unit, diags)?),
                None => None,
            };
            Quantity::All(unit)
        }
        ast::Quantity::Rest => return Some((Part::Rest, Amount::zero(fallback))),
        ast::Quantity::Whole => unreachable!("the parser reads `basis` as a quantity only in an opening"),
    };
    Some((Part::Of(written), written.stand_in(fallback)))
}

/// A written amount of a promise: its literal, or the node of the terms' program that computes it.
fn template_amount<'s>(
    world: &World<'s>,
    file: &ast::File<'s>,
    amount: ast::Amount<'s>,
    roots: &Map<ast::ExprId, crate::law::NodeId>,
    fallback: Id<Commodity>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Expr> {
    match amount {
        ast::Amount::Literal(literal) => {
            world.literal_amount(file, literal, Some(fallback)).or_report(diags).map(Expr::Literal)
        }
        ast::Amount::Computed(root) => {
            let Some(&node) = roots.get(&root) else {
                diags.push(
                    Diagnostic::error("template-root", "a computed template amount was not compiled")
                        .label(file.exprs[root].loc, "this amount has no typed program node"),
                );
                return None;
            };
            Some(Expr::Computed(node))
        }
    }
}

/// One item under a schedule's header: carved from it, added to it or taken from it.
fn lower_header_item<'s>(
    world: &mut World<'s>,
    cx: &TermsCx<'_, 's>,
    header: &HeaderCx,
    roots: &Map<ast::ExprId, crate::law::NodeId>,
    item: &ast::LineItem<'s>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Item<Says>> {
    let (file, home) = (cx.file, cx.written.site.home);
    let amount = template_amount(world, file, item.amount, roots, header.unit, diags)?;
    let tail = lower_term_tail(world, home, file, item.tail, diags);
    Some(Item {
        sign: match item.sign {
            ast::Sign::Carve => Sign::Carve,
            ast::Sign::Add => Sign::Add,
            ast::Sign::Less => Sign::Less,
        },
        amount: Cut::Of(amount),
        loc: item.loc,
        flow: Says {
            purpose: tail.purpose.map(|at| at.value),
            description: tail.description,
            codes: tail.codes,
            select: tail.select,
            waive: tail.waive,
        },
    })
}

/// What a contract's term line says about the flow it promises.
struct TermTail {
    codes: Run<Sym>,
    select: Run<Select>,
    purpose: Option<At<Purposed>>,
    description: Option<Text>,
    waive: Option<Waive>,
}

/// Reads the clauses a term line keeps: codes, a purpose, a description and a waiver. It takes no others: a line
/// that promises a flow does not say what an occurrence of it says about itself (`for`, `due`, `via`, `basis`,
/// `since`, `against`, `@` and `until`), and what is written there is left unread, as it always has been.
fn lower_term_tail<'s>(
    world: &mut World<'s>,
    home: Home,
    file: &ast::File<'s>,
    tail: axiom_syntax::Many<ast::Clause<'s>>,
    diags: &mut Vec<Diagnostic>,
) -> TermTail {
    let code_start = world.book.codes.len();
    let (mut purpose, mut description, mut waive) = (None, None, None);
    for clause in &file[tail] {
        match clause.kind {
            ClauseKind::Code(code) => {
                let sym = world.book.names.intern(code.name());
                world.book.codes.push(sym);
            }
            ClauseKind::Purpose(written) => {
                if let Some(value) = written_purpose(world, home, file, written, Reach::Parties, diags) {
                    purpose = Some(At { value, loc: clause.at });
                }
            }
            ClauseKind::Description(text) => description = Some(world.book.quoted_text(text.0)),
            ClauseKind::Waive(written) => waive = Some(written_waive(world, written)),
            ClauseKind::Due(_)
            | ClauseKind::For(_)
            | ClauseKind::Via(_)
            | ClauseKind::Basis(_)
            | ClauseKind::Since(_)
            | ClauseKind::Against(_)
            | ClauseKind::Price(_)
            | ClauseKind::Until(_) => {}
        }
    }
    let codes = Run::new(Id::new(code_start as u32), (world.book.codes.len() - code_start) as u32);
    let select = Run::new(Id::new(world.book.selectors.len() as u32), 0);
    TermTail { codes, select, purpose, description, waive }
}

fn resolve_commodity<'s>(
    world: &World<'s>,
    file: &ast::File<'s>,
    name: Name<'s>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Id<Commodity>> {
    world.commodity_of(Word::of(file, name.0)).or_report(diags)
}

fn resolve_endpoint<'s>(
    world: &World<'s>,
    home: Home,
    file: &ast::File<'s>,
    name: Name<'s>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Id<Place>> {
    world.end(home, Word::of(file, name.0)).or_report(diags).map(|end| end.place)
}

fn schedule_owner<'s>(
    world: &World<'s>,
    home: Home,
    file: &ast::File<'s>,
    schedule: Option<ast::Schedule<'s>>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Id<Entity>> {
    let holding = schedule?.terms.holding?;
    let place = resolve_endpoint(world, home, file, holding.name, diags)?;
    Some(world.book.places[place].owner)
}

fn node_doc<'s>(world: &mut World<'s>, site: &Site<'_, 's>, loc: Loc) -> Option<Sym> {
    let item = site.source.file.items.iter().find(|item| item.loc == loc)?;
    item.doc.map(|doc| world.book.names.intern(doc.0))
}

fn has_property(file: &ast::File<'_>, props: axiom_syntax::Many<ast::Prop<'_>>, name: &str) -> bool {
    file[props].iter().any(|prop| prop.name.0 == name)
}

/// Where a contract's deposit may be kept: by whom, and in the holding its schedule has, for a deposit that names
/// none.
#[derive(Clone, Copy)]
struct Keeping<'s> {
    owner: Id<Entity>,
    default_holding: Option<(ast::Name<'s>, Loc)>,
}

/// Whether an entity is an owner in this book, which has an account of its own: what it bears is its own.
fn bears(book: &Book, entity: Id<Entity>) -> bool {
    book.entities[entity].place.is_some_and(|place| matches!(book.places[place].role, Role::Holding(_)))
}

/// A share for a party is what it owes, and a contract makes the flow it bears and not the claim on it.
fn share_for_a_party(book: &Book, share: &Share) -> Diagnostic {
    let party = book.name(book.entities[share.entity].path);
    Diagnostic::warning(
        "contract-share-party",
        format!("`{party}` is a party, and a share for a party is what it owes"),
    )
    .label(share.loc, format!("this share is made as a flow `{party}` bears, and no claim on it"))
    .note("an owner of the book (one with an account of its own) bears its share, and nothing more is needed")
    .help(format!("for what `{party}` owes, write the claim: `{party} owes me AMOUNT`"))
}

fn asset_area(world: &World<'_>, asset: Id<Asset>, day: Day) -> Option<Amount> {
    let area = world.book.names.get("area")?;
    match world.book.said(asset, area, day)? {
        crate::law::Value::Amount(amount) => Some(amount),
        _ => None,
    }
}
