//! Native S5 journal lowering: the journal and the contracts become flows.
//!
//! A claim tab, the place that keeps what a party owes an owner, is not found before lowering by a walk of the
//! journal: the claim or the loan that needs one asks `World::tab`, which makes it then.
//!
//! | module       | job                                                                          |
//! |--------------|------------------------------------------------------------------------------|
//! | `record`     | the journal in order: transactions, openings, occurrences and claims         |
//! | `loan_opening` | the debt of a loan made before the book, opened with what its terms say     |
//! | `statements` | the dated statements that are not flows: values, measures, endings, basis    |
//! | `flow`       | one flow: its quantities, its ends, its items                                |
//! | `tail`       | the clauses after an amount, read once for flows, terms and derived lines    |
//! | `infer`      | a flow's purpose, when it names none, from what its ends say                 |
//! | `contracts`  | a contract's facts, schedules, loan, deposit, and the laws its lines imply   |
//! | `staged`     | the guard that takes back what a rejected record had already added           |

mod contracts;
mod flow;
mod infer;
mod loan_opening;
mod record;
mod staged;
mod statements;
pub(crate) mod tail;

pub(crate) use contracts::contracts;
pub(crate) use record::record;

use axiom_core::{Diagnostic, Id, Loc, Map};
use axiom_syntax as ast;
use axiom_syntax::{ExprKind, Subject};

use crate::args::Args;
use crate::book::{Commodity, Input};
use crate::declare::World;
use crate::errors::{Reported, Word};
use crate::journal::Program;
use crate::law::Ty;
use crate::problem::{self, Noun};
use crate::scope::Home;

/// Reads the ordered input bindings a contract template may use (`input NAME [UNIT]`). The engine binds occurrence
/// values by this order, while the compiler resolves each input name to its stable `Var::Input` index.
fn inputs<'s>(world: &mut World<'s>, file: &ast::File<'s>, props: ast::Many<ast::Prop<'s>>) -> Box<[Input]> {
    let mut found: Vec<Input> = Vec::new();
    let mut seen: Map<axiom_core::Sym, Loc> = Map::default();
    for line in file[props].iter().filter(|prop| prop.name.0 == "input") {
        let Some((name, unit)) = input(world, file, line).or_report(world) else { continue };
        let symbol = world.book.names.intern(name.text);
        if let Some(first) = seen.get(&symbol) {
            world.diags.push(problem::duplicate(Noun::Input, Word { text: name.text, loc: line.loc }, Some(*first)));
            continue;
        }
        seen.insert(symbol, line.loc);
        if found.len() > usize::from(u16::MAX) {
            world.diags.push(
                Diagnostic::error("too-many-inputs", "a contract has too many inputs")
                    .label(line.loc, "input index exceeds the template limit")
                    .help("remove unused inputs; a template supports indices 0 through 65535"),
            );
            continue;
        }
        found.push(Input { name: symbol, unit, loc: line.loc });
    }
    found.into_boxed_slice()
}

/// `input hours HR`: an input's name, and the commodity it is counted in if it says.
fn input<'s>(
    world: &World<'s>,
    file: &ast::File<'s>,
    line: &ast::Prop<'s>,
) -> Result<(Word<'s>, Option<Id<Commodity>>), Diagnostic> {
    let mut a = Args::shaped(file, line, "contract-input", "input NAME [UNIT]");
    let name = a.name("a name")?;
    let unit = match a.peek() {
        None => None,
        Some(_) => {
            let unit = |expr: &ast::Expr<'s>| match expr.kind {
                ExprKind::Unit(unit) | ExprKind::Name(unit) => Some(Word { text: unit.0, loc: expr.loc }),
                _ => None,
            };
            Some(world.commodity_of(a.with("contract-input-unit", |a| a.arg("a commodity such as `USD`", unit))?)?)
        }
    };
    a.done().map(|()| (name, unit))
}

/// The independent computed roots owned by the regular and standing terms.
/// Each schedule keeps its own program and chronology.
#[derive(Default)]
struct ContractRoots {
    regular: Vec<(ast::ExprId, Ty)>,
    standing: Vec<(ast::ExprId, Ty)>,
}

fn contract_roots<'s>(file: &ast::File<'s>, contract: &ast::Contract<'s>) -> ContractRoots {
    let roots = |schedule: Option<ast::Schedule<'s>>| {
        schedule.map_or_else(Vec::new, |schedule| {
            changed_term_roots(file, schedule.terms, contract.body, contract.deadline.as_ref())
        })
    };
    ContractRoots { regular: roots(contract.schedule), standing: roots(contract.standing) }
}

fn push_payment_roots<'s>(payment: Option<ast::Payment<'s>>, roots: &mut Vec<(ast::ExprId, Ty)>) {
    match payment {
        Some(ast::Payment::Fixed(amount)) => push_amount_root(amount, roots),
        Some(ast::Payment::Buy { spend, .. }) => push_amount_root(spend, roots),
        None => {}
    }
}

/// The roots a changed `now TERMS` owns. It follows the same typed compiler
/// route as a declaration: payment, body, and any deadline item are compiled
/// together so expressions share nodes and input slots.
fn changed_term_roots<'s>(
    file: &ast::File<'s>,
    terms: ast::Terms<'s>,
    body: ast::Body<'s>,
    deadline: Option<&ast::Deadline<'s>>,
) -> Vec<(ast::ExprId, Ty)> {
    let mut roots = Vec::new();
    push_payment_roots(terms.payment, &mut roots);
    push_body_roots(file, body, &mut roots);
    if let Some(item) = deadline.and_then(|deadline| deadline.otherwise.as_ref()) {
        push_amount_root(item.amount, &mut roots);
    }
    roots
}

fn push_body_roots<'s>(file: &ast::File<'s>, body: ast::Body<'s>, roots: &mut Vec<(ast::ExprId, Ty)>) {
    for leg in &file[body.legs] {
        match leg.amount {
            ast::Quantity::Amount(ast::Amount::Computed(expr))
                if matches!(file.exprs[expr].kind, ast::ExprKind::Pct(_)) => {}
            ast::Quantity::Amount(amount) | ast::Quantity::Pending(amount) | ast::Quantity::Target(amount) => {
                push_amount_root(amount, roots)
            }
            ast::Quantity::Unknown(_) | ast::Quantity::All(_) | ast::Quantity::Rest | ast::Quantity::Whole => {}
        }
    }
    for item in &file[body.items] {
        push_amount_root(item.amount, roots);
    }
}

fn push_amount_root<'s>(amount: ast::Amount<'s>, roots: &mut Vec<(ast::ExprId, Ty)>) {
    if let ast::Amount::Computed(root) = amount {
        roots.push((root, Ty::AMOUNT));
    }
}

/// Compiles a term's computed roots once, retaining a direct root lookup for
/// the template flows that refer to them. A failed expression invalidates the
/// whole template so no caller can mistake a placeholder for a real amount.
fn compile_roots<'s>(
    world: &mut World<'s>,
    file: &ast::File<'s>,
    home: Home,
    subject: Ty,
    name: axiom_core::Sym,
    inputs: &[Input],
    roots: &[(ast::ExprId, Ty)],
) -> Option<(Program, Map<ast::ExprId, crate::law::NodeId>)> {
    if roots.is_empty() {
        return Some((Program::default(), Map::default()));
    }
    let (program, nodes) = crate::laws::compile_template(world, file, home, subject, name, inputs, roots)?;
    let by_expr = roots.iter().zip(nodes.iter()).map(|(&(expr, _), &node)| (expr, node)).collect();
    Some((program, by_expr))
}

/// Where a statement's subject is written.
fn subject_loc(file: &ast::File<'_>, subject: Subject<'_>) -> Loc {
    match subject {
        Subject::Name(name) | Subject::Purpose(name) | Subject::Unit(name) => file.loc(name.0),
        Subject::Code(code) => file.loc(code.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_core::FileId;
    use axiom_syntax::{Folder, ItemKind, Verb, parse};

    #[test]
    fn contract_roots_cover_input_items_and_deadline_else_once() {
        let path = "contracts.ax";
        let source_text = "\
contract flat with greystar
  2_900 USD monthly on 1 from checking
  input water USD
  + 12% of water #utilities
  due 5d else + 5% #late-fee
";
        let (file, diagnostics) = parse(FileId(0), source_text, Folder::of(path));
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let ItemKind::Contract(id) = file.items[0].kind else { panic!("contract expected") };
        let roots = contract_roots(&file, &file[id]);
        assert_eq!(roots.regular.len(), 2);
        assert!(matches!(file.exprs[roots.regular[0].0].kind, ExprKind::Of(_, _)));
        assert!(matches!(file.exprs[roots.regular[1].0].kind, ExprKind::Pct(_)));
    }

    #[test]
    fn contract_and_changed_terms_collect_each_computed_root_once() {
        let source_text = "\
contract c with p
  12% of ^base monthly from checking
  buy VTI for 3/4 of ^base monthly from checking
  + 5% of ^base
  due 5d else + 2% of ^base
2026-01-01 c now 10 USD monthly from checking
  + 3% of ^base
  + 1/4 of ^base
";
        let (file, diagnostics) = parse(FileId(0), source_text, Folder::of("contracts.ax"));
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let ItemKind::Contract(contract_id) = file.items[0].kind else { panic!("contract expected") };
        let contract = &file[contract_id];
        assert!(contract.schedule.is_some() && contract.standing.is_some());
        let roots = contract_roots(&file, contract);
        assert_eq!(roots.regular.len(), 3);
        assert_eq!(roots.standing.len(), 3);
        assert!(matches!(file.exprs[roots.regular[0].0].kind, ExprKind::Of(_, _)));
        assert!(matches!(file.exprs[roots.standing[0].0].kind, ExprKind::Of(_, _)));

        let ItemKind::Statement(statement_id) = file.items[1].kind else { panic!("statement expected") };
        let statement = &file[statement_id];
        let Verb::Now(ast::Change::Terms(terms_id)) = statement.verb else { panic!("terms change expected") };
        let changed = changed_term_roots(&file, file[terms_id], statement.body, None);
        assert_eq!(changed.len(), 2);
    }
}
