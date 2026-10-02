//! Claim tabs: the places that keep what one party owes another, made when a claim or a loan asks for one.

use axiom_core::{FileId, Id};
use axiom_model::book::Book;
use axiom_model::{Class, Entity, Place, Role, Source, build};
use axiom_syntax::{Folder, parse};

/// The book's owner, another owner, parties of both kinds, and an account of the other owner's that is `assets/joint`.
const PARTIES: &str = "use std\nbase USD\nentity me : person\nentity pat : person\nentity jo : org\nentity bank : org\n\
account checking : bank\naccount assets/joint : bank\n  owner pat\n";

/// The kinds and the currency a project of one file uses.
const STD: &str = "system std\nkind person : entity\nkind org : entity\nkind bank : asset\nkind currency : commodity\ncommodity USD : currency\n  precision 2\n";

/// What building a project of one file, with `STD`, makes, which must be without a diagnostic.
fn with_book<R>(text: &str, then: impl FnOnce(&Book<'_>) -> R) -> R {
    let source = |id, path, text, embedded| {
        let (file, syntax) = parse(FileId(id), text, Folder::of(path));
        assert!(syntax.is_empty(), "{syntax:?}");
        Source { path, file, embedded }
    };
    let sources = [source(0, "std.ax", STD, true), source(1, "journal/2026/01.ax", text, false)];
    let (book, diagnostics) = build(&sources);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
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
fn a_tab_made_while_the_book_is_lowered_has_rules_and_a_row_of_facts_like_every_place() {
    let text = format!("{PARTIES}{LOAN}law top\n  on in\n  warn year > 2000 \"late\"\n");
    with_book(&text, |book| {
        let debt = book.contracts[book.contract("mortgage").unwrap()].loan.expect("a loan").debt;

        assert_eq!(book.rules.on_in.keys(), book.places.len(), "rules are worked out once every place exists");
        assert_eq!(book.touching.keys(), book.places.len());
        assert!(!book.rules.on_in[debt].is_empty(), "the project's law watches the tab as it does any place");
        assert_eq!(book.facts.holders(), book.holders.len(), "the facts have a row for every thing, tabs included");
        assert!(book.is_claim(debt) && book.holds(debt).is_none(), "a tab says nothing of itself: its kind's defaults");
    });
}
