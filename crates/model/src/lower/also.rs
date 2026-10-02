//! `also` lines, which declaration and contract lowering share: what every matching flow implies, declared once.

use axiom_core::{Days, Diagnostic, Id, Loc, Run, Sym};
use axiom_syntax as ast;
use axiom_syntax::ClauseKind;

use super::tail::{Reach, written_purpose, written_waive};
use crate::book::{Also, AlsoOn, Amount, Commodity, Implied, Input, Place, Sign, TemplateAmount, Text};
use crate::declare::World;
use crate::errors::{Reported, Word};
use crate::journal::{Detail, Purposed, Select, Waive};
use crate::law::{Law, NodeId, Owner, Rank, Trigger, Ty};
use crate::scope::Home;

/// Pooled metadata shared by contract and declaration `also` clauses.
#[derive(Clone, Copy)]
pub(crate) struct AlsoMetadata {
    pub codes: Run<Sym>,
    pub select: Run<Select>,
    pub detail: Option<Id<Detail>>,
    pub waive: Option<Waive>,
    pub purpose: Option<Purposed>,
    pub description: Option<Text>,
}

/// Resolves exactly the metadata retained by `book::Also`. Endpoints belong to
/// the caller because an implied flow may inherit either endpoint from the
/// flow that caused it.
pub(crate) fn tail<'s>(
    world: &mut World<'s>,
    home: Home,
    file: &ast::File<'s>,
    clauses: ast::Many<ast::Clause<'s>>,
    diags: &mut Vec<Diagnostic>,
) -> AlsoMetadata {
    let code_start = world.book.codes.len();
    let select = Run::new(Id::new(world.book.selectors.len() as u32), 0);
    let mut detail = Detail::NONE;
    let (mut purpose, mut description, mut waive) = (None, None, None);

    for clause in &file[clauses] {
        match clause.kind {
            ClauseKind::Code(code) => {
                world.book.codes.push(world.book.names.intern(code.name()));
            }
            ClauseKind::Purpose(written) => {
                // An object that names nothing has been said, and costs the purpose.
                purpose = written_purpose(world, home, file, written, Reach::Anywhere, diags)
                    .filter(|purposed| written.of.is_none() || purposed.of.is_some());
            }
            ClauseKind::Description(text) => {
                description = Some(world.book.quoted_text(text.0));
            }
            ClauseKind::Waive(written) => waive = Some(written_waive(world, written)),
            ClauseKind::For(ast::For::Whom(name)) => match world.entity(home, Word::of(file, name.0)) {
                Ok(entity) => detail.hold = Some(entity),
                Err(problem) => diags.push(problem),
            },
            ClauseKind::Since(day) => detail.since = Some(day),
            ClauseKind::Due(ast::Due::On(day)) => detail.due = Some(day),
            ClauseKind::Due(ast::Due::After(_)) => diags.push(
                Diagnostic::error("also-relative-due", "a derived flow's due date must be absolute")
                    .label(clause.at, "write `due YYYY-MM-DD` on an implied line"),
            ),
            ClauseKind::Basis(ast::Amount::Literal(literal)) => {
                let Some(unit) =
                    literal.unit().and_then(|unit| world.commodity_of(Word::of(file, unit.0)).or_report(diags))
                else {
                    diags.push(
                        Diagnostic::error("basis-unit", "basis needs an explicit base-currency unit")
                            .label(file.loc(literal.0), "write the unit"),
                    );
                    continue;
                };
                match world.amount(literal.num(), unit, file.loc(literal.0)) {
                    Ok(amount) if amount.unit == world.book.base => detail.basis = Some(amount.qty),
                    Ok(_) => diags.push(
                        Diagnostic::error("basis-unit", "basis must be stated in the base currency")
                            .label(file.loc(literal.0), "another unit is not the base currency"),
                    ),
                    Err(problem) => diags.push(problem),
                }
            }
            ClauseKind::Basis(ast::Amount::Computed(_)) => diags.push(
                Diagnostic::error("computed-also-basis", "an implied basis must be literal")
                    .label(clause.at, "this metadata field has no computed root in the Book"),
            ),
            ClauseKind::For(ast::For::Period(..) | ast::For::Last(_))
            | ClauseKind::Via(_)
            | ClauseKind::Price(_)
            | ClauseKind::Against(_)
            | ClauseKind::Until(_) => diags.push(
                Diagnostic::error("also-tail", "this clause is not retained on an implied line")
                    .label(clause.at, "remove it or write the metadata on the source flow"),
            ),
        }
    }

    let detail = (detail != Detail::NONE).then(|| world.book.details.push(detail));
    let codes = Run::new(Id::new(code_start as u32), (world.book.codes.len() - code_start) as u32);
    AlsoMetadata { codes, select, detail, waive, purpose, description }
}

/// What the `also` lines of one declaration are lowered against. `inputs` are the caller's template inputs, and
/// `currency` is the unit of an amount written without one. The caller chooses the owner and the `AlsoOn` it
/// matches, and owns any fallback endpoint semantics.
pub(crate) struct AlsoCx<'a, 's> {
    pub file: &'a ast::File<'s>,
    pub home: Home,
    pub owner: Owner,
    pub on: AlsoOn,
    pub inputs: &'a [Input],
    pub currency: Id<Commodity>,
}

/// An implied amount: a literal is resolved now, and an expression is compiled with the rest of its `also`.
#[derive(Clone, Copy)]
enum PendingAmount {
    Literal(Amount),
    Computed(usize),
}

/// What an `also` implies, read: the item or flow (its amount still to come), the clauses after it, and the
/// selectors an implied flow narrows its source by.
struct Line<'s> {
    what: Implied,
    amount: PendingAmount,
    clauses: ast::Many<ast::Clause<'s>>,
    selectors: Option<ast::Many<ast::Select<'s>>>,
}

/// Lowers `also` clauses shared by declaration and contract lowering.
pub(crate) fn lower_alsos<'s>(
    world: &mut World<'s>,
    cx: &AlsoCx<'_, 's>,
    alsos: ast::Many<ast::Also<'s>>,
    diags: &mut Vec<Diagnostic>,
) -> Box<[Id<Also>]> {
    cx.file[alsos].iter().filter_map(|also| lower_also(world, cx, also, diags)).collect()
}

/// One `also`, or nothing after what is wrong with it has been said.
fn lower_also<'s>(
    world: &mut World<'s>,
    cx: &AlsoCx<'_, 's>,
    also: &ast::Also<'s>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Id<Also>> {
    let (file, home) = (cx.file, cx.home);
    let mut roots = Vec::new();
    let when_index = also.when.map(|when| {
        roots.push((when, Ty::Bool));
        roots.len() - 1
    });
    let Line { mut what, amount, clauses, selectors } = match &also.line {
        ast::AlsoLine::Item(item) => implied_item(world, cx, item, &mut roots, diags)?,
        ast::AlsoLine::Flow(flow) => implied_flow(world, cx, flow, also.loc, &mut roots, diags)?,
    };
    let metadata_errors = diags.len();
    let metadata = tail(world, home, file, clauses, diags);
    if diags.len() != metadata_errors {
        return None;
    }
    let selector_errors = diags.len();
    let select = selectors.map_or(metadata.select, |selectors| lower_selectors(world, home, file, selectors, diags));
    if diags.len() != selector_errors {
        return None;
    }
    let (law, compiled_roots) = compile_also(world, cx, &roots, also.loc, diags)?;
    let amount = match amount {
        PendingAmount::Literal(amount) => TemplateAmount::Literal(amount),
        PendingAmount::Computed(index) => TemplateAmount::Computed(compiled_roots[index]),
    };
    match &mut what {
        Implied::Item { amount: slot, .. } | Implied::Flow { amount: slot, .. } => *slot = amount,
    }
    Some(world.book.also.push(Also {
        on: cx.on,
        what,
        when: when_index.map(|index| compiled_roots[index]),
        law,
        purpose: metadata.purpose,
        description: metadata.description,
        codes: metadata.codes,
        select,
        detail: metadata.detail,
        waive: metadata.waive,
        loc: also.loc,
    }))
}

/// `+ 5%`, `- 2.9% + 0.30 USD`: an item of the flow that implies it.
fn implied_item<'s>(
    world: &World<'s>,
    cx: &AlsoCx<'_, 's>,
    item: &ast::LineItem<'s>,
    roots: &mut Vec<(ast::ExprId, Ty)>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Line<'s>> {
    let amount = pending_amount(world, cx.file, item.amount, cx.currency, roots, diags)?;
    let sign = match item.sign {
        ast::Sign::Carve => Sign::Carve,
        ast::Sign::Add => Sign::Add,
        ast::Sign::Less => Sign::Less,
    };
    let what = Implied::Item { sign, amount: TemplateAmount::Literal(Amount::zero(cx.currency)) };
    Some(Line { what, amount, clauses: item.tail, selectors: None })
}

/// `-> escrow 410 USD`: a flow of its own, whose ends are the implying flow's own where it names none (`self`).
fn implied_flow<'s>(
    world: &World<'s>,
    cx: &AlsoCx<'_, 's>,
    flow: &ast::Flow<'s>,
    also_loc: Loc,
    roots: &mut Vec<(ast::ExprId, Ty)>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Line<'s>> {
    let file = cx.file;
    if !file[flow.body.legs].is_empty() || !file[flow.body.items].is_empty() {
        diags.push(
            Diagnostic::error("also-flow-body", "a declaration `also` flow cannot have split legs or items")
                .label(also_loc, "write one implied flow here"),
        );
        return None;
    }
    if flow.to.end.is_some_and(|end| !file[end.select].is_empty()) {
        diags.push(
            Diagnostic::error(
                "selector-target",
                "selectors narrow the source endpoint; an implied flow target receives",
            )
            .label(also_loc, "remove selectors from the target endpoint"),
        );
        return None;
    }
    // Both ends are looked up before either failure stops the line, so both are said.
    let (from, to) = (
        implied_end(world, cx.home, file, flow.from.end, diags),
        implied_end(world, cx.home, file, flow.to.end, diags),
    );
    let (from, to) = (from?, to?);
    let from_amount = implied_amount(file, flow.from.amount, also_loc, diags)?;
    let to_amount = implied_amount(file, flow.to.amount, also_loc, diags)?;
    let amount = match (from_amount, to_amount) {
        (Some(_), Some(_)) => {
            diags.push(
                Diagnostic::error("also-flow-amount", "an implied flow states its amount on one side only")
                    .label(also_loc, "remove one of these amounts"),
            );
            return None;
        }
        (Some(amount), None) | (None, Some(amount)) => amount,
        (None, None) => {
            diags.push(
                Diagnostic::error("also-flow-amount", "an implied flow needs an amount")
                    .label(also_loc, "write an amount on one side of the arrow"),
            );
            return None;
        }
    };
    let amount = pending_amount(world, file, amount, cx.currency, roots, diags)?;
    let what = Implied::Flow { from, to, amount: TemplateAmount::Literal(Amount::zero(cx.currency)) };
    Some(Line { what, amount, clauses: flow.tail, selectors: flow.from.end.map(|end| end.select) })
}

/// The place an end of an implied flow names: none for `self` or no end, and nothing at all, after it is said,
/// for a name that is none.
fn implied_end<'s>(
    world: &World<'s>,
    home: Home,
    file: &ast::File<'s>,
    end: Option<ast::End<'s>>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Option<Id<Place>>> {
    match end {
        Some(end) if end.name.0 != "self" => {
            world.end(home, Word::of(file, end.name.0)).map(|end| Some(end.place)).or_report(diags)
        }
        _ => Some(None),
    }
}

/// The amount one side of an implied flow states: none if it states none, and nothing at all, after it is said,
/// if it states something that is not an amount expression.
fn implied_amount<'s>(
    file: &ast::File<'s>,
    quantity: Option<ast::Quantity<'s>>,
    also_loc: Loc,
    diags: &mut Vec<Diagnostic>,
) -> Option<Option<ast::Amount<'s>>> {
    match quantity {
        Some(ast::Quantity::Amount(amount)) => Some(Some(amount)),
        Some(other) => {
            diags.push(
                Diagnostic::error("also-flow-amount", "an implied flow amount must be an amount expression")
                    .label(quantity_loc(file, other, also_loc), "this quantity cannot be implied"),
            );
            None
        }
        None => Some(None),
    }
}

/// An implied amount: a literal is resolved now, and an expression is compiled with the rest of its `also`.
fn pending_amount<'s>(
    world: &World<'s>,
    file: &ast::File<'s>,
    amount: ast::Amount<'s>,
    currency: Id<Commodity>,
    roots: &mut Vec<(ast::ExprId, Ty)>,
    diags: &mut Vec<Diagnostic>,
) -> Option<PendingAmount> {
    match amount {
        ast::Amount::Literal(literal) => {
            world.literal_amount(file, literal, Some(currency)).or_report(diags).map(PendingAmount::Literal)
        }
        ast::Amount::Computed(root) => {
            roots.push((root, Ty::AMOUNT));
            Some(PendingAmount::Computed(roots.len() - 1))
        }
    }
}

/// Compiles the expression roots of an `also` into a law arena of its own. The caller stores the law in its
/// `Also`; roots are ordered as written (`when`, then amounts).
fn compile_also<'s>(
    world: &mut World<'s>,
    cx: &AlsoCx<'_, 's>,
    roots: &[(ast::ExprId, Ty)],
    loc: Loc,
    diags: &mut Vec<Diagnostic>,
) -> Option<(Id<Law>, Box<[NodeId]>)> {
    let name = world.book.names.intern("also");
    let compiled = crate::laws::compile_template(world, diags, cx.file, cx.home, Ty::Flow, name, cx.inputs, roots)?;
    let (program, roots) = compiled;
    let law = Law {
        name,
        doc: None,
        owner: cx.owner,
        system: if let Home::System(system) = cx.home { Some(system) } else { None },
        trigger: Trigger::Flow,
        budget: None,
        overrides: None,
        override_name: None,
        rank: Rank::ZERO,
        steps: Box::default(),
        nodes: program.nodes,
        loc,
    };
    Some((world.book.laws.push(law), roots))
}

fn lower_selectors<'s>(
    world: &mut World<'s>,
    home: Home,
    file: &ast::File<'s>,
    selectors: ast::Many<ast::Select<'s>>,
    diags: &mut Vec<Diagnostic>,
) -> Run<Select> {
    let start = world.book.selectors.len();
    for written in &file[selectors] {
        let resolved = match *written {
            ast::Select::Range(first, last, at) => Days::new(first, last).map(Select::Range).ok_or_else(|| {
                Diagnostic::error("selector-range", "selector range ends before it begins")
                    .label(at, "reverse or correct this date range")
            }),
            ast::Select::Code(code) => Ok(Select::Code(world.book.names.intern(code.name()))),
            ast::Select::Policy(policy, _) => Ok(Select::Policy(policy)),
            ast::Select::Purpose(name) => world.purpose(home, Word::of(file, name.0)).map(Select::Purpose),
            ast::Select::Unit(name) => world.commodity_of(Word::of(file, name.0)).map(Select::Unit),
            ast::Select::End(name) => world.end(home, Word::of(file, name.0)).map(|end| Select::End(end.place)),
        };
        if let Some(selector) = resolved.or_report(diags) {
            world.book.selectors.push(selector);
        }
    }
    Run::new(Id::new(start as u32), (world.book.selectors.len() - start) as u32)
}

fn quantity_loc(file: &ast::File<'_>, quantity: ast::Quantity<'_>, fallback: Loc) -> Loc {
    match quantity {
        ast::Quantity::Amount(ast::Amount::Literal(literal)) => file.loc(literal.0),
        ast::Quantity::Amount(ast::Amount::Computed(root))
        | ast::Quantity::Pending(ast::Amount::Computed(root))
        | ast::Quantity::Target(ast::Amount::Computed(root)) => file.exprs[root].loc,
        ast::Quantity::Pending(ast::Amount::Literal(literal))
        | ast::Quantity::Target(ast::Amount::Literal(literal)) => file.loc(literal.0),
        _ => fallback,
    }
}
