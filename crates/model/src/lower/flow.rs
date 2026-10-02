//! Making a flow from what was written: its ends, its quantities, its tail and the items under it.

use axiom_core::{Day, Days, Diagnostic, Id, Loc, Map, Qty, Ratio, Run, Sym};
use axiom_syntax as ast;
use axiom_syntax::{ClauseKind, Quantity};

use super::infer::infer_for_flow;
use super::push_amount_root;
use super::record::CodeIndex;
use super::staged::Staged;
use super::tail::Tail;
use crate::book::{Amount, Commodity, FlowSide, Place, Sign, TemplateAmount, TemplateItemParent};
use crate::declare::World;
use crate::errors::{Reported, Word};
use crate::journal::{
    Detail, Flow, FlowExpressions, Infer, JournalEnd, JournalItem, JournalQuantity, Mode, Origin, Select, Txn,
};
use crate::law::{NodeId, Ty};
use crate::resolve::End;
use crate::scope::Home;

#[derive(Clone, Copy)]
pub(super) struct ResolvedEnd {
    pub place: Id<Place>,
    pub entity: Option<Id<crate::book::Entity>>,
    pub select: Run<Select>,
}

impl ResolvedEnd {
    /// The end without its lot selectors: the place, and the party at it.
    pub fn end(self) -> End {
        End { place: self.place, entity: self.entity }
    }
}

#[derive(Clone, Copy)]
pub(super) struct ResolvedQuantity {
    pub amount: Amount,
    pub infer: Infer,
    pub mode: Mode,
    pub root: Option<NodeId>,
    pub group: JournalQuantity,
}

/// What the flows of one record are made against: where it is written, what its expressions compiled to, and
/// which transaction they belong to.
#[derive(Clone, Copy)]
pub(super) struct FlowCx<'a, 's> {
    pub file: &'a ast::File<'s>,
    pub home: Home,
    /// The day the record is dated, which a relative `for` and `due` count from.
    pub day: Day,
    pub txn: Id<Txn>,
    /// The record's own place in the source.
    pub loc: Loc,
    pub roots: &'a Map<ast::ExprId, NodeId>,
    pub code_index: &'a CodeIndex,
}

/// A transaction being lowered: what its flows are made against, and what its header says, which each carries.
pub(super) struct TxnCx<'c, 's> {
    pub cx: FlowCx<'c, 's>,
    pub flow: &'c ast::Flow<'s>,
    pub tail: Tail,
    pub codes: Run<Sym>,
}

/// The two ends of a flow.
#[derive(Clone, Copy)]
pub(super) struct Ends {
    pub from: ResolvedEnd,
    pub to: ResolvedEnd,
}

/// The codes a flow carries: its header's, which every flow of the record inherits, and its own.
#[derive(Clone, Copy)]
pub(super) struct Codes {
    pub header: Run<Sym>,
    pub local: Run<Sym>,
}

/// What a flow moves: the amounts out of one end and into the other, and how sure they are.
#[derive(Clone, Copy)]
pub(super) struct Shape {
    pub ends: Ends,
    pub out: Amount,
    pub arrive: Amount,
    pub infer: Infer,
    pub mode: Mode,
}

/// The flow whose items are being lowered: between which ends, on which side, how it is made and what it says.
#[derive(Clone, Copy)]
pub(super) struct Parent<'t> {
    pub ends: Ends,
    pub side: FlowSide,
    pub mode: Mode,
    pub header_codes: Run<Sym>,
    /// What its tail says, which an item's own tail adds to.
    pub tail: Option<&'t Tail>,
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

impl ResolvedQuantity {
    fn new(amount: Amount, infer: Infer, mode: Mode, root: Option<NodeId>, group: JournalQuantity) -> ResolvedQuantity {
        ResolvedQuantity { amount, infer, mode, root, group }
    }
}

/// What a quantity written in a flow is, in `fallback`'s unit when it names none. None when it cannot be, which is
/// said for a commodity that does not exist and, as it always was, for nothing else.
pub(super) fn resolve_quantity<'s>(
    world: &mut World<'s>,
    cx: &FlowCx<'_, 's>,
    quantity: Quantity<'s>,
    fallback: Id<crate::book::Commodity>,
    side: FlowSide,
    diags: &mut Vec<Diagnostic>,
) -> Option<ResolvedQuantity> {
    let file = cx.file;
    let resolved = match quantity {
        Quantity::Amount(written) => {
            let (amount, root) = stated_amount(world, cx, written, fallback)?;
            let group = JournalQuantity::Amount(amount, root);
            ResolvedQuantity::new(amount, Infer::Known, Mode::Actual, root, group)
        }
        Quantity::Pending(written) => {
            let (amount, root) = stated_amount(world, cx, written, fallback)?;
            let group = JournalQuantity::Pending(amount, root);
            ResolvedQuantity::new(amount, Infer::Known, Mode::Pending, root, group)
        }
        Quantity::Target(written) => {
            let (amount, root) = stated_amount(world, cx, written, fallback)?;
            let end = if side == FlowSide::Out { crate::journal::End::From } else { crate::journal::End::To };
            let infer = Infer::Target { end, balance: amount.qty };
            ResolvedQuantity::new(amount, infer, Mode::Actual, root, JournalQuantity::Target(amount, root))
        }
        Quantity::Unknown(unit) => {
            let unit = world.commodity_of(Word::of(file, unit.0)).or_report(diags)?;
            ResolvedQuantity::new(
                Amount::zero(unit),
                Infer::Unknown,
                Mode::Actual,
                None,
                JournalQuantity::Unknown(unit),
            )
        }
        Quantity::All(unit) => {
            let unit = match unit {
                Some(unit) => Some(world.commodity_of(Word::of(file, unit.0)).or_report(diags)?),
                None => None,
            };
            let amount = Amount::zero(unit.unwrap_or(fallback));
            ResolvedQuantity::new(amount, Infer::All, Mode::Actual, None, JournalQuantity::All(unit))
        }
        Quantity::Rest => {
            ResolvedQuantity::new(Amount::zero(fallback), Infer::Known, Mode::Actual, None, JournalQuantity::Rest)
        }
        Quantity::Whole => {
            let amount = Amount::new(Qty(1), fallback);
            ResolvedQuantity::new(amount, Infer::Known, Mode::Opening, None, JournalQuantity::Whole)
        }
    };
    Some(resolved)
}

/// A written amount: its literal, or zero and the node that computes it. A literal that is no amount costs the
/// whole quantity and says nothing: the diagnostic is dropped here, as it always was, and is not the caller's to
/// report.
fn stated_amount<'s>(
    world: &World<'s>,
    cx: &FlowCx<'_, 's>,
    written: ast::Amount<'s>,
    fallback: Id<crate::book::Commodity>,
) -> Option<(Amount, Option<NodeId>)> {
    match written {
        ast::Amount::Literal(literal) => Some((world.literal_amount(cx.file, literal, Some(fallback)).ok()?, None)),
        ast::Amount::Computed(expr) => Some((Amount::zero(fallback), Some(*cx.roots.get(&expr)?))),
    }
}

/// The flow a header with both its ends named makes: what it says moves, checked and priced, and the expressions
/// its amounts and basis are computed by.
pub(super) fn make_flow<'s>(
    world: &mut World<'s>,
    txn: TxnCx<'_, 's>,
    ends: Ends,
    diags: &mut Vec<Diagnostic>,
) -> Option<(Flow, Option<FlowExpressions>)> {
    let TxnCx { cx, flow: written, mut tail, codes: header_codes } = txn;
    let cx = &cx;
    let loc = cx.loc;
    let out = written
        .from
        .amount
        .and_then(|quantity| resolve_quantity(world, cx, quantity, world.book.base, FlowSide::Out, diags));
    let arrive = written.to.amount.and_then(|quantity| {
        let fallback = out.map_or(world.book.base, |out| out.amount.unit);
        resolve_quantity(world, cx, quantity, fallback, FlowSide::Arrive, diags)
    });
    let (mut out_amount, mut arrive_amount, infer, mode) = stated_amounts(loc, out, arrive, diags)?;
    let roots = (out.and_then(|quantity| quantity.root), arrive.and_then(|quantity| quantity.root));
    let basis_root = tail.basis_root;
    if let Some(price) = tail.price.take() {
        (out_amount, arrive_amount) = apply_price(world, out, arrive, price, loc, diags)?;
    }
    let codes = Codes { header: header_codes, local: empty_codes(world) };
    let shape = Shape { ends, out: out_amount, arrive: arrive_amount, infer, mode };
    let flow = make_resolved_flow(world, cx, shape, codes, tail, loc, diags)?;
    let expressions = (roots.0.is_some() || roots.1.is_some() || basis_root.is_some()).then_some(FlowExpressions {
        flow: 0,
        out: roots.0,
        arrive: roots.1,
        basis: basis_root,
    });
    Some((flow, expressions))
}

/// The amounts a header states for each side, whichever sides it states, and how sure they are; a transfer states
/// the same amount at both ends.
fn stated_amounts(
    loc: Loc,
    out: Option<ResolvedQuantity>,
    arrive: Option<ResolvedQuantity>,
    diags: &mut Vec<Diagnostic>,
) -> Option<(Amount, Amount, Infer, Mode)> {
    let stated = match (out, arrive) {
        (None, None) => {
            diags.push(
                Diagnostic::error("flow-amount", "a flow needs an amount or an inference marker")
                    .label(loc, "no quantity is stated"),
            );
            return None;
        }
        (Some(a), Some(b)) => {
            let infer = if !matches!(a.infer, Infer::Known) { a.infer } else { b.infer };
            let mode = if a.mode == Mode::Pending || b.mode == Mode::Pending { Mode::Pending } else { Mode::Actual };
            (a.amount, b.amount, infer, mode)
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
    Some(stated)
}

/// `@ 285.70 USD`: the amount at each end once the price has said what the side that was not written is.
fn apply_price(
    world: &World<'_>,
    out: Option<ResolvedQuantity>,
    arrive: Option<ResolvedQuantity>,
    (rate, quote, at): (Ratio, Id<Commodity>, Loc),
    loc: Loc,
    diags: &mut Vec<Diagnostic>,
) -> Option<(Amount, Amount)> {
    let quoted = match (out, arrive) {
        (Some(out), None) if out.root.is_none() => Some(priced(world, out.amount, quote, rate, at, diags)?),
        (None, Some(arrive)) if arrive.root.is_none() => Some(priced(world, arrive.amount, quote, rate, at, diags)?),
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
    match (out, arrive, quoted) {
        (Some(out), None, Some(arrive)) => Some((out.amount, arrive)),
        (None, Some(arrive), Some(out)) => Some((out, arrive.amount)),
        (Some(out), Some(arrive), None) => Some((out.amount, arrive.amount)),
        _ => None,
    }
}

pub(super) fn make_resolved_flow(
    world: &mut World<'_>,
    cx: &FlowCx,
    shape: Shape,
    codes: Codes,
    tail: Tail,
    loc: Loc,
    diags: &mut Vec<Diagnostic>,
) -> Option<Flow> {
    let Shape { ends: Ends { from, to }, out, arrive, infer, mode } = shape;
    let (day, txn) = (cx.day, cx.txn);
    let purpose = tail.purpose.map(|purpose| (purpose, tail.purpose_loc.unwrap_or(loc)));
    let purpose = infer_for_flow(world, from.end(), to.end(), purpose, loc, diags).ok()?;
    let mut detail = tail.detail;
    detail.spender = from.entity;
    let detail = (detail != Detail::NONE).then(|| world.book.details.push(detail));
    if !to.select.is_empty() {
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
        header_codes: codes.header,
        codes: codes.local,
        loc,
        waive: tail.waive,
        detail,
    })
}

pub(super) fn resolve_end<'s>(
    world: &mut World<'s>,
    cx: &FlowCx<'_, 's>,
    written: ast::End<'s>,
    diags: &mut Vec<Diagnostic>,
) -> Option<ResolvedEnd> {
    let (home, file) = (cx.home, cx.file);
    let word = Word::of(file, written.name.0);
    let end = world.end(home, word).or_report(diags)?;
    let start = world.book.selectors.len();
    for selector in &file[written.select] {
        let resolved = match *selector {
            ast::Select::Range(first, last, _) => Days::new(first, last).map(Select::Range),
            ast::Select::Code(code) => Some(Select::Code(world.book.names.intern(code.name()))),
            ast::Select::Policy(policy, _) => Some(Select::Policy(policy)),
            ast::Select::Purpose(name) => {
                world.purpose(home, Word::of(file, name.0)).or_report(diags).map(Select::Purpose)
            }
            ast::Select::Unit(name) => world.commodity_of(Word::of(file, name.0)).or_report(diags).map(Select::Unit),
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

/// The items under a flow: each is lowered to its place in the transaction's groups, and to a flow of its own
/// when it says something its parent does not.
pub(super) fn lower_items<'s>(
    staged: &mut Staged<'_, 's>,
    cx: &FlowCx<'_, 's>,
    items: ast::Many<ast::LineItem<'s>>,
    parent: Parent<'_>,
    flow_roots: &mut Vec<FlowExpressions>,
    diags: &mut Vec<Diagnostic>,
) -> Box<[JournalItem]> {
    let mut lowered = Vec::with_capacity(items.len());
    for item in &cx.file[items] {
        let Some(amount) = resolve_amount(staged, cx, item.amount, staged.book.base, diags) else {
            continue;
        };
        let (local_codes, item_tail) = cx.lower_tail(staged, item.tail, diags);
        let tail = parent.tail.cloned().unwrap_or_else(Tail::new).merge(item_tail);
        let says_something = tail.purpose.is_some()
            || tail.description.is_some()
            || tail.detail != Detail::NONE
            || tail.basis_root.is_some()
            || tail.payee.is_some()
            || tail.recognized.is_some()
            || tail.price.is_some()
            || tail.waive.is_some()
            || !local_codes.is_empty();
        let flow = if says_something {
            let basis_root = tail.basis_root;
            let (from, to) = match item.sign {
                ast::Sign::Less => (parent.ends.to, parent.ends.from),
                _ => (parent.ends.from, parent.ends.to),
            };
            let shape = Shape {
                ends: Ends { from, to },
                out: amount.0,
                arrive: amount.0,
                infer: Infer::Known,
                mode: parent.mode,
            };
            let codes = Codes { header: parent.header_codes, local: local_codes };
            make_resolved_flow(staged, cx, shape, codes, tail, item.loc, diags).map(|flow| {
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
            side: parent.side,
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
    cx: &FlowCx<'_, 's>,
    amount: ast::Amount<'s>,
    fallback: Id<crate::book::Commodity>,
    diags: &mut Vec<Diagnostic>,
) -> Option<(Amount, Option<NodeId>)> {
    match amount {
        ast::Amount::Literal(literal) => {
            world.literal_amount(cx.file, literal, Some(fallback)).or_report(diags).map(|amount| (amount, None))
        }
        ast::Amount::Computed(root) => Some((Amount::zero(fallback), Some(*cx.roots.get(&root)?))),
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
