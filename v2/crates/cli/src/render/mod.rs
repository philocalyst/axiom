//! Diagnostics, drawn.
//!
//! A [`Diagnostic`] says what is wrong, where, and what to do. The renderer
//! turns it into the shape compilers at their best have: the offending lines,
//! marks under exactly the right columns, the source of the rule that was broken,
//! and advice, with an edit shown where there is one.
//!
//! The work is split by what varies. [`source`] finds lines and columns,
//! [`labels`] arranges the marks under one line, [`snippet`] lays out one file's
//! lines, and [`page`] draws the frame around them. This module gathers them for
//! each diagnostic and orders and counts the lot.

mod labels;
mod page;
mod snippet;
mod source;

use axiom_core::diag::{Diagnostic, FileId, Help, Label, Severity};

use self::page::Page;
use self::snippet::{Row, Snippet};
pub use self::source::Locator;
use crate::project::Sources;
use crate::style::{Ink, Line, Terminal};
use crate::text::plural;

/// The inks a diagnostic draws its labels in.
#[derive(Clone, Copy)]
struct Inks {
    /// The cause: red for an error, yellow for a warning, cyan for a note.
    primary: Ink,
    /// Everything else the reader may need to see.
    secondary: Ink,
}

impl Inks {
    fn of(severity: Severity) -> Inks {
        let primary = match severity {
            Severity::Error => Ink::RED,
            Severity::Warning => Ink::YELLOW,
            Severity::Note => Ink::CYAN,
        };
        Inks { primary: primary.bold(), secondary: Ink::BLUE }
    }
}

/// How many errors and warnings there are.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Tally {
    pub errors: usize,
    pub warnings: usize,
}

impl Tally {
    pub fn of(diagnostics: &[&Diagnostic]) -> Tally {
        let count = |severity| diagnostics.iter().filter(|diagnostic| diagnostic.severity == severity).count();
        Tally { errors: count(Severity::Error), warnings: count(Severity::Warning) }
    }

    /// `✗ 2 errors, 1 warning`; nothing at all when there is nothing to count.
    pub fn line(self) -> Option<Line> {
        if self.errors == 0 {
            return (self.warnings > 0).then(|| Line::text(&plural(self.warnings, "warning"), Ink::YELLOW));
        }
        let mut line = Line::text(&format!("✗ {}", plural(self.errors, "error")), Ink::RED.bold());
        if self.warnings > 0 {
            line.push(", ", Ink::DIM);
            line.push(&plural(self.warnings, "warning"), Ink::YELLOW);
        }
        Some(line)
    }
}

/// Draws diagnostics against the sources they point into.
pub struct Renderer<'a> {
    locator: Locator<'a>,
    terminal: Terminal,
}

impl<'a> Renderer<'a> {
    /// A renderer for diagnostics that point into `sources`.
    pub fn new(sources: &'a Sources, terminal: Terminal) -> Renderer<'a> {
        Renderer { locator: Locator::new(sources), terminal }
    }

    /// Where things are in the sources, for anything else that needs to say.
    pub fn locator(&mut self) -> &mut Locator<'a> {
        &mut self.locator
    }

    /// Every diagnostic in file and source order, each followed by a blank
    /// line. Diagnostics that point nowhere come last.
    pub fn diagnostics(&mut self, diagnostics: &[&Diagnostic]) -> String {
        let mut ordered = diagnostics.to_vec();
        ordered.sort_by_key(|diagnostic| {
            diagnostic.anchor().map_or((true, FileId(0), 0), |loc| (false, loc.file, loc.start))
        });
        ordered.into_iter().map(|diagnostic| self.diagnostic(diagnostic) + "\n").collect()
    }

    /// One diagnostic, each line ending in a newline.
    pub fn diagnostic(&mut self, diagnostic: &Diagnostic) -> String {
        let inks = Inks::of(diagnostic.severity);
        let snippets = self.snippets(diagnostic, inks);
        let fixes: Vec<Vec<Row>> = diagnostic.help.iter().map(|help| self.fix_rows(help)).collect();
        let rows = snippets.iter().flat_map(|snippet| &snippet.rows).chain(fixes.iter().flatten());
        let widest = rows.filter_map(|row| row.gutter.number()).max();
        let page = Page { gutter: widest.map_or(1, |number| number.to_string().len()), width: self.terminal.width };

        let mut lines = vec![header(diagnostic, inks)];
        for (at, snippet) in snippets.iter().enumerate() {
            lines.push(page.frame(at == 0, snippet));
            lines.push(page.bar());
            lines.extend(snippet.rows.iter().map(|row| page.row(row)));
            lines.push(page.bar());
        }
        lines.extend(diagnostic.notes.iter().flat_map(|note| page.remark("note", Ink::BOLD, note)));
        for (help, rows) in diagnostic.help.iter().zip(&fixes) {
            lines.extend(page.remark("help", Ink::GREEN.bold(), &help.text));
            lines.extend(rows.iter().map(|row| page.row(row)));
        }
        if !snippets.is_empty() {
            lines.push(page.closer());
        }
        lines.iter().map(|line| line.render(self.terminal.painter) + "\n").collect()
    }

    /// One snippet per file the labels point into: the anchor's file first, the
    /// others in file order. A label in a file that was never loaded has nothing
    /// to show, and is left out.
    fn snippets(&mut self, diagnostic: &Diagnostic, inks: Inks) -> Vec<Snippet<'a>> {
        let anchor = diagnostic.anchor().map(|loc| loc.file);
        let mut files: Vec<FileId> = diagnostic.labels.iter().map(|label| label.loc.file).collect();
        files.sort_by_key(|&file| (Some(file) != anchor, file));
        files.dedup();
        let sources = self.locator.sources;
        let mut snippets = Vec::new();
        for id in files {
            let Some(file) = sources.get(id) else { continue };
            let labels: Vec<&Label> = diagnostic.labels.iter().filter(|label| label.loc.file == id).collect();
            snippets.push(snippet::snippet(file, self.locator.index(file), &labels, inks));
        }
        snippets
    }

    /// The edit a help carries, shown as the lines it would leave.
    fn fix_rows(&mut self, help: &Help) -> Vec<Row> {
        let Some((loc, replacement)) = &help.edit else { return Vec::new() };
        let Some(file) = self.locator.sources.get(loc.file) else { return Vec::new() };
        snippet::edited_lines(file, self.locator.index(file), *loc, replacement)
    }
}

/// `error[code]: message`. A diagnostic without a code has no brackets.
fn header(diagnostic: &Diagnostic, inks: Inks) -> Line {
    let word = match diagnostic.severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Note => "note",
    };
    let mut line = Line::text(word, inks.primary);
    if !diagnostic.code.is_empty() {
        line.push(&format!("[{}]", diagnostic.code), inks.primary);
    }
    line.push(": ", Ink::BOLD);
    line.push(&diagnostic.message, Ink::BOLD);
    line
}

#[cfg(test)]
mod tests;
