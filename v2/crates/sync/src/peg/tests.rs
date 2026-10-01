use super::*;

fn pattern(program: Vec<Op>) -> (Arena<Pattern>, Id<Pattern>) {
    let mut arena = Arena::default();
    let id = arena.push(Pattern {
        name: None,
        program: program.into_boxed_slice(),
        loc: axiom_core::Loc::default(),
    });
    (arena, id)
}

#[test]
fn generated_names_match_hyphens_and_paths_as_spaces_without_allocating_per_match() {
    let mut names = Interner::default();
    let sym = names.intern("trader-joes/market");
    let (arena, id) = pattern(vec![Op::Name(sym)]);
    let patterns = Patterns::new(&arena, &names);
    assert_eq!(patterns.starts(id), Some(&[b"trader".to_vec()][..]));

    let mut run = Run::default();
    let found = run.matches_at(id, b"TRADER JOES MARKET", 0, &patterns);
    assert_eq!(found.map(|found| (found.start, found.end)), Some((0, 18)));
    assert!(
        run.matches_at(id, b"TRADER-JOES MARKET", 0, &patterns)
            .is_some()
    );
    assert!(
        run.matches_at(id, b"TRADER/JOES/MARKET", 0, &patterns)
            .is_some()
    );
}

#[test]
fn declared_literals_keep_exact_punctuation_while_ignoring_case() {
    let mut names = Interner::default();
    let sym = names.intern("trader-joes");
    let (arena, id) = pattern(vec![Op::Literal(sym)]);
    let patterns = Patterns::new(&arena, &names);
    let mut run = Run::default();
    assert!(run.matches_at(id, b"TRADER-JOES", 0, &patterns).is_some());
    assert!(run.matches_at(id, b"TRADER JOES", 0, &patterns).is_none());
}
