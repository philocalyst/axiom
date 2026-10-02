//! Making a flow from what was written: its ends, its quantities, its tail and the items under it.

use axiom_core::{Day, Days, Diagnostic, Id, Loc, Map, Qty, Run, Sym};
use axiom_syntax as ast;
use axiom_syntax::{ClauseKind, Quantity};

use super::push_amount_root;
use super::record::{CodeIndex, infer_for_flow};
use super::staged::Staged;
use super::tail::{FlowTail, Tail};
use crate::book::{Amount, FlowSide, Place, Sign, TemplateAmount, TemplateItemParent};
use crate::declare::World;
use crate::errors::{Reported, Word};
use crate::journal::{
    Detail, Flow, FlowExpressions, Infer, JournalEnd, JournalItem, JournalQuantity, Mode, Origin, Select, Txn,
};
use crate::law::{NodeId, Ty};
use crate::scope::Home;

#[derive(Clone, Copy)]
pub(super) struct ResolvedEnd {
    pub place: Id<Place>,
    pub entity: Option<Id<crate::book::Entity>>,
    pub select: Run<Select>,
}

#[derive(Clone, Copy)]
pub(super) struct ResolvedQuantity {
    pub amount: Amount,
    pub infer: Infer,
    pub mode: Mode,
    pub root: Option<NodeId>,
    pub group: JournalQuantity,
}

pub(super) fn flow_roots<'s>(file: &ast::File<'s>, flow: &ast::Flow<'s>) -> Vec<(ast::ExprId, Ty)> {
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

pub(super) fn push_tail_roots<'s>(
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

pub(super) fn push_quantity_root<'s>(quantity: Quantity<'s>, roots: &mut Vec<(ast::ExprId, Ty)>) {
    match quantity {
        Quantity::Amount(amount) | Quantity::Pending(amount) | Quantity::Target(amount) => {
            push_amount_root(amount, roots)
        }
        Quantity::Unknown(_) | Quantity::All(_) | Quantity::Rest | Quantity::Whole => {}
    }
}

pub(super) fn resolve_quantity<'s>(
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

pub(super) fn make_flow<'s>(
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

pub(super) fn make_resolved_flow(
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

pub(super) fn resolve_end<'s>(
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

pub(super) fn lower_items<'s>(
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
        let (local_codes, item_tail) = FlowTail { home, file, day, roots, code_index }.lower(staged, item.tail, diags);
        let mut tail = inherited_tail.cloned().unwrap_or(Tail::new());
        tail = tail.merge(item_tail);
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

pub(super) fn push_flow_expressions(
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

pub(super) fn resolve_amount<'s>(
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

/// An empty run of codes at the end of the pool: a flow with none of its own says where they would have gone.
pub(super) fn empty_codes(world: &World<'_>) -> Run<Sym> {
    Run::new(Id::new(world.book.codes.len() as u32), 0)
}

pub(super) fn journal_end(end: ResolvedEnd) -> JournalEnd {
    JournalEnd { place: end.place, entity: end.entity }
}

pub(super) fn priced(
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

pub(super) trait OtherSide {
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
