//! What an `also` or a `derive` written under anything but a contract is, and which row of the rules a flow that has posted
//! finds it in (K6b): one law, owned by what it is written under, read as a flow posts.

use axiom_core::{FileId, Id};
use axiom_model::{Book, Effect, Law, Owner, Shape, Source, Stand, StepKind, Table, Watch, build};
use axiom_syntax::{Folder, parse};

/// The lines written under each declaration of a book: what a test says, and nothing else is.
#[derive(Default)]
struct Under {
    rewards: &'static str,
    processor: &'static str,
    stripe: &'static str,
    checking: &'static str,
    visa: &'static str,
    rebate: &'static str,
    house: &'static str,
    contract: &'static str,
}

impl Under {
    fn book(&self) -> String {
        let Under { rewards, processor, stripe, checking, visa, rebate, house, contract } = self;
        format!(
            "base USD
commodity USD
  precision 2
kind bank : asset
kind rewards : debt
{rewards}kind processor : entity
{processor}kind home : thing
purpose fee : spending
purpose rebate : income
{rebate}entity me
entity issuer
entity shop
entity stripe : processor
{stripe}account checking : bank
{checking}account visa : rewards
{visa}asset house : home
{house}{contract}"
        )
    }
}

fn lowered(text: &str) -> (Book<'_>, Vec<String>) {
    let path = "axiom.ax";
    let (file, syntax) = parse(FileId(0), text, Folder::of(path));
    assert!(syntax.is_empty(), "{syntax:?}");
    let (book, diagnostics) = build(&[Source { path, file, embedded: false }]);
    (book, diagnostics.into_iter().map(|diagnostic| diagnostic.code.to_string()).collect())
}

/// The one law the book has that is owned by `owner`, which derives.
fn law_of(book: &Book, owner: Owner) -> Id<Law> {
    let owned: Vec<_> = book.laws.iter().filter(|(_, law)| law.owner == owner).map(|(id, _)| id).collect();
    assert_eq!(owned.len(), 1, "one law is written under it");
    assert!(book.laws[owned[0]].derives());
    owned[0]
}

fn watched_by(book: &Book, watch: Watch, law: Id<Law>) -> bool {
    book.rules.at(watch).iter().any(|rule| rule.law == law)
}

#[test]
fn an_also_under_an_account_a_kind_or_an_entity_is_a_law_watching_the_flows_at_its_places() {
    let text = Under {
        rewards: "  also issuer -> self 2% of amount #rebate\n",
        stripe: "  also - 3% of amount #fee\n",
        processor: "  also + 1% of amount #fee\n",
        checking: "  also + 1 USD #fee\n",
        ..Under::default()
    }
    .book();
    let (book, codes) = lowered(&text);
    assert!(codes.is_empty(), "{codes:?}");
    let place = |name| book.place(name).unwrap();
    let stripe = book.entity("stripe").unwrap();
    let outside = book.entities[stripe].place.unwrap();

    let of_kind = law_of(&book, Owner::Kind(book.kind("rewards").unwrap()));
    assert!(watched_by(&book, Watch::Touching(place("visa")), of_kind), "a place of the kind");
    assert!(!watched_by(&book, Watch::Touching(place("checking")), of_kind), "and no other");

    let of_entity = law_of(&book, Owner::Entity(stripe));
    assert!(watched_by(&book, Watch::Touching(outside), of_entity), "the entity's own place is where its flows are");

    let of_entity_kind = law_of(&book, Owner::Kind(book.kind("processor").unwrap()));
    assert!(watched_by(&book, Watch::Touching(outside), of_entity_kind), "so is the place of an entity of its kind");
    assert!(!watched_by(&book, Watch::Touching(place("visa")), of_entity_kind));

    let of_account = law_of(&book, Owner::Place(place("checking")));
    assert!(watched_by(&book, Watch::Touching(place("checking")), of_account));
    assert!(book.rules.table(Table::Touching).len() >= 4);
}

#[test]
fn an_also_under_a_purpose_or_an_asset_is_read_where_the_flows_that_are_for_them_are() {
    let text =
        Under { rebate: "  also - 10% of amount #fee\n", house: "  also + 1% of amount #fee\n", ..Under::default() }
            .book();
    let (book, codes) = lowered(&text);
    assert!(codes.is_empty(), "{codes:?}");
    let rebate = book.purpose("rebate").unwrap();
    let of_purpose = law_of(&book, Owner::Purpose(rebate));
    assert!(watched_by(&book, Watch::Purpose(rebate), of_purpose));
    assert!(!watched_by(&book, Watch::Purpose(book.purpose("fee").unwrap()), of_purpose));
    let asset = book.asset("house").unwrap();
    let of_asset = law_of(&book, Owner::Asset(asset));
    assert!(watched_by(&book, Watch::About(book.assets[asset].place), of_asset));
}

#[test]
fn self_is_the_end_the_law_governs_in_a_law_about_a_place_and_the_flows_own_in_a_contracts() {
    let text = Under {
        rewards: "  also issuer -> self 2% of amount #rebate\n",
        contract: "contract rent with shop\n  1_000 USD monthly on 1 from checking\n  from 2026-01-01\n  also issuer -> self 2% of amount #rebate\n",
        ..Under::default()
    }
    .book();
    let (book, codes) = lowered(&text);
    assert!(codes.is_empty(), "{codes:?}");
    let ends = |law: Id<Law>| {
        let StepKind::Effect(Effect::Derive { template, .. }) = book.laws[law].steps[0].kind else {
            panic!("a law that derives")
        };
        book.derived[template].shape
    };
    let issuer = book.entities[book.entity("issuer").unwrap()].place.unwrap();
    let kind = law_of(&book, Owner::Kind(book.kind("rewards").unwrap()));
    assert_eq!(ends(kind), Shape::Flow { from: Stand::At(issuer), to: Stand::Subject });
    let contract = law_of(&book, Owner::Contract(book.contract("rent").unwrap()));
    assert_eq!(ends(contract), Shape::Flow { from: Stand::At(issuer), to: Stand::Flow }, "positional, as K6 built it");
}

#[test]
fn a_hand_written_law_on_flow_is_valid_under_an_account_and_either_judges_or_derives() {
    let text = Under {
        checking: "  law big\n    on flow\n    warn value(amount, USD) <= 500 USD\n",
        visa: "  law cash-back\n    on flow\n    derive issuer -> self 2% of amount #rebate\n",
        ..Under::default()
    }
    .book();
    let (book, codes) = lowered(&text);
    assert!(codes.is_empty(), "{codes:?}");
    let checking = book.place("checking").unwrap();
    let rule = book.rules.at(Watch::Touching(checking))[0];
    assert!(!book.laws[rule.law].derives(), "a law that judges a flow at an account is read as it posts");
    let visa = book.place("visa").unwrap();
    assert!(book.laws[book.rules.at(Watch::Touching(visa))[0].law].derives());
}

#[test]
fn a_selector_needs_the_flows_of_an_occurrence_to_select_from() {
    let text =
        Under { rewards: "  also issuer -> self 2% of ([visa] up to 10% of amount) #rebate\n", ..Under::default() }
            .book();
    assert_eq!(lowered(&text).1, ["selector-owner"]);
}

#[test]
fn a_law_nothing_can_reach_is_said_not_silent() {
    let text = format!("{}kind boss : entity\n  also + 5% of amount #fee\n", Under::default().book());
    assert_eq!(lowered(&text).1, ["law-never-fires"], "no entity is a boss");
}

#[test]
fn an_amount_of_any_commodity_says_what_to_write_where_one_commodity_is_counted() {
    let counted = "  law counted\n    on flow\n    count amount as fees\n";
    let text = Under { stripe: counted, ..Under::default() }.book();
    let path = "axiom.ax";
    let (file, syntax) = parse(FileId(0), &text, Folder::of(path));
    assert!(syntax.is_empty(), "{syntax:?}");
    let (_, diagnostics) = build(&[Source { path, file, embedded: false }]);
    let said = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "type-mismatch")
        .expect("a flow at a party moves any commodity");
    assert_eq!(said.message, "expected an amount of USD, but this amount may be of any commodity");
    assert!(said.help.iter().any(|help| help.text.contains("value(amount, USD)")), "{said:?}");
}
