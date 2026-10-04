//! The tail of a line: the clauses after its amount, read into what the line says about itself.
//!
//! A flow, a contract's term line, an `also` line, a measure and an ending each carry a tail, and one reader reads
//! them all: a clause means the same on every line that takes it, and the [`Line`] says which it takes and what it
//! says of the rest. A value's `via` (a place) and a contract waiver's code (its name) mean something else, and are
//! read where those statements are.

use axiom_core::calendar::{self, Period, Window};
use axiom_core::{Day, Days, Diagnostic, Id, Loc, Ratio, Run, Sym};
use axiom_syntax as ast;
use axiom_syntax::ClauseKind;

use super::flow::FlowCx;
use super::record::CodeIndex;
use crate::book::{Commodity, Entity, Text};
use crate::declare::World;
use crate::errors::{Reported, Word};
use crate::journal::{Detail, Object, Provenance, Purposed, Select, Waive};
use crate::law::NodeId;
use crate::problem::CodeUse;
use crate::scope::Home;

/// What a tail says about its line: a flow, an item or a leg of one, or a line that takes fewer of the clauses.
#[derive(Clone, Default)]
pub(crate) struct Tail {
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
    pub fn merge(self, child: Tail) -> Tail {
        let (mine, theirs) = (self.detail, child.detail);
        let said_basis = theirs.basis.is_some() || child.basis_root.is_some();
        Tail {
            purpose: child.purpose.or(self.purpose),
            description: child.description.or(self.description),
            payee: child.payee.or(self.payee),
            recognized: child.recognized.or(self.recognized),
            waive: child.waive.or(self.waive),
            detail: Detail {
                basis: theirs.basis.or(mine.basis),
                hold: theirs.hold.or(mine.hold),
                since: theirs.since.or(mine.since),
                due: theirs.due.or(mine.due),
                ..mine
            },
            basis_root: if said_basis { child.basis_root } else { self.basis_root },
            price: child.price.or(self.price),
            valid: self.valid && child.valid,
        }
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

/// An empty run of selectors at the end of the pool: a line that narrows nothing says where they would have gone.
pub(crate) fn no_selectors(world: &World<'_>) -> Run<Select> {
    Run::new(Id::new(world.book.selectors.len() as u32), 0)
}

/// The line a tail is on: which of the clauses written after it the line takes, and what it says of the others.
#[derive(Clone, Copy)]
pub(crate) enum Line<'c, 'a, 's> {
    /// A flow in the journal: every clause but `until`, a statement's (`until-position`). A relative `for` or `due`
    /// counts from its day.
    Flow(&'c FlowCx<'a, 's>),
    /// A contract's term line, which promises a flow: its codes, purpose, description and waiver. What an occurrence
    /// says of itself (`for`, `due`, `via`, `basis`, `since`, `against`, `@`) is left unread, as it always has been, and
    /// a purpose whose object names nothing (said) is kept without it.
    Term,
    /// An `also` or `derive` line: what a derived flow keeps (codes, purpose, description, waiver, `for` whom, `since`,
    /// a dated `due`, a literal `basis`); any other clause is said.
    Also,
    /// A measure, `worked 8h` or `used 120 kWh`: codes, purpose, description, `for` whom and `against`; any other clause
    /// is said.
    Measure(&'c CodeIndex),
    /// An ending: codes and a description, all the parser keeps on one.
    Ending,
}

/// Reads the clauses of a tail on `line`: the codes it adds to the pool, and what the rest of it says.
pub(crate) fn read_tail<'s>(
    world: &mut World<'s>,
    home: Home,
    file: &ast::File<'s>,
    line: Line<'_, '_, 's>,
    clauses: ast::Many<ast::Clause<'s>>,
    diags: &mut Vec<Diagnostic>,
) -> (Run<Sym>, Tail) {
    let start = world.book.codes.len();
    let mut tail = Tail::new();
    for clause in &file[clauses] {
        line.read(world, home, file, clause, &mut tail, diags);
    }
    (Run::of(start..world.book.codes.len()), tail)
}

impl<'s> Line<'_, '_, 's> {
    fn read(
        self,
        world: &mut World<'s>,
        home: Home,
        file: &ast::File<'s>,
        clause: &ast::Clause<'s>,
        tail: &mut Tail,
        diags: &mut Vec<Diagnostic>,
    ) {
        use Line::{Also, Flow, Measure, Term};
        match (self, clause.kind) {
            (_, ClauseKind::Code(code)) => {
                let symbol = world.book.names.intern(code.name());
                world.book.codes.push(symbol);
            }
            (_, ClauseKind::Description(text)) => tail.description = Some(world.book.quoted_text(text.0)),
            (Flow(_) | Term | Also | Measure(_), ClauseKind::Purpose(written)) => {
                // An object that names nothing is said; a term line keeps the purpose without it, any other is wrong.
                let reach = if matches!(self, Term) { Reach::Parties } else { Reach::Anywhere };
                let purpose = world.purpose(home, Word::of(file, written.name.0));
                let of = written.of.and_then(|name| resolve_object(world, home, file, name, reach, diags));
                match purpose.or_report(diags) {
                    Some(purpose) if of.is_some() || written.of.is_none() || matches!(self, Term) => {
                        tail.purpose = Some(Purposed { purpose, of, source: Provenance::Written })
                    }
                    _ => tail.valid = false,
                }
            }
            (Flow(_) | Term | Also, ClauseKind::Waive(waive)) => {
                let reason = waive.reason.map(|text| world.book.quoted_text(text.0));
                tail.waive = Some(Waive { loc: waive.at, reason });
            }
            (Flow(_) | Also | Measure(_), ClauseKind::For(ast::For::Whom(name))) => {
                match world.entity(home, Word::of(file, name.0)).or_report(diags) {
                    Some(entity) => tail.detail.hold = Some(entity),
                    None => tail.valid = false,
                }
            }
            (Flow(_) | Also, ClauseKind::Since(day)) => tail.detail.since = Some(day),
            (Flow(_) | Also, ClauseKind::Due(ast::Due::On(day))) => tail.detail.due = Some(day),
            (Flow(_) | Also, ClauseKind::Basis(ast::Amount::Literal(literal))) => {
                self.read_basis(world, file, literal, tail, diags)
            }
            (Flow(&FlowCx { code_index, .. }) | Measure(code_index), ClauseKind::Against(code)) => {
                tail.detail.against = code_index.resolve(world, code, clause.at, CodeUse::Against, diags);
                tail.valid &= tail.detail.against.is_some();
            }
            (Flow(_), ClauseKind::For(ast::For::Period(first, last))) => {
                tail.recognized = Days::new(first, last);
                tail.valid &= tail.recognized.is_some();
            }
            (Flow(cx), ClauseKind::For(ast::For::Last(relative))) => {
                tail.recognized = Some(previous_period(cx.day, relative))
            }
            (Flow(cx), ClauseKind::Due(ast::Due::After(span))) => tail.detail.due = Some(cx.day + span),
            (Flow(_), ClauseKind::Via(name)) => match world.entity(home, Word::of(file, name.0)).or_report(diags) {
                Some(entity) => tail.payee = Some(entity),
                None => tail.valid = false,
            },
            (Flow(cx), ClauseKind::Basis(ast::Amount::Computed(expr))) => match cx.roots.get(&expr) {
                Some(&root) => tail.basis_root = Some(root),
                None => {
                    diags.push(
                        Diagnostic::error("computed-basis", "computed basis expression was not compiled for this flow")
                            .label(clause.at, "the basis expression is not available"),
                    );
                    tail.valid = false;
                }
            },
            (Flow(_), ClauseKind::Price(literal)) => read_price(world, file, literal, clause.at, tail, diags),
            _ => self.refuse(clause, tail, diags),
        }
    }

    /// What a line says of a clause it does not take; a term line and an ending say nothing.
    fn refuse(self, clause: &ast::Clause<'s>, tail: &mut Tail, diags: &mut Vec<Diagnostic>) {
        let (code, message, label) = match (self, clause.kind) {
            (Line::Flow(_), _) => (
                "until-position",
                "`until` is only valid on a statement change or waiver",
                "it has no effect on a flow",
            ),
            (Line::Also, ClauseKind::Due(_)) => (
                "also-relative-due",
                "a derived flow's due date must be absolute",
                "write `due YYYY-MM-DD` on an implied line",
            ),
            (Line::Also, ClauseKind::Basis(_)) => (
                "computed-also-basis",
                "an implied basis must be literal",
                "this metadata field has no computed root in the Book",
            ),
            (Line::Also, _) => (
                "also-tail",
                "this clause is not retained on an implied line",
                "remove it or write the metadata on the source flow",
            ),
            (Line::Measure(_), _) => (
                "measure-tail",
                "this tail clause does not apply to a measure",
                "remove the clause or record it on a flow",
            ),
            (Line::Term | Line::Ending, _) => return,
        };
        diags.push(Diagnostic::error(code, message).label(clause.at, label));
        tail.valid = false;
    }

    /// `basis 400 USD`: what was paid for it, in the base currency. A unit that names no commodity is said as a
    /// missing unit, and on an `also` line as unknown first.
    fn read_basis(
        self,
        world: &World<'s>,
        file: &ast::File<'s>,
        literal: ast::Literal<'s>,
        tail: &mut Tail,
        diags: &mut Vec<Diagnostic>,
    ) {
        let at = file.loc(literal.0);
        let unit = match literal.unit().map(|unit| world.commodity_of(Word::of(file, unit.0))) {
            Some(Ok(unit)) => Some(unit),
            Some(Err(unknown)) if matches!(self, Line::Also) => {
                diags.push(unknown);
                None
            }
            _ => None,
        };
        let Some(unit) = unit else {
            diags.push(
                Diagnostic::error("basis-unit", "basis needs an explicit base-currency unit")
                    .label(at, "write the unit"),
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
}

impl<'s> FlowCx<'_, 's> {
    /// Reads a flow's tail: the codes it adds to the pool, and what the rest of it says.
    pub fn lower_tail(
        &self,
        world: &mut World<'s>,
        clauses: ast::Many<ast::Clause<'s>>,
        diags: &mut Vec<Diagnostic>,
    ) -> (Run<Sym>, Tail) {
        read_tail(world, self.home, self.file, Line::Flow(self), clauses, diags)
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
