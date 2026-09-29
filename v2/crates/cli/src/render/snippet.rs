//! One file's share of a diagnostic: the source lines it points at, the marks
//! under them, and the margin that joins the two ends of a label spanning lines.
//!
//! ```text
//! 15 │ ╭─▶ 2026-01-15 acme -> 5_200 USD
//! 16 │ │     retirement   800 USD
//! 17 │ ├─▶   checking     ...
//!    │ │
//!    │ ╰── the legs add up to more than the header
//! ```

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use axiom_core::Loc;
use axiom_core::diag::Label;

use super::Inks;
use super::labels::{LineLabel, annotate};
use super::source::{LineIndex, clamp, columns_before};
use crate::project::SourceFile;
use crate::style::{Ink, Line, TAB_WIDTH};

/// A label spanning more lines than this shows only its first and last.
const SHOWN_SPAN: usize = 8;

/// One file's labelled lines, ready to be framed.
pub struct Snippet<'a> {
    pub path: &'a str,
    /// Where the snippet's main label begins.
    pub lead: Position,
    pub rows: Vec<Row>,
}

/// A line and column, counted from 1. Columns count characters, so a tab is
/// one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Position {
    pub line: usize,
    pub column: usize,
}

/// One printed row: what stands in the gutter, and what follows it.
pub struct Row {
    pub gutter: Gutter,
    pub content: Line,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Gutter {
    /// A source line, numbered from 1.
    Number(usize),
    /// A line as it would read after a suggested edit.
    Added(usize),
    /// Marks under a source line.
    Bar,
    /// Lines left out.
    Gap,
}

impl Gutter {
    /// The line number a row carries, if it carries one.
    pub fn number(self) -> Option<usize> {
        match self {
            Gutter::Number(number) | Gutter::Added(number) => Some(number),
            Gutter::Bar | Gutter::Gap => None,
        }
    }
}

/// A label whose ends are on different lines. Each gets a margin column.
struct Multi<'a> {
    first: usize,
    last: usize,
    column: usize,
    text: &'a str,
    ink: Ink,
}

impl Multi<'_> {
    /// Whether the label's bar runs on below `line`. Its own text hangs below
    /// its last line, after the tails of the labels to its right, so while
    /// those are printed (`pending` is the column being printed) it is still
    /// open there.
    fn is_open_below(&self, line: usize, pending: usize) -> bool {
        self.first <= line
            && (line < self.last || (line == self.last && !self.text.is_empty() && self.column < pending))
    }
}

/// Lays out the `labels` that fall in `file`. There is at least one.
pub fn snippet<'a>(file: &'a SourceFile, index: &LineIndex, labels: &[&Label], inks: Inks) -> Snippet<'a> {
    let mut layout = Layout { text: &file.text, index, singles: BTreeMap::new(), multis: Vec::new() };
    for label in labels {
        layout.place(label, inks);
    }
    layout.number_margin_columns();
    let lead = labels.iter().find(|label| label.primary).or_else(|| labels.iter().min_by_key(|label| label.loc.start));
    let lead = lead.map_or(Position { line: 1, column: 1 }, |label| layout.position(layout.span(label).0));
    Snippet { path: &file.path, lead, rows: layout.rows() }
}

/// The lines an edit would leave: the text before `loc` and after it, around
/// the replacement, which is highlighted. A replacement of several lines makes
/// several rows.
pub fn edited_lines(file: &SourceFile, index: &LineIndex, loc: Loc, replacement: &str) -> Vec<Row> {
    let text: &str = &file.text;
    let start = clamp(text, loc.start as usize);
    let end = clamp(text, loc.end as usize).max(start);
    let (first, last) = (index.line_of(start), index.line_of(end));
    let before = &text[index.start(first, text)..start];
    let last_line_end = index.start(last, text) + index.line(last, text).len();
    let after = &text[end..last_line_end.max(end)];

    let mut rows = Vec::new();
    let mut current = Line::text(before, Ink::PLAIN);
    for (at, piece) in replacement.split('\n').enumerate() {
        if at > 0 {
            rows.push(Row { gutter: Gutter::Added(first + at), content: std::mem::take(&mut current) });
        }
        current.push(piece, Ink::GREEN.bold());
    }
    current.push(after, Ink::PLAIN);
    rows.push(Row { gutter: Gutter::Added(first + rows.len() + 1), content: current });
    rows
}

struct Layout<'a> {
    text: &'a str,
    index: &'a LineIndex,
    /// Labels within one line, by line.
    singles: BTreeMap<usize, Vec<LineLabel<'a>>>,
    multis: Vec<Multi<'a>>,
}

impl<'a> Layout<'a> {
    fn line(&self, line: usize) -> &'a str {
        self.index.line(line, self.text)
    }

    fn position(&self, offset: usize) -> Position {
        let offset = offset.min(self.text.len());
        let line = self.index.line_of(offset);
        let column = columns_before(self.line(line), offset - self.index.start(line, self.text), 1) + 1;
        Position { line: line + 1, column }
    }

    /// The label's bytes without the whitespace around them: a span often takes
    /// in the indentation before it or the line ending after it. A span of
    /// nothing but whitespace is kept as it is, to point at the gap.
    fn span(&self, label: &Label) -> (usize, usize) {
        let bytes = self.text.as_bytes();
        let start = (label.loc.start as usize).min(bytes.len());
        let end = (label.loc.end as usize).clamp(start, bytes.len());
        let inside = &bytes[start..end];
        let solid = |byte: &u8| !byte.is_ascii_whitespace();
        match (inside.iter().position(solid), inside.iter().rposition(solid)) {
            (Some(first), Some(last)) => (start + first, start + last + 1),
            _ => (start, end),
        }
    }

    fn place(&mut self, label: &'a Label, inks: Inks) {
        let (start, end) = self.span(label);
        let first = self.index.line_of(start);
        let last = self.index.line_of(end.max(start + 1) - 1);
        let ink = if label.primary { inks.primary } else { inks.secondary };
        if first != last {
            self.multis.push(Multi { first, last, column: 0, text: &label.text, ink });
            return;
        }
        let origin = self.index.start(first, self.text);
        let line = self.line(first);
        let from = columns_before(line, start - origin, TAB_WIDTH);
        // An empty span still gets one column, so there is something to point at.
        let to = columns_before(line, end - origin, TAB_WIDTH).max(from + 1);
        let placed = LineLabel { start: from, end: to, text: &label.text, primary: label.primary, ink };
        self.singles.entry(first).or_default().push(placed);
    }

    /// Outer labels go left of the labels nested inside them.
    fn number_margin_columns(&mut self) {
        self.multis.sort_by_key(|multi| (multi.first, Reverse(multi.last)));
        for (column, multi) in self.multis.iter_mut().enumerate() {
            multi.column = column;
        }
    }

    /// Room for a bar per multi-line label, then `─▶` and a space.
    fn margin_width(&self) -> usize {
        if self.multis.is_empty() { 0 } else { self.multis.len() + 3 }
    }

    // ─── Which lines ────────────────────────────────────────────────────────

    /// Every labelled line, the heading a labelled line is indented under, and
    /// a line that is all that separates two shown ones.
    fn shown_lines(&self) -> Vec<usize> {
        let mut shown = BTreeSet::new();
        for &line in self.singles.keys() {
            shown.extend(self.heading_above(line));
            shown.insert(line);
        }
        for multi in &self.multis {
            shown.extend(self.heading_above(multi.first));
            shown.extend([multi.first, multi.last]);
            if multi.last - multi.first <= SHOWN_SPAN {
                shown.extend(multi.first..=multi.last);
            }
        }
        let bridges: Vec<usize> = shown
            .iter()
            .zip(shown.iter().skip(1))
            .filter(|&(&above, &below)| below == above + 2)
            .map(|(&above, _)| above + 1)
            .collect();
        shown.extend(bridges);
        shown.into_iter().collect()
    }

    /// The line directly above `line`, if `line` is indented under it: a leg
    /// under its transaction, a step under its law. It says what the line is
    /// part of.
    fn heading_above(&self, line: usize) -> Option<usize> {
        let above = line.checked_sub(1)?;
        let (this, heading) = (self.line(line), self.line(above));
        let indent = |text: &str| text.len() - text.trim_start().len();
        (!heading.trim().is_empty() && indent(heading) < indent(this)).then_some(above)
    }

    // ─── Rows ───────────────────────────────────────────────────────────────

    fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        let mut previous: Option<usize> = None;
        for line in self.shown_lines() {
            if let Some(above) = previous.filter(|&above| line > above + 1) {
                rows.push(Row { gutter: Gutter::Gap, content: self.verticals(above, 0) });
            }
            rows.extend(self.line_rows(line));
            previous = Some(line);
        }
        rows
    }

    /// A source line, the marks under it, and the text of the multi-line
    /// labels that end on it.
    fn line_rows(&self, line: usize) -> Vec<Row> {
        let mut code = self.code_margin(line);
        code.push(self.line(line), Ink::PLAIN);
        let mut rows = vec![Row { gutter: Gutter::Number(line + 1), content: code }];
        for marks in self.singles.get(&line).map(|labels| annotate(labels)).unwrap_or_default() {
            let mut content = self.verticals(line, usize::MAX);
            content.append(&marks);
            rows.push(Row { gutter: Gutter::Bar, content });
        }
        rows.extend(self.tails(line));
        rows
    }

    /// The text of every multi-line label that ends on `line`, rightmost first
    /// so no bar has to cross another label's text.
    fn tails(&self, line: usize) -> Vec<Row> {
        let mut ending: Vec<&Multi> =
            self.multis.iter().filter(|multi| multi.last == line && !multi.text.is_empty()).collect();
        if ending.is_empty() {
            return Vec::new();
        }
        ending.sort_by_key(|multi| Reverse(multi.column));
        let spacer = Row { gutter: Gutter::Bar, content: self.verticals(line, usize::MAX) };
        let tails = ending.into_iter().map(|multi| Row { gutter: Gutter::Bar, content: self.tail(multi, line) });
        std::iter::once(spacer).chain(tails).collect()
    }

    fn tail(&self, multi: &Multi, line: usize) -> Line {
        let mut row = self.verticals(line, multi.column);
        row.put(multi.column, "╰", multi.ink);
        dash(&mut row, multi.column + 1..self.margin_width() - 1, multi.ink);
        row.pad_to(self.margin_width());
        row.push(multi.text, Ink::PLAIN);
        row
    }

    // ─── Margins ────────────────────────────────────────────────────────────

    fn blank_margin(&self) -> Line {
        let mut margin = Line::new();
        margin.pad_to(self.margin_width());
        margin
    }

    /// The margin beside marks and gaps below `line`.
    fn verticals(&self, line: usize, pending: usize) -> Line {
        let mut margin = self.blank_margin();
        for multi in self.multis.iter().filter(|multi| multi.is_open_below(line, pending)) {
            margin.put(multi.column, "│", multi.ink);
        }
        margin
    }

    /// The margin beside a source line: bars through the labels that span it,
    /// and an arrow into the line where one begins or ends.
    fn code_margin(&self, line: usize) -> Line {
        let mut margin = self.blank_margin();
        for multi in self.multis.iter().filter(|multi| multi.first < line && line < multi.last) {
            margin.put(multi.column, "│", multi.ink);
        }
        for multi in &self.multis {
            if multi.first == line {
                self.arrow(&mut margin, multi, "╭");
            } else if multi.last == line {
                self.arrow(&mut margin, multi, if multi.text.is_empty() { "╰" } else { "├" });
            }
        }
        margin
    }

    fn arrow(&self, margin: &mut Line, multi: &Multi, corner: &str) {
        let tip = self.margin_width() - 2;
        margin.put(multi.column, corner, multi.ink);
        dash(margin, multi.column + 1..tip, multi.ink);
        margin.put(tip, "▶", multi.ink);
    }
}

/// Draws `─` over `columns`, as `┼` where it crosses a bar.
fn dash(margin: &mut Line, columns: Range<usize>, ink: Ink) {
    for column in columns {
        let glyph = if margin.glyph_at(column) == Some('│') { "┼" } else { "─" };
        margin.put(column, glyph, ink);
    }
}
