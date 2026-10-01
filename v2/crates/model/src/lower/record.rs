//! Native S5 journal records. This pass reads the source AST directly and
//! appends resolved records to the pooled Book arenas.

use axiom_core::{Day, Days, Diagnostic, Dim, Groups, Id, Loc, Map, Qty, Run, Span};
use axiom_syntax as ast;
use axiom_syntax::{ClauseKind, ItemKind, Quantity, Subject};

use super::push_amount_root;
use crate::book::{
    Amount, Change as BookChange, FlowSide, Sign, TemplateAmount, TemplateItemParent, TermsState,
    ScheduleKind, Text,
};
use crate::declare::World;
use crate::errors::Word;
use crate::journal::{
    Action, Assert, Detail, EndEvent, EndTarget, Event, Filed, Flow, FlowExpressions, Gap, Infer,
    JournalEnd, JournalGroup, JournalItem, JournalProgram, JournalQuantity, Measure, Mode, Object,
    Origin, Provenance, Purposed, Quote, Reading, Select, Split, Waive,
};
use crate::law::{NodeId, Subject as ModelSubject, Ty};
use crate::scope::Home;
use crate::sources::Site;

#[derive(Clone, Copy)]
struct Dated {
    day: Day,
    site: u32,
    item: u32,
}

#[derive(Clone, Copy)]
struct ResolvedEnd {
    place: Id<crate::book::Place>,
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
enum CodeTarget {
    Unique {
        txn: Id<crate::journal::Txn>,
        loc: Loc,
    },
    Ambiguous {
        first: Loc,
        second: Loc,
    },
}

/// A chronological index of transaction codes. Each transaction is visited
/// once after its lowering succeeds; repeated code storage on its header and
/// flows is deduplicated by the transaction ID.
#[derive(Default)]
struct CodeIndex {
    by_code: Map<axiom_core::Sym, CodeTarget>,
}

impl CodeIndex {
    fn add(&mut self, world: &World<'_>, txn_id: Id<crate::journal::Txn>) {
        let source = &world.book.txns[txn_id];
        let mut add_code = |code| match self.by_code.get(&code).copied() {
            None => {
                self.by_code.insert(
                    code,
                    CodeTarget::Unique {
                        txn: txn_id,
                        loc: source.loc,
                    },
                );
            }
            Some(CodeTarget::Unique { txn, loc }) if txn != txn_id => {
                self.by_code.insert(
                    code,
                    CodeTarget::Ambiguous {
                        first: loc,
                        second: source.loc,
                    },
                );
            }
            Some(CodeTarget::Unique { .. } | CodeTarget::Ambiguous { .. }) => {}
        };
        for code in source.codes.ids().map(|id| world.book.codes[id]) {
            add_code(code);
        }
        for flow_id in source.flows.ids() {
            for code in world.book.flows[flow_id]
                .codes
                .ids()
                .map(|id| world.book.codes[id])
            {
                add_code(code);
            }
        }
    }

    fn resolve<'s>(
        &self,
        world: &mut World<'s>,
        code: ast::Code<'s>,
        loc: Loc,
        diags: &mut Vec<Diagnostic>,
    ) -> Option<Id<crate::journal::Txn>> {
        let symbol = world.book.names.intern(code.name());
        match self.by_code.get(&symbol).copied() {
            Some(CodeTarget::Unique { txn, .. }) => Some(txn),
            Some(CodeTarget::Ambiguous { first, second }) => {
                diags.push(
                    Diagnostic::error(
                        "ambiguous-against",
                        "this code names more than one earlier transaction",
                    )
                    .label(
                        loc,
                        format!("`{}` is not a unique transaction reference", code.name()),
                    )
                    .label(first, "one matching transaction is here")
                    .label(second, "another matching transaction is here")
                    .help("give the original transaction a code used nowhere else"),
                );
                None
            }
            None => {
                diags.push(
                    Diagnostic::error("unknown-against", "this code names no earlier transaction")
                        .label(
                            loc,
                            format!("`{}` has not named a transaction yet", code.name()),
                        )
                        .help("put this code on an earlier transaction or one of its flows"),
                );
                None
            }
        }
    }
}

/// Lowers dated native transactions, statements and openings in stable
/// `(day, source order)` order. Each transaction checkpoints the shared pools
/// so an invalid line cannot leave reachable partial flows or metadata.
pub(crate) fn record<'a, 's>(
    world: &mut World<'s>,
    sites: &[Site<'a, 's>],
    diags: &mut Vec<Diagnostic>,
) {
    let mut dated = Vec::new();
    for (site_at, site) in sites.iter().enumerate() {
        let Ok(site_at) = u32::try_from(site_at) else {
            diags.push(Diagnostic::error(
                "too-many-sources",
                "the project has too many source files",
            ));
            return;
        };
        for (item_at, item) in site.source.file.items.iter().enumerate() {
            let day = match item.kind {
                ItemKind::Txn(id) => Some(site.source.file[id].date),
                ItemKind::Statement(id) => Some(site.source.file[id].date),
                ItemKind::Opening(id) => Some(site.source.file[id].date),
                _ => None,
            };
            let Some(day) = day else { continue };
            let Ok(item_at) = u32::try_from(item_at) else {
                diags.push(
                    Diagnostic::error(
                        "too-many-records",
                        "a source file has too many dated records",
                    )
                    .label(item.loc, "record index exceeds the model limit"),
                );
                continue;
            };
            dated.push(Dated {
                day,
                site: site_at,
                item: item_at,
            });
        }
    }
    dated.sort_unstable_by_key(|item| (item.day, item.site, item.item));

    let expected_flows = dated
        .iter()
        .map(|item| {
            let source = sites[item.site as usize].source;
            match source.file.items[item.item as usize].kind {
                ItemKind::Txn(id) => {
                    let flow = &source.file[id].flow;
                    if flow.from.end.is_some() && flow.to.end.is_some() {
                        1
                    } else {
                        flow.body.legs.len() as usize
                    }
                }
                ItemKind::Opening(id) => source.file[id].lines.len(),
                _ => 0,
            }
        })
        .sum();
    world.book.txns.reserve(dated.len());
    world.book.flows.reserve(expected_flows);

    let mut code_index = CodeIndex::default();
    for record in dated {
        let site = &sites[record.site as usize];
        let file = &site.source.file;
        let item = &file.items[record.item as usize];
        let txn_start = world.book.txns.len();
        let diagnostic_start = diags.len();
        match item.kind {
            ItemKind::Txn(id) => lower_txn(world, site, item, &file[id], &code_index, diags),
            ItemKind::Opening(id) => {
                lower_opening(world, site, item, &file[id], &code_index, diags)
            }
            ItemKind::Statement(id) => {
                lower_statement(world, site, item.loc, item.doc, &file[id], &code_index, false, diags)
            }
            _ => unreachable!("dated index only contains journal records"),
        }
        if diags.len() == diagnostic_start {
            for txn_index in txn_start..world.book.txns.len() {
                code_index.add(world, Id::new(txn_index as u32));
            }
        }
    }

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
    let first = world.book.flows.len();
    let code_start = world.book.codes.len();
    let selector_start = world.book.selectors.len();
    let detail_start = world.book.details.len();
    let program_start = world.book.journal_programs.len();
    let txn_id = Id::new(world.book.txns.len() as u32);
    let diagnostic_start = diags.len();

    let roots = flow_roots(file, &written.flow);
    let name = world.book.names.intern("journal");
    let compiled = if roots.is_empty() {
        Some((crate::book::TemplateProgram::default(), Map::default()))
    } else {
        crate::laws::compile_template(world, diags, file, home, Ty::Flow, name, &[], &roots).map(
            |(program, nodes)| {
                let by_expr = roots
                    .iter()
                    .zip(nodes.iter())
                    .map(|(&(expr, _), &node)| (expr, node))
                    .collect();
                (program, by_expr)
            },
        )
    };
    let Some((program, root_ids)) = compiled else {
        push_empty_txn(world, item, written.date, code_start, diags);
        rollback(
            world,
            first,
            code_start,
            selector_start,
            detail_start,
            program_start,
        );
        return;
    };

    let (header_codes, header_tail) = lower_tail(
        world,
        home,
        file,
        written.flow.tail,
        written.date,
        &root_ids,
        code_index,
        diags,
    );
    let txn_waive = header_tail.waive;
    let mut flow_roots = Vec::new();
    let mut groups = Vec::new();
    let mut successful = header_tail.valid;
    let body_has_group = !written.flow.body.legs.is_empty() || !written.flow.body.items.is_empty();

    let from = written
        .flow
        .from
        .end
        .map(|end| resolve_end(world, home, file, end, diags));
    let to = written
        .flow
        .to
        .end
        .map(|end| resolve_end(world, home, file, end, diags));
    if from.is_some_and(|end| end.is_none()) || to.is_some_and(|end| end.is_none()) {
        successful = false;
    }

    if let (Some(Some(from)), Some(Some(to))) = (from, to) {
        if written.flow.body.legs.is_empty() {
            let flow_at = world.book.flows.len() - first;
            match make_flow(
                world,
                home,
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
                    world.book.flows.push(flow);
                    if let Some(exprs) = exprs {
                        flow_roots.push(FlowExpressions {
                            flow: flow_at as u32,
                            ..exprs
                        });
                    }
                    if body_has_group {
                        let items = lower_items(
                            world,
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
                            first,
                            diags,
                        );
                        groups.push(JournalGroup {
                            header: Some(flow_at as u32),
                            source: journal_end(from),
                            side: FlowSide::Out,
                            total: None,
                            legs: Box::default(),
                            items,
                        });
                    }
                }
                None => successful = false,
            }
        } else {
            diags.push(
                Diagnostic::error(
                    "flow-shape",
                    "a flow with both named ends cannot also have split legs",
                )
                .label(item.loc, "these legs do not have an unnamed side to fill")
                .help("name one end in the header and put the other ends on its indented legs"),
            );
            successful = false;
        }
    } else if written.flow.body.legs.is_empty() {
        diags.push(
            Diagnostic::error(
                "flow-shape",
                "a flow needs a named end and at least one leg",
            )
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
                    Diagnostic::error("flow-shape", "a split header names exactly one end").label(
                        item.loc,
                        "the named end of the split is missing or ambiguous",
                    ),
                );
                push_empty_txn(world, item, written.date, code_start, diags);
                rollback(
                    world,
                    first,
                    code_start,
                    selector_start,
                    detail_start,
                    program_start,
                );
                return;
            }
        };
        let source_qty = if source_is_from {
            written.flow.from.amount
        } else {
            written.flow.to.amount
        };
        let source_side = if source_is_from {
            FlowSide::Out
        } else {
            FlowSide::Arrive
        };
        let total = source_qty.and_then(|qty| {
            resolve_quantity(
                world,
                file,
                qty,
                world.book.base,
                source_side,
                &root_ids,
                diags,
            )
        });
        if source_qty.is_some() && total.is_none() {
            successful = false;
        }
        let mut legs = Vec::with_capacity(written.flow.body.legs.len());
        for leg in &file[written.flow.body.legs] {
            let Some(other) = resolve_end(world, home, file, leg.end, diags) else {
                successful = false;
                continue;
            };
            let (from, to) = if source_is_from {
                (source, other)
            } else {
                (other, source)
            };
            let mut tail = header_tail.clone();
            let (leg_codes, leg_tail) = lower_tail(
                world,
                home,
                file,
                leg.tail,
                written.date,
                &root_ids,
                code_index,
                diags,
            );
            tail = merge_tail(tail, leg_tail);
            let unit = total.map_or(world.book.base, |total| total.amount.unit);
            let Some(quantity) = resolve_quantity(
                world,
                file,
                leg.amount,
                unit,
                source_side.other(),
                &root_ids,
                diags,
            ) else {
                successful = false;
                continue;
            };
            let header_amount = total.map_or_else(|| Amount::zero(unit), |total| total.amount);
            let (out, arrive) = if source_is_from {
                (Amount::zero(header_amount.unit), quantity.amount)
            } else {
                (quantity.amount, Amount::zero(header_amount.unit))
            };
            let flow_at = world.book.flows.len() - first;
            let basis_root = tail.basis_root;
            if let Some(flow) = make_resolved_flow(
                world,
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
                world.book.flows.push(flow);
                legs.push(flow_at as u32);
                push_flow_expressions(
                    &mut flow_roots,
                    flow_at as u32,
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
            .map(|offset| {
                let leg = &world.book.flows[Id::new(first as u32 + *offset)];
                if source_is_from { leg.to } else { leg.from }
            })
            .map(|place| ResolvedEnd {
                place,
                entity: None,
                select: Run::new(Id::new(0), 0),
            })
            .unwrap_or(source);
        let items = lower_items(
            world,
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
            first,
            diags,
        );
        if !written.flow.body.items.is_empty()
            && items.iter().any(|item| item.flow.is_some())
            && legs.is_empty()
        {
            successful = false;
        }
        groups.push(JournalGroup {
            header: None,
            source: journal_end(source),
            side: source_side,
            total: total.map(|total| total.group),
            legs: legs.into_boxed_slice(),
            items,
        });
    }

    if diags.len() != diagnostic_start {
        successful = false;
    }
    if !successful {
        rollback(
            world,
            first,
            code_start,
            selector_start,
            detail_start,
            program_start,
        );
        push_empty_txn(world, item, written.date, code_start, diags);
        return;
    }

    let flow_count = world.book.flows.len() - first;
    let program_id = if !program.nodes.is_empty() || !flow_roots.is_empty() || !groups.is_empty() {
        Some(world.book.journal_programs.push(JournalProgram {
            program,
            flow_roots: flow_roots.into_boxed_slice(),
            groups: groups.into_boxed_slice(),
        }))
    } else {
        None
    };
    world.book.txns.push(crate::journal::Txn {
        day: written.date,
        flows: Run::new(Id::new(first as u32), flow_count as u32),
        inputs: Run::new(Id::new(world.book.input_values.len() as u32), 0),
        program: program_id,
        codes: header_codes,
        waive: txn_waive,
        contract: None,
        contract_schedule: None,
        ends: false,
        doc: item.doc.map(|doc| world.book.names.intern(doc.0)),
        loc: item.loc,
    });
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
    let first = world.book.flows.len();
    let code_start = world.book.codes.len();
    let selector_start = world.book.selectors.len();
    let detail_start = world.book.details.len();
    let program_start = world.book.journal_programs.len();
    let txn_id = Id::new(world.book.txns.len() as u32);
    let diagnostic_start = diags.len();
    let opening_place = world.book.entities[world.book.roots.opening].place;
    let Some(opening_place) = opening_place else {
        diags.push(
            Diagnostic::error("opening-place", "the opening source has no place")
                .label(item.loc, "cannot record opening"),
        );
        return;
    };
    let mut roots = Vec::new();
    for leg in &file[opening.lines] {
        push_tail_roots(file, leg.tail, &mut roots);
    }
    let program_name = world.book.names.intern("journal");
    let compiled = super::compile_roots(
        world,
        file,
        site.home,
        Ty::Flow,
        program_name,
        &[],
        &roots,
        diags,
    );
    let Some((program, root_ids)) = compiled else {
        rollback(
            world,
            first,
            code_start,
            selector_start,
            detail_start,
            program_start,
        );
        push_empty_txn(world, item, opening.date, code_start, diags);
        return;
    };
    let mut flow_roots = Vec::new();
    for leg in &file[opening.lines] {
        let whole_asset = if matches!(leg.amount, Quantity::Whole) {
            world.book.asset(leg.end.name.0)
        } else {
            None
        };
        let end = if let Some(asset) = whole_asset {
            Some(ResolvedEnd {
                place: world.book.assets[asset].place,
                entity: None,
                select: Run::new(Id::new(0), 0),
            })
        } else {
            resolve_end(world, site.home, file, leg.end, diags)
        };
        let Some(end) = end else {
            if matches!(leg.amount, Quantity::Whole) {
                diags.push(
                    Diagnostic::error(
                        "opening-whole",
                        "a whole opening holding must name an asset",
                    )
                    .label(leg.loc, "write a unit amount for an account holding"),
                );
            }
            continue;
        };
        let fallback = whole_asset.map_or(world.book.base, |asset| world.book.assets[asset].unit);
        let Some(quantity) = resolve_quantity(
            world,
            file,
            leg.amount,
            fallback,
            FlowSide::Out,
            &Map::default(),
            diags,
        ) else {
            continue;
        };
        if !matches!(leg.amount, Quantity::Amount(ast::Amount::Literal(_))) && whole_asset.is_none()
        {
            diags.push(
                Diagnostic::error("opening-amount", "an opening line needs a literal amount")
                    .label(
                        leg.loc,
                        "computed and inferred quantities cannot set an opening balance",
                    ),
            );
            continue;
        }
        let tail = lower_tail(
            world,
            site.home,
            file,
            leg.tail,
            opening.date,
            &root_ids,
            code_index,
            diags,
        )
        .1;
        if end.select.len() != 0 {
            diags.push(
                Diagnostic::error("opening-selector", "an opening line sets a whole place")
                    .label(leg.loc, "selectors do not apply to an opening balance"),
            );
            continue;
        }
        let opening_end = ResolvedEnd {
            place: opening_place,
            entity: None,
            select: Run::new(Id::new(0), 0),
        };
        let named_end = ResolvedEnd {
            place: end.place,
            entity: None,
            select: Run::new(Id::new(0), 0),
        };
        let (from, to) = if world.book.places[end.place].class.display_sign() > 0 {
            (opening_end, named_end)
        } else {
            (named_end, opening_end)
        };
        let amount = quantity.amount;
        let basis_root = tail.basis_root;
        if let Some(mut flow) = make_resolved_flow(
            world,
            opening.date,
            from,
            to,
            amount,
            amount,
            Infer::Known,
            Mode::Opening,
            tail,
            Run::new(Id::new(code_start as u32), 0),
            Run::new(Id::new(world.book.codes.len() as u32), 0),
            txn_id,
            leg.loc,
            diags,
        ) {
            flow.owner = whole_asset.map_or(world.book.places[end.place].owner, |asset| {
                world.book.assets[asset].owner
            });
            let flow_at = (world.book.flows.len() - first) as u32;
            world.book.flows.push(flow);
            push_flow_expressions(&mut flow_roots, flow_at, None, None, basis_root);
        }
    }
    if diags.len() != diagnostic_start {
        rollback(
            world,
            first,
            code_start,
            selector_start,
            detail_start,
            program_start,
        );
    }
    let count = world.book.flows.len() - first;
    let program_id = if !program.nodes.is_empty() || !flow_roots.is_empty() {
        Some(world.book.journal_programs.push(JournalProgram {
            program,
            flow_roots: flow_roots.into_boxed_slice(),
            groups: Box::default(),
        }))
    } else {
        None
    };
    world.book.txns.push(crate::journal::Txn {
        day: opening.date,
        flows: Run::new(Id::new(first as u32), count as u32),
        inputs: Run::new(Id::new(world.book.input_values.len() as u32), 0),
        program: program_id,
        codes: Run::new(
            Id::new(code_start as u32),
            (world.book.codes.len() - code_start) as u32,
        ),
        waive: None,
        contract: None,
        contract_schedule: None,
        ends: false,
        doc: item.doc.map(|doc| world.book.names.intern(doc.0)),
        loc: item.loc,
    });
    for claim in &file[opening.claims] {
        lower_statement(
            world,
            site,
            super::subject_loc(file, claim.subject),
            None,
            claim,
            code_index,
            true,
            diags,
        );
    }
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
        ast::Verb::Value(amount) => {
            lower_value(world, site.home, file, loc, statement, *amount, diags)
        }
        ast::Verb::Worked(amount) | ast::Verb::Used(amount) => {
            let action = if matches!(&statement.verb, ast::Verb::Worked(_)) {
                Action::Work
            } else {
                Action::Use
            };
            lower_measure(
                world, site.home, file, loc, statement, *amount, action, code_index, diags,
            );
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
        ast::Verb::Owes { creditor, amount } => lower_owes(
            world, site, loc, statement, *creditor, *amount, code_index, opening, diags,
        ),
        ast::Verb::Basis { amount, since } => lower_basis(
            world, site, loc, statement, *amount, *since, code_index, diags,
        ),
        ast::Verb::Split {
            numerator,
            denominator,
        } => {
            let Subject::Unit(unit) = statement.subject else {
                unsupported_statement(loc, "a split needs a commodity subject", diags);
                return;
            };
            let word = Word {
                text: unit.0,
                loc: file.loc(unit.0),
            };
            let unit = match world.commodity_of(word) {
                Ok(unit) => unit,
                Err(problem) => {
                    diags.push(problem);
                    return;
                }
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
            world.book.splits.push(Split {
                day: statement.date,
                unit,
                ratio,
                loc,
            });
        }
        ast::Verb::Now(ast::Change::Property(_)) => {
            // Custom properties are lowered by props::declare, which stages
            // dated values and their inclusive `until` restoration.
        }
        ast::Verb::Now(ast::Change::Budget(_)) => {
            // Native budgets are lowered by the declaration/law pass.
        }
        ast::Verb::Waived => lower_contract_change(world, site, loc, statement, diags),
        ast::Verb::Ends => lower_end(world, site, loc, statement, diags),
        ast::Verb::Occurrence(amount) => lower_occurrence(
            world,
            site,
            doc,
            loc,
            statement,
            *amount,
            diags,
        ),
        ast::Verb::Now(_) => unsupported_statement(
            loc,
            "this statement kind does not yet have a native record lowering",
            diags,
        ),
    }
}

fn lower_occurrence<'a, 's>(
    world: &mut World<'s>,
    site: &Site<'a, 's>,
    doc: Option<ast::Doc<'s>>,
    loc: Loc,
    statement: &ast::Statement<'s>,
    amount: Option<ast::Amount<'s>>,
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
    let contract = &world.book.contracts[contract_id];
    let (schedule, terms) = match nearest_occurrence(contract, statement.date) {
        Ok(Some(found)) => found,
        Ok(None) => {
            diags.push(
                Diagnostic::error("contract-occurrence-date", "this day is outside every contract schedule's grace window")
                    .label(loc, "no active scheduled occurrence is close enough to this date"),
            );
            return;
        }
        Err((regular, standing)) => {
            diags.push(
                Diagnostic::error("ambiguous-contract-occurrence", "this occurrence is equally close to two contract schedules")
                    .label(loc, "write it on a date that identifies one schedule")
                    .note(format!("nearest regular due day: {regular}; nearest standing due day: {standing}")),
            );
            return;
        }
    };
    if amount.is_some() {
        unsupported_statement(
            loc,
            "a written occurrence amount is not yet retained by the native journal record",
            diags,
        );
        return;
    }

    let code_start = world.book.codes.len();
    let diagnostic_start = diags.len();
    let mut occurrence_codes = Vec::new();
    for clause in &file[statement.tail] {
        match clause.kind {
            ClauseKind::Code(code) => occurrence_codes.push(world.book.names.intern(code.name())),
            _ => {
                diags.push(
                    Diagnostic::error("contract-occurrence-tail", "this clause is not retained on a contract occurrence")
                        .label(clause.at, "remove the clause or put it on a written flow"),
                );
            }
        }
    }

    let mut input_values: Vec<Option<Amount>> = vec![None; terms.inputs.len()];
    let mut bound = vec![false; terms.inputs.len()];
    for leg in &file[statement.body.legs] {
        let input = terms.inputs.iter().position(|input| world.book.name(input.name) == leg.end.name.0);
        let Some(input_at) = input else {
            diags.push(
                Diagnostic::error("contract-occurrence-body", "written flow overrides are not yet lowered for this occurrence")
                    .label(leg.loc, "this line does not bind a declared contract input"),
            );
            continue;
        };
        if bound[input_at] {
            diags.push(
                Diagnostic::error("contract-input-duplicate", "this contract input is supplied twice")
                    .label(leg.loc, "remove the repeated binding")
                    .context(terms.inputs[input_at].loc, "the input is declared here"),
            );
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
            Quantity::Amount(ast::Amount::Literal(literal))
            | Quantity::Target(ast::Amount::Literal(literal)) => literal,
            _ => {
                diags.push(
                    Diagnostic::error("contract-input-value", "a contract input needs a literal amount")
                        .label(leg.loc, "write `input-name = 155 USD`"),
                );
                continue;
            }
        };
        let input_unit = terms.inputs[input_at].unit;
        let unit = match literal.unit() {
            Some(unit) => match world.commodity_of(Word {
                text: unit.0,
                loc: file.loc(unit.0),
            }) {
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
                    .context(terms.inputs[input_at].loc, "the input's expected unit is declared here"),
            );
            continue;
        }
        match world.amount(literal.num(), unit, leg.loc) {
            Ok(value) => {
                input_values[input_at] = Some(value);
                bound[input_at] = true;
            }
            Err(problem) => diags.push(problem),
        }
    }
    if !file[statement.body.items].is_empty() {
        diags.push(
            Diagnostic::error("contract-occurrence-items", "written line items are not yet lowered on an occurrence")
                .label(loc, "move the item to the contract terms or wait for item override lowering"),
        );
    }
    if diags.len() != diagnostic_start {
        return;
    }
    for code in occurrence_codes {
        world.book.codes.push(code);
    }
    let code_count = world.book.codes.len() - code_start;
    let input_start = world.book.input_values.len();
    for value in input_values {
        world.book.input_values.push(value);
    }
    world.book.txns.push(crate::journal::Txn {
        day: statement.date,
        flows: Run::new(Id::new(world.book.flows.len() as u32), 0),
        inputs: Run::new(Id::new(input_start as u32), terms.inputs.len() as u32),
        program: None,
        codes: Run::new(Id::new(code_start as u32), code_count as u32),
        waive: None,
        contract: Some(contract_id),
        contract_schedule: Some(schedule),
        ends: false,
        doc: doc.map(|doc| world.book.names.intern(doc.0)),
        loc,
    });
}

fn nearest_occurrence<'a>(
    contract: &'a crate::book::Contract,
    day: Day,
) -> Result<Option<(ScheduleKind, &'a crate::book::Terms)>, (Day, Day)> {
    if !contract.days.contains(day) {
        return Ok(None);
    }
    let mut radius = 0i64;
    for timeline in [contract.terms.as_ref(), contract.standing.as_ref()].into_iter().flatten() {
        let max_grace = std::iter::once(timeline.at(Day::MIN)).chain(timeline.changes().map(|(_, terms)| terms));
        for terms in max_grace {
            radius = radius.max(i64::from(terms.grace.months).saturating_mul(31).saturating_add(i64::from(terms.grace.days)));
        }
    }
    let radius = radius.clamp(0, i64::from(i32::MAX)) as i32;
    let Some(search) = Days::new(Day(day.0.saturating_sub(radius)), Day(day.0.saturating_add(radius))) else {
        return Ok(None);
    };
    let mut regular = None;
    let mut standing = None;
    for occurrence in contract.occurrences(search) {
        let delta = if day >= occurrence.day {
            day.since(occurrence.day)
        } else {
            occurrence.day.since(day)
        };
        if delta > occurrence.terms.grace {
            continue;
        }
        let distance = (i64::from(day.0) - i64::from(occurrence.day.0)).abs();
        let candidate = (distance, occurrence.day > day, occurrence.day, occurrence.terms);
        let best = match occurrence.schedule {
            ScheduleKind::Regular => &mut regular,
            ScheduleKind::Standing => &mut standing,
        };
        if best.is_none_or(|(best_distance, best_future, _, _)| {
            (distance, candidate.1) < (best_distance, best_future)
        }) {
            *best = Some(candidate);
        }
    }
    match (regular, standing) {
        (Some((r_distance, _, r_day, regular_terms)), Some((s_distance, _, s_day, standing_terms))) => {
            if r_distance == s_distance {
                Err((r_day, s_day))
            } else if r_distance < s_distance {
                Ok(Some((ScheduleKind::Regular, regular_terms)))
            } else {
                Ok(Some((ScheduleKind::Standing, standing_terms)))
            }
        }
        (Some((_, _, _, terms)), None) => Ok(Some((ScheduleKind::Regular, terms))),
        (None, Some((_, _, _, terms))) => Ok(Some((ScheduleKind::Standing, terms))),
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
    let Subject::Name(debtor_name) = statement.subject else {
        unsupported_statement(loc, "a claim needs a named debtor", diags);
        return;
    };
    let entity = |world: &World<'s>, name: ast::Name<'s>| {
        world.entity(
            site.home,
            Word {
                text: name.0,
                loc: file.loc(name.0),
            },
        )
    };
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
            Diagnostic::error("self-claim", "an entity cannot owe itself")
                .label(loc, "name a different creditor"),
        );
        return;
    }
    let debtor_is_owner = world.book.entities[debtor].place.is_some_and(|place| {
        matches!(world.book.places[place].role, crate::book::Role::Holding(owner) if owner == debtor)
    });
    let creditor_is_owner = world.book.entities[creditor].place.is_some_and(|place| {
        matches!(world.book.places[place].role, crate::book::Role::Holding(owner) if owner == creditor)
    });
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
    let outside = ResolvedEnd {
        place: party_place,
        entity: Some(party_end),
        select: empty,
    };
    let tab = ResolvedEnd {
        place: tab,
        entity: None,
        select: empty,
    };
    let (from, to) = if class == crate::book::Class::Asset {
        (outside, tab)
    } else {
        (tab, outside)
    };

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
    let Some((program, roots)) =
        super::compile_roots(world, file, site.home, Ty::Flow, name, &[], &exprs, diags)
    else {
        return;
    };
    let flow_start = world.book.flows.len();
    let code_start = world.book.codes.len();
    let selector_start = world.book.selectors.len();
    let detail_start = world.book.details.len();
    let program_start = world.book.journal_programs.len();
    let txn_id = Id::new(world.book.txns.len() as u32);
    let diagnostic_start = diags.len();
    let (header_codes, header_tail) = lower_tail(
        world,
        site.home,
        file,
        statement.tail,
        statement.date,
        &roots,
        code_index,
        diags,
    );
    if !header_tail.valid {
        rollback(
            world,
            flow_start,
            code_start,
            selector_start,
            detail_start,
            program_start,
        );
        return;
    }
    let mode = if opening { Mode::Opening } else { Mode::Actual };
    let mut flow_roots = Vec::new();
    let mut groups = Vec::new();
    if let Some(written_amount) = amount {
        let Some((amount, root)) =
            resolve_amount(world, file, written_amount, world.book.base, &roots, diags)
        else {
            rollback(
                world,
                flow_start,
                code_start,
                selector_start,
                detail_start,
                program_start,
            );
            return;
        };
        if let Some(mut flow) = make_resolved_flow(
            world,
            statement.date,
            from,
            to,
            amount,
            amount,
            Infer::Known,
            mode,
            header_tail.clone(),
            header_codes,
            Run::new(Id::new(world.book.codes.len() as u32), 0),
            txn_id,
            loc,
            diags,
        ) {
            flow.owner = owner;
            world.book.flows.push(flow);
            push_flow_expressions(&mut flow_roots, 0, root, root, header_tail.basis_root);
            if !statement.body.items.is_empty() {
                let items = lower_items(
                    world,
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
                    flow_start,
                    diags,
                );
                groups.push(JournalGroup {
                    header: Some(0),
                    source: journal_end(from),
                    side: FlowSide::Out,
                    total: None,
                    legs: Box::default(),
                    items,
                });
            }
        }
    } else {
        let items = lower_items(
            world,
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
            flow_start,
            diags,
        );
        groups.push(JournalGroup {
            header: None,
            source: journal_end(from),
            side: FlowSide::Out,
            total: Some(JournalQuantity::Derived),
            legs: Box::default(),
            items,
        });
    }
    if diags.len() != diagnostic_start {
        rollback(
            world,
            flow_start,
            code_start,
            selector_start,
            detail_start,
            program_start,
        );
        return;
    }
    let flow_count = world.book.flows.len() - flow_start;
    let program_id = (!program.nodes.is_empty() || !flow_roots.is_empty() || !groups.is_empty())
        .then(|| {
            world.book.journal_programs.push(JournalProgram {
                program,
                flow_roots: flow_roots.into_boxed_slice(),
                groups: groups.into_boxed_slice(),
            })
        });
    world.book.txns.push(crate::journal::Txn {
        day: statement.date,
        flows: Run::new(Id::new(flow_start as u32), flow_count as u32),
        inputs: Run::new(Id::new(world.book.input_values.len() as u32), 0),
        program: program_id,
        codes: header_codes,
        waive: header_tail.waive,
        contract: None,
        contract_schedule: None,
        ends: false,
        doc: None,
        loc,
    });
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
        unsupported_statement(
            loc,
            "a basis statement cannot have indented journal lines",
            diags,
        );
        return;
    }
    let mut exprs = Vec::new();
    if let ast::Amount::Computed(expr) = written_amount {
        exprs.push((expr, Ty::AMOUNT));
    }
    push_tail_roots(file, statement.tail, &mut exprs);
    let name = world.book.names.intern("journal");
    let Some((program, roots)) =
        super::compile_roots(world, file, site.home, Ty::Asset, name, &[], &exprs, diags)
    else {
        return;
    };
    let flow_start = world.book.flows.len();
    let code_start = world.book.codes.len();
    let selector_start = world.book.selectors.len();
    let detail_start = world.book.details.len();
    let program_start = world.book.journal_programs.len();
    let txn_id = Id::new(world.book.txns.len() as u32);
    let diagnostic_start = diags.len();
    let (header_codes, mut tail) = lower_tail(
        world,
        site.home,
        file,
        statement.tail,
        statement.date,
        &roots,
        code_index,
        diags,
    );
    tail.detail.since = since.or(tail.detail.since);
    let basis_root = match written_amount {
        ast::Amount::Literal(literal) => {
            let Some(amount) = literal_amount(world, file, literal, None, diags) else {
                rollback(
                    world,
                    flow_start,
                    code_start,
                    selector_start,
                    detail_start,
                    program_start,
                );
                return;
            };
            if amount.unit != world.book.base {
                diags.push(
                    Diagnostic::error("basis-unit", "asset basis must be in the base currency")
                        .label(
                            file.loc(literal.0),
                            "convert this amount to the book's base unit",
                        ),
                );
                rollback(
                    world,
                    flow_start,
                    code_start,
                    selector_start,
                    detail_start,
                    program_start,
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
                rollback(
                    world,
                    flow_start,
                    code_start,
                    selector_start,
                    detail_start,
                    program_start,
                );
                return;
            };
            if let Some(Ty::Amount(Dim::Of(unit))) = program.nodes[root].typed_ty()
                && unit != world.book.base
            {
                diags.push(
                    Diagnostic::error("basis-unit", "asset basis must be in the base currency")
                        .label(file.exprs[expr].loc, "this expression has another unit"),
                );
                rollback(
                    world,
                    flow_start,
                    code_start,
                    selector_start,
                    detail_start,
                    program_start,
                );
                return;
            }
            Some(root)
        }
    };
    if !tail.valid {
        rollback(
            world,
            flow_start,
            code_start,
            selector_start,
            detail_start,
            program_start,
        );
        return;
    }
    let asset = &world.book.assets[asset_id];
    let (asset_place, asset_owner, asset_unit) = (asset.place, asset.owner, asset.unit);
    let unknown = world.book.entities[world.book.roots.unknown].place;
    let Some(unknown) = unknown else {
        diags.push(
            Diagnostic::error("basis-source", "the unknown party has no flow endpoint")
                .label(loc, "cannot record this asset's arrival"),
        );
        rollback(
            world,
            flow_start,
            code_start,
            selector_start,
            detail_start,
            program_start,
        );
        return;
    };
    let empty = Run::new(Id::new(0), 0);
    let from = ResolvedEnd {
        place: unknown,
        entity: Some(world.book.roots.unknown),
        select: empty,
    };
    let to = ResolvedEnd {
        place: asset_place,
        entity: None,
        select: empty,
    };
    let quantity = Amount::new(Qty(1), asset_unit);
    if let Some(mut flow) = make_resolved_flow(
        world,
        statement.date,
        from,
        to,
        quantity,
        quantity,
        Infer::Known,
        Mode::Actual,
        tail,
        header_codes,
        Run::new(Id::new(world.book.codes.len() as u32), 0),
        txn_id,
        loc,
        diags,
    ) {
        flow.owner = asset_owner;
        let waive = flow.waive;
        world.book.flows.push(flow);
        let mut flow_roots = Vec::new();
        push_flow_expressions(&mut flow_roots, 0, None, None, basis_root);
        if diags.len() != diagnostic_start {
            rollback(
                world,
                flow_start,
                code_start,
                selector_start,
                detail_start,
                program_start,
            );
            return;
        }
        let program_id = (!program.nodes.is_empty() || !flow_roots.is_empty()).then(|| {
            world.book.journal_programs.push(JournalProgram {
                program,
                flow_roots: flow_roots.into_boxed_slice(),
                groups: Box::default(),
            })
        });
        world.book.txns.push(crate::journal::Txn {
            day: statement.date,
            flows: Run::new(Id::new(flow_start as u32), 1),
            inputs: Run::new(Id::new(world.book.input_values.len() as u32), 0),
            program: program_id,
            codes: header_codes,
            waive,
            contract: None,
            contract_schedule: None,
            ends: false,
            doc: None,
            loc,
        });
    } else {
        rollback(
            world,
            flow_start,
            code_start,
            selector_start,
            detail_start,
            program_start,
        );
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
        unsupported_statement(
            loc,
            "a contract waiver cannot carry recovery lines yet",
            diags,
        );
        return;
    }

    let mut last = statement.date;
    let mut until_loc = None;
    let mut code = None;
    let mut description_loc = None;
    let mut description = None;
    for clause in &file[statement.tail] {
        match clause.kind {
            ClauseKind::Until(until) => {
                if let Some(first) = until_loc {
                    diags.push(
                        Diagnostic::error("duplicate-waiver-until", "a waiver has one end date")
                            .label(first, "the first end date is here")
                            .label(clause.at, "this second end date would replace it"),
                    );
                    return;
                }
                until_loc = Some(clause.at);
                last = until;
            }
            ClauseKind::Code(written) => {
                if let Some((_, first)) = code {
                    diags.push(
                        Diagnostic::error(
                            "duplicate-waiver-code",
                            "a waiver names one change code",
                        )
                        .label(first, "the first code is here")
                        .label(clause.at, "this second code cannot replace it"),
                    );
                    return;
                }
                code = Some((written, clause.at));
            }
            ClauseKind::Description(text) => {
                if let Some(first) = description_loc {
                    diags.push(
                        Diagnostic::error(
                            "duplicate-waiver-description",
                            "a waiver has one description",
                        )
                        .label(first, "the first description is here")
                        .label(clause.at, "this second description cannot replace it"),
                    );
                    return;
                }
                description_loc = Some(clause.at);
                description = Some(world.book.quoted_text(text.0));
            }
            ClauseKind::Purpose(_) => {
                unsupported_statement(
                    loc,
                    "a contract waiver has no claim-recovery purpose",
                    diags,
                );
                return;
            }
            _ => {
                unsupported_statement(
                    loc,
                    "this clause does not apply to a contract waiver",
                    diags,
                );
                return;
            }
        }
    }
    let Some(days) = Days::new(statement.date, last) else {
        diags.push(
            Diagnostic::error("waiver-span", "a waiver ends before it begins")
                .label(loc, "the `until` day must be on or after this day"),
        );
        return;
    };
    let change = BookChange {
        days,
        description,
        code: code.map(|(code, _)| world.book.names.intern(code.name())),
        loc,
    };
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
            Diagnostic::error(
                "waiver-without-schedule",
                "this contract has no schedule to waive",
            )
            .label(loc, "there is no regular or standing occurrence here"),
        );
    }
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
    // Validate the whole row before changing a contract or closing a place.
    // Codes may repeat by design; a description may occur only once.
    let mut description = None;
    let mut description_loc = None;
    for clause in &file[statement.tail] {
        match clause.kind {
            ClauseKind::Code(_) => {}
            ClauseKind::Description(text) => {
                if let Some(first) = description_loc {
                    diags.push(
                        Diagnostic::error(
                            "duplicate-end-description",
                            "an ending has one description",
                        )
                        .label(first, "the first description is here")
                        .label(clause.at, "this second description cannot replace it"),
                    );
                    return;
                }
                description_loc = Some(clause.at);
                description = Some(text);
            }
            _ => {
                unsupported_statement(loc, "this clause does not apply to an ending", diags);
                return;
            }
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
                Diagnostic::error(
                    "end-before-contract",
                    "a contract cannot end before it begins",
                )
                .label(loc, "this date precedes the contract's first day"),
            );
            return;
        }
        EndTarget::Contract(contract_id)
    } else if let Some(asset) = world.book.asset(name.0) {
        EndTarget::Asset(asset)
    } else {
        match world.end(
            site.home,
            Word {
                text: name.0,
                loc: file.loc(name.0),
            },
        ) {
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
        codes: Run::new(
            Id::new(codes_start as u32),
            (world.book.codes.len() - codes_start) as u32,
        ),
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
    let system = match world.system(Word {
        text: system_name.0,
        loc: file.loc(system_name.0),
    }) {
        Ok(system) => system,
        Err(problem) => {
            diags.push(problem);
            return;
        }
    };
    let fallback = world.book.systems[system]
        .currency
        .unwrap_or(world.book.base);
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
        let Some(amount) = literal_amount(world, file, literal, Some(fallback), diags) else {
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
    Place(Id<crate::book::Place>),
    Entity(Id<crate::book::Entity>),
    Asset(Id<crate::book::Asset>),
    Unit(Id<crate::book::Commodity>),
    Code(axiom_core::Sym),
    Purpose(Id<crate::book::Purpose>),
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
            let word = Word {
                text: name.0,
                loc: file.loc(name.0),
            };
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
        Subject::Purpose(name) => world
            .purpose(
                home,
                Word {
                    text: name.0,
                    loc: file.loc(name.0),
                },
            )
            .map(StatementTarget::Purpose)
            .map_err(|problem| diags.push(problem))
            .ok(),
        Subject::Unit(name) => world
            .commodity_of(Word {
                text: name.0,
                loc: file.loc(name.0),
            })
            .map(StatementTarget::Unit)
            .map_err(|problem| diags.push(problem))
            .ok(),
    }
}

fn literal_amount<'s>(
    world: &World<'s>,
    file: &ast::File<'s>,
    literal: ast::Literal<'s>,
    fallback: Option<Id<crate::book::Commodity>>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Amount> {
    let unit = match literal.unit() {
        Some(unit) => match world.commodity_of(Word {
            text: unit.0,
            loc: file.loc(unit.0),
        }) {
            Ok(unit) => unit,
            Err(problem) => {
                diags.push(problem);
                return None;
            }
        },
        None => match fallback {
            Some(unit) => unit,
            None => {
                diags.push(
                    Diagnostic::error("amount-unit", "this amount needs an explicit unit")
                        .label(file.loc(literal.0), "write a commodity after the amount"),
                );
                return None;
            }
        },
    };
    world
        .amount(literal.num(), unit, file.loc(literal.0))
        .map_err(|problem| diags.push(problem))
        .ok()
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
            let Some((amount, computed)) =
                assertion_amount(world, home, file, value, fallback, Ty::Place, diags)
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
            let Some(amount) = literal_amount(world, file, literal, Some(world.book.base), diags)
            else {
                return;
            };
            world.book.readings.push(Reading {
                day: statement.date,
                code,
                amount,
                loc,
            });
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
            let quote = match world.commodity_of(Word {
                text: quote_name.0,
                loc: file.loc(quote_name.0),
            }) {
                Ok(quote) => quote,
                Err(problem) => {
                    diags.push(problem);
                    return;
                }
            };
            let Some(rate) = literal
                .num()
                .to_ratio()
                .filter(|rate| *rate > axiom_core::Ratio::ZERO)
            else {
                diags.push(
                    Diagnostic::error("price-zero", "a price must be greater than zero")
                        .label(file.loc(literal.0), "this price is not positive"),
                );
                return;
            };
            world.book.prices.quotes.push(Quote {
                unit,
                quote,
                day: statement.date,
                rate,
                implied: false,
                loc,
            });
        }
        StatementTarget::Entity(_) | StatementTarget::Purpose(_) => unsupported_statement(
            loc,
            "a value needs an account, asset, code or commodity subject",
            diags,
        ),
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
            literal_amount(world, file, literal, Some(fallback), diags).map(|amount| (amount, None))
        }
        ast::Amount::Computed(root) => {
            let name = world.book.names.intern("assertion");
            let (program, roots) = crate::laws::compile_template(
                world,
                diags,
                file,
                home,
                subject,
                name,
                &[],
                &[(root, Ty::AMOUNT)],
            )?;
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
        Diagnostic::error(
            "computed-value-subject",
            format!("computed values are not supported for {subject}"),
        )
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
            ClauseKind::Via(name) => match world.end(
                home,
                Word {
                    text: name.0,
                    loc: file.loc(name.0),
                },
            ) {
                Ok(end) => {
                    gap = Gap::Via {
                        place: end.place,
                        loc: clause.at,
                    }
                }
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
            _ => {
                diags.push(
                    Diagnostic::error(
                        "assertion-tail",
                        "this tail clause does not apply to a value",
                    )
                    .label(clause.at, "remove the clause or move it to a flow"),
                );
                return None;
            }
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
        StatementTarget::Place(place) => {
            (ModelSubject::Place(place), world.book.places[place].owner)
        }
        StatementTarget::Entity(entity) => (ModelSubject::Entity(entity), entity),
        StatementTarget::Asset(asset) => {
            (ModelSubject::Asset(asset), world.book.assets[asset].owner)
        }
        _ => {
            unsupported_statement(loc, "a measure needs a named entity, place or asset", diags);
            return;
        }
    };
    let Some(quantity) = literal_amount(world, file, literal, None, diags) else {
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
            ClauseKind::For(ast::For::Whom(name)) => match world.entity(
                home,
                Word {
                    text: name.0,
                    loc: file.loc(name.0),
                },
            ) {
                Ok(entity) => party = Some(entity),
                Err(problem) => diags.push(problem),
            },
            ClauseKind::Purpose(written) => {
                let purpose_id = world.purpose(
                    home,
                    Word {
                        text: written.name.0,
                        loc: file.loc(written.name.0),
                    },
                );
                match purpose_id {
                    Ok(purpose_id) => {
                        let of = written
                            .of
                            .and_then(|name| resolve_object(world, home, file, name, diags));
                        purpose = Some(Purposed {
                            purpose: purpose_id,
                            of,
                            source: Provenance::Written,
                        });
                    }
                    Err(problem) => diags.push(problem),
                }
            }
            ClauseKind::Description(text) => {
                description = Some(world.book.quoted_text(text.0));
            }
            ClauseKind::Code(code) => codes.push(world.book.names.intern(code.name())),
            ClauseKind::Against(code) => {
                against = code_index.resolve(world, code, clause.at, diags);
            }
            _ => diags.push(
                Diagnostic::error(
                    "measure-tail",
                    "this tail clause does not apply to a measure",
                )
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
        Diagnostic::error("statement-lowering", message)
            .label(loc, "this record is not included in the Book yet"),
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

fn push_tail_roots<'s>(
    file: &ast::File<'s>,
    clauses: ast::Many<ast::Clause<'s>>,
    roots: &mut Vec<(ast::ExprId, Ty)>,
) {
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
    let loc = match quantity {
        Quantity::Amount(ast::Amount::Literal(literal))
        | Quantity::Pending(ast::Amount::Literal(literal))
        | Quantity::Target(ast::Amount::Literal(literal)) => file.loc(literal.0),
        Quantity::Amount(ast::Amount::Computed(root))
        | Quantity::Pending(ast::Amount::Computed(root))
        | Quantity::Target(ast::Amount::Computed(root)) => file.exprs[root].loc,
        Quantity::Unknown(name) => file.loc(name.0),
        _ => Loc::default(),
    };
    let resolve_literal = |world: &World<'s>, literal: ast::Literal<'s>| -> Option<Amount> {
        let unit = match literal.unit() {
            Some(unit) => world
                .commodity_of(Word {
                    text: unit.0,
                    loc: file.loc(unit.0),
                })
                .ok(),
            None => Some(fallback),
        }?;
        world.amount(literal.num(), unit, file.loc(literal.0)).ok()
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
                    end: if side == FlowSide::Out {
                        crate::journal::End::From
                    } else {
                        crate::journal::End::To
                    },
                    balance: amount.qty,
                },
                mode: Mode::Actual,
                root,
                group: JournalQuantity::Target(amount, root),
            }
        }
        Quantity::Unknown(unit) => {
            let unit = world
                .commodity_of(Word {
                    text: unit.0,
                    loc: file.loc(unit.0),
                })
                .map_err(|problem| diags.push(problem))
                .ok()?;
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
                Some(unit) => Some(
                    world
                        .commodity_of(Word {
                            text: unit.0,
                            loc: file.loc(unit.0),
                        })
                        .map_err(|problem| diags.push(problem))
                        .ok()?,
                ),
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
    let _ = loc;
    Some(resolved)
}

fn make_flow<'s>(
    world: &mut World<'s>,
    home: Home,
    file: &ast::File<'s>,
    day: Day,
    from: ResolvedEnd,
    to: ResolvedEnd,
    out: Option<Quantity<'s>>,
    arrive: Option<Quantity<'s>>,
    mut tail: Tail,
    header_codes: Run<axiom_core::Sym>,
    txn: Id<crate::journal::Txn>,
    loc: Loc,
    roots: &Map<ast::ExprId, NodeId>,
    diags: &mut Vec<Diagnostic>,
) -> Option<(Flow, Option<FlowExpressions>)> {
    let out = out.and_then(|quantity| {
        resolve_quantity(
            world,
            file,
            quantity,
            world.book.base,
            FlowSide::Out,
            roots,
            diags,
        )
    });
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
                Diagnostic::error(
                    "flow-amount",
                    "a flow needs an amount or an inference marker",
                )
                .label(loc, "no quantity is stated"),
            );
            return None;
        }
        (Some(a), Some(b)) => {
            let infer = if !matches!(a.infer, Infer::Known) {
                a.infer
            } else {
                b.infer
            };
            (
                a.amount,
                b.amount,
                infer,
                if a.mode == Mode::Pending || b.mode == Mode::Pending {
                    Mode::Pending
                } else {
                    Mode::Actual
                },
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
            Diagnostic::error(
                "flow-amount-mismatch",
                "a transfer has the same amount at both ends",
            )
            .label(loc, "the two written amounts differ"),
        );
        return None;
    }
    let root_exprs = (out.and_then(|q| q.root), arrive.and_then(|q| q.root));
    let basis_root = tail.basis_root;
    if let Some((rate, quote, at)) = tail.price {
        if let (Some(out), None) = (out.filter(|out| out.root.is_none()), arrive) {
            let computed = priced(world, out.amount, quote, rate, at, diags)?;
            tail.price = None;
            return make_resolved_flow(
                world,
                day,
                from,
                to,
                out.amount,
                computed,
                infer,
                mode,
                tail,
                header_codes,
                Run::new(Id::new(world.book.codes.len() as u32), 0),
                txn,
                loc,
                diags,
            )
            .map(|flow| {
                (
                    flow,
                    (root_exprs.0.is_some() || basis_root.is_some()).then_some(FlowExpressions {
                        flow: 0,
                        out: root_exprs.0,
                        arrive: None,
                        basis: basis_root,
                    }),
                )
            });
        }
        diags.push(
            Diagnostic::error(
                "price-shape",
                "a written price needs one literal amount on the source side",
            )
            .label(at, "this price cannot be applied to the written quantities")
            .help(
                "write one literal source quantity and let the price determine the arriving amount",
            ),
        );
        return None;
    }
    let local_codes = append_codes(world, &[]);
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
        local_codes,
        txn,
        loc,
        diags,
    )?;
    let expressions = (root_exprs.0.is_some() || root_exprs.1.is_some() || basis_root.is_some())
        .then_some(FlowExpressions {
            flow: 0,
            out: root_exprs.0,
            arrive: root_exprs.1,
            basis: basis_root,
        });
    let _ = home;
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
    txn: Id<crate::journal::Txn>,
    loc: Loc,
    diags: &mut Vec<Diagnostic>,
) -> Option<Flow> {
    let mut detail = tail.detail;
    detail.spender = from.entity;
    let detail = (detail != Detail::NONE).then(|| world.book.details.push(detail));
    if to.select.len() != 0 {
        diags.push(
            Diagnostic::error(
                "selector-target",
                "selectors narrow the source endpoint of a flow",
            )
            .label(loc, "this endpoint only receives"),
        );
        return None;
    }
    let select = from.select;
    let owner = world.book.places[from.place].owner;
    let payee = tail.payee.or(to.entity).or(from.entity);
    let recognized = tail.recognized.unwrap_or(Days::on(day));
    if !recognized.contains(day) {
        diags.push(
            Diagnostic::error(
                "recognition-range",
                "a flow day must be inside its recognition period",
            )
            .label(loc, "the written day is outside `for`"),
        );
        return None;
    }
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
        purpose: tail.purpose,
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
    let mut tail = Tail {
        valid: true,
        ..Tail::default()
    };
    for clause in &file[clauses] {
        match clause.kind {
            ClauseKind::Purpose(written) => {
                let purpose = world.purpose(
                    home,
                    Word {
                        text: written.name.0,
                        loc: file.loc(written.name.0),
                    },
                );
                let of = written
                    .of
                    .and_then(|name| resolve_object(world, home, file, name, diags));
                match (purpose, written.of.is_some(), of) {
                    (Ok(purpose), false, _) | (Ok(purpose), true, Some(_)) => {
                        tail.purpose = Some(Purposed {
                            purpose,
                            of,
                            source: Provenance::Written,
                        });
                    }
                    (Err(problem), _, _) => {
                        diags.push(problem);
                        tail.valid = false;
                    }
                    (Ok(_), true, None) => tail.valid = false,
                }
            }
            ClauseKind::Description(text) => {
                tail.description = Some(world.book.quoted_text(text.0))
            }
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
            ClauseKind::For(ast::For::Whom(name)) => match world.entity(
                home,
                Word {
                    text: name.0,
                    loc: file.loc(name.0),
                },
            ) {
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
            ClauseKind::Via(name) => match world.entity(
                home,
                Word {
                    text: name.0,
                    loc: file.loc(name.0),
                },
            ) {
                Ok(entity) => tail.payee = Some(entity),
                Err(problem) => {
                    diags.push(problem);
                    tail.valid = false;
                }
            },
            ClauseKind::Basis(ast::Amount::Literal(literal)) => {
                let Some(unit) = literal.unit().and_then(|unit| {
                    world
                        .commodity_of(Word {
                            text: unit.0,
                            loc: file.loc(unit.0),
                        })
                        .ok()
                }) else {
                    diags.push(
                        Diagnostic::error(
                            "basis-unit",
                            "basis needs an explicit base-currency unit",
                        )
                        .label(file.loc(literal.0), "write the unit"),
                    );
                    tail.valid = false;
                    continue;
                };
                match world.amount(literal.num(), unit, file.loc(literal.0)) {
                    Ok(amount) if amount.unit == world.book.base => {
                        tail.detail.basis = Some(amount.qty)
                    }
                    Ok(_) => {
                        diags.push(
                            Diagnostic::error(
                                "basis-unit",
                                "basis must be stated in the base currency",
                            )
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
                        Diagnostic::error(
                            "computed-basis",
                            "computed basis expression was not compiled for this flow",
                        )
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
                let Ok(unit) = world.commodity_of(Word {
                    text: name.0,
                    loc: file.loc(name.0),
                }) else {
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
                tail.detail.against = code_index.resolve(world, code, clause.at, diags);
                tail.valid &= tail.detail.against.is_some();
            }
            ClauseKind::Until(_) => {
                diags.push(
                    Diagnostic::error(
                        "until-position",
                        "`until` is only valid on a statement change or waiver",
                    )
                    .label(clause.at, "it has no effect on a flow"),
                );
                tail.valid = false;
            }
            ClauseKind::Waive(waive) => {
                tail.waive = Some(Waive {
                    loc: waive.at,
                    reason: waive.reason.map(|text| world.book.quoted_text(text.0)),
                });
            }
        }
    }
    (
        Run::new(
            Id::new(start as u32),
            (world.book.codes.len() - start) as u32,
        ),
        tail,
    )
}

fn resolve_end<'s>(
    world: &mut World<'s>,
    home: Home,
    file: &ast::File<'s>,
    written: ast::End<'s>,
    diags: &mut Vec<Diagnostic>,
) -> Option<ResolvedEnd> {
    let word = Word {
        text: written.name.0,
        loc: file.loc(written.name.0),
    };
    let end = match world.end(home, word) {
        Ok(end) => end,
        Err(problem) => {
            diags.push(problem);
            return None;
        }
    };
    let start = world.book.selectors.len();
    for selector in &file[written.select] {
        let resolved = match *selector {
            ast::Select::Range(first, last, _) => Days::new(first, last).map(Select::Range),
            ast::Select::Code(code) => Some(Select::Code(world.book.names.intern(code.name()))),
            ast::Select::Policy(policy, _) => Some(Select::Policy(policy)),
            ast::Select::Purpose(name) => match world.purpose(
                home,
                Word {
                    text: name.0,
                    loc: file.loc(name.0),
                },
            ) {
                Ok(id) => Some(Select::Purpose(id)),
                Err(problem) => {
                    diags.push(problem);
                    None
                }
            },
            ast::Select::Unit(name) => match world.commodity_of(Word {
                text: name.0,
                loc: file.loc(name.0),
            }) {
                Ok(id) => Some(Select::Unit(id)),
                Err(problem) => {
                    diags.push(problem);
                    None
                }
            },
            ast::Select::End(name) => match world.end(
                home,
                Word {
                    text: name.0,
                    loc: file.loc(name.0),
                },
            ) {
                Ok(id) => Some(Select::End(id.place)),
                Err(problem) => {
                    diags.push(problem);
                    None
                }
            },
        };
        match resolved {
            Some(select) => {
                world.book.selectors.push(select);
            }
            None => diags.push(
                Diagnostic::error(
                    "selector-range",
                    "this selector does not name a valid range or target",
                )
                .label(file.loc(written.name.0), "invalid selector on this end"),
            ),
        }
    }
    Some(ResolvedEnd {
        place: end.place,
        entity: end.entity,
        select: Run::new(
            Id::new(start as u32),
            (world.book.selectors.len() - start) as u32,
        ),
    })
}

fn lower_items<'s>(
    world: &mut World<'s>,
    home: Home,
    file: &ast::File<'s>,
    items: ast::Many<ast::LineItem<'s>>,
    from: ResolvedEnd,
    to: ResolvedEnd,
    parent_side: crate::book::FlowSide,
    txn: Id<crate::journal::Txn>,
    day: Day,
    header_codes: Run<axiom_core::Sym>,
    roots: &Map<ast::ExprId, NodeId>,
    code_index: &CodeIndex,
    mode: Mode,
    inherited_tail: Option<&Tail>,
    flow_roots: &mut Vec<FlowExpressions>,
    first_flow: usize,
    diags: &mut Vec<Diagnostic>,
) -> Box<[JournalItem]> {
    let mut lowered = Vec::with_capacity(items.len());
    for item in &file[items] {
        let Some(amount) = resolve_amount(world, file, item.amount, world.book.base, roots, diags)
        else {
            continue;
        };
        let (local_codes, item_tail) =
            lower_tail(world, home, file, item.tail, day, roots, code_index, diags);
        let mut tail = inherited_tail.cloned().unwrap_or(Tail {
            valid: true,
            ..Tail::default()
        });
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
            let resolved = ResolvedQuantity {
                amount: amount.0,
                infer: Infer::Known,
                mode,
                root: amount.1,
                group: JournalQuantity::Amount(amount.0, amount.1),
            };
            let from_flow = if item.sign == ast::Sign::Less {
                to
            } else {
                from
            };
            let to_flow = if item.sign == ast::Sign::Less {
                from
            } else {
                to
            };
            let (out, arrive) = if parent_side == crate::book::FlowSide::Out {
                (resolved.amount, resolved.amount)
            } else {
                (resolved.amount, resolved.amount)
            };
            let basis_root = tail.basis_root;
            make_resolved_flow(
                world,
                day,
                from_flow,
                to_flow,
                out,
                arrive,
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
                let offset = (world.book.flows.len() - first_flow) as u32;
                world.book.flows.push(flow);
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
            amount: amount
                .1
                .map_or(TemplateAmount::Literal(amount.0), TemplateAmount::Computed),
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
        roots.push(FlowExpressions {
            flow,
            out,
            arrive,
            basis,
        });
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
            let unit = match literal.unit() {
                Some(unit) => world
                    .commodity_of(Word {
                        text: unit.0,
                        loc: file.loc(unit.0),
                    })
                    .map_err(|problem| diags.push(problem))
                    .ok()?,
                None => fallback,
            };
            world
                .amount(literal.num(), unit, file.loc(literal.0))
                .map(|amount| (amount, None))
                .map_err(|problem| diags.push(problem))
                .ok()
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
    let word = Word {
        text: name.0,
        loc: file.loc(name.0),
    };
    if let Ok(end) = world.end(home, word) {
        return Some(end.entity.map_or(Object::Place(end.place), Object::Entity));
    }
    match world.place(word) {
        Ok(place) => Some(Object::Place(place)),
        Err(problem) => {
            diags.push(problem);
            None
        }
    }
}

fn merge_tail(mut parent: Tail, child: Tail) -> Tail {
    if child.purpose.is_some() {
        parent.purpose = child.purpose;
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

fn append_codes(world: &mut World<'_>, codes: &[axiom_core::Sym]) -> Run<axiom_core::Sym> {
    let start = world.book.codes.len();
    for &code in codes {
        world.book.codes.push(code);
    }
    Run::new(Id::new(start as u32), codes.len() as u32)
}

fn journal_end(end: ResolvedEnd) -> JournalEnd {
    JournalEnd {
        place: end.place,
        entity: end.entity,
    }
}

fn rollback(
    world: &mut World<'_>,
    flows: usize,
    codes: usize,
    selectors: usize,
    details: usize,
    programs: usize,
) {
    world.book.flows.truncate(flows);
    world.book.codes.truncate(codes);
    world.book.selectors.truncate(selectors);
    world.book.details.truncate(details);
    world.book.journal_programs.truncate(programs);
}

fn push_empty_txn<'s>(
    world: &mut World<'s>,
    item: &ast::Item<'s>,
    day: Day,
    code_start: usize,
    _diags: &mut Vec<Diagnostic>,
) {
    world.book.txns.push(crate::journal::Txn {
        day,
        flows: Run::new(Id::new(world.book.flows.len() as u32), 0),
        inputs: Run::new(Id::new(world.book.input_values.len() as u32), 0),
        program: None,
        codes: Run::new(
            Id::new(code_start as u32),
            (world.book.codes.len() - code_start) as u32,
        ),
        waive: None,
        contract: None,
        contract_schedule: None,
        ends: false,
        doc: item.doc.map(|doc| world.book.names.intern(doc.0)),
        loc: item.loc,
    });
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
            let current =
                Day::from_ymd(year, quarter_month, 1).expect("quarter starts in calendar");
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
            Diagnostic::error(
                "price-transfer",
                "a price cannot change a same-commodity transfer",
            )
            .label(loc, "remove the price"),
        );
        return None;
    }
    let (from, to) = (
        world.book.commodities[amount.unit].scale,
        world.book.commodities[quote].scale,
    );
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
                Diagnostic::error(
                    "price-overflow",
                    "the priced amount is outside the supported range",
                )
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
