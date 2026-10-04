//! What a loan's schedule is, from the events a book states: the payments, a prepayment in both modes, a rate said and a rate
//! reset, a line that states its own amount, a loan paid off, and what explains a statement that disagrees with it.
//!
//! The numbers are those of an independent reference (`docs/v5/measure/loans.py`: Python integers, K5a's fixed-point rule),
//! and the hard fixture is `examples/07-landlord`: its month-end statements and the interest and principal its author
//! checked by hand, to the cent.

use axiom_core::{Day, FileId, Qty};
use axiom_model::promise::{Cause, Kind};
use axiom_model::{Book, Class, Role, Source, build};
use axiom_syntax::{Folder, parse};

const PRELUDE: &str = "\
base USD
commodity USD
  precision 2
purpose interest : spending
purpose principal : transfer
kind property : thing
kind mortgage : asset
asset condo : property
param idx
  2020-01-01 2%
  2026-01-01 5.5%
  2027-01-01 7%
entity bank
account checking : asset
opening 2026-01-01
  checking 900_000 USD
";

/// The 36-month loan most tests use: 120,000.00 at 6% from 2026-01-15, paid from the 1st of each month, from 2026-02-01.
const LOAN: &str = "\
contract home-loan with bank
  loan 120_000 USD on 2026-01-15 at 6% over 3y for condo
  monthly on 1 from checking
  from 2026-02-01
2026-01-15 home-loan
";

fn day(year: i32, month: u32, date: u32) -> Day {
    Day::from_ymd(year, month, date).unwrap()
}

/// Builds `PRELUDE` and `text`; returns the book and what was said of it.
fn built(text: &str) -> (Book<'static>, Vec<axiom_core::Diagnostic>) {
    // The text outlives the book: a test is short, and a leaked string is the simplest owner of what a book borrows.
    let text: &'static str = Box::leak(format!("{PRELUDE}{text}").into_boxed_str());
    let (file, syntax) = parse(FileId(0), text, Folder::of("main.ax"));
    assert!(syntax.is_empty(), "{syntax:?}");
    build(&[Source { path: "main.ax", file, embedded: false }])
}

fn book(text: &str) -> Book<'static> {
    let (book, diagnostics) = built(text);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    book
}

/// A loan's schedule as `(kind, day, interest, principal, owed after)`, in order.
fn entries(book: &Book<'_>, name: &str) -> Vec<(&'static str, String, i64, i64, i64)> {
    let loan = book.promises.loan(book.contract(name).unwrap()).expect("a loan");
    let kind = |kind| if kind == Kind::Pay { "pay" } else { "prepay" };
    let rows = loan.entries().iter();
    rows.map(|e| (kind(e.kind), e.day.to_string(), e.paid.interest.0, e.paid.principal.0, e.paid.open.0)).collect()
}

fn pays(rows: &[(&'static str, String, i64, i64, i64)]) -> Vec<(String, i64, i64, i64)> {
    rows.iter().filter(|row| row.0 == "pay").map(|row| (row.1.clone(), row.2, row.3, row.4)).collect()
}

#[test]
fn the_payments_of_07_landlords_home_loan_are_the_ones_its_author_checked_by_hand() {
    // `examples/07-landlord`: 279,000.00 at 6.75% over 30 years, made 2024-12-18, paid on the 1st from 2025-02-01. Each month
    // of its journal carries the interest and the principal of the payment and the lender's statement of what is owed.
    let hand_checked = [
        ("2025-02-01", 156_938, 24_021, 27_875_979),
        ("2025-03-01", 156_802, 24_157, 27_851_822),
        ("2025-04-01", 156_666, 24_293, 27_827_529),
        ("2025-05-01", 156_530, 24_429, 27_803_100),
        ("2025-06-01", 156_392, 24_567, 27_778_533),
        ("2025-07-01", 156_254, 24_705, 27_753_828),
        ("2025-08-01", 156_115, 24_844, 27_728_984),
        ("2025-09-01", 155_976, 24_983, 27_704_001),
        ("2025-10-01", 155_835, 25_124, 27_678_877),
        ("2025-11-01", 155_694, 25_265, 27_653_612),
        ("2025-12-01", 155_552, 25_407, 27_628_205),
    ];
    // The journal's own comments: 1,568.02 and 241.57 (March), ... 1,555.52 and 254.07 (December), and the statements
    // 278,759.79 (February) to 276,536.12 (November), 276,282.05 after December.
    let text = "\
entity lender
contract home-loan with lender
  loan 279_000 USD on 2024-12-18 at 6.75% over 30y for condo
  monthly on 1 from checking
  from 2025-02-01
2024-12-18 home-loan
";
    let book = book(text);
    let rows = entries(&book, "home-loan");
    let first = pays(&rows);
    for (at, (due, interest, principal, owed)) in hand_checked.iter().enumerate() {
        assert_eq!(first[at], (due.to_string(), *interest, *principal, *owed));
    }
    // The comments of the example, to the cent, for March to December.
    let comments = [
        (156_802, 24_157),
        (156_666, 24_293),
        (156_530, 24_429),
        (156_392, 24_567),
        (156_254, 24_705),
        (156_115, 24_844),
        (155_976, 24_983),
        (155_835, 25_124),
        (155_694, 25_265),
    ];
    for (at, (interest, principal)) in comments.iter().enumerate() {
        assert_eq!((first[at + 1].1, first[at + 1].2), (*interest, *principal), "payment {}", at + 2);
    }
    let level = book.promises.loan(book.contract("home-loan").unwrap()).unwrap().terms().payment().qty;
    assert_eq!(level, Qty(180_959));
}

#[test]
fn a_payment_is_a_split_of_interest_to_the_lender_and_principal_to_the_debt_tab() {
    let book = book(LOAN);
    let contract = &book.contracts[book.contract("home-loan").unwrap()];
    let (loan, terms) = (contract.loan.unwrap(), contract.terms.as_ref().unwrap());
    let group = &terms.template[0];
    assert_eq!(group.header.flow.to, loan.debt, "what the interest leaves goes to the debt tab");
    assert_eq!(book.places[loan.debt].class, Class::Debt);
    assert_eq!(book.name(book.purposes[group.header.flow.purpose.unwrap().purpose].name), "principal");
    let [interest] = &group.legs[..] else { panic!("one leg: the interest") };
    assert_eq!(interest.flow.to, book.entities[contract.party].place.unwrap());
    let purposed = interest.flow.purpose.unwrap();
    assert_eq!(book.name(book.purposes[purposed.purpose].name), "interest");
    assert_eq!(purposed.of, Some(axiom_model::journal::Object::Asset(book.asset("condo").unwrap())), "`for condo`");
    assert!(matches!(interest.part, axiom_model::Part::Of(axiom_model::Quantity::Interest)));
    assert!(matches!(book.places[loan.debt].role, Role::Tab(_)));
}

#[test]
fn a_prepayment_that_shortens_pays_the_loan_off_sooner_and_the_payment_stays() {
    let rows = entries(&book(&format!("{LOAN}2026-03-15 checking -> home-loan 10_000 USD\n")), "home-loan");
    assert_eq!(
        rows[..5],
        [
            ("pay", "2026-02-01".into(), 60_000, 305_063, 11_694_937),
            ("pay", "2026-03-01".into(), 58_475, 306_588, 11_388_349),
            ("prepay", "2026-03-15".into(), 0, 1_000_000, 10_388_349),
            ("pay", "2026-04-01".into(), 51_942, 313_121, 10_075_228),
            ("pay", "2026-05-01".into(), 50_376, 314_687, 9_760_541),
        ]
    );
    assert_eq!(pays(&rows).len(), 33, "three payments fewer, at the payment it always was");
    assert_eq!(rows.last().unwrap().4, 0);
}

#[test]
fn a_prepayment_that_recasts_lowers_the_payment_and_keeps_the_payments() {
    let text = LOAN.replace("for condo\n", "for condo\n    prepay recasts\n");
    let rows = entries(&book(&format!("{text}2026-03-15 checking -> home-loan 10_000 USD\n")), "home-loan");
    assert_eq!(rows[3], ("pay", "2026-04-01".into(), 51_942, 281_065, 10_107_284));
    assert_eq!(pays(&rows).len(), 36);
    assert_eq!(rows.last().unwrap().4, 0);
}

#[test]
fn a_rate_a_statement_says_applies_from_its_day_and_to_a_payment_due_that_day() {
    let rows = entries(&book(&format!("{LOAN}2026-03-01 home-loan now at 9%\n")), "home-loan");
    assert_eq!(
        pays(&rows)[..2],
        [("2026-02-01".into(), 60_000, 305_063, 11_694_937), ("2026-03-01".into(), 87_712, 293_446, 11_401_491)]
    );
}

#[test]
fn a_prepayment_on_a_due_day_comes_after_that_days_payment() {
    let rows = entries(&book(&format!("{LOAN}2026-03-01 checking -> home-loan 10_000 USD\n")), "home-loan");
    assert_eq!(rows[1], ("pay", "2026-03-01".into(), 58_475, 306_588, 11_388_349));
    assert_eq!(rows[2], ("prepay", "2026-03-01".into(), 0, 1_000_000, 10_388_349));
}

#[test]
fn a_rate_may_be_said_of_a_contract_named_as_a_kind_is() {
    // `mortgage` is a kind in the book, as it is in std: the statement is about the contract.
    let text = LOAN.replace("home-loan", "mortgage");
    let rows = entries(&book(&format!("{text}2026-03-01 mortgage now at 9%\n")), "mortgage");
    assert_eq!(pays(&rows)[1].1, 87_712);
}

#[test]
fn a_rate_said_of_a_contract_with_no_loan_is_said_not_ignored() {
    let (_, diagnostics) =
        built("contract netflix with bank\n  15 USD monthly on 1 from checking\n2026-03-01 netflix now at 9%\n");
    let codes: Vec<_> = diagnostics.iter().map(|diagnostic| diagnostic.code.as_ref()).collect();
    assert_eq!(codes, ["contract-rate-change"], "{diagnostics:?}");
    let (_, diagnostics) = built(&format!("{LOAN}2026-03-01 home-loan now at 12 USD\n"));
    assert_eq!(diagnostics.iter().map(|d| d.code.as_ref()).collect::<Vec<_>>(), ["contract-loan-rate"]);
}

#[test]
fn a_reset_reads_the_index_and_is_held_to_the_cap_and_to_the_life() {
    // idx reads 5.5% from 2026 and 7% from 2027: 5.5% + 2.5% = 8% on 2026-08-01, held to 6% + 1% (the cap) = 7%; 7% + 2.5% = 9.5% on 2027-08-01,
    // held to 7% + 1% = 8%, and to the life, 6% + 3% = 9%: 8%. The payment is refigured at each.
    let text = LOAN.replace("for condo\n", "for condo\n    resets 1y from 2026-08-01 to idx + 2.5% cap 1% life 3%\n");
    let rows = entries(&book(&text), "home-loan");
    let pays = pays(&rows);
    // The yearly rate a payment's interest says: its interest over what was owed before it, twelve times.
    let yearly = |at: usize| pays[at].1 as f64 / (pays[at - 1].3 as f64) * 12.0;
    assert!((yearly(5) - 0.06).abs() < 0.0003, "July 2026 is the loan's rate: {}", yearly(5));
    assert!((yearly(6) - 0.07).abs() < 0.0003, "August 2026: {}", yearly(6));
    assert!((yearly(18) - 0.08).abs() < 0.0003, "August 2027: {}", yearly(18));
}

#[test]
fn a_reset_whose_index_the_book_does_not_have_stops_the_schedule_and_says_why() {
    // `late` has no row before 2026-09-01, so the reset of 2026-08-01 has nothing to read.
    let resets = LOAN.replace("for condo\n", "for condo\n    resets 1y from 2026-08-01 to late + 2% cap 1%\n");
    let text = format!("param late\n  2026-09-01 5.5%\n{resets}");
    let book = book(&text);
    let loan = book.promises.loan(book.contract("home-loan").unwrap()).unwrap();
    assert!(loan.paid_on(day(2026, 7, 1)).is_ok());
    assert!(matches!(loan.paid_on(day(2026, 8, 1)), Err(axiom_model::ForecastError::MissingIndex { .. })));
    assert!(matches!(loan.paid_on(day(2026, 9, 1)), Err(axiom_model::ForecastError::MissingIndex { .. })));
}

#[test]
fn a_line_that_states_more_than_its_payment_prepays_the_difference_and_one_that_states_less_changes_nothing() {
    // 2026-02-01 states 4,650.63 (a thousand over 3,650.63): a thousand is paid off the principal after the payment; 03-01 states less.
    let text = format!("{LOAN}2026-02-01 home-loan 4_650.63 USD\n2026-03-01 home-loan 1_000 USD\n");
    let rows = entries(&book(&text), "home-loan");
    assert_eq!(rows[0], ("pay", "2026-02-01".into(), 60_000, 305_063, 11_694_937));
    assert_eq!(rows[1], ("prepay", "2026-02-01".into(), 0, 100_000, 11_594_937));
    assert_eq!(rows[2].1, "2026-03-01");
    assert_eq!(rows[2].0, "pay");
    assert_eq!(rows[3].1, "2026-04-01", "a payment that states less is still the schedule's payment");
}

#[test]
fn a_loan_paid_off_has_no_payment_after_it_and_nothing_is_owed() {
    let text = format!("{LOAN}2026-04-10 checking -> home-loan 200_000 USD\n");
    let book = book(&text);
    let rows = entries(&book, "home-loan");
    assert_eq!(rows.last().unwrap(), &("prepay", "2026-04-10".into(), 0, 11_080_228, 0));
    let id = book.contract("home-loan").unwrap();
    let loan = book.promises.loan(id).unwrap();
    assert_eq!(loan.open_on(day(2026, 6, 1)), Some(Qty(0)));
    assert_eq!(loan.open_on(day(2026, 1, 14)), None, "before it was made nothing is owed and nothing is said");
    assert_eq!(loan.open_on(day(2026, 3, 31)), Some(Qty(11_388_349)));
    assert!(loan.terms().pays(loan.terms().first() + 2) && !loan.terms().pays(loan.terms().first() + 3));
}

#[test]
fn what_a_statement_says_is_explained_by_the_one_thing_that_explains_it_to_the_cent() {
    // Lines keep 02-01, 04-01 (statements below are on 04-02); 03-01 is not kept.
    let text = format!("{LOAN}2026-02-01 home-loan\n2026-04-01 home-loan\n");
    let book = book(&text);
    let contract = book.contract("home-loan").unwrap();
    let ask = |stated: i64| book.promises.reconcile(&book, contract, day(2026, 4, 2), Qty(stated));
    let schedule = book.promises.loan(contract).unwrap().open_on(day(2026, 4, 2)).unwrap().0;
    assert_eq!(schedule, 11_080_228);
    assert_eq!(ask(schedule), None, "a statement that agrees says nothing");
    let found = ask(schedule + 306_588).unwrap();
    assert_eq!(
        found.cause,
        Cause::Missed(vec![(day(2026, 3, 1), Qty(306_588))]),
        "the principal of the payment of 03-01"
    );
    assert_eq!((found.stated.0, found.owed.0, found.gap().0), (schedule + 306_588, schedule, 306_588));
    assert_eq!(ask(schedule - 5_000).unwrap().cause, Cause::Prepaid);
    assert_eq!(ask(schedule + 1).unwrap().cause, Cause::Unknown, "a cent no one explains is not guessed at");
    assert_eq!(book.promises.reconcile(&book, contract, day(2026, 1, 1), Qty(1)), None, "before the loan was made");
}

#[test]
fn a_short_payment_and_an_extra_are_each_a_cause_and_two_that_tie_are_no_cause() {
    // 02-01 pays 2,000.00 of a 3,650.63 payment (interest 600.00, so 1,400.00 of principal where 3,050.63 was due);
    // 03-01 states 4,650.63, a thousand over; 04-01 is kept as it is.
    let text =
        format!("{LOAN}2026-02-01 home-loan 2_000 USD\n2026-03-01 home-loan 4_650.63 USD\n2026-04-01 home-loan\n");
    let book = book(&text);
    let contract = book.contract("home-loan").unwrap();
    let on = |day, stated| book.promises.reconcile(&book, contract, day, Qty(stated));
    // After the payment of 02-01 alone, the statement is 1,650.63 of principal above the schedule.
    let after_first = book.promises.loan(contract).unwrap().open_on(day(2026, 2, 2)).unwrap().0;
    assert_eq!(
        on(day(2026, 2, 2), after_first + 165_063).unwrap().cause,
        Cause::Short(vec![(day(2026, 2, 1), Qty(165_063))])
    );
    // On 03-02 two things are in play, a short line and an extra: a statement that is only the extra says so.
    let after_second = book.promises.loan(contract).unwrap().open_on(day(2026, 3, 2)).unwrap().0;
    assert_eq!(
        on(day(2026, 3, 2), after_second + 100_000).unwrap().cause,
        Cause::Extra(vec![(day(2026, 3, 1), Qty(100_000))])
    );
    // The sum of the short line's principal and the extra is explained by neither: no cause.
    assert_eq!(on(day(2026, 3, 2), after_second + 165_063 + 100_000).unwrap().cause, Cause::Unknown);
}

#[test]
fn a_loan_with_two_prepayments_to_a_tab_two_loans_share_gives_them_to_the_earlier() {
    let text = "\
contract first with bank
  loan 100_000 USD on 2026-01-15 at 6% over 3y
  monthly on 1 from checking
  from 2026-02-01
contract second with bank
  loan 50_000 USD on 2026-01-15 at 6% over 3y
  monthly on 1 from checking
  from 2026-02-01
2026-03-15 checking -> first 5_000 USD
";
    let book = book(text);
    assert_eq!(entries(&book, "first").iter().filter(|row| row.0 == "prepay").count(), 1);
    assert_eq!(entries(&book, "second").iter().filter(|row| row.0 == "prepay").count(), 0);
}

#[test]
fn two_causes_that_explain_a_difference_alike_are_not_guessed_between() {
    // The principal of the payment due 03-01 that nobody wrote is the same amount as the extra the line of 02-01 states (the
    // extra lowers the interest of the next payment, so the amount is the one that is its own consequence): both explain a
    // statement that says that much more, so neither is named.
    let without = book(&format!("{LOAN}2026-02-01 home-loan\n"));
    let contract = without.contract("home-loan").unwrap();
    let guess = without.promises.loan(contract).unwrap().payments().nth(1).unwrap().paid.principal.0 * 1000 / 995;
    let tie = (guess - 5..guess + 5).find_map(|extra| {
        let book = book(&format!("{LOAN}2026-02-01 home-loan {}\n", amount(Qty(365_063 + extra))));
        let loan = book.promises.loan(contract).unwrap();
        let (schedule, second) =
            (loan.open_on(day(2026, 3, 2)).unwrap(), loan.payments().nth(1).unwrap().paid.principal);
        (second.0 == extra).then(|| book.promises.reconcile(&book, contract, day(2026, 3, 2), schedule + second))
    });
    let tied = tie.expect("an extra that is the principal it leaves").map(|found| found.cause);
    assert!(matches!(&tied, Some(Cause::Several(both)) if both.len() == 2), "{tied:?}");
}

/// An amount as a line of the journal writes it.
fn amount(qty: Qty) -> String {
    format!("{}.{:02} USD", qty.0 / 100, qty.0 % 100)
}
