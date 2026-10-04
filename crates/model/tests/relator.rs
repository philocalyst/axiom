//! What a contract of a kind is: who fills its slots, which of its kind's legs a book has, and what is refused.

use axiom_core::{Diagnostic, FileId, Id};
use axiom_model::{Book, Contract, Effect, Law, Owner, Shape, Source, Stand, StepKind, build};
use axiom_syntax::{Folder, parse};

/// An employment between a person and an employer, whose employer half of the payroll tax is written once.
const PRELUDE: &str = "\
base USD
commodity USD
  precision 2
purpose payroll-tax : spending
purpose wages : income
kind person : entity
kind household : entity
kind org : entity
kind employer : org
kind employment : contract
  has employee person
  has employer employer
  also employer -> irs 7.65% of amount #payroll-tax
entity irs : org
entity acme : employer
entity alex : person
entity me : person
";

fn lowered(text: &str) -> (Book<'_>, Vec<Diagnostic>) {
    let path = "axiom.ax";
    let (file, syntax) = parse(FileId(0), text, Folder::of(path));
    assert!(syntax.is_empty(), "{syntax:?}");
    build(&[Source { path, file, embedded: false }])
}

fn codes(diagnostics: &[Diagnostic]) -> Vec<&str> {
    diagnostics.iter().map(|diagnostic| diagnostic.code.as_ref()).collect()
}

/// The household's book: `me` is paid into `checking`.
fn household(contract: &str) -> String {
    format!("{PRELUDE}account checking\n{contract}")
}

/// The employer's book: `acme` pays from `payroll`, which is its own.
fn employer(contract: &str) -> String {
    format!("{PRELUDE}account payroll\n  owner acme\n{contract}")
}

const PAID: &str =
    "contract pay : employment with acme\n  employee me\n  employer acme\n  5_000 USD monthly on 15 into checking\n";
const PAYS: &str =
    "contract pay : employment with alex\n  employee alex\n  employer acme\n  5_000 USD monthly on 15 from payroll\n";

/// The laws of a contract that its lines abbreviate: what its kind writes, and its own `also`.
fn legs_of<'b>(book: &'b Book<'_>, contract: Id<Contract>) -> Vec<&'b Law> {
    let owner = Owner::Contract(contract);
    book.laws.iter().map(|(_, law)| law).filter(|law| law.owner == owner && book.name(law.name) == "also").collect()
}

/// What the one `derive` step of a leg makes.
fn shape(book: &Book<'_>, law: &Law) -> Shape {
    let derive = law.steps.iter().find_map(|step| match step.kind {
        StepKind::Effect(Effect::Derive { template, .. }) => Some(template),
        _ => None,
    });
    book.derived[derive.expect("a derive step")].shape
}

#[test]
fn the_household_that_pays_an_employment_has_no_employer_half() {
    let text = household(PAID);
    let (book, diagnostics) = lowered(&text);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let contract = book.contract("pay").unwrap();
    assert_eq!(book.contracts[contract].kind, Some(book.kind("employment").unwrap()));
    assert_eq!(book.contracts[contract].fillers.len(), 2);
    assert!(legs_of(&book, contract).is_empty(), "acme to the irs touches nothing of the household's");
}

#[test]
fn the_employer_that_pays_it_has_the_leg_from_its_own_account() {
    let text = employer(PAYS);
    let (book, diagnostics) = lowered(&text);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let contract = book.contract("pay").unwrap();
    let found = legs_of(&book, contract);
    let [leg] = found[..] else { panic!("one leg: {}", found.len()) };
    let (payroll, irs) = (book.place("payroll").unwrap(), book.entities[book.entity("irs").unwrap()].place.unwrap());
    assert_eq!(
        shape(&book, leg),
        Shape::Flow { from: Stand::At(payroll), to: Stand::At(irs) },
        "the employer stands at the account"
    );
}

#[test]
fn a_member_of_the_owner_stands_where_the_owner_does() {
    let text = format!(
        "{PRELUDE}entity family : household\nentity jo : person\n  member family\naccount joint\n  owner family\n\
         contract pay : employment with acme\n  employee jo\n  employer acme\n  5_000 USD monthly on 15 into joint\n"
    )
    .replace(
        "kind employment : contract",
        "kind employment : contract\n  also employee -> irs 1% of amount #payroll-tax",
    );
    let (book, diagnostics) = lowered(&text);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let legs = legs_of(&book, book.contract("pay").unwrap());
    let [leg] = legs[..] else { panic!("the employee's own leg is the household's: {}", legs.len()) };
    let (joint, irs) = (book.place("joint").unwrap(), book.entities[book.entity("irs").unwrap()].place.unwrap());
    assert_eq!(
        shape(&book, leg),
        Shape::Flow { from: Stand::At(joint), to: Stand::At(irs) },
        "jo is a member of the owner of joint"
    );
}

#[test]
fn a_kind_beneath_another_has_its_legs_after_the_other_and_a_leg_that_is_an_item_is_in_every_book() {
    let kinds = "kind salaried : employment\n  also + 1% of amount #payroll-tax\n  also employer -> irs 1% of amount #payroll-tax\n";
    let text = format!("{}{kinds}", employer(PAYS).replace("contract pay : employment", "contract pay : salaried"));
    let (book, diagnostics) = lowered(&text);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let legs = legs_of(&book, book.contract("pay").unwrap());
    let shapes: Vec<_> = legs.iter().map(|leg| matches!(shape(&book, leg), Shape::Flow { .. })).collect();
    assert_eq!(shapes, [true, false, true], "the kind's, then the kind beneath's: a flow, an item, a flow");
    let household =
        format!("{}{kinds}", household(PAID).replace("contract pay : employment", "contract pay : salaried"));
    let (book, _) = lowered(&household);
    let legs = legs_of(&book, book.contract("pay").unwrap());
    assert_eq!(legs.len(), 1, "only the item touches the household's book");
    assert!(matches!(shape(&book, legs[0]), Shape::Item(_)));
}

#[test]
fn a_slot_that_is_not_the_kinds_is_said_with_the_one_it_is_near() {
    let text = household(&PAID.replace("employee me", "employe me"));
    let (_, diagnostics) = lowered(&text);
    assert_eq!(codes(&diagnostics), ["relator-slot-unknown"]);
    assert!(diagnostics[0].message.contains("`employment` has no slot `employe`"), "{diagnostics:?}");
    assert!(diagnostics[0].notes.iter().any(|note| note.contains("`employee` or `employer`")), "{diagnostics:?}");
    assert!(diagnostics[0].help.iter().any(|help| help.edit.is_some()), "did you mean `employee`");
}

#[test]
fn a_slot_filled_twice_names_both_lines() {
    let text = household(&PAID.replace("  employer acme\n", "  employer acme\n  employer acme\n"));
    let (_, diagnostics) = lowered(&text);
    assert_eq!(codes(&diagnostics), ["relator-slot-twice"]);
    assert_eq!(diagnostics[0].labels.len(), 2, "the second line, and the first");
}

#[test]
fn an_entity_of_the_wrong_kind_cannot_fill_a_slot() {
    let text = household(&PAID.replace("employer acme", "employer alex"));
    let (_, diagnostics) = lowered(&text);
    assert_eq!(codes(&diagnostics), ["relator-slot-kind"]);
    assert!(diagnostics[0].labels.iter().any(|label| label.text.contains("a person")), "{diagnostics:?}");
}

#[test]
fn a_slot_nothing_fills_is_said_with_the_line_to_write() {
    let text = household(&PAID.replace("  employer acme\n", ""));
    let (book, diagnostics) = lowered(&text);
    assert_eq!(codes(&diagnostics), ["relator-slot-missing"]);
    assert!(diagnostics[0].help.iter().any(|help| help.text.contains("employer NAME")), "{diagnostics:?}");
    let contract = book.contract("pay").unwrap();
    assert!(
        book.contracts[contract].terms.is_some(),
        "and the contract is still lowered, so its occurrences are not errors"
    );
    assert_eq!(book.contracts[contract].kind, None);
}

#[test]
fn a_kind_that_is_not_of_contracts_is_refused_with_the_ones_there_are() {
    let text = household(&PAID.replace(": employment", ": person"));
    let (_, diagnostics) = lowered(&text);
    assert_eq!(codes(&diagnostics), ["relator-kind-sort"]);
    assert!(diagnostics[0].help.iter().any(|help| help.text.contains("`employment`")), "{diagnostics:?}");
}

#[test]
fn an_owner_that_is_not_the_contracts_owner_does_not_stand_at_its_account() {
    // `me` is an owner of the book (every book has one), and `payroll` is `acme`'s.
    let text = employer(&PAYS.replace("employee alex", "employee me").replace("with alex", "with me"));
    let (_, diagnostics) = lowered(&text);
    assert_eq!(codes(&diagnostics), ["relator-position"]);
    assert!(diagnostics[0].notes.iter().any(|note| note.contains("`acme` and its members")), "{diagnostics:?}");
}

#[test]
fn a_leg_that_names_a_role_the_contract_leaves_empty_says_so() {
    let kind = "kind lease : contract\n  has tenant person\n  has agent org optional\n  also agent -> irs 1% of amount #payroll-tax\n";
    let text = format!(
        "{PRELUDE}{kind}account checking\ncontract flat : lease with acme\n  tenant me\n  5_000 USD monthly on 1 from checking\n"
    );
    let (_, diagnostics) = lowered(&text);
    assert_eq!(codes(&diagnostics), ["relator-role-empty"]);
    assert!(diagnostics[0].message.contains("nothing fills `agent`"), "{diagnostics:?}");
}

#[test]
fn a_kind_of_contract_takes_no_laws_of_its_own() {
    let text = format!("{PRELUDE}kind lease : contract\n  law late\n    on flow\n    warn amount > 1 USD\n");
    let (_, diagnostics) = lowered(&text);
    assert_eq!(codes(&diagnostics), ["law-position"]);
}
