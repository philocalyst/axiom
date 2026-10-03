//! What a session promises: it answers as the pipeline it wraps does, an edit changes what it should and nothing else,
//! an edit it refuses leaves no trace, and a hypothesis is gone when the question is answered.

use axiom_core::{Day, Diagnostic, FileId, Qty};
use axiom_report::{Money, Query, json};

use crate::{Applied, Edit, NewTransaction, Options, Refused, Session, Sources, Texts};

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

/// What a session says is wrong, without where: the identity `Applied` goes by.
fn said(session: &Session<'_>) -> Vec<(String, String)> {
    session.diagnostics().map(|found| (found.code.to_string(), found.message.clone())).collect()
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
    let plan = axiom_engine::Plan::new(&book);
    let folded = axiom_report::Folded::of(&plan, options());
    let direct = axiom_report::Context::over(plan, folded, None).unwrap().report(&balance()).unwrap();
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
fn applying_a_diagnostics_own_fix_clears_the_diagnostic() {
    let text = format!("{BOOK}2026-01-03 checking -> grocer 7 USD #fod\n");
    let texts = Texts::default();
    let mut session = Session::open(Sources::in_memory(&texts, &[("axiom.ax", &text)], &[]), options());
    let unknown = session.diagnostics().find(|found| found.code == "unknown-purpose").expect("`fod` is no purpose");
    let fix = Edit::fix(&unknown.help[0]).expect("the diagnostic says how to fix it");
    assert_eq!(fix, Edit::Replace { at: unknown.anchor().unwrap(), text: "food".to_string() });
    let before = shown(&session);

    let applied = session.apply(fix).expect("a purpose spelled right parses");
    assert_eq!(codes(&applied.removed), ["unknown-purpose"]);
    assert!(applied.added.is_empty(), "{:?}", applied.added);
    assert_eq!(applied.file, FileId(0));
    assert_eq!(session.diagnostics().count(), 0);
    assert!(session.sources().get(FileId(0)).unwrap().text.contains("#food\n"), "the text is the edited text");
    assert_ne!(shown(&session), before, "and what the book says follows it: seven more USD spent");
}

#[test]
fn a_what_if_answers_for_the_edit_and_changes_nothing() {
    let texts = Texts::default();
    let session = Session::open(Sources::in_memory(&texts, &[("axiom.ax", BOOK)], &[]), options());
    let (before, diagnostics_before, kept_before) = (shown(&session), said(&session), texts.len());
    let spend = NewTransaction {
        day: day("2026-01-03"),
        from: "checking",
        to: "grocer",
        amount: Money { qty: Qty(1_250), scale: 2, unit: "USD" },
        purpose: Some("food"),
        description: None,
        codes: &[],
    };
    let edit = spend.append_to(FileId(0)).unwrap();

    let (worth, changed) = session
        .what_if(&edit, |after| (after.summary().net_worth.qty, Applied::between(FileId(0), &session, after)))
        .expect("the edit applies");
    assert_eq!(worth, Qty(8_250), "100.00 less 5.00 and 12.50, in the session the edit would make");
    assert!(changed.added.is_empty() && changed.removed.is_empty());

    assert_eq!(shown(&session), before, "the session itself still says what it said");
    assert_eq!(said(&session), diagnostics_before);
    assert_eq!(session.summary().net_worth.qty, Qty(9_500));
    assert_eq!(texts.len(), kept_before, "and a hypothesis keeps nothing: its text died with it");
}

#[test]
fn a_what_if_sees_the_diagnostics_an_edit_would_add() {
    let texts = Texts::default();
    let session = Session::open(Sources::in_memory(&texts, &[("axiom.ax", BOOK)], &[]), options());
    let edit = Edit::Append { file: FileId(0), text: "2026-01-03 checking = 999 USD\n".to_string() };
    let changed = session.what_if(&edit, |after| Applied::between(FileId(0), &session, after)).unwrap();
    assert_eq!(codes(&changed.added), ["assertion"]);
    assert!(changed.removed.is_empty());
    assert_eq!(session.diagnostics().count(), 0, "and the session that was asked has none");
}

#[test]
fn an_applied_transaction_moves_the_money_and_says_what_it_changed() {
    let texts = Texts::default();
    let mut session = Session::open(Sources::in_memory(&texts, &[("axiom.ax", BOOK)], &[]), options());
    let spend = NewTransaction {
        day: day("2026-01-03"),
        from: "checking",
        to: "grocer",
        amount: Money { qty: Qty(1_250), scale: 2, unit: "USD" },
        purpose: Some("food"),
        description: Some("weekly shop"),
        codes: &["receipt-9"],
    };
    let kept = texts.len();
    let applied = session.apply(spend.append_to(FileId(0)).unwrap()).expect("a flow of the book's own words applies");
    assert!(applied.added.is_empty() && applied.removed.is_empty());
    assert_eq!(texts.len(), kept + 1, "one text more, the file as it reads now");
    assert_eq!(session.summary().net_worth.qty, Qty(8_250));
    let text = &session.sources().get(FileId(0)).unwrap().text;
    assert!(text.ends_with("#food \"weekly shop\" ^receipt-9\n"), "{text}");
}

#[test]
fn an_edit_that_makes_the_book_complain_is_applied_and_says_what_it_made() {
    let texts = Texts::default();
    let mut session = Session::open(Sources::in_memory(&texts, &[("axiom.ax", BOOK)], &[]), options());
    let edit = Edit::Append { file: FileId(0), text: "2026-01-03 checking -> grocer 5 USD #fod\n".to_string() };
    let applied = session.apply(edit).expect("a wrong word is the model's to complain of, not the parser's");
    assert_eq!(codes(&applied.added), ["unknown-purpose"]);
    assert_eq!(codes(session.diagnostics()), ["unknown-purpose"]);
    let rendered = applied.json(session.sources());
    assert!(rendered.starts_with("{\"file\":\"axiom.ax\",\"added\":[{\"code\":\"unknown-purpose\""), "{rendered}");
    assert!(rendered.ends_with("],\"removed\":[]}"), "{rendered}");
}

/// What a session was, in what a client could see of it: its diagnostics, its answer, and what the arena holds.
fn snapshot(session: &Session<'_>, texts: &Texts) -> (Vec<(String, String)>, String, usize) {
    (said(session), shown(session), texts.len())
}

#[test]
fn an_edit_that_is_refused_leaves_the_session_as_it_was() {
    static SYSTEM: [(&str, &str); 1] = [("std.ax", "system std\n")];
    let texts = Texts::default();
    let mut session = Session::open(Sources::in_memory(&texts, &[("axiom.ax", BOOK)], &SYSTEM), options());
    let before = snapshot(&session, &texts);
    let end = BOOK.len() as u32;
    let unparsable = Edit::Append { file: FileId(0), text: "2026-01-04 -> -> ->\n".to_string() };
    let refusals = [
        (Edit::Append { file: FileId(7), text: "x\n".to_string() }, "there is no source file number 7"),
        (
            Edit::Append { file: FileId(1), text: "x\n".to_string() },
            "file number 1 ships with Axiom and cannot be edited",
        ),
        (
            Edit::Replace { at: axiom_core::Loc::new(FileId(0), 5, end + 40), text: String::new() },
            "are not a span of the text of file number 0",
        ),
        (unparsable.clone(), "the edit would add 1 syntax error(s) to the file: expected the end of the line"),
    ];
    for (edit, reason) in refusals {
        let refused = session.apply(edit.clone()).expect_err("this edit is not applied");
        assert!(refused.to_string().contains(reason), "{refused}");
        assert_eq!(snapshot(&session, &texts), before, "{edit:?} left a trace");
        let asked = session.what_if(&edit, |_| ()).expect_err("nor is it a hypothesis");
        assert_eq!(asked.to_string(), refused.to_string());
    }
    assert!(
        matches!(session.apply(unparsable), Err(Refused::Unparsable(errors)) if codes(&errors) == ["expected-end-of-line"])
    );
}

#[test]
fn a_book_with_syntax_errors_can_be_edited_but_not_made_worse() {
    let broken = format!("{BOOK}2026-01-04 -> -> ->\n");
    let texts = Texts::default();
    let mut session = Session::open(Sources::in_memory(&texts, &[("axiom.ax", &broken)], &[]), options());
    assert_eq!(codes(session.book_diagnostics()), ["expected-end-of-line"]);

    let more = Edit::Append { file: FileId(0), text: "2026-01-05 checking -> grocer 1 USD #food\n".to_string() };
    let applied = session.apply(more).expect("the old error is not the edit's");
    assert!(applied.added.is_empty() && applied.removed.is_empty());

    let worse = Edit::Append { file: FileId(0), text: "2026-01-06 -> -> ->\n".to_string() };
    assert!(matches!(session.apply(worse), Err(Refused::Unparsable(_))), "a second of them is the edit's");

    let fixed = Edit::Replace {
        at: axiom_core::Loc::new(FileId(0), BOOK.len() as u32, BOOK.len() as u32 + 20),
        text: String::new(),
    };
    let applied = session.apply(fixed).expect("removing the broken line is an edit");
    assert_eq!(codes(&applied.removed), ["expected-end-of-line"]);
    assert_eq!(session.diagnostics().count(), 0);
}

#[test]
fn an_edit_to_one_file_leaves_the_others_and_every_number() {
    let files = [
        ("axiom.ax", "base USD\ncommodity USD\n  precision 2\nentity me\naccount checking : asset\n"),
        ("journal.ax", "2026-01-02 checking 5 USD -> me\n"),
    ];
    let texts = Texts::default();
    let mut session = Session::open(Sources::in_memory(&texts, &files, &[]), options());
    let first = session.sources().get(FileId(0)).unwrap() as *const _;
    let applied =
        session.apply(Edit::Append { file: FileId(1), text: "2026-01-03 checking 6 USD -> me\n".to_string() }).unwrap();
    assert_eq!(applied.file, FileId(1), "the change says which file it was to");
    assert_eq!(
        session.sources().get(FileId(0)).unwrap() as *const _,
        first,
        "the file that was not edited is the same text"
    );
    let edited = session.sources().get(FileId(1)).unwrap();
    assert_eq!((edited.id, &*edited.path), (FileId(1), "journal.ax"));
    assert_eq!(edited.lines(), 3, "and its lines are counted again");
    assert_eq!(session.sources().project_paths().collect::<Vec<_>>(), ["axiom.ax", "journal.ax"]);
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

    // A second session over a text of its own: an edit to it is no edit to the first.
    let other_texts = Texts::default();
    let mut other = Session::open(Sources::in_memory(&other_texts, &[("axiom.ax", BOOK)], &[]), options());
    other
        .apply(Edit::Append { file: FileId(0), text: "2026-01-03 checking -> grocer 50 USD #food\n".to_string() })
        .unwrap();
    assert_eq!(shown(&session), answers[0]);
    assert_ne!(shown(&other), answers[0]);
}

#[test]
fn an_unknown_owner_is_an_error_and_the_book_is_still_folded() {
    let texts = Texts::default();
    let session = Session::open(Sources::in_memory(&texts, &[("axiom.ax", BOOK)], &[]), options());
    let refused = session.query(&balance(), Some("nobody")).err().expect("no one is called nobody");
    assert_eq!(refused.code, "unknown-entity");
    assert_eq!(session.diagnostics().count(), 0, "the book's own diagnostics are there to be shown beside it");
}
