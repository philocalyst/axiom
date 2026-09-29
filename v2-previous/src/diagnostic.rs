//! Compact source diagnostics shared by authoring and evaluation reports.
//!
//! This is presentation only: labels borrow source locations, while the
//! persisted semantic model remains the authority for claims and findings.

use crate::Span;
use serde::{Deserialize, Serialize};
use std::fmt;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// A named source buffer used to render an issue.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Source<'a> {
    pub name: &'a str,
    pub text: &'a str,
}

impl<'a> Source<'a> {
    pub const fn new(name: &'a str, text: &'a str) -> Self {
        Self { name, text }
    }
}

/// An additional location associated with a diagnostic, such as the other
/// side of a conflict.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Label {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_index: Option<usize>,
    pub span: Span,
    pub message: String,
}

impl Label {
    pub fn new(source_index: Option<usize>, span: Span, message: impl Into<String>) -> Self {
        Self {
            source_index,
            span,
            message: message.into(),
        }
    }
}

/// The kind of question or blocker described by a report.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueKind {
    #[default]
    Error,
    Note,
    NeedsDecision,
    MissingInformation,
    Conflict,
    Incomplete,
}

impl IssueKind {
    const fn heading(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Note => "note",
            Self::NeedsDecision => "needs a decision",
            Self::MissingInformation => "missing information",
            Self::Conflict => "conflict",
            Self::Incomplete => "cannot determine",
        }
    }
}

/// A human-readable problem with optional source locations and actionable help.
///
/// `line` and `message` remain the stable, minimal diagnostic payload. The
/// additional context is optional so callers can gradually provide precise
/// source locations without parsing or changing their prose.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Diagnostic {
    #[serde(default)]
    pub kind: IssueKind,
    pub line: usize,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_index: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span: Option<Span>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub help: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related: Vec<Label>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
}

impl Diagnostic {
    pub fn new(line: usize, message: impl Into<String>) -> Self {
        Self {
            kind: IssueKind::Error,
            line: line.max(1),
            message: message.into(),
            source_index: None,
            span: None,
            help: None,
            related: Vec::new(),
            notes: Vec::new(),
            suggestion: None,
        }
    }

    pub fn kind(mut self, kind: IssueKind) -> Self {
        self.kind = kind;
        self
    }

    /// Attach a span before the caller has assigned its source index.
    pub fn with_span(mut self, span: Span) -> Self {
        self.line = span.line.max(1);
        self.span = Some(span);
        self
    }

    pub fn in_source(mut self, source_index: usize) -> Self {
        self.source_index = Some(source_index);
        self
    }

    pub fn at(mut self, source_index: usize, span: Span) -> Self {
        self.line = span.line.max(1);
        self.source_index = Some(source_index);
        self.span = Some(span);
        self
    }

    pub fn help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    pub fn related(mut self, label: Label) -> Self {
        self.related.push(label);
        self
    }

    pub fn note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    pub fn suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestion = Some(suggestion.into());
        self
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

/// Render diagnostics with bounded source excerpts and no terminal styling.
///
/// Source indices are explicit. An unassigned diagnostic is never silently
/// attributed to the first source in a multi-file compilation.
pub fn render_diagnostics(sources: &[Source<'_>], diagnostics: &[Diagnostic]) -> String {
    let mut output = String::new();
    for (index, diagnostic) in diagnostics.iter().take(MAX_DIAGNOSTICS).enumerate() {
        if index != 0 {
            output.push('\n');
        }
        render_diagnostic(&mut output, sources, diagnostic);
    }
    let omitted = diagnostics.len().saturating_sub(MAX_DIAGNOSTICS);
    if omitted > 0 {
        if !output.is_empty() {
            output.push('\n');
        }
        output.push_str(&format!("= {omitted} additional diagnostics omitted\n"));
    }
    output
}

const MAX_DIAGNOSTICS: usize = 50;
const MAX_RELATED_LABELS: usize = 8;
const MAX_NOTES: usize = 8;
const MAX_MESSAGE_CHARS: usize = 512;
const MAX_SOURCE_NAME_CHARS: usize = 160;
const MAX_SUGGESTION_CHARS: usize = 4096;
const MAX_SUGGESTION_LINES: usize = 16;

fn render_diagnostic(output: &mut String, sources: &[Source<'_>], diagnostic: &Diagnostic) {
    output.push_str(diagnostic.kind.heading());
    output.push_str(": ");
    output.push_str(&escape_controls_limited(
        &diagnostic.message,
        MAX_MESSAGE_CHARS,
    ));
    output.push('\n');

    let mut rendered_primary_location = false;
    if let Some(source_index) = diagnostic.source_index {
        if let Some(source) = sources.get(source_index) {
            let line_number = diagnostic
                .span
                .as_ref()
                .map_or(diagnostic.line.max(1), |span| span.line.max(1));
            let valid_span = diagnostic
                .span
                .and_then(|span| validate_span(source.text, span));
            rendered_primary_location = render_location(output, *source, line_number, valid_span);
        }
    }
    if !rendered_primary_location {
        output.push_str(&format!("  --> line {}\n", diagnostic.line.max(1)));
    }

    for note in diagnostic.notes.iter().take(MAX_NOTES) {
        output.push_str("  = note: ");
        output.push_str(&escape_controls_limited(note, MAX_MESSAGE_CHARS));
        output.push('\n');
    }
    let omitted_notes = diagnostic.notes.len().saturating_sub(MAX_NOTES);
    if omitted_notes > 0 {
        output.push_str(&format!("  = note: {omitted_notes} additional notes omitted\n"));
    }

    for label in diagnostic.related.iter().take(MAX_RELATED_LABELS) {
        output.push_str("  = note: ");
        output.push_str(&escape_controls_limited(&label.message, MAX_MESSAGE_CHARS));
        output.push('\n');
        if let Some(source_index) = label.source_index {
            if let Some(source) = sources.get(source_index) {
                let valid_span = validate_span(source.text, label.span);
                let _ = render_location(output, *source, label.span.line.max(1), valid_span);
            }
        }
    }
    let omitted_related = diagnostic.related.len().saturating_sub(MAX_RELATED_LABELS);
    if omitted_related > 0 {
        output.push_str(&format!(
            "  = note: {omitted_related} additional related locations omitted\n"
        ));
    }

    if let Some(help) = &diagnostic.help {
        output.push_str("  = help: ");
        output.push_str(&escape_controls_limited(help, MAX_MESSAGE_CHARS));
        output.push('\n');
    }

    if let Some(suggestion) = &diagnostic.suggestion {
        render_suggestion(output, suggestion);
    }
}

fn render_suggestion(output: &mut String, suggestion: &str) {
    output.push_str("  = suggestion:\n");
    let mut remaining = MAX_SUGGESTION_CHARS;
    let mut lines = 0usize;
    let mut truncated = false;
    for line in suggestion.split('\n') {
        if lines >= MAX_SUGGESTION_LINES || remaining == 0 {
            truncated = true;
            break;
        }
        let line = line.strip_suffix('\r').unwrap_or(line);
        let escaped = escape_controls_limited(line, remaining);
        remaining = remaining.saturating_sub(escaped.chars().count());
        output.push_str("    ");
        output.push_str(&escaped);
        output.push('\n');
        lines += 1;
        if escaped.ends_with('…') && escaped.chars().count() >= remaining {
            truncated = true;
            break;
        }
    }
    if truncated {
        output.push_str("    … suggestion truncated\n");
    }
}

#[derive(Clone, Copy)]
struct CheckedSpan {
    line_start: usize,
    start: usize,
    end: usize,
}

fn validate_span(source: &str, span: Span) -> Option<CheckedSpan> {
    if span.line == 0 || span.start > span.end || span.end > source.len() {
        return None;
    }
    if !source.is_char_boundary(span.start) || !source.is_char_boundary(span.end) {
        return None;
    }
    let (line_start, line_end) = line_bounds(source, span.line)?;
    if span.start < line_start || span.start > line_end || span.end > line_end {
        return None;
    }
    Some(CheckedSpan {
        line_start,
        start: span.start,
        end: span.end,
    })
}

fn line_bounds(source: &str, requested: usize) -> Option<(usize, usize)> {
    if requested == 0 {
        return None;
    }
    if source.is_empty() {
        return (requested == 1).then_some((0, 0));
    }
    let mut offset = 0;
    for (index, chunk) in source.split_inclusive('\n').enumerate() {
        let body = chunk.strip_suffix('\n').unwrap_or(chunk);
        let body = body.strip_suffix('\r').unwrap_or(body);
        if index + 1 == requested {
            return Some((offset, offset + body.len()));
        }
        offset += chunk.len();
    }
    if source.ends_with('\n') && requested == source.split('\n').count() {
        return Some((source.len(), source.len()));
    }
    None
}

fn render_location(
    output: &mut String,
    source: Source<'_>,
    line_number: usize,
    checked: Option<CheckedSpan>,
) -> bool {
    let Some(line) = line_text(source.text, line_number) else {
        return false;
    };
    let source_name = if source.name.is_empty() {
        "<source>".to_owned()
    } else {
        escape_controls_limited(source.name, MAX_SOURCE_NAME_CHARS)
    };

    let local_span = checked.map(|span| (span.start - span.line_start, span.end - span.line_start));
    let start_col = local_span
        .and_then(|(start, _)| line.get(..start))
        .map(display_width)
        .unwrap_or(0);
    let width = local_span
        .and_then(|(start, end)| line.get(start..end))
        .map(display_width)
        .map(|width| width.max(1))
        .unwrap_or(0);
    let (excerpt, clip_offset, left_ellipsis, right_ellipsis) =
        excerpt(&line, local_span.map(|(start, _)| start));
    let column = start_col.saturating_sub(clip_offset) + usize::from(left_ellipsis);

    output.push_str("  --> ");
    output.push_str(&source_name);
    output.push(':');
    output.push_str(&line_number.to_string());
    if checked.is_some() {
        output.push(':');
        output.push_str(&(start_col + 1).to_string());
    }
    output.push('\n');

    let number_width = line_number.to_string().len().max(1);
    output.push_str(&format!(" {:>number_width$} │ ", ""));
    output.push('\n');
    output.push_str(&format!(" {:>number_width$} │ ", line_number));
    output.push_str(&excerpt);
    output.push('\n');

    if checked.is_some() {
        output.push_str(&format!(" {:>number_width$} │ ", ""));
        output.push_str(&" ".repeat(column));
        output.push('^');
        if width > 1 {
            output.push_str(&"~".repeat(width.saturating_sub(1).min(MAX_EXCERPT_COLUMNS)));
        }
        if right_ellipsis && column + width >= MAX_EXCERPT_COLUMNS {
            output.push('…');
        }
        output.push('\n');
    }
    true
}

fn line_text(source: &str, requested: usize) -> Option<String> {
    if requested == 0 {
        return None;
    }
    let mut lines = source.split('\n');
    let mut line = lines.nth(requested - 1)?;
    if let Some(without_cr) = line.strip_suffix('\r') {
        line = without_cr;
    }
    Some(line.to_owned())
}

const MAX_EXCERPT_COLUMNS: usize = 120;
const LEFT_FOCUS_COLUMNS: usize = 36;

fn excerpt(line: &str, focus_byte: Option<usize>) -> (String, usize, bool, bool) {
    let total_width = display_width(line);
    let focus_column = focus_byte
        .and_then(|byte| line.get(..byte))
        .map(display_width)
        .unwrap_or(0);
    let left = if total_width > MAX_EXCERPT_COLUMNS {
        focus_column.saturating_sub(LEFT_FOCUS_COLUMNS)
    } else {
        0
    };
    let right = left.saturating_add(MAX_EXCERPT_COLUMNS);
    let mut column = 0usize;
    let mut rendered = String::new();
    let left_ellipsis = left > 0;
    let mut right_ellipsis = false;
    for ch in line.chars() {
        let (visible, width) = visible_char(ch, column);
        let char_start = column;
        let char_end = column.saturating_add(width);
        column = char_end;
        if char_end <= left || char_start >= right {
            if char_start >= right {
                right_ellipsis = true;
                break;
            }
            continue;
        }
        let overlap_start = char_start.max(left);
        let overlap_end = char_end.min(right);
        if width == 0 {
            rendered.push_str(&visible);
        } else if visible == "\t" {
            rendered.push_str(&" ".repeat(overlap_end.saturating_sub(overlap_start)));
        } else if overlap_start != char_start || overlap_end != char_end {
            rendered.push(' ');
        } else {
            rendered.push_str(&visible);
        }
        if char_end > right {
            right_ellipsis = true;
            break;
        }
    }
    if column < total_width {
        right_ellipsis = true;
    }
    if left_ellipsis {
        rendered.insert(0, '…');
    }
    if right_ellipsis {
        rendered.push('…');
    }
    if rendered.is_empty() && line.is_empty() {
        rendered.push(' ');
    }
    (rendered, left, left_ellipsis, right_ellipsis)
}

fn display_width(text: &str) -> usize {
    let mut column = 0usize;
    for ch in text.chars() {
        column = column.saturating_add(visible_char(ch, column).1);
    }
    column
}

fn visible_char(ch: char, column: usize) -> (String, usize) {
    if ch == '\t' {
        let width = 4 - (column % 4);
        return ("\t".to_owned(), width);
    }
    if is_invisible_format(ch) {
        let code = ch as u32;
        let value = format!("\\u{{{code:x}}}");
        let width = value.len();
        return (value, width);
    }
    if ch.is_control() {
        let code = ch as u32;
        let value = format!("\\u{{{code:x}}}");
        let width = value.len();
        return (value, width);
    }
    (ch.to_string(), char_width(ch))
}

fn char_width(ch: char) -> usize {
    let value = ch as u32;
    if is_combining(value) {
        0
    } else if is_wide(value) {
        2
    } else {
        1
    }
}

fn is_invisible_format(ch: char) -> bool {
    matches!(ch as u32,
        0x061c | 0x200e..=0x200f | 0x2028..=0x202e | 0x2060..=0x2069 | 0xfeff
    )
}

fn is_combining(value: u32) -> bool {
    matches!(value,
        0x0300..=0x036f | 0x0483..=0x0489 | 0x0591..=0x05bd | 0x05bf |
        0x05c1..=0x05c2 | 0x05c4..=0x05c5 | 0x0610..=0x061a | 0x064b..=0x065f |
        0x0670 | 0x06d6..=0x06ed | 0x0711 | 0x0730..=0x074a | 0x07a6..=0x07b0 |
        0x0816..=0x082d | 0x0859..=0x085b | 0x08d3..=0x0903 | 0x093a..=0x093c |
        0x093e..=0x094f | 0x0951..=0x0957 | 0x0962..=0x0963 | 0x1ab0..=0x1aff |
        0x1dc0..=0x1dff | 0x20d0..=0x20ff | 0xfe00..=0xfe0f | 0xfe20..=0xfe2f |
        0xe0100..=0xe01ef
    )
}

fn is_wide(value: u32) -> bool {
    matches!(value,
        0x1100..=0x115f | 0x231a..=0x231b | 0x2329..=0x232a | 0x23e9..=0x23ec |
        0x23f0 | 0x23f3 | 0x25fd..=0x25fe | 0x2614..=0x2615 | 0x2648..=0x2653 |
        0x267f | 0x2693 | 0x26a1 | 0x26aa..=0x26ab | 0x26bd..=0x26be |
        0x26c4..=0x26c5 | 0x26ce | 0x26d4 | 0x26ea | 0x26f2..=0x26f3 | 0x26f5 |
        0x26fa | 0x26fd | 0x2705 | 0x270a..=0x270b | 0x2728 | 0x274c | 0x274e |
        0x2753..=0x2755 | 0x2757 | 0x2795..=0x2797 | 0x27b0 | 0x27bf |
        0x2b1b..=0x2b1c | 0x2b50 | 0x2b55 | 0x2e80..=0x303e | 0x3040..=0xa4cf |
        0xac00..=0xd7a3 | 0xf900..=0xfaff | 0xfe10..=0xfe19 | 0xfe30..=0xfe6f |
        0xff00..=0xff60 | 0xffe0..=0xffe6 | 0x1f000..=0x1faff | 0x20000..=0x3fffd
    )
}

fn escape_controls_limited(text: &str, max_chars: usize) -> String {
    let mut result = String::with_capacity(text.len());
    let mut emitted = 0usize;
    for ch in text.chars() {
        if emitted >= max_chars {
            result.push('…');
            return result;
        }
        if ch.is_control() || is_invisible_format(ch) {
            let escaped = format!("\\u{{{:x}}}", ch as u32);
            let remaining = max_chars - emitted;
            if escaped.chars().count() > remaining {
                result.extend(escaped.chars().take(remaining));
                result.push('…');
                return result;
            }
            emitted += escaped.chars().count();
            result.push_str(&escaped);
        } else {
            result.push(ch);
            emitted += 1;
        }
    }
    result
}
