//! Runtime for the model's compiled pattern programs (LANGUAGE §14).
//!
//! The syntax compiler owns parsing and lowering. This module only indexes and
//! executes borrowed `model::sync::Pattern` values; it has no second pattern
//! language or semantic representation.

use axiom_core::{Arena, Id, Interner};
use axiom_model::sync::{Capture, CharClass, Op, Pattern};
use memchr::memmem;

const MAX_DEPTH: usize = 64;

/// A compiled program set borrows the book's canonical pattern arena. The
/// start-literal lists are only a lookup index; the Op programs remain in the
/// model arena and are never copied.
pub struct Patterns<'a, 's> {
    arena: &'a Arena<Pattern>,
    names: &'a Interner<'s>,
    starts: Vec<Option<Vec<Vec<u8>>>>,
}

impl<'a, 's> Patterns<'a, 's> {
    pub fn new(arena: &'a Arena<Pattern>, names: &'a Interner<'s>) -> Patterns<'a, 's> {
        let mut starts = vec![None; arena.len()];
        let mut visiting = vec![false; arena.len()];
        for (id, _) in arena.iter() {
            starts[id.index()] = starts_of(arena, names, id, &mut visiting);
        }
        Patterns {
            arena,
            names,
            starts,
        }
    }

    pub fn starts(&self, id: Id<Pattern>) -> Option<&[Vec<u8>]> {
        self.starts.get(id.index())?.as_deref()
    }

    fn pattern(&self, id: Id<Pattern>) -> Option<&Pattern> {
        self.arena.get(id)
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
        self.literal = 0;
        self.captures.clear();
        let pattern = patterns.pattern(id)?;
        let end = body(&pattern.program, hay, at, self, patterns, 0)?;
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
        let prefix = match patterns.starts(id) {
            Some([only]) => only.as_slice(),
            _ => &[],
        };
        let mut at = from;
        loop {
            if !prefix.is_empty() {
                at += memmem::find(hay.get(at..)?, prefix)?;
            }
            if let Some(found) = self.matches_at(id, hay, at, patterns) {
                return Some(found);
            }
            at += 1;
            if at > hay.len() {
                return None;
            }
        }
    }
}

/// The fixed-start literals for a pattern, or `None` if it may start anywhere.
fn starts_of(
    arena: &Arena<Pattern>,
    names: &Interner<'_>,
    id: Id<Pattern>,
    visiting: &mut [bool],
) -> Option<Vec<Vec<u8>>> {
    let index = id.index();
    if *visiting.get(index)? {
        return None;
    }
    *visiting.get_mut(index)? = true;
    let result = arena
        .get(id)
        .and_then(|pattern| starts_in(&pattern.program, arena, names, visiting));
    visiting[index] = false;
    result
}

fn starts_in(
    program: &[Op],
    arena: &Arena<Pattern>,
    names: &Interner<'_>,
    visiting: &mut [bool],
) -> Option<Vec<Vec<u8>>> {
    let mut ops = program;
    while let [Op::Class(CharClass::Start), rest @ ..] = ops {
        ops = rest;
    }
    let (first, rest) = ops.split_first()?;
    match first {
        Op::Literal(sym) => {
            let bytes = names
                .name(*sym)
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
            let name = names.name(*sym);
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
                all.extend(starts_in(one, arena, names, visiting)?);
                match others {
                    [Op::Choice { len }, next @ ..] => (way, rest) = (usize::from(*len), next),
                    last => {
                        all.extend(starts_in(last, arena, names, visiting)?);
                        return Some(all);
                    }
                }
            }
        }
        Op::Capture { len, .. } => {
            starts_in(rest.get(..usize::from(*len))?, arena, names, visiting)
        }
        Op::Repeat { min, len, .. } if *min > 0 => {
            starts_in(rest.get(..usize::from(*len))?, arena, names, visiting)
        }
        Op::Call(callee) => starts_of(arena, names, *callee, visiting),
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
                let literal = patterns.names.name(*sym).as_bytes();
                let end = pos.checked_add(literal.len())?;
                if !hay.get(pos..end)?.eq_ignore_ascii_case(literal) {
                    return None;
                }
                run.literal += literal.len();
                pos = end;
            }
            Op::Name(sym) => {
                let name = patterns.names.name(*sym).as_bytes();
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
        CharClass::Digit => one(u8::is_ascii_digit),
        CharClass::Letter => one(|byte| byte.is_ascii_alphabetic() || byte >= 0x80),
        CharClass::Alnum => one(|byte| byte.is_ascii_alphanumeric() || byte >= 0x80),
        CharClass::Space => one(u8::is_ascii_whitespace),
        CharClass::Any => hay
            .get(at)
            .map(|&lead| at + char_width(lead))
            .filter(|&end| end <= hay.len()),
        CharClass::Rest => Some(hay.len()),
        CharClass::Start => (at == 0).then_some(at),
        CharClass::End => (at == hay.len()).then_some(at),
    }
}

fn char_width(lead: u8) -> usize {
    match lead {
        0xF0.. => 4,
        0xE0.. => 3,
        0xC0.. => 2,
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_core::Loc;

    fn add(arena: &mut Arena<Pattern>, program: Vec<Op>) -> Id<Pattern> {
        arena.push(Pattern {
            name: None,
            program: program.into_boxed_slice(),
            loc: Loc::default(),
        })
    }

    #[test]
    fn executes_borrowed_programs_with_choice_repeat_call_and_original_capture() {
        let mut names = Interner::default();
        let ach = names.intern("ACH ");
        let point = names.intern(".");
        let mut arena = Arena::new();
        let called = add(&mut arena, vec![Op::Literal(ach)]);
        let amount = add(
            &mut arena,
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
        let patterns = Patterns::new(&arena, &names);
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
        let mut names = Interner::default();
        let a = names.intern("A");
        let b = names.intern("B");
        let mut arena = Arena::new();
        let id = add(
            &mut arena,
            vec![Op::Choice { len: 1 }, Op::Literal(a), Op::Literal(b)],
        );
        let patterns = Patterns::new(&arena, &names);
        let mut run = Run::default();
        assert_eq!(run.find(id, b"B", 0, &patterns).unwrap().end, 1);
        assert_eq!(run.find(id, b"C", 0, &patterns), None);
    }
}

#[cfg(test)]
#[path = "peg/tests.rs"]
mod name_tests;
