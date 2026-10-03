//! The names the sources write where a party can stand, and where each is first written.
//!
//! A party that no declaration names exists because a journal names it (LANGUAGE.md: "a party that is never declared
//! can still be written"), and the entities are frozen in their tree before anything is lowered. So the names an end, a
//! claim, a `for`, a `via` or a contract's `with` writes are found first, by one walk of the items that borrows each name
//! from its source and makes no copy of the journal. The walk says only the two things that follow from the names: where
//! each is first written (the entity's source line), and whether it is ever written as a *party*, which a contract's own
//! name must be to be taken for one.
//!
//! Which claim tabs a journal will want is not decided here, or anywhere before the journal is lowered: a tab is made
//! by the first claim that asks for it.

use axiom_core::{Loc, Map, Set};
use axiom_syntax as ast;
use axiom_syntax::{ClauseKind, ItemKind, Name, Subject, Verb};

use crate::sources::Site;

/// The names every kind declares a slot by (`has employer org`).
fn slot_names<'s>(sites: &[Site<'_, 's>]) -> Set<&'s str> {
    let mut names = Set::default();
    for site in sites {
        let file = &site.source.file;
        for item in &file.items {
            if let ItemKind::Decl(id) = item.kind
                && file[id].what == ast::DeclKind::Kind
            {
                names.extend(file[file[id].slots].iter().map(|has| has.name.0));
            }
        }
    }
    names
}

/// How a name is written: as a party, which is a claim's debtor or creditor, `for WHOM`, or a contract's `with`; or as
/// anything else a party can stand for, an end of a flow, a `via`, the object of a purpose.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Role {
    Party,
    End,
}

/// The names written where a party can stand, each with where it is first written.
#[derive(Default, Debug)]
pub(super) struct Mentions<'s> {
    pub first: Map<&'s str, Loc>,
    /// The names that are written as a party at least once.
    pub parties: Set<&'s str>,
    /// The roles of a kind, while its `also` lines are read: a leg of `employment` that starts at `employer` starts at
    /// whoever fills the slot, and no party is made of the word.
    roles: Set<&'s str>,
}

impl<'s> Mentions<'s> {
    /// Every name of every source, in the order the sources were arranged and the items written.
    pub fn of(sites: &[Site<'_, 's>]) -> Mentions<'s> {
        let mut mentions = Mentions::default();
        let slots = slot_names(sites);
        // A contract with no `with` is with the party its own name says. That is a party only once every end has had
        // its say, so that a name written as an end somewhere is first written there.
        let mut named_for_party = Vec::new();
        for site in sites {
            let file = &site.source.file;
            for item in &file.items {
                match item.kind {
                    ItemKind::Txn(id) => mentions.flow(file, &file[id].flow),
                    ItemKind::Statement(id) => mentions.statement(file, &file[id]),
                    ItemKind::Opening(id) => mentions.opening(file, &file[id]),
                    ItemKind::Contract(id) => {
                        let contract = &file[id];
                        mentions.contract(file, contract);
                        if contract.party.is_none() {
                            named_for_party.push((contract.name, item.loc));
                        }
                    }
                    ItemKind::Decl(id) => {
                        let decl = &file[id];
                        mentions.roles = if decl.what == ast::DeclKind::Kind { slots.clone() } else { Set::default() };
                        mentions.alsos(file, decl.alsos);
                    }
                    _ => {}
                }
            }
        }
        for (name, loc) in named_for_party {
            mentions.see(name, loc, Role::Party);
        }
        mentions
    }

    fn see(&mut self, name: Name<'s>, loc: Loc, role: Role) {
        if self.roles.contains(name.0) {
            return;
        }
        self.first.entry(name.0).or_insert(loc);
        if role == Role::Party {
            self.parties.insert(name.0);
        }
    }

    fn opening(&mut self, file: &ast::File<'s>, opening: &ast::Opening<'s>) {
        for leg in &file[opening.lines] {
            self.see(leg.end.name, leg.loc, Role::End);
            self.tail(file, leg.tail);
        }
        for claim in &file[opening.claims] {
            self.statement(file, claim);
        }
    }

    fn contract(&mut self, file: &ast::File<'s>, contract: &ast::Contract<'s>) {
        for schedule in [contract.schedule, contract.standing].into_iter().flatten() {
            if let Some(holding) = schedule.terms.holding {
                self.see(holding.name, schedule.at, Role::End);
            }
        }
        if let Some(party) = contract.party {
            self.see(party, file.loc(party.0), Role::Party);
        }
        for leg in &file[contract.body.legs] {
            self.see(leg.end.name, leg.loc, Role::End);
            self.tail(file, leg.tail);
        }
        for line in &file[contract.body.items] {
            self.tail(file, line.tail);
        }
        self.alsos(file, contract.alsos);
        if let Some(item) = contract.deadline.as_ref().and_then(|deadline| deadline.otherwise.as_ref()) {
            self.tail(file, item.tail);
        }
    }

    /// The lines an `also` adds to a declaration or a contract: flows, whose ends are ends, and items.
    fn alsos(&mut self, file: &ast::File<'s>, alsos: ast::Many<ast::Also<'s>>) {
        for also in &file[alsos] {
            match &also.line {
                ast::AlsoLine::Flow(flow) => self.flow(file, flow),
                ast::AlsoLine::Item(item) => self.tail(file, item.tail),
            }
        }
    }

    fn statement(&mut self, file: &ast::File<'s>, statement: &ast::Statement<'s>) {
        let claim = matches!(&statement.verb, Verb::Owes { .. });
        if let Subject::Name(name) = statement.subject {
            self.see(name, file.loc(name.0), if claim { Role::Party } else { Role::End });
        }
        if let Verb::Owes { creditor, .. } = &statement.verb {
            self.see(*creditor, file.loc(creditor.0), Role::Party);
        }
        for leg in &file[statement.body.legs] {
            self.see(leg.end.name, leg.loc, Role::End);
            self.tail(file, leg.tail);
        }
        for item in &file[statement.body.items] {
            self.tail(file, item.tail);
        }
        self.tail(file, statement.tail);
    }

    fn flow(&mut self, file: &ast::File<'s>, flow: &ast::Flow<'s>) {
        for end in [flow.from.end, flow.to.end].into_iter().flatten() {
            self.see(end.name, file.loc(end.name.0), Role::End);
            for selector in &file[end.select] {
                if let ast::Select::End(name) = selector {
                    self.see(*name, file.loc(name.0), Role::End);
                }
            }
        }
        self.tail(file, flow.tail);
        for leg in &file[flow.body.legs] {
            self.see(leg.end.name, leg.loc, Role::End);
            self.tail(file, leg.tail);
        }
        for item in &file[flow.body.items] {
            self.tail(file, item.tail);
        }
    }

    fn tail(&mut self, file: &ast::File<'s>, clauses: ast::Many<ast::Clause<'s>>) {
        for clause in &file[clauses] {
            match clause.kind {
                ClauseKind::Via(name) => self.see(name, clause.at, Role::End),
                ClauseKind::For(ast::For::Whom(name)) => self.see(name, clause.at, Role::Party),
                ClauseKind::Purpose(purpose) => {
                    if let Some(name) = purpose.of {
                        self.see(name, clause.at, Role::End);
                    }
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use axiom_core::FileId;
    use axiom_syntax::{Folder, parse};

    use super::*;
    use crate::Source;
    use crate::scope::Home;

    /// What the walk says of `text`, as `(name, written as a party)` in the order each name is first written.
    fn walked(text: &str) -> Vec<(String, bool)> {
        let path = "journal/mentions.ax";
        let (file, diagnostics) = parse(FileId(0), text, Folder::of(path));
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let source = Source { path, file, embedded: false };
        let mentions = Mentions::of(&[Site { source: &source, home: Home::Project }]);
        let mut found: Vec<_> = mentions.first.iter().map(|(&name, &loc)| (loc, name)).collect();
        found.sort();
        found.into_iter().map(|(_, name)| (name.to_string(), mentions.parties.contains(name))).collect()
    }

    fn named(found: &[(String, bool)], name: &str) -> Option<bool> {
        found.iter().find(|(written, _)| written == name).map(|&(_, party)| party)
    }

    #[test]
    fn a_claim_a_for_and_a_contracts_with_make_parties_and_every_other_end_is_only_an_end() {
        let found = walked(
            "\
entity fund : institution
  also issuer -> self 2 USD for holder via market
contract lease with landlord
  2_350 USD monthly from checking
  also checking -> escrow 410 USD for recipient via bank
2026-01-03 borrower owes lender 100 USD due 5d for beneficiary via clearing
opening 2026-01-01
  borrower owes lender 20 USD due 3d for opener
  checking 5 USD #rent of flat
",
        );
        for party in ["holder", "landlord", "recipient", "borrower", "lender", "beneficiary", "opener"] {
            assert_eq!(named(&found, party), Some(true), "{party} is written as a party");
        }
        for end in ["issuer", "self", "market", "checking", "escrow", "bank", "clearing", "flat"] {
            assert_eq!(named(&found, end), Some(false), "{end} is written as an end only");
        }
        assert_eq!(found.iter().filter(|(name, _)| name == "borrower").count(), 1, "each name once, however often");
    }

    #[test]
    fn a_contract_with_no_with_is_with_its_own_name_after_every_end_has_had_its_say() {
        let text = "\
contract quill
  50 USD monthly from checking
2026-01-02 checking -> quill 5 USD
contract vera
  70 USD monthly from checking
";
        let found = walked(text);
        assert_eq!(named(&found, "quill"), Some(true));
        assert_eq!(named(&found, "vera"), Some(true));
        let at = |name: &str| found.iter().position(|(written, _)| written == name).unwrap();
        assert!(at("checking") < at("quill"), "`quill` is first written at the flow, not at the contract before it");
        assert!(at("quill") < at("vera"), "`vera` is first written at its contract, which nothing else mentions");
    }
}
