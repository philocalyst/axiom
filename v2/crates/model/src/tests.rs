//! End to end: text in, book and diagnostics out.

use axiom_core::{Day, Diagnostic, FileId};
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
kind household : entity
kind grocer : entity
  via expenses/food
kind receivable : asset
  claim
kind pretax : asset
  deferred
kind gift : asset
  basis zero
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
account expenses/food
account income/salary
";

/// Builds the files of a project beside the tiny standard system, and hands the
/// book and the diagnostics to `then`.
fn with_files<R>(project: &[(&str, &str)], then: impl FnOnce(&mut Book, &[Diagnostic]) -> R) -> R {
    let files = std::iter::once(("std.ax", STD, true)).chain(project.iter().map(|&(path, text)| (path, text, false)));
    let parsed: Vec<_> = files
        .enumerate()
        .map(|(at, (path, text, embedded))| {
            let (file, diags) = parse(FileId(at as u16), text);
            assert!(diags.is_empty(), "{path} does not parse: {diags:?}");
            Source { path, file, embedded }
        })
        .collect();
    let (mut book, diags) = build(&parsed);
    then(&mut book, &diags)
}

/// Builds one project file with the accounts every test uses, then `text`.
fn with_book<R>(text: &str, then: impl FnOnce(&mut Book, &[Diagnostic]) -> R) -> R {
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
fn an_entity_takes_a_name_from_an_account_that_only_ends_with_it_and_the_clash_is_said_once() {
    let text = "
account assets/owed/acme : receivable
2026-01-05 acme -> checking 100 USD
2026-01-06 acme -> checking 100 USD
2026-01-07 owed/acme -> checking 50 USD
";
    with_book(text, |book, diags| {
        assert_eq!(codes(diags), ["ambiguous-name"], "{diags:?}");
        let said = &diags[0];
        assert_eq!(said.message, "`acme` is both an entity and the end of `assets/owed/acme`, and lines still write it");
        assert!(said.notes.iter().any(|note| note.contains("written on 2 lines, and each takes the entity")));
        assert!(said.help.iter().any(|help| help.text.contains("write `owed/acme` where `assets/owed/acme` is meant")));
        let paid = ["income/salary", "income/salary", "assets/owed/acme"];
        let from: Vec<_> = book.flows.iter().map(|(_, flow)| book.name(book.places[flow.from].path)).collect();
        assert_eq!(from, paid, "the entity wins in flows, and the longer suffix still means the account");
    });
}

#[test]
fn an_entity_that_stands_for_the_account_is_no_clash_and_an_unwritten_name_is_not_reported() {
    let text = "
account assets/owed/kim : receivable
entity kim : employer
  via assets/owed/kim
account assets/owed/pat : receivable
2026-01-05 kim -> checking 10 USD
2026-01-06 owed/pat -> checking 10 USD
";
    with_book(text, |book, diags| {
        assert!(diags.is_empty(), "{diags:?}");
        let flow = &book.flows[axiom_core::Id::new(0)];
        assert_eq!(book.name(book.places[flow.from].path), "assets/owed/kim");
        assert!(flow.payee.is_some(), "the entity is still the payee");
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
fn the_expense_legs_of_an_exchange_are_its_costs_and_no_other_split_has_any() {
    let text = "
2026-01-11 brokerage 10 VTI -> 1_500 USD
  checking 1_490 USD
  food 10 USD
2026-01-12 checking -> 2_000 USD
  brokerage 7 VTI
  food 5 USD
2026-01-13 acme -> 5_200 USD
  food 200 USD
  checking ...
";
    with_book(text, |book, diags| {
        assert!(diags.is_empty(), "{diags:?}");
        let costs: Vec<_> = book.flows.iter().map(|(_, flow)| flow.detail().cost.map(|cost| cost.qty.0)).collect();
        // The sale, its payment of the fee, the purchase, its fee, and a paycheck that has none.
        assert_eq!(costs, [Some(10), None, Some(5), None, None, None]);
        let exchanges: Vec<_> = book.flows.iter().filter(|(_, flow)| flow.is_exchange()).map(|(_, f)| f.day).collect();
        assert_eq!(exchanges.len(), 2);
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
fn deep_kind_defaults_keep_nearest_value_and_its_source_location() {
    let text = "\
kind label-0 : person
  has nickname text
  nickname \"root\"
kind label-1 : label-0
  nickname \"middle\"
kind label-2 : label-1
  nickname \"near\"
kind label-3 : label-2
kind label-4 : label-3
entity inherited : label-4
entity own : label-4
  nickname \"instance\"
";
    let source = format!("{ACCOUNTS}\n{text}");
    with_files(&[("axiom.ax", &source)], |book, diags| {
        assert!(diags.is_empty(), "{diags:?}");
        let nickname = book.names.get("nickname").unwrap();
        let value = |entity: &str| {
            let entity = book.entity(entity).unwrap();
            book.entities[entity].props.iter().find(|prop| prop.name == nickname).unwrap()
        };
        let inherited = value("inherited");
        let own = value("own");
        let written = |prop: &crate::Prop, text: &str| {
            assert_eq!(prop.value, crate::Value::Text(book.names.get(text).unwrap()));
            let loc = prop.loc.expect("defaults preserve their source location");
            let start = source.find(&format!("nickname \"{text}\"")).unwrap();
            assert_eq!(loc.range(), start..start + format!("nickname \"{text}\"").len());
        };
        written(inherited, "near");
        written(own, "instance");
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

// ─── The v3 language ────────────────────────────────────────────────────────

fn flows_of<'a>(book: &'a Book) -> Vec<&'a crate::Flow> {
    book.flows.iter().map(|(_, flow)| flow).collect()
}

/// The first and last day a flow is recognized over, as text.
fn recognized(flow: &crate::Flow) -> String {
    format!("{}..{}", flow.recognized.first(), flow.recognized.last())
}

#[test]
fn a_flow_is_recognized_over_its_range_or_the_period_it_is_for() {
    let text = "
2026-01-01..2026-12-31 checking -> savings 1_200 USD
2026-01-15 checking -> savings 100 USD for 2025
2026-02-01 checking -> savings 10 USD for 2026-03
2026-02-02 checking -> savings 10 USD for 2026-04-05
2026-02-03 checking -> savings 10 USD for 2026-06-01..2026-06-30
2026-02-04 checking -> savings 10 USD
";
    with_book(text, |book, diags| {
        assert!(diags.is_empty(), "{diags:?}");
        let flows = flows_of(book);
        let days: Vec<String> = flows.iter().map(|flow| flow.day.to_string()).collect();
        assert_eq!(days, ["2026-01-01", "2026-01-15", "2026-02-01", "2026-02-02", "2026-02-03", "2026-02-04"]);
        let periods: Vec<String> = flows.iter().map(|flow| recognized(flow)).collect();
        assert_eq!(
            periods,
            [
                "2026-01-01..2026-12-31",
                "2025-01-01..2025-12-31",
                "2026-03-01..2026-03-31",
                "2026-04-05..2026-04-05",
                "2026-06-01..2026-06-30",
                "2026-02-04..2026-02-04",
            ]
        );
    });
}

#[test]
fn for_a_code_selects_and_links_and_for_an_entity_holds() {
    let text = "
2026-03-01 acme -> checking 4_800 USD #inv-12
2026-04-02 checking -> savings 3_000 USD for #inv-12
2026-04-03 checking -> savings 100 USD for acme
2026-04-04 checking -> savings 100 USD
";
    with_book(text, |book, diags| {
        assert!(diags.is_empty(), "{diags:?}");
        let flows = flows_of(book);
        let code = book.names.get("inv-12").unwrap();
        assert!(matches!(&*flows[1].select, [crate::Select::Code(sym)] if *sym == code));
        assert_eq!(&*flows[1].codes, [code], "the link is the code on the flow");
        assert_eq!(flows[2].detail().hold, Some(book.entity("acme").unwrap()));
        assert_eq!(flows[3].detail().hold, None);
        assert!(flows[3].detail.is_none(), "a flow that says nothing carries nothing");
    });
}

#[test]
fn due_basis_and_basis_ends_are_kept() {
    let text = "
2026-03-01 acme -> checking 4_800 USD #inv-12 due 30d
2026-03-02 acme -> savings 100 USD due 2026-05-01
2026-03-03 acme -> savings 100 USD basis 40 USD
2026-03-04 checking -> brokerage.basis 200 USD
2026-03-05 brokerage.basis 50 USD -> checking
";
    with_book(text, |book, diags| {
        assert!(diags.is_empty(), "{diags:?}");
        let flows = flows_of(book);
        let dues: Vec<_> = flows.iter().map(|flow| flow.detail().due.map(|day| day.to_string())).collect();
        assert_eq!(dues, [Some("2026-03-31".into()), Some("2026-05-01".into()), None, None, None]);
        let usd = book.commodity("USD").unwrap();
        assert_eq!(
            flows[2].detail().basis.map(|qty| book.show(crate::Amount::new(qty, usd)).to_string()),
            Some("40 USD".into())
        );
        assert_eq!(flows[3].detail().basis_end, Some(crate::End::To));
        assert_eq!(flows[4].detail().basis_end, Some(crate::End::From));
        // Quantity crosses every end but the basis one.
        let crosses = |flow: &crate::Flow| [flow.moves_quantity(crate::End::From), flow.moves_quantity(crate::End::To)];
        assert_eq!(crosses(flows[0]), [true, true]);
        assert_eq!(crosses(flows[3]), [true, false]);
        assert_eq!(crosses(flows[4]), [false, true]);
    });
}

#[test]
fn a_basis_in_another_commodity_than_the_base_is_refused() {
    with_book("2026-03-03 acme -> brokerage 3 VTI basis 40 VTI\n", |_, diags| {
        assert_eq!(codes(diags), ["basis-unit"]);
    });
}

#[test]
fn a_closing_statement_is_one_exchange_its_legs_allocate() {
    let text = "
2026-12-29 brokerage 1 VTI -> 431_500 USD
  expenses/food  25_000 USD
  savings        400_000 USD
  checking       ...
";
    with_book(text, |book, diags| {
        assert!(diags.is_empty(), "{diags:?}");
        // The exchange runs through the remainder leg's place, and the other
        // legs pay out of the proceeds.
        assert_eq!(
            moves(book),
            [
                "assets/brokerage -> assets/bank/checking  1 VTI  arrives 431,500 USD",
                "assets/bank/checking -> expenses/food  25,000 USD  arrives 25,000 USD",
                "assets/bank/checking -> assets/bank/savings  400,000 USD  arrives 400,000 USD",
            ]
        );
    });
}

#[test]
fn an_all_source_is_solved_from_what_the_place_holds() {
    with_book("2026-05-01 brokerage all VTI -> savings 500 USD\n", |book, diags| {
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(flows_of(book)[0].infer, crate::Infer::All);
    });
}

#[test]
fn a_plan_occurrence_is_the_plan_redated_with_overrides() {
    let text = "
plan paycheck every 2w from 2026-01-02 acme -> 5_200 USD
  savings 800 USD
  checking ...
2026-01-16 paycheck
2026-01-30 paycheck
  savings 900 USD
2026-03-13 paycheck 5_900 USD
";
    with_book(text, |book, diags| {
        assert!(diags.is_empty(), "{diags:?}");
        let plan = book.plans.iter().next().unwrap().0;
        // The plan's own transaction follows the journal's and holds no flows.
        let planned: Vec<_> = book.txns.iter().map(|(_, txn)| txn.plan).collect();
        assert_eq!(planned, [Some(plan), Some(plan), Some(plan), None]);
        assert_eq!(
            moves(book),
            [
                "income/salary -> assets/bank/savings  800 USD  arrives 800 USD",
                "income/salary -> assets/bank/checking  4,400 USD  arrives 4,400 USD",
                "income/salary -> assets/bank/savings  900 USD  arrives 900 USD",
                "income/salary -> assets/bank/checking  4,300 USD  arrives 4,300 USD",
                "income/salary -> assets/bank/savings  800 USD  arrives 800 USD",
                "income/salary -> assets/bank/checking  5,100 USD  arrives 5,100 USD",
            ]
        );
        assert_eq!(book.name(book.plans[plan].name.unwrap()), "paycheck");
    });
}

#[test]
fn an_unknown_plan_is_an_error_with_a_suggestion() {
    let text = "
plan paycheck every 2w from 2026-01-02 acme -> 5_200 USD
  checking ...
2026-01-16 paychek
";
    with_book(text, |_, diags| {
        assert_eq!(codes(diags), ["unknown-plan"]);
        assert_eq!(diags[0].help[0].text, "did you mean `paycheck`?");
    });
}

#[test]
fn openings_are_flows_from_equity_in_the_place_display_sign() {
    let text = "
opening 2024-12-31
  checking   10_000 USD
  visa       500 USD
  brokerage  40 VTI  basis 7_200 USD  since 2019-03-04
  brokerage  25 VTI
";
    with_book(text, |book, diags| {
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(
            moves(book),
            [
                "equity/opening -> assets/bank/checking  10,000 USD  arrives 10,000 USD",
                "liabilities/visa -> equity/opening  500 USD  arrives 500 USD",
                "equity/opening -> assets/brokerage  40 VTI  arrives 40 VTI",
                "equity/opening -> assets/brokerage  25 VTI  arrives 25 VTI",
            ]
        );
        assert!(book.flows.iter().all(|(_, flow)| flow.mode == crate::Mode::Opening));
        assert_eq!(book.txns.len(), 1, "one transaction per block");
        let terms = flows_of(book)[2].detail();
        assert_eq!(terms.since.map(|day| day.to_string()), Some("2019-03-04".into()));
        assert!(terms.basis.is_some());
    });
}

#[test]
fn splits_keep_their_ratio_and_are_sorted_by_day() {
    let text = "
2026-06-01 VTI split 1 for 10
2026-05-22 VTI split 2 for 1
";
    with_book(text, |book, diags| {
        assert!(diags.is_empty(), "{diags:?}");
        let ratios: Vec<String> = book.splits.iter().map(|split| format!("{} {}", split.day, split.ratio)).collect();
        assert_eq!(ratios, ["2026-05-22 2", "2026-06-01 0.1"]);
    });
}

#[test]
fn an_assertion_says_where_its_gap_goes() {
    let text = "
2026-01-31 checking = 5 USD
2026-01-31 savings = 7 USD !
2026-01-31 brokerage = 3 USD via market
2026-01-31 visa = -3 USD
";
    with_book(text, |book, diags| {
        assert!(diags.is_empty(), "{diags:?}");
        use crate::Gap;
        let gaps: Vec<_> = book.asserts.iter().map(|assert| assert.gap).collect();
        assert!(matches!(gaps[0], Gap::Refused));
        assert!(matches!(gaps[1], Gap::Unexplained(_)));
        assert!(matches!(gaps[2], Gap::Via { place, .. } if book.name(book.places[place].path) == "income/market"));
        let shown: Vec<String> = book.asserts.iter().map(|assert| book.show(assert.amount).to_string()).collect();
        assert_eq!(shown[3], "-3 USD");
    });
}

#[test]
fn a_price_may_have_more_decimals_than_its_commodity() {
    let text = "
commodity USD
  precision 2
2026-01-12 checking -> brokerage 3 VTI @ 285.7043 USD
";
    with_book(text, |book, diags| {
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(moves(book), ["assets/bank/checking -> assets/brokerage  857.11 USD  arrives 3 VTI"]);
    });
}

#[test]
fn an_alias_wins_over_a_suffix_and_an_ambiguity_is_reported_once_at_the_declaration() {
    let text = "
account assets/broker/checking as brk : broker
2026-01-05 savings -> checking 10 USD
2026-01-06 savings -> checking 10 USD
2026-01-07 savings -> checking 10 USD
2026-01-08 savings -> brk 10 USD
2026-01-09 savings -> bank/checking 10 USD
";
    with_book(text, |book, diags| {
        assert_eq!(codes(diags), ["ambiguous-place"], "{diags:?}");
        let diagnostic = &diags[0];
        let declared = book.places[book.place("brk").unwrap()].loc.unwrap();
        assert_eq!(diagnostic.labels[0].loc.file, declared.file);
        assert!(diagnostic.notes.iter().any(|note| note.contains("3 lines")), "{:?}", diagnostic.notes);
        assert!(diagnostic.help.iter().any(|help| help.text.contains("as")), "{:?}", diagnostic.help);
        assert_eq!(book.name(book.places[book.place("brk").unwrap()].path), "assets/broker/checking");
        assert_eq!(book.flows.len(), 2, "only the lines that named a place are kept");
    });
}

#[test]
fn several_entities_share_one_declaration() {
    let text = "
entity aldi, kroger : grocer
2026-01-05 checking -> aldi 10 USD
2026-01-06 checking -> kroger 12 USD
";
    with_book(text, |book, diags| {
        assert!(diags.is_empty(), "{diags:?}");
        let grocer = book.kind("grocer").unwrap();
        for name in ["aldi", "kroger"] {
            let entity = &book.entities[book.entity(name).unwrap()];
            assert_eq!(entity.kind, grocer);
            assert_eq!(entity.place, Some(book.place("expenses/food").unwrap()), "{name} takes its kind's via");
        }
        assert_eq!(book.flows.len(), 2);
    });
}

#[test]
fn kinds_resolve_basis_deferred_and_claim_down_their_chain() {
    let text = "
kind roth : pretax
  basis cost
account assets/ira : pretax
account assets/roth : roth
account assets/gifted : gift
account assets/owed : receivable
";
    with_book(text, |book, diags| {
        assert!(diags.is_empty(), "{diags:?}");
        let place = |name: &str| &book.places[book.place(name).unwrap()];
        assert_eq!((place("ira").deferred, place("ira").basis), (true, crate::Basis::Zero));
        assert_eq!((place("roth").deferred, place("roth").basis), (true, crate::Basis::Cost));
        assert_eq!(place("gifted").basis, crate::Basis::Zero);
        assert!(place("owed").claim && !place("ira").claim);
        assert_eq!(book.kinds[book.kind("roth").unwrap()].basis, Some(crate::Basis::Cost));
    });
}

#[test]
fn a_place_that_holds_one_commodity_still_takes_a_flow_into_its_basis() {
    let text = "
commodity HOME : stock
account assets/house
  holds HOME
2026-04-01 checking -> house.basis 100 USD
2026-04-02 checking -> house 100 USD
2026-04-03 house.basis 40 USD -> checking
";
    with_book(text, |_, diags| {
        assert_eq!(codes(diags), ["not-held"], "only the flow that brings dollars into the house: {diags:?}");
    });
}

#[test]
fn a_kind_of_commodity_says_how_its_parcels_are_relieved() {
    let text = "
kind money : currency
  select lifo
kind legal-tender : commodity
  select fifo
kind good : commodity
commodity EUR : currency
commodity CHF : money
commodity GLD : good
commodity BRL : legal-tender
";
    with_book(text, |book, diags| {
        assert!(diags.is_empty(), "{diags:?}");
        let select = |symbol: &str| book.commodities[book.commodity(symbol).unwrap()].select;
        assert_eq!((select("EUR"), select("BRL"), select("GLD")), (None, Some(crate::Policy::Fifo), None));
        assert_eq!(select("CHF"), Some(crate::Policy::Lifo), "a kind may say otherwise than the one it comes from");
    });
}

#[test]
fn the_built_in_places_and_kinds_resolve_as_names() {
    let text = "
2026-01-31 savings = 1 USD via market
2026-01-05 opening -> savings 10 USD
2026-01-06 ? -> savings 5 USD
";
    with_book(text, |book, diags| {
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(book.places[book.roots.unknown].path, book.names.get("equity/unknown").unwrap());
        assert_eq!(book.places[book.roots.opening].path, book.names.get("equity/opening").unwrap());
        assert_eq!(book.kinds[book.places[book.place("market").unwrap()].kind].name, book.names.get("market").unwrap());
    });
}

#[test]
fn a_member_belongs_to_another_entity_and_residences_may_overlap() {
    let files = [
        ("systems/us.ax", "system us/ca\n"),
        ("systems/de.ax", "system de\n"),
        (
            "axiom.ax",
            "
base USD
entity family : household
  lives us
entity alex : person
  member family
  lives de from 2025-07-01
  lives us/ca until 2025-06-30
entity ghost : person
  member nobody
entity narcissus : person
  member narcissus
",
        ),
    ];
    with_files(&files, |book, diags| {
        assert_eq!(codes(diags), ["unknown-entity", "member-self"], "{diags:?}");
        let (family, alex) = (book.entity("family").unwrap(), &book.entities[book.entity("alex").unwrap()]);
        assert_eq!(alex.member, Some(family));
        let path = |system: axiom_core::Id<crate::System>| book.name(book.systems[system].path);
        let lives: Vec<_> = alex.lives.iter().map(|res| (res.days.first() == Day::MIN, path(res.system))).collect();
        assert_eq!(lives, [(true, "us/ca"), (false, "de")], "sorted by their start, not chained");
        assert_eq!(alex.lives[0].days.last().to_string(), "2025-06-30");
        assert_eq!(alex.lives[1].days.last(), Day::MAX);
    });
}

#[test]
fn a_law_that_reads_a_tally_runs_after_the_laws_that_count_into_it() {
    let text = "
law reads
  on in
  require tally(base-total) <= 10 USD

law counts
  on in
  count amount as base-total
";
    with_book(text, |book, diags| {
        assert!(diags.is_empty(), "{diags:?}");
        let checking = book.place("checking").unwrap();
        let order: Vec<&str> =
            book.rules.on_in[checking].iter().map(|rule| book.name(book.laws[rule.law].name)).collect();
        assert_eq!(order, ["counts", "reads"], "the count comes first though it is written second");
    });
}

#[test]
fn a_tally_is_read_for_another_year_by_a_number_or_a_date_and_by_nothing_else() {
    let law = |read: &str| {
        let reads = format!("law reads\n  each year\n  count {read} as before\n");
        format!("law counts\n  on in\n  count amount as base-total\n\n{reads}")
    };
    for read in ["tally(base-total)", "tally(base-total, year - 1)", "tally(base-total, date(2025, 1, 1))"] {
        with_book(&law(read), |_, diags| assert!(diags.is_empty(), "{read}: {diags:?}"));
    }
    with_book(&law("tally(base-total, 5 USD)"), |_, diags| assert_eq!(codes(diags), ["type-mismatch"]));
    with_book(&law("tally(base-total, year, year)"), |_, diags| assert_eq!(codes(diags), ["call-arity"]));
}

#[test]
fn a_closing_judges_a_year_on_its_day_of_the_next() {
    let day = |year, month, day| axiom_core::Day::from_ymd(year, month, day);
    let april = crate::Closing { month: 4, day: 15 };
    assert_eq!(april.day_for(2025), day(2026, 4, 15));
    // A February 29 closing falls on the 28th in a year that has no 29th.
    let leap_day = crate::Closing { month: 2, day: 29 };
    assert_eq!(leap_day.day_for(2025), day(2026, 2, 28));
    assert_eq!(leap_day.day_for(2027), day(2028, 2, 29));
}

#[test]
fn only_a_law_that_is_one_cap_on_a_total_is_a_cap() {
    let text = "
law yearly
  on in
  warn total(in, year) < 9_000 USD

law filtered
  on in
  when amount > 1 USD
  warn total(in, month) <= 500 USD

law priced
  on in
  require total(in, month) <= 500 USD else owe 5 USD to acme

law computed
  on in
  warn total(in, month) <= 400 USD + 100 USD
";
    with_book(text, |book, diags| {
        assert!(diags.is_empty(), "{diags:?}");
        let cap = |name: &str| book.law(name).ok().and_then(|id| book.laws[id].cap());
        let yearly = cap("yearly").expect("a cap");
        assert_eq!((yearly.dir, yearly.window, yearly.strict), (crate::Dir::In, crate::Window::Year, true));
        assert_eq!(book.show(yearly.limit).to_string(), "9,000 USD");
        for other in ["filtered", "priced", "computed"] {
            assert!(cap(other).is_none(), "{other} says more than a cap");
        }
    });
}

#[test]
fn laws_that_need_each_other_are_a_cycle_naming_both() {
    let text = "
law first
  on in
  require tally(x) <= 10 USD
  count amount as y

law second
  on in
  require tally(y) <= 10 USD
  count amount as x
";
    with_book(text, |book, diags| {
        assert_eq!(codes(diags), ["law-cycle"], "{diags:?}");
        let shown = format!("{:?}", diags[0]);
        assert!(shown.contains("first") && shown.contains("second"), "{shown}");
        assert!(book.law("first").is_ok(), "the laws stay; only their order is undecided");
    });
}

#[test]
fn a_commodity_one_letter_from_a_known_one_is_a_typo_but_a_new_one_is_a_commodity() {
    with_book("2026-01-05 checking -> savings 10 UDS\n2026-01-06 checking -> brokerage 7 VTI\n", |book, diags| {
        assert_eq!(codes(diags), ["unknown-commodity"], "{diags:?}");
        assert_eq!(diags[0].help[0].text, "did you mean `USD`?");
        assert!(book.commodity("UDS").is_none() && book.commodity("VTI").is_some());
        assert_eq!(book.flows.len(), 1);
    });
}

#[test]
fn a_full_path_one_letter_from_a_declared_place_is_a_typo_but_a_new_path_opens() {
    let text = "
2026-01-05 checking -> expenses/fod 10 USD
2026-01-06 checking -> expenses/travel/taxis 10 USD
";
    with_book(text, |book, diags| {
        assert_eq!(codes(diags), ["unknown-place"], "{diags:?}");
        assert_eq!(diags[0].help[0].text, "did you mean `expenses/food`?");
        assert!(book.place("expenses/fod").is_err());
        assert!(book.place("expenses/travel/taxis").is_ok());
        assert_eq!(book.flows.len(), 1);
    });
}

#[test]
fn an_event_dated_before_its_flow_is_an_error() {
    let text = "
2026-01-06 checking -> savings 1 USD #a
2026-01-05 #a settled
2026-01-07 #a settled
2026-01-07 #nothing settled
";
    with_book(text, |book, diags| {
        assert_eq!(codes(diags), ["event-before-flow", "unknown-code"], "{diags:?}");
        assert_eq!(book.events.len(), 1, "the one dated after its flow is kept");
    });
}

#[test]
fn the_base_is_the_one_currency_used_and_several_need_a_base() {
    let one = [(
        "axiom.ax",
        "account assets/bank : bank\naccount assets/other : bank\n2026-01-05 bank 5 EUR -> other 5 EUR\n",
    )];
    with_files(&one, |book, diags| {
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(book.name(book.commodities[book.base].symbol), "EUR");
    });
    let two = [(
        "axiom.ax",
        "account assets/bank : bank\naccount assets/other : bank\n2026-01-05 bank 5 EUR -> other 6 USD\n",
    )];
    with_files(&two, |_, diags| {
        assert_eq!(codes(diags), ["no-base"], "{diags:?}");
        assert!(diags[0].message.contains("`EUR` and `USD`"), "{}", diags[0].message);
    });
}

#[test]
fn a_code_rule_takes_several_globs_and_is_met_by_any_leg() {
    let text = "
code trip-* holiday
  on expenses/food | expenses/travel/*
2026-01-05 acme -> 100 USD #trip-1
  expenses/food  40 USD
  checking       ...
";
    with_book(text, |book, diags| {
        assert!(diags.is_empty(), "{diags:?}");
        let patterns: Vec<&str> = book.codes.iter().map(|rule| book.name(rule.pattern)).collect();
        assert_eq!(patterns, ["trip-*", "holiday"]);
        assert!(book.codes.iter().all(|rule| rule.on.len() == 2));
        let code = book.names.get("trip-1").unwrap();
        assert!(book.flows.iter().all(|(_, flow)| flow.codes.contains(&code)), "a header code marks every leg");
    });
}

#[test]
fn each_year_closing_is_a_timed_law_with_its_closing_day() {
    let text = "
law estimated
  each year closing 04-15
  count 1 USD as paid
";
    with_book(text, |book, diags| {
        assert!(diags.is_empty(), "{diags:?}");
        let law = &book.laws[book.law("estimated").unwrap()];
        assert_eq!(law.trigger, crate::Trigger::Each(crate::Period::Year, Some(crate::Closing { month: 4, day: 15 })));
    });
}

/// The rules a place answers to on its inflows: law, subject, and the days.
fn inflow_rules(book: &Book, place: &str) -> Vec<String> {
    let place = book.place(place).unwrap();
    let subject = |subject| match subject {
        crate::Subject::Entity(entity) => book.name(book.entities[entity].path),
        crate::Subject::Place(place) => book.name(book.places[place].path),
        crate::Subject::Asset(asset) => book.name(book.assets[asset].name),
    };
    let day = |day: axiom_core::Day| match day.0 {
        i32::MIN => "..".to_string(),
        i32::MAX => "..".to_string(),
        _ => day.to_string(),
    };
    let rules = &book.rules.on_in[place];
    rules
        .iter()
        .map(|rule| {
            let law = book.name(book.laws[rule.law].name);
            format!("{law} for {} {}~{}", subject(rule.subject), day(rule.days.first()), day(rule.days.last()))
        })
        .collect()
}

const COUNTRIES: [(&str, &str); 2] = [
    ("systems/us.ax", "system us\nlaw us-law\n  on in\n  count amount as inflow\n"),
    ("systems/de.ax", "system de\nlaw de-law\n  on in\n  count amount as inflow\n"),
];

fn in_countries<R>(project: &str, then: impl FnOnce(&mut Book, &[Diagnostic]) -> R) -> R {
    let project = format!("base USD\naccount assets/bank/joint : bank\naccount assets/bank/alex : bank\n{project}");
    let files = [COUNTRIES[0], COUNTRIES[1], ("axiom.ax", project.as_str())];
    with_files(&files, then)
}

#[test]
fn a_household_is_governed_as_one_with_its_members_places() {
    let text = "
entity family : household
  lives us
entity alex : person
  member family
";
    let text = format!("{text}account assets/bank/joint2 : bank\n  owner family\n");
    in_countries(&text.replace("joint2", "shared"), |book, diags| {
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(inflow_rules(book, "shared"), ["us-law for family ..~.."]);
        // `alex` owns nothing yet: his place is owned by `me`.
        assert!(inflow_rules(book, "alex").is_empty());
    });
    let members = "
entity family : household
  lives us
entity alex : person
  member family
account assets/bank/mine : bank
  owner alex
";
    in_countries(members, |book, diags| {
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(inflow_rules(book, "mine"), ["us-law for family ..~.."], "the household is the subject");
    });
}

#[test]
fn a_member_who_lives_elsewhere_is_also_governed_there_as_themselves() {
    let text = "
entity family : household
  lives us
entity alex : person
  member family
  lives de
account assets/bank/mine : bank
  owner alex
";
    in_countries(text, |book, diags| {
        assert!(diags.is_empty(), "{diags:?}");
        let mut rules = inflow_rules(book, "mine");
        rules.sort();
        assert_eq!(rules, ["de-law for alex ..~..", "us-law for family ..~.."]);
    });
}

#[test]
fn rules_are_dated_by_residence_and_overlapping_residences_merge() {
    let text = "
entity me : person
  lives us from 2025-01-01 until 2025-06-30
  lives us from 2025-06-01 until 2025-12-31
  lives de from 2026-01-01
account assets/bank/mine : bank
";
    in_countries(text, |book, diags| {
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(
            inflow_rules(book, "mine"),
            ["de-law for me 2026-01-01~..", "us-law for me 2025-01-01~2025-12-31"],
            "in declaration order: systems are read in path order"
        );
    });
}

#[test]
fn a_kind_that_never_reaches_a_root_is_reported_once_and_not_at_every_use() {
    let text = "
kind loop-a : loop-b
kind loop-b : loop-a
kind orphan : nosuch
kind under-orphan : orphan
  law inside
    on in
    count amount as n
account assets/x : loop-a
account assets/y : orphan
account assets/z : under-orphan
";
    with_book(text, |book, diags| {
        let mut found = codes(diags);
        found.sort();
        assert_eq!(found, ["kind-cycle", "unknown-kind"], "{diags:?}");
        assert!(book.place("x").is_ok() && book.place("z").is_ok(), "the accounts exist, with their root kind");
    });
}

#[test]
fn a_law_with_a_line_that_did_not_parse_adds_nothing_to_that_error() {
    let text = format!(
        "{ACCOUNTS}
law damaged
  on in
  let cap = if amount > 5 USD then 5 USD
  require amount <= cap
"
    );
    let parsed = [("std.ax", STD, true), ("axiom.ax", text.as_str(), false)].map(|(path, text, embedded)| {
        let (file, syntax) = parse(FileId(embedded as u16), text);
        (Source { path, file, embedded }, syntax.len())
    });
    assert_eq!(parsed[1].1, 1, "the parser reports the incomplete `if`");
    let sources: Vec<_> = parsed.into_iter().map(|(source, _)| source).collect();
    let (book, diags) = build(&sources);
    assert!(diags.is_empty(), "nothing follows from the missing line: {diags:?}");
    assert!(book.law("damaged").is_err(), "and the law is left out rather than run without its `let`");
}

#[test]
fn a_header_waiver_covers_every_leg_with_one_location() {
    let text = "
2026-01-13 acme -> 5_200 USD ! \"late\"
  savings   800 USD
  checking  ...
2026-01-14 acme -> 100 USD
  savings   40 USD ! \"only this leg\"
  checking  ...
";
    with_book(text, |book, diags| {
        assert!(diags.is_empty(), "{diags:?}");
        let flows = flows_of(book);
        let waived: Vec<_> = flows.iter().map(|flow| flow.waive.map(|waive| waive.loc)).collect();
        assert!(waived[0].is_some() && waived[0] == waived[1], "one `!` on the header, one location: {waived:?}");
        assert!(waived[2].is_some() && waived[3].is_none(), "a leg's own `!` covers that leg alone: {waived:?}");
        let txns: Vec<_> = book.txns.iter().map(|(_, txn)| txn.waive.is_some()).collect();
        assert_eq!(txns, [true, false]);
        assert_ne!(waived[0], waived[2]);
    });
}

#[test]
fn a_leg_names_its_own_counterparty_and_due_day_over_the_headers() {
    let text = "
entity aldi : grocer
2026-03-01 checking -> 300 USD / acme #x due 2026-03-08
  food     100 USD
  savings  100 USD / aldi
  aldi     100 USD due 30d
2026-03-02 -> food 100 USD / aldi
  savings   40 USD
  acme      ...
";
    with_book(text, |book, diags| {
        assert!(diags.is_empty(), "{diags:?}");
        let name = |flow: &crate::Flow| flow.payee.map(|entity| book.name(book.entities[entity].path));
        let flows = flows_of(book);
        let due = |flow: &crate::Flow| flow.detail().due.map(|day| day.to_string());
        let says: Vec<_> = flows.iter().map(|flow| (name(flow), due(flow))).collect();
        assert_eq!(
            says,
            [
                (Some("acme"), Some("2026-03-08".into())),
                (Some("aldi"), Some("2026-03-08".into())),
                (Some("aldi"), Some("2026-03-31".into())),
                (Some("aldi"), None),
                (Some("aldi"), None),
            ],
            "a leg's own payee, else the entity it pays, else the header's; and its own due"
        );
    });
}

#[test]
fn an_occurrence_may_only_override_legs_the_plan_has() {
    let text = "
plan paycheck every 2w from 2026-01-02 acme -> 5_200 USD
  savings   800 USD
  checking  ...
2026-01-16 paycheck
  brokerage  100 USD
";
    with_book(text, |book, diags| {
        assert_eq!(codes(diags), ["plan-leg"], "{diags:?}");
        assert!(diags[0].notes[0].contains("savings"), "{:?}", diags[0].notes);
        assert!(book.flows.is_empty(), "the occurrence adds no flows when one of its lines is wrong");
    });
}

#[test]
fn a_filter_on_the_trigger_and_the_basis_and_unit_fields_type_check() {
    let text = "
law from-payer
  on in from acme | savings
  when amount.unit == USD
  require from.basis >= empty

law bad-filter
  on in from acme
  when amount.unit == 5%
  require amount > empty
";
    with_book(text, |book, diags| {
        assert_eq!(codes(diags), ["type-mismatch"], "{diags:?}");
        assert!(book.law("from-payer").is_ok() && book.law("bad-filter").is_err());
    });
}

#[test]
fn a_flow_that_breaks_an_opening_closing_or_holding_rule_shows_the_line_that_says_it() {
    let text = "
account assets/old : bank
  opened 2025-01-01
  closed 2025-12-31
account assets/wallet
  holds VTI
2026-03-20 checking -> old 5 USD
2026-03-21 checking -> wallet 5 USD
";
    with_book(text, |book, diags| {
        assert_eq!(codes(diags), ["place-closed", "not-held"], "{diags:?}");
        for diagnostic in diags {
            let context = diagnostic.labels.iter().find(|label| !label.primary).expect("the rule's own line");
            assert_ne!(context.loc, diagnostic.labels[0].loc);
        }
        assert!(diags[0].message.contains("closed on 2025-12-31") && diags[1].message.contains("only holds `VTI`"));
        assert!(book.flows.is_empty());
    });
}

/// Each way of being refused, the smallest text that does it, and the one code
/// it must be reported under: exactly once, with the primary label in the
/// project's own file.
const REFUSALS: [(&str, &str); 44] = [
    ("account-root", "account nonsense/x\n"),
    ("all-target", "2026-01-05 checking ->\n  savings all USD\n"),
    ("all-unknown", "2026-01-05 checking all USD -> savings ? USD\n"),
    ("basis-both", "2026-01-05 checking.basis -> savings.basis 5 USD\n"),
    ("call-keyword", "law l\n  on in\n  require total(sideways, month) <= 5 USD\n"),
    ("closed-before-opened", "account assets/odd\n  opened 2026-05-01\n  closed 2026-01-01\n"),
    ("duplicate-base", "base EUR\n"),
    ("duplicate-row", "param p\n  2026 5 USD\n  2026 6 USD\n"),
    ("kind-parent", "kind orphan\n"),
    ("law-trigger", "law l\n  on spend\n  require amount <= 5 USD\n"),
    ("missing-amount", "2026-01-05 checking -> savings\n"),
    ("not-constant", "param p\n  2026 5 USD + 1 USD\n"),
    ("opening-place", "opening 2026-01-01\n  checking[fifo] 5 USD\n"),
    ("param-key-order", "param p\n  single 2026 5 USD\n"),
    ("param-lookup", "law l\n  on in\n  require limit[2026, 2026] <= 5 USD\nparam limit\n  2026 5 USD\n"),
    ("param-shape", "param p\n  2026 5 USD\n  2026 single 6 USD\n"),
    ("param-type", "param p\n  2026 5 USD\n  2027 6%\n"),
    ("price-inferred", "2026-01-05 checking ? USD -> brokerage 3 VTI @ 300 USD\n"),
    ("price-needs-two", "2026-01-05 checking -> savings 5 USD @ 3 USD\n"),
    ("price-on-transfer", "2026-01-05 checking 5 USD -> savings 5 USD @ 1 USD\n"),
    ("price-self", "2026-01-05 USD 1 USD\n"),
    ("price-unit", "2026-01-05 checking 5 USD -> brokerage 3 VTI @ 3 EUR\n"),
    ("price-vanishes", "commodity USD\n  precision 2\n2026-01-05 checking -> brokerage 1 VTI @ 0.001 USD\n"),
    ("property-type", "account assets/odd\n  opened yes\n"),
    ("reserved-property", "kind x : asset\n  has balance date\n"),
    ("residence-order", "entity someone\n  lives std from 2026-01-01 until 2025-01-01\n"),
    ("schedule-order", "param p\n  2026 0 USD 10% | 0 USD 12%\n"),
    ("schedule-threshold", "param p\n  2026 5 10%\n"),
    ("schedule-unit", "param p\n  2026 0 USD 10% | 100 EUR 12%\n"),
    ("selector-target", "2026-01-05 checking -> savings[fifo] 5 USD\n"),
    ("split-commodity", "2026-01-05 acme ->\n  checking ...\n"),
    ("split-inferred", "2026-01-05 acme -> 100 USD\n  checking ...\n  savings ? USD\n"),
    ("split-price", "2026-01-05 acme -> 100 USD\n  checking 5 USD\n  brokerage 2 VTI\n  brokerage 3 VXUS\n"),
    ("split-rest", "2026-01-05 acme -> 100 USD\n  checking ...\n  brokerage 2 VTI\n"),
    (
        "split-rest",
        "plan p every 2w from 2026-01-02 acme -> 100 USD\n  savings 40 USD\n  checking ...\n2026-01-16 p\n  savings ...\n",
    ),
    ("split-total", "2026-01-05 acme -> ? USD\n  checking ...\n"),
    ("unknown-exchange", "2026-01-05 checking ? USD -> brokerage ? VTI\n"),
    ("empty-amount", "2026-01-05 checking -> savings empty\n"),
    ("zero-flow", "2026-01-05 checking -> savings 0 USD\n"),
    ("price-zero", "2026-01-05 checking -> brokerage 3 VTI @ 0 USD\n"),
    ("amount-precision", "commodity USD\n  precision 2\n2026-01-05 checking -> savings 1.005 USD\n"),
    ("unknown-code", "2026-01-05 #nothing settled\n"),
    ("bad-split", "2026-01-05 VTI split 99999999999999999 for 0.00000000000000001\n"),
    ("amount-range", "commodity USD\n  precision 2\n2026-01-05 checking -> savings 99999999999999999 USD\n"),
];

#[test]
fn every_refusal_is_reported_once_under_its_own_code_at_the_users_line() {
    let mut wrong = Vec::new();
    for (code, text) in REFUSALS {
        let text = format!("{ACCOUNTS}\n{text}");
        let parsed = [("std.ax", STD, true), ("axiom.ax", text.as_str(), false)].map(|(path, text, embedded)| {
            let (file, syntax) = parse(FileId(1 - embedded as u16), text);
            (Source { path, file, embedded }, syntax)
        });
        if let Some(syntax) = parsed.iter().flat_map(|(_, syntax)| syntax).next() {
            wrong.push(format!("{code}: the parser stops it first, as {}", syntax.code));
            continue;
        }
        let sources: Vec<_> = parsed.into_iter().map(|(source, _)| source).collect();
        let (_, diags) = build(&sources);
        let found = codes(&diags);
        let project =
            |diagnostic: &Diagnostic| diagnostic.labels.iter().find(|label| label.primary).map(|l| l.loc.file);
        if found != [code] {
            wrong.push(format!("{code}: reported as {found:?}"));
        } else if project(&diags[0]) != Some(FileId(1)) {
            wrong.push(format!("{code}: not at a line of the project"));
        }
    }
    assert!(wrong.is_empty(), "\n{}", wrong.join("\n"));
}

#[test]
fn a_system_may_only_hold_what_systems_hold_and_is_defined_once() {
    let bank = "account assets/bank : bank\n";
    let cases: [(&str, &[(&str, &str)]); 3] = [
        ("system-item", &[("systems/a.ax", "system a\naccount assets/x\n"), ("axiom.ax", bank)]),
        ("duplicate-system", &[("systems/a.ax", "system a\n"), ("systems/b.ax", "system a\n"), ("axiom.ax", bank)]),
        ("law-position", &[("axiom.ax", "commodity X\n  law l\n    on in\n    require amount <= 5 USD\n")]),
    ];
    for (code, files) in cases {
        with_files(files, |_, diags| {
            assert_eq!(codes(diags), [code], "{diags:?}");
            let primary = diags[0].labels.iter().find(|label| label.primary).unwrap();
            assert_ne!(primary.loc.file, FileId(0), "{code}: the built-in std is never the primary");
        });
    }
}

// v3 bridge: what the v4 types make of a v3 book.
#[test]
fn a_v3_book_fits_the_v4_types() {
    use crate::{Class, Origin, Role};
    with_book("2026-01-05 checking -> food 10 USD\n", |book, diags| {
        assert!(diags.is_empty(), "{diags:?}");
        let place = |path| &book.places[book.place(path).unwrap()];
        let account = Role::Account { institution: None };
        assert_eq!((place("checking").class, place("checking").role), (Class::Asset, account));
        assert_eq!((place("visa").class, place("visa").role), (Class::Debt, account));
        assert_eq!((place("food").class, place("food").role), (Class::Outside, Role::Outside(None)));
        assert_eq!(place("equity/unknown").class, Class::Outside);

        let roots = book.roots;
        assert_eq!(book.name(book.entities[roots.market].path), "market");
        assert_eq!(book.entities[roots.market].place, book.place("income/market").ok());
        assert!(book.entity("market").is_err(), "no line names the market entity: `market` is the place");
        assert_eq!(book.name(book.kinds[roots.thing].name), "thing");
        let purposes = [roots.income, roots.spending, roots.capital].map(|root| book.name(book.purposes[root].name));
        assert_eq!(purposes, ["income", "spending", "capital"]);

        // The flow is its account's owner's, and nothing says what it is for.
        let flow = &book.flows[axiom_core::Id::new(0)];
        assert_eq!((flow.owner, flow.origin, flow.purpose), (roots.me, Origin::Written, None));

        // What the v3 model cannot say is empty or default: an owner counts in the base, on cash books.
        let me = &book.entities[roots.me];
        assert_eq!((me.currency, me.books), (book.base, crate::Books::Cash));
        assert!(me.owned_by.is_empty() && me.citizen.is_empty() && me.known_as.is_empty());
        assert!(place("checking").shares.is_empty() && place("checking").known_as.is_empty());
        let (promised, derived) = (book.contracts.len() + book.also.len(), book.budgets.len() + book.patterns.len());
        assert_eq!((promised, derived, book.formats.len()), (0, 0, 0));
        assert_eq!((book.measures.len(), book.readings.len(), book.filed.len()), (0, 0, 0));
        assert!(book.laws.values().all(|law| law.overrides.is_none() && law.rank == crate::Rank(0)));
    });
}

/// What a contract's terms need to be built by hand: one flow to be the template.
fn terms(every: crate::Cadence, on: &[crate::On], anchor: axiom_core::Day, template: &[crate::Flow]) -> crate::Terms {
    crate::Terms {
        state: crate::TermsState::Active,
        every,
        on: on.into(),
        anchor,
        template: template.into(),
        inputs: Box::default(),
        estimate: false,
        due: None,
        grace: axiom_core::Span::default(),
        period: None,
        covers: None,
        prorated: false,
        escalation: None,
        shares: Box::default(),
        also: Box::default(),
        rate: None,
        change: None,
    }
}

fn contract(days: axiom_core::Days, terms: axiom_core::Timeline<crate::Terms>) -> crate::Contract {
    let loc = axiom_core::Loc::new(FileId(0), 0, 0);
    crate::Contract {
        name: axiom_core::Interner::default().intern("rent"),
        party: axiom_core::Id::new(0),
        owner: axiom_core::Id::new(0),
        days,
        terms,
        buys: None,
        deposit: None,
        loan: None,
        matching: None,
        ended: None,
        laws: Box::default(),
        doc: None,
        loc,
    }
}

// v3 bridge: `sync FILE` with its `run COMMAND` is a source whose sink is that file.
#[test]
fn a_v3_sync_is_a_source_that_merges_into_its_file() {
    use crate::sync::{Fetch, Sink};
    with_book("sync prices/2026.ax\n  run ./scripts/quotes.py VTI BND\n", |book, diags| {
        assert!(diags.is_empty(), "{diags:?}");
        let [source] = &book.sources[..] else { panic!("one source") };
        assert!(matches!(source.fetch, Fetch::Run(command) if book.name(command) == "./scripts/quotes.py VTI BND"));
        assert!(matches!(source.sink, Sink::File(file) if book.name(file) == "prices/2026.ax"));
        assert_eq!(book.name(source.name), "prices/2026.ax");
        assert_eq!((source.doc, source.format, source.system), (None, None, None));
    });
}

#[test]
fn a_contract_falls_due_by_the_terms_in_force_over_its_days() {
    use crate::{Cadence, ForecastError, On};
    use axiom_core::{Day, Days, Span, Timeline};
    let day = |month, day| Day::from_ymd(2026, month, day).unwrap();
    let days = |from: (u32, u32), to: (u32, u32)| Days::new(day(from.0, from.1), day(to.0, to.1)).unwrap();
    let (monthly, fortnightly) = (Cadence::Every(Span::months(1)), Cadence::Every(Span::days(14)));
    with_book("2026-01-05 checking -> food 10 USD\n", |book, _| {
        let template = [book.flows[axiom_core::Id::new(0)].clone()];
        // A month's end does not drag the months after it, and `within` cuts the start off.
        let rent = contract(Days::ALWAYS, Timeline::new(terms(monthly, &[], day(1, 31), &template)));
        assert_eq!(rent.due_days(days((2, 1), (4, 30))), [day(2, 28), day(3, 31), day(4, 30)]);
        // The contract's own days bound it, and nothing falls due before its first.
        let mut mortgage_terms = terms(monthly, &[On::MonthDay(15)], day(1, 1), &[]);
        mortgage_terms.rate = Some(axiom_core::Ratio::percent(5, 0).unwrap());
        let mut mortgage = contract(Days::ALWAYS, Timeline::new(mortgage_terms));
        mortgage.loan = Some(crate::Loan {
            principal: crate::Amount::new(axiom_core::Qty(100_000), book.base),
            on: day(1, 1),
            term: Span::months(360),
            asset: None,
            debt: axiom_core::Id::new(0),
            resets: None,
            prepay: crate::Prepay::default(),
        });
        assert_eq!(
            mortgage.occurrences(days((2, 1), (3, 31))).map(|occurrence| occurrence.day).collect::<Vec<_>>(),
            [day(2, 15), day(3, 15)],
            "a loan's empty explicit template does not waive its payment schedule"
        );
        let mortgage_id = book.contracts.push(mortgage);
        assert_eq!(
            book.contracts[mortgage_id].forecast_flows(book, mortgage_id, days((2, 1), (2, 28))).next(),
            Some(Err(ForecastError::UnsupportedLoan(day(2, 15)))),
            "a scheduled loan is not silently forecast without its payment derivation"
        );
        let on_the_15th = terms(monthly, &[On::MonthDay(15)], day(1, 31), &template);
        let short = contract(days((1, 31), (3, 20)), Timeline::new(on_the_15th));
        assert_eq!(short.due_days(Days::ALWAYS), [day(2, 15), day(3, 15)]);
        assert_eq!(short.due_days(days((4, 1), (4, 30))), Vec::<Day>::new(), "it ended before");
        // A statement changes the terms from its day: each stretch steps on its own schedule.
        let mut changing = Timeline::new(terms(monthly, &[], day(1, 1), &template));
        changing.paint(Days::new(day(3, 1), Day::MAX).unwrap(), terms(fortnightly, &[], day(3, 1), &template));
        let changing = contract(Days::ALWAYS, changing);
        assert_eq!(changing.terms_on(day(2, 1)).every, monthly);
        assert_eq!(changing.terms_on(day(3, 1)).every, fortnightly);
        assert_eq!(
            changing.due_days(days((1, 1), (4, 30))),
            [day(1, 1), day(2, 1), day(3, 1), day(3, 15), day(3, 29), day(4, 12), day(4, 26)]
        );
        // A waived stretch expects nothing, and the terms before it resume after it.
        let mut waived = Timeline::new(terms(monthly, &[], day(1, 1), &template));
        let mut waiver = terms(monthly, &[], day(1, 1), &[]);
        waiver.state = crate::TermsState::Waived;
        waived.paint(days((2, 1), (2, 28)), waiver);
        let waived = contract(Days::ALWAYS, waived);
        assert!(waived.terms_on(day(2, 10)).is_waived() && !waived.terms_on(day(3, 1)).is_waived());
        assert_eq!(waived.due_days(days((1, 1), (4, 1))), [day(1, 1), day(3, 1), day(4, 1)]);
    });
}

#[test]
fn a_contract_forecasts_typed_flows_and_reports_bad_escalation_inputs() {
    use crate::{Amount, Cadence, Escalation, ForecastError, On, Param, ParamRow, TermsState, Value};
    use axiom_core::{Day, Days, Ratio, Span, Timeline};
    let day = |year, month, day| Day::from_ymd(year, month, day).unwrap();
    with_book("2026-01-05 checking -> food 1_000 USD\n", |book, _| {
        let template = book.flows[axiom_core::Id::new(0)].clone();
        let first = day(2026, 1, 15);
        let after_one_year = day(2027, 1, 15);
        let end = day(2028, 1, 15);
        let days = Days::new(first, end).unwrap();

        let mut rising = terms(
            Cadence::Every(Span::months(1)),
            &[On::MonthDay(15)],
            first,
            &[template.clone()],
        );
        rising.escalation = Some(Escalation::Rising(Ratio::percent(3, 0).unwrap()));
        let rising_id = book.contracts.push(contract(days, Timeline::new(rising)));
        let projected: Vec<_> = book.contracts[rising_id]
            .forecast_flows(book, rising_id, Days::on(after_one_year))
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(projected.len(), 1);
        assert_eq!(projected[0].day, after_one_year);
        assert_eq!(
            projected[0].out.qty,
            axiom_core::Qty(1_030),
            "3% at the anniversary"
        );
        assert_eq!(projected[0].recognized, Days::on(after_one_year));
        assert_eq!(projected[0].mode, crate::Mode::Planned);
        assert_eq!(projected[0].origin, crate::Origin::Occurrence(rising_id));

        let index_id = book.params.push(Param {
            name: book.names.intern("cpi"),
            unit: None,
            system: None,
            rows: vec![
                ParamRow {
                    since: Some(first),
                    names: Box::default(),
                    value: Value::Num(Ratio::int(100)),
                    loc: axiom_core::Loc::default(),
                },
                ParamRow {
                    since: Some(day(2026, 7, 15)),
                    names: Box::default(),
                    value: Value::Num(Ratio::int(120)),
                    loc: axiom_core::Loc::default(),
                },
                ParamRow {
                    since: Some(after_one_year),
                    names: Box::default(),
                    value: Value::Num(Ratio::int(125)),
                    loc: axiom_core::Loc::default(),
                },
                ParamRow {
                    since: Some(day(2027, 7, 15)),
                    names: Box::default(),
                    value: Value::Num(Ratio::int(150)),
                    loc: axiom_core::Loc::default(),
                },
            ]
            .into(),
            loc: axiom_core::Loc::default(),
        });
        let mut indexed = terms(
            Cadence::Every(Span::months(1)),
            &[On::MonthDay(15)],
            first,
            &[template],
        );
        indexed.escalation = Some(Escalation::Indexed(index_id));
        let indexed_id = book.contracts.push(contract(days, Timeline::new(indexed)));
        assert_eq!(
            book.contracts[indexed_id].amount_on(book, after_one_year),
            Ok(Ratio::new(5, 4).unwrap())
        );
        assert_eq!(
            book.contracts[indexed_id].amount_on(book, day(2026, 8, 15)),
            Ok(Ratio::ONE),
            "index rows between anniversaries do not change a yearly escalation"
        );
        assert_eq!(
            book.contracts[indexed_id].amount_on(book, day(2027, 8, 15)),
            Ok(Ratio::new(5, 4).unwrap()),
            "the next index value applies only at the next anniversary"
        );

        let mut missing = terms(
            Cadence::Every(Span::months(1)),
            &[],
            first,
            &[book.flows[axiom_core::Id::new(0)].clone()],
        );
        missing.state = TermsState::Active;
        missing.escalation = Some(Escalation::Indexed(axiom_core::Id::new(99)));
        let missing_id = book.contracts.push(contract(days, Timeline::new(missing)));
        assert_eq!(
            book.contracts[missing_id].amount_on(book, after_one_year),
            Err(ForecastError::MissingIndex {
                param: axiom_core::Id::new(99),
                day: first
            })
        );
        assert_eq!(
            book.contracts[missing_id]
                .forecast_flows(book, missing_id, Days::on(after_one_year))
                .next(),
            Some(Err(ForecastError::MissingIndex {
                param: axiom_core::Id::new(99),
                day: first
            })),
            "an unavailable index is surfaced instead of becoming an absent forecast"
        );

        let mut no_template = terms(Cadence::Every(Span::months(1)), &[], first, &[]);
        no_template.state = TermsState::Active;
        let no_template_id = book
            .contracts
            .push(contract(days, Timeline::new(no_template)));
        assert_eq!(
            book.contracts[no_template_id]
                .forecast_flows(book, no_template_id, Days::on(after_one_year))
                .next(),
            Some(Err(ForecastError::MissingTemplate(after_one_year)))
        );

        let missing_name = book.names.intern("water");
        let mut needs_input = terms(
            Cadence::Every(Span::months(1)),
            &[],
            first,
            &[book.flows[axiom_core::Id::new(0)].clone()],
        );
        needs_input.inputs = vec![crate::Input {
            name: missing_name,
            unit: None,
            loc: axiom_core::Loc::default(),
        }]
        .into();
        let needs_input_id = book.contracts.push(contract(days, Timeline::new(needs_input)));
        assert_eq!(
            book.contracts[needs_input_id]
                .forecast_flows(book, needs_input_id, Days::on(after_one_year))
                .next(),
            Some(Err(ForecastError::MissingInput {
                input: missing_name,
                day: after_one_year,
            }))
        );

        let mut unresolved_flow = book.flows[axiom_core::Id::new(0)].clone();
        unresolved_flow.infer = crate::Infer::Unknown;
        let unresolved = terms(Cadence::Every(Span::months(1)), &[], first, &[unresolved_flow]);
        let unresolved_id = book.contracts.push(contract(days, Timeline::new(unresolved)));
        assert_eq!(
            book.contracts[unresolved_id]
                .forecast_flows(book, unresolved_id, Days::on(after_one_year))
                .next(),
            Some(Err(ForecastError::UnresolvedAmount(after_one_year)))
        );

        let mut detailed_template = book.flows[axiom_core::Id::new(0)].clone();
        detailed_template.detail = Some(Box::new(crate::Detail {
            basis: Some(axiom_core::Qty(1_000)),
            cost: Some(Amount::new(axiom_core::Qty(200), book.base)),
            due: Some(day(2026, 1, 20)),
            ..crate::Detail::default()
        }));
        let mut detailed_terms = terms(
            Cadence::Every(Span::months(1)),
            &[On::MonthDay(15)],
            first,
            &[detailed_template],
        );
        detailed_terms.escalation = Some(Escalation::Rising(Ratio::percent(3, 0).unwrap()));
        let detailed_id = book.contracts.push(contract(days, Timeline::new(detailed_terms)));
        let detailed = book.contracts[detailed_id]
            .forecast_flows(book, detailed_id, Days::on(after_one_year))
            .next()
            .unwrap()
            .unwrap();
        let detail = detailed.detail.as_deref().unwrap();
        assert_eq!(detail.basis, Some(axiom_core::Qty(1_030)));
        assert_eq!(detail.cost, Some(Amount::new(axiom_core::Qty(206), book.base)));
        assert_eq!(detail.due, Some(day(2027, 1, 30)));

        use crate::{Deadline, ForecastFeature, Match, Share};
        let mut unsupported_terms = terms(
            Cadence::Every(Span::months(1)),
            &[On::MonthDay(15)],
            first,
            &[book.flows[axiom_core::Id::new(0)].clone()],
        );
        let mut cases = Vec::new();
        unsupported_terms.due = Some(Deadline { after: Span::days(5), otherwise: None });
        cases.push((ForecastFeature::Deadline, contract(days, Timeline::new(unsupported_terms))));
        let mut unsupported_terms = terms(
            Cadence::Every(Span::months(1)),
            &[On::MonthDay(15)],
            first,
            &[book.flows[axiom_core::Id::new(0)].clone()],
        );
        unsupported_terms.shares = vec![Share {
            rate: Ratio::new(1, 2).unwrap(),
            entity: axiom_core::Id::new(0),
            measure: None,
            loc: axiom_core::Loc::default(),
        }]
        .into();
        cases.push((ForecastFeature::Shares, contract(days, Timeline::new(unsupported_terms))));
        let mut unsupported_terms = terms(
            Cadence::Every(Span::months(1)),
            &[On::MonthDay(15)],
            first,
            &[book.flows[axiom_core::Id::new(0)].clone()],
        );
        unsupported_terms.also = vec![axiom_core::Id::new(0)].into();
        cases.push((ForecastFeature::Also, contract(days, Timeline::new(unsupported_terms))));
        let mut buys = contract(
            days,
            Timeline::new(terms(
                Cadence::Every(Span::months(1)),
                &[On::MonthDay(15)],
                first,
                &[book.flows[axiom_core::Id::new(0)].clone()],
            )),
        );
        buys.buys = Some(axiom_core::Id::new(0));
        cases.push((ForecastFeature::Buy, buys));
        let mut deposit = contract(
            days,
            Timeline::new(terms(
                Cadence::Every(Span::months(1)),
                &[On::MonthDay(15)],
                first,
                &[book.flows[axiom_core::Id::new(0)].clone()],
            )),
        );
        deposit.deposit = Some(Amount::new(axiom_core::Qty(1_000), book.base));
        cases.push((ForecastFeature::Deposit, deposit));
        let mut matching = contract(
            days,
            Timeline::new(terms(
                Cadence::Every(Span::months(1)),
                &[On::MonthDay(15)],
                first,
                &[book.flows[axiom_core::Id::new(0)].clone()],
            )),
        );
        matching.matching = Some(Match {
            rate: Ratio::new(1, 2).unwrap(),
            into: axiom_core::Id::new(0),
            up_to: Ratio::ONE,
        });
        cases.push((ForecastFeature::Matching, matching));
        for (feature, contract) in cases {
            let id = book.contracts.push(contract);
            assert_eq!(
                book.contracts[id].forecast_flows(book, id, Days::on(after_one_year)).next(),
                Some(Err(ForecastError::UnsupportedFeature { feature, day: after_one_year }))
            );
        }
    });
}

#[test]
fn contracts_cover_matching_fallback_flows_by_interval_and_typed_identity() {
    use crate::{Cadence, ContractCoverage, TermsState};
    use axiom_core::{Day, Days, Span, Timeline};
    let day = |month, day| Day::from_ymd(2026, month, day).unwrap();
    with_book("2026-01-05 checking -> food 1_000 USD\n", |book, _| {
        let template = book.flows[axiom_core::Id::new(0)].clone();
        let start = day(2, 15);
        let end = day(4, 30);
        let terms = terms(
            Cadence::Every(Span::months(1)),
            &[crate::On::MonthDay(15)],
            start,
            &[template.clone()],
        );
        let mut timeline = Timeline::new(terms.clone());
        let mut changed_identity = terms;
        let mut other_movement = template.clone();
        other_movement.to = axiom_core::Id::new(99);
        changed_identity.template = vec![other_movement.clone()].into();
        timeline.paint(Days::new(day(3, 1), day(3, 9)).unwrap(), changed_identity);
        let mut waiver = timeline.at(day(3, 1)).clone();
        waiver.state = TermsState::Waived;
        waiver.template = Box::default();
        timeline.paint(Days::new(day(3, 10), day(3, 31)).unwrap(), waiver);
        let rent = contract(Days::new(start, end).unwrap(), timeline);

        assert_eq!(rent.covers(&template, day(2, 10)), ContractCoverage::None, "before it starts");
        assert_eq!(
            rent.covers(&template, day(2, 20)),
            ContractCoverage::Active,
            "coverage follows the interval, not the contract's monthly due day"
        );
        assert_eq!(
            rent.covers(&template, day(3, 15)),
            ContractCoverage::None,
            "the empty waiver does not inherit an older, no-longer-current movement"
        );
        assert_eq!(
            rent.covers(&other_movement, day(3, 15)),
            ContractCoverage::Waived,
            "an empty waiver inherits the nearest active movement"
        );
        assert_eq!(rent.covers(&template, day(5, 1)), ContractCoverage::None, "after it ends");

        let mut other_owner = template.clone();
        other_owner.owner = axiom_core::Id::new(99);
        assert_eq!(rent.covers(&other_owner, day(2, 20)), ContractCoverage::None);
        let mut other_payee = template.clone();
        other_payee.payee = Some(axiom_core::Id::new(99));
        assert_eq!(rent.covers(&other_payee, day(2, 20)), ContractCoverage::None);
        let mut other_purpose = template.clone();
        other_purpose.purpose = Some(crate::Purposed {
            purpose: axiom_core::Id::new(99),
            of: None,
            source: crate::Provenance::Written,
        });
        assert_eq!(rent.covers(&other_purpose, day(2, 20)), ContractCoverage::None);
        let mut other_unit = template;
        other_unit.out.unit = axiom_core::Id::new(99);
        assert_eq!(rent.covers(&other_unit, day(2, 20)), ContractCoverage::None);
    });
}

#[test]
fn contract_occurrences_use_calendar_recognition_windows_and_proration() {
    use crate::{Cadence, Coverage, Relative};
    use axiom_core::{Day, Days, Period, Ratio, Span, Timeline};
    let day = |year, month, day| Day::from_ymd(year, month, day).unwrap();
    with_book("2026-01-05 checking -> food 1_000 USD\n", |book, _| {
        let template = book.flows[axiom_core::Id::new(0)].clone();
        let every_month = Cadence::Every(Span::months(1));
        let feb15 = day(2024, 2, 15);
        let mar15 = day(2024, 3, 15);
        let mut previous_month = terms(every_month, &[crate::On::MonthDay(15)], feb15, &[template.clone()]);
        previous_month.period = Some(Relative::Last(Period::Month));
        let mut changed = Timeline::new(previous_month);
        let mut current_month = terms(
            every_month,
            &[crate::On::MonthDay(15)],
            day(2024, 4, 15),
            &[template.clone()],
        );
        current_month.covers = Some(Coverage::Calendar(Period::Month));
        changed.paint(Days::new(day(2024, 4, 15), Day::MAX).unwrap(), current_month);
        let rent = contract(Days::new(feb15, day(2024, 5, 15)).unwrap(), changed);
        assert_eq!(
            rent.recognition_on(&template, feb15),
            Ok(Days::new(day(2024, 1, 1), day(2024, 1, 31)).unwrap())
        );
        assert_eq!(
            rent.recognition_on(&template, mar15),
            Ok(Days::new(day(2024, 2, 1), day(2024, 2, 29)).unwrap()),
            "last-month rent in March recognizes the leap February"
        );

        let april15 = day(2024, 4, 15);
        assert_eq!(
            rent.recognition_on(&template, april15),
            Ok(Days::new(day(2024, 4, 1), day(2024, 4, 30)).unwrap()),
            "terms changes switch recognition to the new coverage"
        );
        let days = Days::new(april15, day(2025, 4, 15)).unwrap();
        let recognition = |relative, covers| {
            let mut terms = terms(every_month, &[crate::On::MonthDay(15)], april15, &[template.clone()]);
            terms.period = relative;
            terms.covers = covers;
            contract(days, Timeline::new(terms)).recognition_on(&template, april15)
        };
        assert_eq!(
            recognition(Some(Relative::LastQuarter), None),
            Ok(Days::new(day(2024, 1, 1), day(2024, 3, 31)).unwrap())
        );
        assert_eq!(
            recognition(None, Some(Coverage::Quarter)),
            Ok(Days::new(day(2024, 4, 1), day(2024, 6, 30)).unwrap())
        );
        assert_eq!(
            recognition(None, Some(Coverage::Span(Span::months(6)))),
            Ok(Days::new(april15, day(2024, 10, 14)).unwrap())
        );

        let feb10 = day(2024, 2, 10);
        let feb20 = day(2024, 2, 20);
        let mut prorated = terms(every_month, &[crate::On::MonthDay(15)], feb10, &[template.clone()]);
        prorated.covers = Some(Coverage::Calendar(Period::Month));
        prorated.prorated = true;
        let partial = contract(Days::new(feb10, feb20).unwrap(), Timeline::new(prorated));
        assert_eq!(partial.amount_on(book, day(2024, 2, 15)), Ok(Ratio::new(11, 29).unwrap()));
        assert_eq!(
            partial.recognition_on(&template, day(2024, 2, 15)),
            Ok(Days::new(day(2024, 2, 1), day(2024, 2, 29)).unwrap())
        );

        let mut unbounded_proration = terms(every_month, &[], feb10, &[template.clone()]);
        unbounded_proration.prorated = true;
        let unbounded_proration = contract(Days::new(feb10, feb20).unwrap(), Timeline::new(unbounded_proration));
        assert_eq!(
            unbounded_proration.amount_on(book, day(2024, 2, 15)),
            Err(crate::ForecastError::UnsupportedProration(day(2024, 2, 15)))
        );

        let mut conflicting = terms(every_month, &[], feb10, &[template.clone()]);
        conflicting.period = Some(Relative::Last(Period::Month));
        conflicting.covers = Some(Coverage::Calendar(Period::Month));
        let conflicting = contract(Days::new(feb10, feb20).unwrap(), Timeline::new(conflicting));
        assert_eq!(
            conflicting.recognition_on(&template, day(2024, 2, 15)),
            Err(crate::ForecastError::ConflictingRecognition(day(2024, 2, 15)))
        );

        let earliest = day(-999_999, 1, 15);
        let mut at_boundary = terms(every_month, &[], earliest, &[template.clone()]);
        at_boundary.period = Some(Relative::Last(Period::Year));
        let boundary = contract(Days::on(earliest), Timeline::new(at_boundary));
        assert_eq!(
            boundary.recognition_on(&template, earliest),
            Err(crate::ForecastError::Overflow),
            "the previous civil year is outside the supported calendar"
        );
    });
}

#[test]
fn contract_anniversaries_clamp_leap_days_without_drifting() {
    use crate::{Cadence, Escalation};
    use axiom_core::{Day, Days, Ratio, Span, Timeline};
    let day = |year, month, day| Day::from_ymd(year, month, day).unwrap();
    with_book("2026-01-05 checking -> food 10 USD\n", |book, _| {
        let first = day(2024, 2, 29);
        let mut rent = terms(Cadence::Every(Span::months(1)), &[], first, &[]);
        rent.escalation = Some(Escalation::Rising(Ratio::percent(3, 0).unwrap()));
        let rent = contract(Days::new(first, day(2028, 3, 1)).unwrap(), Timeline::new(rent));
        let yearly = Ratio::new(103, 100).unwrap();
        let three_years = yearly.checked_mul(yearly).unwrap().checked_mul(yearly).unwrap();
        let four_years = three_years.checked_mul(yearly).unwrap();
        assert_eq!(rent.amount_on(book, day(2025, 2, 27)), Ok(Ratio::ONE));
        assert_eq!(rent.amount_on(book, day(2025, 2, 28)), Ok(yearly));
        assert_eq!(rent.amount_on(book, day(2028, 2, 28)), Ok(three_years));
        assert_eq!(rent.amount_on(book, day(2028, 2, 29)), Ok(four_years));
    });
}

#[test]
fn the_row_of_a_property_in_force_is_the_latest_that_has_begun() {
    use crate::{Prop, Value, prop};
    use axiom_core::{Day, Interner};
    let mut names = Interner::default();
    let (rate, other) = (names.intern("rate"), names.intern("other"));
    let day = |month, day| Day::from_ymd(2026, month, day).unwrap();
    let row = |name, since, whole| Prop { name, value: Value::Num(axiom_core::Ratio::int(whole)), since, loc: None };
    let props = [row(other, Day::MIN, 9), row(rate, Day::MIN, 1), row(rate, day(6, 15), 2), row(rate, day(9, 1), 3)];
    let at = |on: Day| prop(&props, rate, on).map(|found| found.value);
    let want = |whole| Some(Value::Num(axiom_core::Ratio::int(whole)));
    let asked = [Day::MIN, day(6, 14), day(6, 15), day(8, 31), Day::MAX];
    assert_eq!(asked.map(at), [want(1), want(1), want(2), want(2), want(3)]);
    assert!(prop(&props, names.intern("absent"), day(6, 15)).is_none());
    let two_at_once = [row(rate, Day::MIN, 1), row(rate, Day::MIN, 2)];
    assert_eq!(prop(&two_at_once, rate, day(1, 1)).map(|found| found.value), want(1), "own before inherited");
}

#[test]
fn a_flow_stays_small() {
    // A flow is copied into every reader of the journal: its size is felt at a million.
    assert!(size_of::<crate::Flow>() <= 200, "a Flow is {} bytes", size_of::<crate::Flow>());
    assert_eq!(size_of::<Option<crate::Purposed>>(), 20);
}
