//! Shared metadata lowering for declaration and contract `also` lines.

use axiom_core::{Diagnostic, Id, Run, Sym};
use axiom_syntax as ast;
use axiom_syntax::ClauseKind;

use crate::book::Text;
use crate::declare::World;
use crate::errors::Word;
use crate::journal::{Detail, Provenance, Purposed, Select, Waive};
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
                let word = Word::of(file, written.name.0);
                match world.purpose(home, word) {
                    Ok(id) => {
                        let of =
                            written.of.and_then(|name| super::record::resolve_object(world, home, file, name, diags));
                        if written.of.is_some() && of.is_none() {
                            continue;
                        }
                        purpose = Some(Purposed { purpose: id, of, source: Provenance::Written });
                    }
                    Err(problem) => diags.push(problem),
                }
            }
            ClauseKind::Description(text) => {
                description = Some(world.book.quoted_text(text.0));
            }
            ClauseKind::Waive(written) => {
                waive =
                    Some(Waive { loc: written.at, reason: written.reason.map(|text| world.book.quoted_text(text.0)) });
            }
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
                let Some(unit) = literal.unit().and_then(|unit| {
                    world.commodity_of(Word::of(file, unit.0)).map_err(|problem| diags.push(problem)).ok()
                }) else {
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
