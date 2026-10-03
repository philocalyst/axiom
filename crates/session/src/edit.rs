//! What a client sends a session and what it gets back: an edit as a typed value, the reasons one is refused, and
//! what an applied one changed.
//!
//! An edit is the form a diagnostic's own fix already has (`Help::edit`): the bytes of one file's text and what to put
//! there, so applying a fix is one call and a code action is an edit. Lines added at the end of a file are the other
//! form. Nothing here knows the language: an edit is applied to a text, and the text is parsed to see what it said.

use std::fmt;

use axiom_core::diag::Help;
use axiom_core::{Diagnostic, FileId, Loc, Map, Severity};
use axiom_report::{SourceProvider, json};

use crate::Session;

/// A change to one file of the project.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Edit {
    /// Put `text` where the bytes of `at` are. A diagnostic's fix is one of these ([`Edit::fix`]).
    Replace { at: Loc, text: String },
    /// Add `text` as lines at the end of the file, which gets a line ending first if it had none.
    Append { file: FileId, text: String },
}

impl Edit {
    /// The edit a diagnostic's help carries, if it carries one.
    pub fn fix(help: &Help) -> Option<Edit> {
        let (at, text) = help.edit.as_ref()?;
        Some(Edit::Replace { at: *at, text: text.clone() })
    }

    /// The file this edit is to.
    pub fn file(&self) -> FileId {
        match self {
            Edit::Replace { at, .. } => at.file,
            Edit::Append { file, .. } => *file,
        }
    }

    /// What `text`, the text of that file, reads after this edit.
    pub(crate) fn applied_to(&self, text: &str) -> Result<String, Refused> {
        match self {
            Edit::Replace { at, text: with } => {
                let (start, end) = (at.start as usize, at.end as usize);
                // `get` is `None` past the end and inside a character; a range that runs backwards is neither.
                let (Some(head), Some(tail)) = (text.get(..start), text.get(end..)) else {
                    return Err(Refused::NotSpan(*at));
                };
                if start > end {
                    return Err(Refused::NotSpan(*at));
                }
                Ok([head, with, tail].concat())
            }
            Edit::Append { text: lines, .. } => {
                let ended = text.is_empty() || text.ends_with('\n');
                let closed = lines.is_empty() || lines.ends_with('\n');
                Ok([text, if ended { "" } else { "\n" }, lines, if closed { "" } else { "\n" }].concat())
            }
        }
    }
}

/// Why an edit was not applied. Nothing has changed when a session says one of these.
#[derive(Clone, Debug)]
pub enum Refused {
    /// The edit is to a file the session has not loaded, or to a data file it was only given to read.
    NoSuchFile(FileId),
    /// The file ships with Axiom: it is not the client's to edit.
    Embedded(FileId),
    /// The bytes are not a span of the file's text: past its end, backwards, or inside a character.
    NotSpan(Loc),
    /// The file would say something that does not parse, and these are the syntax errors the edit would add.
    Unparsable(Vec<Diagnostic>),
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refused::NoSuchFile(file) => write!(f, "there is no source file number {}", file.0),
            Refused::Embedded(file) => write!(f, "file number {} ships with Axiom and cannot be edited", file.0),
            Refused::NotSpan(at) => {
                write!(f, "bytes {} to {} are not a span of the text of file number {}", at.start, at.end, at.file.0)
            }
            Refused::Unparsable(errors) => {
                let first = errors.first().map_or("", |error| error.message.lines().next().unwrap_or_default());
                write!(f, "the edit would add {} syntax error(s) to the file: {first}", errors.len())
            }
        }
    }
}

impl std::error::Error for Refused {}

/// What an applied edit did to the book: the diagnostics it introduced and the ones it cleared, in the book's own
/// words, so that a client knows what to refresh and what to say.
#[derive(Clone, Debug)]
pub struct Applied {
    /// The file that was edited.
    pub file: FileId,
    /// Found by the rebuilt book and not by the one before it.
    pub added: Vec<Diagnostic>,
    /// Found by the book before the edit and not by the rebuilt one.
    pub removed: Vec<Diagnostic>,
}

impl Applied {
    /// What `after` found that `before` did not, and the other way about, for an edit to `file`: what a client that
    /// asked [`Session::what_if`] reads from the session it was given, and what `apply` returns.
    pub fn between(file: FileId, before: &Session<'_>, after: &Session<'_>) -> Applied {
        let added = introduced(after.diagnostics(), before.diagnostics());
        let removed = introduced(before.diagnostics(), after.diagnostics());
        Applied { file, added: added.into_iter().cloned().collect(), removed: removed.into_iter().cloned().collect() }
    }

    /// `{"file":"journal/2026/03.ax","added":[...],"removed":[...]}`, each diagnostic as `check --json` writes it.
    pub fn json(&self, sources: &dyn SourceProvider) -> String {
        let mut out = String::from("{\"file\":");
        let path = sources.describe(Loc::new(self.file, 0, 0)).map_or("", |position| position.path);
        json::string(&mut out, path);
        for (key, diagnostics) in [("added", &self.added), ("removed", &self.removed)] {
            out.push_str(&format!(",\"{key}\":["));
            let objects: Vec<String> = diagnostics.iter().map(|found| json::diagnostic(found, sources)).collect();
            out.push_str(&objects.join(","));
            out.push(']');
        }
        out.push('}');
        out
    }
}

/// The diagnostics of `now` that `before` did not have. Two say the same thing if they have one severity, code and
/// message, and a thing said twice now and once before is said once more: where a diagnostic points is not compared,
/// because an edit moves every line after it.
pub(crate) fn introduced<'a>(
    now: impl IntoIterator<Item = &'a Diagnostic>,
    before: impl IntoIterator<Item = &'a Diagnostic>,
) -> Vec<&'a Diagnostic> {
    let said = |found: &'a Diagnostic| -> (Severity, &'a str, &'a str) {
        (found.severity, found.code.as_ref(), found.message.as_str())
    };
    let mut had: Map<(Severity, &str, &str), usize> = Map::default();
    for found in before {
        *had.entry(said(found)).or_default() += 1;
    }
    let unmatched = |found: &&'a Diagnostic| match had.get_mut(&said(found)) {
        Some(left) if *left > 0 => {
            *left -= 1;
            false
        }
        _ => true,
    };
    now.into_iter().filter(unmatched).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(code: &'static str, message: &str) -> Diagnostic {
        Diagnostic::error(code, message)
    }

    fn at(file: u16, start: u32, end: u32) -> Loc {
        Loc::new(FileId(file), start, end)
    }

    #[test]
    fn a_replacement_puts_its_text_where_the_bytes_were() {
        let edit = Edit::Replace { at: at(0, 3, 7), text: "this".to_string() };
        assert_eq!(edit.applied_to("one two three").unwrap(), "onethis three");
        let insert = Edit::Replace { at: at(0, 3, 3), text: "!".to_string() };
        assert_eq!(insert.applied_to("one two").unwrap(), "one! two", "an empty range inserts");
        let delete = Edit::Replace { at: at(0, 0, 4), text: String::new() };
        assert_eq!(delete.applied_to("one two").unwrap(), "two");
    }

    #[test]
    fn bytes_that_are_not_a_span_of_the_text_are_refused() {
        let text = "a\u{3bb}b";
        for (start, end) in [(0, 99), (99, 99), (3, 1), (2, 3), (1, 2)] {
            let edit = Edit::Replace { at: at(0, start, end), text: "x".to_string() };
            assert!(
                matches!(edit.applied_to(text), Err(Refused::NotSpan(found)) if found == at(0, start, end)),
                "{start}..{end} is not a span of {text:?}"
            );
        }
        let whole = Edit::Replace { at: at(0, 0, text.len() as u32), text: "x".to_string() };
        assert_eq!(whole.applied_to(text).unwrap(), "x", "the whole text, and its end, are spans");
    }

    #[test]
    fn lines_are_added_after_a_line_ending() {
        let append = |text: &str| Edit::Append { file: FileId(0), text: text.to_string() };
        assert_eq!(append("b\n").applied_to("a\n").unwrap(), "a\nb\n");
        assert_eq!(append("b").applied_to("a").unwrap(), "a\nb\n", "a file without a last line ending gets one first");
        assert_eq!(append("b\n").applied_to("").unwrap(), "b\n", "an empty file starts with the line");
        assert_eq!(append("").applied_to("a\n").unwrap(), "a\n", "nothing added is nothing changed");
    }

    #[test]
    fn a_help_with_an_edit_is_an_edit() {
        let help = Help { text: "write it so".to_string(), edit: Some((at(2, 4, 9), "fixed".to_string())) };
        assert_eq!(Edit::fix(&help), Some(Edit::Replace { at: at(2, 4, 9), text: "fixed".to_string() }));
        assert_eq!(Edit::fix(&Help { text: "think".to_string(), edit: None }), None);
        assert_eq!(Edit::fix(&help).unwrap().file(), FileId(2));
    }

    #[test]
    fn what_a_diagnostic_says_is_counted_not_located() {
        let before = [said("a", "one"), said("a", "one"), said("b", "two")];
        let now = [said("a", "one"), said("b", "two"), said("c", "three"), said("a", "one"), said("a", "one")];
        let new: Vec<_> = introduced(&now, &before).into_iter().map(|found| &*found.code).collect();
        assert_eq!(new, ["c", "a"], "the third `a one` and the new `c`, in the order `now` has them");
        let gone: Vec<_> = introduced(&before, &now).into_iter().collect();
        assert!(gone.is_empty());
        let warning = Diagnostic::warning("a", "one");
        assert_eq!(introduced([&warning], &before).len(), 1, "a warning is not the error of the same words");
        let other = said("z", "one");
        assert_eq!(introduced([&other], &before).len(), 1, "nor is another code that says the same words");
    }
}
