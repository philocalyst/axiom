use crate::format::{Field, Place, Shape, Spec};
use crate::peg::Patterns;
use crate::recognize::Known;
use crate::reconcile::Batch;

use super::*;

const USD: Unit = Unit {
    name: "USD",
    scale: 2,
};
const EUR: Unit = Unit {
    name: "EUR",
    scale: 2,
};

fn day(text: &str) -> Day {
    Day::parse(text.as_bytes()).unwrap()
}

fn format(specs: Vec<Spec>) -> Format {
    Format {
        shape: Shape::Rows,
        specs,
        categories: Vec::new(),
    }
}

fn name(text: &str) -> Place {
    Place::Name(text.into())
}

/// Rows are `date,amount,memo,balance,pending`, with no header.
fn feed() -> Feed<'static> {
    let at = |column| Place::Index(column);
    let specs = vec![
        Spec::new(Field::Date, [at(1)]),
        Spec::new(Field::Amount, [at(2)]),
        Spec::new(Field::Memo, [at(3)]),
        Spec::new(Field::Balance, [at(4)]),
        Spec::new(Field::Pending, [at(5)]),
    ];
    Feed {
        account: "checking",
        unit: USD,
        format: format(specs),
    }
}

fn world(known: Vec<Known<'static>>) -> World<'static> {
    let codes = ["code:(\"inv-\" digit+ \"-\" digit+)"];
    World {
        recognizer: Recognizer::new(known, &codes, &Patterns::default()).unwrap(),
        layout: Layout::new(["journal/2026/01.ax"]),
        accounts: Map::default(),
        units: vec![USD, EUR],
        dues: Vec::new(),
        claims: Map::default(),
    }
}

fn party(name: &'static str, pattern: &'static str) -> Known<'static> {
    Known {
        name,
        account: false,
        patterns: vec![pattern],
    }
}

fn account(name: &'static str) -> Known<'static> {
    Known {
        name,
        account: true,
        patterns: Vec::new(),
    }
}

/// The lines a feed adds, dated as they are written.
fn written_by(world: &mut World<'static>, feed: &Feed<'static>, text: &str) -> Vec<String> {
    let inserts = world
        .feed(feed, text)
        .unwrap_or_else(|problems| panic!("{}", problems[0].message));
    inserts
        .iter()
        .map(|insert| match &insert.form {
            Form::Item(body) => format!("{} {body}", insert.day.to_string().split_at(8).1),
            Form::Row { .. } => unreachable!("a statement adds journal lines"),
        })
        .collect()
}

fn written(world: &mut World<'static>, text: &str) -> Vec<String> {
    written_by(world, &feed(), text)
}

/// Syncs `text` twice: what it adds, and what the second time adds.
fn twice(
    world: &mut World<'static>,
    feed: &Feed<'static>,
    text: &str,
) -> (Vec<String>, Vec<String>) {
    (written_by(world, feed, text), written_by(world, feed, text))
}

#[test]
fn a_pending_record_is_written_in_parentheses_and_settled_when_it_posts() {
    let mut world = world(vec![]);
    let pending = "2026-01-05,-12.50,CORNER STORE,,pending\n2026-01-05,-3.00,COFFEE,,pending\n";
    assert_eq!(
        written(&mut world, pending),
        [
            "05 checking -> ? (12.50 USD) \"CORNER STORE\" ^pending-20260105-1",
            "05 checking -> ? (3 USD) \"COFFEE\" ^pending-20260105-2",
        ]
    );
    // The book now has the flow, still pending, with its code; the bank posts it a day later.
    let mut world = self::world(vec![]);
    let account = world.accounts.entry("checking").or_default();
    account.flows.push(Existing {
        settle: Some("pending-20260105-1"),
        ..Existing::new(day("2026-01-05"), Qty(-1250))
    });
    account
        .flows
        .push(Existing::new(day("2026-01-05"), Qty(-300)));
    let posted = "2026-01-06,-12.50,CORNER STORE 1234,,\n2026-01-06,-3.00,COFFEE,,\n";
    assert_eq!(
        written(&mut world, posted),
        ["06 ^pending-20260105-1 settled"],
        "the flow with no code has nothing to say"
    );
}

#[test]
fn an_invoice_code_finds_its_party_and_only_its_partys_codes_are_carried() {
    let mut world = world(vec![party("halcyon", "\"HALCYON\"")]);
    world.claims.insert("inv-2026-01", "halcyon");
    world.claims.insert("inv-2026-09", "northwind");
    let text = "2026-01-08,3800.00,WIRE FROM SOMEONE PAYING INV-2026-01,,\n\
                2026-01-09,100.00,HALCYON RE INV-2026-09,,\n\
                2026-01-10,5.00,INV-2026-77 UNKNOWN,,\n";
    assert_eq!(
        written(&mut world, text),
        [
            "08 halcyon -> checking 3_800 USD ^inv-2026-01",
            "09 halcyon -> checking 100 USD",
            "10 ? -> checking 5 USD \"INV-2026-77 UNKNOWN\"",
        ]
    );
}

#[test]
fn an_occurrence_that_differs_says_so_and_a_pending_one_waits() {
    let mut world = world(vec![party("mint", "\"MINT MOBILE\"")]);
    let due = |on: &str| Due {
        contract: "phone",
        party: "mint",
        account: "checking",
        day: day(on),
        qty: Qty(-4500),
        window: 15,
    };
    world.dues = vec![due("2026-01-08"), due("2026-02-08")];
    let text = "2026-01-09,-47.30,MINT MOBILE,,\n2026-02-08,-45.00,MINT MOBILE,,pending\n";
    assert_eq!(
        written(&mut world, text),
        [
            "09 phone 47.30 USD",
            "08 checking -> mint (45 USD) ^pending-20260208-1"
        ]
    );
    let mut world = self::world(vec![party("mint", "\"MINT MOBILE\"")]);
    world.dues = vec![due("2026-01-08")];
    assert_eq!(
        written(&mut world, "2026-01-08,-45.00,MINT MOBILE,,\n"),
        ["08 phone"]
    );
}

#[test]
fn a_statement_ends_in_an_assertion_however_its_days_are_ordered() {
    let asserted = |text: &str| {
        written(&mut world(vec![]), text)
            .into_iter()
            .filter(|line| line.contains(" = "))
            .collect::<Vec<_>>()
    };
    let oldest_first =
        "2026-01-05,-10.00,A,90.00,\n2026-01-06,-5.00,B,85.00,\n2026-01-06,-1.00,C,84.00,\n";
    let newest_first =
        "2026-01-06,-1.00,C,84.00,\n2026-01-06,-5.00,B,85.00,\n2026-01-05,-10.00,A,90.00,\n";
    assert_eq!(asserted(oldest_first), ["06 checking = 84 USD"]);
    assert_eq!(asserted(newest_first), ["06 checking = 84 USD"]);
    assert_eq!(
        asserted("2026-01-06,-1.00,C,-84.00,\n"),
        ["06 checking = -84 USD"]
    );
    assert!(
        asserted("2026-01-06,-5.00,B,85.00,\n2026-01-06,-1.00,C,50.00,\n").is_empty(),
        "balances that do not add up are no assertion"
    );
    assert!(asserted("2026-01-06,-5.00,B,,\n").is_empty());
    let mut world = world(vec![]);
    world
        .accounts
        .entry("checking")
        .or_default()
        .asserted
        .push(day("2026-01-06"));
    assert!(
        written(&mut world, oldest_first)
            .iter()
            .all(|line| !line.contains(" = ")),
        "one assertion to a day"
    );
}

#[test]
fn a_memo_nobody_is_known_as_is_a_description_that_reads_back() {
    let mut world = world(vec![]);
    let lines = written(
        &mut world,
        "2026-01-05,-9.99,\"  SQ   *CAFE \"\"LUNA\"\" \\ ETC \",,\n2026-01-05,0.00,NOTHING MOVED,,\n",
    );
    assert_eq!(
        lines,
        ["05 checking -> ? 9.99 USD \"SQ *CAFE \\\"LUNA\\\" \\\\ ETC\""]
    );
}

#[test]
fn two_memos_that_tie_are_an_error_naming_both_and_nothing_is_written() {
    let mut world = world(vec![
        party("shell-oil", "\"SHELL\""),
        party("shell-station", "\"SHELL\" any*"),
    ]);
    let problems = world
        .feed(&feed(), "2026-01-05,-9.99,SHELL 1234,,\n")
        .err()
        .expect("refused");
    assert_eq!(
        problems[0].message,
        "`SHELL 1234` is known as both shell-oil and shell-station"
    );
    assert!(
        problems[0].help[0]
            .text
            .contains("matches more of the memo")
    );
    assert!(
        world.accounts.is_empty(),
        "nothing is remembered from a source that failed"
    );
}

#[test]
fn a_record_already_written_is_not_in_the_way_of_a_tie() {
    let mut world = world(vec![
        party("shell-oil", "\"SHELL\""),
        party("shell-station", "\"SHELL\" any*"),
    ]);
    let account = world.accounts.entry("checking").or_default();
    account
        .flows
        .push(Existing::new(day("2026-01-05"), Qty(-999)));
    assert!(
        written(&mut world, "2026-01-05,-9.99,SHELL 1234,,\n").is_empty(),
        "a tie in what is written is not an error"
    );
}

/// A processor's export: what a row says besides its amount.
fn processor(extra: Vec<Spec>) -> Feed<'static> {
    let mut specs = vec![
        Spec::new(Field::Date, [name("Date")]),
        Spec::new(Field::Memo, [name("Memo")]),
    ];
    specs.extend(extra);
    Feed {
        account: "stripe",
        unit: USD,
        format: format(specs),
    }
}

#[test]
fn what_the_export_says_beats_what_patterns_find_and_the_memos_party_is_the_go_between() {
    let known = vec![
        party("paypal", "\"PAYPAL\""),
        party("etsy-seller", "\"ETSY\""),
        party("halcyon", "\"HALCYON\""),
        party("shell", "\"SHELL\""),
        party("uber", "\"UBER\""),
    ];
    let mut world = world(known);
    world.claims.insert("inv-2026-01", "halcyon");
    let feed = processor(vec![
        Spec::new(Field::Amount, [name("Amount")]),
        Spec::new(Field::Code, [name("Ref")]),
        Spec::new(Field::Party, [name("Who")]),
        Spec::new(Field::Via, [name("For")]),
    ]);
    let text = "Date,Memo,Amount,Ref,Who,For\n\
                2026-01-05,PAYPAL TRANSFER,-20.00,,,Etsy Seller\n\
                2026-01-06,SHELL 44,-30.00,,UBER RIDES,\n\
                2026-01-07,SOMEBODY ELSE,3800.00,INV-2026-01,,\n\
                2026-01-08,SHELL 45,-31.00,check 1041,,\n";
    let (first, second) = twice(&mut world, &feed, text);
    assert_eq!(
        first,
        [
            "05 stripe -> etsy-seller 20 USD via paypal",
            "06 stripe -> uber 30 USD via shell",
            "07 halcyon -> stripe 3_800 USD ^inv-2026-01",
            "08 stripe -> shell 31 USD ^check-1041",
        ],
        "a `via` field names who it was for, a `party` field who it was with, and a `code` of a claim its party"
    );
    assert!(second.is_empty());
}

#[test]
fn a_payout_with_its_fee_is_the_gross_and_the_fee_an_item_and_a_category_says_what_it_is_for() {
    let mut feed = processor(vec![
        Spec::new(Field::Gross, [name("Gross")]),
        Spec::new(Field::Fee, [name("Fee")]),
        Spec::new(Field::Category, [name("Category")]),
        Spec::new(Field::Object, [name("Object")]),
    ]);
    feed.format
        .categories
        .push(("Supplies".into(), "supplies".into()));
    let mut world = world(vec![party("halcyon", "\"HALCYON\"")]);
    let text = "Date,Memo,Gross,Fee,Category,Object\n\
                2026-01-05,HALCYON ORDER 9,100.00,3.20,Supplies,laptop\n\
                2026-01-06,REFUND,-40.00,0,Unmapped,\n\
                2026-01-07,SHIPPING,-10.00,1.00,,\n";
    let (first, second) = twice(&mut world, &feed, text);
    assert_eq!(
        first,
        [
            "05 halcyon -> stripe 100 USD #supplies of laptop\n  - 3.20 USD #fees via stripe",
            "06 stripe -> ? 40 USD \"REFUND\"",
            "07 stripe -> ? 10 USD \"SHIPPING\"\n  + 1 USD #fees via stripe",
        ],
        "the tail is in the language's order, and a fee comes off what arrives and goes on what leaves"
    );
    let net: Vec<i64> = world.accounts["stripe"]
        .flows
        .iter()
        .map(|flow| flow.qty.0)
        .collect();
    assert_eq!(net, [9680, -4000, -1100], "what the account saw is the net");
    assert!(second.is_empty(), "so the second sync matches all three");
}

#[test]
fn the_two_sides_of_an_exchange_are_one_line() {
    let feed = Feed {
        account: "wise",
        unit: USD,
        format: format(vec![
            Spec::new(Field::Date, [name("Date")]),
            Spec::new(Field::Amount, [name("Amount")]),
            Spec::new(Field::Memo, [name("Memo")]),
            Spec::new(Field::Currency, [name("Currency")]),
            Spec::new(Field::Id, [name("Id")]),
        ]),
    };
    let text = "Date,Amount,Memo,Currency,Id\n\
                2026-01-14,-100.00,CONVERTED,USD,tx9\n\
                2026-01-14,92.00,CONVERTED,EUR,tx9\n\
                2026-01-15,-5.00,COFFEE,EUR,tx10\n\
                2026-01-15,-4.00,LONE,USD,tx11\n";
    let mut world = world(vec![]);
    let (first, second) = twice(&mut world, &feed, text);
    assert_eq!(
        first,
        [
            "14 wise 100 USD -> 92 EUR \"CONVERTED\"",
            "15 wise -> ? 5 EUR \"COFFEE\"",
            "15 wise -> ? 4 USD \"LONE\"",
        ],
        "an id shared by two units is one flow; one that stands alone is nothing to merge"
    );
    assert!(
        second.is_empty(),
        "each side matched a flow of its own unit"
    );
}

#[test]
fn an_export_of_several_cards_gives_each_row_to_its_own_account() {
    let feed = Feed {
        account: "cards",
        unit: USD,
        format: format(vec![
            Spec::new(Field::Date, [name("Date")]),
            Spec::new(Field::Amount, [name("Amount")]),
            Spec::new(Field::Memo, [name("Memo")]),
            Spec::new(Field::Route, [name("Card")]),
        ]),
    };
    let mut world = world(vec![account("visa"), account("amex")]);
    let text = "Date,Amount,Memo,Card\n2026-01-05,-5.00,A,VISA\n2026-01-06,-6.00,B,Amex\n2026-01-07,-7.00,C,visa\n";
    let (first, second) = twice(&mut world, &feed, text);
    assert_eq!(
        first,
        [
            "05 visa -> ? 5 USD \"A\"",
            "07 visa -> ? 7 USD \"C\"",
            "06 amex -> ? 6 USD \"B\""
        ]
    );
    assert!(second.is_empty());
    let problems = world
        .feed(&feed, "Date,Amount,Memo,Card\n2026-01-05,-5.00,A,DINERS\n")
        .err()
        .expect("refused");
    assert_eq!(
        problems[0].message,
        "`DINERS` is not an account the book knows"
    );
}

#[test]
fn a_memo_that_says_its_own_amount_and_day_is_written_and_matched_by_them() {
    let pattern = "\"FX \" amount:((digit / letter / \".\")+) \" ON \" date:(digit+ \"-\" digit+ \"-\" digit+)";
    let mut world = world(vec![party("wise-fx", pattern)]);
    let text = "2026-01-10,-60.00,FX 58.40 ON 2026-01-09 REF 1,,\n2026-01-11,-2.00,FX 1.5.5 ON 2026-01-09,,\n";
    let bad = world
        .feed(&feed(), text)
        .err()
        .expect("the second is not an amount");
    assert_eq!(
        bad[0].message,
        "`1.5.5`, which a pattern took for the amount, is not one"
    );
    let text = "2026-01-10,-60.00,FX 58.40 ON 2026-01-09 REF 1,,\n";
    let (first, second) = twice(&mut world, &feed(), text);
    assert_eq!(first, ["09 checking -> wise-fx 58.40 USD"]);
    assert!(
        second.is_empty(),
        "the book has what the memo said, and so does the next sync"
    );
}

#[test]
fn a_record_in_another_currency_is_written_in_it() {
    let feed = Feed {
        account: "checking",
        unit: USD,
        format: format(vec![
            Spec::new(Field::Date, [name("Date")]),
            Spec::new(Field::Amount, [name("Amount")]),
            Spec::new(Field::Memo, [name("Memo")]),
            Spec::new(Field::Currency, [name("Currency")]),
        ]),
    };
    let mut world = world(vec![]);
    let text =
        "Date,Amount,Memo,Currency\n2026-01-05,-12.50,CAFE,eur\n2026-01-05,-12.50,CAFE,USD\n";
    let (first, second) = twice(&mut world, &feed, text);
    assert_eq!(
        first,
        [
            "05 checking -> ? 12.50 EUR \"CAFE\"",
            "05 checking -> ? 12.50 USD \"CAFE\""
        ]
    );
    assert!(
        second.is_empty(),
        "euros and dollars of the same size are two flows"
    );
}

#[test]
fn a_document_a_source_printed_is_what_the_banks_line_for_the_same_money_is() {
    let mut world = world(vec![
        account("checking"),
        account("stripe"),
        party("halcyon", "\"HALCYON\""),
    ]);
    let printed = |day: &str, body: &str| Insert {
        path: "journal/2026/01.ax".into(),
        day: self::day(day),
        form: Form::Item(body.into()),
    };
    world.learn(&[
        printed("2026-01-05", "stripe -> checking 970 USD ^po-1"),
        printed(
            "2026-01-06",
            "halcyon -> checking 1_000 USD ^inv-1\n  - 30 USD #fees via halcyon",
        ),
        printed("2026-01-07", "halcyon -> checking (500 USD) ^inv-2"),
        printed(
            "2026-01-08",
            "27 halcyon owes studio 900 USD due 30d ^inv-3",
        ),
        printed("2026-01-09", "checking -> nobody 5 EUR"),
    ]);
    let flows = |name: &str| -> Vec<(String, i64, Option<&str>)> {
        world.accounts[name]
            .flows
            .iter()
            .map(|flow| (flow.day.to_string(), flow.qty.0, flow.unit))
            .collect()
    };
    assert_eq!(
        flows("checking"),
        [
            ("2026-01-05".to_string(), 97_000, Some("USD")),
            ("2026-01-06".to_string(), 97_000, Some("USD")),
            ("2026-01-09".to_string(), -500, Some("EUR")),
        ],
        "a fee item comes off what arrives; a pending flow and a claim move nothing"
    );
    assert_eq!(
        flows("stripe"),
        [("2026-01-05".to_string(), -97_000, Some("USD"))]
    );
    let text = "2026-01-07,970.00,STRIPE PAYOUT,,\n2026-01-08,970.00,HALCYON PAYMENT,,\n2026-01-08,12.00,SOMETHING NEW,,\n";
    assert_eq!(
        written(&mut world, text),
        ["08 ? -> checking 12 USD \"SOMETHING NEW\""],
        "both payouts were already printed"
    );
}

#[test]
fn a_batch_the_book_has_is_matched_as_its_total_or_one_by_one() {
    let batch = |world: &mut World<'static>| {
        let account = world.accounts.entry("checking").or_default();
        let part = |qty, batch| Existing {
            batch,
            ..Existing::new(day("2026-01-05"), Qty(qty))
        };
        account.flows.extend([
            part(100_000, Batch::Member(7)),
            part(50_000, Batch::Member(7)),
            part(150_000, Batch::Total(7)),
        ]);
    };
    let mut total = world(vec![]);
    batch(&mut total);
    assert!(
        written(&mut total, "2026-01-06,1500.00,PAYROLL RUN,,\n").is_empty(),
        "the bank shows the total"
    );
    let mut one_by_one = world(vec![]);
    batch(&mut one_by_one);
    assert!(
        written(
            &mut one_by_one,
            "2026-01-06,1000.00,PAY A,,\n2026-01-06,500.00,PAY B,,\n"
        )
        .is_empty()
    );
    let mut both = world(vec![]);
    batch(&mut both);
    assert_eq!(
        written(
            &mut both,
            "2026-01-06,1500.00,PAYROLL RUN,,\n2026-01-06,1000.00,PAY A,,\n"
        ),
        ["06 ? -> checking 1_000 USD \"PAY A\""],
        "the members went with the total"
    );
}

#[test]
fn an_account_says_where_a_source_should_start() {
    let mut account = Account::default();
    assert_eq!(account.since(day("2026-01-01")), day("2026-01-01"));
    account.flows.extend([
        Existing::new(day("2026-03-05"), Qty(1)),
        Existing::new(day("2026-02-01"), Qty(1)),
    ]);
    assert_eq!(account.since(day("2026-01-01")), day("2026-03-06"));
}

#[test]
#[ignore = "a timing, alone: cargo test -p axiom-sync --release -- --ignored --test-threads=1"]
fn a_statement_of_a_hundred_thousand_records_against_an_account_of_a_million_flows() {
    let mut seed = 11u64;
    let mut next = |bound: u64| {
        seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (seed >> 33) % bound
    };
    let names: Vec<String> = (0..100).map(|n| format!("merchant-{n}")).collect();
    let patterns: Vec<String> = (0..100).map(|n| format!("\"SHOP {n:03}\"")).collect();
    let known = names.iter().zip(&patterns).map(|(name, pattern)| Known {
        name: Box::leak(name.clone().into_boxed_str()),
        account: false,
        patterns: vec![Box::leak(pattern.clone().into_boxed_str())],
    });
    let mut world = world(known.collect());
    let first = day("2016-01-01");
    let account = world.accounts.entry("checking").or_default();
    account.flows = (0..1_000_000)
        .map(|_| {
            Existing::new(
                first.add_days(next(3650) as i32),
                Qty(-(next(50_000) as i64) - 1),
            )
        })
        .collect();
    let mut text = String::new();
    for at in 0..100_000 {
        // A fifth are on the book already, a few days off; the rest are new.
        let (day, cents) = match at % 5 {
            0 => {
                let known = account.flows[next(1_000_000) as usize];
                (known.day.add_days(next(3) as i32), known.qty.0)
            }
            _ => (
                first.add_days(next(3650) as i32),
                -(next(50_000) as i64) - 1,
            ),
        };
        let (whole, fraction) = (cents.abs() / 100, cents.abs() % 100);
        text += &format!(
            "{day},-{whole}.{fraction:02},POS PURCHASE SHOP {:03} SAN FRANCISCO,,\n",
            next(120)
        );
    }
    let started = std::time::Instant::now();
    let lines = world
        .feed(&feed(), &text)
        .unwrap_or_else(|problems| panic!("{}", problems[0].message));
    eprintln!(
        "a statement of 100,000 records against 1,000,000 flows: {} lines in {:?}",
        lines.len(),
        started.elapsed()
    );
    assert!(
        lines.len() > 70_000 && lines.len() < 85_000,
        "{}",
        lines.len()
    );
}
