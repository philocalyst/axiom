//! The tail of a line: the clauses after its amount, read into what the line says about itself.
//!
//! A flow, a contract's term line and an `also` line each carry a tail. They read four clauses the same way, so
//! once: a code, a purpose, a description and a waiver. A flow's tail takes every other clause as well; what a
//! contract's term line or an `also` line does with the rest is theirs, and is said where they read it.

use axiom_core::calendar::{self, Period, Window};
use axiom_core::{Day, Days, Diagnostic, Id, Loc, Ratio, Run, Sym};
use axiom_syntax as ast;
use axiom_syntax::ClauseKind;

use super::flow::FlowCx;
use crate::book::{Commodity, Entity, Text};
use crate::declare::World;
use crate::errors::{Reported, Word};
use crate::journal::{Detail, Object, Provenance, Purposed, Waive};
use crate::law::NodeId;
use crate::problem::CodeUse;
use crate::scope::Home;

/// What a flow's tail says about its flow, or its item, or its leg.
#[derive(Clone, Default)]
pub(super) struct Tail {
    pub purpose: Option<Purposed>,
    pub description: Option<Text>,
    pub payee: Option<Id<Entity>>,
    pub recognized: Option<Days>,
    pub waive: Option<Waive>,
    pub detail: Detail,
    pub basis_root: Option<NodeId>,
    pub price: Option<(Ratio, Id<Commodity>, Loc)>,
    pub valid: bool,
}

impl Tail {
    pub fn new() -> Tail {
        Tail { valid: true, ..Tail::default() }
    }

    /// This tail with a child's on top: what the child says wins, and one that is wrong makes both so.
    pub fn merge(mut self, child: Tail) -> Tail {
        if child.purpose.is_some() {
            self.purpose = child.purpose;
        }
        if child.description.is_some() {
            self.description = child.description;
        }
        if child.payee.is_some() {
            self.payee = child.payee;
        }
        if child.recognized.is_some() {
            self.recognized = child.recognized;
        }
        if child.waive.is_some() {
            self.waive = child.waive;
        }
        if child.detail.basis.is_some() {
            self.detail.basis = child.detail.basis;
        }
        if child.detail.basis.is_some() || child.basis_root.is_some() {
            self.basis_root = child.basis_root;
        }
        if child.detail.hold.is_some() {
            self.detail.hold = child.detail.hold;
        }
        if child.detail.since.is_some() {
            self.detail.since = child.detail.since;
        }
        if child.detail.due.is_some() {
            self.detail.due = child.detail.due;
        }
        if child.price.is_some() {
            self.price = child.price;
        }
        self.valid &= child.valid;
        self
    }
}

/// What the object of a purpose (`#improvement of condo`) may be.
#[derive(Clone, Copy)]
pub(crate) enum Reach {
    /// An asset or a party, as a contract's lines have it.
    Parties,
    /// An asset, a party or a place, as a flow has it.
    Anywhere,
}

/// What `name` is, as the object of a purpose, or why it is none.
pub(super) fn resolve_object<'s>(
    world: &World<'s>,
    home: Home,
    file: &ast::File<'s>,
    name: ast::Name<'s>,
    reach: Reach,
    diags: &mut Vec<Diagnostic>,
) -> Option<Object> {
    if let Some(sym) = world.book.names.get(name.0)
        && let Some(&asset) = world.book.lookup.assets.get(&sym)
    {
        return Some(Object::Asset(asset));
    }
    let word = Word::of(file, name.0);
    match reach {
        Reach::Parties => world.entity(home, word).map(Object::Entity).or_report(diags),
        Reach::Anywhere => {
            if let Ok(end) = world.end(home, word) {
                return Some(end.entity.map_or(Object::Place(end.place), Object::Entity));
            }
            world.place(word).map(Object::Place).or_report(diags)
        }
    }
}

/// A written `#purpose [of OBJECT]`: the purpose with its object, which is none when it was written and names
/// nothing (said, and the purpose is kept), or no purpose at all when the purpose itself names nothing.
pub(crate) fn written_purpose<'s>(
    world: &World<'s>,
    home: Home,
    file: &ast::File<'s>,
    written: ast::Purpose<'s>,
    reach: Reach,
    diags: &mut Vec<Diagnostic>,
) -> Option<Purposed> {
    let purpose = world.purpose(home, Word::of(file, written.name.0));
    let of = written.of.and_then(|name| resolve_object(world, home, file, name, reach, diags));
    let purpose = purpose.or_report(diags)?;
    Some(Purposed { purpose, of, source: Provenance::Written })
}

/// A written `!` and the reason after it.
pub(crate) fn written_waive<'s>(world: &mut World<'s>, waive: ast::Waive<'s>) -> Waive {
    Waive { loc: waive.at, reason: waive.reason.map(|text| world.book.quoted_text(text.0)) }
}

impl<'s> FlowCx<'_, 's> {
    /// Reads every clause of a flow's tail: the codes it adds to the pool, and what the rest of it says.
    pub fn lower_tail(
        &self,
        world: &mut World<'s>,
        clauses: ast::Many<ast::Clause<'s>>,
        diags: &mut Vec<Diagnostic>,
    ) -> (Run<Sym>, Tail) {
        let start = world.book.codes.len();
        let mut tail = Tail::new();
        for clause in &self.file[clauses] {
            self.read(world, clause, &mut tail, diags);
        }
        (Run::new(Id::new(start as u32), (world.book.codes.len() - start) as u32), tail)
    }

    fn read(&self, world: &mut World<'s>, clause: &ast::Clause<'s>, tail: &mut Tail, diags: &mut Vec<Diagnostic>) {
        let (home, file) = (self.home, self.file);
        match clause.kind {
            ClauseKind::Purpose(written) => match written_purpose(world, home, file, written, Reach::Anywhere, diags) {
                Some(purposed) if written.of.is_some() && purposed.of.is_none() => tail.valid = false,
                Some(purposed) => tail.purpose = Some(purposed),
                None => tail.valid = false,
            },
            ClauseKind::Description(text) => tail.description = Some(world.book.quoted_text(text.0)),
            ClauseKind::Code(code) => {
                let symbol = world.book.names.intern(code.name());
                world.book.codes.push(symbol);
            }
            ClauseKind::For(ast::For::Period(first, last)) => {
                tail.recognized = Days::new(first, last);
                tail.valid &= tail.recognized.is_some();
            }
            ClauseKind::For(ast::For::Last(relative)) => tail.recognized = Some(previous_period(self.day, relative)),
            ClauseKind::For(ast::For::Whom(name)) => {
                match world.entity(home, Word::of(file, name.0)).or_report(diags) {
                    Some(entity) => tail.detail.hold = Some(entity),
                    None => tail.valid = false,
                }
            }
            ClauseKind::Due(due) => {
                tail.detail.due = Some(match due {
                    ast::Due::On(day) => day,
                    ast::Due::After(span) => self.day + span,
                });
            }
            ClauseKind::Via(name) => match world.entity(home, Word::of(file, name.0)).or_report(diags) {
                Some(entity) => tail.payee = Some(entity),
                None => tail.valid = false,
            },
            ClauseKind::Basis(ast::Amount::Literal(literal)) => read_basis(world, file, literal, tail, diags),
            ClauseKind::Basis(ast::Amount::Computed(expr)) => match self.roots.get(&expr) {
                Some(&root) => tail.basis_root = Some(root),
                None => {
                    diags.push(
                        Diagnostic::error("computed-basis", "computed basis expression was not compiled for this flow")
                            .label(clause.at, "the basis expression is not available"),
                    );
                    tail.valid = false;
                }
            },
            ClauseKind::Price(literal) => read_price(world, file, literal, clause.at, tail, diags),
            ClauseKind::Since(day) => tail.detail.since = Some(day),
            ClauseKind::Against(code) => {
                tail.detail.against = self.code_index.resolve(world, code, clause.at, CodeUse::Against, diags);
                tail.valid &= tail.detail.against.is_some();
            }
            ClauseKind::Until(_) => {
                diags.push(
                    Diagnostic::error("until-position", "`until` is only valid on a statement change or waiver")
                        .label(clause.at, "it has no effect on a flow"),
                );
                tail.valid = false;
            }
            ClauseKind::Waive(waive) => tail.waive = Some(written_waive(world, waive)),
        }
    }
}

/// `basis 400 USD`: what was paid for it, in the base currency.
fn read_basis<'s>(
    world: &World<'s>,
    file: &ast::File<'s>,
    literal: ast::Literal<'s>,
    tail: &mut Tail,
    diags: &mut Vec<Diagnostic>,
) {
    let at = file.loc(literal.0);
    // A unit that names no commodity is said as a missing unit, not as an unknown commodity.
    let Some(unit) = literal.unit().and_then(|unit| world.commodity_of(Word::of(file, unit.0)).ok()) else {
        diags.push(
            Diagnostic::error("basis-unit", "basis needs an explicit base-currency unit").label(at, "write the unit"),
        );
        tail.valid = false;
        return;
    };
    match world.amount(literal.num(), unit, at) {
        Ok(amount) if amount.unit == world.book.base => tail.detail.basis = Some(amount.qty),
        Ok(_) => {
            diags.push(
                Diagnostic::error("basis-unit", "basis must be stated in the base currency")
                    .label(at, "another unit is not the base currency"),
            );
            tail.valid = false;
        }
        Err(problem) => {
            diags.push(problem);
            tail.valid = false;
        }
    }
}

/// `@ 285.70 USD`: what one of the commodity is worth in another.
fn read_price<'s>(
    world: &World<'s>,
    file: &ast::File<'s>,
    literal: ast::Literal<'s>,
    clause_at: Loc,
    tail: &mut Tail,
    diags: &mut Vec<Diagnostic>,
) {
    let at = file.loc(literal.0);
    let Some(name) = literal.unit() else {
        diags.push(
            Diagnostic::error("price-unit", "a price needs a quoted commodity").label(at, "write `@ 285.70 USD`"),
        );
        tail.valid = false;
        return;
    };
    // An unknown commodity is the one thing a price does not say, as it always has not.
    let Ok(unit) = world.commodity_of(Word::of(file, name.0)) else {
        tail.valid = false;
        return;
    };
    let Some(rate) = literal.num().to_ratio().filter(|rate| !rate.is_zero()) else {
        diags
            .push(Diagnostic::error("price-zero", "a price must be greater than zero").label(at, "this price is zero"));
        tail.valid = false;
        return;
    };
    tail.price = Some((rate, unit, clause_at));
}

/// The month, quarter or year before the one `day` is in.
fn previous_period(day: Day, relative: ast::Relative) -> Days {
    match relative {
        ast::Relative::Month => Window::containing(Period::Month, day).previous().days(),
        ast::Relative::Quarter => calendar::quarter(day, -1),
        ast::Relative::Year => Window::containing(Period::Year, day).previous().days(),
    }
}
