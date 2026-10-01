use axiom_core::FileId;
use axiom_model::{Source, build};
use axiom_syntax::{Folder, parse};

#[test]
fn journal_codes_are_interned_when_records_are_lowered() {
    let path = "journal/2026/01.ax";
    let text = "\
base USD
commodity USD
account assets/checking
2026-01-01 checking -> ? 1 USD ^wire-code
2026-01-02 ^event-code settled
";
    let (file, syntax) = parse(FileId(0), text, Folder::of(path));
    assert!(syntax.is_empty(), "{syntax:?}");

    let (book, diagnostics) = build(&[Source {
        path,
        file,
        embedded: false,
    }]);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert!(
        book.codes
            .iter()
            .any(|(_, code)| book.name(*code) == "wire-code")
    );
    assert_eq!(book.name(book.events[0].code), "event-code");
}

#[test]
fn malformed_journal_text_returns_diagnostics_without_panicking() {
    let path = "journal/2026/01.ax";
    let text = "\
base USD
commodity USD
account assets/checking
2026-01-01 checking -> ? 1 USD ^still-lowerable ^
";
    let (file, syntax) = parse(FileId(0), text, Folder::of(path));
    assert!(
        !syntax.is_empty(),
        "the malformed trailing code needs a syntax diagnostic"
    );
    let (book, _diagnostics) = build(&[Source {
        path,
        file,
        embedded: false,
    }]);
    assert!(book.txns.len() <= 1);
}

#[test]
fn computed_contract_terms_use_the_native_amount_grammar() {
    let text = "\
contract c with p
  12 USD monthly from checking
  buy VTI for 3/4 of ^base monthly from checking
  + 5% of ^base
  due 5d else + 2% of ^base
2026-01-01 c now 10 USD monthly from checking
  + 3% of ^base
";
    let (file, syntax) = parse(FileId(0), text, Folder::of("contracts.ax"));
    assert!(syntax.is_empty(), "{syntax:?}");
    assert_eq!(file.items.len(), 2);
}
