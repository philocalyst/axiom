//! What a name stands for when more than one thing answers to it, and what is said when none does: built from
//! whole sources, as a user would write them.

use axiom_core::{Diagnostic, FileId};
use axiom_syntax::{Folder, parse};

use crate::{Source, build};

const STD: &str = "\
system std
kind bank : asset
kind person : entity
kind currency : commodity
";

fn build_project(project: &str) -> (crate::Book<'_>, Vec<Diagnostic>) {
    let (std, diagnostics) = parse(FileId(0), STD, Folder::default());
    assert!(diagnostics.is_empty(), "std should parse: {diagnostics:?}");
    let (file, diagnostics) = parse(FileId(1), project, Folder::default());
    assert!(diagnostics.is_empty(), "project should parse: {diagnostics:?}");
    build(&[Source { path: "std.ax", file: std, embedded: true }, Source { path: "axiom.ax", file, embedded: false }])
}

fn codes(diagnostics: &[Diagnostic]) -> Vec<&str> {
    diagnostics.iter().map(|diagnostic| diagnostic.code.as_ref()).collect()
}

#[test]
fn account_suffix_and_entity_name_are_reported_at_the_later_declaration() {
    let project = "\
use std
base USD
commodity USD : currency
account assets/lantern : bank
entity lantern : person
";
    let (_, diagnostics) = build_project(project);

    assert_eq!(codes(&diagnostics), ["ambiguous-name"], "{diagnostics:?}");
    let diagnostic = &diagnostics[0];
    assert_eq!(diagnostic.labels.len(), 2);
    assert!(diagnostic.labels[0].primary);
    assert_eq!(&project[diagnostic.labels[0].loc.start as usize..diagnostic.labels[0].loc.end as usize], "lantern");
    assert!(!diagnostic.labels[1].primary);
    assert_eq!(
        &project[diagnostic.labels[1].loc.start as usize..diagnostic.labels[1].loc.end as usize],
        "assets/lantern"
    );
}

#[test]
fn endpoint_lookup_does_not_let_a_place_suffix_hide_an_entity() {
    let project = "\
use std
base USD
commodity USD : currency
account bank/checking : bank
entity checking : person
2026-01-01 checking -> bank/checking 1 USD
";
    let (_, diagnostics) = build_project(project);

    assert!(diagnostics.iter().any(|diagnostic| diagnostic.code == "ambiguous-name"), "{diagnostics:?}");
    let endpoint = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "ambiguous-end")
        .expect("endpoint lookup should preserve both namespace candidates");
    assert!(endpoint.message.contains("account `bank/checking`"), "{endpoint:?}");
    assert!(endpoint.message.contains("entity `checking`"), "{endpoint:?}");
    assert_eq!(endpoint.labels.len(), 3, "use site plus both declarations: {endpoint:?}");
}

#[test]
fn contract_can_share_its_party_name_but_not_an_unrelated_entity_name() {
    let matching = "\
use std
base USD
commodity USD : currency
account checking : bank
contract lantern with lantern
  1 USD monthly from checking
entity lantern : person
";
    let (_, diagnostics) = build_project(matching);
    assert!(!diagnostics.iter().any(|diagnostic| diagnostic.code == "ambiguous-name"), "{diagnostics:?}");

    let unrelated = "\
use std
base USD
commodity USD : currency
account checking : bank
contract lantern with landlord
  1 USD monthly from checking
entity lantern : person
entity landlord : person
";
    let (_, diagnostics) = build_project(unrelated);
    assert_eq!(codes(&diagnostics), ["ambiguous-name"], "{diagnostics:?}");
}

#[test]
fn an_ambiguous_owner_says_where_each_candidate_is_declared_and_how_to_write_only_it() {
    let project = "\
use std
base USD
commodity USD : currency
entity x/bob : person
entity y/bob : person
account checking : bank
  owner bob
";
    let (_, diagnostics) = build_project(project);

    let owner = diagnostics.iter().find(|diagnostic| diagnostic.code == "ambiguous-owner").expect("bob is ambiguous");
    assert_eq!(owner.message, "`bob` could be either of these owners");
    let written: Vec<_> =
        owner.help.iter().filter_map(|help| help.edit.as_ref()).map(|(_, text)| text.as_str()).collect();
    assert_eq!(written, ["x/bob", "y/bob"]);
    assert_eq!(owner.labels.len(), 3, "the use and both declarations: {owner:?}");
}

#[test]
fn a_kind_a_system_declares_is_offered_when_the_project_has_not_used_it() {
    let std = parse(FileId(0), STD, Folder::default()).0;
    let extra = parse(FileId(1), "system extra\nkind gadget : asset\n", Folder::default()).0;
    let project = "use std\nbase USD\ncommodity USD : currency\naccount checking : gadget\n";
    let file = parse(FileId(2), project, Folder::default()).0;
    let sources = [
        Source { path: "std.ax", file: std, embedded: true },
        Source { path: "extra.ax", file: extra, embedded: true },
        Source { path: "axiom.ax", file, embedded: false },
    ];

    let (_, diagnostics) = build(&sources);

    let unknown = diagnostics.iter().find(|diagnostic| diagnostic.code == "unknown-kind").expect("gadget is not used");
    assert_eq!(unknown.notes, ["the kind `gadget` is declared by system `extra`, which is not used here"]);
    assert_eq!(unknown.help[0].text, "add `use extra` to bring it into scope");
}
