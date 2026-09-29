//! The diagnostics most stages share: a name nothing answers to, and a name
//! several things answer to.

use axiom_core::{Day, Diagnostic, Loc};
use axiom_syntax::Name;

/// `there is no place `chekcing`` with the closest known name as a fix.
pub(crate) fn unknown(code: &'static str, noun: &str, name: Name, suggestion: Option<&str>) -> Diagnostic {
    let diagnostic = Diagnostic::error(code, format!("there is no {noun} `{}`", name.text))
        .label(name.loc, format!("not a known {noun}"));
    match suggestion {
        Some(near) => diagnostic.fix(format!("did you mean `{near}`?"), name.loc, near),
        None => diagnostic,
    }
}

/// A thing that exists, declared by a system this reader has not used.
pub(crate) fn not_used(diagnostic: Diagnostic, noun: &str, name: &str, system: &str) -> Diagnostic {
    diagnostic
        .note(format!("the {noun} `{name}` is declared by system `{system}`, which is not used here"))
        .help(format!("add `use {system}` to bring it into scope"))
}

/// `entity acme` written twice.
pub(crate) fn duplicate(noun: &str, name: Name, first: Option<Loc>) -> Diagnostic {
    let diagnostic = Diagnostic::error("duplicate-declaration", format!("{noun} `{}` is declared twice", name.text))
        .label(name.loc, "declared again here");
    match first {
        Some(loc) => diagnostic.context(loc, "first declared here").help("remove one of the two declarations"),
        None => diagnostic.note("it is built in").help("remove this declaration"),
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

pub(crate) fn ambiguous(code: &'static str, plural: &str, name: Name, candidates: &[Candidate]) -> Diagnostic {
    let which = if candidates.len() == 2 { "either of these" } else { "any of these" };
    let mut diagnostic = Diagnostic::error(code, format!("`{}` could be {which} {plural}", name.text))
        .label(name.loc, "which one is meant?");
    for candidate in candidates {
        if let Some(loc) = candidate.declared {
            diagnostic = diagnostic.context(loc, format!("{} is declared here", candidate.is));
        }
        diagnostic = match &candidate.write {
            Some(write) => diagnostic.fix(format!("write `{write}` for {}", candidate.is), name.loc, write),
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
