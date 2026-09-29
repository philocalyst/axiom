//! End to end: text in, book and diagnostics out.

use axiom_core::{Diagnostic, FileId};
use axiom_syntax::parse;

use crate::{Book, Miss, Source, build};

/// The few standard kinds these books use, so that they read like real ones.
const STD: &str = "\
system std
kind bank : asset
kind broker : asset
kind credit-card : liability
kind person : entity
kind employer : entity
kind currency : commodity
kind stock : commodity
";

const ACCOUNTS: &str = "\
base USD
entity acme : employer
  via income/salary
account assets/bank/checking : bank
account assets/bank/savings : bank
account assets/brokerage : broker
account liabilities/visa : credit-card
";

/// Builds the files of a project beside the tiny standard system, and hands the
/// book and the diagnostics to `then`.
fn with_files<R>(project: &[(&str, &str)], then: impl FnOnce(&Book, &[Diagnostic]) -> R) -> R {
    let files = std::iter::once(("std.ax", STD, true)).chain(project.iter().map(|&(path, text)| (path, text, false)));
    let parsed: Vec<_> = files
        .enumerate()
        .map(|(at, (path, text, embedded))| {
            let (file, diags) = parse(FileId(at as u16), text);
            assert!(diags.is_empty(), "{path} does not parse: {diags:?}");
            Source { path, file, embedded }
        })
        .collect();
    let (book, diags) = build(&parsed);
    then(&book, &diags)
}

/// Builds one project file with the accounts every test uses, then `text`.
fn with_book<R>(text: &str, then: impl FnOnce(&Book, &[Diagnostic]) -> R) -> R {
    let text = format!("{ACCOUNTS}\n{text}");
    with_files(&[("axiom.ax", &text)], then)
}

fn codes(diags: &[Diagnostic]) -> Vec<&str> {
    diags.iter().map(|diag| &*diag.code).collect()
}

/// `checking -> savings 10 USD`, as the book records it.
fn moves(book: &Book) -> Vec<String> {
    let flow = |flow: &crate::Flow| {
        let (from, to) = (book.places[flow.from].path, book.places[flow.to].path);
        format!("{} -> {}  {}  arrives {}", book.name(from), book.name(to), book.show(flow.out), book.show(flow.arrive))
    };
    book.flows.iter().map(|(_, one)| flow(one)).collect()
}

#[test]
fn the_standard_systems_compile() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../systems/src");
    let mut written = Vec::new();
    collect(&dir, &dir, &mut written);
    let files: Vec<_> = written.iter().map(|(path, text)| (path.as_str(), text.as_str(), true)).collect();
    let parsed: Vec<_> = files
        .iter()
        .enumerate()
        .map(|(at, &(path, text, embedded))| Source { path, file: parse(FileId(at as u16), text).0, embedded })
        .collect();
    let (_, diags) = build(&parsed);
    assert!(diags.is_empty(), "{:?}", diags.iter().map(|diag| &diag.message).collect::<Vec<_>>());
}

/// Every `.ax` file under `dir`, with its path relative to `root`.
fn collect(dir: &std::path::Path, root: &std::path::Path, into: &mut Vec<(String, String)>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().map(|entry| entry.unwrap().path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            collect(&path, root, into);
        } else if path.extension().is_some_and(|extension| extension == "ax") {
            let relative = path.strip_prefix(root).unwrap().to_string_lossy().into_owned();
            into.push((relative, std::fs::read_to_string(&path).unwrap()));
        }
    }
}

#[test]
fn a_suffix_names_one_place_or_asks_which() {
    let text = "
account assets/broker/checking : broker
2026-01-05 savings -> bank/checking 10 USD
2026-01-06 savings -> checking 10 USD
";
    with_book(text, |book, diags| {
        assert_eq!(codes(diags), ["ambiguous-place"]);
        assert_eq!(moves(book), ["assets/bank/savings -> assets/bank/checking  10 USD  arrives 10 USD"]);
        assert!(book.place("savings").is_ok());
        assert!(matches!(book.place("checking"), Err(Miss::Ambiguous(both)) if both.len() == 2));
    });
}

#[test]
fn a_misspelled_name_is_reported_with_the_closest_one() {
    with_book("2026-01-05 savings -> chekcing 10 USD\n2026-01-06 savings -> checking 10 USD", |book, diags| {
        assert_eq!(codes(diags), ["unknown-place"]);
        assert_eq!(diags[0].help[0].text, "did you mean `checking`?");
        assert_eq!(book.flows.len(), 1, "the good transaction survives the bad one");
    });
}

#[test]
fn each_form_of_transaction_pairs_what_leaves_with_what_arrives() {
    let text = "
2026-01-10 checking -> savings 100 USD
2026-01-11 checking 2_000 USD -> brokerage 7 VTI
2026-01-12 checking -> brokerage 3 VTI @ 300 USD
2026-01-13 acme -> 5_200 USD
  savings 800 USD
  checking ...
";
    with_book(text, |book, diags| {
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(
            moves(book),
            [
                "assets/bank/checking -> assets/bank/savings  100 USD  arrives 100 USD",
                "assets/bank/checking -> assets/brokerage  2,000 USD  arrives 7 VTI",
                "assets/bank/checking -> assets/brokerage  900 USD  arrives 3 VTI",
                "income/salary -> assets/bank/savings  800 USD  arrives 800 USD",
                "income/salary -> assets/bank/checking  4,400 USD  arrives 4,400 USD",
            ]
        );
        let implied = book.prices.quotes().iter().filter(|quote| quote.implied).count();
        assert_eq!(implied, 2, "both exchanges say what a share cost");
    });
}

#[test]
fn a_transaction_that_does_not_add_up_says_by_how_much() {
    let text = "
2026-01-13 acme -> 5_200 USD
  savings 800 USD
  checking 4_000 USD
";
    with_book(text, |_, diags| assert_eq!(codes(diags), ["split-short"]));
}

#[test]
fn every_written_amount_teaches_its_commodity_a_precision() {
    let text = "
2026-01-10 checking -> savings 100 USD
2026-01-11 checking -> savings 0.5 USD
2026-01-12 checking -> brokerage 0.125 VTI @ 300.25 USD
";
    with_book(text, |book, _| {
        let scale = |symbol: &str| book.commodities[book.commodity(symbol).unwrap()].scale;
        assert_eq!((scale("USD"), scale("VTI")), (2, 3));
    });
}

#[test]
fn kinds_inherit_and_a_cycle_is_cut() {
    let text = "
kind hi-yield : bank
kind first : second
kind second : first
account assets/bank/hy : hi-yield
";
    with_book(text, |book, diags| {
        assert_eq!(codes(diags), ["kind-cycle"]);
        let (hi_yield, bank) = (book.kind("hi-yield").unwrap(), book.kind("bank").unwrap());
        assert!(book.is_a(hi_yield, bank) && book.is_a(hi_yield, book.roots.asset));
        assert!(!book.is_a(bank, hi_yield));
    });
}

#[test]
fn a_law_is_type_checked_when_it_is_compiled() {
    let text = "
law fine
  on in
  when to is bank
  require amount <= 500 USD

law adds-a-date
  on in
  require amount + date <= 5 USD
";
    with_book(text, |book, diags| {
        assert_eq!(codes(diags), ["type-mismatch"]);
        assert!(book.law("fine").is_ok());
        assert!(book.law("adds-a-date").is_err(), "a law that does not type-check is left out");
    });
}

#[test]
fn the_built_in_me_is_configured_but_never_declared_twice() {
    let once = "entity me : person\n";
    with_book(once, |book, diags| {
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(book.entities[book.roots.me].kind, book.kind("person").unwrap());
    });
    with_book(&format!("{once}entity me : person\n"), |_, diags| assert_eq!(codes(diags), ["duplicate-declaration"]));
}

#[test]
fn assertions_keep_the_sign_they_were_written_in() {
    with_book("2026-01-31 visa = 1_234.56 USD\n2026-01-31 checking = 10 USD\n", |book, diags| {
        assert!(diags.is_empty(), "{diags:?}");
        let shown: Vec<_> = book.asserts.iter().map(|assert| book.show(assert.amount).to_string()).collect();
        assert_eq!(shown, ["1,234.56 USD", "10.00 USD"]);
    });
}

/// A journal long enough to be elaborated by several threads, written out of
/// order in places. `n` USD in transaction `n` tells the transactions apart.
fn long_journal(days: impl Fn(u32) -> u32) -> String {
    let mut text = String::new();
    for n in 1..=20_000u32 {
        let day = days(n);
        text += &format!("2026-{:02}-{:02} checking -> savings {n} USD\n", 1 + day / 28, 1 + day % 28);
    }
    text
}

/// By day, and within a day in the order written: which the quantity tells.
fn ordered_by_day_then_by_declaration(book: &Book) -> bool {
    book.flows.iter().map(|(_, flow)| (flow.day, flow.out.qty)).is_sorted()
}

#[test]
fn flows_are_laid_out_by_day_whichever_way_they_were_written() {
    for written in [long_journal(|n| n / 2_000), long_journal(|n| (n * 7919) % 300)] {
        with_book(&written, |book, diags| {
            assert!(diags.is_empty(), "{:?}", diags.first());
            assert_eq!(book.flows.len(), 20_000);
            assert!(ordered_by_day_then_by_declaration(book));
            let checking = book.place("checking").unwrap();
            assert_eq!(book.touching[checking].len(), 20_000);
        });
    }
}

#[test]
fn an_entity_in_a_place_position_is_the_payee_and_stands_for_its_via() {
    with_book("2026-01-13 acme -> savings 800 USD\n", |book, diags| {
        assert!(diags.is_empty(), "{diags:?}");
        let (_, flow) = book.flows.iter().next().unwrap();
        assert_eq!(flow.payee, Some(book.entity("acme").unwrap()));
        assert_eq!(book.name(book.places[flow.from].path), "income/salary");
    });
}

#[test]
fn the_unwritten_side_of_an_exchange_keeps_its_commodity_with_no_quantity() {
    let text = "
2026-01-11 checking ? USD -> brokerage 7 VTI
2026-01-12 checking 900 USD -> brokerage ? VTI
";
    with_book(text, |book, diags| {
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(
            moves(book),
            [
                "assets/bank/checking -> assets/brokerage  0 USD  arrives 7 VTI",
                "assets/bank/checking -> assets/brokerage  900 USD  arrives 0 VTI",
            ]
        );
        assert!(book.flows.iter().all(|(_, flow)| flow.infer == crate::Infer::Unknown));
    });
}

#[test]
fn a_flow_from_a_place_to_itself_is_listed_once_under_that_place() {
    let text = "2026-01-11 brokerage 100 USD -> brokerage 1 VTI\n2026-01-12 brokerage 200 USD -> brokerage 2 VTI\n";
    with_book(text, |book, diags| {
        assert!(diags.is_empty(), "{diags:?}");
        let brokerage = book.place("brokerage").unwrap();
        assert_eq!(book.touching[brokerage].len(), 2);
    });
}

#[test]
fn a_kind_named_with_digits_is_a_kind_like_any_other() {
    let text = "
kind 529 : asset
account assets/college : 529
law only-college
  on in
  when to is 529
  require amount <= 5_000 USD
";
    with_book(text, |book, diags| {
        assert!(diags.is_empty(), "{diags:?}");
        let college = book.places[book.place("college").unwrap()].kind;
        assert_eq!(book.name(book.kinds[college].name), "529");
    });
}

#[test]
fn a_law_that_reads_age_makes_sure_born_is_a_name() {
    let text = "
law of-age
  on out
  require owner.age >= 18y
";
    with_book(text, |book, diags| {
        assert!(diags.is_empty(), "{diags:?}");
        assert!(book.names.get("born").is_some());
    });
}

#[test]
fn assertions_and_events_are_kept_in_day_order() {
    let text = "
2026-01-31 checking = 5 USD
2026-01-05 checking = 1 USD
2026-01-06 checking -> savings 1 USD #a
2026-01-05 #a settled
";
    with_book(text, |book, _| {
        assert!(book.asserts.windows(2).all(|pair| pair[0].day <= pair[1].day));
        assert!(book.events.windows(2).all(|pair| pair[0].day <= pair[1].day));
    });
}
