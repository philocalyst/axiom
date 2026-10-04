//! Making a flow from what was written: its ends, its quantities, its tail and the items under it.

use std::mem;

use axiom_core::{Day, Days, Diagnostic, Id, Loc, Map, Qty, Ratio, Run, Sym};
use axiom_syntax as ast;
use axiom_syntax::ClauseKind;

use super::infer::classify;
use super::push_amount_root;
use super::record::CodeIndex;
use super::staged::Staged;
use super::tail::{self, Tail, read_tail};
use crate::balance::Settled;
use crate::book::{Amount, Commodity, Input, Place};
use crate::declare::World;
use crate::errors::{Reported, Word};
use crate::journal::{Detail, Flow, FlowExpressions, Infer, Mode, Origin, Program, Select, Txn, TxnKind};
use crate::law::{NodeId, Ty};
use crate::resolve::End;
use crate::scope::Home;
use crate::solve::{Line, LiteralEnv, Resolved};
use crate::sources::Site;
use crate::split::{Cut, Endpoint, Expr, FlowSide, Item, Made, Part, Quantity};

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

/// A quantity written in a flow, as the flow carries it and as its group keeps it.
#[derive(Clone, Copy)]
pub(super) struct ResolvedQuantity {
    pub amount: Amount,
    pub infer: Infer,
    pub mode: Mode,
    pub part: Part,
}

/// A dated record of the journal being lowered, and all its lowering needs: the book, staged so that nothing the record
/// writes stays unless it is kept; where the record is written and its day; the transaction its flows belong to; what
/// its expressions compiled to; the codes the journal has so far; and what its flows compute. A transaction, an
/// opening, a claim, a basis, a loan's origination and a written occurrence are each lowered in one: it is the context
/// of that one pass, and each step of the pass is a method of it.
pub(crate) struct Recording<'w, 'a, 's> {
    pub staged: Staged<'w, 's>,
    pub file: &'a ast::File<'s>,
    pub home: Home,
    /// The day the record is dated, which a relative `for` and `due` count from.
    pub day: Day,
    pub txn: Id<Txn>,
    /// The record's own place in the source.
    pub loc: Loc,
    pub code_index: &'a CodeIndex,
    /// The record's expressions, compiled as one program, and the node each came to.
    pub program: Program,
    pub roots: Map<ast::ExprId, NodeId>,
    /// What the record's flows compute.
    pub flow_roots: Vec<FlowExpressions>,
    /// How many diagnostics had been said when the record opened: one more makes it wrong.
    said: usize,
}

/// What a transaction's header says, which each of its flows carries.
pub(super) struct TxnHeader<'c, 's> {
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
        push_item_root(file, item.amount, &mut roots);
        push_tail_roots(file, item.tail, &mut roots);
    }
    roots
}

/// The share of its header an item says, if its amount is only `6%`: what a percentage alone under a header is of.
fn share_of(file: &ast::File<'_>, amount: ast::Amount<'_>) -> Option<Ratio> {
    let ast::Amount::Computed(expr) = amount else { return None };
    let ast::ExprKind::Pct(percent) = file.exprs[expr].kind else { return None };
    Ratio::percent(percent.mantissa as i128, percent.scale)
}

/// An item's amount is a root unless it is a share, which is not an expression the program computes.
pub(super) fn push_item_root<'s>(file: &ast::File<'s>, amount: ast::Amount<'s>, roots: &mut Vec<(ast::ExprId, Ty)>) {
    if share_of(file, amount).is_none() {
        push_amount_root(amount, roots);
    }
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

pub(super) fn push_quantity_root<'s>(quantity: ast::Quantity<'s>, roots: &mut Vec<(ast::ExprId, Ty)>) {
    match quantity {
        ast::Quantity::Amount(amount) | ast::Quantity::Pending(amount) | ast::Quantity::Target(amount) => {
            push_amount_root(amount, roots)
        }
        ast::Quantity::Unknown(_) | ast::Quantity::All(_) | ast::Quantity::Rest | ast::Quantity::Whole => {}
    }
}

impl ResolvedQuantity {
    /// What a written part comes to as a flow carries it: its amount, how it is known and how real it is. What the
    /// fold computes or the book says is the zero a flow carries meanwhile, which is what an environment that
    /// evaluates nothing makes of it. `own` is the mode of the flow that carries it.
    fn of(part: Part, own: Mode, fallback: Id<Commodity>, side: FlowSide) -> ResolvedQuantity {
        let said = match part {
            Part::Of(quantity) => {
                match quantity.resolve(&mut LiteralEnv, Line::Header(side), None, side.end(), fallback) {
                    Ok(resolved) => resolved.expect("a literal environment leaves nothing out"),
                    Err(never) => match never {},
                }
            }
            Part::Rest | Part::Share(_) => Resolved::unsaid(fallback),
        };
        ResolvedQuantity { amount: said.amount, infer: said.infer, mode: said.mode.unwrap_or(own), part }
    }

    /// The node that computes its amount, if it is computed.
    pub fn root(&self) -> Option<NodeId> {
        self.part.root()
    }

    /// What a side of a header says. A header never says a share or a remainder: the parser keeps `...` and a
    /// bare share off it.
    pub fn quantity(&self) -> Quantity {
        match self.part {
            Part::Of(quantity) => quantity,
            Part::Share(_) | Part::Rest => unreachable!("a header side says an amount, not a share or a remainder"),
        }
    }
}

impl<'w, 'a, 's> Recording<'w, 'a, 's> {
    /// A record written at `site`, dated `day`, staged on the world until it is kept.
    pub fn open(
        world: &'w mut World<'s>,
        site: &'a Site<'a, 's>,
        day: Day,
        loc: Loc,
        code_index: &'a CodeIndex,
    ) -> Self {
        let staged = Staged::open(world);
        let (txn, said, file, home) =
            (Id::new(staged.book.txns.len() as u32), staged.diags.len(), &site.source.file, site.home);
        let (program, roots, flow_roots) = (Program::default(), Map::default(), Vec::new());
        Recording { staged, file, home, day, txn, loc, code_index, program, roots, flow_roots, said }
    }

    /// Compiles the record's expressions as one program about `subject`, with a template's `inputs`; false once what is
    /// wrong is said.
    pub fn compile(&mut self, subject: Ty, roots: &[(ast::ExprId, Ty)], inputs: &[Input]) -> bool {
        let name = self.staged.book.names.intern("journal");
        let compiled = super::compile_roots(&mut self.staged, self.file, self.home, subject, name, inputs, roots);
        compiled.map(|(program, roots)| (self.program, self.roots) = (program, roots)).is_some()
    }

    /// Whether anything has been said since the record opened, which makes it wrong.
    pub fn failed(&self) -> bool {
        self.staged.diags.len() != self.said
    }

    /// Reads a line's tail: the codes it adds to the pool, and what the rest of it says.
    pub fn tail(&mut self, clauses: ast::Many<ast::Clause<'s>>) -> (Run<Sym>, Tail) {
        let line = tail::Line::Flow { day: self.day, roots: &self.roots, code_index: self.code_index };
        read_tail(&mut self.staged, self.home, self.file, line, clauses)
    }

    /// A written amount, in `fallback` when it names no unit; what is wrong with it is the caller's to say.
    pub fn amount(&self, written: ast::Amount<'s>, fallback: Id<Commodity>) -> Result<Expr, Diagnostic> {
        written_amount(&self.staged, self.file, &self.roots, written, fallback)
    }

    /// What a quantity written in a flow is, in `fallback`'s unit when it names none. None when it cannot be, which is
    /// said for a commodity that does not exist and, as it always was, for nothing else: an amount that is not one is
    /// dropped here.
    pub fn quantity(
        &mut self,
        written: ast::Quantity<'s>,
        fallback: Id<Commodity>,
        side: FlowSide,
    ) -> Option<ResolvedQuantity> {
        let read = written_part(&self.staged, self.file, &self.roots, written, fallback);
        let (part, own) = match written {
            ast::Quantity::Unknown(_) | ast::Quantity::All(_) => read.or_report(&mut self.staged)?,
            _ => read.ok()?,
        };
        Some(ResolvedQuantity::of(part, own, fallback, side))
    }

    /// An end written on one of the record's lines, with the lots it selects.
    pub fn end(&mut self, written: ast::End<'s>) -> Option<ResolvedEnd> {
        let (home, file, day, world) = (self.home, self.file, self.day, &mut *self.staged);
        let word = Word::of(file, written.name.0);
        let end = world.end_on(home, word, Some(day)).or_report(world)?;
        let start = world.book.selectors.len();
        for selector in &file[written.select] {
            let resolved = match *selector {
                ast::Select::Range(first, last, _) => Days::new(first, last).map(Select::Range),
                ast::Select::Code(code) => Some(Select::Code(world.book.names.intern(code.name()))),
                ast::Select::Policy(policy, _) => Some(Select::Policy(policy)),
                ast::Select::Purpose(name) => {
                    world.purpose(home, Word::of(file, name.0)).or_report(world).map(Select::Purpose)
                }
                ast::Select::Unit(name) => {
                    world.commodity_of(Word::of(file, name.0)).or_report(world).map(Select::Unit)
                }
                ast::Select::End(name) => world
                    .end_on(home, Word::of(file, name.0), Some(day))
                    .or_report(world)
                    .map(|id| Select::End(id.place)),
            };
            match resolved {
                Some(select) => {
                    world.book.selectors.push(select);
                }
                None => world.diags.push(
                    Diagnostic::error("selector-range", "this selector does not name a valid range or target")
                        .label(file.loc(written.name.0), "invalid selector on this end"),
                ),
            }
        }
        let select = Run::new(Id::new(start as u32), (world.book.selectors.len() - start) as u32);
        Some(ResolvedEnd { place: end.place, entity: end.entity, select })
    }

    /// A flow of the record: what moves between its ends, with what its tail says. None after what is wrong is said.
    pub fn flow(&mut self, shape: Shape, codes: Codes, tail: Tail, loc: Loc) -> Option<Flow> {
        let Shape { ends: Ends { from, to }, out, arrive, infer, mode } = shape;
        let (day, txn, world) = (self.day, self.txn, &mut *self.staged);
        let purpose = classify(world, from.end(), to.end(), tail.purpose, loc).ok()?;
        let mut detail = tail.detail;
        detail.spender = from.entity;
        let detail = (detail != Detail::NONE).then(|| world.book.details.push(detail));
        if !to.select.is_empty() {
            world.diags.push(
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

    /// The flow a header with both its ends named makes: what it says moves, checked and priced, and the expressions
    /// its amounts and basis are computed by.
    pub fn header_flow(&mut self, header: TxnHeader<'_, 's>, ends: Ends) -> Option<(Flow, Option<FlowExpressions>)> {
        let TxnHeader { flow: written, mut tail, codes: header_codes } = header;
        let (loc, base) = (self.loc, self.staged.book.base);
        let out = written.from.amount.and_then(|quantity| self.quantity(quantity, base, FlowSide::Out));
        let arrive = written.to.amount.and_then(|quantity| {
            let fallback = out.map_or(base, |out| out.amount.unit);
            self.quantity(quantity, fallback, FlowSide::Arrive)
        });
        let (mut out_amount, mut arrive_amount, infer, mode) =
            stated_amounts(loc, out, arrive, &mut self.staged.diags)?;
        let roots = (out.and_then(|quantity| quantity.root()), arrive.and_then(|quantity| quantity.root()));
        let basis_root = tail.basis_root;
        if let Some(price) = tail.price.take() {
            (out_amount, arrive_amount) = apply_price(&mut self.staged, out, arrive, price, loc)?;
        }
        let codes = Codes { header: header_codes, local: empty_codes(&self.staged) };
        let shape = Shape { ends, out: out_amount, arrive: arrive_amount, infer, mode };
        let flow = self.flow(shape, codes, tail, loc)?;
        let expressions = (roots.0.is_some() || roots.1.is_some() || basis_root.is_some()).then_some(FlowExpressions {
            flow: 0,
            out: roots.0,
            arrive: roots.1,
            basis: basis_root,
        });
        Some((flow, expressions))
    }

    /// The items under a flow: each is lowered to its place in the transaction's groups, and to a flow of its own
    /// when it says something its parent does not.
    pub fn items(&mut self, items: ast::Many<ast::LineItem<'s>>, parent: Parent<'_>) -> Box<[Item<Option<u32>>]> {
        let (file, base) = (self.file, self.staged.book.base);
        let mut lowered = Vec::with_capacity(items.len());
        for item in &file[items] {
            let cut = match share_of(file, item.amount) {
                Some(rate) => Cut::Share(rate),
                None => match self.amount(item.amount, base).or_report(&mut self.staged) {
                    Some(expr) => Cut::Of(expr),
                    None => continue,
                },
            };
            let amount = match cut {
                Cut::Of(expr) => expr.stand_in(base),
                Cut::Share(_) => Amount::zero(base),
            };
            let (local_codes, item_tail) = self.tail(item.tail);
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
                    out: amount,
                    arrive: amount,
                    infer: Infer::Known,
                    mode: parent.mode,
                };
                let codes = Codes { header: parent.header_codes, local: local_codes };
                self.flow(shape, codes, tail, item.loc).map(|flow| {
                    let offset = self.staged.flows().len();
                    self.staged.book.flows.push(flow);
                    push_flow_expressions(&mut self.flow_roots, offset, None, None, basis_root);
                    offset
                })
            } else {
                None
            };
            lowered.push(Item { sign: item.sign, amount: cut, loc: item.loc, flow });
        }
        lowered.into_boxed_slice()
    }

    /// The record's transaction, owning the flows and codes written so far; the record fills in what else it has.
    pub fn transaction(&self) -> Txn {
        journal_txn(&self.staged, self.day, self.loc)
    }

    /// The doc comment written above the record.
    pub fn doc(&mut self, doc: Option<ast::Doc<'s>>) -> Option<Sym> {
        doc.map(|doc| self.staged.book.names.intern(doc.0))
    }

    /// The record's program, with what its flows compute and the group it is, if there is anything to keep: literal
    /// transactions pay no program.
    pub fn keep_program(&mut self, group: Option<(Made, Settled)>) -> Option<Id<Program>> {
        let open = matches!(group, Some((_, Settled::Open)));
        let (roots, group) =
            (mem::take(&mut self.flow_roots).into_boxed_slice(), group.map(|(made, _)| Box::new(made)));
        let program = Program { roots, group, open, ..mem::take(&mut self.program) };
        let says = !program.nodes.is_empty() || !program.roots.is_empty() || program.group.is_some();
        says.then(|| self.staged.book.journal_programs.push(program))
    }

    /// Keeps the record: its transaction, and everything it wrote.
    pub fn keep(mut self, txn: Txn) {
        self.staged.book.txns.push(txn);
        self.staged.commit();
    }

    /// Takes back everything the record wrote, and leaves an empty transaction in its place: every dated record of the
    /// journal owns one.
    pub fn reject(mut self, item: &ast::Item<'s>) {
        let (flows, codes) = (Run::new(self.staged.flows().start(), 0), Run::new(self.staged.codes().start(), 0));
        let doc = self.doc(item.doc);
        let txn = Txn { flows, codes, doc, ..self.transaction() };
        self.staged.book.txns.push(txn);
    }
}

/// The transaction of a record of the journal itself: it owns the flows and codes staged so far, and has no program,
/// contract or inputs. The record fills in what it has.
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

/// A written amount: the literal in its unit, else in `fallback`, or the node compiled for its expression (said to be
/// missing when it is: a template's expressions are compiled with the contract, and one may have failed).
pub(super) fn written_amount<'s>(
    world: &World<'s>,
    file: &ast::File<'s>,
    roots: &Map<ast::ExprId, NodeId>,
    written: ast::Amount<'s>,
    fallback: Id<Commodity>,
) -> Result<Expr, Diagnostic> {
    match written {
        ast::Amount::Literal(literal) => world.literal_amount(file, literal, Some(fallback)).map(Expr::Literal),
        ast::Amount::Computed(root) => match roots.get(&root) {
            Some(&node) => Ok(Expr::Computed(node)),
            None => Err(Diagnostic::error("template-root", "a computed template amount was not compiled")
                .label(file.exprs[root].loc, "this amount has no typed program node")),
        },
    }
}

/// What a quantity written at one side of a line takes of its group, and how it is had, or what is wrong with it.
pub(super) fn written_part<'s>(
    world: &World<'s>,
    file: &ast::File<'s>,
    roots: &Map<ast::ExprId, NodeId>,
    written: ast::Quantity<'s>,
    fallback: Id<Commodity>,
) -> Result<(Part, Mode), Diagnostic> {
    let commodity = |unit: ast::Name<'s>| world.commodity_of(Word::of(file, unit.0));
    let amount = |amount: ast::Amount<'s>| written_amount(world, file, roots, amount, fallback);
    let quantity = match written {
        ast::Quantity::Amount(written) => Quantity::Amount(amount(written)?),
        ast::Quantity::Pending(written) => Quantity::Pending(amount(written)?),
        ast::Quantity::Target(written) => Quantity::Target(amount(written)?),
        ast::Quantity::Unknown(unit) => Quantity::Unknown(commodity(unit)?),
        ast::Quantity::All(None) => Quantity::All(None),
        ast::Quantity::All(Some(unit)) => Quantity::All(Some(commodity(unit)?)),
        ast::Quantity::Rest => return Ok((Part::Rest, Mode::Actual)),
        // An opening line's one unit of an asset: nothing keeps it as a quantity, only as an amount.
        ast::Quantity::Whole => {
            return Ok((Part::Of(Quantity::Amount(Expr::Literal(Amount::new(Qty(1), fallback)))), Mode::Opening));
        }
    };
    Ok((Part::Of(quantity), Mode::Actual))
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
        && out.root().is_none()
        && arrive.root().is_none()
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
    world: &mut World<'_>,
    out: Option<ResolvedQuantity>,
    arrive: Option<ResolvedQuantity>,
    (rate, quote, at): (Ratio, Id<Commodity>, Loc),
    loc: Loc,
) -> Option<(Amount, Amount)> {
    let quoted = match (out, arrive) {
        (Some(out), None) if out.root().is_none() => Some(priced(world, out.amount, quote, rate, at)?),
        (None, Some(arrive)) if arrive.root().is_none() => Some(priced(world, arrive.amount, quote, rate, at)?),
        (Some(out), Some(arrive)) if out.root().is_none() && arrive.root().is_none() => {
            let expected = if out.amount.unit == quote {
                priced(world, arrive.amount, quote, rate, at)?
            } else if arrive.amount.unit == quote {
                priced(world, out.amount, quote, rate, at)?
            } else {
                world.diags.push(
                    Diagnostic::error("price-unit", "the stated price unit must match one side of the flow")
                        .label(at, "the quote unit appears on neither side"),
                );
                return None;
            };
            let actual = if out.amount.unit == quote { out.amount } else { arrive.amount };
            if expected != actual {
                world.diags.push(
                    Diagnostic::error("price-disagrees", "the stated price does not match the flow amounts")
                        .label(at, "this price implies a different amount")
                        .label(loc, "the written quantities disagree with the price"),
                );
                return None;
            }
            None
        }
        _ => {
            world.diags.push(
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

/// An empty run of codes at the end of the pool: a flow with none of its own says where they would have gone.
pub(super) fn empty_codes(world: &World<'_>) -> Run<Sym> {
    Run::new(Id::new(world.book.codes.len() as u32), 0)
}

pub(super) fn endpoint(end: ResolvedEnd) -> Endpoint {
    Endpoint { place: end.place, entity: end.entity }
}

pub(super) fn priced(
    world: &mut World<'_>,
    amount: Amount,
    quote: Id<crate::book::Commodity>,
    rate: axiom_core::Ratio,
    loc: Loc,
) -> Option<Amount> {
    if amount.unit == quote {
        world.diags.push(
            Diagnostic::error("price-transfer", "a price cannot change a same-commodity transfer")
                .label(loc, "remove the price"),
        );
        return None;
    }
    let (from, to) = (world.book.commodities[amount.unit].scale, world.book.commodities[quote].scale);
    match crate::prices::rescale(amount.qty, from, to, rate) {
        Some(qty) if !qty.is_zero() => Some(Amount::new(qty, quote)),
        Some(_) => {
            world.diags.push(
                Diagnostic::error("price-vanishes", "the priced amount rounds to nothing")
                    .label(loc, "increase precision or state an amount"),
            );
            None
        }
        None => {
            world.diags.push(
                Diagnostic::error("price-overflow", "the priced amount is outside the supported range")
                    .label(loc, "this conversion overflows"),
            );
            None
        }
    }
}
