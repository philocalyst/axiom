//! Diagnostics, drawn.
//!
//! A [`Diagnostic`] says what is wrong, where, and what to do. The renderer
//! turns it into the shape compilers at their best have: the offending lines in
//! the reader's own file, the other end of a conflict under its own header,
//! marks under exactly the right columns, facts, and advice with the edit shown
//! as a diff where there is one.
//!
//! The work is split by what varies. [`labels`] arranges the marks under one
//! line, [`snippet`] lays out one file's rows (labelled lines, or an edit), and
//! [`page`] draws the frame around them. [`findings`] decides what to show of
//! many diagnostics; this module gathers the rest for each one.

mod findings;
pub mod json;
mod labels;
mod page;
mod snippet;

use axiom_core::diag::{Diagnostic, Help, Label, Loc, Severity};
use axiom_session::{SourceFile, Sources};

pub use self::findings::Tally;
use self::findings::{SHOWN, arrange};
use self::page::Page;
use self::snippet::Panel;
use crate::style::{Ink, Line, Terminal};
use crate::text::plural;

/// How many of the other places a repeated diagnostic occurs at are named.
const NAMED: usize = 4;

/// Draws diagnostics against the sources they point into.
/// How many of the findings are drawn.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Limit {
    /// No more than [`SHOWN`]; the rest are counted.
    Capped,
    /// Every one.
    Every,
}

pub struct Renderer<'a> {
    sources: &'a Sources<'a>,
    terminal: Terminal,
}

impl<'a> Renderer<'a> {
    /// A renderer for diagnostics that point into `sources`.
    pub fn new(sources: &'a Sources<'a>, terminal: Terminal) -> Renderer<'a> {
        Renderer { sources, terminal }
    }

    /// The diagnostics as a reader wants them, each followed by a blank line:
    /// errors first, each kind in source order, and those that say the same
    /// thing once, naming where else. Unless `limit` is [`Limit::Every`], no
    /// more than [`SHOWN`] are drawn, and the rest are counted. Also how many
    /// of every kind there were, drawn or not.
    pub fn present(&self, diagnostics: &[&Diagnostic], limit: Limit) -> (String, Tally) {
        let findings = arrange(diagnostics, |diagnostic| self.lead(diagnostic));
        let shown = match limit {
            Limit::Every => findings.keys(),
            Limit::Capped => findings.keys().min(SHOWN),
        };
        let mut text = String::new();
        for (_, group) in findings.iter().take(shown) {
            let mut first = group[0].clone();
            first.notes.extend(self.also(group[0], &group[1..]));
            text += &(self.diagnostic(&first) + "\n");
        }
        let hidden: Vec<&Diagnostic> =
            findings.iter().skip(shown).flat_map(|(_, group)| group.iter().copied()).collect();
        if !hidden.is_empty() {
            let counts = Tally::of(hidden.iter().copied()).counts();
            let counted = if counts.is_empty() { String::new() } else { format!(" ({counts})") };
            let note =
                format!("… {} not shown{counted}; `--all` shows every one", plural(hidden.len(), "more diagnostic"));
            text += &self.terminal.painter.paint(&[Line::text(&note, Ink::DIM), Line::new()]);
        }
        (text, Tally::of(diagnostics.iter().copied()))
    }

    /// Where a diagnostic is read from: its primary label in a file the reader
    /// can edit, else any label in one, else wherever it is anchored (a built-in
    /// source, when the fault is in the reader's file but the rule is not).
    fn lead(&self, diagnostic: &Diagnostic) -> Option<Loc> {
        let editable = |label: &&Label| self.sources.get(label.loc.file).is_some_and(|file| !file.embedded);
        let mut labels = diagnostic.labels.iter().filter(editable);
        let label = labels.clone().find(|label| label.primary).or_else(|| labels.next());
        label.map(|label| label.loc).or_else(|| diagnostic.anchor())
    }

    /// What to say of the repeats of a diagnostic: `also at a.ax:11, a.ax:12,
    /// … (296 more)`. They come in source order, so a place named twice is
    /// named once.
    fn also(&self, first: &Diagnostic, repeats: &[&Diagnostic]) -> Option<String> {
        let place = |diagnostic: &Diagnostic| self.sources.describe(self.lead(diagnostic)?);
        let own = place(first);
        let mut elsewhere: Vec<String> = repeats.iter().filter_map(|repeat| place(repeat)).collect();
        elsewhere.retain(|place| Some(place) != own.as_ref());
        elsewhere.dedup();
        let named = elsewhere.len().min(NAMED);
        let (more, places) = (repeats.len() - named, elsewhere[..named].join(", "));
        (!repeats.is_empty()).then(|| match (named, more) {
            (0, _) => format!("{more} more like this"),
            (_, 0) => format!("also at {places}"),
            _ => format!("also at {places}, … ({more} more)"),
        })
    }

    /// One diagnostic, each line ending in a newline.
    pub fn diagnostic(&self, diagnostic: &Diagnostic) -> String {
        let (word, ink) = match diagnostic.severity {
            Severity::Error => ("error", Ink::RED),
            Severity::Warning => ("warning", Ink::YELLOW),
            Severity::Note => ("note", Ink::CYAN),
        };
        let panels = self.panels(diagnostic, ink.bold());
        let fixes: Vec<Option<Panel>> = diagnostic.help.iter().map(|help| self.fix(help)).collect();
        let rows = panels.iter().chain(fixes.iter().flatten()).flat_map(|panel| &panel.rows);
        let widest = rows.filter_map(|row| row.gutter.number()).max();
        let page = Page { gutter: widest.map_or(1, |number| number.to_string().len()), width: self.terminal.width };

        let mut lines = vec![header(diagnostic, word, ink.bold())];
        for (at, panel) in panels.iter().enumerate() {
            lines.push(page.frame(at == 0, panel));
            lines.push(page.bar());
            lines.extend(panel.rows.iter().map(|row| page.row(row)));
            lines.push(page.bar());
        }
        let remarks: Vec<Line> =
            diagnostic.notes.iter().flat_map(|note| page.remark("note", Ink::BOLD, note)).collect();
        if remarks.is_empty() && diagnostic.help.is_empty() && !panels.is_empty() {
            lines.pop();
        }
        lines.extend(remarks);
        for (help, fix) in diagnostic.help.iter().zip(&fixes) {
            lines.extend(page.remark("help", Ink::GREEN.bold(), &help.text));
            let Some(panel) = fix else { continue };
            if !panels.iter().any(|shown| shown.file.id == panel.file.id) {
                lines.push(page.frame(panels.is_empty(), panel));
            }
            lines.extend(panel.rows.iter().map(|row| page.row(row)));
        }
        self.terminal.painter.paint(&lines)
    }

    /// One panel per file the labels point into: those in files the reader can
    /// edit first, the diagnostic's own file before the others, built-in sources
    /// last. A label in a file that was never loaded has nothing to show, and is
    /// left out.
    fn panels(&self, diagnostic: &Diagnostic, primary: Ink) -> Vec<Panel<'a>> {
        let lead = self.lead(diagnostic).map(|loc| loc.file);
        let mut files: Vec<&SourceFile> =
            diagnostic.labels.iter().filter_map(|label| self.sources.get(label.loc.file)).collect();
        files.sort_by_key(|file| (file.embedded, Some(file.id) != lead, file.id));
        files.dedup_by_key(|file| file.id);
        let panel = |file: &'a SourceFile| {
            let labels: Vec<&Label> = diagnostic.labels.iter().filter(|label| label.loc.file == file.id).collect();
            snippet::snippet(file, &labels, primary, self.terminal.width)
        };
        files.into_iter().map(panel).collect()
    }

    /// The edit a help carries, drawn as a diff.
    fn fix(&self, help: &Help) -> Option<Panel<'a>> {
        let (loc, replacement) = help.edit.as_ref()?;
        Some(snippet::edit(self.sources.get(loc.file)?, *loc, replacement))
    }
}

/// `error[code]: message`. A diagnostic without a code has no brackets.
fn header(diagnostic: &Diagnostic, word: &str, ink: Ink) -> Line {
    let mut line = Line::text(word, ink);
    if !diagnostic.code.is_empty() {
        line.push(&format!("[{}]", diagnostic.code), ink);
    }
    line.push(&format!(": {}", diagnostic.message), Ink::BOLD);
    line
}

#[cfg(test)]
mod tests;
