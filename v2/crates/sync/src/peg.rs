//! Runtime for the model's compiled pattern programs (LANGUAGE §14).
//!
//! The syntax compiler owns parsing and lowering. This module only indexes and
//! executes borrowed `model::sync::Pattern` values; it has no second pattern
//! language or semantic representation.

use axiom_core::Id;
use axiom_model::sync::{Capture, CharClass, Op, Pattern};
use axiom_model::Book;
use memchr::memmem;

const MAX_DEPTH: usize = 64;

/// A compiled program set borrows the book's canonical pattern arena. The
/// start-literal lists are only a lookup index; the Op programs remain in the
/// model arena and are never copied.
pub struct Patterns<'a, 's> {
    book: &'a Book<'s>,
    starts: Vec<Option<Vec<Vec<u8>>>>,
}

impl<'a, 's> Patterns<'a, 's> {
    pub fn new(book: &'a Book<'s>) -> Patterns<'a, 's> {
        let mut starts = vec![None; book.patterns.len()];
        let mut visiting = vec![false; book.patterns.len()];
        for (id, _) in book.patterns.iter() {
            starts[id.index()] = starts_of(book, id, &mut visiting, 0);
        }
        Patterns { book, starts }
    }

    pub fn starts(&self, id: Id<Pattern>) -> Option<&[Vec<u8>]> {
        self.starts.get(id.index())?.as_deref()
    }

    fn pattern(&self, id: Id<Pattern>) -> Option<&Pattern> {
        self.book.patterns.get(id)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Found {
    pub start: usize,
    pub end: usize,
    /// Number of literal bytes matched; used to choose the most specific name.
    pub literal: usize,
}

/// Scratch for one execution, reused between candidate matches.
#[derive(Default)]
pub struct Run {
    literal: usize,
    captures: Vec<(Capture, usize, usize)>,
}

impl Run {
    pub fn capture(&self, name: Capture) -> Option<(usize, usize)> {
        self.captures
            .iter()
            .rev()
            .find(|(found, _, _)| *found == name)
            .map(|(_, start, end)| (*start, *end))
    }

    pub fn matches_at(
        &mut self,
        id: Id<Pattern>,
        hay: &[u8],
        at: usize,
        patterns: &Patterns<'_, '_>,
    ) -> Option<Found> {
        std::str::from_utf8(hay).ok()?;
        self.matches_at_valid(id, hay, at, patterns)
    }

    /// Execute against UTF-8 bytes already validated by the caller. This is
    /// used while scanning every scalar boundary of a memo, so validation is
    /// done once rather than rescanning the remaining suffix at every step.
    pub(crate) fn matches_at_valid(
        &mut self,
        id: Id<Pattern>,
        hay: &[u8],
        at: usize,
        patterns: &Patterns<'_, '_>,
    ) -> Option<Found> {
        if !boundary(hay, at) {
            return None;
        }
        self.literal = 0;
        self.captures.clear();
        let pattern = patterns.pattern(id)?;
        let end = body(&pattern.program, hay, at, self, patterns, 0)?;
        if !boundary(hay, end) {
            self.literal = 0;
            self.captures.clear();
            return None;
        }
        Some(Found {
            start: at,
            end,
            literal: self.literal,
        })
    }

    pub fn find(
        &mut self,
        id: Id<Pattern>,
        hay: &[u8],
        from: usize,
        patterns: &Patterns<'_, '_>,
    ) -> Option<Found> {
        let text = std::str::from_utf8(hay).ok()?;
        let prefix = match patterns.starts(id) {
            Some([only]) => only.as_slice(),
            _ => &[],
        };
        let mut at = from.min(hay.len());
        if !text.is_char_boundary(at) {
            at = text.char_indices().find_map(|(at, _)| (at > from).then_some(at))?;
        }
        loop {
            if !prefix.is_empty() {
                at += memmem::find(hay.get(at..)?, prefix)?;
            }
            if let Some(found) = self.matches_at_valid(id, hay, at, patterns) {
                return Some(found);
            }
            let character = text.get(at..)?.chars().next()?;
            at += character.len_utf8();
            if at > hay.len() {
                return None;
            }
        }
    }
}

fn boundary(hay: &[u8], at: usize) -> bool {
    at <= hay.len()
        && (at == 0
            || at == hay.len()
            || hay.get(at).is_some_and(|byte| byte & 0b1100_0000 != 0b1000_0000))
}

/// The fixed-start literals for a pattern, or `None` if it may start anywhere.
fn starts_of(
    book: &Book<'_>,
    id: Id<Pattern>,
    visiting: &mut [bool],
    depth: usize,
) -> Option<Vec<Vec<u8>>> {
    if depth > MAX_DEPTH {
        return None;
    }
    let index = id.index();
    if *visiting.get(index)? {
        return None;
    }
    *visiting.get_mut(index)? = true;
    let result = book
        .patterns
        .get(id)
        .and_then(|pattern| starts_in(&pattern.program, book, visiting, depth));
    visiting[index] = false;
    result
}

fn starts_in(
    program: &[Op],
    book: &Book<'_>,
    visiting: &mut [bool],
    depth: usize,
) -> Option<Vec<Vec<u8>>> {
    if depth > MAX_DEPTH {
        return None;
    }
    let mut ops = program;
    while let [Op::Class(CharClass::Start), rest @ ..] = ops {
        ops = rest;
    }
    let (first, rest) = ops.split_first()?;
    match first {
        Op::Literal(sym) => {
            let bytes = book
                .text(*sym)
                .as_bytes()
                .iter()
                .map(u8::to_ascii_lowercase)
                .collect::<Vec<_>>();
            (!bytes.is_empty()).then(|| vec![bytes])
        }
        Op::Name(sym) => {
            // The separators may occur as spaces, hyphens or slashes in the
            // memo. Index only the first literal component; the matcher checks
            // the full generated name without allocating.
            let name = book.name(*sym);
            let first = name.split(['-', '/']).next().unwrap_or(name);
            let bytes: Vec<u8> = first
                .bytes()
                .map(|byte| byte.to_ascii_lowercase())
                .collect();
            (!bytes.is_empty()).then(|| vec![bytes])
        }
        Op::Choice { len } => {
            let (mut way, mut rest, mut all) = (usize::from(*len), rest, Vec::new());
            loop {
                let (one, others) = rest.split_at_checked(way)?;
                all.extend(starts_in(one, book, visiting, depth + 1)?);
                match others {
                    [Op::Choice { len }, next @ ..] => (way, rest) = (usize::from(*len), next),
                    last => {
                        all.extend(starts_in(last, book, visiting, depth + 1)?);
                        return Some(all);
                    }
                }
            }
        }
        Op::Capture { len, .. } => starts_in(
            rest.get(..usize::from(*len))?,
            book,
            visiting,
            depth + 1,
        ),
        Op::Repeat { min, len, .. } if *min > 0 => starts_in(
            rest.get(..usize::from(*len))?,
            book,
            visiting,
            depth + 1,
        ),
        Op::Call(callee) => starts_of(book, *callee, visiting, depth + 1),
        Op::Class(_) | Op::Repeat { .. } => None,
    }
}

/// A body either succeeds wholly or leaves the scratch state as it found it.
fn body(
    ops: &[Op],
    hay: &[u8],
    pos: usize,
    run: &mut Run,
    patterns: &Patterns<'_, '_>,
    depth: usize,
) -> Option<usize> {
    if depth > MAX_DEPTH {
        return None;
    }
    let (literal, captured) = (run.literal, run.captures.len());
    let ended = sequence(ops, hay, pos, run, patterns, depth);
    if ended.is_none() {
        run.literal = literal;
        run.captures.truncate(captured);
    }
    ended
}

fn sequence(
    ops: &[Op],
    hay: &[u8],
    mut pos: usize,
    run: &mut Run,
    patterns: &Patterns<'_, '_>,
    depth: usize,
) -> Option<usize> {
    let mut at = 0;
    while let Some(op) = ops.get(at) {
        at += 1;
        match op {
            Op::Literal(sym) => {
                let literal = patterns.book.text(*sym).as_bytes();
                let end = pos.checked_add(literal.len())?;
                if !hay.get(pos..end)?.eq_ignore_ascii_case(literal) {
                    return None;
                }
                run.literal += literal.len();
                pos = end;
            }
            Op::Name(sym) => {
                let name = patterns.book.name(*sym).as_bytes();
                let end = match_name(name, hay, pos)?;
                run.literal += end - pos;
                pos = end;
            }
            Op::Class(class) => pos = step(*class, hay, pos)?,
            Op::Choice { len } => {
                // The ways are a chain to the end of this body.
                let (mut way, mut rest) = (usize::from(*len), ops.get(at..)?);
                loop {
                    let (one, others) = rest.split_at_checked(way)?;
                    if let Some(end) = body(one, hay, pos, run, patterns, depth + 1) {
                        return Some(end);
                    }
                    match others {
                        [Op::Choice { len }, next @ ..] => (way, rest) = (usize::from(*len), next),
                        last => return body(last, hay, pos, run, patterns, depth + 1),
                    }
                }
            }
            Op::Repeat { min, max, len } => {
                let inner = ops.get(at..at + usize::from(*len))?;
                at += usize::from(*len);
                let (mut end, mut count) = (pos, 0u32);
                while max.is_none_or(|max| count < u32::from(max)) {
                    let Some(next) = body(inner, hay, end, run, patterns, depth + 1) else {
                        break;
                    };
                    count += 1;
                    if next == end {
                        count = count.max(u32::from(*min));
                        break;
                    }
                    end = next;
                }
                (count >= u32::from(*min)).then_some(())?;
                pos = end;
            }
            Op::Capture { name, len } => {
                let inner = ops.get(at..at + usize::from(*len))?;
                at += usize::from(*len);
                let end = body(inner, hay, pos, run, patterns, depth + 1)?;
                run.captures.push((*name, pos, end));
                pos = end;
            }
            Op::Call(callee) => {
                let called = patterns.pattern(*callee)?;
                pos = body(&called.program, hay, pos, run, patterns, depth + 1)?;
            }
        }
    }
    Some(pos)
}

/// Match a built-in entity/account name without constructing its normalized
/// spelling. Hyphens and path separators in the declaration each mean one
/// space in a memo; ordinary literal patterns retain exact punctuation.
fn match_name(name: &[u8], hay: &[u8], at: usize) -> Option<usize> {
    let mut pos = at;
    for &expected in name {
        let actual = *hay.get(pos)?;
        let matches = match expected {
            b'-' | b'/' => matches!(actual, b' ' | b'-' | b'/'),
            other => actual.eq_ignore_ascii_case(&other),
        };
        if !matches {
            return None;
        }
        pos += 1;
    }
    Some(pos)
}

fn step(class: CharClass, hay: &[u8], at: usize) -> Option<usize> {
    let one = |fits: fn(u8) -> bool| {
        hay.get(at)
            .copied()
            .filter(|&byte| fits(byte))
            .map(|_| at + 1)
    };
    match class {
        CharClass::Digit => one(|byte| byte.is_ascii_digit()),
        CharClass::Letter | CharClass::Alnum => {
            let character = scalar_at(hay, at)?;
            let matches = match class {
                CharClass::Letter => character.is_alphabetic(),
                CharClass::Alnum => character.is_alphanumeric(),
                _ => unreachable!(),
            };
            matches.then_some(at + character.len_utf8())
        }
        CharClass::Space => one(|byte| byte.is_ascii_whitespace()),
        CharClass::Any => {
            let character = scalar_at(hay, at)?;
            Some(at + character.len_utf8())
        }
        CharClass::Rest => Some(hay.len()),
        CharClass::Start => (at == 0).then_some(at),
        CharClass::End => (at == hay.len()).then_some(at),
    }
}

/// Decode exactly one scalar, validating at most four bytes. The matcher can
/// apply `any*` to a long Unicode memo without validating every remaining
/// suffix repeatedly.
fn scalar_at(hay: &[u8], at: usize) -> Option<char> {
    let width = match *hay.get(at)? {
        0x00..=0x7f => 1,
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        _ => return None,
    };
    let scalar = hay.get(at..at.checked_add(width)?)?;
    std::str::from_utf8(scalar).ok()?.chars().next()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_core::{FileId, Folder, Loc};

    fn book() -> Book<'static> {
        let std = include_str!("../../systems/src/std.ax");
        let sources = [("std.ax", std, true), ("axiom.ax", "base USD\n", false)].map(
            |(path, text, embedded)| {
                let (file, diagnostics) =
                    axiom_syntax::parse(FileId(0), text, Folder::default());
                assert!(diagnostics.is_empty(), "{path}: {diagnostics:?}");
                axiom_model::Source { path, file, embedded }
            },
        );
        let (book, diagnostics) = axiom_model::build(&sources);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        book
    }

    fn add(book: &mut Book<'static>, program: Vec<Op>) -> Id<Pattern> {
        book.patterns.push(Pattern {
            name: None,
            program: program.into_boxed_slice(),
            loc: Loc::default(),
        })
    }

    #[test]
    fn executes_borrowed_programs_with_choice_repeat_call_and_original_capture() {
        let mut book = book();
        let ach = book.intern_text("ACH ");
        let point = book.intern_text(".");
        let called = add(&mut book, vec![Op::Literal(ach)]);
        let amount = add(
            &mut book,
            vec![
                Op::Call(called),
                Op::Capture {
                    name: Capture::Original,
                    len: 4,
                },
                Op::Class(CharClass::Digit),
                Op::Repeat {
                    min: 0,
                    max: None,
                    len: 1,
                },
                Op::Class(CharClass::Digit),
                Op::Literal(point),
                Op::Repeat {
                    min: 1,
                    max: Some(2),
                    len: 1,
                },
                Op::Class(CharClass::Digit),
            ],
        );
        let patterns = Patterns::new(&book);
        let mut run = Run::default();
        let hay = b"memo ach 3290.00 next";
        let found = run.find(amount, hay, 0, &patterns).unwrap();
        assert_eq!(
            std::str::from_utf8(&hay[found.start..found.end]).unwrap(),
            "ach 3290.00"
        );
        let (start, end) = run.capture(Capture::Original).unwrap();
        assert_eq!(std::str::from_utf8(&hay[start..end]).unwrap(), "3290.00");
    }

    #[test]
    fn ordered_choice_uses_the_model_op_chain() {
        let mut book = book();
        let a = book.intern_text("A");
        let b = book.intern_text("B");
        let id = add(
            &mut book,
            vec![Op::Choice { len: 1 }, Op::Literal(a), Op::Literal(b)],
        );
        let patterns = Patterns::new(&book);
        let mut run = Run::default();
        assert_eq!(run.find(id, b"B", 0, &patterns).unwrap().end, 1);
        assert_eq!(run.find(id, b"C", 0, &patterns), None);
    }

    #[test]
    fn unicode_classes_and_captures_stay_on_utf8_boundaries() {
        let mut book = book();
        let id = add(
            &mut book,
            vec![
                Op::Capture {
                    name: Capture::Code,
                    len: 1,
                },
                Op::Class(CharClass::Letter),
                Op::Class(CharClass::Alnum),
                Op::Class(CharClass::Any),
            ],
        );
        let patterns = Patterns::new(&book);
        let mut run = Run::default();
        let memo = "é2🙂";
        let found = run.find(id, memo.as_bytes(), 0, &patterns).unwrap();
        assert_eq!(found.end, memo.len());
        let (start, end) = run.capture(Capture::Code).unwrap();
        assert_eq!(&memo[start..end], "é");
        assert!(memo.is_char_boundary(start));
        assert!(memo.is_char_boundary(end));
    }
}

#[cfg(test)]
#[path = "peg/tests.rs"]
mod name_tests;
