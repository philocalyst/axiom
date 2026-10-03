//! One file's share of a diagnostic, as rows: the source lines its labels point
//! at with the marks under them, or the lines an edit would change.
//!
//! ```text
//! 15 │ 2026-01-15 acme -> 5_200 USD
//!    │            ──┬─
//!    │              ╰── acme pays
//! 16 │   retirement   800 USD
//! ```
//!
//! A label that spans several lines is marked on each of them, with its text
//! under the last, so one layout serves every label.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use axiom_core::Loc;
use axiom_core::diag::Label;
use axiom_session::SourceFile;

use super::labels::{LineLabel, annotate};
use crate::style::{Ink, Line, TAB_WIDTH};

/// A label spanning more lines than this is marked only on its first and last.
const SHOWN_SPAN: usize = 8;

/// The fewest columns of a source line worth showing, however narrow the
/// terminal claims to be.
const MIN_ROOM: usize = 30;

/// One file's rows, ready to be framed.
pub struct Panel<'a> {
    pub file: &'a SourceFile,
    /// Where the panel's main label begins.
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
    /// A line as a suggested edit would leave it.
    Added(usize),
    /// A line a suggested edit takes away or rewrites.
    Removed(usize),
    /// Marks under a source line.
    Bar,
    /// Lines left out.
    Gap,
}

impl Gutter {
    /// The line number a row carries, if it carries one.
    pub fn number(self) -> Option<usize> {
        match self {
            Gutter::Number(number) | Gutter::Added(number) | Gutter::Removed(number) => Some(number),
            Gutter::Bar | Gutter::Gap => None,
        }
    }
}

/// Lays out the `labels` that fall in `file`, with source lines drawn no wider
/// than `width`. There is at least one label.
pub fn snippet<'a>(file: &'a SourceFile, labels: &[&Label], primary: Ink, width: usize) -> Panel<'a> {
    let room = width.saturating_sub(file.lines().to_string().len() + 5).max(MIN_ROOM);
    let mut layout = Layout { file, room, marks: BTreeMap::new() };
    for label in labels {
        layout.place(label, primary);
    }
    let lead = labels.iter().find(|label| label.primary).or_else(|| labels.iter().min_by_key(|label| label.loc.start));
    let lead = lead.map_or(Position { line: 1, column: 1 }, |label| layout.position(layout.span(label).0));
    Panel { file, lead, rows: layout.rows() }
}

/// An edit as a diff: the lines it takes away, then the lines it puts in their
/// place. What both versions share above and below is left out, so adding a
/// line shows only that line, and a line that ends up blank is not drawn.
pub fn edit<'a>(file: &'a SourceFile, loc: Loc, replacement: &str) -> Panel<'a> {
    let text: &str = &file.text;
    let start = clamp(text, loc.start as usize);
    let end = clamp(text, loc.end as usize).max(start);
    let (first, last) = (file.line_of(start), file.line_of(end));
    let head = &text[file.line_start(first)..start];
    let tail = &text[end..(file.line_start(last) + file.line(last).len()).max(end)];

    let removed = &text[start..end];
    let (before, after) = (format!("{head}{removed}{tail}"), format!("{head}{replacement}{tail}"));
    let old = split(&before, head.len()..head.len() + removed.len(), Ink::RED.bold());
    let new = split(&after, head.len()..head.len() + replacement.len(), Ink::GREEN.bold());
    let top = old.iter().zip(&new).take_while(|(a, b)| a.0 == b.0).count();
    let bottom = old[top..].iter().rev().zip(new[top..].iter().rev()).take_while(|(a, b)| a.0 == b.0).count();

    // An insertion takes nothing away, so there is nothing to strike out.
    let mut rows = if removed.is_empty() { Vec::new() } else { changed(old, top, bottom, first, Gutter::Removed) };
    rows.extend(changed(new, top, bottom, first, Gutter::Added));
    let column = columns_before(file.line(first), head.len(), 1) + 1;
    Panel { file, lead: Position { line: first + 1, column }, rows }
}

/// `full` as lines, each with its text and drawn with what lies in `marked`
/// in `ink`.
fn split(full: &str, marked: Range<usize>, ink: Ink) -> Vec<(&str, Line)> {
    let mut offset = 0;
    full.split('\n')
        .map(|piece| {
            let cut = |at: usize| at.clamp(offset, offset + piece.len()) - offset;
            let (from, to) = (cut(marked.start), cut(marked.end));
            let mut line = Line::text(&piece[..from], Ink::PLAIN);
            line.push(&piece[from..to], ink);
            line.push(&piece[to..], Ink::PLAIN);
            offset += piece.len() + 1;
            (piece, line)
        })
        .collect()
}

/// The rows for the lines of `all` between the `top` and `bottom` ones that
/// did not change, numbered from `first`. A blank line is not worth a row.
fn changed(all: Vec<(&str, Line)>, top: usize, bottom: usize, first: usize, gutter: fn(usize) -> Gutter) -> Vec<Row> {
    let end = all.len() - bottom;
    all.into_iter()
        .enumerate()
        .take(end)
        .skip(top)
        .filter(|(_, (piece, _))| !piece.trim().is_empty())
        .map(|(at, (_, content))| Row { gutter: gutter(first + at + 1), content })
        .collect()
}

struct Layout<'a> {
    file: &'a SourceFile,
    /// The most columns of a source line that are shown.
    room: usize,
    /// The marks on each line.
    marks: BTreeMap<usize, Vec<LineLabel<'a>>>,
}

impl<'a> Layout<'a> {
    fn position(&self, offset: usize) -> Position {
        let line = self.file.line_of(offset);
        let column = columns_before(self.file.line(line), offset - self.file.line_start(line), 1) + 1;
        Position { line: line + 1, column }
    }

    /// The bytes `start..end` without the whitespace around them: a span often
    /// takes in the indentation before it or the line ending after it. A span of
    /// nothing but whitespace is kept as it is, to point at the gap.
    fn trim(&self, start: usize, end: usize) -> (usize, usize) {
        let bytes = self.file.text.as_bytes();
        let start = start.min(bytes.len());
        let inside = &bytes[start..end.clamp(start, bytes.len())];
        let solid = |byte: &u8| !byte.is_ascii_whitespace();
        match (inside.iter().position(solid), inside.iter().rposition(solid)) {
            (Some(first), Some(last)) => (start + first, start + last + 1),
            _ => (start, start + inside.len()),
        }
    }

    fn span(&self, label: &Label) -> (usize, usize) {
        self.trim(label.loc.start as usize, label.loc.end as usize)
    }

    /// Marks `label` on every line it covers, its text hanging from the last.
    fn place(&mut self, label: &'a Label, primary: Ink) {
        let (start, end) = self.span(label);
        let (first, last) = (self.file.line_of(start), self.file.line_of(end.max(start + 1) - 1));
        let ink = if label.primary { primary } else { Ink::BLUE };
        let covered: Vec<usize> = if last - first > SHOWN_SPAN { vec![first, last] } else { (first..=last).collect() };
        for line in covered {
            let text = self.file.line(line);
            if first != last && text.trim().is_empty() {
                continue;
            }
            let origin = self.file.line_start(line);
            let (from, to) = self.trim(start.max(origin), end.min(origin + text.len()));
            let from = columns_before(text, from - origin, TAB_WIDTH);
            // An empty span still gets one column, so there is something to point at.
            let to = columns_before(text, to - origin, TAB_WIDTH).max(from + 1);
            let words = if line == last { label.text.as_str() } else { "" };
            self.marks.entry(line).or_default().push(LineLabel {
                start: from,
                end: to,
                text: words,
                primary: label.primary,
                ink,
            });
        }
    }

    // ─── Which lines ────────────────────────────────────────────────────────

    /// Every marked line, the heading a marked line is indented under, and a
    /// line that is all that separates two shown ones.
    fn shown_lines(&self) -> Vec<usize> {
        let mut shown = BTreeSet::new();
        for &line in self.marks.keys() {
            shown.extend(self.heading_above(line));
            shown.insert(line);
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
        let (this, heading) = (self.file.line(line), self.file.line(above));
        let indent = |text: &str| text.len() - text.trim_start().len();
        (!heading.trim().is_empty() && indent(heading) < indent(this)).then_some(above)
    }

    // ─── Rows ───────────────────────────────────────────────────────────────

    fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        let mut previous: Option<usize> = None;
        for line in self.shown_lines() {
            if previous.is_some_and(|above| line > above + 1) {
                rows.push(Row { gutter: Gutter::Gap, content: Line::new() });
            }
            rows.extend(self.line_rows(line));
            previous = Some(line);
        }
        rows
    }

    /// A source line, cut to what fits around its marks (`…` shows where), and
    /// the marks under it.
    fn line_rows(&self, line: usize) -> Vec<Row> {
        let text = self.file.line(line);
        let marks = self.marks.get(&line).map_or(&[][..], Vec::as_slice);
        let width = columns_before(text, text.len(), TAB_WIDTH);
        // A mark may point just past the end of the line: at where something is missing.
        let focus = focus_of(marks);
        let shown = window(width.max(focus.end), focus, self.room);
        let lead = usize::from(shown.start > 0);

        let mut code = Line::new();
        if lead > 0 {
            code.push("…", Ink::DIM);
        }
        code.append(&Line::excerpt(text, shown.start, shown.end, Ink::PLAIN));
        if shown.end < width {
            code.push("…", Ink::DIM);
        }
        let in_view = |mark: &&LineLabel| mark.start < shown.end && mark.end > shown.start;
        let moved: Vec<LineLabel> = marks
            .iter()
            .filter(in_view)
            .map(|mark| LineLabel {
                start: mark.start.max(shown.start) + lead - shown.start,
                end: mark.end.min(shown.end) + lead - shown.start,
                ..*mark
            })
            .collect();
        let mut rows = vec![Row { gutter: Gutter::Number(line + 1), content: code }];
        if !moved.is_empty() {
            rows.extend(annotate(&moved).into_iter().map(|content| Row { gutter: Gutter::Bar, content }));
        }
        rows
    }
}

/// The columns the reader must see: those of the primary marks, or of all of
/// them if none is primary.
fn focus_of(marks: &[LineLabel]) -> Range<usize> {
    let any_primary = marks.iter().any(|mark| mark.primary);
    let chosen = || marks.iter().filter(move |mark| mark.primary || !any_primary);
    let start = chosen().map(|mark| mark.start).min().unwrap_or(0);
    start..chosen().map(|mark| mark.end).max().unwrap_or(0)
}

/// Which of the `width` columns of a line to show when only `room` fit: from
/// its start if the `focus` is in view from there, else with about as much
/// before the focus as after.
fn window(width: usize, focus: Range<usize>, room: usize) -> Range<usize> {
    if width <= room {
        return 0..width;
    }
    let before = room.saturating_sub(focus.len()) / 2;
    let start = if focus.end <= room { 0 } else { focus.start.saturating_sub(before).min(width - room) };
    start..start + room
}

/// How many columns the part of `line` before byte `offset` takes, counting a
/// tab as `tab` columns and every other character as one. An `offset` inside a
/// character counts that character, and one past the end counts the whole line.
fn columns_before(line: &str, offset: usize, tab: usize) -> usize {
    line.char_indices().take_while(|&(at, _)| at < offset).map(|(_, ch)| if ch == '\t' { tab } else { 1 }).sum()
}

/// The nearest character boundary at or before `offset`, and never past the end.
fn clamp(text: &str, offset: usize) -> usize {
    (0..=offset.min(text.len())).rev().find(|&at| text.is_char_boundary(at)).unwrap_or(0)
}
