//! Diagnostics: what went wrong, where, and what to do about it.
//!
//! A diagnostic is data. It names source ranges and says things about them; the
//! command line decides how to draw it.

use std::borrow::Cow;

/// A loaded source file.
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct FileId(pub u16);

/// A byte range in one source file.
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Loc {
    pub file: FileId,
    pub start: u32,
    pub end: u32,
}

impl Loc {
    pub const fn new(file: FileId, start: u32, end: u32) -> Loc {
        Loc { file, start, end }
    }

    /// The smallest range covering both.
    pub fn to(self, other: Loc) -> Loc {
        debug_assert_eq!(self.file, other.file);
        Loc { file: self.file, start: self.start.min(other.start), end: self.end.max(other.end) }
    }

    pub fn range(self) -> std::ops::Range<usize> {
        self.start as usize..self.end as usize
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Severity {
    Error,
    Warning,
    Note,
}

/// What became of a problem the ledger accepted rather than rejected. The
/// summary line counts each.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, Debug)]
pub enum Disposition {
    /// Nothing: an error, a warning, or a plain note.
    #[default]
    Open,
    /// A `require … else owe …` that failed and was resolved to a loss.
    Priced,
    /// Accepted by `!` or relaxed mode.
    Waived,
}

#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub severity: Severity,
    /// A stable kebab-case name: `unknown-place`, or a law's own name
    /// (`deferral-limit`) for what a law found.
    pub code: Cow<'static, str>,
    pub disposition: Disposition,
    pub message: String,
    /// The first primary label anchors the diagnostic.
    pub labels: Vec<Label>,
    pub notes: Vec<String>,
    pub help: Vec<Help>,
}

#[derive(Clone, Debug)]
pub struct Label {
    pub loc: Loc,
    pub text: String,
    pub primary: bool,
}

/// Advice, optionally with the exact edit that carries it out.
#[derive(Clone, Debug)]
pub struct Help {
    pub text: String,
    pub edit: Option<(Loc, String)>,
}

impl Diagnostic {
    pub fn new(severity: Severity, code: impl Into<Cow<'static, str>>, message: impl Into<String>) -> Diagnostic {
        Diagnostic {
            severity,
            code: code.into(),
            disposition: Disposition::Open,
            message: message.into(),
            labels: vec![],
            notes: vec![],
            help: vec![],
        }
    }

    pub fn error(code: impl Into<Cow<'static, str>>, message: impl Into<String>) -> Diagnostic {
        Diagnostic::new(Severity::Error, code, message)
    }

    pub fn warning(code: impl Into<Cow<'static, str>>, message: impl Into<String>) -> Diagnostic {
        Diagnostic::new(Severity::Warning, code, message)
    }

    pub fn info(code: impl Into<Cow<'static, str>>, message: impl Into<String>) -> Diagnostic {
        Diagnostic::new(Severity::Note, code, message)
    }

    /// Records what became of the problem.
    pub fn disposed(mut self, disposition: Disposition) -> Diagnostic {
        self.disposition = disposition;
        self
    }

    /// Points at the cause.
    pub fn label(mut self, loc: Loc, text: impl Into<String>) -> Diagnostic {
        self.labels.push(Label { loc, text: text.into(), primary: true });
        self
    }

    /// Points at something relevant.
    pub fn context(mut self, loc: Loc, text: impl Into<String>) -> Diagnostic {
        self.labels.push(Label { loc, text: text.into(), primary: false });
        self
    }

    pub fn note(mut self, text: impl Into<String>) -> Diagnostic {
        self.notes.push(text.into());
        self
    }

    pub fn help(mut self, text: impl Into<String>) -> Diagnostic {
        self.help.push(Help { text: text.into(), edit: None });
        self
    }

    /// Advice that replaces `loc` with `replacement`.
    pub fn fix(mut self, text: impl Into<String>, loc: Loc, replacement: impl Into<String>) -> Diagnostic {
        self.help.push(Help { text: text.into(), edit: Some((loc, replacement.into())) });
        self
    }

    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }

    pub fn anchor(&self) -> Option<Loc> {
        self.labels.iter().find(|l| l.primary).or(self.labels.first()).map(|l| l.loc)
    }

    /// Demotes an error to a warning: relaxed mode, or a waived item.
    pub fn relaxed(mut self) -> Diagnostic {
        if self.severity == Severity::Error {
            self.severity = Severity::Warning;
        }
        self
    }
}

/// The candidate closest to `name` by edit distance, if it is close enough to
/// be a plausible typo. Candidates whose length alone puts them out of reach
/// are passed over without computing a distance.
pub fn closest<'a>(name: &str, candidates: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    let limit = (name.len() / 3).max(1);
    candidates
        .into_iter()
        .filter(|c| c.len().abs_diff(name.len()) <= limit)
        .map(|c| (distance(name, c), c))
        .filter(|&(d, c)| d <= limit && c != name)
        .min()
        .map(|(_, c)| c)
}

/// Optimal-string-alignment distance: insertions, deletions, substitutions, and
/// transpositions of adjacent characters each cost one.
pub fn distance(a: &str, b: &str) -> usize {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut rows = vec![vec![0usize; b.len() + 1]; 3];
    for (j, cell) in rows[1].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=a.len() {
        rows.rotate_left(1);
        rows[1][0] = i;
        for j in 1..=b.len() {
            let cost = (a[i - 1] != b[j - 1]) as usize;
            let mut best = (rows[0][j] + 1).min(rows[1][j - 1] + 1).min(rows[0][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                best = best.min(rows[2][j - 2] + 1);
            }
            rows[1][j] = best;
        }
    }
    rows[1][b.len()]
}

#[cfg(test)]
mod tests {
    #[test]
    fn suggestions() {
        let names = ["beneficiary", "born", "budget"];
        assert_eq!(super::closest("benificiary", names), Some("beneficiary"));
        assert_eq!(super::closest("bron", names), Some("born"));
        assert_eq!(super::closest("zzz", names), None);
    }
}
