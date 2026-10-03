//! What a book's contracts compile to: terms, the schedules they fall due on, and what is still owed.

use axiom_core::{Day, Days, FileId};
use axiom_model::promise::{Keep, Residual, Term};
use axiom_model::{Book, ScheduleKind, Source, build};
use axiom_syntax::{Folder, parse};

const PRELUDE: &str = "\
base USD
commodity USD
  precision 2
commodity VTI
purpose fees : spending
entity bank
entity greystar
account checking : asset
opening 2026-01-01
  checking 12_000 USD
";

fn day(year: i32, month: u32, date: u32) -> Day {
    Day::from_ymd(year, month, date).unwrap()
}

/// Builds `PRELUDE` and `text`, and hands the book to `then`.
fn with_book<T>(text: &str, then: impl FnOnce(&Book<'_>) -> T) -> T {
    let (path, text) = ("main.ax", format!("{PRELUDE}{text}"));
    let (file, syntax) = parse(FileId(0), &text, Folder::of(path));
    assert!(syntax.is_empty(), "{syntax:?}");
    let (book, diagnostics) = build(&[Source { path, file, embedded: false }]);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    then(&book)
}

fn due_days(book: &Book<'_>, name: &str, kind: ScheduleKind, window: Days) -> Vec<String> {
    let contract = book.contract(name).unwrap();
    let schedule = book.promises.schedule(contract, kind).unwrap();
    schedule.days(window).map(|day| day.to_string()).collect()
}

fn window(first: Day, last: Day) -> Days {
    Days::new(first, last).unwrap()
}

#[test]
fn a_contract_compiles_to_a_term_whose_children_come_first() {
    let text = "\
contract rent with greystar
  1_000 USD monthly on 1 from checking
  buy VTI for 100 USD monthly on 15 from checking
  from 2026-01-01
";
    with_book(text, |book| {
        let promise = book.promises.of(book.contract("rent").unwrap());
        let Term::All(children) = book.promises.term(promise.root) else { panic!("two streams are one All") };
        let every = book.promises.children(children);
        assert_eq!(every.len(), 2);
        assert!(every.iter().all(|child| *child < promise.root), "post-order: children before their parent");
        assert_eq!(promise.regular.map(|stream| stream.every), Some(every[0]));
        assert_eq!(promise.standing.map(|stream| stream.every), Some(every[1]));
        let Term::Every { body, .. } = book.promises.term(every[0]) else { panic!("an Every") };
        assert!(matches!(book.promises.term(body), Term::Pay(_)));
    });
}

#[test]
fn the_due_days_are_counted_from_the_first_day_and_a_waiver_is_a_hole_in_them() {
    let text = "\
contract rent with greystar
  1_000 USD monthly on 1 from checking
  from 2026-01-01
2026-03-01 rent waived until 2026-04-15
";
    with_book(text, |book| {
        let rent = book.contract("rent").unwrap();
        let schedule = book.promises.schedule(rent, ScheduleKind::Regular).unwrap();
        let year = window(day(2026, 1, 1), day(2026, 8, 31));
        let owed = due_days(book, "rent", ScheduleKind::Regular, year);
        assert_eq!(owed, ["2026-01-01", "2026-02-01", "2026-05-01", "2026-06-01", "2026-07-01", "2026-08-01"]);
        // An ordinal counts the days that are owed: May is the third, though it is the fifth first of a month.
        let ordinals: Vec<_> =
            [day(2026, 2, 1), day(2026, 5, 1), day(2026, 8, 1)].map(|due| schedule.ordinal(due)).into();
        assert_eq!(ordinals, [Some(1), Some(2), Some(5)]);
        assert_eq!((schedule.ordinal(day(2026, 3, 1)), schedule.ordinal(day(2026, 1, 2))), (None, None));
        for index in 0..6 {
            let due = schedule.nth(index).unwrap();
            assert_eq!(schedule.ordinal(due), Some(index));
            assert_eq!(schedule.before(due), index);
        }
        assert_eq!(schedule.before(day(2026, 4, 1)), 2, "the days before a hole are the days before it");
        assert_eq!(schedule.before(day(2026, 4, 30)), 2, "and so are the days inside it");
    });
}

#[test]
fn the_last_day_of_a_month_is_not_lost_when_a_waiver_ends_in_the_month() {
    // `due` asked from the 16th of March, where the waiver ends, misses 2026-03-31: its step is the 10th.
    let text = "\
contract rent with greystar
  1_000 USD monthly on last from checking
  from 2026-01-10
2026-03-01 rent waived until 2026-03-15
";
    with_book(text, |book| {
        let rent = book.contract("rent").unwrap();
        let owed = due_days(book, "rent", ScheduleKind::Regular, window(day(2026, 1, 1), day(2026, 5, 31)));
        assert_eq!(owed, ["2026-01-31", "2026-02-28", "2026-03-31", "2026-04-30", "2026-05-31"]);
        let walked: Vec<_> = book.contracts[rent].due_days(window(day(2026, 1, 1), day(2026, 5, 31)));
        assert!(!walked.contains(&day(2026, 3, 31)), "the old walk loses it");
    });
}

#[test]
fn a_contract_with_no_from_is_counted_from_the_beginning_of_time_without_walking_there() {
    let text = "\
contract rent with greystar
  1_000 USD monthly on 1 from checking
";
    with_book(text, |book| {
        let rent = book.contract("rent").unwrap();
        let schedule = book.promises.schedule(rent, ScheduleKind::Regular).unwrap();
        let ordinal = schedule.ordinal(day(2026, 2, 1)).unwrap();
        assert!(ordinal > 70_000_000, "{ordinal}: about 71 million months since Day::MIN");
        assert_eq!(schedule.ordinal(day(2026, 3, 1)), Some(ordinal + 1));
        assert_eq!(schedule.nth(ordinal), Some(day(2026, 2, 1)));
    });
}

#[test]
fn a_line_keeps_the_nearest_due_day_and_the_earlier_of_two_equally_near() {
    let text = "\
contract rent with greystar
  1_000 USD monthly on 1 from checking
  from 2026-01-01
";
    with_book(text, |book| {
        let rent = book.contract("rent").unwrap();
        let keeps = |at| book.promises.keep(rent, at);
        let regular = |due| Keep::Kept { schedule: ScheduleKind::Regular, due };
        assert_eq!(keeps(day(2026, 2, 5)), regular(day(2026, 2, 1)));
        assert_eq!(keeps(day(2026, 2, 20)), regular(day(2026, 3, 1)));
        // 2026-02-15 is 14 days from the 1st of February and 14 from the 1st of March: the earlier is kept.
        assert_eq!(keeps(day(2026, 2, 15)), regular(day(2026, 2, 1)));
        assert_eq!(keeps(day(2026, 2, 16)), regular(day(2026, 3, 1)));
        assert_eq!(keeps(day(2025, 12, 31)), Keep::Outside, "before the contract begins");
    });
}

#[test]
fn a_day_equally_near_two_schedules_is_ambiguous() {
    let text = "\
contract invest with greystar
  50 USD monthly on 1 from checking
  buy VTI for 500 USD monthly on 1 from checking
  from 2026-01-01
";
    with_book(text, |book| {
        let invest = book.contract("invest").unwrap();
        let first = day(2026, 2, 1);
        assert_eq!(book.promises.keep(invest, first), Keep::Ambiguous { regular: first, standing: first });
    });
}

#[test]
fn a_loan_is_done_after_its_last_payment() {
    let text = "\
contract car-loan with bank
  loan 3_000 USD on 2026-01-01 at 0% over 3m
  monthly on 1 from checking
";
    with_book(text, |book| {
        let promise = book.promises.of(book.contract("car-loan").unwrap());
        let every = promise.regular.unwrap().every;
        let mut owed = Residual::start(&book.promises, every);
        let mut payments = Vec::new();
        while let Some(next) = owed.next() {
            payments.push((next.to_string(), owed.open().0));
            owed.advance(&book.promises);
        }
        // The loan was made on 2026-01-01, so its payments are the first of the three months after it.
        assert_eq!(
            payments,
            [
                ("2026-02-01".to_string(), 300_000),
                ("2026-03-01".to_string(), 200_000),
                ("2026-04-01".to_string(), 100_000)
            ]
        );
        assert!(owed.is_done());
    });
}

#[test]
fn a_deadline_is_carried_to_the_residual() {
    let text = "\
contract rent with greystar
  1_000 USD monthly on 1 into checking
  from 2026-01-01
  due 5d else + 5% of 100 USD #fees
";
    with_book(text, |book| {
        let rent = book.contract("rent").unwrap();
        let promise = book.promises.of(rent);
        let Term::Every { body, .. } = book.promises.term(promise.regular.unwrap().every) else { panic!("an Every") };
        let Term::Due { deadline, blame, .. } = book.promises.term(body) else { panic!("a Due") };
        assert_eq!(book.promises.deadline(deadline).after, axiom_core::Span::days(5));
        assert_eq!(blame, book.contracts[rent].party, "money into the owner's account is owed by the party");
        let owed = Residual::start(&book.promises, promise.regular.unwrap().every);
        assert_eq!(owed.next(), Some(day(2026, 1, 1)));
        assert_eq!(owed.deadline(&book.promises), Some(day(2026, 1, 6)));
    });
}

#[test]
fn a_grace_is_carried_though_nothing_reads_it() {
    let text = "\
contract rent with greystar
  1_000 USD monthly on 1 into checking
  from 2026-01-01
  grace 3d
";
    with_book(text, |book| {
        assert_eq!(book.promises.of(book.contract("rent").unwrap()).grace, Some(axiom_core::Span::days(3)));
    });
}

#[test]
fn the_types_are_small() {
    assert!(size_of::<Term>() <= 16, "{}", size_of::<Term>());
    assert!(size_of::<Residual>() <= 24, "{}", size_of::<Residual>());
}
