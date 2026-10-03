//! Addresses: an account written as the entities that fill its slots and then its name, and a reference that is any run
//! of those words that means one account open on the line's day.

use axiom_core::{Day, FileId, Id};
use axiom_model::book::Book;
use axiom_model::{Entity, Holder, Miss, Place, Role, Source, Value, build};
use axiom_syntax::{Folder, parse};

const STD: &str = "\
system std
kind person : entity
kind household : entity
kind org : entity
kind employer : org
kind deposit : asset
kind 401k : asset
  has employer employer optional
kind 529 : asset
  has beneficiary person
kind currency : commodity
commodity USD : currency
  precision 2
";

/// The entities every book below has.
const PEOPLE: &str = "use std\nbase USD\nentity me : person\nentity jordan : person\nentity riley : person\n\
entity family : household\nentity acme : employer\nentity bluefin : employer\nentity fidelity : org\n";

fn source<'s>(id: u16, path: &'s str, text: &'s str, embedded: bool) -> Source<'s> {
    let (file, syntax) = parse(FileId(id), text, Folder::of(path));
    assert!(syntax.is_empty(), "{syntax:?}");
    Source { path, file, embedded }
}

/// What building `PEOPLE` and then `text` makes, and what it says is wrong.
fn built<R>(text: &str, then: impl FnOnce(&Book<'_>, &[axiom_core::Diagnostic]) -> R) -> R {
    let text = format!("{PEOPLE}{text}");
    let sources = [source(0, "std.ax", STD, true), source(1, "journal/2026/01.ax", &text, false)];
    let (book, diagnostics) = build(&sources);
    then(&book, &diagnostics)
}

fn clean<R>(text: &str, then: impl FnOnce(&Book<'_>) -> R) -> R {
    built(text, |book, diagnostics| {
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
        then(book)
    })
}

fn codes(diagnostics: &[axiom_core::Diagnostic]) -> Vec<&str> {
    diagnostics.iter().map(|diagnostic| &*diagnostic.code).collect()
}

fn entity(book: &Book<'_>, name: &str) -> Id<Entity> {
    book.entity(name).unwrap()
}

/// What an account says of one of its slots.
fn said(book: &Book<'_>, place: Id<Place>, slot: &str) -> Option<Value> {
    book.said(Holder::Place(place), book.names.get(slot)?, Day::MIN)
}

#[test]
fn the_words_before_the_name_fill_the_slots_they_fit() {
    clean("account jordan/bluefin/401k\n", |book| {
        let place = book.place("jordan/bluefin/401k").unwrap();
        let account = &book.places[place];
        assert_eq!(account.owner, entity(book, "jordan"), "a person fits no slot but the owner");
        assert_eq!(said(book, place, "employer"), Some(Value::Entity(entity(book, "bluefin"))));
        assert_eq!(book.name(book.kinds[account.kind].name), "401k", "the name is a kind, so the account is of it");
        assert!(book.is_spelled(place));
    });
}

#[test]
fn a_spelled_account_is_a_root_with_no_place_for_the_words_before_its_name() {
    clean("account family/checking : deposit\naccount family/savings : deposit\n", |book| {
        let checking = book.place("family/checking").unwrap();
        assert_eq!(book.places.parent(checking), None);
        assert_eq!(book.places.depth(checking), 0);
        let family = entity(book, "family");
        assert_eq!(book.places[checking].owner, family);
        // `family` is the party that owns it, and holds what it owns: its own place is a holding, not a group of accounts.
        assert_eq!(book.places[book.entities[family].place.unwrap()].role, Role::Holding(family));
        assert_eq!(book.place("family").unwrap(), book.entities[family].place.unwrap());
    });
}

#[test]
fn a_path_whose_words_are_not_entities_is_a_tree_as_it_was() {
    clean("account assets/bank/checking : deposit\n", |book| {
        let place = book.place("assets/bank/checking").unwrap();
        assert!(!book.is_spelled(place));
        assert_eq!(book.places.depth(place), 2);
        assert_eq!(book.places[place].owner, entity(book, "me"));
    });
}

#[test]
fn a_word_that_fits_two_slots_is_placed_where_the_other_words_leave_it() {
    // `bluefin` fits the owner and the employer, `jordan` only the owner: jordan takes it, bluefin is the employer.
    clean("account bluefin/jordan/401k\n", |book| {
        let place = book.place("bluefin/jordan/401k").unwrap();
        assert_eq!(book.places[place].owner, entity(book, "jordan"));
        assert_eq!(said(book, place, "employer"), Some(Value::Entity(entity(book, "bluefin"))));
    });
    // `family` is no person, so it is the owner and `riley` the beneficiary of the 529.
    clean("account family/riley/529\n", |book| {
        let place = book.place("family/riley/529").unwrap();
        assert_eq!(book.places[place].owner, entity(book, "family"));
        assert_eq!(said(book, place, "beneficiary"), Some(Value::Entity(entity(book, "riley"))));
    });
}

#[test]
fn a_role_line_settles_what_a_word_could_fill_two_ways() {
    // Rung 3 of the ladder: `employer bluefin` says what the path would have left open.
    clean("account jordan/401k\n  employer bluefin\n", |book| {
        let place = book.place("jordan/401k").unwrap();
        assert_eq!(book.places[place].owner, entity(book, "jordan"));
        assert_eq!(said(book, place, "employer"), Some(Value::Entity(entity(book, "bluefin"))));
    });
}

#[test]
fn two_words_that_could_each_fill_two_slots_are_ambiguous_with_the_role_that_settles_it() {
    built("account bluefin/acme/401k\n", |_, diagnostics| {
        assert_eq!(codes(diagnostics), ["ambiguous-placement", "ambiguous-placement"], "{diagnostics:#?}");
        let first = &diagnostics[0];
        assert_eq!(first.message, "`bluefin` could fill `owner` or `employer` of `bluefin/acme/401k`");
        let edits: Vec<&str> = first.help.iter().map(|help| help.edit.as_ref().unwrap().1.as_str()).collect();
        assert_eq!(edits, ["acme/401k\n  owner bluefin", "acme/401k\n  employer bluefin"]);
    });
}

#[test]
fn a_word_that_fits_no_free_slot_and_words_that_cannot_all_be_placed_are_said() {
    // The owner is given by a line, and riley is no employer: nothing is left for the word.
    built("account riley/401k\n  owner jordan\n", |_, diagnostics| {
        assert_eq!(codes(diagnostics), ["wrong-kind"], "{diagnostics:#?}");
        assert_eq!(diagnostics[0].message, "`riley` fits no slot of `riley/401k` that is still free");
    });
    // Two people, and a 401(k) takes one of them: both can only be the owner.
    built("account jordan/riley/401k\n", |_, diagnostics| {
        assert_eq!(codes(diagnostics), ["too-many"], "{diagnostics:#?}");
        assert_eq!(diagnostics[0].notes[0], "`jordan` fits `owner`; `riley` fits `owner`");
    });
}

#[test]
fn a_slot_a_word_fills_is_not_called_missing_and_one_nothing_fills_still_is() {
    clean("account family/riley/529\n", |_| ());
    built("account family/529\n", |_, diagnostics| {
        assert_eq!(codes(diagnostics), ["missing-role"], "family is the owner, and nobody is the beneficiary");
    });
    built("account riley/529\n", |_, diagnostics| {
        assert_eq!(
            codes(diagnostics),
            ["ambiguous-placement"],
            "a person is the owner or the beneficiary: no other word says which"
        );
    });
}

#[test]
fn the_old_spelling_keeps_working_and_has_an_address_too() {
    let text = "account jordan-401k : 401k at fidelity\n  owner jordan\n  employer bluefin\n\
account me-checking : deposit\nopening 2026-01-01\n  me-checking 10 USD\n2026-01-02 me-checking -> jordan-401k 5 USD\n";
    clean(text, |book| {
        let place = book.place("jordan-401k").unwrap();
        assert!(!book.is_spelled(place));
        // Its fillers are in its address, so it is reached by them, and by the name alone as ever.
        assert_eq!(book.place("bluefin/jordan-401k").unwrap(), place);
        assert_eq!(book.place("jordan/fidelity/jordan-401k").unwrap(), place);
        assert_eq!(book.place("fidelity/jordan-401k").unwrap(), place);
        assert!(matches!(book.place("fidelity/bluefin/jordan-401k"), Err(Miss::Unknown { .. })), "out of order");
    });
}

#[test]
fn a_reference_that_is_a_run_of_the_address_means_the_account() {
    let text = "account jordan/bluefin/401k at fidelity\naccount me/acme/401k at fidelity\n\
opening 2026-01-01\n  jordan/bluefin/401k 10 USD\n  me/acme/401k 10 USD\n\
2026-01-02 jordan/bluefin/401k -> me/401k 1 USD\n2026-01-03 jordan/401k -> acme/401k 1 USD\n\
2026-01-04 bluefin/401k -> me/acme/fidelity/401k 1 USD\n";
    clean(text, |book| {
        let (jordan, me) = (book.place("jordan/bluefin/401k").unwrap(), book.place("me/acme/401k").unwrap());
        let ends: Vec<_> = book.flows.values().map(|flow| (flow.from, flow.to)).collect();
        assert!(
            ends.contains(&(jordan, me)) && ends.iter().filter(|&&end| end == (jordan, me)).count() == 3,
            "{ends:?}"
        );
    });
}

#[test]
fn a_reference_that_means_several_accounts_says_each_with_the_shortest_address_that_means_only_it() {
    let text = "account jordan/bluefin/401k at fidelity\naccount me/acme/401k at fidelity\n\
2026-01-02 me/acme/401k -> fidelity/401k 1 USD\n";
    built(text, |_, diagnostics| {
        let error = diagnostics.iter().find(|diagnostic| diagnostic.code == "ambiguous-address").expect("said");
        assert_eq!(error.message, "`fidelity/401k` could be either of these addresses");
        let writes: Vec<&str> = error.help.iter().map(|help| help.edit.as_ref().unwrap().1.as_str()).collect();
        assert_eq!(writes, ["jordan/401k", "me/401k"]);
        assert!(error.help[0].text.contains("jordan/bluefin/fidelity/401k"), "{:?}", error.help[0]);
    });
}

#[test]
fn nothing_with_the_address_is_unknown_with_the_closest_name_and_never_a_party() {
    let text = "account jordan/bluefin/401k at fidelity\n2026-01-02 jordan/bluefin/401k -> jordan/40lk 1 USD\n";
    built(text, |book, diagnostics| {
        assert_eq!(codes(diagnostics).iter().filter(|&&code| code == "unknown-address").count(), 1, "{diagnostics:#?}");
        let error = diagnostics.iter().find(|diagnostic| diagnostic.code == "unknown-address").unwrap();
        assert_eq!(error.message, "there is no address `jordan/40lk`");
        assert_eq!(error.help[0].edit.as_ref().unwrap().1, "jordan/401k");
        assert!(book.entity("jordan/40lk").is_err(), "a mistyped address is no party");
    });
}

#[test]
fn a_path_that_ends_in_an_accounts_name_is_an_address_attempt_and_makes_no_party_whatever_its_first_word() {
    // `jordanq` is no entity, but `401k` is an account's name: the path was meant as an address, and a party named
    // `jordanq/401k` would also be known as `401k`, and make the name ambiguous for every line that uses it.
    let text = "account jordan/bluefin/401k at fidelity\naccount family/checking : deposit\n\
2026-01-02 family/checking -> jordanq/401k 1 USD\n2026-01-03 family/checking -> 401k 1 USD\n";
    built(text, |book, diagnostics| {
        let said: Vec<&str> = codes(diagnostics).into_iter().filter(|&code| code != "flow-shape").collect();
        assert_eq!(said, ["unknown-address"], "{diagnostics:#?}");
        assert!(book.entity("jordanq/401k").is_err());
    });
}

#[test]
fn a_path_that_no_address_could_be_is_a_party_the_journal_makes_as_it_always_was() {
    // `fidelity` fills no slot (the account is not at it) and `hq` is no account's name: an implied party, no mistake.
    let text = "account family/checking : deposit\n2026-01-02 family/checking -> fidelity/hq 1 USD\n";
    clean(text, |book| assert!(book.entity("fidelity/hq").is_ok()));
}

#[test]
fn a_sibling_that_opens_later_does_not_make_the_earlier_lines_ambiguous() {
    let accounts = "account family/checking : deposit\naccount jordan/401k\n  employer bluefin\n\
account me/401k\n  opened 2026-06-01\nopening 2026-01-01\n  family/checking 100 USD\n";
    // On 01-02 only jordan's 401(k) is open, so the bare name means it: nothing is said.
    built(&format!("{accounts}2026-01-02 family/checking -> 401k 1 USD\n"), |_, diagnostics| {
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    });
    // From 06-01 there are two, and the name is ambiguous that day and on no day before.
    let later =
        format!("{accounts}2026-01-02 family/checking -> 401k 1 USD\n2026-07-01 family/checking -> 401k 1 USD\n");
    built(&later, |_, diagnostics| {
        let ambiguous: Vec<_> =
            diagnostics.iter().filter(|diagnostic| diagnostic.code == "ambiguous-address").collect();
        assert_eq!(ambiguous.len(), 1, "{diagnostics:#?}");
        assert!(ambiguous[0].notes[0].contains("2026-07-01"), "{:?}", ambiguous[0].notes);
    });
}

#[test]
fn an_account_is_not_found_before_it_opens_or_after_it_closes_and_the_diagnostic_says_so() {
    let accounts = "account family/checking : deposit\naccount jordan/401k\n  employer bluefin\n  opened 2026-06-01\n  closed 2026-08-31\n";
    for (day, found) in [("2026-05-31", false), ("2026-06-01", true), ("2026-08-31", true), ("2026-09-01", false)] {
        built(&format!("{accounts}{day} family/checking -> bluefin/401k 1 USD\n"), |_, diagnostics| {
            let said: Vec<&str> = codes(diagnostics).into_iter().filter(|&code| code != "flow-shape").collect();
            assert_eq!(said.is_empty(), found, "{day}: {diagnostics:#?}");
            if !found {
                assert_eq!(said, ["unknown-address"]);
                let note = diagnostics.iter().find(|d| d.code == "unknown-address").unwrap().notes[0].clone();
                assert_eq!(note, format!("`jordan/bluefin/401k` is not open on {day}"));
            }
        });
    }
}

#[test]
fn a_name_that_resolved_before_means_the_same_account() {
    // `checking` is the one account of that name by suffix; the index is not asked, and nothing declared later changes it.
    let text = "account family/checking : deposit\nopening 2026-01-01\n  checking 5 USD\n";
    clean(text, |book| {
        assert_eq!(book.place("checking").unwrap(), book.place("family/checking").unwrap());
    });
}
