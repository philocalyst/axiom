use super::*;
use axiom_core::{FileId, Folder};

fn book() -> Book<'static> {
    let std = include_str!("../../../systems/src/std.ax");
    let sources = [("std.ax", std, true), ("axiom.ax", "base USD\n", false)].map(
        |(path, text, embedded)| {
            let (file, diagnostics) = axiom_syntax::parse(FileId(0), text, Folder::default());
            assert!(diagnostics.is_empty(), "{path}: {diagnostics:?}");
            axiom_model::Source { path, file, embedded }
        },
    );
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
