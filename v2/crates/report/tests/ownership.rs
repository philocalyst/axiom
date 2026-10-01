use axiom_core::{Day, FileId, Qty};
use axiom_engine::Options;
use axiom_model::Source;
use axiom_report::{Cell, Context, Query};

fn day(year: i32, month: u32, date: u32) -> Day {
    Day::from_ymd(year, month, date).unwrap()
}

fn with_context<T>(owner: Option<&str>, then: impl FnOnce(&Context<'_, '_>) -> T) -> T {
    let source = "\
base USD
commodity USD
  precision 2
entity me
entity jordan
account assets/fund
account assets/shared
  owner me 60%, jordan 40%
opening 2026-01-01
  fund 0.02 USD
2026-01-02 fund -> shared 0.01 USD
2026-01-03 fund -> shared 0.01 USD
";
    let (file, parsed) = axiom_syntax::parse(FileId(0), source, axiom_syntax::Folder::default());
    assert!(parsed.is_empty(), "source parse failed: {parsed:?}");
    let (book, diagnostics) = axiom_model::build(&[Source {
        path: "owners.ax",
        file,
        embedded: false,
    }]);
    assert!(
        diagnostics.iter().all(|diagnostic| !diagnostic.is_error()),
        "book build failed: {diagnostics:?}"
    );
    let context = Context::new(
        &book,
        Options {
            today: day(2026, 1, 4),
            relaxed: false,
        },
        owner,
    )
    .unwrap();
    then(&context)
}

fn register_steps(context: &Context<'_, '_>) -> Vec<(Qty, Qty)> {
    let report = context
        .report(&Query::Register {
            place: "assets/shared",
            from: None,
            to: None,
        })
        .unwrap();
    report.sections[0]
        .rows
        .iter()
        .filter_map(|row| {
            let amount = row.cells.iter().find_map(|cell| match cell {
                Cell::Amount { qty, .. } => Some(*qty),
                _ => None,
            })?;
            let balance = row.cells.iter().rev().find_map(|cell| match cell {
                Cell::Amount { qty, .. } => Some(*qty),
                _ => None,
            })?;
            Some((amount, balance))
        })
        .collect()
}

#[test]
fn shared_register_rounding_conserves_cents_across_postings() {
    let me = with_context(Some("me"), register_steps);
    let jordan = with_context(Some("jordan"), register_steps);
    let everyone = with_context(None, register_steps);

    // Cumulative allocation assigns the first cent to me and the second to
    // Jordan. The balances and line amounts both reconcile to the physical
    // two-cent balance instead of rounding each posting independently.
    assert_eq!(me, [(Qty(1), Qty(1)), (Qty(0), Qty(1))]);
    assert_eq!(jordan, [(Qty(0), Qty(0)), (Qty(1), Qty(1))]);
    assert_eq!(everyone, [(Qty(1), Qty(1)), (Qty(1), Qty(2))]);
}
