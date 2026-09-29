//! The words diagnostics share: a name nothing answers to, a name several
//! things answer to, a thing declared twice.

use axiom_core::{Day, Diagnostic, Loc};

/// A word as written, and where.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Word<'s> {
    pub text: &'s str,
    pub loc: Loc,
}

/// `there is no place `chekcing``, with the closest known name as the fix.
pub(crate) fn unknown(code: &'static str, noun: &str, word: Word, suggestion: Option<&str>) -> Diagnostic {
    let diagnostic = Diagnostic::error(code, format!("there is no {noun} `{}`", word.text))
        .label(word.loc, format!("not a known {noun}"));
    match suggestion {
        Some(near) => diagnostic.fix(format!("did you mean `{near}`?"), word.loc, near),
        None => diagnostic,
    }
}

/// A thing that exists, declared by a system this reader has not used.
pub(crate) fn not_used(diagnostic: Diagnostic, noun: &str, name: &str, system: &str) -> Diagnostic {
    diagnostic
        .note(format!("the {noun} `{name}` is declared by system `{system}`, which is not used here"))
        .help(format!("add `use {system}` to bring it into scope"))
}

/// `entity acme` written twice. A first declaration in a system the project
/// uses is a fact of the language, not something to delete.
pub(crate) fn duplicate(noun: &str, name: Word, first: Option<Loc>, system: Option<&str>) -> Diagnostic {
    let (text, loc) = (name.text, name.loc);
    match (first, system) {
        (Some(first), Some(system)) => {
            Diagnostic::error("duplicate-declaration", format!("`{text}` is already declared by system `{system}`"))
                .label(loc, "declared again here")
                .context(first, "first declared here (built in)")
                .help(format!("delete this declaration: the {noun} already exists"))
        }
        (Some(first), None) => Diagnostic::error("duplicate-declaration", format!("{noun} `{text}` is declared twice"))
            .label(loc, "declared again here")
            .context(first, "first declared here")
            .help("keep the declaration you mean and delete the other"),
        (None, _) => Diagnostic::error("duplicate-declaration", format!("{noun} `{text}` is built in"))
            .label(loc, "declared again here")
            .help("delete this declaration"),
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

pub(crate) fn ambiguous(code: &'static str, plural: &str, word: Word, candidates: &[Candidate]) -> Diagnostic {
    let which = if candidates.len() == 2 { "either of these" } else { "any of these" };
    let mut diagnostic = Diagnostic::error(code, format!("`{}` could be {which} {plural}", word.text))
        .label(word.loc, "which one is meant?");
    for candidate in candidates {
        if let Some(loc) = candidate.declared {
            diagnostic = diagnostic.context(loc, format!("{} is declared here", candidate.is));
        }
        diagnostic = match &candidate.write {
            Some(write) => diagnostic.fix(format!("write `{write}` for {}", candidate.is), word.loc, write),
            None => diagnostic
                .note(format!("{} cannot be written any other way: rename it to tell them apart", candidate.is)),
        };
    }
    diagnostic
}

/// `a` or `an`, followed by the word.
pub(crate) fn article(word: &str) -> String {
    let vowel = word.starts_with(['a', 'e', 'i', 'o']);
    format!("{} {word}", if vowel { "an" } else { "a" })
}

/// `2026-01-31`
pub(crate) fn iso(day: Day) -> String {
    let (year, month, date) = day.ymd();
    format!("{year:04}-{month:02}-{date:02}")
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
