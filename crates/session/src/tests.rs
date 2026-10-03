//! What a session promises: it answers as the pipeline it wraps does, an edit changes what it should and nothing else,
//! an edit it refuses leaves no trace, and a hypothesis is gone when the question is answered.

use axiom_core::{Day, Diagnostic, Qty};
use axiom_report::{Query, json};

use crate::{Options, Session, Sources, Texts};

const BOOK: &str = "\
base USD
commodity USD
  precision 2
entity me
entity grocer
purpose food : spending
account checking : asset
opening 2026-01-01
  checking 100 USD
2026-01-02 checking -> grocer 5 USD #food
";

fn day(text: &str) -> Day {
    Day::parse(text.as_bytes()).expect("a date")
}

fn options() -> Options {
    Options { today: day("2026-02-01"), relaxed: false }
}

fn balance() -> Query<'static> {
    Query::Balance { globs: vec![], at: None, value: false, monthly: false }
}

/// The balance as a client would be sent it.
fn shown(session: &Session<'_>) -> String {
    json::render(&session.query(&balance(), None).expect("the balance resolves"), session.sources())
}

fn codes<'a>(found: impl IntoIterator<Item = &'a Diagnostic>) -> Vec<String> {
    found.into_iter().map(|found| found.code.to_string()).collect()
}

#[test]
fn a_session_answers_as_the_pipeline_it_wraps() {
    let texts = Texts::default();
    let session = Session::open(Sources::in_memory(&texts, &[("axiom.ax", BOOK)], &[]), options());

    let parsed = Sources::in_memory(&texts, &[("axiom.ax", BOOK)], &[]).parse();
    let (book, built) = axiom_model::build(&parsed.0);
    let run = axiom_engine::run(&book, options());
    let summary = axiom_report::summary(&book, &run);

    assert_eq!(session.run().holdings.len(), run.holdings.len());
    assert_eq!(session.diagnostics().count(), parsed.1.len() + built.len() + run.diagnostics.len());
    let ours = session.summary();
    assert_eq!((ours.flows, ours.places, ours.laws), (summary.flows, summary.places, summary.laws));
    assert_eq!(ours.net_worth, summary.net_worth);
    assert_eq!(ours.net_worth.qty, Qty(9_500), "100.00 USD less the 5.00 USD spent");
    let direct = axiom_report::report(&book, &run, &balance(), None).unwrap();
    assert_eq!(shown(&session), json::render(&direct, session.sources()));
}

#[test]
fn what_reading_and_folding_found_comes_in_the_order_check_lists_it() {
    let text =
        format!("{BOOK}2026-01-03 checking -> grocer 7 USD #fod\n2026-01-04 -> -> ->\n2026-01-10 checking = 999 USD\n");
    let texts = Texts::default();
    let session = Session::open(Sources::in_memory(&texts, &[("axiom.ax", &text)], &[]), options());
    assert_eq!(
        codes(session.book_diagnostics()),
        ["expected-end-of-line", "unknown-purpose"],
        "the parser's, then the model's"
    );
    assert_eq!(codes(session.run_diagnostics()), ["assertion"]);
    assert_eq!(codes(session.diagnostics()), ["expected-end-of-line", "unknown-purpose", "assertion"]);
}

#[test]
fn an_unknown_owner_is_an_error_and_the_book_is_still_folded() {
    let texts = Texts::default();
    let session = Session::open(Sources::in_memory(&texts, &[("axiom.ax", BOOK)], &[]), options());
    let refused = session.query(&balance(), Some("nobody")).err().expect("no one is called nobody");
    assert_eq!(refused.code, "unknown-entity");
    assert_eq!(session.diagnostics().count(), 0, "the book's own diagnostics are there to be shown beside it");
}

#[test]
fn threads_query_one_session_and_sessions_are_each_their_own() {
    let texts = Texts::default();
    let session = Session::open(Sources::in_memory(&texts, &[("axiom.ax", BOOK)], &[]), options());
    let answers: Vec<String> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..4).map(|_| scope.spawn(|| shown(&session))).collect();
        handles.into_iter().map(|handle| handle.join().expect("a query does not panic")).collect()
    });
    assert!(answers.windows(2).all(|pair| pair[0] == pair[1]), "the fold is made once, and every thread reads it");
}
