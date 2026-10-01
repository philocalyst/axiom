//! Native S5 journal lowering.
//!
//! The first pass is deliberately small: declaration building needs only the
//! raw relationships that may require a party tab. Ordinary journal ends stay
//! in their syntax tables and are resolved by the recording pass; this survey
//! does not copy the journal into a second per-item plan.

use axiom_core::Loc;
use axiom_syntax as ast;
use axiom_syntax::{ClauseKind, Direction, End, ExprKind, ItemKind, Name, Subject, Verb};

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

    use crate::{Source, layout::Layout, scope::Home};

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
            layout: Layout::of(path),
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
}
