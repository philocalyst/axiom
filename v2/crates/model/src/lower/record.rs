//! Native S5 journal records. This pass reads the source AST directly and
//! appends resolved records to the pooled Book arenas.

use axiom_core::{Day, Days, Diagnostic, Id, Loc, Map, Qty, Run, Span};
use axiom_syntax as ast;
use axiom_syntax::{ClauseKind, ItemKind, Quantity, Subject};

use super::push_amount_root;
use crate::book::{Amount, FlowSide, Sign, TemplateAmount, TemplateItemParent, Text};
use crate::declare::World;
use crate::errors::Word;
use crate::journal::{
    Action, Assert, Detail, Event, Flow, FlowExpressions, Gap, Infer, JournalEnd, JournalGroup,
    JournalItem, JournalProgram, JournalQuantity, Measure, Mode, Object, Origin, Provenance,
    Purposed, Quote, Reading, Select, Split, Waive,
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
    world.book.codes.reserve(dated.len().saturating_mul(2));
    world
        .book
        .selectors
        .reserve(expected_flows.saturating_mul(2));

    for record in dated {
        let site = &sites[record.site as usize];
        let file = &site.source.file;
        let item = &file.items[record.item as usize];
        match item.kind {
            ItemKind::Txn(id) => lower_txn(world, site, item, &file[id], diags),
            ItemKind::Opening(id) => lower_opening(world, site, item, &file[id], diags),
            ItemKind::Statement(id) => lower_statement(world, site, item.loc, &file[id], diags),
            _ => unreachable!("dated index only contains journal records"),
        }
    }
}

fn lower_txn<'a, 's>(
    world: &mut World<'s>,
    site: &Site<'a, 's>,
    item: &ast::Item<'s>,
    written: &ast::Txn<'s>,
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
            let (leg_codes, leg_tail) =
                lower_tail(world, home, file, leg.tail, written.date, &root_ids, diags);
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
        let Some(end) = resolve_end(world, site.home, file, leg.end, diags) else {
            continue;
        };
        let Some(quantity) = resolve_quantity(
            world,
            file,
            leg.amount,
            world.book.base,
            FlowSide::Out,
            &Map::default(),
            diags,
        ) else {
            continue;
        };
        if !matches!(leg.amount, Quantity::Amount(ast::Amount::Literal(_))) {
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
            flow.owner = world.book.places[end.place].owner;
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
            claim,
            diags,
        );
    }
}

fn lower_statement<'a, 's>(
    world: &mut World<'s>,
    site: &Site<'a, 's>,
    loc: Loc,
    statement: &ast::Statement<'s>,
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
                world, site.home, file, loc, statement, *amount, action, diags,
            );
        }
        ast::Verb::Event(state) => {
            let Subject::Code(code) = statement.subject else {
                unsupported_statement(loc, "events need a code subject", diags);
                return;
            };
            world.book.events.push(Event {
                day: statement.date,
                code: world.sym(code.name()),
                state: *state,
                loc,
            });
        }
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
        ast::Verb::Occurrence(_)
        | ast::Verb::Owes { .. }
        | ast::Verb::Now(_)
        | ast::Verb::Waived
        | ast::Verb::Ends
        | ast::Verb::Basis { .. }
        | ast::Verb::Filed(_) => unsupported_statement(
            loc,
            "this statement kind does not yet have a native record lowering",
            diags,
        ),
    }
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
    world: &World<'s>,
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
        Subject::Code(code) => Some(StatementTarget::Code(world.sym(code.name()))),
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
    let ast::Amount::Literal(literal) = value else {
        diags.push(
            Diagnostic::error("computed-value", "a value assertion needs a literal amount").label(
                loc,
                "computed assertions are not yet retained by the journal",
            ),
        );
        return;
    };
    let target = statement_target(world, home, file, statement.subject, diags);
    let Some(target) = target else { return };
    match target {
        StatementTarget::Place(place) => {
            let fallback = match world.book.places[place].holds.as_deref() {
                Some([unit]) => Some(*unit),
                _ => Some(world.book.base),
            };
            let Some(amount) = literal_amount(world, file, literal, fallback, diags) else {
                return;
            };
            let Some(gap) = assertion_gap(world, home, file, statement, diags) else {
                return;
            };
            world.book.asserts.push(Assert {
                day: statement.date,
                place,
                amount,
                gap,
                loc,
            });
        }
        StatementTarget::Asset(asset) => {
            let place = world.book.assets[asset].place;
            let Some(amount) = literal_amount(world, file, literal, Some(world.book.base), diags)
            else {
                return;
            };
            let Some(gap) = assertion_gap(world, home, file, statement, diags) else {
                return;
            };
            world.book.asserts.push(Assert {
                day: statement.date,
                place,
                amount,
                gap,
                loc,
            });
        }
        StatementTarget::Code(code) => {
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
            ClauseKind::Code(code) => codes.push(world.sym(code.name())),
            _ => diags.push(
                Diagnostic::error(
                    "measure-tail",
                    "this tail clause does not apply to a measure",
                )
                .label(clause.at, "remove the clause or record it on a flow"),
            ),
        }
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
                let symbol = world.sym(code.name());
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
            ClauseKind::Against(_) => {
                diags.push(
                    Diagnostic::error(
                        "against-lookup",
                        "the referenced transaction code is not resolved in this pass",
                    )
                    .label(clause.at, "against metadata was not retained"),
                );
                tail.valid = false;
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
            ast::Select::Code(code) => Some(Select::Code(world.sym(code.name()))),
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
        let (local_codes, tail) = lower_tail(world, home, file, item.tail, day, roots, diags);
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
                mode: Mode::Actual,
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
                Mode::Actual,
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

fn resolve_object<'s>(
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
