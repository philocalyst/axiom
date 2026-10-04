//! Claim tabs: the places that keep what one party owes another, made when a claim or a loan asks for one.

use axiom_core::{FileId, Id};
use axiom_model::book::Book;
use axiom_model::{Class, Entity, Place, Role, Source, Watch, build};
use axiom_syntax::{Folder, parse};

/// The book's owner, another owner, parties of both kinds, and an account of the other owner's that is `assets/joint`.
const PARTIES: &str = "use std\nbase USD\nentity me : person\nentity pat : person\nentity jo : org\nentity bank : org\n\
account checking : bank\naccount assets/joint : bank\n  owner pat\n";

/// The kinds and the currency a project of one file uses.
const STD: &str = "system std\nkind person : entity\nkind org : entity\nkind bank : asset\nkind currency : commodity\ncommodity USD : currency\n  precision 2\n";

fn source<'s>(id: u16, path: &'s str, text: &'s str, embedded: bool) -> Source<'s> {
    let (file, syntax) = parse(FileId(id), text, Folder::of(path));
    assert!(syntax.is_empty(), "{syntax:?}");
    Source { path, file, embedded }
}

/// What building a project of one file, with `STD`, makes, which must be without a diagnostic but for the note that says a loan
/// was opened from its terms (these books write a loan and no line that originates it).
fn with_book<R>(text: &str, then: impl FnOnce(&Book<'_>) -> R) -> R {
    let sources = [source(0, "std.ax", STD, true), source(1, "journal/2026/01.ax", text, false)];
    let (book, diagnostics) = build(&sources);
    assert!(diagnostics.iter().all(|diagnostic| &*diagnostic.code == "loan-opening"), "{diagnostics:?}");
    then(&book)
}

/// Every tab of the book, in id order, with the party it is of, its owner and its class.
fn tabs(book: &Book<'_>) -> Vec<(Id<Place>, Id<Entity>, Id<Entity>, Class)> {
    let tab = |(id, place): (Id<Place>, &Place)| match place.role {
        Role::Tab(party) => Some((id, party, place.owner, place.class)),
        _ => None,
    };
    book.places.iter().filter_map(tab).collect()
}

/// The book's own tabs for a loan paid from an account `pat` owns, written by its last name only.
const LOAN: &str = "contract mortgage with bank\n  loan 20_000 USD on 2026-01-01 at 4% over 10y\n  \
monthly on 3 from joint\n  from 2026-02-01\n";

#[test]
fn a_loan_paid_from_an_account_another_owner_holds_has_its_debt_tab_of_that_owner() {
    // The account is `assets/joint`, written `joint`: a name that is not its path, and an owner that is not `me`.
    with_book(&format!("{PARTIES}{LOAN}"), |book| {
        let (pat, bank) = (book.entity("pat").unwrap(), book.entity("bank").unwrap());
        let loan = book.contracts[book.contract("mortgage").unwrap()].loan.expect("a loan");

        assert_eq!(book.places[loan.debt].role, Role::Tab(bank));
        assert_eq!((book.places[loan.debt].owner, book.places[loan.debt].class), (pat, Class::Debt));
        assert!(tabs(book).iter().any(|&(id, ..)| id == loan.debt));
    });
}

#[test]
fn a_tab_made_while_the_journal_is_lowered_has_rules_and_a_row_of_facts_like_every_place() {
    let law = "law top\n  on in\n  warn year > 2000 \"late\"\n";
    let text = format!("{PARTIES}{LOAN}2026-01-10 jo owes me 20 USD\n{law}");
    with_book(&text, |book| {
        let debt = book.contracts[book.contract("mortgage").unwrap()].loan.expect("a loan").debt;
        let (claim, ..) = tabs(book).into_iter().find(|&(id, ..)| id != debt).expect("the claim's tab");

        assert!(
            book.places.ids().all(|place| !book.rules.at(Watch::In(place)).is_empty()),
            "rules are worked out once every place exists"
        );
        assert_eq!(book.touching.keys(), book.places.len());
        for tab in [debt, claim] {
            assert!(!book.rules.at(Watch::In(tab)).is_empty(), "the project's law watches a tab as it does any place");
            assert!(
                book.is_claim(tab) && book.holds(tab).is_none(),
                "a tab says nothing of itself: its kind's defaults"
            );
        }
        assert_eq!(book.facts.holders(), book.holders.len(), "the facts have a row for every thing, tabs included");
    });
}

/// What a tab is of: the party's name, whose it is and which way the claim goes.
fn of<'s>(book: &Book<'s>, party: &str, owner: &str, class: Class) -> (Id<Entity>, Id<Entity>, Class) {
    (book.entity(party).unwrap(), book.entity(owner).unwrap(), class)
}

#[test]
fn a_claim_makes_its_tab_at_the_end_of_the_tree_and_a_second_claim_of_the_same_shape_asks_again() {
    let claims =
        "2026-01-10 jo owes me 20 USD due 30d ^c1\n2026-01-20 me owes bank 5 USD\n2026-02-01 jo owes me 8 USD\n";
    with_book(&format!("{PARTIES}{claims}"), |book| {
        let made: Vec<_> = tabs(book).into_iter().map(|(_, party, owner, class)| (party, owner, class)).collect();
        assert_eq!(made, [of(book, "jo", "me", Class::Asset), of(book, "bank", "me", Class::Debt)]);

        let ids: Vec<_> = tabs(book).iter().map(|&(id, ..)| id).collect();
        assert!(
            ids.iter().all(|&tab| book.places.end(tab).index() == tab.index() + 1 && book.places.parent(tab).is_none())
        );
        assert_eq!(ids, [Id::new(book.places.len() as u32 - 2), Id::new(book.places.len() as u32 - 1)]);
        assert_eq!(book.places.roots().last(), ids.last().copied(), "the last root of the tree is the last tab made");
    });
}

#[test]
fn tabs_come_in_the_order_the_journal_is_lowered_in_and_not_the_order_it_is_written_in() {
    let claims = "2026-03-01 me owes bank 5 USD\n2026-01-10 jo owes me 20 USD\n";
    with_book(&format!("{PARTIES}{claims}"), |book| {
        let made: Vec<_> = tabs(book).into_iter().map(|(_, party, owner, class)| (party, owner, class)).collect();
        assert_eq!(made, [of(book, "jo", "me", Class::Asset), of(book, "bank", "me", Class::Debt)]);
    });
}

#[test]
fn a_party_a_flow_mentions_with_due_for_or_via_has_no_tab_until_a_claim_asks_for_one() {
    let flows =
        "2026-01-05 checking -> jo   5 USD for bank due 30d via pat\n2026-01-06 checking <- bank 5 USD for jo\n";
    with_book(&format!("{PARTIES}{flows}"), |book| assert_eq!(tabs(book), []));
}

#[test]
fn a_template_may_name_a_loan_declared_after_it_and_its_name_stands_for_the_debt_tab() {
    let contracts =
        "contract plan with jo\n  100 USD monthly on 2 from checking\n  from 2026-02-01\n  -> mortgage 20 USD\n";
    with_book(&format!("{PARTIES}{contracts}{LOAN}"), |book| {
        let debt = book.contracts[book.contract("mortgage").unwrap()].loan.expect("a loan").debt;
        let plan = &book.contracts[book.contract("plan").unwrap()];
        let template = &plan.terms.as_ref().expect("terms").template[0];
        let leg = &template.legs[0];
        assert!([leg.flow.from, leg.flow.to].contains(&debt), "the leg ends at the loan's debt tab");
        assert_eq!(tabs(book).iter().filter(|&&(id, ..)| id == debt).count(), 1);
    });
}

#[test]
fn a_party_only_a_journal_writes_is_an_entity_and_a_contract_named_for_its_party_is_with_it() {
    let text = "2026-01-02 checking -> zorb 5 USD \"a gift\"\ncontract quill\n  50 USD monthly on 1 from checking\n  from 2026-02-01\n";
    with_book(&format!("{PARTIES}{text}"), |book| {
        assert!(book.entity("zorb").is_ok() && book.entity("quill").is_ok());
        assert_eq!(book.contracts[book.contract("quill").unwrap()].party, book.entity("quill").unwrap());
    });
}

#[test]
fn places_are_listed_in_the_trees_order_and_the_tabs_after_them_by_whom_they_are_with() {
    let claims = "2026-01-10 me owes jo 5 USD\n2026-01-11 jo owes me 20 USD\n2026-01-12 bank owes me 7 USD\n\
2026-01-13 pat owes jo 3 USD\n";
    with_book(&format!("{PARTIES}{claims}"), |book| {
        let made: Vec<_> = tabs(book).into_iter().map(|(_, party, owner, class)| (party, owner, class)).collect();
        assert_eq!(
            made,
            [
                of(book, "jo", "me", Class::Debt),
                of(book, "jo", "me", Class::Asset),
                of(book, "bank", "me", Class::Asset),
                of(book, "jo", "pat", Class::Debt),
            ],
            "the tree has them in the order their claims were recorded"
        );

        let listed = book.listed_places();
        let mut every: Vec<_> = listed.clone();
        every.sort();
        assert_eq!(every, book.places.ids().collect::<Vec<_>>(), "every place is listed once");
        let (declared, after): (Vec<_>, Vec<_>) =
            listed.iter().copied().partition(|&place| !matches!(book.places[place].role, Role::Tab(_)));
        assert!(declared.windows(2).all(|pair| pair[0] < pair[1]), "the places of the tree keep its order");
        assert_eq!(listed[..declared.len()], declared[..], "and come first");
        let by_whom: Vec<_> = after
            .iter()
            .map(|&place| (book.places[place].role, book.places[place].owner, book.places[place].class))
            .collect();
        let wanted = [
            of(book, "bank", "me", Class::Asset),
            of(book, "jo", "me", Class::Asset),
            of(book, "jo", "me", Class::Debt),
            of(book, "jo", "pat", Class::Debt),
        ];
        let expected: Vec<_> = wanted.iter().map(|&(party, owner, class)| (Role::Tab(party), owner, class)).collect();
        assert_eq!(by_whom, expected, "by the party's name, what is owed to the owner first, then by owner");
        assert!(after.iter().all(|&place| book.listing(place) > book.listing(*declared.last().unwrap())));
    });
}

#[test]
fn a_loan_whose_lender_is_the_owner_is_refused_and_makes_no_tab() {
    // The survey never made a tab between a party and itself, so the lowering found none and said `unregistered-tab`:
    // a refusal by accident. It is one by design now.
    let text = format!(
        "{PARTIES}contract mortgage with me\n  loan 20_000 USD on 2026-01-01 at 4% over 10y\n  monthly on 3 from checking\n  from 2026-02-01\n"
    );
    let sources = [source(0, "std.ax", STD, true), source(1, "journal/2026/01.ax", &text, false)];
    let (book, diagnostics) = build(&sources);
    let codes: Vec<_> = diagnostics.iter().map(|diagnostic| diagnostic.code.to_string()).collect();
    assert_eq!(codes, ["contract-loan-party"]);
    assert_eq!(tabs(&book), []);
}

#[test]
fn a_tab_is_a_claim_because_its_kind_says_so_and_not_because_of_its_role() {
    let text = format!("{PARTIES}2026-01-10 jo owes me 20 USD\n2026-01-11 me owes jo 5 USD\n");
    with_book(&text, |book| {
        let kinds = book.roots.kinds;
        let found = tabs(book);
        assert_eq!(found.len(), 2);
        for (tab, _, _, class) in found {
            let kind = if class == Class::Debt { kinds.debt_claim } else { kinds.claim };
            assert_eq!(book.places[tab].kind, kind, "{class:?}");
            assert!(book.is_a(kind, if class == Class::Debt { kinds.debt } else { kinds.asset }));
            assert_eq!(
                book.fact(axiom_model::builtin::CLAIM, tab),
                Some(true),
                "the kind says `claim`, and the tab says nothing"
            );
            assert!(book.is_claim(tab));
        }
        let checking = book.place("checking").unwrap();
        assert!(!book.is_claim(checking), "a place whose kind does not say `claim` is not one");
    });
}
