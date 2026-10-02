//! The words diagnostics share: a word as written, the ways to say it, and the near miss that fixes it.

use axiom_core::diag::closest;
use axiom_core::{Diagnostic, Loc};
use axiom_syntax::File;

/// A word as written, and where.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Word<'s> {
    pub text: &'s str,
    pub loc: Loc,
}

impl<'s> Word<'s> {
    /// `text`, a slice of `file`'s source, with where it was written.
    pub fn of(file: &File<'s>, text: &'s str) -> Word<'s> {
        Word { text, loc: file.loc(text) }
    }
}

/// `did you mean X?`, as an edit at `loc`, when `candidates` holds a near miss of `word`.
pub(crate) fn suggest<'a>(
    diagnostic: Diagnostic,
    loc: Loc,
    word: &str,
    candidates: impl IntoIterator<Item = &'a str>,
) -> Diagnostic {
    match closest(word, candidates) {
        Some(near) => diagnostic.fix(format!("did you mean `{near}`?"), loc, near),
        None => diagnostic,
    }
}

/// One of the things an ambiguous name could mean.
pub(crate) struct Candidate {
    /// What it is, ready to read in a sentence: `assets/bank/checking` in
    /// backticks, or "the project's `bank`".
    pub is: String,
    pub declared: Option<Loc>,
    /// A written form that means only this one, if there is one.
    pub write: Option<String>,
}

/// `a` or `an`, followed by the word.
pub(crate) fn article(word: &str) -> String {
    let vowel = word.starts_with(['a', 'e', 'i', 'o']);
    format!("{} {word}", if vowel { "an" } else { "a" })
}

/// `a, b or c`, each in backticks.
pub(crate) fn list(words: &[&str]) -> String {
    match words {
        [] => String::new(),
        [only] => format!("`{only}`"),
        [init @ .., last] => {
            let init: Vec<String> = init.iter().map(|word| format!("`{word}`")).collect();
            format!("{} or `{last}`", init.join(", "))
        }
    }
}

/// `a, b and c`, each in backticks.
pub(crate) fn list_and(words: &[&str]) -> String {
    match words {
        [init @ .., last] if !init.is_empty() => {
            let init: Vec<String> = init.iter().map(|word| format!("`{word}`")).collect();
            format!("{} and `{last}`", init.join(", "))
        }
        _ => list(words),
    }
}

/// One count in words: `1 line`, `3 lines`.
pub(crate) fn count(n: usize, noun: &str) -> String {
    format!("{n} {noun}{}", if n == 1 { "" } else { "s" })
}
