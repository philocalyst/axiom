//! What a line that derives says: the `FLOW` or `ITEM` after an `also` or a `derive`, read.
//!
//! The two spellings are one line (`+ 5% of amount #fee`, `lumen -> retirement 50% of ... #match`), so one reader
//! turns it into a [`Said`] (its shape against the flow that fires it, its amount as written, its clauses) and one
//! reader turns the clauses into the metadata a derived flow carries.
//!
//! A line a contract's kind writes (`also employer -> irs 7.65% of amount #payroll-tax`) has the kind's roles for
//! ends. A role is not an entity: it is whoever fills the slot in the contract being lowered, standing where
//! [`Positions`] says, so the same line reads one way for the household that pays `employer` and another for the
//! `employer` whose book it is.

use axiom_core::diag::closest;
use axiom_core::{Days, Diagnostic, Id, Loc, Run, Sym};
use axiom_syntax as ast;
use axiom_syntax::ClauseKind;

use crate::book::{Place, Shape, Stand, Text};
use crate::declare::World;
use crate::errors::{Reported, Word};
use crate::journal::{Detail, Purposed, Select, Waive};
use crate::lower::tail::{Reach, written_purpose, written_waive};
use crate::scope::Home;

/// Where each role of a contract's kind stands in the book the contract is lowered into: the place its filler is at,
/// which is the owner's holding where the filler is the owner and the filler's outside place where it is anyone else.
#[derive(Clone, Copy)]
pub(crate) struct Positions<'a> {
    pub stands: &'a [(Sym, Id<Place>)],
    /// The roles the contract leaves empty (an `optional` slot nothing fills).
    pub empty: &'a [Sym],
}

/// What a name written as an end is, when a contract's kind has roles.
enum Standing {
    Stands(Id<Place>),
    Empty,
    NoRole,
}

impl Positions<'_> {
    /// The positions of a contract with no kind: it has no roles.
    pub(crate) const NONE: Positions<'static> = Positions { stands: &[], empty: &[] };

    fn of(self, name: Option<Sym>) -> Standing {
        let Some(name) = name else { return Standing::NoRole };
        match self.stands.iter().find(|(slot, _)| *slot == name) {
            Some(&(_, place)) => Standing::Stands(place),
            None if self.empty.contains(&name) => Standing::Empty,
            None => Standing::NoRole,
        }
    }
}

/// What the clauses of a derived line say, pooled in the book.
#[derive(Clone, Copy)]
pub(crate) struct Metadata {
    pub codes: Run<Sym>,
    pub select: Run<Select>,
    pub detail: Option<Id<Detail>>,
    pub waive: Option<Waive>,
    pub purpose: Option<Purposed>,
    pub description: Option<Text>,
}

/// Resolves exactly the metadata a [`crate::book::Derived`] keeps. Its ends belong to the line, because a derived flow
/// may take either end from the flow that fired it.
pub(crate) fn tail<'s>(
    world: &mut World<'s>,
    home: Home,
    file: &ast::File<'s>,
    clauses: ast::Many<ast::Clause<'s>>,
    diags: &mut Vec<Diagnostic>,
) -> Metadata {
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
    Metadata { codes, select, detail, waive, purpose, description }
}

/// What a line that derives, an `also` or a `derive`, says, read: how it lies against the flow that fires it, its
/// amount as written, the clauses after it, and the selectors it narrows its source by.
pub(crate) struct Said<'s> {
    pub shape: Shape,
    pub amount: ast::Amount<'s>,
    pub clauses: ast::Many<ast::Clause<'s>>,
    pub selectors: Option<ast::Many<ast::Select<'s>>>,
}

/// What `line` says, or nothing after what is wrong with it has been said.
pub(crate) fn read_line<'s>(
    world: &World<'s>,
    home: Home,
    file: &ast::File<'s>,
    line: &ast::AlsoLine<'s>,
    at: Loc,
    positions: Positions<'_>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Said<'s>> {
    match line {
        ast::AlsoLine::Item(item) => Some(implied_item(item)),
        ast::AlsoLine::Flow(flow) => implied_flow(world, home, file, flow, at, positions, diags),
    }
}

/// The places a line's two ends stand at (`None` is the end of the flow that fires it), if it is a flow and both ends
/// are something. Nothing is said of an end that is not: reading the line for real says it once.
pub(crate) fn flow_ends<'s>(
    world: &World<'s>,
    home: Home,
    file: &ast::File<'s>,
    line: &ast::AlsoLine<'s>,
    positions: Positions<'_>,
) -> Option<(Option<Id<Place>>, Option<Id<Place>>)> {
    let ast::AlsoLine::Flow(flow) = line else { return None };
    let mut unsaid = Vec::new();
    let place = |stand| match stand {
        Stand::At(place) => Some(place),
        Stand::Flow | Stand::Subject => None,
    };
    let from = implied_end(world, home, file, flow.from.end, positions, &mut unsaid)?;
    let to = implied_end(world, home, file, flow.to.end, positions, &mut unsaid)?;
    Some((place(from), place(to)))
}

/// `+ 5%`, `- 2.9% + 0.30 USD`: an item of the flow that implies it.
fn implied_item<'s>(item: &ast::LineItem<'s>) -> Said<'s> {
    Said { shape: Shape::Item(item.sign), amount: item.amount, clauses: item.tail, selectors: None }
}

/// `-> escrow 410 USD`: a flow of its own, whose ends are the implying flow's own where it names none (`self`).
fn implied_flow<'s>(
    world: &World<'s>,
    home: Home,
    file: &ast::File<'s>,
    flow: &ast::Flow<'s>,
    also_loc: Loc,
    positions: Positions<'_>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Said<'s>> {
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
        implied_end(world, home, file, flow.from.end, positions, diags),
        implied_end(world, home, file, flow.to.end, positions, diags),
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
    let selectors = flow.from.end.map(|end| end.select);
    Some(Said { shape: Shape::Flow { from, to }, amount, clauses: flow.tail, selectors })
}

/// Where an end of an implied flow stands: with the flow that fires the law where none is written, at what the law
/// governs for `self`, at the place a role stands at or the one a name says, and nowhere at all, after it is said,
/// for a name that is none.
fn implied_end<'s>(
    world: &World<'s>,
    home: Home,
    file: &ast::File<'s>,
    end: Option<ast::End<'s>>,
    positions: Positions<'_>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Stand> {
    let Some(end) = end else { return Some(Stand::Flow) };
    if end.name.0 == "self" {
        return Some(Stand::Subject);
    }
    let word = Word::of(file, end.name.0);
    match positions.of(world.book.names.get(word.text)) {
        Standing::Stands(place) => Some(Stand::At(place)),
        Standing::Empty => {
            diags.push(empty_role(world, word, positions));
            None
        }
        Standing::NoRole => world.end(home, word).map(|end| Stand::At(end.place)).or_report(diags),
    }
}

/// A leg of a contract's kind names a role the contract leaves empty.
fn empty_role(world: &World<'_>, word: Word, positions: Positions<'_>) -> Diagnostic {
    let filled: Vec<&str> = positions.stands.iter().map(|&(slot, _)| world.book.name(slot)).collect();
    let near = closest(word.text, filled.iter().copied());
    let mut diagnostic = Diagnostic::error("relator-role-empty", format!("nothing fills `{}` here", word.text))
        .label(word.loc, "this contract leaves the role empty, so the leg has no end")
        .help(format!("fill it in the contract: `{} NAME`", word.text));
    if let Some(near) = near {
        diagnostic = diagnostic.note(format!("the roles it fills are {}", crate::errors::list(&filled))).fix(
            format!("did you mean `{near}`?"),
            word.loc,
            near,
        );
    }
    diagnostic
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

pub(crate) fn lower_selectors<'s>(
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
