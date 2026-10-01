//! Views over books written as source text, run through the whole pipeline.
//!
//! What a view should say depends on what the engine made of a flow, so the
//! behaviour that turns on it is tested from `.ax` text to report.

use std::borrow::Cow;

use axiom_core::{Day, FileId, Loc, Qty};
use axiom_engine::{Options, Run};
use axiom_model::{Book, Source};

use crate::tests::{lines, show};
use crate::{FlowBy, Query, SourcePosition, SourceProvider};

fn day(y: i32, m: u32, d: u32) -> Day {
    Day::from_ymd(y, m, d).unwrap()
}

/// Compiles `text` as a project of one file, runs it through `today`, and
/// hands the book and the run to `then`. The book must have no errors.
pub(crate) fn with_run<R>(text: &str, today: Day, then: impl FnOnce(&Book, &Run) -> R) -> R {
    let (file, parsed) = axiom_syntax::parse(FileId(0), text, axiom_syntax::Folder::default());
    assert!(parsed.is_empty(), "the source does not parse: {parsed:?}");
    let (book, built) = axiom_model::build(&[Source {
        path: "axiom.ax",
        file,
        embedded: false,
    }]);
    assert!(
        built.iter().all(|diagnostic| !diagnostic.is_error()),
        "the book has errors: {built:?}"
    );
    let run = axiom_engine::run(
        &book,
        Options {
            today,
            relaxed: false,
        },
    );
    then(&book, &run)
}

/// The rows of the first section of the report `query` asks for.
fn rows(book: &Book, run: &Run, query: Query) -> Vec<String> {
    lines(
        &crate::report(book, run, &query, None)
            .expect("the query resolves")
            .sections[0],
    )
}

struct BorrowedSources<'a> {
    path: &'a str,
    text: &'a str,
}

impl SourceProvider for BorrowedSources<'_> {
    fn locate(&self, path: &str, line: usize) -> Option<Loc> {
        if path != self.path || line == 0 {
            return None;
        }
        let mut start = 0;
        for (index, part) in self.text.split_inclusive('\n').enumerate() {
            if index + 1 == line {
                let end = start + part.trim_end_matches(['\n', '\r']).len();
                return Some(Loc::new(FileId(0), start as u32, end as u32));
            }
            start += part.len();
        }
        None
    }

    fn describe(&self, loc: Loc) -> Option<SourcePosition<'_>> {
        let (start, end) = (loc.start as usize, loc.end as usize);
        if loc.file != FileId(0)
            || start > end
            || end > self.text.len()
            || !self.text.is_char_boundary(start)
            || !self.text.is_char_boundary(end)
        {
            return None;
        }
        let line = self.text[..start]
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count();
        let line_start = self.text[..start].rfind('\n').map_or(0, |at| at + 1);
        let column = self.text[line_start..start].chars().count() + 1;
        Some(SourcePosition {
            path: self.path,
            line: line + 1,
            column,
        })
    }
}

#[test]
fn why_source_lines_resolve_in_the_report_layer_with_borrowed_windows_paths() {
    let source = "\
base USD
commodity USD
  precision 2
account assets/checking
account expenses/food
opening 2026-01-01
  checking 100 USD
2026-01-02 checking -> food 5 USD
";
    let provider = BorrowedSources {
        path: r"C:\ledger\january.ax",
        text: source,
    };
    let line = source
        .lines()
        .position(|text| text.starts_with("2026-01-02"))
        .unwrap()
        + 1;
    with_run(source, day(2026, 1, 3), |book, run| {
        let target = format!(r"C:\ledger\january.ax:{line}");
        let query = Query::Why { target: &target };
        let report = crate::report_with_sources(book, run, &query, None, &provider).unwrap();
        let context = crate::Context::new(
            book,
            Options {
                today: run.today,
                relaxed: book.relaxed,
            },
            None,
        )
        .unwrap();
        let shared = context.report_with_sources(&query, &provider).unwrap();
        assert_eq!(report.title, "Why this line");
        assert_eq!(show(&shared), show(&report));
        assert!(
            crate::tests::lines(&report.sections[0])[0]
                .contains("flow: assets/checking → expenses/food")
        );
        assert!(crate::resolve_source_line(&query, &provider).is_some());
        let unknown = Query::Why {
            target: r"C:\ledger\missing.ax:1",
        };
        assert_eq!(crate::resolve_source_line(&unknown, &provider), None);
    });
}

/// A parent purpose's total can be zero while two child purposes remain useful
/// facts. Keep the hierarchy visible from those nonzero child rows.
#[test]
fn purpose_tree_keeps_offsetting_children_when_the_parent_nets_to_zero() {
    let source = "\
base USD
commodity USD
  precision 2

entity me
entity grocer
entity diner
account checking : asset

purpose food : spending
purpose groceries : food
purpose dining : food

opening 2026-01-01
  checking 500.00 USD

2026-01-02 checking -> grocer 50.00 USD #groceries
2026-01-03 diner -> checking 50.00 USD #dining
";

    with_run(source, day(2026, 1, 4), |book, run| {
        let report = crate::report(
            book,
            run,
            &Query::Flow {
                by: FlowBy::Period(axiom_model::Period::Month),
                from: Some(day(2026, 1, 1)),
                to: None,
            },
            None,
        )
        .unwrap();
        let rows = &report.sections[0].rows;
        let amount = |purpose: &str| {
            rows.iter()
                .find(|row| row.cells.first().is_some_and(|cell| matches!(cell, crate::Cell::Name(name) if *name == purpose)))
                .and_then(|row| row.cells.get(1))
        };
        let crate::Cell::Amount { qty: groceries, .. } = amount("groceries").unwrap() else {
            panic!("groceries has a nonzero amount cell")
        };
        let crate::Cell::Amount { qty: dining, .. } = amount("dining").unwrap() else {
            panic!("dining has a nonzero amount cell")
        };
        assert_eq!((*groceries, *dining), (Qty(5_000), Qty(-5_000)));
        assert!(amount("food").is_none_or(|cell| matches!(cell, crate::Cell::Blank)));
    });
}

/// Party detail remains useful when separate counterparties offset in the
/// spending total. The compact backing grid must retain each row independently.
#[test]
fn party_flow_keeps_offsetting_counterparty_rows() {
    let source = "\
base USD
commodity USD
  precision 2

entity me
entity grocer
entity diner
account checking : asset

purpose food : spending
purpose groceries : food
purpose dining : food

opening 2026-01-01
  checking 500.00 USD

2026-01-02 checking -> grocer 50.00 USD #groceries
2026-01-03 diner -> checking 50.00 USD #dining
";

    with_run(source, day(2026, 1, 4), |book, run| {
        let report = crate::report(
            book,
            run,
            &Query::Flow {
                by: FlowBy::Party,
                from: Some(day(2026, 1, 1)),
                to: None,
            },
            None,
        )
        .unwrap();
        let rows = &report.sections[0].rows;
        let amount = |party: &str| {
            rows.iter()
                .find(|row| {
                    row.cells.first().is_some_and(
                        |cell| matches!(cell, crate::Cell::Name(name) if *name == party),
                    )
                })
                .and_then(|row| row.cells.get(1))
        };
        let crate::Cell::Amount { qty: paid, .. } = amount("grocer").unwrap() else {
            panic!("grocer keeps its spending row")
        };
        let crate::Cell::Amount { qty: refund, .. } = amount("diner").unwrap() else {
            panic!("diner keeps its offsetting income row")
        };
        assert_eq!((*paid, *refund), (Qty(5_000), Qty(-5_000)));
    });
}

#[test]
fn decoded_flow_descriptions_stay_borrowed_in_report_cells() {
    let source = r#"base USD
commodity USD
  precision 2

entity me : person
entity cafe
account checking : asset

opening 2026-01-01
  checking 100 USD

2026-01-02 checking -> cafe 5 USD "line one\nline two"
"#;
    with_run(source, day(2026, 1, 3), |book, run| {
        let report = crate::report(
            book,
            run,
            &Query::Flow {
                by: FlowBy::Period(axiom_model::Period::Month),
                from: Some(day(2026, 1, 1)),
                to: None,
            },
            None,
        )
        .unwrap();
        assert!(report.sections[0].rows.iter().any(|row| {
            row.cells.iter().any(|cell| {
                matches!(cell, crate::Cell::Text(Cow::Borrowed(text)) if *text == "line one\nline two")
            })
        }));
    });
}

/// A month with only a measure still starts the default flow window. Otherwise
/// a measure-only project would be reported in a synthetic day at `today`.
#[test]
fn a_measure_only_book_anchors_flow_to_its_first_measure() {
    let source = "\
base USD
entity me
entity halcyon
commodity HR : measure

2026-03-04 me worked 6 HR for halcyon
";

    with_run(source, day(2026, 4, 10), |book, run| {
        let report = crate::report(
            book,
            run,
            &Query::Flow {
                by: FlowBy::Period(axiom_model::Period::Month),
                from: None,
                to: None,
            },
            None,
        )
        .unwrap();
        let measures = report
            .sections
            .iter()
            .find(|section| section.heading.as_deref() == Some("Measures"))
            .expect("measure section");
        let headers: Vec<_> = measures
            .columns
            .iter()
            .map(|column| match &column.title {
                crate::Cell::Text(text) => text.as_ref(),
                _ => "",
            })
            .collect();
        let march = headers.iter().position(|&header| header == "2026-03").unwrap();
        let april = headers.iter().position(|&header| header == "2026-04").unwrap();
        assert!(march < april, "the earlier measure anchors the displayed range");
        assert!(measures.rows.iter().any(|row| {
            row.cells.iter().any(|cell| matches!(cell, crate::Cell::Name("HR")))
                && row.cells.iter().any(|cell| matches!(cell, crate::Cell::Amount { qty, unit: "HR", .. } if !qty.is_zero()))
        }));
    });
}

/// A contract may share its name with its party. Bare targets select the
/// contract, and `entity:NAME` keeps the party register addressable.
#[test]
fn register_resolves_a_contract_sharing_its_partys_name() {
    let source = "\
base USD
commodity USD
  precision 2

entity me : person
entity figma
account checking : asset

contract figma with figma
  15 USD monthly on 3 from checking

asset laptop : thing
";

    with_run(source, day(2026, 1, 5), |book, run| {
        let contract = crate::report(
            book,
            run,
            &Query::Register {
                place: "figma",
                from: None,
                to: None,
            },
            None,
        )
        .unwrap();
        assert!(contract.sections.iter().flat_map(|s| &s.rows).any(|row| {
            row.cells
                .iter()
                .any(|cell| matches!(cell, crate::Cell::Word("terms active")))
        }));

        let party = crate::report(
            book,
            run,
            &Query::Register {
                place: "entity:figma",
                from: None,
                to: None,
            },
            None,
        )
        .unwrap();
        assert!(!party.sections.iter().flat_map(|s| &s.rows).any(|row| {
            row.cells
                .iter()
                .any(|cell| matches!(cell, crate::Cell::Word("terms active")))
        }));

        let why_contract = crate::report(
            book,
            run,
            &Query::Why { target: "figma" },
            None,
        )
        .unwrap();
        assert_eq!(why_contract.sections[0].heading.as_deref(), Some("Terms over time"));
        let why_party = crate::report(
            book,
            run,
            &Query::Why { target: "entity:figma" },
            None,
        )
        .unwrap();
        assert_eq!(why_party.sections[0].heading.as_deref(), Some("Places"));

        let why_asset = crate::report(
            book,
            run,
            &Query::Why { target: "asset:laptop" },
            None,
        )
        .unwrap();
        assert_eq!(why_asset.title, "Why laptop");
        assert_eq!(why_asset.sections[0].heading.as_deref(), Some("Asset"));
    });
}

// ─── Basis flows ────────────────────────────────────────────────────────────

/// An improvement to a holding: 100 USD paid into the basis of ten shares.
const DISCOUNT: &str = "\
base USD
commodity USD
  precision 2
commodity UNH

account assets/broker
account assets/checking
account income/discount

opening 2025-01-01
  broker    10 UNH basis 1_000 USD since 2024-01-01
  checking  5_000 USD

2025-01-01 UNH 150 USD
2025-02-01 income/discount -> broker[2024-01-01].basis 100 USD
";

/// What the shares fetch is their price, whatever their basis: the 100 USD
/// changed what they cost, and nothing arrived in the account.
#[test]
fn a_basis_flow_is_worth_nothing_to_the_place_it_rebases() {
    with_run(DISCOUNT, day(2025, 6, 1), |book, run| {
        let balance = Query::Balance {
            globs: vec![],
            at: None,
            value: true,
            monthly: false,
        };
        let report = crate::report(book, run, &balance, None).unwrap();
        let worth = lines(&report.sections[1]);
        assert_eq!(
            worth[2], "=Net worth | 6,500.00 USD",
            "1,500 of shares and 5,000 of cash"
        );
        let broker = lines(&report.sections[0])
            .into_iter()
            .find(|row| row.starts_with("  broker"))
            .unwrap();
        assert_eq!(broker, "  broker | 1,500.00 USD");
    });
}

#[test]
fn a_register_lists_a_basis_flow_as_a_change_of_basis_not_an_amount() {
    with_run(DISCOUNT, day(2025, 6, 1), |book, run| {
        let register = Query::Register {
            place: "broker",
            from: None,
            to: None,
        };
        assert_eq!(
            rows(book, run, register),
            [
                "2025-01-01 | equity/opening |  |  | 10 UNH | 10 UNH",
                "2025-02-01 | income/discount |  | basis +100.00 USD |  |",
            ]
        );
    });
}

#[test]
fn a_register_lists_an_exchange_inside_one_place_at_both_of_its_ends() {
    let source = "\
base USD
commodity USD
  precision 2
commodity GBP
  precision 2

account assets/wise

opening 2025-01-01
  wise 1_000 USD

2025-02-01 wise 500 USD -> wise 400 GBP
";
    with_run(source, day(2025, 6, 1), |book, run| {
        let register = Query::Register {
            place: "wise",
            from: None,
            to: None,
        };
        assert_eq!(
            rows(book, run, register),
            [
                "2025-01-01 | equity/opening |  |  | 1,000.00 USD | 1,000.00 USD",
                "2025-02-01 | assets/wise |  |  | -500.00 USD | 500.00 USD",
                "2025-02-01 | assets/wise |  |  | 400.00 GBP | 400.00 GBP",
            ]
        );
    });
}

// ─── Laws ───────────────────────────────────────────────────────────────────

#[test]
fn why_a_law_says_the_day_a_closing_law_judges_the_year() {
    let source = "\
base USD
commodity USD
  precision 2

account assets/checking

/// Figures the year's return.
law return
  each year closing 04-15
  count 1 USD as returns

law audit
  each year
  count 1 USD as audits
";
    with_run(source, day(2026, 6, 1), |book, run| {
        let when = |law| {
            let report = crate::report(book, run, &Query::Why { target: law }, None).unwrap();
            lines(&report.sections[0])
                .into_iter()
                .find(|row| row.starts_with("When"))
                .unwrap()
        };
        assert_eq!(when("return"), "When | each year closing 04-15");
        assert_eq!(when("audit"), "When | each year");
    });
}

// ─── Plans ──────────────────────────────────────────────────────────────────

/// Three plans that say more than where the money goes: a premium paid each
/// January for the year, a savings deposit held for an envelope, and a
/// depreciation that changes basis.
const PLANS: &str = "\
base USD
commodity USD
  precision 2
commodity HOME

kind envelope : entity
  restricted

entity car-fund : envelope

account assets/checking
account assets/savings
account assets/house
account expenses/insurance
account expenses/depreciation

plan premium every year from 2026-01-15 checking -> insurance 1_200 USD for 2026
plan deposit every month on 1 from 2026-01-01 checking -> savings 100 USD for car-fund basis 90 USD
plan depreciation every month on 28 from 2026-01-28 house.basis -> depreciation 300 USD
";

#[test]
fn a_planned_occurrence_keeps_the_terms_of_its_template_and_moves_its_period_along() {
    with_run(PLANS, day(2026, 3, 1), |book, _| {
        let template = |plan: usize| &book.plans.values().nth(plan).unwrap().template[0];
        let usd = |flow: &axiom_model::Flow| flow.out;

        let premium = template(0);
        assert_eq!(
            (premium.recognized.first(), premium.recognized.last()),
            (day(2026, 1, 1), day(2026, 12, 31))
        );
        let next = crate::synth::planned(premium, day(2027, 1, 15), usd(premium), usd(premium));
        assert_eq!(next.day, day(2027, 1, 15));
        assert_eq!(
            (next.recognized.first(), next.recognized.last()),
            (day(2027, 1, 1), day(2027, 12, 31))
        );

        let deposit = template(1);
        let next = crate::synth::planned(deposit, day(2026, 5, 1), usd(deposit), usd(deposit));
        assert_eq!(next.detail, deposit.detail);
        assert!(
            next.detail().hold.is_some() && next.detail().basis.is_some(),
            "held for the envelope, at 90 USD of basis"
        );

        let depreciation = template(2);
        let next = crate::synth::planned(
            depreciation,
            day(2026, 5, 28),
            usd(depreciation),
            usd(depreciation),
        );
        assert_eq!(next.detail().basis_end, Some(axiom_model::End::From));
        assert_eq!(
            crate::places::route(book, &next),
            "assets/house.basis → expenses/depreciation"
        );
    });
}

/// A plan of depreciation, and the law that recaptures what it took.
const DEPRECIATING: &str = "\
base USD
commodity USD
  precision 2
commodity HOME
  precision 0

kind property : asset
  liquidity 90d

entity treasury

account assets/house : property
account assets/checking
account income/salary
account expenses/depreciation

opening 2026-01-01
  house 1 HOME basis 120_000 USD
  checking 5_000 USD

plan depreciation every month on 28 from 2026-01-28 house.basis -> depreciation 300 USD

law count-depreciation
  on in
  when to is expenses/depreciation
  count amount as depreciation

law recapture
  each year
  owe tally(depreciation) * 25% to treasury as recapture

2026-01-05 income/salary -> checking 100 USD
";

/// The plan lowers the house's basis and recognizes the expense: the laws see
/// ten months of depreciation, and no money leaves the house, which holds none.
#[test]
fn the_forecast_runs_a_plan_that_changes_basis_as_one_that_moves_no_money() {
    with_run(DEPRECIATING, day(2026, 3, 15), |book, run| {
        let forecast = Query::Forecast {
            until: Some(day(2026, 12, 31)),
            paths: 1,
        };
        let report = crate::report(book, run, &forecast, None).unwrap();
        let section = |heading: &str| {
            report
                .sections
                .iter()
                .find(|s| s.heading.as_deref() == Some(heading))
                .unwrap()
        };
        let outlook = lines(section("Liquid net worth"));
        assert_eq!(outlook[0], "2026-03-15 | 5,100.00 USD | 5,100.00 USD");
        assert_eq!(
            outlook.last().unwrap(),
            "2026-12-31 | 4,350.00 USD | 5,100.00 USD",
            "5,100 less the 750 recaptured"
        );
        assert_eq!(
            lines(section("Obligations coming due")),
            ["2026-12-31 | recapture | treasury | 750.00 USD"]
        );
        assert!(
            lines(section("Problems ahead")).is_empty(),
            "the house is not overdrawn"
        );
    });
}

/// A withdrawal that `available` invents is not a plan: it says nothing about
/// period, envelope or basis, whatever the flow it borrows its line from said.
#[test]
fn a_hypothetical_flow_borrows_only_the_transaction_and_the_line() {
    with_run(PLANS, day(2026, 3, 1), |book, _| {
        let deposit = &book.plans.values().nth(1).unwrap().template[0];
        let flow = crate::synth::hypothetical(
            deposit,
            day(2026, 4, 2),
            deposit.to,
            deposit.from,
            deposit.out,
            deposit.out,
        );
        assert_eq!((flow.txn, flow.loc), (deposit.txn, deposit.loc));
        assert_eq!(
            (flow.detail, flow.recognized.first(), flow.recognized.last()),
            (None, day(2026, 4, 2), day(2026, 4, 2))
        );
    });
}

// ─── Taxes before the return closes ─────────────────────────────────────────

/// A tally counted as the year goes, and a tax figured from it on April 15 of
/// the next. The last flow is in 2027, so the journal itself reaches into the
/// year after the one taxed.
const RETURN: &str = "\
base USD
commodity USD
  precision 2

entity treasury

account assets/checking
account income/salary

law count-pay
  on in
  when to is assets/checking
  count amount as pay

law return
  each year closing 04-15
  owe tally(pay) * 10% to treasury as income-tax

2026-03-01 income/salary -> checking 1_000 USD
2026-09-01 income/salary -> checking 1_000 USD
2027-01-05 income/salary -> checking 500 USD
";

#[test]
fn tax_says_the_return_is_not_closed_and_leaves_what_it_owes_out_instead_of_at_zero() {
    with_run(RETURN, day(2027, 3, 1), |book, run| {
        let tax = Query::Tax { year: Some(2026) };
        let report = crate::report(book, run, &tax, None).unwrap();
        let [counted, owed] = &report.sections[..] else {
            panic!("two sections: {}", show(&report))
        };
        assert_eq!(
            lines(counted),
            ["=project |  |", "  pay | 2,000.00 USD | 2 sources"]
        );
        assert!(owed.rows.is_empty(), "no line of the return is figured yet");
        assert_eq!(
            owed.notes,
            [
                "The 2026 return closes on 2027-04-15; what it owes is not figured yet; the tallies are counted so far."
            ]
        );
    });
}

#[test]
fn tax_after_the_return_closes_shows_what_it_owes() {
    with_run(RETURN, day(2027, 4, 20), |book, run| {
        let tax = Query::Tax { year: Some(2026) };
        let report = crate::report(book, run, &tax, None).unwrap();
        let [_, owed] = &report.sections[..] else {
            panic!("two sections: {}", show(&report))
        };
        assert_eq!(
            lines(owed)[1],
            "  income-tax | treasury | 2027-04-15 | 200.00 USD | period end"
        );
        assert!(
            owed.notes.iter().all(|note| !note.contains("not figured")),
            "{:?}",
            owed.notes
        );
    });
}

#[test]
fn context_views_match_the_legacy_views_before_today_today_and_after_today() {
    with_run(RETURN, day(2027, 3, 1), |book, run| {
        let context = crate::Context::new(
            book,
            Options {
                today: run.today,
                relaxed: book.relaxed,
            },
            None,
        )
        .unwrap();
        for at in [day(2026, 10, 1), run.today, day(2027, 4, 20)] {
            for query in [
                Query::Balance {
                    globs: vec![],
                    at: Some(at),
                    value: false,
                    monthly: false,
                },
                Query::Register {
                    place: "checking",
                    from: None,
                    to: Some(at),
                },
                Query::Flow {
                    by: FlowBy::Period(axiom_model::Period::Month),
                    from: None,
                    to: Some(at),
                },
                Query::Available { at: Some(at) },
                Query::Claims { at: Some(at) },
                Query::Lots {
                    place: None,
                    at: Some(at),
                },
                Query::Forecast {
                    until: Some(at),
                    paths: 0,
                },
            ] {
                let shared = context.report(&query).unwrap();
                let old = crate::report(book, context.run(), &query, None).unwrap();
                assert_eq!(show(&shared), show(&old), "{query:?} at {at}");
            }
        }
    });
}

#[test]
fn a_context_forecast_keeps_historical_and_same_day_obligations_once() {
    let source = "\
base USD
commodity USD
  precision 2

entity treasury

account assets/checking
account income/reserve
account income/salary
account expenses/food

law count-pay
  on in
  when from is income/salary
  count amount as pay

law historical-fee
  on out
  when from is assets/checking
  require amount < 0 USD else owe 5 USD to treasury by date(2027, 2, 15) as historical-fee

law pad-fee
  on in
  when from is income/reserve
  owe 2 USD to treasury by date(2027, 3, 1) as pad-fee

law year-end-tax
  each year
  owe tally(pay) * 10% to treasury by date(year + 1, 1, 15) as year-end-tax

2026-01-05 income/salary -> checking 100 USD
2026-02-01 checking -> expenses/food 10 USD
2026-12-31 checking = 100 USD via reserve
";

    with_run(source, day(2026, 12, 31), |book, run| {
        let context = crate::Context::new(
            book,
            Options {
                today: run.today,
                relaxed: book.relaxed,
            },
            None,
        )
        .unwrap();
        let query = Query::Forecast {
            until: Some(day(2027, 3, 1)),
            paths: 0,
        };
        let shared = context.report(&query).unwrap();
        let old = crate::report(book, context.run(), &query, None).unwrap();
        assert_eq!(show(&shared), show(&old));

        let owed = shared
            .sections
            .iter()
            .find(|section| section.heading.as_deref() == Some("Obligations coming due"))
            .unwrap();
        assert_eq!(
            lines(owed),
            [
                "2027-01-15 | year-end-tax | treasury | 10.00 USD",
                "2027-02-15 | historical-fee | treasury | 5.00 USD",
                "2027-03-01 | pad-fee | treasury | 2.00 USD",
            ],
            "the prefix includes the old flow and pre-close pad, while the year-end close is resumed once"
        );
    });
}

#[test]
fn a_context_checkpoint_keeps_same_day_closings_pending_for_a_withdrawal() {
    with_run(YEAR_END_RETURN, day(2026, 6, 1), |book, run| {
        let context = crate::Context::new(
            book,
            Options {
                today: run.today,
                relaxed: book.relaxed,
            },
            None,
        )
        .unwrap();
        let query = Query::Available {
            at: Some(day(2026, 12, 31)),
        };
        let shared = context.report(&query).unwrap();
        let old = crate::report(book, context.run(), &query, None).unwrap();
        assert_eq!(show(&shared), show(&old));
        let reach = shared
            .sections
            .iter()
            .find(|section| {
                section.heading.as_deref() == Some("What it would take to reach the rest")
            })
            .unwrap();
        assert_eq!(
            lines(reach)[0],
            "assets/ira | 30d | 10,000.00 USD | 2,000.00 USD | 8,000.00 USD | driven by income-tax 2,000.00 USD"
        );
    });
}

#[test]
fn a_context_scopes_views_to_the_named_owner() {
    let source = "\
base USD
commodity USD
  precision 2

entity me
entity jordan

account assets/mine
  owner me
account assets/theirs
  owner jordan

opening 2026-01-01
  mine   100 USD
  theirs 200 USD
";
    with_run(source, day(2026, 1, 2), |book, run| {
        let make = |owner| {
            crate::Context::new(
                book,
                Options {
                    today: run.today,
                    relaxed: book.relaxed,
                },
                Some(owner),
            )
            .unwrap()
        };
        let query = Query::Balance {
            globs: vec![],
            at: None,
            value: false,
            monthly: false,
        };
        let mine = make("me").report(&query).unwrap();
        let jordan = make("jordan").report(&query).unwrap();
        assert!(show(&mine).contains("mine | 100.00 USD"));
        assert!(!show(&mine).contains("theirs | 200.00 USD"));
        assert!(show(&jordan).contains("theirs | 200.00 USD"));
        assert!(!show(&jordan).contains("mine | 100.00 USD"));

        let register = crate::report(
            book,
            run,
            &Query::Register {
                place: "jordan",
                from: None,
                to: None,
            },
            Some("me"),
        )
        .unwrap();
        assert!(!show(&register).contains("200.00 USD"));
        let explanation = crate::report(
            book,
            run,
            &Query::Why { target: "jordan" },
            Some("me"),
        )
        .unwrap();
        assert!(!show(&explanation).contains("theirs | 200.00 USD"));
    });
}

/// One of two returns has closed: what it owes is listed, and the total is what
/// is owed so far.
#[test]
fn tax_with_one_return_closed_and_one_not_totals_what_is_owed_so_far() {
    let state = "\nlaw state-return\n  each year closing 06-15\n  owe tally(pay) * 5% to treasury as state-tax\n";
    with_run(&format!("{RETURN}{state}"), day(2027, 5, 1), |book, run| {
        let tax = Query::Tax { year: Some(2026) };
        let report = crate::report(book, run, &tax, None).unwrap();
        let [_, owed] = &report.sections[..] else {
            panic!("two sections: {}", show(&report))
        };
        assert_eq!(
            lines(owed),
            [
                "=project |  |  |  |",
                "  income-tax | treasury | 2027-04-15 | 200.00 USD | period end",
                "=Total owed so far |  |  | 200.00 USD |",
            ]
        );
        assert_eq!(
            owed.notes[0],
            "The 2026 return closes on 2027-06-15; what it owes is not figured yet; the tallies are counted so far."
        );
    });
}

// ─── Budgets ────────────────────────────────────────────────────────────────

/// A monthly budget that nothing touched in February, and one that nothing
/// touched at all.
const ENVELOPES: &str = "\
base USD
commodity USD
  precision 2

account assets/checking
account expenses/dining
  budget 150 USD monthly
account expenses/clothing
  budget 80 USD monthly

opening 2026-01-01
  checking 1_000 USD

2026-01-10 checking -> dining 120 USD
";

/// The year is its months so far, each one read: a month no flow reached is
/// wholly unspent, not missing.
#[test]
fn a_years_budget_reads_every_month_so_far_even_those_nothing_touched() {
    with_run(ENVELOPES, day(2026, 2, 14), |book, run| {
        let budget = Query::Budget {
            at: Some(day(2026, 1, 1)),
            by: axiom_model::Period::Year,
        };
        assert_eq!(
            rows(book, run, budget),
            [
                "expenses/clothing | budget | 2026 | 0.00 USD | 160.00 USD | 160.00 USD | 0%",
                "~   |  | 2026-01 | 0.00 USD | 80.00 USD | 80.00 USD | 0%",
                "~   |  | 2026-02 | 0.00 USD | 80.00 USD | 80.00 USD | 0%",
                "expenses/dining | budget | 2026 | 120.00 USD | 300.00 USD | 180.00 USD | 40%",
                "~   |  | 2026-01 | 120.00 USD | 150.00 USD | 30.00 USD | 80%",
                "~   |  | 2026-02 | 0.00 USD | 150.00 USD | 150.00 USD | 0%",
            ]
        );
    });
}

// ─── Looking ahead to the day a return closes ───────────────────────────────

/// An account whose withdrawals count as income (it has only its opening
/// balance: no flow of the journal touches it), and a return that taxes the
/// year's income on April 15 of the next.
const IRA: &str = "\
base USD
commodity USD
  precision 2

kind retirement : asset
  liquidity 30d
  law count-withdrawals
    on out
    count amount as income

entity treasury

account assets/checking
account assets/ira : retirement
account income/salary

opening 2026-01-01
  checking 1_000 USD
  ira      10_000 USD

law return
  each year closing 04-15
  owe tally(income) * 20% to treasury as income-tax

2026-01-05 income/salary -> checking 100 USD
";

/// Drawing the account down in June makes income in 2026, and the 2026 return
/// closes in April 2027: the tax on the withdrawal is a cost of making it.
#[test]
fn available_runs_the_books_to_the_day_the_return_closes_to_price_a_withdrawal() {
    with_run(IRA, day(2026, 6, 1), |book, run| {
        let available = Query::Available { at: None };
        let report = crate::report(book, run, &available, None).unwrap();
        let reach = report
            .sections
            .iter()
            .find(|s| s.heading.as_deref() == Some("What it would take to reach the rest"));
        assert_eq!(
            lines(reach.unwrap())[0],
            "assets/ira | 30d | 10,000.00 USD | 2,000.00 USD | 8,000.00 USD | driven by income-tax 2,000.00 USD"
        );
    });
}

/// The IRA again, with a return that is figured on the last day of the year it
/// judges, the day the withdrawal below is made.
const YEAR_END_RETURN: &str = "\
base USD
commodity USD
  precision 2

kind retirement : asset
  liquidity 30d
  law count-withdrawals
    on out
    count amount as income

entity treasury

account assets/checking
account assets/ira : retirement
account income/salary

opening 2026-01-01
  checking 1_000 USD
  ira      10_000 USD

law return
  each year
  owe tally(income) * 20% to treasury as income-tax

2026-01-05 income/salary -> checking 100 USD
";

/// What is drawn on the day a year ends is a fact of that day, and the law that
/// figures the year on that day comes after it: the withdrawal is priced with the
/// tax it makes, as it is on any other day of the year.
#[test]
fn a_withdrawal_on_the_last_day_of_the_year_is_taxed_by_the_law_that_closes_the_year() {
    for today in [day(2026, 6, 1), day(2026, 12, 31)] {
        with_run(YEAR_END_RETURN, today, |book, run| {
            let report = crate::report(book, run, &Query::Available { at: None }, None).unwrap();
            let reach = report
                .sections
                .iter()
                .find(|s| s.heading.as_deref() == Some("What it would take to reach the rest"))
                .unwrap();
            assert_eq!(
                lines(reach)[0],
                "assets/ira | 30d | 10,000.00 USD | 2,000.00 USD | 8,000.00 USD | driven by income-tax 2,000.00 USD",
                "on {today}"
            );
        });
    }
}

/// A salary each month, taxed by a return that closes in April 2027.
const SALARY: &str = "\
base USD
commodity USD
  precision 2

entity treasury

account assets/checking
account income/salary

law count-pay
  on in
  when to is assets/checking
  count amount as pay

law return
  each year closing 04-15
  owe tally(pay) * 10% to treasury as income-tax

every month on 5 income/salary -> checking 1_000 USD

2026-01-05 income/salary -> checking 1_000 USD
";

/// A year from today ends in January 2027, three months before the return of
/// 2026 closes: the forecast goes on to that day, to show what the year owes.
#[test]
fn the_forecast_goes_on_to_the_next_closing_day_when_it_is_close_after_its_horizon() {
    with_run(SALARY, day(2026, 1, 10), |book, run| {
        let forecast = |until| Query::Forecast { until, paths: 1 };
        let report = crate::report(book, run, &forecast(None), None).unwrap();
        assert_eq!(report.title, "Forecast to 2027-04-15");
        let owed = report
            .sections
            .iter()
            .find(|s| s.heading.as_deref() == Some("Obligations coming due"))
            .unwrap();
        assert_eq!(
            lines(owed),
            ["2027-04-15 | income-tax | treasury | 1,200.00 USD"]
        );

        // What was asked for is what is shown.
        let asked = crate::report(book, run, &forecast(Some(day(2027, 1, 10))), None).unwrap();
        assert_eq!(asked.title, "Forecast to 2027-01-10");
    });
}

/// Farther than a few months, the next closing day is another year's business.
#[test]
fn the_forecast_stops_at_a_year_when_the_next_closing_day_is_far() {
    with_run(SALARY, day(2026, 5, 10), |book, run| {
        let report = crate::report(
            book,
            run,
            &Query::Forecast {
                until: None,
                paths: 1,
            },
            None,
        )
        .unwrap();
        assert_eq!(report.title, "Forecast to 2027-05-10");
    });
}

// ─── Accepted gaps ──────────────────────────────────────────────────────────

/// One gap of each kind: a revaluation, a gap accepted as unexplained, and a
/// gap that came out of another account.
const GAPS: &str = "\
base USD
commodity USD
  precision 2

account assets/k
account assets/checking

opening 2025-01-01
  k        10_000 USD
  checking  5_000 USD

2025-03-31 k = 9_000 USD via market
2025-06-30 k = 9_500 USD !
2025-09-30 k = 9_800 USD via checking
";

#[test]
fn a_register_says_where_each_gap_came_from() {
    with_run(GAPS, day(2025, 12, 31), |book, run| {
        let register = Query::Register {
            place: "k",
            from: None,
            to: None,
        };
        assert_eq!(
            rows(book, run, register),
            [
                "2025-01-01 | equity/opening |  |  | 10,000.00 USD | 10,000.00 USD",
                "2025-03-31 | income/market |  | revalued via income/market | -1,000.00 USD | 9,000.00 USD",
                "2025-06-30 | equity/unknown |  | unexplained gap, accepted with ! | 500.00 USD | 9,500.00 USD",
                "2025-09-30 | assets/checking |  | gap via assets/checking | 300.00 USD | 9,800.00 USD",
            ]
        );
    });
}

#[test]
fn the_line_of_an_assertion_says_where_its_gap_came_from() {
    with_run(GAPS, day(2025, 12, 31), |book, run| {
        let words: Vec<String> = book
            .asserts
            .iter()
            .map(|assertion| {
                let line = Query::Line { loc: assertion.loc };
                lines(&crate::report(book, run, &line, None).unwrap().sections[0])[0].clone()
            })
            .collect();
        assert!(
            words[0].starts_with("assertion: assets/k = 9,000.00 USD, revalued via income/market"),
            "{words:?}"
        );
        assert!(
            words[1].starts_with(
                "assertion: assets/k = 9,500.00 USD, unexplained gap, accepted with !"
            )
        );
        assert!(
            words[2].starts_with("assertion: assets/k = 9,800.00 USD, gap via assets/checking")
        );
    });
}

/// The gap is a flow from its counter place, so that place's register lists it too.
#[test]
fn the_register_of_a_gaps_counter_place_lists_it_as_well() {
    with_run(GAPS, day(2025, 12, 31), |book, run| {
        let register = |place| Query::Register {
            place,
            from: None,
            to: None,
        };
        assert_eq!(
            rows(book, run, register("checking")).last().unwrap(),
            "2025-09-30 | assets/k |  | gap via assets/checking | -300.00 USD | 4,700.00 USD"
        );
        assert_eq!(
            rows(book, run, register("market")),
            [
                "2025-03-31 | assets/k |  | revalued via income/market | -1,000.00 USD | -1,000.00 USD"
            ]
        );
    });
}

#[test]
fn snapshots_apply_assertion_pads_through_each_requested_day() {
    with_run(GAPS, day(2025, 12, 31), |book, run| {
        let whose = crate::lens::Whose::default();
        let lens = crate::lens::Lens::new(book, &whose, run.today);
        let days = [
            day(2025, 1, 1),
            day(2025, 3, 31),
            day(2025, 6, 30),
            day(2025, 9, 30),
        ];
        let snapshots = crate::history::Snapshots::of(lens, run, &days, false);
        let place = book.place("assets/k").unwrap();
        let balances: Vec<_> = (0..days.len())
            .map(|column| snapshots.subtree(book, column, place).get(book.base))
            .collect();
        assert_eq!(
            balances,
            [Qty(1_000_000), Qty(900_000), Qty(950_000), Qty(980_000)]
        );
    });
}

#[test]
fn snapshots_preserve_stock_splits_and_returned_flow_edges() {
    let source = "\
base USD
commodity USD
  precision 2
commodity FAST

account assets/broker
account assets/checking
account income/pay

opening 2025-01-01
  broker 10 FAST

2025-01-02 income/pay -> checking 100 USD #deposit
2025-01-03 FAST split 2 for 1
2025-01-04 #deposit returned
";
    with_run(source, day(2025, 1, 5), |book, run| {
        let whose = crate::lens::Whose::default();
        let lens = crate::lens::Lens::new(book, &whose, run.today);
        let days = [
            day(2025, 1, 1),
            day(2025, 1, 2),
            day(2025, 1, 3),
            day(2025, 1, 4),
        ];
        let snapshots = crate::history::Snapshots::of(lens, run, &days, false);
        let broker = book.place("assets/broker").unwrap();
        let checking = book.place("assets/checking").unwrap();
        let fast = book.commodity("FAST").unwrap();
        let amount = |column, place, unit| snapshots.subtree(book, column, place).get(unit);
        assert_eq!(amount(0, broker, fast), Qty(10));
        assert_eq!(amount(1, broker, fast), Qty(10));
        assert_eq!(amount(2, broker, fast), Qty(20));
        assert_eq!(amount(1, checking, book.base), Qty(10_000));
        assert_eq!(amount(2, checking, book.base), Qty(10_000));
        assert_eq!(amount(3, checking, book.base), Qty::ZERO);
    });
}

/// Depreciation lowers a house's basis and recognizes an expense, and the
/// house still holds its one HOME. A later flow makes the balance replay the
/// journal instead of reading the run's holdings.
const DEPRECIATION: &str = "\
base USD
commodity USD
  precision 2
commodity HOME
  precision 0

account assets/house
account assets/checking
account expenses/depreciation

opening 2025-01-01
  checking 101_000 USD

2025-01-01 checking -> house 1 HOME @ 100_000 USD
2025-02-01 house.basis -> expenses/depreciation 300 USD
2026-05-01 checking -> expenses/depreciation 1 USD
";

#[test]
fn a_balance_replaying_the_journal_books_no_money_out_of_a_place_that_lost_basis() {
    with_run(DEPRECIATION, day(2026, 4, 16), |book, run| {
        let balance = Query::Balance {
            globs: vec![],
            at: None,
            value: false,
            monthly: false,
        };
        assert_eq!(
            rows(book, run, balance),
            [
                "=assets | 1,000.00 USD",
                "= | 1 HOME",
                "  checking | 1,000.00 USD",
                "  house | 1 HOME",
                "=equity | 101,000.00 USD",
                "  opening | 101,000.00 USD",
                "=expenses | 300.00 USD",
                "  depreciation | 300.00 USD",
            ]
        );
    });
}
