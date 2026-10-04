//! What the views that list flows say of a flow a law derived: it is a posting like any other, right after the flow it came
//! from, and says what derived it.

use axiom_core::{Day, FileId, Loc};
use axiom_engine::Run;
use axiom_model::Book;

use crate::source_tests::with_run;
use crate::tests::{lines, report, show};
use crate::{FlowBy, Query};

fn day(y: i32, m: u32, d: u32) -> Day {
    Day::from_ymd(y, m, d).unwrap()
}

/// A card that credits 2% of every charge, and a book of two charges (and what else a test adds).
fn card(journal: &str) -> String {
    format!(
        "base USD
commodity USD
  precision 2
purpose rebate : income
purpose groceries : spending
entity me
entity issuer
entity shop
kind card : debt
  also issuer -> self 2% of amount #rebate
account checking
account visa : card
opening 2026-01-01
  checking 1_000.00 USD
{journal}"
    )
}

/// The same book with the flows the law derives written by hand, each after the line it comes from.
fn written(journal: &str) -> String {
    let mut text = card("").replace("  also issuer -> self 2% of amount #rebate\n", "");
    for line in journal.lines() {
        text += &format!("{line}\n");
        let amount: f64 = line.split(' ').nth(4).and_then(|amount| amount.parse().ok()).unwrap_or(0.0);
        let date = line.split(' ').next().unwrap();
        text += &format!("{date} issuer -> visa {:.2} USD #rebate\n", amount * 0.02);
    }
    text
}

fn rows(book: &Book, run: &Run, query: Query) -> Vec<String> {
    lines(&report(book, run, &query, None).expect("the query resolves").sections[0])
}

const JOURNAL: &str = "2026-02-05 visa -> shop 100.00 USD #groceries\n2026-02-05 visa -> shop 50.00 USD #groceries\n";

#[test]
fn a_register_lists_a_derived_flow_right_after_the_flow_it_came_from_and_says_what_derived_it() {
    with_run(&card(JOURNAL), day(2026, 3, 1), |book, run| {
        let register = Query::Register { place: "visa", from: None, to: None };
        let rows = rows(book, run, register);
        let notes: Vec<_> = rows.iter().map(|row| row.split(" | ").nth(3).unwrap_or("").to_owned()).collect();
        assert_eq!(rows.len(), 4, "{rows:?}");
        assert!(notes[0].is_empty() && notes[2].is_empty(), "the charges are lines of the journal: {notes:?}");
        assert!(notes[1].starts_with("derived by the `also` of kind `card`"), "{notes:?}");
        assert!(rows[1].contains("-2.00 USD") && rows[3].contains("-1.00 USD"), "{rows:?}");
    });
}

#[test]
fn the_views_say_of_a_derived_flow_what_they_say_of_the_line_that_would_have_written_it() {
    let (derived, by_hand) = (card(JOURNAL), written(JOURNAL));
    let asks = |query: Query| {
        let (a, b) = (
            with_run(&derived, day(2026, 3, 1), |book, run| rows(book, run, query.clone())),
            with_run(&by_hand, day(2026, 3, 1), |book, run| rows(book, run, query.clone())),
        );
        (a, b)
    };
    for query in [
        Query::Flow { by: FlowBy::Party, from: None, to: None },
        Query::Flow { by: FlowBy::Period(axiom_core::Period::Month), from: None, to: None },
        Query::Balance { globs: vec![], at: None, value: false, monthly: false },
    ] {
        let (derived, by_hand) = asks(query.clone());
        assert_eq!(derived, by_hand, "{query:?}");
    }
}

#[test]
fn a_register_of_the_party_that_paid_a_derived_flow_lists_it() {
    with_run(&card(JOURNAL), day(2026, 3, 1), |book, run| {
        let register = Query::Register { place: "entity:issuer", from: None, to: None };
        let rows = rows(book, run, register);
        assert_eq!(rows.len(), 2, "{rows:?}");
        assert!(
            rows[0].contains("#rebate") && rows[0].contains("2.00 USD") && rows[0].contains("derived by"),
            "{rows:?}"
        );
    });
}

#[test]
fn a_flow_returned_returns_what_it_derived_in_the_register_too() {
    let journal = "2026-02-05 visa -> shop 100.00 USD ^c1\n2026-02-20 ^c1 returned\n";
    with_run(&card(journal), day(2026, 3, 1), |book, run| {
        let register = Query::Register { place: "visa", from: None, to: None };
        let rows = rows(book, run, register);
        assert!(rows.iter().all(|row| row.starts_with('~') || row.starts_with("2026-02-05")), "{rows:?}");
        assert!(rows.iter().any(|row| row.contains("returned 2026-02-20") && row.contains("derived by")), "{rows:?}");
    });
}

#[test]
fn why_a_line_says_what_its_flows_derived() {
    let text = card(JOURNAL);
    with_run(&text, day(2026, 3, 1), |book, run| {
        let start = text.find("2026-02-05 visa -> shop 100.00").unwrap();
        let loc =
            Loc::new(FileId(0), start as u32, (start + "2026-02-05 visa -> shop 100.00 USD #groceries".len()) as u32);
        let report = report(book, run, &Query::Line { loc }, None).unwrap();
        let shown = show(&report);
        assert!(shown.contains("derived flow: issuer → visa, 2.00 USD for #rebate"), "{shown}");
        assert!(!shown.contains("1.00 USD"), "what the other line derived is not this line's:\n{shown}");
    });
}
