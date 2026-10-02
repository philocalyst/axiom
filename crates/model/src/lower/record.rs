//! Native S5 journal records. This pass reads the source AST directly and
//! appends resolved records to the pooled Book arenas.

use axiom_core::{Day, Days, Diagnostic, Dim, Groups, Id, Loc, Map, Qty, Run, Span, Sym};
use axiom_syntax as ast;
use axiom_syntax::{ClauseKind, Quantity, Subject};

use super::push_amount_root;
use super::staged::Staged;
use crate::book::{
    Amount, Change as BookChange, FlowSide, Place, ScheduleKind, Sign, TemplateAmount, TemplateItemParent, TermsState,
    Text,
};
use crate::collect::{Collected, Order, Written};
use crate::declare::World;
use crate::errors::{Reported, Word};
use crate::journal::{
    Action, Assert, ClaimChange, ClaimChangeAction, Detail, EndEvent, EndTarget, Event, Filed, Flow, FlowExpressions,
    Gap, Infer, JournalEnd, JournalGroup, JournalItem, JournalProgram, JournalQuantity, Measure, Mode, Object,
    OccurrenceTail, Origin, Provenance, Purposed, Quote, Reading, Select, Split, Txn, TxnKind, Waive, WrittenGroup,
    WrittenOccurrence,
};
use crate::law::{NodeId, Subject as ModelSubject, Ty};
use crate::problem::{self, CodeUse};
use crate::scope::Home;
use crate::sources::Site;

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

#[derive(Clone, Copy)]
struct ResolvedEnd {
    place: Id<Place>,
    entity: Option<Id<crate::book::Entity>>,
    select: Run<Select>,
}

#[derive(Clone, Copy)]
struct ResolvedQuantity {
    amount: Amount,
    infer: Infer,
    mode: Mode,
    root: Option<NodeId>,
    group: JournalQuantity,
}

#[derive(Clone, Default)]
struct Tail {
    purpose: Option<Purposed>,
    purpose_loc: Option<Loc>,
    description: Option<Text>,
    payee: Option<Id<crate::book::Entity>>,
    recognized: Option<Days>,
    waive: Option<Waive>,
    detail: Detail,
    basis_root: Option<NodeId>,
    price: Option<(axiom_core::Ratio, Id<crate::book::Commodity>, Loc)>,
    valid: bool,
}

#[derive(Clone, Copy)]
struct PurposeEvidence {
    purposed: Purposed,
    loc: Loc,
}

struct OccurrenceGroupDraft {
    template: u32,
    source: JournalEnd,
    side: FlowSide,
    legs: Vec<u32>,
    leg_quantities: Vec<JournalQuantity>,
    items: Box<[JournalItem]>,
}

#[derive(Clone, Copy)]
enum CodeTarget {
    Unique { txn: Id<Txn>, loc: Loc },
    Ambiguous { first: Loc, second: Loc },
}

/// A chronological index of transaction codes. Each transaction is visited
/// once after its lowering succeeds; repeated code storage on its header and
/// flows is deduplicated by the transaction ID.
#[derive(Default)]
struct CodeIndex {
    by_code: Map<axiom_core::Sym, CodeTarget>,
}

impl CodeIndex {
    fn add(&mut self, world: &World<'_>, txn_id: Id<Txn>) {
        let source = &world.book.txns[txn_id];
        let mut add_code = |code| match self.by_code.get(&code).copied() {
            None => {
                self.by_code.insert(code, CodeTarget::Unique { txn: txn_id, loc: source.loc });
            }
            Some(CodeTarget::Unique { txn, loc }) if txn != txn_id => {
                self.by_code.insert(code, CodeTarget::Ambiguous { first: loc, second: source.loc });
            }
            Some(CodeTarget::Unique { .. } | CodeTarget::Ambiguous { .. }) => {}
        };
        for code in source.codes.ids().map(|id| world.book.codes[id]) {
            add_code(code);
        }
        for flow_id in source.flows.ids() {
            for code in world.book.flows[flow_id].codes.ids().map(|id| world.book.codes[id]) {
                add_code(code);
            }
        }
    }

    /// The transaction `code` names, or why it names none, said the way `used` asks.
    fn resolve<'s>(
        &self,
        world: &mut World<'s>,
        code: ast::Code<'s>,
        at: Loc,
        used: CodeUse,
        diags: &mut Vec<Diagnostic>,
    ) -> Option<Id<Txn>> {
        let symbol = world.book.names.intern(code.name());
        let problem = match self.by_code.get(&symbol).copied() {
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
    for record in dated {
        let txn_start = world.book.txns.len();
        let diagnostic_start = diags.len();
        match record {
            Record::Txn(written) => lower_txn(world, written.site, written.item, written.node, &code_index, diags),
            Record::Opening(written) => {
                lower_opening(world, written.site, written.item, written.node, &code_index, diags)
            }
            Record::Statement(written) => {
                let (site, item) = (written.site, written.item);
                lower_statement(world, site, item.loc, item.doc, written.node, &code_index, false, diags)
            }
        }
        if diags.len() == diagnostic_start {
            for txn_index in txn_start..world.book.txns.len() {
                code_index.add(world, Id::new(txn_index as u32));
            }
        }
    }

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

fn lower_txn<'a, 's>(
    world: &mut World<'s>,
    site: &Site<'a, 's>,
    item: &ast::Item<'s>,
    written: &ast::Txn<'s>,
    code_index: &CodeIndex,
    diags: &mut Vec<Diagnostic>,
) {
    let (file, home) = (&site.source.file, site.home);
    let mut staged = Staged::open(world);
    let txn_id = Id::new(staged.book.txns.len() as u32);
    let diagnostic_start = diags.len();

    let roots = flow_roots(file, &written.flow);
    let name = staged.book.names.intern("journal");
    let compiled = if roots.is_empty() {
        Some((crate::book::TemplateProgram::default(), Map::default()))
    } else {
        crate::laws::compile_template(&mut staged, diags, file, home, Ty::Flow, name, &[], &roots).map(
            |(program, nodes)| {
                let by_expr = roots.iter().zip(nodes.iter()).map(|(&(expr, _), &node)| (expr, node)).collect();
                (program, by_expr)
            },
        )
    };
    let Some((program, root_ids)) = compiled else {
        push_rejected_txn(&mut staged, item, written.date);
        return;
    };

    let (header_codes, header_tail) =
        lower_tail(&mut staged, home, file, written.flow.tail, written.date, &root_ids, code_index, diags);
    let txn_waive = header_tail.waive;
    let mut flow_roots = Vec::new();
    let mut groups = Vec::new();
    let mut successful = header_tail.valid;
    let body_has_group = !written.flow.body.legs.is_empty() || !written.flow.body.items.is_empty();

    let from = written.flow.from.end.map(|end| resolve_end(&mut staged, home, file, end, diags));
    let to = written.flow.to.end.map(|end| resolve_end(&mut staged, home, file, end, diags));
    if from.is_some_and(|end| end.is_none()) || to.is_some_and(|end| end.is_none()) {
        successful = false;
    }

    if let (Some(Some(from)), Some(Some(to))) = (from, to) {
        if written.flow.body.legs.is_empty() {
            let flow_at = staged.flows().len();
            match make_flow(
                &mut staged,
                file,
                written.date,
                from,
                to,
                written.flow.from.amount,
                written.flow.to.amount,
                header_tail,
                header_codes,
                txn_id,
                item.loc,
                &root_ids,
                diags,
            ) {
                Some((flow, exprs)) => {
                    staged.book.flows.push(flow);
                    if let Some(exprs) = exprs {
                        flow_roots.push(FlowExpressions { flow: flow_at, ..exprs });
                    }
                    if body_has_group {
                        let items = lower_items(
                            &mut staged,
                            home,
                            file,
                            written.flow.body.items,
                            from,
                            to,
                            FlowSide::Out,
                            txn_id,
                            written.date,
                            header_codes,
                            &root_ids,
                            code_index,
                            Mode::Actual,
                            None,
                            &mut flow_roots,
                            diags,
                        );
                        groups.push(JournalGroup {
                            header: Some(flow_at),
                            source: journal_end(from),
                            side: FlowSide::Out,
                            total: None,
                            legs: Box::default(),
                            leg_quantities: Box::default(),
                            items,
                        });
                    }
                }
                None => successful = false,
            }
        } else {
            diags.push(
                Diagnostic::error("flow-shape", "a flow with both named ends cannot also have split legs")
                    .label(item.loc, "these legs do not have an unnamed side to fill")
                    .help("name one end in the header and put the other ends on its indented legs"),
            );
            successful = false;
        }
    } else if written.flow.body.legs.is_empty() {
        diags.push(
            Diagnostic::error("flow-shape", "a flow needs a named end and at least one leg")
                .label(item.loc, "no complete flow can be formed here")
                .help("write both ends in the header, or write one end and indent the other legs"),
        );
        successful = false;
    } else {
        let (source, source_is_from) = match (from, to) {
            (Some(Some(source)), None) => (source, true),
            (None, Some(Some(source))) => (source, false),
            _ => {
                diags.push(
                    Diagnostic::error("flow-shape", "a split header names exactly one end")
                        .label(item.loc, "the named end of the split is missing or ambiguous"),
                );
                push_rejected_txn(&mut staged, item, written.date);
                return;
            }
        };
        let source_qty = if source_is_from { written.flow.from.amount } else { written.flow.to.amount };
        let source_side = if source_is_from { FlowSide::Out } else { FlowSide::Arrive };
        let base = staged.book.base;
        let total =
            source_qty.and_then(|qty| resolve_quantity(&mut staged, file, qty, base, source_side, &root_ids, diags));
        if source_qty.is_some() && total.is_none() {
            successful = false;
        }
        let mut legs = Vec::with_capacity(written.flow.body.legs.len());
        let mut leg_quantities = Vec::with_capacity(written.flow.body.legs.len());
        for leg in &file[written.flow.body.legs] {
            let Some(other) = resolve_end(&mut staged, home, file, leg.end, diags) else {
                successful = false;
                continue;
            };
            let (from, to) = if source_is_from { (source, other) } else { (other, source) };
            let mut tail = header_tail.clone();
            let (leg_codes, leg_tail) =
                lower_tail(&mut staged, home, file, leg.tail, written.date, &root_ids, code_index, diags);
            tail = merge_tail(tail, leg_tail);
            let unit = total.map_or(staged.book.base, |total| total.amount.unit);
            let Some(quantity) =
                resolve_quantity(&mut staged, file, leg.amount, unit, source_side.other(), &root_ids, diags)
            else {
                successful = false;
                continue;
            };
            let header_amount = total.map_or_else(|| Amount::zero(unit), |total| total.amount);
            let (out, arrive) = if source_is_from {
                (Amount::zero(header_amount.unit), quantity.amount)
            } else {
                (quantity.amount, Amount::zero(header_amount.unit))
            };
            let flow_at = staged.flows().len();
            let basis_root = tail.basis_root;
            if let Some(flow) = make_resolved_flow(
                &mut staged,
                written.date,
                from,
                to,
                out,
                arrive,
                quantity.infer,
                quantity.mode,
                tail,
                header_codes,
                leg_codes,
                txn_id,
                leg.loc,
                diags,
            ) {
                staged.book.flows.push(flow);
                legs.push(flow_at);
                leg_quantities.push(quantity.group);
                push_flow_expressions(
                    &mut flow_roots,
                    flow_at,
                    (!source_is_from).then_some(quantity.root).flatten(),
                    source_is_from.then_some(quantity.root).flatten(),
                    basis_root,
                );
            } else {
                successful = false;
            }
        }
        let remainder_end = legs
            .first()
            .map(|&offset| {
                let leg = staged.flow(offset);
                if source_is_from { leg.to } else { leg.from }
            })
            .map(|place| ResolvedEnd { place, entity: None, select: Run::new(Id::new(0), 0) })
            .unwrap_or(source);
        let items = lower_items(
            &mut staged,
            home,
            file,
            written.flow.body.items,
            source,
            remainder_end,
            source_side,
            txn_id,
            written.date,
            header_codes,
            &root_ids,
            code_index,
            Mode::Actual,
            None,
            &mut flow_roots,
            diags,
        );
        if !written.flow.body.items.is_empty() && items.iter().any(|item| item.flow.is_some()) && legs.is_empty() {
            successful = false;
        }
        groups.push(JournalGroup {
            header: None,
            source: journal_end(source),
            side: source_side,
            total: total.map(|total| total.group),
            legs: legs.into_boxed_slice(),
            leg_quantities: leg_quantities.into_boxed_slice(),
            items,
        });
    }

    if diags.len() != diagnostic_start {
        successful = false;
    }
    if !successful {
        push_rejected_txn(&mut staged, item, written.date);
        return;
    }

    let program_id = if !program.nodes.is_empty() || !flow_roots.is_empty() || !groups.is_empty() {
        Some(staged.book.journal_programs.push(JournalProgram {
            program,
            flow_roots: flow_roots.into_boxed_slice(),
            groups: groups.into_boxed_slice(),
        }))
    } else {
        None
    };
    let doc = item.doc.map(|doc| staged.book.names.intern(doc.0));
    let txn = Txn {
        program: program_id,
        codes: header_codes,
        waive: txn_waive,
        doc,
        ..journal_txn(&staged, written.date, item.loc)
    };
    staged.book.txns.push(txn);
    staged.commit();
}

fn lower_opening<'a, 's>(
    world: &mut World<'s>,
    site: &Site<'a, 's>,
    item: &ast::Item<'s>,
    opening: &ast::Opening<'s>,
    code_index: &CodeIndex,
    diags: &mut Vec<Diagnostic>,
) {
    let file = &site.source.file;
    let Some(opening_place) = world.book.entities[world.book.roots.opening].place else {
        diags.push(
            Diagnostic::error("opening-place", "the opening source has no place")
                .label(item.loc, "cannot record opening"),
        );
        return;
    };
    lower_opening_balances(world, site, item, opening, opening_place, code_index, diags);
    for claim in &file[opening.claims] {
        lower_statement(world, site, super::subject_loc(file, claim.subject), None, claim, code_index, true, diags);
    }
}

/// The opening's balances as one transaction of flows out of the opening entity: kept whole, or not at all.
fn lower_opening_balances<'a, 's>(
    world: &mut World<'s>,
    site: &Site<'a, 's>,
    item: &ast::Item<'s>,
    opening: &ast::Opening<'s>,
    opening_place: Id<Place>,
    code_index: &CodeIndex,
    diags: &mut Vec<Diagnostic>,
) {
    let file = &site.source.file;
    let mut staged = Staged::open(world);
    let txn_id = Id::new(staged.book.txns.len() as u32);
    let diagnostic_start = diags.len();
    let mut roots = Vec::new();
    for leg in &file[opening.lines] {
        push_tail_roots(file, leg.tail, &mut roots);
    }
    let program_name = staged.book.names.intern("journal");
    let compiled = super::compile_roots(&mut staged, file, site.home, Ty::Flow, program_name, &[], &roots, diags);
    let Some((program, root_ids)) = compiled else {
        push_rejected_txn(&mut staged, item, opening.date);
        return;
    };
    let mut flow_roots = Vec::new();
    for leg in &file[opening.lines] {
        let whole_asset = if matches!(leg.amount, Quantity::Whole) { staged.book.asset(leg.end.name.0) } else { None };
        let end = if let Some(asset) = whole_asset {
            Some(ResolvedEnd { place: staged.book.assets[asset].place, entity: None, select: Run::new(Id::new(0), 0) })
        } else {
            resolve_end(&mut staged, site.home, file, leg.end, diags)
        };
        let Some(end) = end else {
            if matches!(leg.amount, Quantity::Whole) {
                diags.push(
                    Diagnostic::error("opening-whole", "a whole opening holding must name an asset")
                        .label(leg.loc, "write a unit amount for an account holding"),
                );
            }
            continue;
        };
        let fallback = whole_asset.map_or(staged.book.base, |asset| staged.book.assets[asset].unit);
        let Some(quantity) =
            resolve_quantity(&mut staged, file, leg.amount, fallback, FlowSide::Out, &Map::default(), diags)
        else {
            continue;
        };
        if !matches!(leg.amount, Quantity::Amount(ast::Amount::Literal(_))) && whole_asset.is_none() {
            diags.push(
                Diagnostic::error("opening-amount", "an opening line needs a literal amount")
                    .label(leg.loc, "computed and inferred quantities cannot set an opening balance"),
            );
            continue;
        }
        let tail = lower_tail(&mut staged, site.home, file, leg.tail, opening.date, &root_ids, code_index, diags).1;
        if end.select.len() != 0 {
            diags.push(
                Diagnostic::error("opening-selector", "an opening line sets a whole place")
                    .label(leg.loc, "selectors do not apply to an opening balance"),
            );
            continue;
        }
        let opening_end = ResolvedEnd { place: opening_place, entity: None, select: Run::new(Id::new(0), 0) };
        let named_end = ResolvedEnd { place: end.place, entity: None, select: Run::new(Id::new(0), 0) };
        let (from, to) = if staged.book.places[end.place].class.display_sign() > 0 {
            (opening_end, named_end)
        } else {
            (named_end, opening_end)
        };
        let amount = quantity.amount;
        let basis_root = tail.basis_root;
        let header_codes = Run::new(staged.codes().start(), 0);
        let local_codes = empty_codes(&staged);
        if let Some(mut flow) = make_resolved_flow(
            &mut staged,
            opening.date,
            from,
            to,
            amount,
            amount,
            Infer::Known,
            Mode::Opening,
            tail,
            header_codes,
            local_codes,
            txn_id,
            leg.loc,
            diags,
        ) {
            flow.owner =
                whole_asset.map_or(staged.book.places[end.place].owner, |asset| staged.book.assets[asset].owner);
            let flow_at = staged.flows().len();
            staged.book.flows.push(flow);
            push_flow_expressions(&mut flow_roots, flow_at, None, None, basis_root);
        }
    }
    if diags.len() != diagnostic_start {
        push_rejected_txn(&mut staged, item, opening.date);
        return;
    }
    let program_id = if !program.nodes.is_empty() || !flow_roots.is_empty() {
        Some(staged.book.journal_programs.push(JournalProgram {
            program,
            flow_roots: flow_roots.into_boxed_slice(),
            groups: Box::default(),
        }))
    } else {
        None
    };
    let doc = item.doc.map(|doc| staged.book.names.intern(doc.0));
    let txn = Txn { program: program_id, doc, ..journal_txn(&staged, opening.date, item.loc) };
    staged.book.txns.push(txn);
    staged.commit();
}

fn lower_statement<'a, 's>(
    world: &mut World<'s>,
    site: &Site<'a, 's>,
    loc: Loc,
    doc: Option<ast::Doc<'s>>,
    statement: &ast::Statement<'s>,
    code_index: &CodeIndex,
    opening: bool,
    diags: &mut Vec<Diagnostic>,
) {
    let file = &site.source.file;
    match &statement.verb {
        ast::Verb::Value(amount) => lower_value(world, site.home, file, loc, statement, *amount, diags),
        ast::Verb::Worked(amount) | ast::Verb::Used(amount) => {
            let action = if matches!(&statement.verb, ast::Verb::Worked(_)) { Action::Work } else { Action::Use };
            lower_measure(world, site.home, file, loc, statement, *amount, action, code_index, diags);
        }
        ast::Verb::Event(state) => {
            let Subject::Code(code) = statement.subject else {
                unsupported_statement(loc, "events need a code subject", diags);
                return;
            };
            world.book.events.push(Event {
                day: statement.date,
                code: world.book.names.intern(code.name()),
                state: *state,
                loc,
            });
        }
        ast::Verb::Filed(year) => lower_filed(world, site, loc, statement, *year, diags),
        ast::Verb::Owes { creditor, amount } => {
            lower_owes(world, site, loc, statement, *creditor, *amount, code_index, opening, diags)
        }
        ast::Verb::Basis { amount, since } => {
            lower_basis(world, site, loc, statement, *amount, *since, code_index, diags)
        }
        ast::Verb::Split { numerator, denominator } => {
            let Subject::Unit(unit) = statement.subject else {
                unsupported_statement(loc, "a split needs a commodity subject", diags);
                return;
            };
            let word = Word::of(file, unit.0);
            let Some(unit) = world.commodity_of(word).or_report(diags) else {
                return;
            };
            let Some(ratio) = numerator
                .to_ratio()
                .zip(denominator.to_ratio())
                .and_then(|(numerator, denominator)| numerator.checked_div(denominator))
                .filter(|ratio| *ratio > axiom_core::Ratio::ZERO)
            else {
                diags.push(
                    Diagnostic::error("split-ratio", "a split ratio must be greater than zero")
                        .label(loc, "the written ratio cannot be represented"),
                );
                return;
            };
            world.book.splits.push(Split { day: statement.date, unit, ratio, loc });
        }
        ast::Verb::Now(ast::Change::Property(_)) => {
            // Custom properties are lowered by props::declare, which stages
            // dated values and their inclusive `until` restoration.
        }
        ast::Verb::Now(ast::Change::Budget(_)) => {
            // Native budgets are lowered by the declaration/law pass.
        }
        ast::Verb::Waived => match statement.subject {
            Subject::Code(code) => lower_claim_change(world, site, loc, statement, code, code_index, diags),
            _ => lower_contract_change(world, site, loc, statement, diags),
        },
        ast::Verb::Ends => lower_end(world, site, loc, statement, diags),
        ast::Verb::Occurrence(amount) => lower_occurrence(world, site, doc, loc, statement, *amount, code_index, diags),
        ast::Verb::Now(_) => {
            unsupported_statement(loc, "this statement kind does not yet have a native record lowering", diags)
        }
    }
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
    let (schedule, due, terms) = match nearest_occurrence(contract, statement.date) {
        Ok(Some(found)) => found,
        Ok(None) => {
            diags.push(
                Diagnostic::error(
                    "contract-occurrence-date",
                    "this day is outside every contract schedule's grace window",
                )
                .label(loc, "no active scheduled occurrence is close enough to this date"),
            );
            return;
        }
        Err((regular, standing)) => {
            diags.push(
                Diagnostic::error(
                    "ambiguous-contract-occurrence",
                    "this occurrence is equally close to two contract schedules",
                )
                .label(loc, "write it on a date that identifies one schedule")
                .note(format!("nearest regular due day: {regular}; nearest standing due day: {standing}")),
            );
            return;
        }
    };
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
        push_amount_root(item.amount, &mut expressions);
        push_tail_roots(file, item.tail, &mut expressions);
    }
    let name = staged.book.names.intern("journal");
    let compiled = if expressions.is_empty() {
        Some((crate::book::TemplateProgram::default(), Box::<[NodeId]>::default()))
    } else {
        crate::laws::compile_template(&mut staged, diags, file, site.home, Ty::Flow, name, &inputs, &expressions)
    };
    let Some((program, root_ids)) = compiled else { return };
    let roots: Map<_, _> = expressions.iter().zip(root_ids.iter()).map(|(&(expr, _), &node)| (expr, node)).collect();
    let occurrence_amount = amount.and_then(|amount| {
        resolve_amount(&staged, file, amount, fallback, &roots, diags)
            .map(|(literal, root)| root.map_or(TemplateAmount::Literal(literal), TemplateAmount::Computed))
    });
    if amount.is_some() && occurrence_amount.is_none() {
        return;
    }

    let (codes, header_tail) =
        lower_tail(&mut staged, site.home, file, statement.tail, statement.date, &roots, code_index, diags);
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

    let txn_id = Id::new(staged.book.txns.len() as u32);
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
                Quantity::Amount(ast::Amount::Literal(literal)) | Quantity::Target(ast::Amount::Literal(literal)) => {
                    literal
                }
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

        let Some(endpoint) = resolve_end(&mut staged, site.home, file, leg.end, diags) else {
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
        let side = template_leg.map_or_else(|| template_side(&staged, template), |leg| leg.side);
        let base_flow = template_leg.map_or_else(|| template.flow.clone(), |leg| leg.flow.clone());
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
        let Some(quantity) = resolve_quantity(&mut staged, file, leg.amount, fallback, side, &roots, diags) else {
            continue;
        };
        if quantity.mode == Mode::Opening {
            diags.push(
                Diagnostic::error("contract-occurrence-whole", "a written occurrence leg needs a quantity")
                    .label(leg.loc, "whole assets are only valid in an opening"),
            );
            continue;
        }
        let (local_codes, written_tail) =
            lower_tail(&mut staged, site.home, file, leg.tail, statement.date, &roots, code_index, diags);
        let mut tail = merge_tail(header_tail.clone(), written_tail);
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
            if quantity.root.is_some() {
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
        let Some(mut flow) = make_resolved_flow(
            &mut staged,
            statement.date,
            from,
            to,
            out,
            arrive,
            quantity.infer,
            quantity.mode,
            tail.clone(),
            codes,
            local_codes,
            txn_id,
            leg.loc,
            diags,
        ) else {
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
        push_flow_expressions(
            &mut flow_roots,
            offset,
            (side == FlowSide::Out).then_some(quantity.root).flatten(),
            (side == FlowSide::Arrive).then_some(quantity.root).flatten(),
            tail.basis_root,
        );
        if written_groups[template_at].is_none() {
            let side = template_side(&staged, template);
            written_groups[template_at] = Some(occurrence_group_draft(template_at, template, side));
        }
        let draft = written_groups[template_at].as_mut().expect("inserted occurrence group");
        draft.legs.push(offset);
        draft.leg_quantities.push(quantity.group);
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
        let items = lower_items(
            &mut staged,
            site.home,
            file,
            statement.body.items,
            from,
            to,
            side,
            txn_id,
            statement.date,
            codes,
            &roots,
            code_index,
            Mode::Actual,
            Some(&header_tail),
            &mut flow_roots,
            diags,
        );
        let template_at = 0;
        if written_groups[template_at].is_none() {
            written_groups[template_at] = Some(occurrence_group_draft(template_at, template, side));
        }
        let draft = written_groups[template_at].as_mut().expect("inserted occurrence group");
        draft.source = JournalEnd { place: common, entity: None };
        draft.items = items;
    }
    if diags.len() != diagnostic_start {
        return;
    }

    let groups: Box<[WrittenGroup]> = written_groups
        .into_iter()
        .flatten()
        .map(|draft| WrittenGroup {
            template: draft.template,
            out: None,
            arrive: None,
            group: JournalGroup {
                header: None,
                source: draft.source,
                side: draft.side,
                total: None,
                legs: draft.legs.into_boxed_slice(),
                leg_quantities: draft.leg_quantities.into_boxed_slice(),
                items: draft.items,
            },
        })
        .collect();
    let program_id = (!program.nodes.is_empty() || !flow_roots.is_empty()).then(|| {
        staged.book.journal_programs.push(JournalProgram {
            program,
            flow_roots: flow_roots.into_boxed_slice(),
            groups: Box::default(),
        })
    });
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
        let template = contract
            .terms
            .as_ref()
            .map(|timeline| timeline.at(loan.on))
            .filter(|terms| !terms.template.is_empty())
            .or_else(|| {
                contract
                    .standing
                    .as_ref()
                    .map(|timeline| timeline.at(loan.on))
                    .filter(|terms| !terms.template.is_empty())
            })
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
        let mut candidates = [template.flow.from, template.flow.to].into_iter().filter(|&place| {
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
        Some((crate::book::TemplateProgram::default(), Box::<[NodeId]>::default()))
    } else {
        crate::laws::compile_template(&mut staged, diags, file, site.home, Ty::Flow, name, &[], &expressions)
    };
    let Some((program, root_ids)) = compiled else { return };
    let roots: Map<_, _> = expressions.iter().zip(root_ids.iter()).map(|(&(expr, _), &node)| (expr, node)).collect();
    let (codes, mut tail) =
        lower_tail(&mut staged, site.home, file, statement.tail, statement.date, &roots, code_index, diags);
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
    let txn_id = Id::new(staged.book.txns.len() as u32);
    let empty = Run::new(Id::new(0), 0);
    let from = ResolvedEnd { place: loan.debt, entity: None, select: empty };
    let to = ResolvedEnd { place: funding, entity: None, select: empty };
    let no_local_codes = empty_codes(&staged);
    let Some(mut flow) = make_resolved_flow(
        &mut staged,
        loan.on,
        from,
        to,
        loan.principal,
        loan.principal,
        Infer::Known,
        Mode::Actual,
        tail,
        codes,
        no_local_codes,
        txn_id,
        loc,
        diags,
    ) else {
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
    let program = (!program.nodes.is_empty() || !flow_roots.is_empty()).then(|| {
        staged.book.journal_programs.push(JournalProgram {
            program,
            flow_roots: flow_roots.into_boxed_slice(),
            groups: Box::default(),
        })
    });
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

fn nearest_occurrence<'a>(
    contract: &'a crate::book::Contract,
    day: Day,
) -> Result<Option<(ScheduleKind, Day, &'a crate::book::Terms)>, (Day, Day)> {
    if !contract.days.contains(day) {
        return Ok(None);
    }
    let mut radius = 0i64;
    for timeline in [contract.terms.as_ref(), contract.standing.as_ref()].into_iter().flatten() {
        let schedules = std::iter::once(timeline.at(Day::MIN)).chain(timeline.changes().map(|(_, terms)| terms));
        for terms in schedules {
            let cadence = match terms.every {
                crate::book::Cadence::Every(span) => {
                    i64::from(span.months).saturating_mul(31).saturating_add(i64::from(span.days))
                }
                crate::book::Cadence::TwiceMonthly => 31,
            };
            radius = radius.max(cadence);
        }
    }
    let radius = radius.clamp(0, i64::from(i32::MAX)) as i32;
    let Some(search) = Days::new(Day(day.0.saturating_sub(radius)), Day(day.0.saturating_add(radius))) else {
        return Ok(None);
    };
    let mut regular = None;
    let mut standing = None;
    for occurrence in contract.occurrences(search) {
        let distance = (i64::from(day.0) - i64::from(occurrence.day.0)).abs();
        let candidate = (distance, occurrence.day > day, occurrence.day, occurrence.terms);
        let best = match occurrence.schedule {
            ScheduleKind::Regular => &mut regular,
            ScheduleKind::Standing => &mut standing,
        };
        if best.is_none_or(|(best_distance, best_future, _, _)| (distance, candidate.1) < (best_distance, best_future))
        {
            *best = Some(candidate);
        }
    }
    match (regular, standing) {
        (Some((r_distance, _, r_day, regular_terms)), Some((s_distance, _, s_day, standing_terms))) => {
            if r_distance == s_distance {
                Err((r_day, s_day))
            } else if r_distance < s_distance {
                Ok(Some((ScheduleKind::Regular, r_day, regular_terms)))
            } else {
                Ok(Some((ScheduleKind::Standing, s_day, standing_terms)))
            }
        }
        (Some((_, _, due, terms)), None) => Ok(Some((ScheduleKind::Regular, due, terms))),
        (None, Some((_, _, due, terms))) => Ok(Some((ScheduleKind::Standing, due, terms))),
        (None, None) => Ok(None),
    }
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
    if let Some(leg) = file[statement.body.legs].first() {
        diags.push(
            Diagnostic::error("claim-split", "a claim cannot contain split flow legs")
                .label(leg.loc, "write claim line items here, not a transfer between endpoints"),
        );
        return;
    }
    let Subject::Name(debtor_name) = statement.subject else {
        unsupported_statement(loc, "a claim needs a named debtor", diags);
        return;
    };
    let entity = |world: &World<'s>, name: ast::Name<'s>| world.entity(site.home, Word::of(file, name.0));
    let debtor = match entity(world, debtor_name) {
        Ok(entity) => entity,
        Err(problem) => {
            diags.push(problem);
            return;
        }
    };
    let creditor = match entity(world, creditor_name) {
        Ok(entity) => entity,
        Err(problem) => {
            diags.push(problem);
            return;
        }
    };
    if debtor == creditor {
        diags.push(
            Diagnostic::error("self-claim", "an entity cannot owe itself").label(loc, "name a different creditor"),
        );
        return;
    }
    let debtor_is_owner = world.book.entities[debtor].place.is_some_and(
        |place| matches!(world.book.places[place].role, crate::book::Role::Holding(owner) if owner == debtor),
    );
    let creditor_is_owner = world.book.entities[creditor].place.is_some_and(
        |place| matches!(world.book.places[place].role, crate::book::Role::Holding(owner) if owner == creditor),
    );
    let (party, owner, class, party_end) = if creditor_is_owner {
        (debtor, creditor, crate::book::Class::Asset, debtor)
    } else if debtor_is_owner {
        (creditor, debtor, crate::book::Class::Debt, creditor)
    } else {
        // The declaration survey uses this same default when neither end is
        // an owner: the subject owes the creditor, who holds the claim.
        (debtor, creditor, crate::book::Class::Asset, debtor)
    };
    let tab = match world.tab(party, owner, class, loc) {
        Ok(place) => place,
        Err(problem) => {
            diags.push(problem);
            return;
        }
    };
    let Some(party_place) = world.book.entities[party_end].place else {
        diags.push(
            Diagnostic::error("claim-party-place", "the claim party has no flow endpoint")
                .label(loc, "this claim cannot be attached to a party"),
        );
        return;
    };
    let empty = Run::new(Id::new(0), 0);
    let outside = ResolvedEnd { place: party_place, entity: Some(party_end), select: empty };
    let tab = ResolvedEnd { place: tab, entity: None, select: empty };
    let (from, to) = if class == crate::book::Class::Asset { (outside, tab) } else { (tab, outside) };

    if amount.is_none() && statement.body.items.is_empty() {
        diags.push(
            Diagnostic::error("claim-amount", "a claim needs an amount or line items")
                .label(loc, "nothing states what is owed"),
        );
        return;
    }
    let mut exprs = Vec::new();
    if let Some(amount) = amount {
        push_amount_root(amount, &mut exprs);
    }
    for item in &file[statement.body.items] {
        push_amount_root(item.amount, &mut exprs);
        push_tail_roots(file, item.tail, &mut exprs);
    }
    push_tail_roots(file, statement.tail, &mut exprs);
    let name = world.book.names.intern("journal");
    let Some((program, roots)) = super::compile_roots(world, file, site.home, Ty::Flow, name, &[], &exprs, diags)
    else {
        return;
    };
    let mut staged = Staged::open(world);
    let txn_id = Id::new(staged.book.txns.len() as u32);
    let diagnostic_start = diags.len();
    let (header_codes, header_tail) =
        lower_tail(&mut staged, site.home, file, statement.tail, statement.date, &roots, code_index, diags);
    if !header_tail.valid {
        return;
    }
    let mode = if opening { Mode::Opening } else { Mode::Actual };
    let mut flow_roots = Vec::new();
    let mut groups = Vec::new();
    if let Some(written_amount) = amount {
        let base = staged.book.base;
        let Some((amount, root)) = resolve_amount(&staged, file, written_amount, base, &roots, diags) else {
            return;
        };
        let no_local_codes = empty_codes(&staged);
        if let Some(mut flow) = make_resolved_flow(
            &mut staged,
            statement.date,
            from,
            to,
            amount,
            amount,
            Infer::Known,
            mode,
            header_tail.clone(),
            header_codes,
            no_local_codes,
            txn_id,
            loc,
            diags,
        ) {
            flow.owner = owner;
            staged.book.flows.push(flow);
            push_flow_expressions(&mut flow_roots, 0, root, root, header_tail.basis_root);
            if !statement.body.items.is_empty() {
                let items = lower_items(
                    &mut staged,
                    site.home,
                    file,
                    statement.body.items,
                    from,
                    to,
                    FlowSide::Out,
                    txn_id,
                    statement.date,
                    header_codes,
                    &roots,
                    code_index,
                    mode,
                    None,
                    &mut flow_roots,
                    diags,
                );
                groups.push(JournalGroup {
                    header: Some(0),
                    source: journal_end(from),
                    side: FlowSide::Out,
                    total: None,
                    legs: Box::default(),
                    leg_quantities: Box::default(),
                    items,
                });
            }
        }
    } else {
        let items = lower_items(
            &mut staged,
            site.home,
            file,
            statement.body.items,
            from,
            to,
            FlowSide::Out,
            txn_id,
            statement.date,
            header_codes,
            &roots,
            code_index,
            mode,
            Some(&header_tail),
            &mut flow_roots,
            diags,
        );
        groups.push(JournalGroup {
            header: None,
            source: journal_end(from),
            side: FlowSide::Out,
            total: Some(JournalQuantity::Derived),
            legs: Box::default(),
            leg_quantities: Box::default(),
            items,
        });
    }
    if diags.len() != diagnostic_start {
        return;
    }
    let program_id = (!program.nodes.is_empty() || !flow_roots.is_empty() || !groups.is_empty()).then(|| {
        staged.book.journal_programs.push(JournalProgram {
            program,
            flow_roots: flow_roots.into_boxed_slice(),
            groups: groups.into_boxed_slice(),
        })
    });
    let txn = Txn {
        program: program_id,
        codes: header_codes,
        waive: header_tail.waive,
        ..journal_txn(&staged, statement.date, loc)
    };
    staged.book.txns.push(txn);
    staged.commit();
}

fn lower_basis<'a, 's>(
    world: &mut World<'s>,
    site: &Site<'a, 's>,
    loc: Loc,
    statement: &ast::Statement<'s>,
    written_amount: ast::Amount<'s>,
    since: Option<Day>,
    code_index: &CodeIndex,
    diags: &mut Vec<Diagnostic>,
) {
    let file = &site.source.file;
    let Subject::Name(name) = statement.subject else {
        unsupported_statement(loc, "a basis statement must name an asset", diags);
        return;
    };
    let Some(asset_id) = world.book.asset(name.0) else {
        diags.push(
            Diagnostic::error("basis-asset", "a basis statement must name an asset")
                .label(file.loc(name.0), "this name is not a declared asset"),
        );
        return;
    };
    if !statement.body.items.is_empty() || !statement.body.legs.is_empty() {
        unsupported_statement(loc, "a basis statement cannot have indented journal lines", diags);
        return;
    }
    let mut exprs = Vec::new();
    if let ast::Amount::Computed(expr) = written_amount {
        exprs.push((expr, Ty::AMOUNT));
    }
    push_tail_roots(file, statement.tail, &mut exprs);
    let name = world.book.names.intern("journal");
    let Some((program, roots)) = super::compile_roots(world, file, site.home, Ty::Asset, name, &[], &exprs, diags)
    else {
        return;
    };
    let mut staged = Staged::open(world);
    let txn_id = Id::new(staged.book.txns.len() as u32);
    let diagnostic_start = diags.len();
    let (header_codes, mut tail) =
        lower_tail(&mut staged, site.home, file, statement.tail, statement.date, &roots, code_index, diags);
    tail.detail.since = since.or(tail.detail.since);
    let basis_root = match written_amount {
        ast::Amount::Literal(literal) => {
            let Some(amount) = staged.literal_amount(file, literal, None).or_report(diags) else {
                return;
            };
            if amount.unit != staged.book.base {
                diags.push(
                    Diagnostic::error("basis-unit", "asset basis must be in the base currency")
                        .label(file.loc(literal.0), "convert this amount to the book's base unit"),
                );
                return;
            }
            tail.detail.basis = Some(amount.qty);
            None
        }
        ast::Amount::Computed(expr) => {
            let Some(&root) = roots.get(&expr) else {
                diags.push(
                    Diagnostic::error("basis-expression", "the basis expression was not compiled")
                        .label(file.exprs[expr].loc, "the expression is not available here"),
                );
                return;
            };
            if let Some(Ty::Amount(Dim::Of(unit))) = program.nodes[root].typed_ty()
                && unit != staged.book.base
            {
                diags.push(
                    Diagnostic::error("basis-unit", "asset basis must be in the base currency")
                        .label(file.exprs[expr].loc, "this expression has another unit"),
                );
                return;
            }
            Some(root)
        }
    };
    if !tail.valid {
        return;
    }
    let asset = &staged.book.assets[asset_id];
    let (asset_place, asset_owner, asset_unit) = (asset.place, asset.owner, asset.unit);
    let unknown = staged.book.entities[staged.book.roots.unknown].place;
    let Some(unknown) = unknown else {
        diags.push(
            Diagnostic::error("basis-source", "the unknown party has no flow endpoint")
                .label(loc, "cannot record this asset's arrival"),
        );
        return;
    };
    let empty = Run::new(Id::new(0), 0);
    let from = ResolvedEnd { place: unknown, entity: Some(staged.book.roots.unknown), select: empty };
    let to = ResolvedEnd { place: asset_place, entity: None, select: empty };
    let quantity = Amount::new(Qty(1), asset_unit);
    let no_local_codes = empty_codes(&staged);
    let Some(mut flow) = make_resolved_flow(
        &mut staged,
        statement.date,
        from,
        to,
        quantity,
        quantity,
        Infer::Known,
        Mode::Actual,
        tail,
        header_codes,
        no_local_codes,
        txn_id,
        loc,
        diags,
    ) else {
        return;
    };
    flow.owner = asset_owner;
    let waive = flow.waive;
    staged.book.flows.push(flow);
    let mut flow_roots = Vec::new();
    push_flow_expressions(&mut flow_roots, 0, None, None, basis_root);
    if diags.len() != diagnostic_start {
        return;
    }
    let program_id = (!program.nodes.is_empty() || !flow_roots.is_empty()).then(|| {
        staged.book.journal_programs.push(JournalProgram {
            program,
            flow_roots: flow_roots.into_boxed_slice(),
            groups: Box::default(),
        })
    });
    let txn = Txn { program: program_id, codes: header_codes, waive, ..journal_txn(&staged, statement.date, loc) };
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
    if template.flow.out.unit == base || template.flow.arrive.unit == base { base } else { template.flow.arrive.unit }
}

fn template_side(world: &World<'_>, template: &crate::book::TemplateFlow) -> FlowSide {
    if let Some(leg) = template.legs.first() {
        return leg.side;
    }
    if world.book.places[template.flow.from].class != crate::book::Class::Outside {
        FlowSide::Arrive
    } else {
        FlowSide::Out
    }
}

fn occurrence_group_draft(
    template_at: usize,
    template: &crate::book::TemplateFlow,
    side: FlowSide,
) -> OccurrenceGroupDraft {
    let common = match side {
        FlowSide::Out => template.flow.to,
        FlowSide::Arrive => template.flow.from,
    };
    OccurrenceGroupDraft {
        template: template_at as u32,
        source: JournalEnd { place: common, entity: None },
        side,
        legs: Vec::new(),
        leg_quantities: Vec::new(),
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
    template: &crate::book::TemplateFlow,
    side: FlowSide,
) -> (ResolvedEnd, ResolvedEnd, Id<Place>) {
    let common = match side {
        FlowSide::Out => template.flow.to,
        FlowSide::Arrive => template.flow.from,
    };
    let remainder = template.legs.first().map_or_else(
        || match side {
            FlowSide::Out => template.flow.from,
            FlowSide::Arrive => template.flow.to,
        },
        |leg| match leg.side {
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

fn lower_contract_change<'a, 's>(
    world: &mut World<'s>,
    site: &Site<'a, 's>,
    loc: Loc,
    statement: &ast::Statement<'s>,
    diags: &mut Vec<Diagnostic>,
) {
    let file = &site.source.file;
    let Subject::Name(name) = statement.subject else {
        unsupported_statement(loc, "only a contract occurrence can be waived here", diags);
        return;
    };
    let sym = world.book.names.intern(name.0);
    let Some(contract_id) = world.book.lookup.contracts.get(&sym).copied() else {
        unsupported_statement(loc, "waiving a claim is not yet lowered natively", diags);
        return;
    };
    if !statement.body.items.is_empty() || !statement.body.legs.is_empty() {
        unsupported_statement(loc, "a contract waiver cannot carry recovery lines yet", diags);
        return;
    }

    // The parser takes one `until` and one description per statement, and no clause a waiver has no use for;
    // only codes may be repeated.
    let mut last = statement.date;
    let mut code = None;
    let mut description = None;
    for clause in &file[statement.tail] {
        match clause.kind {
            ClauseKind::Until(until) => last = until,
            ClauseKind::Code(written) => {
                if let Some((_, first)) = code {
                    diags.push(problem::twice("waiver code", clause.at, first));
                    return;
                }
                code = Some((written, clause.at));
            }
            ClauseKind::Description(text) => description = Some(world.book.quoted_text(text.0)),
            ClauseKind::Purpose(_) => {
                unsupported_statement(loc, "a contract waiver has no claim-recovery purpose", diags);
                return;
            }
            other => unreachable!("the parser keeps {other:?} off a waiver"),
        }
    }
    let Some(days) = Days::new(statement.date, last) else {
        diags.push(
            Diagnostic::error("waiver-span", "a waiver ends before it begins")
                .label(loc, "the `until` day must be on or after this day"),
        );
        return;
    };
    let change =
        BookChange { days, description, code: code.map(|(code, _)| world.book.names.intern(code.name())), loc };
    let contract = &mut world.book.contracts[contract_id];
    let mut painted = false;
    if let Some(terms) = contract.terms.as_mut() {
        let mut waived = terms.at(statement.date).clone();
        waived.state = TermsState::Waived;
        waived.change = Some(change);
        terms.paint(days, waived);
        painted = true;
    }
    if let Some(terms) = contract.standing.as_mut() {
        let mut waived = terms.at(statement.date).clone();
        waived.state = TermsState::Waived;
        waived.change = Some(change);
        terms.paint(days, waived);
        painted = true;
    }
    if !painted {
        diags.push(
            Diagnostic::error("waiver-without-schedule", "this contract has no schedule to waive")
                .label(loc, "there is no regular or standing occurrence here"),
        );
    }
}

fn lower_claim_change<'a, 's>(
    world: &mut World<'s>,
    site: &Site<'a, 's>,
    loc: Loc,
    statement: &ast::Statement<'s>,
    code: ast::Code<'s>,
    code_index: &CodeIndex,
    diags: &mut Vec<Diagnostic>,
) {
    let file = &site.source.file;
    if !statement.body.legs.is_empty() || !statement.body.items.is_empty() {
        unsupported_statement(loc, "a full claim write-off cannot include recovery lines", diags);
        return;
    }

    let mut description = None;
    for clause in &file[statement.tail] {
        match clause.kind {
            // The parser takes one description per statement.
            ClauseKind::Description(text) => description = Some(world.book.quoted_text(text.0)),
            _ => {
                unsupported_statement(loc, "a full claim write-off only accepts a description", diags);
                return;
            }
        }
    }

    let reference_loc = file.loc(code.name());
    let Some(target) = code_index.resolve(world, code, reference_loc, CodeUse::ClaimWaiver, diags) else {
        return;
    };
    let source = &world.book.txns[target];
    let has_claim_flow = source.flows.ids().any(|flow_id| {
        let flow = &world.book.flows[flow_id];
        matches!(world.book.places[flow.from].role, crate::book::Role::Tab(_))
            || matches!(world.book.places[flow.to].role, crate::book::Role::Tab(_))
    });
    if !has_claim_flow {
        diags.push(
            Diagnostic::error("claim-writeoff-target", "this transaction did not create an open claim")
                .label(reference_loc, "the referenced transaction has no claim flow")
                .context(source.loc, "the transaction identified by this code is here")
                .help("use the code on an earlier `owes` statement"),
        );
        return;
    }
    if statement.date < source.day {
        diags.push(
            Diagnostic::error("claim-writeoff-date", "a claim cannot be waived before it exists")
                .label(loc, "this date precedes the claim transaction"),
        );
        return;
    }

    world.book.claim_changes.push(ClaimChange {
        day: statement.date,
        target,
        action: ClaimChangeAction::WriteOff,
        description,
        loc,
    });
}

fn lower_end<'a, 's>(
    world: &mut World<'s>,
    site: &Site<'a, 's>,
    loc: Loc,
    statement: &ast::Statement<'s>,
    diags: &mut Vec<Diagnostic>,
) {
    let file = &site.source.file;
    if !statement.body.legs.is_empty() || !statement.body.items.is_empty() {
        unsupported_statement(loc, "an ending cannot carry journal lines", diags);
        return;
    }
    // The parser takes one description per statement, codes any number of times, and nothing else on an ending.
    let mut description = None;
    for clause in &file[statement.tail] {
        match clause.kind {
            ClauseKind::Code(_) => {}
            ClauseKind::Description(text) => description = Some(text),
            other => unreachable!("the parser keeps {other:?} off an ending"),
        }
    }
    let Subject::Name(name) = statement.subject else {
        unsupported_statement(loc, "this subject cannot end here", diags);
        return;
    };
    let sym = world.book.names.intern(name.0);
    let target = if let Some(contract_id) = world.book.lookup.contracts.get(&sym).copied() {
        let contract = &world.book.contracts[contract_id];
        if statement.date < contract.days.first() {
            diags.push(
                Diagnostic::error("end-before-contract", "a contract cannot end before it begins")
                    .label(loc, "this date precedes the contract's first day"),
            );
            return;
        }
        EndTarget::Contract(contract_id)
    } else if let Some(asset) = world.book.asset(name.0) {
        EndTarget::Asset(asset)
    } else {
        match world.end(site.home, Word::of(file, name.0)) {
            Ok(end) => match world.book.places[end.place].role {
                crate::book::Role::Asset(asset) => EndTarget::Asset(asset),
                _ => EndTarget::Place(end.place),
            },
            Err(problem) => {
                diags.push(problem);
                return;
            }
        }
    };
    let codes_start = world.book.codes.len();
    for clause in &file[statement.tail] {
        if let ClauseKind::Code(code) = clause.kind {
            world.book.codes.push(world.book.names.intern(code.name()));
        }
    }
    let event = EndEvent {
        day: statement.date,
        target,
        codes: Run::new(Id::new(codes_start as u32), (world.book.codes.len() - codes_start) as u32),
        description: description.map(|text| world.book.quoted_text(text.0)),
        loc,
    };

    match target {
        EndTarget::Contract(contract_id) => {
            let contract = &mut world.book.contracts[contract_id];
            let last = statement.date.min(contract.days.last());
            if let Some(days) = Days::new(contract.days.first(), last) {
                contract.days = days;
                contract.ended = Some(loc);
            }
        }
        EndTarget::Place(place) => world.book.places[place].closed = Some(statement.date),
        EndTarget::Asset(asset) => {
            let place = world.book.assets[asset].place;
            world.book.places[place].closed = Some(statement.date);
        }
    }
    world.book.endings.push(event);
}

fn lower_filed<'a, 's>(
    world: &mut World<'s>,
    site: &Site<'a, 's>,
    loc: Loc,
    statement: &ast::Statement<'s>,
    year: i32,
    diags: &mut Vec<Diagnostic>,
) {
    let file = &site.source.file;
    let Subject::Name(system_name) = statement.subject else {
        unsupported_statement(loc, "a return needs a system subject", diags);
        return;
    };
    let Some(system) = world.system(Word::of(file, system_name.0)).or_report(diags) else {
        return;
    };
    let fallback = world.book.systems[system].currency.unwrap_or(world.book.base);
    let start = diags.len();
    let mut lines = Vec::with_capacity(statement.body.legs.len());
    for line in &file[statement.body.legs] {
        let Quantity::Amount(ast::Amount::Literal(literal)) = line.amount else {
            diags.push(
                Diagnostic::error("filed-amount", "a filed tally needs a literal amount")
                    .label(line.loc, "write the amount as reported"),
            );
            continue;
        };
        let Some(amount) = world.literal_amount(file, literal, Some(fallback)).or_report(diags) else {
            continue;
        };
        lines.push((world.book.names.intern(line.end.name.0), amount, line.loc));
    }
    if diags.len() != start || lines.len() != statement.body.legs.len() {
        return;
    }
    world.book.filed.push(Filed {
        day: statement.date,
        system,
        year,
        owner: world.book.roots.me,
        lines: lines.into_boxed_slice(),
        loc,
    });
}

#[derive(Clone, Copy)]
enum StatementTarget {
    Place(Id<Place>),
    Entity(Id<crate::book::Entity>),
    Asset(Id<crate::book::Asset>),
    Unit(Id<crate::book::Commodity>),
    Code(axiom_core::Sym),
    Purpose,
}

fn statement_target<'s>(
    world: &mut World<'s>,
    home: Home,
    file: &ast::File<'s>,
    subject: Subject<'s>,
    diags: &mut Vec<Diagnostic>,
) -> Option<StatementTarget> {
    match subject {
        Subject::Name(name) => {
            if let Some(asset) = world.book.asset(name.0) {
                return Some(StatementTarget::Asset(asset));
            }
            if let Some(contract) = world.book.contract(name.0)
                && let Some(loan) = world.book.contracts[contract].loan
            {
                // A loan contract's name denotes its debt position in a
                // balance assertion, not the lender entity that resolves as
                // its ordinary flow endpoint.
                return Some(StatementTarget::Place(loan.debt));
            }
            let word = Word::of(file, name.0);
            match world.end(home, word) {
                Ok(end) => {
                    if let Some(entity) = end.entity {
                        Some(StatementTarget::Entity(entity))
                    } else {
                        match world.book.places[end.place].role {
                            crate::book::Role::Asset(asset) => Some(StatementTarget::Asset(asset)),
                            _ => Some(StatementTarget::Place(end.place)),
                        }
                    }
                }
                Err(problem) => {
                    diags.push(problem);
                    None
                }
            }
        }
        Subject::Code(code) => Some(StatementTarget::Code(world.book.names.intern(code.name()))),
        Subject::Purpose(name) => {
            world.purpose(home, Word::of(file, name.0)).map(|_| StatementTarget::Purpose).or_report(diags)
        }
        Subject::Unit(name) => world.commodity_of(Word::of(file, name.0)).map(StatementTarget::Unit).or_report(diags),
    }
}

fn lower_value<'s>(
    world: &mut World<'s>,
    home: Home,
    file: &ast::File<'s>,
    loc: Loc,
    statement: &ast::Statement<'s>,
    value: ast::Amount<'s>,
    diags: &mut Vec<Diagnostic>,
) {
    let target = statement_target(world, home, file, statement.subject, diags);
    let Some(target) = target else { return };
    match target {
        StatementTarget::Place(place) => {
            let fallback = match world.book.places[place].holds.as_deref() {
                Some([unit]) => *unit,
                _ => world.book.base,
            };
            let Some(gap) = assertion_gap(world, home, file, statement, diags) else {
                return;
            };
            let Some((amount, computed)) = assertion_amount(world, home, file, value, fallback, Ty::Place, diags)
            else {
                return;
            };
            world.book.asserts.push(Assert {
                day: statement.date,
                place,
                subject: ModelSubject::Place(place),
                amount,
                computed,
                gap,
                loc,
            });
        }
        StatementTarget::Asset(asset) => {
            let place = world.book.assets[asset].place;
            let Some(gap) = assertion_gap(world, home, file, statement, diags) else {
                return;
            };
            let Some((amount, computed)) =
                assertion_amount(world, home, file, value, world.book.base, Ty::Asset, diags)
            else {
                return;
            };
            world.book.asserts.push(Assert {
                day: statement.date,
                place,
                subject: ModelSubject::Asset(asset),
                amount,
                computed,
                gap,
                loc,
            });
        }
        StatementTarget::Code(code) => {
            let ast::Amount::Literal(literal) = value else {
                unsupported_computed_value(loc, "a named measure reading", diags);
                return;
            };
            let Some(amount) = world.literal_amount(file, literal, Some(world.book.base)).or_report(diags) else {
                return;
            };
            world.book.readings.push(Reading { day: statement.date, code, amount, loc });
        }
        StatementTarget::Unit(unit) => {
            let ast::Amount::Literal(literal) = value else {
                unsupported_computed_value(loc, "a price quote", diags);
                return;
            };
            let Some(quote_name) = literal.unit() else {
                diags.push(
                    Diagnostic::error("price-unit", "a price needs a quoted commodity")
                        .label(loc, "write `VTI = 285.70 USD`"),
                );
                return;
            };
            let Some(quote) = world.commodity_of(Word::of(file, quote_name.0)).or_report(diags) else {
                return;
            };
            let Some(rate) = literal.num().to_ratio().filter(|rate| *rate > axiom_core::Ratio::ZERO) else {
                diags.push(
                    Diagnostic::error("price-zero", "a price must be greater than zero")
                        .label(file.loc(literal.0), "this price is not positive"),
                );
                return;
            };
            world.book.prices.quotes.push(Quote { unit, quote, day: statement.date, rate, implied: false, loc });
        }
        StatementTarget::Entity(_) | StatementTarget::Purpose => {
            unsupported_statement(loc, "a value needs an account, asset, code or commodity subject", diags)
        }
    }
}

fn assertion_amount<'s>(
    world: &mut World<'s>,
    home: Home,
    file: &ast::File<'s>,
    value: ast::Amount<'s>,
    fallback: Id<crate::book::Commodity>,
    subject: Ty,
    diags: &mut Vec<Diagnostic>,
) -> Option<(Amount, Option<(Id<crate::book::TemplateProgram>, NodeId)>)> {
    match value {
        ast::Amount::Literal(literal) => {
            world.literal_amount(file, literal, Some(fallback)).or_report(diags).map(|amount| (amount, None))
        }
        ast::Amount::Computed(root) => {
            let name = world.book.names.intern("assertion");
            let (program, roots) =
                crate::laws::compile_template(world, diags, file, home, subject, name, &[], &[(root, Ty::AMOUNT)])?;
            let [root] = roots.as_ref() else {
                return None;
            };
            let unit = match program.nodes[*root].typed_ty() {
                Some(Ty::Amount(Dim::Of(unit))) => unit,
                _ => fallback,
            };
            let program = world.book.assertion_programs.push(program);
            Some((Amount::zero(unit), Some((program, *root))))
        }
    }
}

fn unsupported_computed_value(loc: Loc, subject: &str, diags: &mut Vec<Diagnostic>) {
    diags.push(
        Diagnostic::error("computed-value-subject", format!("computed values are not supported for {subject}"))
            .label(loc, "write a literal amount here"),
    );
}

fn assertion_gap<'s>(
    world: &mut World<'s>,
    home: Home,
    file: &ast::File<'s>,
    statement: &ast::Statement<'s>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Gap> {
    let mut gap = Gap::Refused;
    for clause in &file[statement.tail] {
        match clause.kind {
            ClauseKind::Via(name) => match world.end(home, Word::of(file, name.0)) {
                Ok(end) => gap = Gap::Via { place: end.place, loc: clause.at },
                Err(problem) => {
                    diags.push(problem);
                    return None;
                }
            },
            ClauseKind::Waive(waive) => {
                gap = Gap::Unexplained(Waive {
                    loc: waive.at,
                    reason: waive.reason.map(|reason| world.book.quoted_text(reason.0)),
                });
            }
            ClauseKind::Description(_) | ClauseKind::Code(_) => {}
            other => unreachable!("the parser keeps {other:?} off a value"),
        }
    }
    Some(gap)
}

fn lower_measure<'s>(
    world: &mut World<'s>,
    home: Home,
    file: &ast::File<'s>,
    loc: Loc,
    statement: &ast::Statement<'s>,
    literal: ast::Literal<'s>,
    action: Action,
    code_index: &CodeIndex,
    diags: &mut Vec<Diagnostic>,
) {
    let Some(target) = statement_target(world, home, file, statement.subject, diags) else {
        return;
    };
    let (subject, owner) = match target {
        StatementTarget::Place(place) => (ModelSubject::Place(place), world.book.places[place].owner),
        StatementTarget::Entity(entity) => (ModelSubject::Entity(entity), entity),
        StatementTarget::Asset(asset) => (ModelSubject::Asset(asset), world.book.assets[asset].owner),
        _ => {
            unsupported_statement(loc, "a measure needs a named entity, place or asset", diags);
            return;
        }
    };
    let Some(quantity) = world.literal_amount(file, literal, None).or_report(diags) else {
        return;
    };
    let mut party = None;
    let mut purpose = None;
    let mut description = None;
    let mut codes = Vec::new();
    let mut against = None;
    let diagnostic_start = diags.len();
    for clause in &file[statement.tail] {
        match clause.kind {
            ClauseKind::For(ast::For::Whom(name)) => match world.entity(home, Word::of(file, name.0)) {
                Ok(entity) => party = Some(entity),
                Err(problem) => diags.push(problem),
            },
            ClauseKind::Purpose(written) => {
                let purpose_id = world.purpose(home, Word::of(file, written.name.0));
                match purpose_id {
                    Ok(purpose_id) => {
                        let of = written.of.and_then(|name| resolve_object(world, home, file, name, diags));
                        purpose = Some(Purposed { purpose: purpose_id, of, source: Provenance::Written });
                    }
                    Err(problem) => diags.push(problem),
                }
            }
            ClauseKind::Description(text) => {
                description = Some(world.book.quoted_text(text.0));
            }
            ClauseKind::Code(code) => codes.push(world.book.names.intern(code.name())),
            ClauseKind::Against(code) => {
                against = code_index.resolve(world, code, clause.at, CodeUse::Against, diags);
            }
            _ => diags.push(
                Diagnostic::error("measure-tail", "this tail clause does not apply to a measure")
                    .label(clause.at, "remove the clause or record it on a flow"),
            ),
        }
    }
    if diags.len() != diagnostic_start {
        return;
    }
    world.book.measures.push(Measure {
        day: statement.date,
        action,
        subject,
        quantity,
        owner,
        party,
        purpose,
        description,
        codes: codes.into_boxed_slice(),
        against,
        loc,
    });
}

fn unsupported_statement(loc: Loc, message: &str, diags: &mut Vec<Diagnostic>) {
    diags.push(
        Diagnostic::error("statement-lowering", message).label(loc, "this record is not included in the Book yet"),
    );
}

fn flow_roots<'s>(file: &ast::File<'s>, flow: &ast::Flow<'s>) -> Vec<(ast::ExprId, Ty)> {
    let mut roots = Vec::new();
    push_tail_roots(file, flow.tail, &mut roots);
    for quantity in [flow.from.amount, flow.to.amount].into_iter().flatten() {
        push_quantity_root(quantity, &mut roots);
    }
    for leg in &file[flow.body.legs] {
        push_quantity_root(leg.amount, &mut roots);
        push_tail_roots(file, leg.tail, &mut roots);
    }
    for item in &file[flow.body.items] {
        push_amount_root(item.amount, &mut roots);
        push_tail_roots(file, item.tail, &mut roots);
    }
    roots
}

fn push_tail_roots<'s>(file: &ast::File<'s>, clauses: ast::Many<ast::Clause<'s>>, roots: &mut Vec<(ast::ExprId, Ty)>) {
    for clause in &file[clauses] {
        if let ClauseKind::Basis(ast::Amount::Computed(expr)) = clause.kind {
            roots.push((expr, Ty::AMOUNT));
        }
    }
}

fn push_quantity_root<'s>(quantity: Quantity<'s>, roots: &mut Vec<(ast::ExprId, Ty)>) {
    match quantity {
        Quantity::Amount(amount) | Quantity::Pending(amount) | Quantity::Target(amount) => {
            push_amount_root(amount, roots)
        }
        Quantity::Unknown(_) | Quantity::All(_) | Quantity::Rest | Quantity::Whole => {}
    }
}

fn resolve_quantity<'s>(
    world: &mut World<'s>,
    file: &ast::File<'s>,
    quantity: Quantity<'s>,
    fallback: Id<crate::book::Commodity>,
    side: crate::book::FlowSide,
    roots: &Map<ast::ExprId, NodeId>,
    diags: &mut Vec<Diagnostic>,
) -> Option<ResolvedQuantity> {
    // A literal that is no amount costs the whole quantity and says nothing: the diagnostic is dropped here, as it
    // always was, and is not the caller's to report.
    let resolve_literal = |world: &World<'s>, literal: ast::Literal<'s>| -> Option<Amount> {
        world.literal_amount(file, literal, Some(fallback)).ok()
    };
    let resolved = match quantity {
        Quantity::Amount(amount) => {
            let (amount, root) = match amount {
                ast::Amount::Literal(literal) => (resolve_literal(world, literal)?, None),
                ast::Amount::Computed(expr) => (Amount::zero(fallback), Some(*roots.get(&expr)?)),
            };
            ResolvedQuantity {
                amount,
                infer: Infer::Known,
                mode: Mode::Actual,
                root,
                group: JournalQuantity::Amount(amount, root),
            }
        }
        Quantity::Pending(amount) => {
            let (amount, root) = match amount {
                ast::Amount::Literal(literal) => (resolve_literal(world, literal)?, None),
                ast::Amount::Computed(expr) => (Amount::zero(fallback), Some(*roots.get(&expr)?)),
            };
            ResolvedQuantity {
                amount,
                infer: Infer::Known,
                mode: Mode::Pending,
                root,
                group: JournalQuantity::Pending(amount, root),
            }
        }
        Quantity::Target(amount) => {
            let (amount, root) = match amount {
                ast::Amount::Literal(literal) => (resolve_literal(world, literal)?, None),
                ast::Amount::Computed(expr) => (Amount::zero(fallback), Some(*roots.get(&expr)?)),
            };
            ResolvedQuantity {
                amount,
                infer: Infer::Target {
                    end: if side == FlowSide::Out { crate::journal::End::From } else { crate::journal::End::To },
                    balance: amount.qty,
                },
                mode: Mode::Actual,
                root,
                group: JournalQuantity::Target(amount, root),
            }
        }
        Quantity::Unknown(unit) => {
            let unit = world.commodity_of(Word::of(file, unit.0)).or_report(diags)?;
            let amount = Amount::zero(unit);
            ResolvedQuantity {
                amount,
                infer: Infer::Unknown,
                mode: Mode::Actual,
                root: None,
                group: JournalQuantity::Unknown(unit),
            }
        }
        Quantity::All(unit) => {
            let unit = match unit {
                Some(unit) => Some(world.commodity_of(Word::of(file, unit.0)).or_report(diags)?),
                None => None,
            };
            let amount = Amount::zero(unit.unwrap_or(fallback));
            ResolvedQuantity {
                amount,
                infer: Infer::All,
                mode: Mode::Actual,
                root: None,
                group: JournalQuantity::All(unit),
            }
        }
        Quantity::Rest => ResolvedQuantity {
            amount: Amount::zero(fallback),
            infer: Infer::Known,
            mode: Mode::Actual,
            root: None,
            group: JournalQuantity::Rest,
        },
        Quantity::Whole => ResolvedQuantity {
            amount: Amount::new(Qty(1), fallback),
            infer: Infer::Known,
            mode: Mode::Opening,
            root: None,
            group: JournalQuantity::Whole,
        },
    };
    Some(resolved)
}

fn make_flow<'s>(
    world: &mut World<'s>,
    file: &ast::File<'s>,
    day: Day,
    from: ResolvedEnd,
    to: ResolvedEnd,
    out: Option<Quantity<'s>>,
    arrive: Option<Quantity<'s>>,
    mut tail: Tail,
    header_codes: Run<axiom_core::Sym>,
    txn: Id<Txn>,
    loc: Loc,
    roots: &Map<ast::ExprId, NodeId>,
    diags: &mut Vec<Diagnostic>,
) -> Option<(Flow, Option<FlowExpressions>)> {
    let out =
        out.and_then(|quantity| resolve_quantity(world, file, quantity, world.book.base, FlowSide::Out, roots, diags));
    let arrive = arrive.and_then(|quantity| {
        resolve_quantity(
            world,
            file,
            quantity,
            out.map_or(world.book.base, |q| q.amount.unit),
            FlowSide::Arrive,
            roots,
            diags,
        )
    });
    let (out_amount, arrive_amount, infer, mode) = match (out, arrive) {
        (None, None) => {
            diags.push(
                Diagnostic::error("flow-amount", "a flow needs an amount or an inference marker")
                    .label(loc, "no quantity is stated"),
            );
            return None;
        }
        (Some(a), Some(b)) => {
            let infer = if !matches!(a.infer, Infer::Known) { a.infer } else { b.infer };
            (
                a.amount,
                b.amount,
                infer,
                if a.mode == Mode::Pending || b.mode == Mode::Pending { Mode::Pending } else { Mode::Actual },
            )
        }
        (Some(a), None) => (a.amount, a.amount, a.infer, a.mode),
        (None, Some(b)) => (b.amount, b.amount, b.infer, b.mode),
    };
    if let (Some(out), Some(arrive)) = (out, arrive)
        && out.root.is_none()
        && arrive.root.is_none()
        && out.amount.unit == arrive.amount.unit
        && out.amount.qty != arrive.amount.qty
    {
        diags.push(
            Diagnostic::error("flow-amount-mismatch", "a transfer has the same amount at both ends")
                .label(loc, "the two written amounts differ"),
        );
        return None;
    }
    let root_exprs = (out.and_then(|q| q.root), arrive.and_then(|q| q.root));
    let basis_root = tail.basis_root;
    let no_local_codes = empty_codes(world);
    if let Some((rate, quote, at)) = tail.price {
        let quoted = match (out, arrive) {
            (Some(out), None) if out.root.is_none() => Some(priced(world, out.amount, quote, rate, at, diags)?),
            (None, Some(arrive)) if arrive.root.is_none() => {
                Some(priced(world, arrive.amount, quote, rate, at, diags)?)
            }
            (Some(out), Some(arrive)) if out.root.is_none() && arrive.root.is_none() => {
                let expected = if out.amount.unit == quote {
                    priced(world, arrive.amount, quote, rate, at, diags)?
                } else if arrive.amount.unit == quote {
                    priced(world, out.amount, quote, rate, at, diags)?
                } else {
                    diags.push(
                        Diagnostic::error("price-unit", "the stated price unit must match one side of the flow")
                            .label(at, "the quote unit appears on neither side"),
                    );
                    return None;
                };
                let actual = if out.amount.unit == quote { out.amount } else { arrive.amount };
                if expected != actual {
                    diags.push(
                        Diagnostic::error("price-disagrees", "the stated price does not match the flow amounts")
                            .label(at, "this price implies a different amount")
                            .label(loc, "the written quantities disagree with the price"),
                    );
                    return None;
                }
                None
            }
            _ => {
                diags.push(
                    Diagnostic::error("price-shape", "a written price needs a literal quantity")
                        .label(at, "this price cannot be applied to a computed or missing amount")
                        .help("write one literal quantity and let the price determine the other side"),
                );
                return None;
            }
        };
        tail.price = None;
        let (out_amount, arrive_amount) = match (out, arrive, quoted) {
            (Some(out), None, Some(arrive)) => (out.amount, arrive),
            (None, Some(arrive), Some(out)) => (out, arrive.amount),
            (Some(out), Some(arrive), None) => (out.amount, arrive.amount),
            _ => return None,
        };
        return make_resolved_flow(
            world,
            day,
            from,
            to,
            out_amount,
            arrive_amount,
            infer,
            mode,
            tail,
            header_codes,
            no_local_codes,
            txn,
            loc,
            diags,
        )
        .map(|flow| {
            (
                flow,
                (root_exprs.0.is_some() || root_exprs.1.is_some() || basis_root.is_some()).then_some(FlowExpressions {
                    flow: 0,
                    out: root_exprs.0,
                    arrive: root_exprs.1,
                    basis: basis_root,
                }),
            )
        });
    }
    let flow = make_resolved_flow(
        world,
        day,
        from,
        to,
        out_amount,
        arrive_amount,
        infer,
        mode,
        tail,
        header_codes,
        no_local_codes,
        txn,
        loc,
        diags,
    )?;
    let expressions = (root_exprs.0.is_some() || root_exprs.1.is_some() || basis_root.is_some())
        .then_some(FlowExpressions { flow: 0, out: root_exprs.0, arrive: root_exprs.1, basis: basis_root });
    Some((flow, expressions))
}

fn make_resolved_flow(
    world: &mut World<'_>,
    day: Day,
    from: ResolvedEnd,
    to: ResolvedEnd,
    out: Amount,
    arrive: Amount,
    infer: Infer,
    mode: Mode,
    tail: Tail,
    header_codes: Run<axiom_core::Sym>,
    local_codes: Run<axiom_core::Sym>,
    txn: Id<Txn>,
    loc: Loc,
    diags: &mut Vec<Diagnostic>,
) -> Option<Flow> {
    let purpose = infer_for_flow(
        world,
        from.place,
        from.entity,
        to.place,
        to.entity,
        tail.purpose.map(|purpose| (purpose, tail.purpose_loc.unwrap_or(loc))),
        loc,
        diags,
    )
    .ok()?;
    let mut detail = tail.detail;
    detail.spender = from.entity;
    let detail = (detail != Detail::NONE).then(|| world.book.details.push(detail));
    if to.select.len() != 0 {
        diags.push(
            Diagnostic::error("selector-target", "selectors narrow the source endpoint of a flow")
                .label(loc, "this endpoint only receives"),
        );
        return None;
    }
    let select = from.select;
    let from_place = &world.book.places[from.place];
    let to_place = &world.book.places[to.place];
    let owner = if from_place.class != crate::book::Class::Outside {
        from_place.owner
    } else if to_place.class != crate::book::Class::Outside {
        to_place.owner
    } else {
        from_place.owner
    };
    let payee = tail.payee.or(to.entity).or(from.entity);
    let recognized = tail.recognized.unwrap_or(Days::on(day));
    Some(Flow {
        day,
        recognized,
        from: from.place,
        to: to.place,
        out,
        arrive,
        mode,
        infer,
        txn,
        payee,
        owner,
        purpose,
        description: tail.description,
        origin: Origin::Written,
        select,
        header_codes,
        codes: local_codes,
        loc,
        waive: tail.waive,
        detail,
    })
}

/// The party at an endpoint classifies flows through its own purpose or the
/// applicable purpose of its kind. A commodity issuer contributes `pays`;
/// an account recipient can transform that source through `takes` below.
fn endpoint_purpose(
    world: &World<'_>,
    place: Id<Place>,
    named_entity: Option<Id<crate::book::Entity>>,
    source: bool,
) -> Option<PurposeEvidence> {
    if let crate::book::Role::Issuer(unit) = world.book.places[place].role {
        if !source {
            return None;
        }
        let mut kind = world.book.commodities[unit].kind;
        let pays = world.book.kinds[kind].pays?;
        while let Some(parent) = world.book.kinds.parent(kind) {
            if world.book.kinds[parent].pays == Some(pays) {
                kind = parent;
            } else {
                break;
            }
        }
        return Some(PurposeEvidence {
            purposed: Purposed { purpose: pays.value, of: None, source: Provenance::Commodity(kind) },
            loc: pays.loc,
        });
    }

    let role_entity = match world.book.places[place].role {
        crate::book::Role::Outside(Some(entity)) | crate::book::Role::Tab(entity) => Some(entity),
        _ => None,
    };
    let entity = named_entity.or(role_entity)?;
    let party = &world.book.entities[entity];
    let kind_id = party.kind;
    let kind = &world.book.kinds[kind_id];
    if let Some(purpose) = party.purpose {
        // The declaration builder may carry an inherited kind value on an
        // entity. Preserve its true provenance so explanations name the kind.
        if kind.purpose != Some(purpose) && kind.pays != Some(purpose) {
            return Some(PurposeEvidence {
                purposed: Purposed { purpose: purpose.value, of: None, source: Provenance::Entity(entity) },
                loc: purpose.loc,
            });
        }
    }

    let purpose = if source { kind.pays.or(kind.purpose) } else { kind.purpose }?;
    Some(PurposeEvidence {
        purposed: Purposed { purpose: purpose.value, of: None, source: Provenance::Party(party.kind) },
        loc: purpose.loc,
    })
}

/// Resolve purpose sources once for ordinary and contract flow templates. The
/// returned purpose is absent when no source classifies the flow; conflicting
/// sources return an error and retain both declaration locations.
pub(super) fn infer_for_flow(
    world: &World<'_>,
    from: Id<Place>,
    from_entity: Option<Id<crate::book::Entity>>,
    to: Id<Place>,
    to_entity: Option<Id<crate::book::Entity>>,
    written: Option<(Purposed, Loc)>,
    loc: Loc,
    diags: &mut Vec<Diagnostic>,
) -> Result<Option<Purposed>, ()> {
    let from_purpose = endpoint_purpose(world, from, from_entity, true);
    let to_purpose = endpoint_purpose(world, to, to_entity, false);
    if let (Some(from), Some(to)) = (from_purpose, to_purpose)
        && !same_purpose(world, from.purposed, to.purposed)
    {
        diags.push(purpose_disagreement(world, loc, from, to));
        return Err(());
    }
    let inferred = from_purpose.or(to_purpose).map(|source| taken_purpose(world, to, source).unwrap_or(source));
    if let (Some((written, written_loc)), Some(inferred)) = (written, inferred)
        && !same_purpose(world, written, inferred.purposed)
    {
        diags.push(purpose_disagreement(world, loc, PurposeEvidence { purposed: written, loc: written_loc }, inferred));
        return Err(());
    }
    Ok(written.map(|(purpose, _)| purpose).or(inferred.map(|source| source.purposed)))
}

fn taken_purpose(world: &World<'_>, destination: Id<Place>, source: PurposeEvidence) -> Option<PurposeEvidence> {
    if !matches!(world.book.places[destination].role, crate::book::Role::Account { .. }) {
        return None;
    }
    let kind_id = world.book.places[destination].kind;
    let take = world.book.kinds[kind_id].takes.iter().find(|take| take.value.from == source.purposed.purpose)?;
    Some(PurposeEvidence {
        purposed: Purposed { purpose: take.value.to, of: source.purposed.of, source: Provenance::Account(kind_id) },
        loc: take.loc,
    })
}

/// Ancestor and descendant purposes refine the same classification; sibling
/// purposes remain distinct even when they share a broad spending/income root.
fn same_purpose(world: &World<'_>, left: Purposed, right: Purposed) -> bool {
    let related = world.book.purposes.covers(left.purpose, right.purpose)
        || world.book.purposes.covers(right.purpose, left.purpose);
    let object_compatible = match (left.of, right.of) {
        (Some(left), Some(right)) => left == right,
        // An unqualified purpose carries no object fact to contradict an
        // explicit `of` target from another source.
        _ => true,
    };
    related && object_compatible
}

fn purpose_disagreement(world: &World<'_>, loc: Loc, first: PurposeEvidence, second: PurposeEvidence) -> Diagnostic {
    Diagnostic::error("purpose-disagreement", "this flow's purpose sources disagree")
        .label(first.loc, purpose_evidence_label(world, first))
        .label(second.loc, purpose_evidence_label(world, second))
        .label(loc, "these sources classify the same flow differently")
}

fn purpose_evidence_label(world: &World<'_>, evidence: PurposeEvidence) -> String {
    let purpose = world.book.name(world.book.purposes[evidence.purposed.purpose].name);
    match evidence.purposed.source {
        Provenance::Written => format!("the written purpose is `#{purpose}`"),
        Provenance::Contract(contract) => {
            format!("contract `{}` gives purpose `#{purpose}`", world.book.name(world.book.contracts[contract].name),)
        }
        Provenance::Entity(entity) => {
            format!("party `{}` gives purpose `#{purpose}`", world.book.name(world.book.entities[entity].path),)
        }
        Provenance::Party(kind) => {
            format!("party kind `{}` gives purpose `#{purpose}`", world.book.name(world.book.kinds[kind].name),)
        }
        Provenance::Commodity(kind) => {
            format!("commodity kind `{}` gives purpose `#{purpose}`", world.book.name(world.book.kinds[kind].name),)
        }
        Provenance::Account(kind) => {
            format!("account kind `{}` takes the flow as `#{purpose}`", world.book.name(world.book.kinds[kind].name),)
        }
        Provenance::Derived => format!("the derived flow has purpose `#{purpose}`"),
    }
}

fn lower_tail<'s>(
    world: &mut World<'s>,
    home: Home,
    file: &ast::File<'s>,
    clauses: ast::Many<ast::Clause<'s>>,
    day: Day,
    roots: &Map<ast::ExprId, NodeId>,
    code_index: &CodeIndex,
    diags: &mut Vec<Diagnostic>,
) -> (Run<axiom_core::Sym>, Tail) {
    let start = world.book.codes.len();
    let mut tail = Tail { valid: true, ..Tail::default() };
    for clause in &file[clauses] {
        match clause.kind {
            ClauseKind::Purpose(written) => {
                let purpose = world.purpose(home, Word::of(file, written.name.0));
                let of = written.of.and_then(|name| resolve_object(world, home, file, name, diags));
                match (purpose, written.of.is_some(), of) {
                    (Ok(purpose), false, _) | (Ok(purpose), true, Some(_)) => {
                        tail.purpose = Some(Purposed { purpose, of, source: Provenance::Written });
                        tail.purpose_loc = Some(clause.at);
                    }
                    (Err(problem), _, _) => {
                        diags.push(problem);
                        tail.valid = false;
                    }
                    (Ok(_), true, None) => tail.valid = false,
                }
            }
            ClauseKind::Description(text) => tail.description = Some(world.book.quoted_text(text.0)),
            ClauseKind::Code(code) => {
                let symbol = world.book.names.intern(code.name());
                world.book.codes.push(symbol);
            }
            ClauseKind::For(ast::For::Period(first, last)) => {
                tail.recognized = Days::new(first, last);
                if tail.recognized.is_none() {
                    tail.valid = false;
                }
            }
            ClauseKind::For(ast::For::Last(relative)) => {
                tail.recognized = Some(previous_period(day, relative));
            }
            ClauseKind::For(ast::For::Whom(name)) => match world.entity(home, Word::of(file, name.0)) {
                Ok(entity) => tail.detail.hold = Some(entity),
                Err(problem) => {
                    diags.push(problem);
                    tail.valid = false;
                }
            },
            ClauseKind::Due(due) => {
                tail.detail.due = Some(match due {
                    ast::Due::On(day) => day,
                    ast::Due::After(span) => day.add(span),
                });
            }
            ClauseKind::Via(name) => match world.entity(home, Word::of(file, name.0)) {
                Ok(entity) => tail.payee = Some(entity),
                Err(problem) => {
                    diags.push(problem);
                    tail.valid = false;
                }
            },
            ClauseKind::Basis(ast::Amount::Literal(literal)) => {
                let Some(unit) = literal.unit().and_then(|unit| world.commodity_of(Word::of(file, unit.0)).ok()) else {
                    diags.push(
                        Diagnostic::error("basis-unit", "basis needs an explicit base-currency unit")
                            .label(file.loc(literal.0), "write the unit"),
                    );
                    tail.valid = false;
                    continue;
                };
                match world.amount(literal.num(), unit, file.loc(literal.0)) {
                    Ok(amount) if amount.unit == world.book.base => tail.detail.basis = Some(amount.qty),
                    Ok(_) => {
                        diags.push(
                            Diagnostic::error("basis-unit", "basis must be stated in the base currency")
                                .label(file.loc(literal.0), "another unit is not the base currency"),
                        );
                        tail.valid = false;
                    }
                    Err(problem) => {
                        diags.push(problem);
                        tail.valid = false;
                    }
                }
            }
            ClauseKind::Basis(ast::Amount::Computed(expr)) => {
                if let Some(&root) = roots.get(&expr) {
                    tail.basis_root = Some(root);
                } else {
                    diags.push(
                        Diagnostic::error("computed-basis", "computed basis expression was not compiled for this flow")
                            .label(clause.at, "the basis expression is not available"),
                    );
                    tail.valid = false;
                }
            }
            ClauseKind::Price(literal) => {
                let Some(name) = literal.unit() else {
                    diags.push(
                        Diagnostic::error("price-unit", "a price needs a quoted commodity")
                            .label(file.loc(literal.0), "write `@ 285.70 USD`"),
                    );
                    tail.valid = false;
                    continue;
                };
                let Ok(unit) = world.commodity_of(Word::of(file, name.0)) else {
                    tail.valid = false;
                    continue;
                };
                let Some(rate) = literal.num().to_ratio().filter(|rate| !rate.is_zero()) else {
                    diags.push(
                        Diagnostic::error("price-zero", "a price must be greater than zero")
                            .label(file.loc(literal.0), "this price is zero"),
                    );
                    tail.valid = false;
                    continue;
                };
                tail.price = Some((rate, unit, clause.at));
            }
            ClauseKind::Since(day) => tail.detail.since = Some(day),
            ClauseKind::Against(code) => {
                tail.detail.against = code_index.resolve(world, code, clause.at, CodeUse::Against, diags);
                tail.valid &= tail.detail.against.is_some();
            }
            ClauseKind::Until(_) => {
                diags.push(
                    Diagnostic::error("until-position", "`until` is only valid on a statement change or waiver")
                        .label(clause.at, "it has no effect on a flow"),
                );
                tail.valid = false;
            }
            ClauseKind::Waive(waive) => {
                tail.waive =
                    Some(Waive { loc: waive.at, reason: waive.reason.map(|text| world.book.quoted_text(text.0)) });
            }
        }
    }
    (Run::new(Id::new(start as u32), (world.book.codes.len() - start) as u32), tail)
}

fn resolve_end<'s>(
    world: &mut World<'s>,
    home: Home,
    file: &ast::File<'s>,
    written: ast::End<'s>,
    diags: &mut Vec<Diagnostic>,
) -> Option<ResolvedEnd> {
    let word = Word::of(file, written.name.0);
    let Some(end) = world.end(home, word).or_report(diags) else {
        return None;
    };
    let start = world.book.selectors.len();
    for selector in &file[written.select] {
        let resolved = match *selector {
            ast::Select::Range(first, last, _) => Days::new(first, last).map(Select::Range),
            ast::Select::Code(code) => Some(Select::Code(world.book.names.intern(code.name()))),
            ast::Select::Policy(policy, _) => Some(Select::Policy(policy)),
            ast::Select::Purpose(name) => {
                world.purpose(home, Word::of(file, name.0)).or_report(diags).map(|id| Select::Purpose(id))
            }
            ast::Select::Unit(name) => {
                world.commodity_of(Word::of(file, name.0)).or_report(diags).map(|id| Select::Unit(id))
            }
            ast::Select::End(name) => {
                world.end(home, Word::of(file, name.0)).or_report(diags).map(|id| Select::End(id.place))
            }
        };
        match resolved {
            Some(select) => {
                world.book.selectors.push(select);
            }
            None => diags.push(
                Diagnostic::error("selector-range", "this selector does not name a valid range or target")
                    .label(file.loc(written.name.0), "invalid selector on this end"),
            ),
        }
    }
    Some(ResolvedEnd {
        place: end.place,
        entity: end.entity,
        select: Run::new(Id::new(start as u32), (world.book.selectors.len() - start) as u32),
    })
}

fn lower_items<'s>(
    staged: &mut Staged<'_, 's>,
    home: Home,
    file: &ast::File<'s>,
    items: ast::Many<ast::LineItem<'s>>,
    from: ResolvedEnd,
    to: ResolvedEnd,
    parent_side: crate::book::FlowSide,
    txn: Id<Txn>,
    day: Day,
    header_codes: Run<axiom_core::Sym>,
    roots: &Map<ast::ExprId, NodeId>,
    code_index: &CodeIndex,
    mode: Mode,
    inherited_tail: Option<&Tail>,
    flow_roots: &mut Vec<FlowExpressions>,
    diags: &mut Vec<Diagnostic>,
) -> Box<[JournalItem]> {
    let mut lowered = Vec::with_capacity(items.len());
    for item in &file[items] {
        let Some(amount) = resolve_amount(staged, file, item.amount, staged.book.base, roots, diags) else {
            continue;
        };
        let (local_codes, item_tail) = lower_tail(staged, home, file, item.tail, day, roots, code_index, diags);
        let mut tail = inherited_tail.cloned().unwrap_or(Tail { valid: true, ..Tail::default() });
        tail = merge_tail(tail, item_tail);
        let has_own_metadata = tail.purpose.is_some()
            || tail.description.is_some()
            || tail.detail != Detail::NONE
            || tail.basis_root.is_some()
            || tail.payee.is_some()
            || tail.recognized.is_some()
            || tail.price.is_some()
            || tail.waive.is_some()
            || !local_codes.is_empty();
        let flow = if has_own_metadata {
            let from_flow = if item.sign == ast::Sign::Less { to } else { from };
            let to_flow = if item.sign == ast::Sign::Less { from } else { to };
            let basis_root = tail.basis_root;
            make_resolved_flow(
                staged,
                day,
                from_flow,
                to_flow,
                amount.0,
                amount.0,
                Infer::Known,
                mode,
                tail,
                header_codes,
                local_codes,
                txn,
                item.loc,
                diags,
            )
            .map(|flow| {
                let offset = staged.flows().len();
                staged.book.flows.push(flow);
                push_flow_expressions(flow_roots, offset, None, None, basis_root);
                offset
            })
        } else {
            None
        };
        lowered.push(JournalItem {
            flow,
            sign: match item.sign {
                ast::Sign::Carve => Sign::Carve,
                ast::Sign::Add => Sign::Add,
                ast::Sign::Less => Sign::Less,
            },
            parent: TemplateItemParent::Header,
            side: parent_side,
            amount: amount.1.map_or(TemplateAmount::Literal(amount.0), TemplateAmount::Computed),
            loc: item.loc,
        });
    }
    lowered.into_boxed_slice()
}

fn push_flow_expressions(
    roots: &mut Vec<FlowExpressions>,
    flow: u32,
    out: Option<NodeId>,
    arrive: Option<NodeId>,
    basis: Option<NodeId>,
) {
    if out.is_some() || arrive.is_some() || basis.is_some() {
        roots.push(FlowExpressions { flow, out, arrive, basis });
    }
}

fn resolve_amount<'s>(
    world: &World<'s>,
    file: &ast::File<'s>,
    amount: ast::Amount<'s>,
    fallback: Id<crate::book::Commodity>,
    roots: &Map<ast::ExprId, NodeId>,
    diags: &mut Vec<Diagnostic>,
) -> Option<(Amount, Option<NodeId>)> {
    match amount {
        ast::Amount::Literal(literal) => {
            world.literal_amount(file, literal, Some(fallback)).or_report(diags).map(|amount| (amount, None))
        }
        ast::Amount::Computed(root) => Some((Amount::zero(fallback), Some(*roots.get(&root)?))),
    }
}

pub(super) fn resolve_object<'s>(
    world: &World<'s>,
    home: Home,
    file: &ast::File<'s>,
    name: ast::Name<'s>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Object> {
    if let Some(sym) = world.book.names.get(name.0)
        && let Some(&asset) = world.book.lookup.assets.get(&sym)
    {
        return Some(Object::Asset(asset));
    }
    let word = Word::of(file, name.0);
    if let Ok(end) = world.end(home, word) {
        return Some(end.entity.map_or(Object::Place(end.place), Object::Entity));
    }
    world.place(word).or_report(diags).map(|place| Object::Place(place))
}

fn merge_tail(mut parent: Tail, child: Tail) -> Tail {
    if child.purpose.is_some() {
        parent.purpose = child.purpose;
        parent.purpose_loc = child.purpose_loc;
    }
    if child.description.is_some() {
        parent.description = child.description;
    }
    if child.payee.is_some() {
        parent.payee = child.payee;
    }
    if child.recognized.is_some() {
        parent.recognized = child.recognized;
    }
    if child.waive.is_some() {
        parent.waive = child.waive;
    }
    if child.detail.basis.is_some() {
        parent.detail.basis = child.detail.basis;
    }
    if child.detail.basis.is_some() || child.basis_root.is_some() {
        parent.basis_root = child.basis_root;
    }
    if child.detail.hold.is_some() {
        parent.detail.hold = child.detail.hold;
    }
    if child.detail.since.is_some() {
        parent.detail.since = child.detail.since;
    }
    if child.detail.due.is_some() {
        parent.detail.due = child.detail.due;
    }
    if child.price.is_some() {
        parent.price = child.price;
    }
    parent.valid &= child.valid;
    parent
}

/// An empty run of codes at the end of the pool: a flow with none of its own says where they would have gone.
fn empty_codes(world: &World<'_>) -> Run<Sym> {
    Run::new(Id::new(world.book.codes.len() as u32), 0)
}

fn journal_end(end: ResolvedEnd) -> JournalEnd {
    JournalEnd { place: end.place, entity: end.entity }
}

/// The transaction of a record of the journal itself: it owns the flows and codes staged so far, and has
/// no program, contract or inputs. The record fills in what it has.
fn journal_txn(staged: &Staged<'_, '_>, day: Day, loc: Loc) -> Txn {
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

fn previous_period(day: Day, relative: ast::Relative) -> Days {
    match relative {
        ast::Relative::Month => {
            let last = day.month_start().add_days(-1);
            Days::new(last.month_start(), last).expect("previous month is ordered")
        }
        ast::Relative::Quarter => {
            let (year, month, _) = day.ymd();
            let quarter_month = ((month - 1) / 3) * 3 + 1;
            let current = Day::from_ymd(year, quarter_month, 1).expect("quarter starts in calendar");
            let last = current.add_days(-1);
            let first = last.add(Span::months(-2)).month_start();
            Days::new(first, last).expect("previous quarter is ordered")
        }
        ast::Relative::Year => {
            let last = day.year_start().add_days(-1);
            Days::new(last.year_start(), last).expect("previous year is ordered")
        }
    }
}

fn priced(
    world: &World<'_>,
    amount: Amount,
    quote: Id<crate::book::Commodity>,
    rate: axiom_core::Ratio,
    loc: Loc,
    diags: &mut Vec<Diagnostic>,
) -> Option<Amount> {
    if amount.unit == quote {
        diags.push(
            Diagnostic::error("price-transfer", "a price cannot change a same-commodity transfer")
                .label(loc, "remove the price"),
        );
        return None;
    }
    let (from, to) = (world.book.commodities[amount.unit].scale, world.book.commodities[quote].scale);
    match crate::prices::rescale(amount.qty, from, to, rate) {
        Some(qty) if !qty.is_zero() => Some(Amount::new(qty, quote)),
        Some(_) => {
            diags.push(
                Diagnostic::error("price-vanishes", "the priced amount rounds to nothing")
                    .label(loc, "increase precision or state an amount"),
            );
            None
        }
        None => {
            diags.push(
                Diagnostic::error("price-overflow", "the priced amount is outside the supported range")
                    .label(loc, "this conversion overflows"),
            );
            None
        }
    }
}

trait OtherSide {
    fn other(self) -> Self;
}

impl OtherSide for crate::book::FlowSide {
    fn other(self) -> Self {
        match self {
            Self::Out => Self::Arrive,
            Self::Arrive => Self::Out,
        }
    }
}
