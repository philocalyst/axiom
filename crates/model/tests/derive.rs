//! What a `derive` step compiles to, where its law is kept, and what is refused.

use axiom_core::FileId;
use axiom_model::{Book, Effect, Shape, Sign, Source, Stand, StepKind, Watch, build};
use axiom_syntax::{Folder, parse};

const PRELUDE: &str = "\
base USD
commodity USD
  precision 2
purpose fee : spending
purpose match : transfer
entity lender
entity acme
account checking
account escrow
";

fn lowered(text: &str) -> (Book<'_>, Vec<String>) {
    let path = "contracts.ax";
    let (file, syntax) = parse(FileId(0), text, Folder::of(path));
    assert!(syntax.is_empty(), "{syntax:?}");
    let (book, diagnostics) = build(&[Source { path, file, embedded: false }]);
    (book, diagnostics.into_iter().map(|diagnostic| diagnostic.code.to_string()).collect())
}

fn contract_with(law: &str) -> String {
    format!(
        "{PRELUDE}contract rent with lender\n  1_000 USD monthly on 1 from checking\n  from 2026-01-01\n  law {law}\n"
    )
}

#[test]
fn a_flow_of_its_own_and_an_item_are_templates_beside_a_node_of_the_law() {
    let text = contract_with(
        "extras\n    on flow\n    when value(amount, USD) > 10 USD\n    derive -> escrow 100 USD #match\n    derive + 2% of amount #fee\n    derive - 1 USD\n",
    );
    let (book, codes) = lowered(&text);
    assert!(codes.is_empty(), "{codes:?}");
    let contract = book.contract("rent").unwrap();
    let law = &book.laws[book.rules.at(Watch::Occurrence(contract))[0].law];
    assert!(law.derives());
    let templates: Vec<_> = law
        .steps
        .iter()
        .filter_map(|step| match step.kind {
            StepKind::Effect(Effect::Derive { template, .. }) => Some(book.derived[template]),
            _ => None,
        })
        .collect();
    let escrow = book.place("escrow").unwrap();
    assert_eq!(templates[0].shape, Shape::Flow { from: Stand::Flow, to: Stand::At(escrow) });
    assert_eq!(templates[0].purpose.unwrap().purpose, book.purpose("match").unwrap());
    assert_eq!(templates[1].shape, Shape::Item(Sign::Add));
    assert_eq!(templates[2].shape, Shape::Item(Sign::Less), "a `-` item is taken off the header");
    assert!(templates[2].purpose.is_none());
    assert!(book.rules.at(Watch::Contract(contract)).is_empty(), "a law that derives is not read as a flow posts");
}

#[test]
fn a_law_that_judges_is_read_as_a_flow_posts_and_one_that_derives_as_an_occurrence_is_made() {
    let text = format!(
        "{}  law cap\n    on flow\n    warn value(amount, USD) <= 5_000 USD\n",
        contract_with("escrowed\n    on flow\n    derive -> escrow 100 USD #match\n")
    );
    let (book, codes) = lowered(&text);
    assert!(codes.is_empty(), "{codes:?}");
    let contract = book.contract("rent").unwrap();
    let (judged, derived) = (book.rules.at(Watch::Contract(contract)), book.rules.at(Watch::Occurrence(contract)));
    assert_eq!((judged.len(), derived.len()), (1, 1));
    assert!(!book.laws[judged[0].law].derives() && book.laws[derived[0].law].derives());
}

#[test]
fn a_derive_that_cannot_be_made_is_said_where_it_is_written() {
    let under = |step: &str| format!("{PRELUDE}purpose gifts : spending\n  law gives\n    on flow\n    {step}\n");
    assert!(
        lowered(&under("derive -> escrow 5 USD")).1.is_empty(),
        "a law that is not a contract's derives from a flow that has posted"
    );
    for carved in ["derive 5% of amount #fee", "derive - 1 USD", "derive + 2% of amount"] {
        assert_eq!(
            lowered(&under(carved)).1,
            ["derive-posted"],
            "a flow that has moved cannot give a part of itself away: {carved}"
        );
    }
    let timed = contract_with("monthly\n    each month\n    derive -> escrow 5 USD #match\n");
    assert_eq!(lowered(&timed).1, ["derive-trigger"]);
    let both =
        contract_with("both\n    on flow\n    warn value(amount, USD) <= 5 USD\n    derive -> escrow 5 USD #match\n");
    assert_eq!(lowered(&both).1, ["derive-and-judge"]);
}

#[test]
fn a_contracts_law_may_carve_an_item_out_of_the_occurrence_it_is_made_with() {
    let text = contract_with("share\n    on flow\n    derive 5% of amount #fee\n");
    assert!(lowered(&text).1.is_empty(), "an occurrence is made before it posts, so a part of it can be given away");
}

#[test]
fn a_let_before_a_derive_is_a_step_of_the_law_that_derives() {
    let text = contract_with("fees\n    on flow\n    let fee = amount * 2 / 100\n    derive + fee #fee\n");
    let (book, codes) = lowered(&text);
    assert!(codes.is_empty(), "{codes:?}");
    let contract = book.contract("rent").unwrap();
    let law = &book.laws[book.rules.at(Watch::Occurrence(contract))[0].law];
    assert!(
        matches!(law.steps[0].kind, StepKind::Let(_))
            && matches!(law.steps[1].kind, StepKind::Effect(Effect::Derive { .. }))
    );
}
