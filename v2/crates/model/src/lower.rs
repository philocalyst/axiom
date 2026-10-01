//! Native S5 journal lowering.
//!
//! The first pass is deliberately small: declaration building needs only the
//! raw relationships that may require a party tab. Ordinary journal ends stay
//! in their syntax tables and are resolved by the recording pass; this survey
//! does not copy the journal into a second per-item plan.

pub(crate) mod also;
mod contracts;
mod record;

pub(crate) use contracts::contracts;
pub(crate) use record::record;

use axiom_core::{Diagnostic, Loc, Map};
use axiom_syntax as ast;
use axiom_syntax::{ClauseKind, Direction, End, ExprKind, ItemKind, Name, Subject, Verb};

use crate::book::Input;
use crate::declare::World;
use crate::errors::Word;
use crate::law::Ty;
use crate::scope::Home;
use crate::sources::Site;

/// The two named ends that may need a claim tab. Either end may be omitted in
/// a split header; its legs fill that side in.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Ends<'s> {
    pub from: Option<Name<'s>>,
    pub to: Option<Name<'s>>,
}

/// A syntactic relationship the declaration pass must account for before it
/// freezes the place tree. Names remain borrowed from their source.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Mention<'s> {
    /// A tab-relevant pair of ends, from a claim clause or contract promise.
    Ends { ends: Ends<'s>, loc: Loc },
    /// `PARTY owes OWNER AMOUNT`, or its reverse.
    Claim {
        subject: Name<'s>,
        creditor: Name<'s>,
        loc: Loc,
    },
    /// A party or owner named by `for` on a tab-relevant line.
    For {
        other: Name<'s>,
        ends: Ends<'s>,
        loc: Loc,
    },
    /// A flow with an explicit deadline.
    Due { ends: Ends<'s>, loc: Loc },
    /// The contract's party and any holding/loan/deposit endpoints.
    Promise {
        name: Name<'s>,
        party: Name<'s>,
        holding: Option<Name<'s>>,
        loan_party: Option<Name<'s>>,
        deposit: Option<Name<'s>>,
        loc: Loc,
    },
}

/// Only claim and promise relationships needed before stable Place ids exist.
#[derive(Default, Debug)]
pub(crate) struct JournalSurvey<'s> {
    pub mentions: Vec<Mention<'s>>,
}

/// Where a raw endpoint occurred, so declaration preallocation can distinguish
/// ordinary journal counterparties from contextual contract/opening names.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum EndpointContext<'s> {
    Transaction,
    Statement,
    Opening,
    StatementSubject,
    ClaimSubject,
    ClaimCreditor,
    Via,
    ForParty,
    PurposeObject,
    SelectorEnd,
    ContractParty {
        contract: Name<'s>,
    },
    ContractHolding {
        contract: Name<'s>,
        direction: Direction,
    },
    ContractBody {
        contract: Name<'s>,
    },
    ContractAlso {
        contract: Name<'s>,
    },
    DeclarationAlso {
        name: Name<'s>,
        kind: ast::DeclKind,
    },
}

/// Visits every endpoint candidate without allocating or interning it. The
/// declaration builder filters names already declared as places/entities,
/// then preallocates only genuinely novel parties before place ids freeze.
pub(crate) fn visit_endpoints<'s>(
    sites: &[Site<'_, 's>],
    mut visit: impl FnMut(Home, Name<'s>, Loc, EndpointContext<'s>),
) {
    for site in sites {
        let file = &site.source.file;
        for item in &file.items {
            match item.kind {
                ItemKind::Txn(id) => visit_flow_ends(
                    file,
                    &file[id].flow,
                    site.home,
                    EndpointContext::Transaction,
                    &mut visit,
                ),
                ItemKind::Statement(id) => {
                    visit_statement_ends(file, &file[id], site.home, &mut visit)
                }
                ItemKind::Opening(id) => {
                    let opening = &file[id];
                    for leg in &file[opening.lines] {
                        visit(site.home, leg.end.name, leg.loc, EndpointContext::Opening);
                        visit_tail_names(file, leg.tail, site.home, &mut visit);
                    }
                    for statement in &file[opening.claims] {
                        visit_statement_ends(file, statement, site.home, &mut visit);
                    }
                }
                ItemKind::Contract(id) => {
                    let contract = &file[id];
                    for schedule in [contract.schedule, contract.standing].into_iter().flatten() {
                        if let Some(holding) = schedule.terms.holding {
                            visit(
                                site.home,
                                holding.name,
                                schedule.at,
                                EndpointContext::ContractHolding {
                                    contract: contract.name,
                                    direction: holding.direction,
                                },
                            );
                        }
                    }
                    if let Some(party) = contract.party {
                        visit(
                            site.home,
                            party,
                            file.loc(party.0),
                            EndpointContext::ContractParty {
                                contract: contract.name,
                            },
                        );
                    }
                    for leg in &file[contract.body.legs] {
                        visit(
                            site.home,
                            leg.end.name,
                            leg.loc,
                            EndpointContext::ContractBody {
                                contract: contract.name,
                            },
                        );
                        visit_tail_names(file, leg.tail, site.home, &mut visit);
                    }
                    for line in &file[contract.body.items] {
                        visit_tail_names(file, line.tail, site.home, &mut visit);
                    }
                    for also in &file[contract.alsos] {
                        match &also.line {
                            ast::AlsoLine::Flow(flow) => visit_flow_ends(
                                file,
                                flow,
                                site.home,
                                EndpointContext::ContractAlso {
                                    contract: contract.name,
                                },
                                &mut visit,
                            ),
                            ast::AlsoLine::Item(item) => {
                                visit_tail_names(file, item.tail, site.home, &mut visit)
                            }
                        }
                    }
                    if let Some(deadline) = &contract.deadline
                        && let Some(item) = &deadline.otherwise
                    {
                        visit_tail_names(file, item.tail, site.home, &mut visit);
                    }
                }
                ItemKind::Decl(id) => {
                    let declaration = &file[id];
                    for also in &file[declaration.alsos] {
                        let context = EndpointContext::DeclarationAlso {
                            name: declaration.name,
                            kind: declaration.what,
                        };
                        match &also.line {
                            ast::AlsoLine::Flow(flow) => {
                                visit_flow_ends(file, flow, site.home, context, &mut visit)
                            }
                            ast::AlsoLine::Item(item) => {
                                visit_tail_names(file, item.tail, site.home, &mut visit)
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

fn visit_statement_ends<'s>(
    file: &ast::File<'s>,
    statement: &ast::Statement<'s>,
    home: Home,
    visit: &mut impl FnMut(Home, Name<'s>, Loc, EndpointContext<'s>),
) {
    if let Subject::Name(name) = statement.subject {
        let context = if matches!(&statement.verb, ast::Verb::Owes { .. }) {
            EndpointContext::ClaimSubject
        } else {
            EndpointContext::StatementSubject
        };
        visit(home, name, file.loc(name.0), context);
    }
    if let ast::Verb::Owes { creditor, .. } = &statement.verb {
        visit(
            home,
            *creditor,
            file.loc(creditor.0),
            EndpointContext::ClaimCreditor,
        );
    }
    for leg in &file[statement.body.legs] {
        visit(home, leg.end.name, leg.loc, EndpointContext::Statement);
        visit_tail_names(file, leg.tail, home, visit);
    }
    for item in &file[statement.body.items] {
        visit_tail_names(file, item.tail, home, visit);
    }
    visit_tail_names(file, statement.tail, home, visit);
}

fn visit_flow_ends<'s>(
    file: &ast::File<'s>,
    flow: &ast::Flow<'s>,
    home: Home,
    context: EndpointContext<'s>,
    visit: &mut impl FnMut(Home, Name<'s>, Loc, EndpointContext<'s>),
) {
    for end in [flow.from.end, flow.to.end].into_iter().flatten() {
        visit(home, end.name, file.loc(end.name.0), context);
        for selector in &file[end.select] {
            if let ast::Select::End(name) = selector {
                visit(home, *name, file.loc(name.0), EndpointContext::SelectorEnd);
            }
        }
    }
    visit_tail_names(file, flow.tail, home, visit);
    for leg in &file[flow.body.legs] {
        visit(home, leg.end.name, leg.loc, context);
        visit_tail_names(file, leg.tail, home, visit);
    }
    for item in &file[flow.body.items] {
        visit_tail_names(file, item.tail, home, visit);
    }
}

fn visit_tail_names<'s>(
    file: &ast::File<'s>,
    clauses: ast::Many<ast::Clause<'s>>,
    home: Home,
    visit: &mut impl FnMut(Home, Name<'s>, Loc, EndpointContext<'s>),
) {
    for clause in &file[clauses] {
        match clause.kind {
            ClauseKind::Via(name) => visit(home, name, clause.at, EndpointContext::Via),
            ClauseKind::For(ast::For::Whom(name)) => {
                visit(home, name, clause.at, EndpointContext::ForParty)
            }
            ClauseKind::Purpose(purpose) => {
                if let Some(name) = purpose.of {
                    visit(home, name, clause.at, EndpointContext::PurposeObject);
                }
            }
            _ => {}
        }
    }
}

/// Finds tab and promise relationships before declarations freeze the place
/// tree. The scan borrows each syntax node directly and retains no ordinary
/// flow or statement records.
pub(crate) fn survey<'s>(sites: &[Site<'_, 's>]) -> JournalSurvey<'s> {
    let mut survey = JournalSurvey::default();
    for site in sites {
        let file = &site.source.file;
        for item in &file.items {
            match item.kind {
                ItemKind::Txn(id) => scan_flow(file, &file[id].flow, &mut survey),
                ItemKind::Statement(id) => scan_statement(file, &file[id], item.loc, &mut survey),
                ItemKind::Opening(id) => {
                    let opening = &file[id];
                    for leg in &file[opening.lines] {
                        let ends = Ends {
                            from: None,
                            to: Some(leg.end.name),
                        };
                        scan_tail(file, leg.tail, ends, &mut survey);
                    }
                    for claim in &file[opening.claims] {
                        scan_statement(file, claim, subject_loc(file, claim.subject), &mut survey);
                    }
                }
                ItemKind::Contract(id) => scan_contract(file, &file[id], item.loc, &mut survey),
                _ => {}
            }
        }
    }
    survey
}

/// Reads the ordered input bindings a contract template may use. The engine
/// binds occurrence values by this order, while the compiler resolves each
/// input name to its stable `Var::Input` index.
fn inputs<'s>(
    world: &mut World<'s>,
    file: &ast::File<'s>,
    props: ast::Many<ast::Prop<'s>>,
    diags: &mut Vec<Diagnostic>,
) -> Box<[Input]> {
    let mut found: Vec<Input> = Vec::new();
    let mut seen: Map<axiom_core::Sym, Loc> = Map::default();
    for prop in &file[props] {
        if prop.name.0 != "input" {
            continue;
        }
        let args = &file[prop.args];
        if args.len() > 2 {
            diags.push(
                Diagnostic::error(
                    "contract-input",
                    "an input takes a name and at most one unit",
                )
                .label(prop.loc, "extra input arguments are not used")
                .help("write `input NAME` or `input NAME UNIT`"),
            );
            continue;
        }
        let Some(&name_id) = args.first() else {
            diags.push(
                Diagnostic::error("contract-input", "an input needs a name")
                    .label(prop.loc, "write `input NAME [UNIT]`"),
            );
            continue;
        };
        let ExprKind::Name(name) = file.exprs[name_id].kind else {
            diags.push(
                Diagnostic::error("contract-input", "an input name must be a word")
                    .label(file.exprs[name_id].loc, "write the input name here"),
            );
            continue;
        };
        let symbol = world.book.names.intern(name.0);
        if let Some(first) = seen.get(&symbol) {
            diags.push(
                Diagnostic::error(
                    "duplicate-input",
                    format!("input `{}` is declared twice", name.0),
                )
                .label(prop.loc, "declared again here")
                .context(*first, "first declared here")
                .help("keep one declaration so every occurrence has one binding"),
            );
            continue;
        }
        seen.insert(symbol, prop.loc);

        if found.len() > usize::from(u16::MAX) {
            diags.push(
                Diagnostic::error("too-many-inputs", "a contract has too many inputs")
                    .label(prop.loc, "input index exceeds the template limit")
                    .help("remove unused inputs; a template supports indices 0 through 65535"),
            );
            continue;
        }

        let unit = match args.get(1).map(|&id| &file.exprs[id]) {
            None => None,
            Some(expr) => {
                let unit_name = match expr.kind {
                    ExprKind::Unit(unit) | ExprKind::Name(unit) => unit,
                    _ => {
                        diags.push(
                            Diagnostic::error(
                                "contract-input-unit",
                                "an input unit must name a commodity",
                            )
                            .label(expr.loc, "write a commodity such as `USD`"),
                        );
                        continue;
                    }
                };
                match world.commodity_of(Word {
                    text: unit_name.0,
                    loc: expr.loc,
                }) {
                    Ok(unit) => Some(unit),
                    Err(diagnostic) => {
                        diags.push(diagnostic);
                        continue;
                    }
                }
            }
        };

        found.push(Input {
            name: symbol,
            unit,
            loc: prop.loc,
        });
    }
    found.into_boxed_slice()
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
            changed_term_roots(
                file,
                schedule.terms,
                contract.body,
                contract.deadline.as_ref(),
            )
        })
    };
    ContractRoots {
        regular: roots(contract.schedule),
        standing: roots(contract.standing),
    }
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

fn push_body_roots<'s>(
    file: &ast::File<'s>,
    body: ast::Body<'s>,
    roots: &mut Vec<(ast::ExprId, Ty)>,
) {
    for leg in &file[body.legs] {
        match leg.amount {
            ast::Quantity::Amount(amount)
            | ast::Quantity::Pending(amount)
            | ast::Quantity::Target(amount) => push_amount_root(amount, roots),
            ast::Quantity::Unknown(_)
            | ast::Quantity::All(_)
            | ast::Quantity::Rest
            | ast::Quantity::Whole => {}
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
    diags: &mut Vec<Diagnostic>,
) -> Option<(
    crate::book::TemplateProgram,
    Map<ast::ExprId, crate::law::NodeId>,
)> {
    if roots.is_empty() {
        return Some((crate::book::TemplateProgram::default(), Map::default()));
    }
    let (program, nodes) =
        crate::laws::compile_template(world, diags, file, home, subject, name, inputs, roots)?;
    let by_expr = roots
        .iter()
        .zip(nodes.iter())
        .map(|(&(expr, _), &node)| (expr, node))
        .collect();
    Some((program, by_expr))
}

fn scan_contract<'s>(
    file: &ast::File<'s>,
    contract: &ast::Contract<'s>,
    loc: Loc,
    survey: &mut JournalSurvey<'s>,
) {
    let party = contract.party.unwrap_or(contract.name);
    let primary = contract.schedule.or(contract.standing);
    let holding = primary.and_then(|schedule| schedule.terms.holding.map(|holding| holding.name));
    let has_loan = file[contract.props]
        .iter()
        .any(|prop| prop.name.0 == "loan");
    let deposit = file[contract.props]
        .iter()
        .find(|prop| prop.name.0 == "deposit")
        .and_then(|prop| last_name(file, prop.args));

    survey.mentions.push(Mention::Promise {
        name: contract.name,
        party,
        holding,
        loan_party: has_loan.then_some(party),
        deposit,
        loc,
    });

    let mut template_ends = None;
    if let Some(schedule) = contract.schedule {
        let ends = schedule_ends(
            party,
            schedule
                .terms
                .holding
                .map(|holding| (holding.direction, holding.name)),
        );
        mention_ends(ends, schedule.at, survey);
        template_ends = Some(ends);
    }
    if let Some(schedule) = contract.standing {
        let ends = schedule_ends(
            party,
            schedule
                .terms
                .holding
                .map(|holding| (holding.direction, holding.name)),
        );
        mention_ends(ends, schedule.at, survey);
        template_ends.get_or_insert(ends);
    }

    if let Some(ends) = template_ends {
        scan_body(file, contract.body, ends, survey);
        if let Some(deadline) = contract.deadline.as_ref() {
            if let Some(item) = deadline.otherwise.as_ref() {
                scan_item(file, item, ends, survey);
            }
        }
        for also in &file[contract.alsos] {
            match &also.line {
                ast::AlsoLine::Flow(flow) => scan_flow(file, flow, survey),
                ast::AlsoLine::Item(item) => scan_item(file, item, ends, survey),
            }
        }
    } else {
        for also in &file[contract.alsos] {
            if let ast::AlsoLine::Flow(flow) = &also.line {
                scan_flow(file, flow, survey);
            }
        }
    }
}

fn last_name<'s>(file: &ast::File<'s>, expressions: ast::Many<ast::ExprId>) -> Option<Name<'s>> {
    file[expressions]
        .iter()
        .rev()
        .find_map(|&id| match file.exprs[id].kind {
            ExprKind::Name(name) if name.0 != "into" => Some(name),
            _ => None,
        })
}

fn schedule_ends<'s>(party: Name<'s>, holding: Option<(Direction, Name<'s>)>) -> Ends<'s> {
    match holding {
        Some((Direction::From, holding)) => Ends {
            from: Some(holding),
            to: Some(party),
        },
        Some((Direction::Into, holding)) => Ends {
            from: Some(party),
            to: Some(holding),
        },
        None => Ends {
            from: Some(party),
            to: None,
        },
    }
}

fn scan_flow<'s>(file: &ast::File<'s>, flow: &ast::Flow<'s>, survey: &mut JournalSurvey<'s>) {
    let from = flow.from.end.map(end_name);
    let to = flow.to.end.map(end_name);
    let header = Ends { from, to };
    if flow.body.legs.is_empty() && flow.body.items.is_empty() {
        scan_tail(file, flow.tail, header, survey);
        return;
    }

    for leg in &file[flow.body.legs] {
        let ends = leg_ends(header, leg.end);
        scan_tail(file, flow.tail, ends, survey);
        scan_tail(file, leg.tail, ends, survey);
    }
    for item in &file[flow.body.items] {
        scan_tail(file, flow.tail, header, survey);
        scan_tail(file, item.tail, header, survey);
    }
}

fn scan_body<'s>(
    file: &ast::File<'s>,
    body: ast::Body<'s>,
    header: Ends<'s>,
    survey: &mut JournalSurvey<'s>,
) {
    for leg in &file[body.legs] {
        let ends = leg_ends(header, leg.end);
        scan_tail(file, leg.tail, ends, survey);
    }
    for item in &file[body.items] {
        scan_tail(file, item.tail, header, survey);
    }
}

fn scan_item<'s>(
    file: &ast::File<'s>,
    item: &ast::LineItem<'s>,
    ends: Ends<'s>,
    survey: &mut JournalSurvey<'s>,
) {
    scan_tail(file, item.tail, ends, survey);
}

fn scan_statement<'s>(
    file: &ast::File<'s>,
    statement: &ast::Statement<'s>,
    loc: Loc,
    survey: &mut JournalSurvey<'s>,
) {
    let subject = match statement.subject {
        Subject::Name(name) => Some(name),
        _ => None,
    };
    if let (Some(subject), Verb::Owes { creditor, .. }) = (subject, &statement.verb) {
        survey.mentions.push(Mention::Claim {
            subject,
            creditor: *creditor,
            loc,
        });
        let ends = Ends {
            from: Some(subject),
            to: Some(*creditor),
        };
        scan_tail(file, statement.tail, ends, survey);
        for item in &file[statement.body.items] {
            scan_tail(file, item.tail, ends, survey);
        }
        return;
    }

    // In `NAME now TERMS`, the subject is syntactically a contract name. Keep
    // that borrowed name as the contextual party marker; the declaration
    // resolver substitutes the contract's actual party before registering a
    // tab. A restated holding changes which end is explicit.
    let ends = match (&statement.subject, &statement.verb) {
        (Subject::Name(contract), Verb::Now(ast::Change::Terms(id))) => {
            let terms = &file[*id];
            terms.holding.map_or(
                Ends {
                    from: Some(*contract),
                    to: None,
                },
                |holding| schedule_ends(*contract, Some((holding.direction, holding.name))),
            )
        }
        _ => Ends {
            from: subject,
            to: None,
        },
    };
    if matches!(statement.verb, Verb::Now(ast::Change::Terms(_))) {
        mention_ends(ends, loc, survey);
    }
    scan_tail(file, statement.tail, ends, survey);
    match &statement.verb {
        Verb::Occurrence(_) | Verb::Now(ast::Change::Terms(_)) => {
            for leg in &file[statement.body.legs] {
                let leg_ends = leg_ends(ends, leg.end);
                mention_ends(leg_ends, leg.loc, survey);
                scan_tail(file, leg.tail, leg_ends, survey);
            }
            for item in &file[statement.body.items] {
                scan_tail(file, item.tail, ends, survey);
            }
        }
        Verb::Waived => {
            for item in &file[statement.body.items] {
                scan_tail(file, item.tail, ends, survey);
            }
        }
        _ => {}
    }
}

fn scan_tail<'s>(
    file: &ast::File<'s>,
    clauses: ast::Many<ast::Clause<'s>>,
    ends: Ends<'s>,
    survey: &mut JournalSurvey<'s>,
) {
    for clause in &file[clauses] {
        match clause.kind {
            ClauseKind::For(ast::For::Whom(other)) => {
                mention_ends(ends, clause.at, survey);
                survey.mentions.push(Mention::For {
                    other,
                    ends,
                    loc: clause.at,
                });
            }
            ClauseKind::Due(_) => {
                mention_ends(ends, clause.at, survey);
                survey.mentions.push(Mention::Due {
                    ends,
                    loc: clause.at,
                });
            }
            _ => {}
        }
    }
}

fn mention_ends<'s>(ends: Ends<'s>, loc: Loc, survey: &mut JournalSurvey<'s>) {
    if ends.from.is_some() || ends.to.is_some() {
        survey.mentions.push(Mention::Ends { ends, loc });
    }
}

fn leg_ends<'s>(header: Ends<'s>, leg: End<'s>) -> Ends<'s> {
    match (header.from, header.to) {
        (Some(from), None) => Ends {
            from: Some(from),
            to: Some(leg.name),
        },
        (None, Some(to)) => Ends {
            from: Some(leg.name),
            to: Some(to),
        },
        _ => header,
    }
}

fn end_name<'s>(end: End<'s>) -> Name<'s> {
    end.name
}

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
    use axiom_syntax::{Folder, parse};

    use crate::{Source, scope::Home};

    #[test]
    fn survey_keeps_only_tab_relevant_syntax_and_contract_endpoints() {
        let path = "journal/2026/01.ax";
        let source_text = "\
contract lease with dana
  2_350 USD monthly on 1 into checking
  input water USD
  deposit 2_350 USD into escrow
  + 12% of water #utilities
  also -> escrow 410 USD #escrow
2026-01-02 lease now 2_500 USD monthly into savings
2026-01-02 checking -> dana 100 USD due 30d for dana
2026-01-03 dana owes me 100 USD due 30d
opening 2026-01-01
  dana owes me 20 USD
2026-01-04 checking -> grocer 50 USD
";
        let (file, diagnostics) = parse(FileId(0), source_text, Folder::of(path));
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let source = Source {
            path,
            file,
            embedded: false,
        };
        let site = Site {
            source: &source,
            home: Home::Project,
        };
        let survey = survey(&[site]);

        assert!(survey.mentions.iter().any(|mention| matches!(
            mention,
            Mention::Promise { name, party, holding: Some(holding), deposit: Some(deposit), loan_party: None, .. }
                if (name.0, party.0, holding.0, deposit.0) == ("lease", "dana", "checking", "escrow")
        )));
        assert!(survey.mentions.iter().any(|mention| matches!(
            mention,
            Mention::Claim { subject, creditor, .. } if (subject.0, creditor.0) == ("dana", "me")
        )));
        assert!(survey.mentions.iter().any(|mention| matches!(
            mention,
            Mention::Due { ends: Ends { from: Some(from), to: Some(to) }, .. }
                if (from.0, to.0) == ("checking", "dana")
        )));
        assert!(survey.mentions.iter().any(|mention| matches!(
            mention,
            Mention::Ends { ends: Ends { from: Some(from), to: Some(to) }, .. }
                if (from.0, to.0) == ("lease", "savings")
        )));
        assert!(survey.mentions.iter().any(|mention| matches!(
            mention,
            Mention::For { other, ends: Ends { from: Some(from), to: Some(to) }, .. }
                if (other.0, from.0, to.0) == ("dana", "checking", "dana")
        )));
        assert!(!survey.mentions.iter().any(|mention| matches!(
            mention,
            Mention::Ends { ends: Ends { from: Some(from), to: Some(to) }, .. }
                if (from.0, to.0) == ("checking", "grocer")
        )));
    }

    #[test]
    fn endpoint_visitor_covers_claims_declaration_also_and_flow_tails() {
        let path = "journal/endpoints.ax";
        let source_text = "\
entity fund : institution
  also issuer -> self 2 USD for holder via market
contract lease with landlord
  2_350 USD monthly from checking
  also checking -> escrow 410 USD for recipient via bank
2026-01-03 borrower owes lender 100 USD due 5d for beneficiary via clearing
opening 2026-01-01
  borrower owes lender 20 USD due 3d for opener
";
        let (file, diagnostics) = parse(FileId(0), source_text, Folder::of(path));
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let source = Source {
            path,
            file,
            embedded: false,
        };
        let site = Site {
            source: &source,
            home: Home::Project,
        };
        let mut seen = Vec::new();
        visit_endpoints(&[site], |home, name, loc, context| {
            assert_eq!(home, Home::Project);
            assert!(loc.start <= loc.end);
            assert_eq!(loc.file, FileId(0));
            assert!(
                source_text[loc.range()].contains(name.0),
                "{name:?} at {loc:?}"
            );
            seen.push((name.0, context, loc));
        });

        assert!(seen.iter().any(|(name, context, _)| {
            *name == "borrower" && matches!(context, EndpointContext::ClaimSubject)
        }));
        assert_eq!(
            seen.iter()
                .filter(|(name, context, _)| {
                    *name == "borrower" && matches!(context, EndpointContext::ClaimSubject)
                })
                .count(),
            2,
            "the dated claim and opening claim are both visited"
        );
        assert!(seen.iter().any(|(name, context, _)| {
            *name == "lender" && matches!(context, EndpointContext::ClaimCreditor)
        }));
        assert!(seen.iter().any(|(name, context, _)| {
            *name == "holder" && matches!(context, EndpointContext::ForParty)
        }));
        assert!(seen.iter().any(|(name, context, _)| {
            *name == "bank" && matches!(context, EndpointContext::Via)
        }));
        assert!(seen.iter().any(|(name, context, _)| {
            *name == "issuer"
                && matches!(context, EndpointContext::DeclarationAlso { name: decl, kind: ast::DeclKind::Entity } if decl.0 == "fund")
        }));
        assert!(seen.iter().any(|(name, context, _)| {
            *name == "self"
                && matches!(context, EndpointContext::DeclarationAlso { name: decl, kind: ast::DeclKind::Entity } if decl.0 == "fund")
        }));
        assert!(seen.iter().any(|(name, context, _)| {
            *name == "escrow"
                && matches!(context, EndpointContext::ContractAlso { contract } if contract.0 == "lease")
        }));
        assert!(seen.iter().any(|(name, context, _)| {
            *name == "recipient" && matches!(context, EndpointContext::ForParty)
        }));
    }

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
        let ItemKind::Contract(id) = file.items[0].kind else {
            panic!("contract expected")
        };
        let roots = contract_roots(&file, &file[id]);
        assert_eq!(roots.regular.len(), 2);
        assert!(matches!(
            file.exprs[roots.regular[0].0].kind,
            ExprKind::Of(_, _)
        ));
        assert!(matches!(
            file.exprs[roots.regular[1].0].kind,
            ExprKind::Pct(_)
        ));
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
        let ItemKind::Contract(contract_id) = file.items[0].kind else {
            panic!("contract expected")
        };
        let contract = &file[contract_id];
        assert!(contract.schedule.is_some() && contract.standing.is_some());
        let roots = contract_roots(&file, contract);
        assert_eq!(roots.regular.len(), 3);
        assert_eq!(roots.standing.len(), 3);
        assert!(matches!(
            file.exprs[roots.regular[0].0].kind,
            ExprKind::Of(_, _)
        ));
        assert!(matches!(
            file.exprs[roots.standing[0].0].kind,
            ExprKind::Of(_, _)
        ));

        let ItemKind::Statement(statement_id) = file.items[1].kind else {
            panic!("statement expected")
        };
        let statement = &file[statement_id];
        let Verb::Now(ast::Change::Terms(terms_id)) = statement.verb else {
            panic!("terms change expected")
        };
        let changed = changed_term_roots(&file, file[terms_id], statement.body, None);
        assert_eq!(changed.len(), 2);
    }
}
