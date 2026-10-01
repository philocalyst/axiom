use super::*;
use axiom_core::FileId;
use axiom_syntax::Folder;

fn book() -> Book<'static> {
    let (file, diagnostics) =
        axiom_syntax::parse(FileId(0), "base USD\n", Folder::default());
    assert!(diagnostics.is_empty(), "axiom.ax: {diagnostics:?}");
    let sources = [axiom_model::Source {
        path: "axiom.ax",
        file,
        embedded: false,
    }];
    let (book, diagnostics) = axiom_model::build(&sources);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    book
}

#[test]
fn generated_names_match_hyphens_and_paths_as_spaces_without_allocating_per_match() {
    let mut book = book();
    let sym = book.names.intern("trader-joes/market");
    let id = book.patterns.push(Pattern {
        name: None,
        program: Box::new([Op::Name(sym)]),
        loc: axiom_core::Loc::default(),
    });
    let patterns = Patterns::new(&book);
    assert_eq!(patterns.starts(id), Some(&[b"trader".to_vec()][..]));

    let mut run = Run::default();
    let found = run.matches_at(id, b"TRADER JOES MARKET", 0, &patterns);
    assert_eq!(found.map(|found| (found.start, found.end)), Some((0, 18)));
    assert!(run.matches_at(id, b"TRADER-JOES MARKET", 0, &patterns).is_some());
    assert!(run.matches_at(id, b"TRADER/JOES/MARKET", 0, &patterns).is_some());
}

#[test]
fn declared_literals_keep_exact_punctuation_while_ignoring_case() {
    let mut book = book();
    let text = book.intern_text("trader-joes");
    let id = book.patterns.push(Pattern {
        name: None,
        program: Box::new([Op::Literal(text)]),
        loc: axiom_core::Loc::default(),
    });
    let patterns = Patterns::new(&book);
    let mut run = Run::default();
    assert!(run.matches_at(id, b"TRADER-JOES", 0, &patterns).is_some());
    assert!(run.matches_at(id, b"TRADER JOES", 0, &patterns).is_none());
}

#[test]
fn unicode_repeat_decodes_one_scalar_at_a_time_and_never_starts_mid_character() {
    let mut book = book();
    let any = book.patterns.push(Pattern {
        name: None,
        program: Box::new([Op::Class(CharClass::Any)]),
        loc: axiom_core::Loc::default(),
    });
    let repeated = book.patterns.push(Pattern {
        name: None,
        program: Box::new([
            Op::Repeat {
                min: 1,
                max: None,
                len: 1,
            },
            Op::Class(CharClass::Any),
        ]),
        loc: axiom_core::Loc::default(),
    });
    let literal = book.intern_text("é");
    let one_scalar = book.patterns.push(Pattern {
        name: None,
        program: Box::new([Op::Literal(literal)]),
        loc: axiom_core::Loc::default(),
    });
    let patterns = Patterns::new(&book);
    let mut run = Run::default();
    let memo = "é".repeat(25_000);
    assert_eq!(run.find(repeated, memo.as_bytes(), 0, &patterns).unwrap().end, memo.len());
    assert!(run.matches_at(one_scalar, memo.as_bytes(), 1, &patterns).is_none());
    assert_eq!(run.matches_at(any, memo.as_bytes(), 0, &patterns).unwrap().end, "é".len());
}
