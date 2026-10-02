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
