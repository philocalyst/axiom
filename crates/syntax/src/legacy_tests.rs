//! The v4 spellings the parser still reads: each is one warning for its file, and means what its v5 spelling means.

use axiom_core::{Diagnostic, FileId};

use crate::ast::*;
use crate::{Folder, parse_in};

fn parsed(src: &str, pieces: usize) -> (File<'_>, Vec<Diagnostic>) {
    parse_in(FileId(0), src, Folder::default(), pieces)
}

/// Who gives, who takes and where each leg goes, for every flow of a file: what the model reads, whichever way it was
/// spelled.
fn ends(file: &File<'_>) -> Vec<(Option<String>, Option<String>, Vec<String>)> {
    let name = |side: &Side<'_>| side.end.map(|end| end.name.0.to_string());
    let flows = file.items.iter().filter_map(|item| match item.kind {
        ItemKind::Txn(id) => Some(&file[id].flow),
        _ => None,
    });
    flows
        .map(|flow| {
            let legs = file[flow.body.legs].iter().map(|leg| leg.end.name.0.to_string()).collect();
            (name(&flow.from), name(&flow.to), legs)
        })
        .collect()
}

/// The one warning a legacy text makes, and what the same text written the v5 way makes: nothing, and the same ends.
fn said_once(v4: &str, v5: &str) -> Diagnostic {
    let (old, mut warned) = parsed(v4, 1);
    assert_eq!(warned.len(), 1, "one warning for the file: {warned:?}");
    let (new, clean) = parsed(v5, 1);
    assert!(clean.is_empty(), "{clean:?}");
    assert_eq!(ends(&old), ends(&new), "the two spellings are one flow");
    warned.remove(0)
}

fn first_label(warning: &Diagnostic) -> &str {
    &warning.labels[0].text
}

#[test]
fn a_leg_with_no_arrow_is_old_and_goes_where_its_arrowed_spelling_does() {
    let v4 = "2026-01-15 acme -> 8_000 USD\n  irs 880 USD\n  checking ...\n";
    let v5 = "2026-01-15 acme -> 8_000 USD\n  -> irs 880 USD\n  -> checking ...\n";
    let warning = said_once(v4, v5);
    assert_eq!((&*warning.code, warning.is_error()), ("v4-syntax", false));
    assert_eq!(warning.message, "2 lines are written the v4 way");
    assert_eq!(first_label(&warning), "the first of 2 legs with no arrow");
    assert!(warning.help.iter().any(|help| help.text.contains("axiom fmt --upgrade")));
}

#[test]
fn an_amount_on_each_side_of_the_arrow_is_old_and_states_its_price_once_in_v5() {
    let v4 = "2024-09-05 checking 1_499.99 USD -> broker 5.5851 VTI @ 268.57 USD\n";
    let v5 = "2024-09-05 checking -> broker 5.5851 VTI @ 268.57 USD\n";
    let warning = said_once(v4, v5);
    assert_eq!(warning.message, "a line is written the v4 way");
    assert_eq!(first_label(&warning), "an amount on both sides of the arrow");
}

#[test]
fn an_amount_before_an_arrow_with_only_legs_after_it_is_old() {
    let v4 = "2025-08-08 checking 900.00 EUR ->\n  shop 4.77 EUR\n  savings ...\n";
    let v5 = "2025-08-08 checking -> 900.00 EUR\n  -> shop 4.77 EUR\n  -> savings ...\n";
    let warning = said_once(v4, v5);
    assert_eq!(warning.message, "3 lines are written the v4 way");
    assert_eq!(warning.labels.len(), 2, "one label for each form, at the first of it");
    assert_eq!(first_label(&warning), "an amount before an arrow with nothing after it");
}

#[test]
fn an_arrow_with_no_subject_is_old_and_is_the_end_that_takes_in_v5() {
    let v4 = "2026-03-07 -> checking 100 USD\n  shop 30 USD\n  acme ...\n";
    let v5 = "2026-03-07 checking <- 100 USD\n  <- shop 30 USD\n  <- acme ...\n";
    let warning = said_once(v4, v5);
    assert_eq!(first_label(&warning), "an arrow with no subject");
}

#[test]
fn a_file_is_told_once_however_many_pieces_it_is_read_in() {
    let book = "2026-01-15 acme -> 8_000 USD\n  irs 880 USD\n  checking ...\n\n".repeat(40);
    let (_, once) = parsed(&book, 1);
    let (_, in_pieces) = parsed(&book, 7);
    let (whole, pieces) = (&once[0], &in_pieces[0]);
    assert_eq!((once.len(), in_pieces.len()), (1, 1));
    assert_eq!(whole.message, "80 lines are written the v4 way");
    assert_eq!(pieces.message, whole.message);
    assert_eq!(pieces.labels[0].loc, whole.labels[0].loc, "at the first of them, in whichever piece");
}

#[test]
fn what_the_parser_cannot_tell_from_the_text_is_not_old() {
    // A party first, a lone amount before an arrow, an exchange that names one end, an amount that is not written out:
    // each is what v5 writes too, or has no other spelling yet.
    for src in [
        "2026-01-15 acme -> checking 3_200 USD\n",
        "2026-01-05 checking 12.5 USD -> food\n",
        "2026-02-05 fidelity[2026-01-20] 1.62 VTI -> 481.14 USD\n",
        "2026-03-14 checking 50% of 1_800 USD ->\n  -> shop 5 USD\n  -> savings 800 EUR\n",
        "2026-03-14 savings all ->\n  -> checking 60 USD\n  -> shop ...\n",
    ] {
        let (_, diags) = parsed(src, 1);
        assert!(diags.is_empty(), "{src}: {diags:?}");
    }
}
