//! Native S5 journal records. This pass reads the source AST directly and
//! appends resolved records to the pooled Book arenas.

use axiom_core::{Day, Diagnostic, Groups, Id, Loc, Map, Run};
use axiom_syntax as ast;
use axiom_syntax::Subject;

use super::flow::{
    Codes, Ends, FlowCx, Parent, ResolvedEnd, ResolvedQuantity, Shape, TxnCx, empty_codes, endpoint, flow_roots,
    keep_program, lower_items, make_flow, make_resolved_flow, priced, push_flow_expressions, push_item_root,
    push_quantity_root, push_tail_roots, resolve_amount, resolve_end, resolve_quantity,
};
use super::loan_opening::{Insertion, Unopened};
use super::push_amount_root;
use super::staged::Staged;
use super::statements::{
    Stated, Within, lower_basis, lower_claim_change, lower_contract_change, lower_end, lower_event, lower_filed,
    lower_measure, lower_rate_change, lower_split, lower_value, names_a_contract, unsupported_statement,
};
use crate::balance::{self, Settled, Total};
use crate::book::{Amount, Contract, Place, ScheduleKind, Terms};
use crate::collect::{Collected, Order, Written};
use crate::declare::World;
use crate::errors::Word;
use crate::journal::{
    Action, Detail, Flow, FlowExpressions, Infer, Mode, OccurrenceTail, Origin, Program, Txn, TxnKind,
    WrittenOccurrence,
};
use crate::law::{NodeId, Ty};
use crate::problem::{self, CodeUse};
use crate::promise::{Keep, Promises};
use crate::scope::Home;
use crate::solve::Remaining;
use crate::sources::Site;
use crate::split::{Endpoint, Expr, FlowSide, Heading, Item, Leg, Made, Part, Promised, Quantity};

/// A dated record of the journal, whichever kind of item wrote it.
#[derive(Clone, Copy)]
enum Record<'a, 's> {
    Txn(Written<'a, 's, ast::Txn<'s>>),
    Statement(Written<'a, 's, ast::Statement<'s>>),
    Opening(Written<'a, 's, ast::Opening<'s>>),
}

impl<'a, 's> Record<'a, 's> {
    /// Every dated record of every source, from the buckets of `collected`, not yet in order.
    fn all(collected: &Collected<'a, 's>) -> Vec<Record<'a, 's>> {
        let txns = collected.txns.iter().map(|&written| Record::Txn(written));
        let statements = collected.statements.iter().map(|&written| Record::Statement(written));
        let openings = collected.openings.iter().map(|&written| Record::Opening(written));
        txns.chain(statements).chain(openings).collect()
    }

    /// Records are lowered by day, and by the order they were written within one.
    fn when(&self) -> (Day, Order) {
        match self {
            Record::Txn(written) => (written.node.date, written.order),
            Record::Statement(written) => (written.node.date, written.order),
            Record::Opening(written) => (written.node.date, written.order),
        }
    }

    /// The item the record is written as.
    fn item(&self) -> &ast::Item<'s> {
        match self {
            Record::Txn(written) => written.item,
            Record::Statement(written) => written.item,
            Record::Opening(written) => written.item,
        }
    }

    /// Where a line that says what the book begins with goes, if this is the record that begins it: among the lines of an
    /// opening, before its first and indented as it is, else as an opening before the record.
    fn insertion(&self) -> Insertion {
        let at = self.item().loc;
        let before_record = Insertion::Opening { before: Loc::new(at.file, at.start, at.start) };
        let Record::Opening(written) = self else { return before_record };
        let file = written.file();
        let Some(first) = file[written.node.lines].first() else { return before_record };
        let start = first.loc.start as usize;
        let line = file.src[..start].rfind('\n').map_or(0, |newline| newline + 1);
        let before = Loc::new(file.id, line as u32, line as u32);
        Insertion::Line { before, indent: file.src[line..start].to_owned() }
    }

    /// How many flows the record is expected to make, to reserve room for them.
    fn flows(&self) -> usize {
        match self {
            Record::Txn(written) => {
                let flow = &written.node.flow;
                if flow.from.end.is_some() && flow.to.end.is_some() { 1 } else { flow.body.legs.len() }
            }
            Record::Opening(written) => written.node.lines.len(),
            Record::Statement(_) => 0,
        }
    }
}

/// What an occurrence has said of one template group so far.
struct OccurrenceGroupDraft {
    source: Endpoint,
    side: FlowSide,
    legs: Vec<Leg<u32>>,
    items: Box<[Item<Option<u32>>]>,
}

#[derive(Clone, Copy)]
enum CodeTarget {
    Unique { txn: Id<Txn>, loc: Loc },
    Ambiguous { first: Loc, second: Loc },
}

/// The transactions that carry each code, said as far as it matters: one, or that two do.
#[derive(Default)]
struct Carriers {
    by_code: Map<axiom_core::Sym, CodeTarget>,
}

impl Carriers {
    fn note(&mut self, code: axiom_core::Sym, txn: Id<Txn>, loc: Loc) {
        match self.by_code.get(&code).copied() {
            None => {
                self.by_code.insert(code, CodeTarget::Unique { txn, loc });
            }
            Some(CodeTarget::Unique { txn: first, loc: at }) if first != txn => {
                self.by_code.insert(code, CodeTarget::Ambiguous { first: at, second: loc });
            }
            Some(CodeTarget::Unique { .. } | CodeTarget::Ambiguous { .. }) => {}
        }
    }
}

/// A chronological index of transaction codes. Each transaction is visited
/// once after its lowering succeeds; repeated code storage on its header and
/// flows is deduplicated by the transaction ID. A payment carries the code of
/// the claim it settles (LANGUAGE §7), so a code is often on two transactions:
/// the claims are indexed apart, for a write-off names the one that made the claim.
#[derive(Default)]
pub(super) struct CodeIndex {
    all: Carriers,
    claims: Carriers,
}

impl CodeIndex {
    fn add(&mut self, world: &World<'_>, txn_id: Id<Txn>) {
        let (book, source) = (&world.book, &world.book.txns[txn_id]);
        let flows = source.flows.ids().flat_map(|flow| book.flows[flow].codes.ids().map(|id| book.codes[id]));
        let mut codes = source.codes.ids().map(|id| book.codes[id]).chain(flows).peekable();
        // Only a transaction with a code asks what its flows make: nearly none has one.
        let makes_claim = codes.peek().is_some() && source.flows.ids().any(|flow| book.makes_claim(&book.flows[flow]));
        for code in codes {
            self.all.note(code, txn_id, source.loc);
            if makes_claim {
                self.claims.note(code, txn_id, source.loc);
            }
        }
    }

    /// The transaction `code` names, or why it names none, said the way `used` asks.
    pub(super) fn resolve<'s>(
        &self,
        world: &mut World<'s>,
        code: ast::Code<'s>,
        at: Loc,
        used: CodeUse,
        diags: &mut Vec<Diagnostic>,
    ) -> Option<Id<Txn>> {
        let symbol = world.book.names.intern(code.name());
        let claimed = matches!(used, CodeUse::ClaimWaiver) && self.claims.by_code.contains_key(&symbol);
        let carriers = if claimed { &self.claims } else { &self.all };
        let problem = match carriers.by_code.get(&symbol).copied() {
            Some(CodeTarget::Unique { txn, .. }) => return Some(txn),
            Some(CodeTarget::Ambiguous { first, second }) => {
                problem::ambiguous_code(used, code.name(), at, first, second)
            }
            None => problem::unknown_code(used, code.name(), at),
        };
        diags.push(problem);
        None
    }
}

/// Lowers dated native transactions, statements and openings in stable
/// `(day, source order)` order. Each transaction checkpoints the shared pools
/// so an invalid line cannot leave reachable partial flows or metadata.
pub(crate) fn record<'a, 's>(world: &mut World<'s>, collected: &Collected<'a, 's>, diags: &mut Vec<Diagnostic>) {
    let mut dated = Record::all(collected);
    dated.sort_unstable_by_key(Record::when);
    world.book.txns.reserve(dated.len());
    world.book.flows.reserve(dated.iter().map(Record::flows).sum());

    let mut code_index = CodeIndex::default();
    let mut unopened = Unopened::of(world, collected);
    for record in dated {
        let txn_start = world.book.txns.len();
        let diagnostic_start = diags.len();
        match record {
            Record::Txn(written) => lower_txn(world, written, &code_index, diags),
            Record::Opening(written) => lower_opening(world, written, &code_index, diags),
            Record::Statement(written) => {
                let (site, item) = (written.site, written.item);
                let at = Stated {
                    site,
                    statement: written.node,
                    loc: item.loc,
                    within: Within::Journal,
                    code_index: &code_index,
                };
                lower_statement(world, at, item.doc, diags)
            }
        }
        if diags.len() == diagnostic_start {
            for txn_index in txn_start..world.book.txns.len() {
                code_index.add(world, Id::new(txn_index as u32));
            }
        }
        unopened.after(world, || record.insertion(), diags);
    }
    unopened.finish(world, diags);
    index_flows(world);
}

/// What the book's flows say that is read by place and by pair of commodities, once they are all lowered: the prices their
/// exchanges imply, and the flows that touch each place.
fn index_flows(world: &mut World<'_>) {
    // Actual, known exchanges provide dated price evidence for their
    // commodity pair. Derive these after successful transaction lowering so
    // rollback cannot leave a quote from a rejected record. Written quotes
    // are already in the pool and retain priority on the same pair and day.
    for index in 0..world.book.flows.len() {
        let flow_id = Id::new(index as u32);
        let quote = crate::prices::implied_quote(&world.book, &world.book.flows[flow_id]);
        if let Some(quote) = quote {
            world.book.prices.quotes.push(quote);
        }
    }
    let quotes = std::mem::take(&mut world.book.prices.quotes);
    world.book.prices = crate::journal::Prices::new(quotes);

    let places = world.book.places.len();
    world.book.touching = Groups::build(
        places,
        world.book.flows.iter().flat_map(|(flow_id, flow)| {
            let ends = [(flow.from, flow_id), (flow.to, flow_id)];
            ends.into_iter().take(1 + usize::from(flow.from != flow.to))
        }),
    );
}

/// What the lowering of one transaction has made so far.
struct Built {
    flow_roots: Vec<FlowExpressions>,
    /// The split, or the header with items, the transaction is: at most one, and whether the fold has it left to solve.
    group: Option<(Made, Settled)>,
    /// Whether anything said so far makes the transaction wrong.
    successful: bool,
}

impl Built {
    /// Says what is wrong with the transaction's shape, which makes it wrong.
    fn reject(&mut self, problem: Diagnostic, diags: &mut Vec<Diagnostic>) {
        diags.push(problem);
        self.successful = false;
    }

    /// Keeps the group the transaction is, solved as far as what is written allows; or says why it cannot add up.
    fn keep(&mut self, group: Made, settled: Result<Settled, Diagnostic>, diags: &mut Vec<Diagnostic>) {
        match settled {
            Ok(settled) => self.group = Some((group, settled)),
            Err(problem) => self.reject(problem, diags),
        }
    }
}

fn lower_txn<'a, 's>(
    world: &mut World<'s>,
    record: Written<'a, 's, ast::Txn<'s>>,
    code_index: &CodeIndex,
    diags: &mut Vec<Diagnostic>,
) {
    let (item, written, home) = (record.item, record.node, record.home());
    let mut staged = Staged::open(world);
    let txn = Id::new(staged.book.txns.len() as u32);
    let diagnostic_start = diags.len();
    let Some((program, root_ids)) = compile_flow_roots(&mut staged, record.file(), home, &written.flow, diags) else {
        push_rejected_txn(&mut staged, item, written.date);
        return;
    };
    let cx = FlowCx { file: record.file(), home, day: written.date, txn, loc: item.loc, roots: &root_ids, code_index };
    let (codes, tail) = cx.lower_tail(&mut staged, written.flow.tail, diags);
    let (waive, valid) = (tail.waive, tail.valid);
    let mut built = Built { flow_roots: Vec::new(), group: None, successful: valid };
    lower_flows(&mut staged, TxnCx { cx, flow: &written.flow, tail, codes }, &mut built, diags);

    if diags.len() != diagnostic_start || !built.successful {
        push_rejected_txn(&mut staged, item, written.date);
        return;
    }
    let program = keep_program(&mut staged, program, built.flow_roots, built.group);
    let doc = item.doc.map(|doc| staged.book.names.intern(doc.0));
    let record = Txn { program, codes, waive, doc, ..journal_txn(&staged, written.date, item.loc) };
    staged.book.txns.push(record);
    staged.commit();
}

/// The expressions a transaction's header, legs and items compute by, compiled as one program, and which node
/// each came to.
fn compile_flow_roots<'s>(
    staged: &mut Staged<'_, 's>,
    file: &ast::File<'s>,
    home: Home,
    flow: &ast::Flow<'s>,
    diags: &mut Vec<Diagnostic>,
) -> Option<(Program, Map<ast::ExprId, NodeId>)> {
    let roots = flow_roots(file, flow);
    if roots.is_empty() {
        return Some((Program::default(), Map::default()));
    }
    let name = staged.book.names.intern("journal");
    let (program, nodes) = crate::laws::compile_template(staged, diags, file, home, Ty::Flow, name, &[], &roots)?;
    let by_expr = roots.iter().zip(nodes.iter()).map(|(&(expr, _), &node)| (expr, node)).collect();
    Some((program, by_expr))
}

/// What a header's ends make of the transaction: one flow, a split from the one end it names, or a mistake in
/// the shape, which is said.
fn lower_flows<'s>(staged: &mut Staged<'_, 's>, txn: TxnCx<'_, 's>, built: &mut Built, diags: &mut Vec<Diagnostic>) {
    let (cx, flow) = (txn.cx, txn.flow);
    let from = flow.from.end.map(|end| resolve_end(staged, &cx, end, diags));
    let to = flow.to.end.map(|end| resolve_end(staged, &cx, end, diags));
    built.successful &= from.is_none_or(|end| end.is_some()) && to.is_none_or(|end| end.is_some());
    if let Some(problem) = party_subject(staged, &cx, flow, [from, to].map(Option::flatten), diags) {
        return built.reject(problem, diags);
    }
    let has_legs = !cx.file[flow.body.legs].is_empty();
    match (from, to) {
        (Some(Some(from)), Some(Some(to))) => {
            assert!(!has_legs, "the parser rejects legs under a header that names both ends");
            lower_named_flow(staged, txn, Ends { from, to }, built, diags)
        }
        _ if !has_legs => {
            let problem = Diagnostic::error("flow-shape", "a flow needs a named end and at least one leg")
                .label(cx.loc, "no complete flow can be formed here")
                .help("write both ends in the header, or write one end and indent the other legs");
            built.reject(problem, diags);
        }
        (Some(Some(end)), None) => lower_split_flow(staged, &txn, SplitEnd { end, side: FlowSide::Out }, built, diags),
        (None, Some(Some(end))) => {
            lower_split_flow(staged, &txn, SplitEnd { end, side: FlowSide::Arrive }, built, diags)
        }
        _ => {
            let problem = Diagnostic::error("flow-shape", "a split header names exactly one end")
                .label(cx.loc, "the named end of the split is missing or ambiguous");
            built.reject(problem, diags);
        }
    }
}

/// A line written `<-`, as a purchase or a sale, or as a split through an owner is about its subject, whose book it is,
/// so the subject is an owner's. A party reaches an owner's book by the other end: `checking -> acme 3_200 USD`.
fn party_subject<'s>(
    staged: &mut Staged<'_, 's>,
    cx: &FlowCx<'_, 's>,
    flow: &ast::Flow<'s>,
    [from, to]: [Option<ResolvedEnd>; 2],
    diags: &mut Vec<Diagnostic>,
) -> Option<Diagnostic> {
    let named = flow.owner(cx.file)?;
    let subject = match flow.course {
        ast::Course::Through(..) => resolve_end(staged, cx, named, diags),
        ast::Course::Direct(ast::Junction::In) => to,
        ast::Course::Direct(ast::Junction::Out) => from,
    }?;
    let party = |end: ResolvedEnd| staged.book.places[end.place].class == crate::book::Class::Outside;
    if !party(subject) {
        return None;
    }
    let (name, ends) = (named.name.0, [(flow.from.end, from), (flow.to.end, to)]);
    let other =
        ends.into_iter().find_map(|(written, end)| Some((written?.name.0, party(end?)))).filter(|o| o.0 != name);
    let (label, help) = match other {
        Some((other, false)) => (format!("`{name}` is a party"), format!("swap the ends: `{other} -> {name} ...`")),
        Some((other, true)) => {
            (format!("`{name}` and `{other}` are both parties"), "write one of your books first".into())
        }
        None => (
            format!("`{name}` is a party"),
            "name the book it happens in first: `brokerage <- 7 VTI @ 285.70 USD`".into(),
        ),
    };
    let said = format!(
        "`{name}` is a party, and a `{}` line is written from one of your books",
        flow.course.junction().spelling()
    );
    Some(Diagnostic::error("junction-subject", said).label(cx.file.arrow_after(named), label).help(help))
}

/// A header that names both its ends is one flow, with the items under it as a group of their own.
fn lower_named_flow<'s>(
    staged: &mut Staged<'_, 's>,
    txn: TxnCx<'_, 's>,
    ends: Ends,
    built: &mut Built,
    diags: &mut Vec<Diagnostic>,
) {
    let (cx, written, codes) = (txn.cx, txn.flow, txn.codes);
    let flow_at = staged.flows().len();
    let Some((flow, exprs)) = make_flow(staged, txn, ends, diags) else {
        built.successful = false;
        return;
    };
    let says_amount =
        flow.infer == Infer::Known && exprs.is_none_or(|exprs| exprs.out.is_none() && exprs.arrive.is_none());
    let header = Remaining { out: flow.out, arrive: flow.arrive };
    staged.book.flows.push(flow);
    if let Some(exprs) = exprs {
        built.flow_roots.push(FlowExpressions { flow: flow_at, ..exprs });
    }
    if cx.file[written.body.items].is_empty() {
        return;
    }
    let parent = Parent { ends, mode: Mode::Actual, header_codes: codes, tail: None };
    let items = lower_items(staged, &cx, written.body.items, parent, &mut built.flow_roots, diags);
    let group = Made { header: Heading::Flow(flow_at), side: FlowSide::Out, legs: Box::default(), items };
    let total = if says_amount { Total::Is(header) } else { Total::Later };
    let flows = staged.flows();
    let settled = balance::settle(&mut staged.book, &group, flows, total, cx.loc);
    built.keep(group, settled, diags);
}

/// The end a header names when it is the source of the legs under it, and which side of their flows it is on.
#[derive(Clone, Copy)]
struct SplitEnd {
    end: ResolvedEnd,
    side: FlowSide,
}

/// A split being lowered: the transaction it is in, its source, and the total its header states, if it does.
struct Split<'c, 's> {
    txn: &'c TxnCx<'c, 's>,
    source: SplitEnd,
    total: Option<ResolvedQuantity>,
}

/// The legs of a split so far: where each flow went, and what each said it moved.
struct SplitLegs {
    legs: Vec<Leg<u32>>,
}

/// What a split's header says it moves, if it says: the amount on the source's own side of the arrow, or else the one
/// on the other. Either is the total the legs and the items add up to.
fn stated_total<'s>(source: SplitEnd, written: &ast::Flow<'s>) -> Option<(ast::Quantity<'s>, FlowSide)> {
    let (own, over) = match source.side {
        FlowSide::Out => (written.from.amount, written.to.amount),
        FlowSide::Arrive => (written.to.amount, written.from.amount),
    };
    own.map(|quantity| (quantity, source.side)).or(over.map(|quantity| (quantity, source.side.other())))
}

/// What the header's total gives the solver to take from: an amount that is written, one the fold computes, or none.
fn total_of(total: Option<ResolvedQuantity>) -> Total {
    match total.map(|total| total.quantity()) {
        Some(Quantity::Amount(Expr::Literal(amount)) | Quantity::Pending(Expr::Literal(amount))) => {
            Total::Is(Remaining { out: amount, arrive: amount })
        }
        Some(Quantity::Amount(Expr::Computed(_)) | Quantity::Pending(Expr::Computed(_)) | Quantity::All(_)) => {
            Total::Later
        }
        _ => Total::Nothing,
    }
}

/// A header that names one end is the source of its legs, which name the others, and of the items under it.
fn lower_split_flow<'s>(
    staged: &mut Staged<'_, 's>,
    txn: &TxnCx<'_, 's>,
    source: SplitEnd,
    built: &mut Built,
    diags: &mut Vec<Diagnostic>,
) {
    let (cx, written) = (&txn.cx, txn.flow);
    let said = diags.len();
    let stated = stated_total(source, written);
    let base = staged.book.base;
    let total = stated.and_then(|(quantity, side)| resolve_quantity(staged, cx, quantity, base, side, diags));
    if stated.is_some() && total.is_none() {
        built.successful = false;
    }
    let split = Split { txn, source, total };
    let mut made = SplitLegs { legs: Vec::new() };
    for leg in &cx.file[written.body.legs] {
        let Some(leg) = split.lower_leg(staged, leg, diags) else {
            built.successful = false;
            continue;
        };
        made.legs.push(Leg { flow: leg.at, part: leg.quantity });
        push_flow_expressions(&mut built.flow_roots, leg.at, leg.out, leg.arrive, leg.basis);
    }
    let items = split.items(staged, &made, built, diags);
    if !written.body.items.is_empty() && items.iter().any(|item| item.flow.is_some()) && made.legs.is_empty() {
        built.successful = false;
    }
    let header = Heading::Source { end: endpoint(source.end), total: total.map(|total| total.quantity()) };
    let group = Made { header, side: source.side, legs: made.legs.into_boxed_slice(), items };
    let flows = staged.flows();
    // A leg or a total that failed to lower is not there to add up, and what is missing would be what the split is
    // short of: the error already said is the one to read.
    let total = if diags.len() == said { total_of(total) } else { Total::Later };
    let settled = balance::settle(&mut staged.book, &group, flows, total, cx.loc);
    built.keep(group, settled, diags);
}

/// One leg of a split once its flow is made: where the flow is, what the leg said it moved, and the expressions
/// that compute it.
struct MadeLeg {
    at: u32,
    quantity: Part,
    out: Option<NodeId>,
    arrive: Option<NodeId>,
    basis: Option<NodeId>,
}

impl<'s> Split<'_, 's> {
    /// One leg: a flow between the source and the end it names. None after the problem is said.
    fn lower_leg(
        &self,
        staged: &mut Staged<'_, 's>,
        leg: &ast::Leg<'s>,
        diags: &mut Vec<Diagnostic>,
    ) -> Option<MadeLeg> {
        let (cx, SplitEnd { end: source, side }) = (&self.txn.cx, self.source);
        let other = resolve_end(staged, cx, leg.end, diags)?;
        let source_is_from = side == FlowSide::Out;
        let (from, to) = if source_is_from { (source, other) } else { (other, source) };
        let (leg_codes, leg_tail) = cx.lower_tail(staged, leg.tail, diags);
        let tail = self.txn.tail.clone().merge(leg_tail);
        let unit = self.total.map_or(staged.book.base, |total| total.amount.unit);
        let quantity = resolve_quantity(staged, cx, leg.amount, unit, side.other(), diags)?;
        let at = staged.flows().len();
        let basis = tail.basis_root;
        // A leg written in another commodity than the total is the exchange of what the others leave: it keeps the
        // amount it says on its own side, and the source's side is the solver's to say.
        let exchange = self.total.is_some() && quantity.infer == Infer::Known && quantity.amount.unit != unit;
        let (out, arrive) = match (exchange, source_is_from) {
            (false, _) => (quantity.amount, quantity.amount),
            (true, true) => (Amount::zero(unit), quantity.amount),
            (true, false) => (quantity.amount, Amount::zero(unit)),
        };
        let shape = Shape { ends: Ends { from, to }, out, arrive, infer: quantity.infer, mode: quantity.mode };
        let codes = Codes { header: self.txn.codes, local: leg_codes };
        let flow = make_resolved_flow(staged, cx, shape, codes, tail, leg.loc, diags)?;
        staged.book.flows.push(flow);
        let (out, arrive) = if source_is_from { (None, quantity.root()) } else { (quantity.root(), None) };
        Some(MadeLeg { at, quantity: quantity.part, out, arrive, basis })
    }

    /// The items under the header: between the source and the remainder leg's end, or the first leg's when none is the
    /// remainder, which is what an item of the source's own flow is.
    fn items(
        &self,
        staged: &mut Staged<'_, 's>,
        made: &SplitLegs,
        built: &mut Built,
        diags: &mut Vec<Diagnostic>,
    ) -> Box<[Item<Option<u32>>]> {
        let (cx, SplitEnd { end: source, side }) = (&self.txn.cx, self.source);
        let remainder = made.legs.iter().find(|leg| matches!(leg.part, Part::Rest));
        let leg_end = remainder.or(made.legs.first()).map(|leg| {
            let leg = staged.flow(leg.flow);
            let place = if side == FlowSide::Out { leg.to } else { leg.from };
            ResolvedEnd { place, entity: None, select: Run::new(Id::new(0), 0) }
        });
        // An item goes the way the legs do: from the source to the end that is the other side of it, or the other way
        // when the source is where the legs arrive.
        let other = leg_end.unwrap_or(source);
        let ends =
            if side == FlowSide::Out { Ends { from: source, to: other } } else { Ends { from: other, to: source } };
        let parent = Parent { ends, mode: Mode::Actual, header_codes: self.txn.codes, tail: None };
        lower_items(staged, cx, self.txn.flow.body.items, parent, &mut built.flow_roots, diags)
    }
}

fn lower_opening<'a, 's>(
    world: &mut World<'s>,
    record: Written<'a, 's, ast::Opening<'s>>,
    code_index: &CodeIndex,
    diags: &mut Vec<Diagnostic>,
) {
    let (file, site) = (record.file(), record.site);
    let Some(opening_place) = world.book.entities[world.book.roots.opening].place else {
        diags.push(
            Diagnostic::error("opening-place", "the opening source has no place")
                .label(record.item.loc, "cannot record opening"),
        );
        return;
    };
    lower_opening_balances(world, record, opening_place, code_index, diags);
    for claim in &file[record.node.claims] {
        let loc = super::subject_loc(file, claim.subject);
        let at = Stated { site, statement: claim, loc, within: Within::Opening, code_index };
        lower_statement(world, at, None, diags);
    }
}

/// The opening's balances as one transaction of flows out of the opening entity: kept whole, or not at all.
fn lower_opening_balances<'a, 's>(
    world: &mut World<'s>,
    record: Written<'a, 's, ast::Opening<'s>>,
    opening_place: Id<Place>,
    code_index: &CodeIndex,
    diags: &mut Vec<Diagnostic>,
) {
    let (file, item, opening) = (record.file(), record.item, record.node);
    let mut staged = Staged::open(world);
    let txn = Id::new(staged.book.txns.len() as u32);
    let diagnostic_start = diags.len();
    let mut roots = Vec::new();
    for leg in &file[opening.lines] {
        push_tail_roots(file, leg.tail, &mut roots);
    }
    let program_name = staged.book.names.intern("journal");
    let compiled = super::compile_roots(&mut staged, file, record.home(), Ty::Flow, program_name, &[], &roots, diags);
    let Some((program, root_ids)) = compiled else {
        push_rejected_txn(&mut staged, item, opening.date);
        return;
    };
    let cx = FlowCx { file, home: record.home(), day: opening.date, txn, loc: item.loc, roots: &root_ids, code_index };
    let mut flow_roots = Vec::new();
    for leg in &file[opening.lines] {
        let Some((flow, basis_root)) = lower_opening_leg(&mut staged, &cx, opening_place, leg, diags) else {
            continue;
        };
        let flow_at = staged.flows().len();
        staged.book.flows.push(flow);
        push_flow_expressions(&mut flow_roots, flow_at, None, None, basis_root);
    }
    if diags.len() != diagnostic_start {
        push_rejected_txn(&mut staged, item, opening.date);
        return;
    }
    let program_id = keep_program(&mut staged, program, flow_roots, None);
    let doc = item.doc.map(|doc| staged.book.names.intern(doc.0));
    let record = Txn { program: program_id, doc, ..journal_txn(&staged, opening.date, item.loc) };
    staged.book.txns.push(record);
    staged.commit();
}

/// One line of an opening: a flow between the opening entity and the place or asset it names, and the node its
/// basis is computed by, if it is. None after the problem is said.
fn lower_opening_leg<'s>(
    staged: &mut Staged<'_, 's>,
    cx: &FlowCx<'_, 's>,
    opening_place: Id<Place>,
    leg: &ast::Leg<'s>,
    diags: &mut Vec<Diagnostic>,
) -> Option<(Flow, Option<NodeId>)> {
    let whole = matches!(leg.amount, ast::Quantity::Whole);
    let whole_asset = if whole { staged.book.asset(leg.end.name.0) } else { None };
    let no_select = Run::new(Id::new(0), 0);
    let end = match whole_asset {
        Some(asset) => Some(ResolvedEnd { place: staged.book.assets[asset].place, entity: None, select: no_select }),
        None => resolve_end(staged, cx, leg.end, diags),
    };
    let Some(end) = end else {
        if whole {
            diags.push(
                Diagnostic::error("opening-whole", "a whole opening holding must name an asset")
                    .label(leg.loc, "write a unit amount for an account holding"),
            );
        }
        return None;
    };
    let fallback = whole_asset.map_or(staged.book.base, |asset| staged.book.assets[asset].unit);
    let quantity = resolve_quantity(staged, cx, leg.amount, fallback, FlowSide::Out, diags)?;
    if !matches!(leg.amount, ast::Quantity::Amount(ast::Amount::Literal(_))) && whole_asset.is_none() {
        diags.push(
            Diagnostic::error("opening-amount", "an opening line needs a literal amount")
                .label(leg.loc, "computed and inferred quantities cannot set an opening balance"),
        );
        return None;
    }
    let tail = cx.lower_tail(staged, leg.tail, diags).1;
    if !end.select.is_empty() {
        diags.push(
            Diagnostic::error("opening-selector", "an opening line sets a whole place")
                .label(leg.loc, "selectors do not apply to an opening balance"),
        );
        return None;
    }
    let opening_end = ResolvedEnd { place: opening_place, entity: None, select: no_select };
    let named_end = ResolvedEnd { place: end.place, entity: None, select: no_select };
    let (from, to) = match staged.book.places[end.place].class.display_sign() > 0 {
        true => (opening_end, named_end),
        false => (named_end, opening_end),
    };
    let basis_root = tail.basis_root;
    let codes = Codes { header: Run::new(staged.codes().start(), 0), local: empty_codes(staged) };
    let shape = Shape {
        ends: Ends { from, to },
        out: quantity.amount,
        arrive: quantity.amount,
        infer: Infer::Known,
        mode: Mode::Opening,
    };
    let mut flow = make_resolved_flow(staged, cx, shape, codes, tail, leg.loc, diags)?;
    flow.owner = whole_asset.map_or(staged.book.places[end.place].owner, |asset| staged.book.assets[asset].owner);
    Some((flow, basis_root))
}

/// Reads one statement into the record its verb makes.
fn lower_statement<'s>(
    world: &mut World<'s>,
    at: Stated<'_, '_, 's>,
    doc: Option<ast::Doc<'s>>,
    diags: &mut Vec<Diagnostic>,
) {
    let Stated { site, statement, loc, within, code_index } = at;
    match &statement.verb {
        ast::Verb::Value(amount) => lower_value(world, at, *amount, diags),
        ast::Verb::Worked(amount) => lower_measure(world, at, *amount, Action::Work, diags),
        ast::Verb::Used(amount) => lower_measure(world, at, *amount, Action::Use, diags),
        ast::Verb::Event(state) => lower_event(world, at, *state, diags),
        ast::Verb::Filed(year) => lower_filed(world, at, *year, diags),
        ast::Verb::Owes { creditor, amount } => {
            let opening = matches!(within, Within::Opening);
            lower_owes(world, site, loc, statement, *creditor, *amount, code_index, opening, diags)
        }
        ast::Verb::Basis { amount, since } => lower_basis(world, at, *amount, *since, diags),
        ast::Verb::Split { numerator, denominator } => lower_split(world, at, *numerator, *denominator, diags),
        // Custom properties are lowered by props::declare, which stages dated values and their inclusive `until`
        // restoration; native budgets by the declaration and law pass.
        ast::Verb::Now(ast::Change::Property(line))
            if line.name.0 == "at" && names_a_contract(world, statement.subject) =>
        {
            lower_rate_change(world, at, line, diags)
        }
        ast::Verb::Now(ast::Change::Property(_) | ast::Change::Budget(_)) => {}
        ast::Verb::Waived => match statement.subject {
            Subject::Code(code) => lower_claim_change(world, at, code, diags),
            _ => lower_contract_change(world, at, diags),
        },
        ast::Verb::Ends => lower_end(world, at, diags),
        ast::Verb::Occurrence(amount) => lower_occurrence(world, site, doc, loc, statement, *amount, code_index, diags),
        ast::Verb::Now(_) => {
            unsupported_statement(loc, "this statement kind does not yet have a native record lowering", diags)
        }
    }
}

/// What a line dated `date` keeps of a contract as it stands: the schedule, its due day and the terms the occurrence
/// is made from, or the error that says why it keeps none.
fn kept_by<'c>(
    contract: &'c Contract,
    date: Day,
    loc: Loc,
    diags: &mut Vec<Diagnostic>,
) -> Option<(ScheduleKind, Day, &'c Terms)> {
    let (promises, promise) = Promises::alone(contract);
    match promise.keep(&promises, date) {
        Keep::Kept { schedule, due } => {
            let kept = contract.terms_of(schedule).map(|terms| (schedule, due, terms));
            if kept.is_none() {
                diags.push(outside_every_window(loc));
            }
            kept
        }
        Keep::Outside => {
            diags.push(outside_every_window(loc));
            None
        }
        Keep::Ambiguous { regular, standing } => {
            diags.push(
                Diagnostic::error(
                    "ambiguous-contract-occurrence",
                    "this occurrence is equally close to two contract schedules",
                )
                .label(loc, "write it on a date that identifies one schedule")
                .note(format!("nearest regular due day: {regular}; nearest standing due day: {standing}")),
            );
            None
        }
    }
}

fn outside_every_window(loc: Loc) -> Diagnostic {
    Diagnostic::error("contract-occurrence-date", "this day is outside every contract schedule's grace window")
        .label(loc, "no active scheduled occurrence is close enough to this date")
}

fn lower_occurrence<'a, 's>(
    world: &mut World<'s>,
    site: &Site<'a, 's>,
    doc: Option<ast::Doc<'s>>,
    loc: Loc,
    statement: &ast::Statement<'s>,
    amount: Option<ast::Amount<'s>>,
    code_index: &CodeIndex,
    diags: &mut Vec<Diagnostic>,
) {
    let file = &site.source.file;
    let Subject::Name(name) = statement.subject else {
        unsupported_statement(loc, "a contract occurrence needs a named subject", diags);
        return;
    };
    let Some(contract_id) = world.book.contract(name.0) else {
        diags.push(
            Diagnostic::error("unknown-contract-occurrence", "this occurrence names no contract")
                .label(file.loc(name.0), format!("`{}` is not a declared contract", name.0))
                .help("declare a contract with this name before recording an occurrence"),
        );
        return;
    };
    if world.book.contracts[contract_id].loan.is_some_and(|loan| loan.on == statement.date) {
        lower_loan_origin(world, site, doc, loc, statement, amount, contract_id, code_index, diags);
        return;
    }
    let contract = &world.book.contracts[contract_id];
    let Some((schedule, due, terms)) = kept_by(contract, statement.date, loc, diags) else { return };
    let inputs = terms.inputs.clone();
    let templates = terms.template.clone();
    let fallback = occurrence_amount_unit(contract, terms, world.book.base);

    let mut staged = Staged::open(world);
    let diagnostic_start = diags.len();
    let mut expressions = Vec::new();
    if let Some(amount) = amount {
        push_amount_root(amount, &mut expressions);
    }
    push_tail_roots(file, statement.tail, &mut expressions);
    for leg in &file[statement.body.legs] {
        push_quantity_root(leg.amount, &mut expressions);
        push_tail_roots(file, leg.tail, &mut expressions);
    }
    for item in &file[statement.body.items] {
        push_item_root(file, item.amount, &mut expressions);
        push_tail_roots(file, item.tail, &mut expressions);
    }
    let name = staged.book.names.intern("journal");
    let compiled = if expressions.is_empty() {
        Some((Program::default(), Box::<[NodeId]>::default()))
    } else {
        crate::laws::compile_template(&mut staged, diags, file, site.home, Ty::Flow, name, &inputs, &expressions)
    };
    let Some((program, root_ids)) = compiled else { return };
    let roots: Map<_, _> = expressions.iter().zip(root_ids.iter()).map(|(&(expr, _), &node)| (expr, node)).collect();
    let txn_id = Id::new(staged.book.txns.len() as u32);
    let cx = FlowCx { file, home: site.home, day: statement.date, txn: txn_id, loc, roots: &roots, code_index };
    let occurrence_amount = amount.and_then(|amount| resolve_amount(&staged, &cx, amount, fallback, diags));
    if amount.is_some() && occurrence_amount.is_none() {
        return;
    }

    let (codes, header_tail) = cx.lower_tail(&mut staged, statement.tail, diags);
    if !header_tail.valid || header_tail.price.is_some() {
        if header_tail.price.is_some() {
            diags.push(
                Diagnostic::error("contract-occurrence-price", "write an occurrence price as part of its amount")
                    .label(loc, "a detached price cannot override a contract flow"),
            );
        }
        return;
    }
    let occurrence_tail = OccurrenceTail {
        codes,
        purpose: header_tail.purpose,
        description: header_tail.description,
        payee: header_tail.payee,
        recognized: header_tail.recognized,
        detail: header_tail.detail,
        basis: header_tail.basis_root,
        waive: header_tail.waive,
    };

    let mut input_values: Vec<Option<Amount>> = vec![None; inputs.len()];
    let mut bound = vec![false; inputs.len()];
    let mut replaced_legs = Vec::new();
    let mut added_ends: Vec<(Id<Place>, Loc)> = Vec::new();
    let mut written_groups: Vec<Option<OccurrenceGroupDraft>> = (0..templates.len()).map(|_| None).collect();
    let mut flow_roots = Vec::new();
    for leg in &file[statement.body.legs] {
        let input = inputs.iter().position(|input| staged.book.name(input.name) == leg.end.name.0);
        if let Some(input_at) = input {
            if bound[input_at] {
                let first = inputs[input_at].loc;
                diags.push(problem::twice("input binding", leg.loc, first));
                continue;
            }
            if !file[leg.tail].is_empty() {
                diags.push(
                    Diagnostic::error("contract-input-tail", "a contract input binding cannot have flow clauses")
                        .label(leg.loc, "put clauses on the occurrence's actual flow"),
                );
                continue;
            }
            let literal = match leg.amount {
                ast::Quantity::Amount(ast::Amount::Literal(literal))
                | ast::Quantity::Target(ast::Amount::Literal(literal)) => literal,
                _ => {
                    diags.push(
                        Diagnostic::error("contract-input-value", "a contract input needs a literal amount")
                            .label(leg.loc, "write `input-name = 155 USD`"),
                    );
                    continue;
                }
            };
            let input_unit = inputs[input_at].unit;
            let unit = match literal.unit() {
                Some(unit) => match staged.commodity_of(Word::of(file, unit.0)) {
                    Ok(unit) => unit,
                    Err(problem) => {
                        diags.push(problem);
                        continue;
                    }
                },
                None => match input_unit {
                    Some(unit) => unit,
                    None => {
                        diags.push(
                            Diagnostic::error("contract-input-unit", "this input has no declared unit to infer")
                                .label(leg.loc, "state the amount's commodity"),
                        );
                        continue;
                    }
                },
            };
            if input_unit.is_some_and(|expected| expected != unit) {
                diags.push(
                    Diagnostic::error("contract-input-unit", "this input amount has the wrong commodity")
                        .label(leg.loc, "use the unit declared by this input")
                        .context(inputs[input_at].loc, "the input's expected unit is declared here"),
                );
                continue;
            }
            match staged.amount(literal.num(), unit, leg.loc) {
                Ok(value) => {
                    input_values[input_at] = Some(value);
                    bound[input_at] = true;
                }
                Err(problem) => diags.push(problem),
            }
            continue;
        }

        let Some(endpoint) = resolve_end(&mut staged, &cx, leg.end, diags) else {
            continue;
        };
        let mut matching = None;
        let mut ambiguous = None;
        for (template_at, template) in templates.iter().enumerate() {
            for (leg_at, template_leg) in template.legs.iter().enumerate() {
                let named_end = template_leg.flow.to;
                if named_end == endpoint.place {
                    if matching.is_some() {
                        ambiguous = Some((template_at, leg_at));
                        break;
                    }
                    matching = Some((template_at, leg_at));
                }
            }
            if ambiguous.is_some() {
                break;
            }
        }
        if let Some((template_at, leg_at)) = ambiguous {
            let other = templates[template_at].legs[leg_at].flow.loc;
            diags.push(
                Diagnostic::error("contract-occurrence-leg-ambiguous", "this end matches more than one template leg")
                    .label(leg.loc, "write a more specific occurrence override")
                    .context(other, "a matching template leg is here"),
            );
            continue;
        }
        let (template_at, template_leg) = match matching {
            Some((template_at, leg_at)) => {
                if replaced_legs.contains(&(template_at, leg_at)) {
                    let first = templates[template_at].legs[leg_at].flow.loc;
                    diags.push(problem::twice("occurrence leg", leg.loc, first));
                    continue;
                }
                replaced_legs.push((template_at, leg_at));
                (template_at, Some(&templates[template_at].legs[leg_at]))
            }
            None if templates.len() == 1 => {
                // Occurrence statements may add a recipient that was not
                // listed in the promise (for example, a one-off tax
                // withholding on a paycheck). The engine appends this flow
                // to the same group and subtracts it from the header's
                // remainder.
                if let Some((_, first_loc)) = added_ends.iter().find(|(place, _)| *place == endpoint.place) {
                    diags.push(problem::twice("occurrence leg", leg.loc, *first_loc));
                    continue;
                }
                added_ends.push((endpoint.place, leg.loc));
                (0, None)
            }
            None => {
                diags.push(
                    Diagnostic::error(
                        "contract-occurrence-leg-group",
                        "this additional end does not identify one contract flow group",
                    )
                    .label(leg.loc, "name an end covered by a template leg"),
                );
                continue;
            }
        };
        let template = &templates[template_at];
        let side = template_leg.map_or_else(|| template_side(&staged, template), |_| template.side);
        let base_flow = template_leg.map_or_else(|| template.header.flow.clone(), |leg| leg.flow.clone());
        if endpoint.select.len() != 0 {
            diags.push(
                Diagnostic::error("selector-target", "selectors narrow the source endpoint of a flow")
                    .label(leg.loc, "a contract split end names the recipient"),
            );
            continue;
        }
        let fallback = match side {
            FlowSide::Out => base_flow.out.unit,
            FlowSide::Arrive => base_flow.arrive.unit,
        };
        let Some(quantity) = resolve_quantity(&mut staged, &cx, leg.amount, fallback, side, diags) else {
            continue;
        };
        if quantity.mode == Mode::Opening {
            diags.push(
                Diagnostic::error("contract-occurrence-whole", "a written occurrence leg needs a quantity")
                    .label(leg.loc, "whole assets are only valid in an opening"),
            );
            continue;
        }
        let (local_codes, written_tail) = cx.lower_tail(&mut staged, leg.tail, diags);
        let mut tail = header_tail.clone().merge(written_tail);
        if !tail.valid {
            continue;
        }

        let mut out = base_flow.out;
        let mut arrive = base_flow.arrive;
        match side {
            FlowSide::Out => out = quantity.amount,
            FlowSide::Arrive => arrive = quantity.amount,
        }
        if out.unit == arrive.unit {
            out = quantity.amount;
            arrive = quantity.amount;
        }
        if let Some((rate, quote, at)) = tail.price {
            if quantity.root().is_some() {
                diags.push(
                    Diagnostic::error("price-shape", "a written price needs a literal occurrence quantity")
                        .label(at, "computed quantities cannot be priced here"),
                );
                continue;
            }
            let (other_unit, chosen_unit) = match side {
                FlowSide::Out => (arrive.unit, out.unit),
                FlowSide::Arrive => (out.unit, arrive.unit),
            };
            let other = if chosen_unit == quote {
                let Some(inverse) = rate.recip() else {
                    diags.push(
                        Diagnostic::error("price-zero", "a price must be greater than zero")
                            .label(at, "the reciprocal price is not representable"),
                    );
                    continue;
                };
                if other_unit == quote {
                    diags.push(
                        Diagnostic::error("price-transfer", "a price cannot change a same-commodity transfer")
                            .label(at, "remove the price"),
                    );
                    continue;
                }
                priced(&staged, quantity.amount, other_unit, inverse, at, diags)
            } else if other_unit == quote {
                priced(&staged, quantity.amount, quote, rate, at, diags)
            } else {
                diags.push(
                    Diagnostic::error("price-unit", "the stated price unit must match the other side")
                        .label(at, "the quote unit appears on neither counterpart side"),
                );
                None
            };
            let Some(other) = other else { continue };
            match side {
                FlowSide::Out => arrive = other,
                FlowSide::Arrive => out = other,
            }
            tail.price = None;
        }

        let from = ResolvedEnd {
            place: base_flow.from,
            entity: place_entity(&staged, base_flow.from),
            select: base_flow.select,
        };
        let to = ResolvedEnd { place: endpoint.place, entity: endpoint.entity, select: endpoint.select };
        let local_codes = if local_codes.is_empty() { base_flow.codes } else { local_codes };
        let shape = Shape { ends: Ends { from, to }, out, arrive, infer: quantity.infer, mode: quantity.mode };
        let flow_codes = Codes { header: codes, local: local_codes };
        let Some(mut flow) = make_resolved_flow(&mut staged, &cx, shape, flow_codes, tail.clone(), leg.loc, diags)
        else {
            continue;
        };
        flow.owner = base_flow.owner;
        flow.payee = flow.payee.or(base_flow.payee);
        flow.purpose = flow.purpose.or(base_flow.purpose);
        flow.description = flow.description.or(base_flow.description);
        flow.waive = flow.waive.or(base_flow.waive);
        flow.header_codes = base_flow.header_codes;
        flow.codes = local_codes;
        flow.select = from.select;
        flow.detail = merge_detail_pool(&mut staged, base_flow.detail, flow.detail);
        let offset = staged.flows().len();
        staged.book.flows.push(flow);
        let (out_root, arrive_root) = match side {
            FlowSide::Out => (quantity.root(), None),
            FlowSide::Arrive => (None, quantity.root()),
        };
        push_flow_expressions(&mut flow_roots, offset, out_root, arrive_root, tail.basis_root);
        if written_groups[template_at].is_none() {
            let side = template_side(&staged, template);
            written_groups[template_at] = Some(occurrence_group_draft(template, side));
        }
        let draft = written_groups[template_at].as_mut().expect("inserted occurrence group");
        draft.legs.push(Leg { flow: offset, part: quantity.part });
    }
    if !file[statement.body.items].is_empty() {
        let Some(template) = templates.first() else {
            diags.push(
                Diagnostic::error("contract-occurrence-items", "this contract schedule has no flow group for items")
                    .label(loc, "items need a scheduled flow to modify"),
            );
            return;
        };
        let side = template_side(&staged, template);
        let (from, to, common) = occurrence_item_ends(&staged, template, side);
        let parent =
            Parent { ends: Ends { from, to }, mode: Mode::Actual, header_codes: codes, tail: Some(&header_tail) };
        let items = lower_items(&mut staged, &cx, statement.body.items, parent, &mut flow_roots, diags);
        let template_at = 0;
        if written_groups[template_at].is_none() {
            written_groups[template_at] = Some(occurrence_group_draft(template, side));
        }
        let draft = written_groups[template_at].as_mut().expect("inserted occurrence group");
        draft.source = Endpoint { place: common, entity: None };
        draft.items = items;
    }
    if diags.len() != diagnostic_start {
        return;
    }

    let groups: Box<[Option<Made>]> = written_groups
        .into_iter()
        .map(|draft| {
            draft.map(|draft| Made {
                header: Heading::Source { end: draft.source, total: None },
                side: draft.side,
                legs: draft.legs.into_boxed_slice(),
                items: draft.items,
            })
        })
        .collect();
    // The flows of an occurrence are never posted: the engine builds the occurrence's own from the groups and reads
    // the nodes, so what its flows compute (`flow_roots`) is not kept.
    let program_id = keep_program(&mut staged, program, Vec::new(), None);
    let occurrence_id = staged.book.written_occurrences.push(WrittenOccurrence {
        due,
        schedule,
        amount: occurrence_amount,
        program: program_id,
        groups,
        tail: occurrence_tail,
    });
    let input_start = staged.book.input_values.len();
    for value in input_values {
        staged.book.input_values.push(value);
    }
    let doc = doc.map(|doc| staged.book.names.intern(doc.0));
    let txn = Txn {
        inputs: Run::new(Id::new(input_start as u32), inputs.len() as u32),
        codes,
        waive: header_tail.waive,
        contract: Some(contract_id),
        contract_schedule: Some(schedule),
        occurrence: Some(occurrence_id),
        doc,
        ..journal_txn(&staged, statement.date, loc)
    };
    staged.book.txns.push(txn);
    staged.commit();
}

/// Record the source loan's funding as one balanced debt-to-cash flow. It is
/// deliberately separate from the first scheduled payment, which may begin
/// months after the origination date.
fn lower_loan_origin<'a, 's>(
    world: &mut World<'s>,
    site: &Site<'a, 's>,
    doc: Option<ast::Doc<'s>>,
    loc: Loc,
    statement: &ast::Statement<'s>,
    amount: Option<ast::Amount<'s>>,
    contract_id: Id<crate::book::Contract>,
    code_index: &CodeIndex,
    diags: &mut Vec<Diagnostic>,
) {
    let file = &site.source.file;
    if amount.is_some() || !file[statement.body.legs].is_empty() || !file[statement.body.items].is_empty() {
        diags.push(
            Diagnostic::error(
                "loan-origination-shape",
                "a loan origination uses the principal declared by the contract",
            )
            .label(loc, "do not add a second amount or split body to the origination marker"),
        );
        return;
    }
    let (loan, party, owner, funding) = {
        let contract = &world.book.contracts[contract_id];
        let Some(loan) = contract.loan else {
            diags.push(
                Diagnostic::error("loan-origination-contract", "this contract has no loan principal to originate")
                    .label(loc, "only a declared loan can have an origination record"),
            );
            return;
        };
        let template = [&contract.terms, &contract.standing]
            .into_iter()
            .flatten()
            .find(|terms| !terms.template.is_empty())
            .and_then(|terms| terms.template.first());
        let Some(template) = template else {
            diags.push(
                Diagnostic::error(
                    "loan-origination-holding",
                    "the loan schedule does not identify a cash holding for its principal",
                )
                .label(loc, "add a payment schedule from the account that receives the loan"),
            );
            return;
        };
        let mut candidates = [template.header.flow.from, template.header.flow.to].into_iter().filter(|&place| {
            let place = &world.book.places[place];
            place.owner == contract.owner
                && place.class == crate::book::Class::Asset
                && matches!(place.role, crate::book::Role::Account { .. } | crate::book::Role::Holding(_))
        });
        let Some(funding) = candidates.next() else {
            diags.push(
                Diagnostic::error(
                    "loan-origination-holding",
                    "the loan schedule does not identify an owner cash account",
                )
                .label(loc, "the origination needs the owner-side payment holding"),
            );
            return;
        };
        if candidates.next().is_some() {
            diags.push(
                Diagnostic::error("loan-origination-holding", "the loan schedule names more than one owner holding")
                    .label(loc, "the principal destination is ambiguous"),
            );
            return;
        }
        (loan, contract.party, contract.owner, funding)
    };
    if world.book.entities[party].place.is_none() {
        diags.push(
            Diagnostic::error("loan-origination-party", "the lender has no flow endpoint")
                .label(loc, "cannot identify the source of this principal"),
        );
        return;
    }

    let mut staged = Staged::open(world);
    let diagnostic_start = diags.len();
    let mut expressions = Vec::new();
    push_tail_roots(file, statement.tail, &mut expressions);
    let name = staged.book.names.intern("journal");
    let compiled = if expressions.is_empty() {
        Some((Program::default(), Box::<[NodeId]>::default()))
    } else {
        crate::laws::compile_template(&mut staged, diags, file, site.home, Ty::Flow, name, &[], &expressions)
    };
    let Some((program, root_ids)) = compiled else { return };
    let roots: Map<_, _> = expressions.iter().zip(root_ids.iter()).map(|(&(expr, _), &node)| (expr, node)).collect();
    let txn_id = Id::new(staged.book.txns.len() as u32);
    let cx = FlowCx { file, home: site.home, day: statement.date, txn: txn_id, loc, roots: &roots, code_index };
    let (codes, mut tail) = cx.lower_tail(&mut staged, statement.tail, diags);
    let basis_root = tail.basis_root;
    let waive = tail.waive;
    if tail.price.is_some() {
        diags.push(
            Diagnostic::error("loan-origination-price", "loan principal is transferred in the loan's declared unit")
                .label(loc, "a loan origination cannot add a detached price"),
        );
        tail.valid = false;
    }
    if !tail.valid || diags.len() != diagnostic_start {
        return;
    }

    // The debt tab's outflow records the owner's new liability; the same
    // principal arrives in the account named by the payment schedule.
    tail.payee = Some(party);
    let empty = Run::new(Id::new(0), 0);
    let from = ResolvedEnd { place: loan.debt, entity: None, select: empty };
    let to = ResolvedEnd { place: funding, entity: None, select: empty };
    let shape = Shape {
        ends: Ends { from, to },
        out: loan.principal,
        arrive: loan.principal,
        infer: Infer::Known,
        mode: Mode::Actual,
    };
    let flow_codes = Codes { header: codes, local: empty_codes(&staged) };
    let Some(mut flow) = make_resolved_flow(&mut staged, &cx, shape, flow_codes, tail, loc, diags) else {
        return;
    };
    flow.owner = owner;
    flow.payee = Some(party);
    flow.origin = Origin::Occurrence(contract_id);
    staged.book.flows.push(flow);
    let mut flow_roots = Vec::new();
    if let Some(basis) = basis_root {
        push_flow_expressions(&mut flow_roots, 0, None, None, Some(basis));
    }
    let program = keep_program(&mut staged, program, flow_roots, None);
    let doc = doc.map(|doc| staged.book.names.intern(doc.0));
    let txn = Txn {
        program,
        codes,
        waive,
        contract: Some(contract_id),
        kind: TxnKind::LoanOrigin,
        doc,
        ..journal_txn(&staged, loan.on, loc)
    };
    staged.book.txns.push(txn);
    staged.commit();
}

/// The ends of a claim and whose it is: the party it is with on one end, and on the other the tab the owners keep it in.
/// None after the problem is said.
fn claim_ends<'a, 's>(
    world: &mut World<'s>,
    site: &Site<'a, 's>,
    loc: Loc,
    statement: &ast::Statement<'s>,
    creditor_name: ast::Name<'s>,
    diags: &mut Vec<Diagnostic>,
) -> Option<(Ends, Id<crate::book::Entity>)> {
    let file = &site.source.file;
    if let Some(leg) = file[statement.body.legs].first() {
        diags.push(
            Diagnostic::error("claim-split", "a claim cannot contain split flow legs")
                .label(leg.loc, "write claim line items here, not a transfer between endpoints"),
        );
        return None;
    }
    let Subject::Name(debtor_name) = statement.subject else {
        unsupported_statement(loc, "a claim needs a named debtor", diags);
        return None;
    };
    let entity = |world: &World<'s>, name: ast::Name<'s>| world.entity(site.home, Word::of(file, name.0));
    let debtor = match entity(world, debtor_name) {
        Ok(entity) => entity,
        Err(problem) => {
            diags.push(problem);
            return None;
        }
    };
    let creditor = match entity(world, creditor_name) {
        Ok(entity) => entity,
        Err(problem) => {
            diags.push(problem);
            return None;
        }
    };
    if debtor == creditor {
        diags.push(
            Diagnostic::error("self-claim", "an entity cannot owe itself").label(loc, "name a different creditor"),
        );
        return None;
    }
    let debtor_is_owner = world.book.entities[debtor].place.is_some_and(
        |place| matches!(world.book.places[place].role, crate::book::Role::Holding(owner) if owner == debtor),
    );
    let creditor_is_owner = world.book.entities[creditor].place.is_some_and(
        |place| matches!(world.book.places[place].role, crate::book::Role::Holding(owner) if owner == creditor),
    );
    let kinds = world.book.roots.kinds;
    let (party, owner, kind, party_end) = if creditor_is_owner {
        (debtor, creditor, kinds.claim, debtor)
    } else if debtor_is_owner {
        (creditor, debtor, kinds.debt_claim, creditor)
    } else {
        // Neither end is an owner: the subject owes the creditor, who holds the claim.
        (debtor, creditor, kinds.claim, debtor)
    };
    let tab = world.tab(party, owner, kind, loc);
    let Some(party_place) = world.book.entities[party_end].place else {
        diags.push(
            Diagnostic::error("claim-party-place", "the claim party has no flow endpoint")
                .label(loc, "this claim cannot be attached to a party"),
        );
        return None;
    };
    let empty = Run::new(Id::new(0), 0);
    let outside = ResolvedEnd { place: party_place, entity: Some(party_end), select: empty };
    let tab = ResolvedEnd { place: tab, entity: None, select: empty };
    let (from, to) = if kind == kinds.claim { (outside, tab) } else { (tab, outside) };
    Some((Ends { from, to }, owner))
}

/// The expressions of a claim (its amount, its items' and its tail's), compiled; a claim says what is owed by one or the
/// other. None after the problem is said.
fn claim_program<'a, 's>(
    world: &mut World<'s>,
    site: &Site<'a, 's>,
    (loc, statement): (Loc, &ast::Statement<'s>),
    amount: Option<ast::Amount<'s>>,
    diags: &mut Vec<Diagnostic>,
) -> Option<(Program, Map<ast::ExprId, NodeId>)> {
    let file = &site.source.file;
    if amount.is_none() && statement.body.items.is_empty() {
        diags.push(
            Diagnostic::error("claim-amount", "a claim needs an amount or line items")
                .label(loc, "nothing states what is owed"),
        );
        return None;
    }
    let mut exprs = Vec::new();
    if let Some(amount) = amount {
        push_amount_root(amount, &mut exprs);
    }
    for item in &file[statement.body.items] {
        push_item_root(file, item.amount, &mut exprs);
        push_tail_roots(file, item.tail, &mut exprs);
    }
    push_tail_roots(file, statement.tail, &mut exprs);
    let name = world.book.names.intern("journal");
    super::compile_roots(world, file, site.home, Ty::Flow, name, &[], &exprs, diags)
}

fn lower_owes<'a, 's>(
    world: &mut World<'s>,
    site: &Site<'a, 's>,
    loc: Loc,
    statement: &ast::Statement<'s>,
    creditor_name: ast::Name<'s>,
    amount: Option<ast::Amount<'s>>,
    code_index: &CodeIndex,
    opening: bool,
    diags: &mut Vec<Diagnostic>,
) {
    let file = &site.source.file;
    let Some((Ends { from, to }, owner)) = claim_ends(world, site, loc, statement, creditor_name, diags) else {
        return;
    };
    let Some((program, roots)) = claim_program(world, site, (loc, statement), amount, diags) else {
        return;
    };
    let mut staged = Staged::open(world);
    let txn_id = Id::new(staged.book.txns.len() as u32);
    let cx = FlowCx { file, home: site.home, day: statement.date, txn: txn_id, loc, roots: &roots, code_index };
    let diagnostic_start = diags.len();
    let (header_codes, header_tail) = cx.lower_tail(&mut staged, statement.tail, diags);
    if !header_tail.valid {
        return;
    }
    let mode = if opening { Mode::Opening } else { Mode::Actual };
    let mut built = Built { flow_roots: Vec::new(), group: None, successful: true };
    if let Some(written_amount) = amount {
        let base = staged.book.base;
        let Some(expr) = resolve_amount(&staged, &cx, written_amount, base, diags) else {
            return;
        };
        let (amount, root) = (expr.stand_in(base), expr.root());
        let shape = Shape { ends: Ends { from, to }, out: amount, arrive: amount, infer: Infer::Known, mode };
        let codes = Codes { header: header_codes, local: empty_codes(&staged) };
        if let Some(mut flow) = make_resolved_flow(&mut staged, &cx, shape, codes, header_tail.clone(), loc, diags) {
            flow.owner = owner;
            staged.book.flows.push(flow);
            push_flow_expressions(&mut built.flow_roots, 0, root, root, header_tail.basis_root);
            if !statement.body.items.is_empty() {
                let parent = Parent { ends: Ends { from, to }, mode, header_codes, tail: None };
                let items = lower_items(&mut staged, &cx, statement.body.items, parent, &mut built.flow_roots, diags);
                let made = Made { header: Heading::Flow(0), side: FlowSide::Out, legs: Box::default(), items };
                let total = match expr {
                    Expr::Literal(_) => Total::Is(Remaining { out: amount, arrive: amount }),
                    Expr::Computed(_) => Total::Later,
                };
                let flows = staged.flows();
                let settled = balance::settle(&mut staged.book, &made, flows, total, loc);
                built.keep(made, settled, diags);
            }
        }
    } else {
        let parent = Parent { ends: Ends { from, to }, mode, header_codes, tail: Some(&header_tail) };
        let items = lower_items(&mut staged, &cx, statement.body.items, parent, &mut built.flow_roots, diags);
        let header = Heading::Source { end: endpoint(from), total: Some(Quantity::Derived) };
        let made = Made { header, side: FlowSide::Out, legs: Box::default(), items };
        let flows = staged.flows();
        let settled = balance::settle(&mut staged.book, &made, flows, Total::Nothing, loc);
        built.keep(made, settled, diags);
    }
    if diags.len() != diagnostic_start {
        return;
    }
    let program_id = keep_program(&mut staged, program, built.flow_roots, built.group);
    let txn = Txn {
        program: program_id,
        codes: header_codes,
        waive: header_tail.waive,
        ..journal_txn(&staged, statement.date, loc)
    };
    staged.book.txns.push(txn);
    staged.commit();
}

fn occurrence_amount_unit(
    contract: &crate::book::Contract,
    terms: &crate::book::Terms,
    base: Id<crate::book::Commodity>,
) -> Id<crate::book::Commodity> {
    if let Some(unit) = contract.buys {
        return unit;
    }
    let Some(template) = terms.template.first() else {
        return base;
    };
    let flow = &template.header.flow;
    if flow.out.unit == base || flow.arrive.unit == base { base } else { flow.arrive.unit }
}

fn template_side(world: &World<'_>, template: &Promised) -> FlowSide {
    if !template.legs.is_empty() {
        return template.side;
    }
    if world.book.places[template.header.flow.from].class != crate::book::Class::Outside {
        FlowSide::Arrive
    } else {
        FlowSide::Out
    }
}

fn occurrence_group_draft(template: &Promised, side: FlowSide) -> OccurrenceGroupDraft {
    let common = match side {
        FlowSide::Out => template.header.flow.to,
        FlowSide::Arrive => template.header.flow.from,
    };
    OccurrenceGroupDraft {
        source: Endpoint { place: common, entity: None },
        side,
        legs: Vec::new(),
        items: Box::default(),
    }
}

fn place_entity(world: &World<'_>, place: Id<Place>) -> Option<Id<crate::book::Entity>> {
    match world.book.places[place].role {
        crate::book::Role::Outside(Some(entity)) | crate::book::Role::Tab(entity) => Some(entity),
        crate::book::Role::Holding(owner) => Some(owner),
        crate::book::Role::Outside(None)
        | crate::book::Role::Account { .. }
        | crate::book::Role::Issuer(_)
        | crate::book::Role::Asset(_) => None,
    }
}

fn occurrence_item_ends(
    world: &World<'_>,
    template: &Promised,
    side: FlowSide,
) -> (ResolvedEnd, ResolvedEnd, Id<Place>) {
    let common = match side {
        FlowSide::Out => template.header.flow.to,
        FlowSide::Arrive => template.header.flow.from,
    };
    let remainder = template.legs.first().map_or_else(
        || match side {
            FlowSide::Out => template.header.flow.from,
            FlowSide::Arrive => template.header.flow.to,
        },
        |leg| match template.side {
            FlowSide::Out => leg.flow.from,
            FlowSide::Arrive => leg.flow.to,
        },
    );
    let end = |place| ResolvedEnd {
        place,
        entity: match world.book.places[place].role {
            crate::book::Role::Outside(entity) => entity,
            crate::book::Role::Tab(entity) => Some(entity),
            _ => None,
        },
        select: Run::new(Id::new(0), 0),
    };
    let (from, to) = match side {
        FlowSide::Out => (end(remainder), end(common)),
        FlowSide::Arrive => (end(common), end(remainder)),
    };
    (from, to, common)
}

fn merge_detail_pool(
    world: &mut World<'_>,
    base: Option<Id<Detail>>,
    override_detail: Option<Id<Detail>>,
) -> Option<Id<Detail>> {
    let (Some(base), Some(override_detail)) = (base, override_detail) else {
        return base.or(override_detail);
    };
    let (base_value, override_value) = (world.book.details[base], world.book.details[override_detail]);
    let merged = Detail {
        basis: override_value.basis.or(base_value.basis),
        hold: override_value.hold.or(base_value.hold),
        since: override_value.since.or(base_value.since),
        spender: override_value.spender.or(base_value.spender),
        cost: override_value.cost.or(base_value.cost),
        due: override_value.due.or(base_value.due),
        against: override_value.against.or(base_value.against),
        reckoned: override_value.reckoned.or(base_value.reckoned),
    };
    if merged == base_value {
        Some(base)
    } else if merged == override_value {
        Some(override_detail)
    } else {
        Some(world.book.details.push(merged))
    }
}

/// The transaction of a record of the journal itself: it owns the flows and codes staged so far, and has
/// no program, contract or inputs. The record fills in what it has.
pub(super) fn journal_txn(staged: &Staged<'_, '_>, day: Day, loc: Loc) -> Txn {
    Txn {
        day,
        flows: staged.flows(),
        inputs: Run::new(Id::new(staged.book.input_values.len() as u32), 0),
        program: None,
        codes: staged.codes(),
        waive: None,
        contract: None,
        contract_schedule: None,
        occurrence: None,
        kind: TxnKind::Journal,
        doc: None,
        loc,
    }
}

/// What a rejected record leaves in place of its transaction, so that every dated record still owns one.
fn push_rejected_txn<'s>(staged: &mut Staged<'_, 's>, item: &ast::Item<'s>, day: Day) {
    let doc = item.doc.map(|doc| staged.book.names.intern(doc.0));
    let txn = Txn {
        flows: Run::new(staged.flows().start(), 0),
        codes: Run::new(staged.codes().start(), 0),
        doc,
        ..journal_txn(staged, day, item.loc)
    };
    staged.book.txns.push(txn);
}
