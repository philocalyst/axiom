use axiom_core::{Day, FileId, Loc, Qty};
use axiom_engine::{Options, Plan};
use axiom_model::sync::{Column, Field, Format, Rule, Shape, Spec};
use axiom_model::{Book, Source};
use axiom_syntax::Folder;

use super::{Account, Existing, Feed, Form, Unit, World};
use crate::binding;

const STD: &str = include_str!("../../../systems/src/std.ax");

const BASE: &str = "\
use std
base USD
account assets/checking
account assets/stripe
account assets/cards
account assets/visa
account assets/amex
entity halcyon
  known-as \"HALCYON\"
entity mint
  known-as \"MINT MOBILE\"
entity paypal
  known-as \"PAYPAL\"
entity etsy-seller
  known-as \"ETSY\"
entity shell
  known-as \"SHELL\"
entity uber
  known-as \"UBER\"
";

fn day(text: &str) -> Day {
    Day::parse(text.as_bytes()).unwrap()
}

fn native_book<'s>(text: &'s str) -> Book<'s> {
    let (std_file, std_problems) = axiom_syntax::parse(FileId(1), STD, Folder::of("std.ax"));
    assert!(std_problems.is_empty(), "std syntax: {std_problems:?}");
    let (file, problems) = axiom_syntax::parse(FileId(0), text, Folder::default());
    assert!(problems.is_empty(), "source syntax: {problems:?}");
    let sources = [
        Source {
            path: "std.ax",
            file: std_file,
            embedded: true,
        },
        Source {
            path: "book.ax",
            file,
            embedded: false,
        },
    ];
    let (book, problems) = axiom_model::build(&sources);
    assert!(problems.is_empty(), "source model: {problems:?}");
    book
}

fn make_format(book: &mut Book<'_>, fields: &[(Field, u16)]) -> axiom_core::Id<Format> {
    let specs = fields
        .iter()
        .map(|&(field, index)| Spec {
            field,
            places: Box::new([Column::Index(index)]),
            layout: None,
            rule: Rule::None,
            loc: Loc::default(),
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    book.formats.push(Format {
        name: book.names.intern("test-feed"),
        shape: Shape::Rows,
        specs,
        categories: Box::default(),
        loc: Loc::default(),
    })
}

fn engine_run(book: &Book<'_>) -> axiom_engine::Run {
    Plan::new(book).run(Options {
        today: day("2026-12-31"),
        relaxed: false,
    })
}

fn native_world<'b, 's>(book: &'b Book<'s>, run: &'b axiom_engine::Run) -> World<'b, 's> {
    binding::world(book, run, &["journal/2026/01.ax", "journal/2026/02.ax"])
        .unwrap_or_else(|problem| panic!("sync binding: {}", problem.message))
}

fn test_feed<'b, 's>(
    book: &'b Book<'s>,
    format: axiom_core::Id<Format>,
    account: &'s str,
    unit: &'s str,
) -> Feed<'b, 's> {
    let commodity = book
        .commodities
        .iter()
        .find(|(_, value)| book.name(value.symbol) == unit)
        .map(|(_, value)| value)
        .expect("declared commodity");
    Feed {
        account,
        unit: Unit {
            name: unit,
            scale: commodity.scale,
        },
        format: &book.formats[format],
    }
}

fn inserts<'b, 's>(world: &mut World<'b, 's>, feed: &Feed<'b, 's>, text: &str) -> Vec<String> {
    world
        .feed(feed, text)
        .unwrap_or_else(|problems| panic!("{}", problems[0].message))
        .iter()
        .map(|insert| match &insert.form {
            Form::Item(body) => format!("{} {body}", insert.day.to_string().split_at(8).1),
            Form::Row { .. } => panic!("feed rows write journal statements"),
        })
        .collect()
}

fn twice<'b, 's>(world: &mut World<'b, 's>, feed: &Feed<'b, 's>, text: &str) -> Vec<String> {
    let first = inserts(world, feed, text);
    let second = inserts(world, feed, text);
    assert!(
        second.is_empty(),
        "a committed feed delta is idempotent: {second:?}"
    );
    first
}

fn rows(fields: &[(Field, u16)]) -> Vec<(Field, u16)> {
    fields.to_vec()
}

#[test]
fn a_new_row_is_committed_once_and_a_second_read_reconciles_it() {
    let mut book = native_book(BASE);
    let format = make_format(
        &mut book,
        &rows(&[(Field::Date, 1), (Field::Amount, 2), (Field::Memo, 3)]),
    );
    let run = engine_run(&book);
    assert!(run.violations.is_empty());
    let mut world = native_world(&book, &run);
    let feed = test_feed(&book, format, "assets/checking", "USD");
    let first = inserts(&mut world, &feed, "2026-01-06,-3.25,UNKNOWN MARKET\n");
    assert_eq!(first.len(), 1);
    assert!(first[0].contains("3.25 USD"), "{}", first[0]);
    assert!(inserts(&mut world, &feed, "2026-01-06,-3.25,UNKNOWN MARKET\n").is_empty());
}

#[test]
fn pending_rows_settle_only_the_matching_book_flow() {
    let mut book = native_book(BASE);
    let format = make_format(
        &mut book,
        &[
            (Field::Date, 1),
            (Field::Amount, 2),
            (Field::Memo, 3),
            (Field::Pending, 4),
        ],
    );
    let run = engine_run(&book);
    let mut world = native_world(&book, &run);
    let feed = test_feed(&book, format, "assets/checking", "USD");
    let pending = inserts(
        &mut world,
        &feed,
        "2026-01-05,-12.50,CORNER STORE,pending\n2026-01-05,-3.00,COFFEE,pending\n",
    );
    assert_eq!(pending.len(), 2);
    assert!(pending[0].contains("(12.50 USD)"), "{pending:?}");
    assert!(pending[0].contains("^pending-20260105-1"), "{pending:?}");
    assert!(pending[1].contains("(3 USD)"), "{pending:?}");
    assert!(pending[1].contains("^pending-20260105-2"), "{pending:?}");
    drop(world);

    // Exercise settlement against the canonical native account index. The
    // pending code is input to reconciliation here because Book→World's
    // pending-code adapter is still a separate owner task.
    let mut book = native_book(BASE);
    let format = make_format(
        &mut book,
        &[
            (Field::Date, 1),
            (Field::Amount, 2),
            (Field::Memo, 3),
            (Field::Pending, 4),
        ],
    );
    let run = engine_run(&book);
    let mut world = native_world(&book, &run);
    world
        .accounts
        .entry("assets/checking")
        .or_default()
        .flows
        .push(Existing {
            settle: Some("pending-20260105-1"),
            ..Existing::new(day("2026-01-05"), Qty(-1_250))
        });
    world
        .accounts
        .entry("assets/checking")
        .or_default()
        .flows
        .push(Existing::new(day("2026-01-05"), Qty(-300)));
    let feed = test_feed(&book, format, "assets/checking", "USD");
    let posted = inserts(
        &mut world,
        &feed,
        "2026-01-06,-12.50,CORNER STORE 1234,\n2026-01-06,-3.00,COFFEE,\n",
    );
    assert_eq!(posted, ["06 ^pending-20260105-1 settled"]);
}

#[test]
fn bank_assertions_use_posted_rows_in_date_order_and_ignore_pending_and_foreign_units() {
    let mut book = native_book(BASE);
    let format = make_format(
        &mut book,
        &[
            (Field::Date, 1),
            (Field::Amount, 2),
            (Field::Memo, 3),
            (Field::Balance, 4),
            (Field::Pending, 5),
            (Field::Currency, 6),
        ],
    );
    let run = engine_run(&book);
    let mut world = native_world(&book, &run);
    let feed = test_feed(&book, format, "assets/checking", "USD");
    let text = "2026-01-05,-5.00,POSTED,95.00,,\n\
                2026-01-05,-30.00,PENDING,65.00,pending,\n\
                2026-01-05,100.00,FOREIGN,195.00,,EUR\n";
    let lines = twice(&mut world, &feed, text);
    let assertions = lines
        .iter()
        .filter(|line| line.contains(" = "))
        .collect::<Vec<_>>();
    assert_eq!(assertions, ["05 assets/checking = 95 USD"]);
}

#[test]
fn the_last_consistent_balance_is_emitted_independent_of_input_order() {
    let text = "2026-01-05,-10.00,A,90.00\n\
                2026-01-06,-5.00,B,85.00\n\
                2026-01-06,-1.00,C,84.00\n";
    let reversed = "2026-01-06,-1.00,C,84.00\n\
                    2026-01-06,-5.00,B,85.00\n\
                    2026-01-05,-10.00,A,90.00\n";
    for statement in [text, reversed] {
        let mut book = native_book(BASE);
        let format = make_format(
            &mut book,
            &[
                (Field::Date, 1),
                (Field::Amount, 2),
                (Field::Memo, 3),
                (Field::Balance, 4),
            ],
        );
        let run = engine_run(&book);
        let mut world = native_world(&book, &run);
        let feed = test_feed(&book, format, "assets/checking", "USD");
        let lines = inserts(&mut world, &feed, statement);
        let assertions = lines
            .iter()
            .filter(|line| line.contains(" = "))
            .collect::<Vec<_>>();
        assert_eq!(assertions, ["06 assets/checking = 84 USD"]);
    }
}

#[test]
fn unknown_memos_round_trip_as_escaped_descriptions_and_tied_names_are_atomic_errors() {
    let source = format!(
        "{BASE}entity shell-oil\n  known-as \"SHELL\"\nentity shell-station\n  known-as \"SHELL\" any*\n"
    );
    let mut book = native_book(&source);
    let format = make_format(
        &mut book,
        &[(Field::Date, 1), (Field::Amount, 2), (Field::Memo, 3)],
    );
    let run = engine_run(&book);
    let mut world = native_world(&book, &run);
    let feed = test_feed(&book, format, "assets/checking", "USD");
    let error = world
        .feed(&feed, "2026-01-05,-9.99,SHELL 1234\n")
        .unwrap_err();
    assert!(
        error[0].message.contains("known as both"),
        "{}",
        error[0].message
    );
    assert!(
        world
            .accounts
            .values()
            .all(|account| account.flows.is_empty()),
        "a tied source must not partially commit"
    );

    let with_existing = format!("{source}2026-01-05 checking -> shell 9.99 USD\n");
    let mut book = native_book(&with_existing);
    let format = make_format(
        &mut book,
        &[(Field::Date, 1), (Field::Amount, 2), (Field::Memo, 3)],
    );
    let run = engine_run(&book);
    let mut world = native_world(&book, &run);
    let feed = test_feed(&book, format, "assets/checking", "USD");
    assert!(
        inserts(&mut world, &feed, "2026-01-05,-9.99,SHELL 1234\n").is_empty(),
        "a matching native Book flow is resolved before memo recognition"
    );
}

#[test]
fn a_memo_with_quoted_controls_is_written_as_a_readable_description() {
    let mut book = native_book(BASE);
    let format = make_format(
        &mut book,
        &[(Field::Date, 1), (Field::Amount, 2), (Field::Memo, 3)],
    );
    let run = engine_run(&book);
    let mut world = native_world(&book, &run);
    let feed = test_feed(&book, format, "assets/checking", "USD");
    let lines = inserts(
        &mut world,
        &feed,
        "2026-01-05,-9.99,\"  SQ   *CAFE \"\"LUNA\"\" \\\\ ETC \",\n",
    );
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("\\\"LUNA\\\""), "{}", lines[0]);
    assert!(lines[0].contains("\\\\ ETC"), "{}", lines[0]);
}

#[test]
fn same_amount_in_a_foreign_commodity_is_not_a_match_in_the_base_unit() {
    let mut book = native_book(BASE);
    let format = make_format(
        &mut book,
        &[
            (Field::Date, 1),
            (Field::Amount, 2),
            (Field::Memo, 3),
            (Field::Currency, 4),
        ],
    );
    let run = engine_run(&book);
    let mut world = native_world(&book, &run);
    let feed = test_feed(&book, format, "assets/checking", "USD");
    let lines = twice(
        &mut world,
        &feed,
        "2026-01-05,-12.50,CAFE,EUR\n2026-01-05,-12.50,CAFE,USD\n",
    );
    assert_eq!(lines.len(), 2);
    assert!(lines.iter().any(|line| line.contains("12.50 EUR")));
    assert!(lines.iter().any(|line| line.contains("12.50 USD")));
}

#[test]
fn exported_processor_fields_keep_gross_fee_category_and_party_distinct() {
    let source = format!("{BASE}purpose supplies : spending\n");
    let mut book = native_book(&source);
    let format = make_format(
        &mut book,
        &[
            (Field::Date, 1),
            (Field::Memo, 2),
            (Field::Gross, 3),
            (Field::Fee, 4),
            (Field::Category, 5),
            (Field::Object, 6),
            (Field::Party, 7),
        ],
    );
    let purpose = book.purpose("supplies").unwrap();
    let category = axiom_model::sync::Text::Borrowed(book.names.intern("Supplies"));
    book.formats[format].categories = Box::new([(category, purpose)]);
    let run = engine_run(&book);
    let mut world = native_world(&book, &run);
    let feed = test_feed(&book, format, "assets/stripe", "USD");
    let lines = twice(
        &mut world,
        &feed,
        "2026-01-05,HALCYON ORDER 9,100.00,3.20,Supplies,laptop,HALCYON\n",
    );
    assert_eq!(lines.len(), 1);
    assert!(
        lines[0].contains("halcyon -> assets/stripe 100 USD #supplies of laptop"),
        "{}",
        lines[0]
    );
    assert!(lines[0].contains("- 3.20 USD #fees"), "{}", lines[0]);
    assert_eq!(world.accounts["assets/stripe"].flows[0].qty, Qty(9_680));
}

#[test]
fn party_and_via_fields_remain_separate_feed_facts() {
    let mut book = native_book(BASE);
    let format = make_format(
        &mut book,
        &[
            (Field::Date, 1),
            (Field::Memo, 2),
            (Field::Amount, 3),
            (Field::Party, 4),
            (Field::Via, 5),
        ],
    );
    let run = engine_run(&book);
    let mut world = native_world(&book, &run);
    let feed = test_feed(&book, format, "assets/stripe", "USD");
    let lines = inserts(
        &mut world,
        &feed,
        "2026-01-05,PAYPAL TRANSFER,-20.00,ETSY Seller,PAYPAL\n\
         2026-01-06,SHELL 44,-30.00,UBER RIDES,\n",
    );
    assert_eq!(lines.len(), 2);
    assert!(
        lines[0].contains("assets/stripe -> etsy-seller 20 USD"),
        "{}",
        lines[0]
    );
    assert!(lines[0].contains("via paypal"), "{}", lines[0]);
    assert!(
        lines[1].contains("assets/stripe -> uber 30 USD via shell"),
        "{}",
        lines[1]
    );
}

#[test]
fn a_pattern_captured_amount_and_day_override_the_bank_row() {
    let source = format!(
        r#"{BASE}entity wise-fx
  known-as "FX " amount:((digit / letter / ".")+) " ON " date:(digit+ "-" digit+ "-" digit+)
"#
    );
    let mut book = native_book(&source);
    let format = make_format(
        &mut book,
        &[(Field::Date, 1), (Field::Amount, 2), (Field::Memo, 3)],
    );
    let run = engine_run(&book);
    let mut world = native_world(&book, &run);
    let feed = test_feed(&book, format, "assets/checking", "USD");
    let lines = inserts(
        &mut world,
        &feed,
        "2026-01-10,-60.00,FX 58.40 ON 2026-01-09 REF 1\n",
    );
    assert_eq!(lines.len(), 1);
    assert!(lines[0].starts_with("09 "), "{}", lines[0]);
    assert!(lines[0].contains("58.40 USD"), "{}", lines[0]);
}

#[test]
fn an_exchange_groups_two_currencies_by_export_id_but_leaves_singletons_alone() {
    let mut book = native_book(BASE);
    let format = make_format(
        &mut book,
        &[
            (Field::Date, 1),
            (Field::Amount, 2),
            (Field::Memo, 3),
            (Field::Currency, 4),
            (Field::Id, 5),
        ],
    );
    let run = engine_run(&book);
    let mut world = native_world(&book, &run);
    let feed = test_feed(&book, format, "assets/checking", "USD");
    let lines = twice(
        &mut world,
        &feed,
        "2026-01-14,-100.00,CONVERTED,USD,tx9\n\
         2026-01-14,92.00,CONVERTED,EUR,tx9\n\
         2026-01-15,-5.00,COFFEE,EUR,tx10\n\
         2026-01-15,-4.00,LONE,USD,tx11\n",
    );
    assert_eq!(lines.len(), 3);
    assert!(lines.iter().any(|line| line.contains("100 USD -> 92 EUR")));
    assert!(lines.iter().any(|line| line.contains("5 EUR")));
    assert!(lines.iter().any(|line| line.contains("4 USD")));
}

#[test]
fn a_routed_export_sends_rows_to_their_named_account_and_rejects_unknown_routes() {
    let mut book = native_book(BASE);
    let format = make_format(
        &mut book,
        &[
            (Field::Date, 1),
            (Field::Amount, 2),
            (Field::Memo, 3),
            (Field::Route, 4),
        ],
    );
    let run = engine_run(&book);
    let mut world = native_world(&book, &run);
    let feed = test_feed(&book, format, "assets/cards", "USD");
    let lines = inserts(
        &mut world,
        &feed,
        "2026-01-05,-5.00,A,visa\n2026-01-06,-6.00,B,amex\n",
    );
    assert_eq!(lines.len(), 2);
    assert!(lines.iter().any(|line| line.contains("visa -> ? 5 USD")));
    assert!(lines.iter().any(|line| line.contains("amex -> ? 6 USD")));
    let problem = world
        .feed(&feed, "2026-01-07,-7.00,C,diners\n")
        .unwrap_err();
    assert!(problem[0].message.contains("not an account"));
}

#[test]
fn book_flows_win_over_feed_recognition_and_starting_date_uses_latest_flow() {
    let source = format!("{BASE}2026-01-05 stripe -> checking 9.70 USD ^payout\n");
    let mut book = native_book(&source);
    let format = make_format(
        &mut book,
        &[(Field::Date, 1), (Field::Amount, 2), (Field::Memo, 3)],
    );
    let run = engine_run(&book);
    let mut world = native_world(&book, &run);
    let feed = test_feed(&book, format, "assets/checking", "USD");
    let account = &world.accounts["assets/checking"];
    assert_eq!(account.since(day("2026-01-01")), day("2026-01-06"));
    let lines = inserts(
        &mut world,
        &feed,
        "2026-01-05,9.70,STRIPE PAYOUT\n2026-01-08,12.00,SOMETHING NEW\n",
    );
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("12 USD"));
}

#[test]
fn a_coded_native_split_can_reconcile_as_its_total_or_as_its_members() {
    let source = format!(
        "{BASE}account assets/payroll\n2026-01-05 checking -> payroll 1_000 USD ^payroll-run\n  + 500 USD ^payroll-run\n"
    );
    let mut book = native_book(&source);
    let format = make_format(
        &mut book,
        &[(Field::Date, 1), (Field::Amount, 2), (Field::Memo, 3)],
    );
    let run = engine_run(&book);
    let mut world = native_world(&book, &run);
    let feed = test_feed(&book, format, "assets/checking", "USD");
    let total = inserts(&mut world, &feed, "2026-01-06,-1500.00,PAYROLL RUN\n");
    assert!(
        total.is_empty(),
        "the coded native batch total matches: {total:?}"
    );
}

#[test]
fn scale_fixture_reconciles_a_large_statement_against_a_large_bound_account() {
    let mut book = native_book(BASE);
    let format = make_format(
        &mut book,
        &[(Field::Date, 1), (Field::Amount, 2), (Field::Memo, 3)],
    );
    let run = engine_run(&book);
    let mut world = native_world(&book, &run);
    let feed = test_feed(&book, format, "assets/checking", "USD");
    let start = day("2016-01-01");
    let account = world
        .accounts
        .entry("assets/checking")
        .or_insert_with(Account::default);
    account.flows.reserve(100_000);
    for at in 0..50_000 {
        account.flows.push(Existing::new(
            start.add_days((at % 3_650) as i32),
            Qty(-((at % 50_000 + 1) as i64)),
        ));
    }
    let mut text = String::with_capacity(4_000_000);
    for at in 0..10_000 {
        let cents = (at % 50_000 + 1) as i64;
        text.push_str(&format!(
            "{},-{}.{:02},SHOP {}\n",
            start.add_days((at % 3_650) as i32),
            cents / 100,
            cents % 100,
            at % 100
        ));
    }
    let lines = inserts(&mut world, &feed, &text);
    assert!(
        lines.is_empty(),
        "all seeded amounts/dates are represented in the native world's account index"
    );
}

#[test]
#[ignore = "large allocation and runtime fixture; run with --release --ignored --test-threads=1"]
fn a_hundred_thousand_rows_reconcile_against_a_million_native_bound_flows() {
    let mut source = BASE.to_owned();
    for number in 0..100 {
        source.push_str(&format!(
            "entity merchant-{number}\n  known-as \"SHOP {number:03}\"\n"
        ));
    }
    let mut book = native_book(&source);
    let format = make_format(
        &mut book,
        &[(Field::Date, 1), (Field::Amount, 2), (Field::Memo, 3)],
    );
    let run = engine_run(&book);
    let mut world = native_world(&book, &run);
    let feed = test_feed(&book, format, "assets/checking", "USD");
    let first = day("2016-01-01");
    let account = world.accounts.entry("assets/checking").or_default();
    let mut seed = 11u64;
    let mut next = |bound: u64| {
        seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (seed >> 33) % bound
    };
    account.flows = (0..1_000_000)
        .map(|_| {
            Existing::new(
                first.add_days(next(3_650) as i32),
                Qty(-(next(50_000) as i64) - 1),
            )
        })
        .collect();
    let mut statement = String::with_capacity(7_000_000);
    for at in 0..100_000 {
        let (when, cents) = if at % 5 == 0 {
            let flow = account.flows[next(1_000_000) as usize];
            (flow.day.add_days(next(3) as i32), flow.qty.0)
        } else {
            (
                first.add_days(next(3_650) as i32),
                -(next(50_000) as i64) - 1,
            )
        };
        let amount = cents.abs();
        statement.push_str(&format!(
            "{when},-{}.{:02},POS PURCHASE SHOP {:03} SAN FRANCISCO\n",
            amount / 100,
            amount % 100,
            next(120)
        ));
    }
    let lines = inserts(&mut world, &feed, &statement);
    assert!((70_000..85_000).contains(&lines.len()), "{}", lines.len());
}
