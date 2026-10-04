//! A loan made before its book began opens its debt with what its schedule says is owed when the book begins.
//!
//! The loan is the 36-month one of `loans.rs`, made a year earlier: 120,000.00 at 6% on 2025-01-15, paid on the 1st from 2025-02-01,
//! so that 11 payments (the last on 2025-12-01) are behind a book whose first fact is on 2026-01-01. After them 85,591.44 is owed
//! (the numbers of `docs/v5/measure/loans.py`, whose `pre` books hold the whole of this to an independent reference).

use axiom_core::{Day, Diagnostic, FileId, Severity};
use axiom_model::{Book, Derivation, Flow, Mode, Origin, Source, build};
use axiom_syntax::{Folder, parse};

const PRELUDE: &str = "\
base USD
commodity USD
  precision 2
purpose interest : spending
purpose principal : transfer
kind property : thing
asset condo : property
param idx
  2020-01-01 2%
  2026-01-01 5.5%
entity bank
account checking : asset
";

const LOAN: &str = "\
contract home-loan with bank
  loan 120_000 USD on 2025-01-15 at 6% over 3y for condo
  monthly on 1 from checking
  from 2025-02-01
";

/// A book that begins on 2026-01-01 with what is held then.
const OPENING: &str = "opening 2026-01-01\n  checking 900_000 USD\n";

/// What is owed after the 11 payments due from 2025-02-01 to 2025-12-01.
const OWED: i64 = 8_559_144;

fn day(year: i32, month: u32, date: u32) -> Day {
    Day::from_ymd(year, month, date).unwrap()
}

fn built(text: &str) -> (Book<'static>, Vec<Diagnostic>) {
    let text: &'static str = Box::leak(format!("{PRELUDE}{text}").into_boxed_str());
    let (file, syntax) = parse(FileId(0), text, Folder::of("main.ax"));
    assert!(syntax.is_empty(), "{syntax:?}");
    build(&[Source { path: "main.ax", file, embedded: false }])
}

/// The flows the lowering made that no line of the book wrote, as an opening of a loan's debt.
fn implied(book: &Book<'_>) -> Vec<Flow> {
    let opens = |flow: &&Flow| matches!(flow.origin, Origin::Derived(Derivation::Opening(_)));
    book.flows.values().filter(opens).cloned().collect()
}

fn notes(diagnostics: &[Diagnostic]) -> Vec<&Diagnostic> {
    diagnostics.iter().filter(|d| &*d.code == "loan-opening").collect()
}

fn debt(book: &Book<'_>) -> axiom_core::Id<axiom_model::Place> {
    book.contracts[book.contract("home-loan").unwrap()].loan.unwrap().debt
}

#[test]
fn a_loan_made_before_the_book_opens_its_debt_with_what_its_schedule_says_on_the_books_first_day() {
    let (book, diagnostics) = built(&format!("{LOAN}{OPENING}"));
    let [opening] = &implied(&book)[..] else { panic!("one opening: {:?}", implied(&book)) };
    assert_eq!(
        opening.day,
        day(2026, 1, 1),
        "the day of the first fact, which an opening that nobody wrote must not move"
    );
    assert_eq!((opening.from, opening.to), (debt(&book), book.entities[book.roots.opening].place.unwrap()));
    assert_eq!((opening.out.qty.0, opening.arrive.qty.0), (OWED, OWED));
    assert_eq!(opening.mode, Mode::Opening, "states, not flows: no law sees it");
    assert_eq!(book.first_fact(), Some(day(2026, 1, 1)), "the book begins where it did");
    let [note] = &notes(&diagnostics)[..] else { panic!("one note: {diagnostics:?}") };
    assert_eq!(note.severity, Severity::Note);
    assert_eq!(note.message, "`home-loan` opens owing 85,591.44 USD, what its terms say on 2026-01-01");
}

#[test]
fn the_opening_is_the_number_the_schedule_compiled_after_the_journal_gives() {
    // What a loan owes before the first fact is a function of events before it: the rates a statement has said by then and the
    // index a reset reads, never of the journal. The lowering and the compiled schedule are two walks that must not differ.
    let variations = [
        ("", ""),
        ("", "2025-06-01 home-loan now at 3%\n"),
        ("", "2025-06-01 home-loan now at 3%\n2025-09-01 home-loan now at 7.5%\n"),
        ("    prepay recasts\n", ""),
        ("    resets 6m from 2025-07-01 to idx + 1%\n    prepay recasts\n", "2025-10-01 home-loan now at 5%\n"),
    ];
    for (nested, statements) in variations {
        let contract = LOAN.replacen("for condo\n", &format!("for condo\n{nested}"), 1);
        let (book, diagnostics) = built(&format!("{contract}{statements}{OPENING}"));
        let name = format!("{nested}{statements}");
        assert!(diagnostics.iter().all(|d| !d.is_error()), "{name:?}: {diagnostics:?}");
        let [opening] = &implied(&book)[..] else { panic!("{name:?}: one opening") };
        let compiled = book.promises.loan(book.contract("home-loan").unwrap()).unwrap().open_on(day(2025, 12, 31));
        assert_eq!(Some(opening.out.qty), compiled, "{name:?}");
    }
}

#[test]
fn a_rate_said_before_the_book_began_is_in_what_is_owed_then() {
    // 6% for the 4 payments to 2025-05-01 and 3% for the 7 after it: 84,874.83 where 6% for all would owe 85,591.44.
    let (book, _) = built(&format!("{LOAN}2025-06-01 home-loan now at 3%\n{OPENING}"));
    let [opening] = &implied(&book)[..] else { panic!("one opening") };
    assert_eq!(opening.out.qty.0, 8_487_483);
    assert_eq!(book.first_fact(), Some(day(2026, 1, 1)), "a rate said is no fact: the book begins when it did");
}

#[test]
fn an_opening_that_names_the_debt_is_the_books_own_and_nothing_is_implied() {
    let (book, diagnostics) = built(&format!("{LOAN}{OPENING}  home-loan 84_000 USD\n"));
    assert!(implied(&book).is_empty());
    assert!(notes(&diagnostics).is_empty(), "nothing is opened that the book did not already say: {diagnostics:?}");
    let tab: Vec<_> = book.flows.values().filter(|flow| flow.from == debt(&book) || flow.to == debt(&book)).collect();
    let [written] = &tab[..] else { panic!("only the book's own line touches the debt: {tab:?}") };
    assert_eq!((written.origin, written.mode, written.out.qty.0), (Origin::Written, Mode::Opening, 8_400_000));
}

#[test]
fn an_opening_on_any_other_day_that_names_the_debt_is_also_the_books_own() {
    let (book, diagnostics) = built(&format!("{LOAN}{OPENING}opening 2026-02-01\n  home-loan 84_000 USD\n"));
    assert!(implied(&book).is_empty() && notes(&diagnostics).is_empty());
}

#[test]
fn a_loan_the_journal_originates_was_made_in_the_book_and_is_not_opened() {
    let (book, diagnostics) =
        built(&format!("opening 2025-01-01\n  checking 900_000 USD\n{LOAN}2025-01-15 home-loan\n"));
    assert!(implied(&book).is_empty());
    assert!(notes(&diagnostics).is_empty(), "{diagnostics:?}");
}

/// The edit the note offers: where it goes (the byte its text is inserted at) and what is written.
fn edit(diagnostics: &[Diagnostic]) -> (u32, String) {
    let [note] = &notes(diagnostics)[..] else { panic!("one note: {diagnostics:?}") };
    let [help] = &note.help[..] else { panic!("one help: {note:?}") };
    let (loc, text) = help.edit.clone().expect("the help is an edit");
    assert_eq!(loc.start, loc.end, "an insertion");
    (loc.start, text)
}

#[test]
fn the_note_offers_the_lenders_number_as_a_line_of_the_opening_that_begins_the_book() {
    let text = format!("{LOAN}{OPENING}");
    let (_, diagnostics) = built(&text);
    let (at, line) = edit(&diagnostics);
    assert_eq!(
        line, "  home-loan 85_591.44 USD\n",
        "indented as the opening's own lines are, the amount as the language writes it"
    );
    let source = format!("{PRELUDE}{text}");
    assert!(source[at as usize..].starts_with("  checking 900_000 USD"), "before the first line of the opening");
}

#[test]
fn the_note_offers_an_opening_of_its_own_when_the_book_begins_with_something_else() {
    let text = format!("{LOAN}2026-01-01 checking -> bank 5 USD\n");
    let (_, diagnostics) = built(&text);
    let (at, block) = edit(&diagnostics);
    assert_eq!(block, "opening 2026-01-01\n  home-loan 85_591.44 USD\n\n");
    let source = format!("{PRELUDE}{text}");
    assert!(source[at as usize..].starts_with("2026-01-01 checking -> bank"), "before the line that begins the book");
}

#[test]
fn a_loan_whose_schedule_could_not_be_followed_to_the_books_first_day_is_not_opened() {
    // `late` has no row before 2026-09-01, so the reset of 2025-07-01 has nothing to read: what is owed on 2026-01-01 is not known.
    let resets = LOAN.replace("for condo\n", "for condo\n    resets 1y from 2025-07-01 to late + 2%\n");
    let (book, diagnostics) = built(&format!("param late\n  2026-09-01 5.5%\n{resets}{OPENING}"));
    assert!(implied(&book).is_empty() && notes(&diagnostics).is_empty());
}

#[test]
fn a_reset_on_the_day_the_book_begins_that_cannot_be_read_does_not_stop_the_opening() {
    // Nothing before the reset of 2026-01-01 depends on it: the payments to 2025-12-01 are what they were.
    let resets = LOAN.replace("for condo\n", "for condo\n    resets 1y from 2026-01-01 to late + 2%\n");
    let (book, _) = built(&format!("param late\n  2026-09-01 5.5%\n{resets}{OPENING}"));
    let [opening] = &implied(&book)[..] else { panic!("one opening") };
    assert_eq!(opening.out.qty.0, OWED);
}

#[test]
fn a_loan_whose_origination_is_written_is_not_opened_even_when_the_line_is_rejected() {
    // `2025-01-15 home-loan 10 USD` says the loan began that day (and is wrong to add an amount): the book has said where the
    // loan begins, so its terms do not say it again on top of the error.
    let (book, diagnostics) = built(&format!("{LOAN}{OPENING}2025-01-15 home-loan 10 USD\n"));
    assert!(diagnostics.iter().any(|d| &*d.code == "loan-origination-shape"), "{diagnostics:?}");
    assert!(implied(&book).is_empty() && notes(&diagnostics).is_empty());
}

#[test]
fn a_loan_made_after_the_books_first_fact_is_left_as_the_journal_says() {
    // The language has no opening for a loan that has not been made: its origination line says where the cash arrived, and a book
    // that does not write one has not said. Nothing is opened (the tab starts at nothing, as it did).
    let (book, diagnostics) = built(&format!("opening 2025-01-01\n  checking 900_000 USD\n{LOAN}"));
    assert!(implied(&book).is_empty() && notes(&diagnostics).is_empty());
}

#[test]
fn a_loan_made_on_the_day_the_book_begins_was_made_in_it_and_is_not_opened() {
    // The book's first fact is on the loan's own day and nothing originates it: the journal has not said where the cash went,
    // and the terms do not say it for the journal. Only a loan made before the first fact is opened.
    let (book, diagnostics) = built(&format!("opening 2025-01-15\n  checking 900_000 USD\n{LOAN}"));
    assert!(implied(&book).is_empty() && notes(&diagnostics).is_empty());
}

#[test]
fn a_loan_paid_off_before_the_book_began_opens_nothing() {
    let paid = "contract car with bank\n  loan 1_000 USD on 2024-01-15 at 0% over 6m\n  monthly on 1 from checking\n";
    let (book, diagnostics) = built(&format!("{paid}{OPENING}"));
    assert!(implied(&book).is_empty() && notes(&diagnostics).is_empty());
}

#[test]
fn a_book_with_no_fact_is_begun_by_its_loan_on_the_day_it_was_made() {
    let (book, diagnostics) = built(LOAN);
    let [opening] = &implied(&book)[..] else { panic!("one opening: {:?}", implied(&book)) };
    assert_eq!(
        (opening.day, opening.out.qty.0),
        (day(2025, 1, 15), 12_000_000),
        "all of what was borrowed, the day it was"
    );
    assert_eq!(
        book.first_fact(),
        Some(day(2025, 1, 15)),
        "the book begins with the loan, as it does with an origination line"
    );
    let [note] = &notes(&diagnostics)[..] else { panic!("one note: {diagnostics:?}") };
    assert!(note.labels.iter().any(|label| label.text.contains("the book has no fact yet")), "{note:?}");
}

#[test]
fn two_loans_of_a_book_with_no_fact_open_each_on_its_own_day_in_the_order_of_the_days() {
    let later = "contract car with bank\n  loan 30_000 USD on 2025-03-10 at 5% over 5y\n  monthly on 1 from checking\n";
    let (book, _) = built(&format!("{later}{LOAN}"));
    let days: Vec<_> = implied(&book).iter().map(|opening| opening.day).collect();
    assert_eq!(
        days,
        [day(2025, 1, 15), day(2025, 3, 10)],
        "the arena of flows is in day order whichever was declared first"
    );
}

#[test]
fn the_first_fact_is_the_earliest_of_a_flow_an_occurrence_an_assertion_and_a_split() {
    let first =
        |lines: &str| built(&format!("entity me\naccount savings : asset\ncommodity VTI\n{lines}")).0.first_fact();
    assert_eq!(first(""), None, "declarations are no fact");
    assert_eq!(first("2026-03-01 savings = 0 USD\n"), Some(day(2026, 3, 1)), "an assertion is");
    assert_eq!(
        first("opening 2026-02-01\n  savings 5 USD\n2026-03-01 savings = 5 USD\n"),
        Some(day(2026, 2, 1)),
        "an opening is"
    );
    assert_eq!(
        first("2026-03-05 savings -> me 1 USD\n2026-03-01 savings = 0 USD\n"),
        Some(day(2026, 3, 1)),
        "the earlier of two"
    );
    assert_eq!(first("2026-01-09 VTI split 2 for 1\n"), Some(day(2026, 1, 9)), "a split is");
    let kept = "contract rent with me\n  1_000 USD monthly on 1 from savings\n2026-04-01 rent\n";
    assert_eq!(first(kept), Some(day(2026, 4, 1)), "a line that keeps an occurrence is");
}
