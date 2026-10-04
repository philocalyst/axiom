//! Tests of `axiom fmt --upgrade`: what each shape of v4 line is written as, what is refused, and that a second
//! upgrade changes nothing.

use axiom_core::FileId;

use crate::upgrade::{Registry, Standing, Upgraded, upgrade};
use crate::{Folder, parse};

/// A book of a few names: what it holds, what its owners hold, and what it counts in.
struct Names;

impl Registry for Names {
    fn standing(&self, name: &str) -> Standing {
        match name {
            "checking" | "savings" | "broker" | "visa" | "me" | "retirement" | "joint" | "sam-401k" => Standing::Own,
            "acme" | "shop" | "irs" | "buyer" | "lumen" | "?" | "VTI" => Standing::Outside,
            _ => Standing::Unknown,
        }
    }

    fn owner(&self, name: &str) -> Option<&str> {
        match name {
            "sam-401k" => Some("sam"),
            "joint" => Some("family"),
            _ => None,
        }
    }

    fn keeper(&self) -> &str {
        "me"
    }

    fn base(&self) -> &str {
        "USD"
    }

    fn scale(&self, unit: &str) -> Option<u8> {
        Some(if unit == "USD" || unit == "EUR" { 2 } else { 4 })
    }
}

fn upgraded(src: &str) -> Upgraded {
    let file = parse(FileId(0), src, Folder::default()).0;
    upgrade(src, &file, Folder::default(), &Names)
}

/// What `src` is written as, which must be clean and stable: nothing is refused, and a second upgrade changes nothing.
fn written(src: &str) -> String {
    let once = upgraded(src);
    assert!(
        once.refused.is_empty(),
        "refused {src:?}: {:?}",
        once.refused.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
    let twice = upgraded(&once.text);
    assert_eq!(twice.text, once.text, "a second upgrade changes the text");
    once.text
}

#[test]
fn a_payment_that_arrives_from_a_party_is_written_from_the_books_own_side() {
    assert_eq!(
        written("2026-01-15 acme -> checking 3_200 USD #wages\n"),
        "2026-01-15 checking <- acme 3_200 USD #wages\n"
    );
    assert_eq!(written("2026-01-18 checking -> shop 84.20 USD\n"), "2026-01-18 checking -> shop 84.20 USD\n");
    assert_eq!(written("2026-01-19 checking -> savings 40 USD\n"), "2026-01-19 checking -> savings 40 USD\n");
    // The amount that stood before the arrow stands after the end it takes from.
    assert_eq!(
        written("2025-02-12 shop 3_200 USD -> checking #design ^inv-1\n"),
        "2025-02-12 checking <- shop 3_200 USD #design ^inv-1\n"
    );
    // A commodity is a party: a fund pays.
    assert_eq!(written("2026-02-03 VTI -> broker 198.12 USD\n"), "2026-02-03 broker <- VTI 198.12 USD\n");
}

#[test]
fn a_paystub_names_the_owner_it_passes_through_and_each_leg_its_arrow() {
    let v4 = "2026-01-15 acme -> 8_000 USD // gross\n  retirement 800 USD\n  irs 880 USD\n  checking ...\n";
    let v5 = "2026-01-15 me <- acme 8_000 USD  // gross\n  -> retirement 800 USD\n  -> irs        880 USD\n  -> checking   ...\n";
    assert_eq!(written(v4), v5);
    // The owner is the one the legs' own accounts have.
    let family = "2026-01-15 acme -> 8_000 USD\n  joint 800 USD\n  irs ...\n";
    assert_eq!(written(family), "2026-01-15 family <- acme 8_000 USD\n  -> joint 800 USD\n  -> irs   ...\n");
}

#[test]
fn a_split_from_an_own_end_and_one_into_it_get_arrows_on_their_legs() {
    let v4 = "2026-03-07 visa -> 85.05 USD\n  shop 28.35 USD\n  buyer ...\n";
    assert_eq!(written(v4), "2026-03-07 visa -> 85.05 USD\n  -> shop  28.35 USD\n  -> buyer ...\n");
    let dangling = "2025-08-08 checking 900.00 EUR ->\n  shop 4.77 EUR\n  savings ...\n";
    assert_eq!(written(dangling), "2025-08-08 checking -> 900.00 EUR\n  -> shop    4.77 EUR\n  -> savings ...\n");
    let into = "2026-03-07 -> checking 100 USD\n  shop 30 USD\n  acme ...\n";
    assert_eq!(written(into), "2026-03-07 checking <- 100 USD\n  <- shop 30 USD\n  <- acme ...\n");
    let party = "2026-02-14 -> shop 45_046.25 USD #purchase\n  checking 5_000 USD\n  savings ...\n";
    assert_eq!(
        written(party),
        "2026-02-14 me -> shop 45_046.25 USD #purchase\n  <- checking 5_000 USD\n  <- savings  ...\n"
    );
}

#[test]
fn an_amount_written_twice_is_written_once_with_its_price() {
    let v4 = "2024-09-05 checking 1_499.99 USD -> broker 5.5851 VTI @ 268.57 USD\n";
    assert_eq!(written(v4), "2024-09-05 checking -> broker 5.5851 VTI @ 268.57 USD\n");
    // A price that is not what the amounts say is left for the model to refuse.
    let wrong = "2024-09-05 checking 1_500.00 USD -> broker 5.5851 VTI @ 268.57 USD\n";
    assert_eq!(written(wrong), "2024-09-05 checking 1_500.00 USD -> broker 5.5851 VTI @ 268.57 USD\n");
    // No price: the one that is exact is written, and the amount in the other unit is not.
    assert_eq!(
        written("2025-09-02 broker 60 FAST -> checking 2_748.60 USD\n"),
        "2025-09-02 broker 60 FAST -> checking @ 45.81 USD\n"
    );
    assert_eq!(
        written("2025-09-05 checking 9_900.00 USD -> broker 99 NWND\n"),
        "2025-09-05 checking -> broker 99 NWND @ 100.00 USD\n"
    );
}

#[test]
fn a_price_nobody_wrote_is_refused_with_the_one_that_would_do() {
    let found = upgraded("2024-11-15 broker 13.2000 NWND -> irs 1_788.73 USD\n");
    assert_eq!(found.refused.len(), 1);
    assert_eq!(found.refused[0].code, "upgrade-price");
    let (_, fixed) = found.refused[0].help.iter().find_map(|help| help.edit.clone()).expect("the line with the price");
    assert_eq!(fixed, "2024-11-15 broker 13.2000 NWND -> irs @ 135.51 USD");
    assert_eq!(found.text, "2024-11-15 broker 13.2000 NWND -> irs 1_788.73 USD\n", "the line stays as it was");
}

#[test]
fn a_line_the_book_rejects_is_left_as_it_was() {
    // An exchange that names one end is an error in v4 and a sale in v5: reading it would change the book.
    let text = "2026-02-05 broker[2026-01-20] 1.620 VTI -> 481.14 USD\n";
    assert_eq!(written(text), text);
    assert_eq!(
        written("2026-02-05 broker 2_000.00 USD -> 2_000 EUR\n"),
        "2026-02-05 broker 2_000.00 USD -> 2_000 EUR\n"
    );
}

#[test]
fn a_line_it_cannot_place_is_refused_and_not_guessed() {
    let found = upgraded("2026-01-15 acme -> irs 40 USD\n");
    assert_eq!(found.refused[0].code, "upgrade-sides");
    assert_eq!(found.text, "2026-01-15 acme -> irs 40 USD\n");
    let found = upgraded("2026-01-15 acme -> 8_000 USD\n  joint 800 USD\n  sam-401k 800 USD\n");
    assert_eq!(found.refused[0].code, "upgrade-owner");
}

#[test]
fn legs_of_an_occurrence_and_of_a_contract_get_arrows_and_a_return_keeps_its_tallies() {
    let v4 = "contract job with acme\n  8_000 USD monthly on 13 into checking\n  irs 880 USD\n  checking ...\n\n2026-02-13 job\n  irs 900 USD\n\n2026-04-01 me filed 2025\n  wages 124_200.00 USD\n";
    let v5 = "contract job with acme\n  8_000 USD monthly on 13 into checking\n  -> irs 880 USD\n  -> checking ...\n\n2026-02-13 job\n  -> irs 900 USD\n\n2026-04-01 me filed 2025\n  wages 124_200.00 USD\n";
    assert_eq!(upgraded(v4).text, v5);
}

#[test]
fn comments_and_blank_lines_are_kept() {
    let v4 = "// January\n\n2026-01-15 acme -> checking 3_200 USD // pay\n\n/// Rent.\n2026-01-16 checking -> shop 900 USD\n";
    let found = written(v4);
    assert!(found.contains("// January\n\n") && found.contains("// pay") && found.contains("/// Rent.\n"));
}
