//! Machine-readable renderers for reports and diagnostics.
//!
//! The serializers consume the same typed report data as the terminal renderer;
//! they do not recompute balances or interpret view-specific text.

use std::fmt::Write as _;

use crate::{Align, Cell, Column, Fact, Report, ReportRenderer, Row, Section, SourceProvider, Style, When};
use axiom_core::{Days, Diagnostic, Loc, Qty, calendar::Window};
use axiom_model::{Closing, Period, Trigger};

/// The report renderer used by clients that want the stable JSON shape.
#[derive(Clone, Copy, Debug, Default)]
pub struct JsonRenderer;

impl ReportRenderer for JsonRenderer {
    type Output = String;

    fn render<'s>(&self, report: &Report<'s>, sources: &dyn SourceProvider) -> Self::Output {
        render(report, sources)
    }
}

/// One stable JSON document for a report.
pub fn render(report: &Report<'_>, sources: &dyn SourceProvider) -> String {
    let mut out = String::new();
    out.push_str("{\"title\":");
    plain_string(&mut out, &report.title, sources);
    out.push_str(",\"sections\":");
    array(&mut out, &report.sections, |out, section| write_section(out, section, sources));
    out.push_str(",\"facts\":");
    array(&mut out, report.sections.iter().flat_map(|section| &section.facts), write_fact);
    out.push_str("}\n");
    out
}

/// A JSON array of `items`, each written by `write`.
fn array<T>(out: &mut String, items: impl IntoIterator<Item = T>, mut write: impl FnMut(&mut String, T)) {
    out.push('[');
    for (index, item) in items.into_iter().enumerate() {
        comma(out, index);
        write(out, item);
    }
    out.push(']');
}

fn write_section(out: &mut String, section: &Section<'_>, sources: &dyn SourceProvider) {
    out.push_str("{\"heading\":");
    match &section.heading {
        Some(heading) => plain_string(out, heading, sources),
        None => out.push_str("null"),
    }
    out.push_str(",\"columns\":");
    array(out, &section.columns, |out, column| write_column(out, column, sources));
    out.push_str(",\"rows\":");
    array(out, &section.rows, |out, row| write_row(out, row, sources));
    out.push_str(",\"notes\":");
    array(out, &section.notes, |out, note| plain_string(out, note, sources));
    out.push('}');
}

fn write_column(out: &mut String, column: &Column<'_>, sources: &dyn SourceProvider) {
    out.push_str("{\"title\":");
    plain_string(out, &column.title, sources);
    out.push_str(",\"align\":");
    string(
        out,
        match column.align {
            Align::Left => "left",
            Align::Right => "right",
        },
    );
    out.push('}');
}

fn write_row(out: &mut String, row: &Row<'_>, sources: &dyn SourceProvider) {
    let _ = write!(out, "{{\"depth\":{},\"style\":", row.depth);
    string(
        out,
        match row.style {
            Style::Normal => "normal",
            Style::Total => "total",
            Style::Muted => "muted",
            Style::Alert => "alert",
        },
    );
    out.push_str(",\"cells\":");
    array(out, &row.cells, |out, cell| write_cell(out, cell, sources));
    out.push('}');
}

/// One JSON object per diagnostic, in input order, as required by `check --json`.
pub fn diagnostics(diagnostics: &[&Diagnostic], sources: &dyn SourceProvider) -> String {
    let mut out = String::new();
    for diagnostic in diagnostics {
        write_diagnostic(&mut out, diagnostic, sources);
        out.push('\n');
    }
    out
}

/// One diagnostic as a JSON object, without a line ending: for a client that puts diagnostics in a document of its own.
pub fn diagnostic(diagnostic: &Diagnostic, sources: &dyn SourceProvider) -> String {
    let mut out = String::new();
    write_diagnostic(&mut out, diagnostic, sources);
    out
}

fn write_diagnostic(out: &mut String, diagnostic: &Diagnostic, sources: &dyn SourceProvider) {
    out.push_str("{\"code\":");
    string(out, &diagnostic.code);
    out.push_str(",\"severity\":");
    string(
        out,
        match diagnostic.severity {
            axiom_core::Severity::Error => "error",
            axiom_core::Severity::Warning => "warning",
            axiom_core::Severity::Note => "note",
        },
    );
    out.push_str(",\"headline\":");
    string(out, diagnostic.message.lines().next().unwrap_or_default());
    out.push_str(",\"message\":");
    string(out, &diagnostic.message);
    out.push_str(",\"labels\":");
    array(out, &diagnostic.labels, |out, label| {
        out.push('{');
        location(out, label.loc, sources);
        out.push_str(",\"text\":");
        string(out, &label.text);
        let _ = write!(out, ",\"primary\":{}}}", label.primary);
    });
    out.push_str(",\"notes\":");
    array(out, &diagnostic.notes, |out, note| string(out, note));
    out.push_str(",\"helps\":");
    array(out, &diagnostic.help, |out, help| string(out, &help.text));
    out.push_str(",\"fixes\":");
    let edits = diagnostic.help.iter().filter_map(|help| help.edit.as_ref());
    array(out, edits, |out, (loc, replacement)| write_fix(out, *loc, replacement, sources));
    out.push('}');
}

/// A fix: where the text it replaces is, where that ends, and what to put there.
fn write_fix(out: &mut String, loc: Loc, replacement: &str, sources: &dyn SourceProvider) {
    out.push('{');
    location(out, loc, sources);
    // Edits use half-open ranges; the end position is the cursor just
    // after the replaced text, which stays on a UTF-8 boundary.
    let end = Loc { start: loc.end, end: loc.end, ..loc };
    let end_position = SourceProvider::describe(sources, end);
    out.push_str(",\"end_line\":");
    optional_number(out, end_position.map(|position| position.line));
    out.push_str(",\"end_column\":");
    optional_number(out, end_position.map(|position| position.column));
    out.push_str(",\"replacement\":");
    string(out, replacement);
    out.push('}');
}

fn write_cell(out: &mut String, cell: &Cell<'_>, sources: &dyn SourceProvider) {
    match cell {
        Cell::Blank => out.push_str("{\"type\":\"blank\"}"),
        Cell::Word(word) => text_cell(out, "word", word),
        Cell::Text(text) | Cell::Said(text) => text_cell(out, "text", text),
        Cell::Name(name) => text_cell(out, "name", name),
        Cell::Code(code) => text_cell(out, "code", code),
        Cell::Purpose(purpose) => text_cell(out, "purpose", purpose),
        Cell::Amount { qty, scale, unit } => {
            start_cell(out, "amount", |value| write!(value, "{} {unit}", qty.show(*scale)));
            out.push_str(",\"unit\":");
            string(out, unit);
            out.push('}');
        }
        Cell::Day(day) => shown_cell(out, "day", |value| write!(value, "{day}")),
        Cell::Span(span) => shown_cell(out, "span", |value| write!(value, "{span}")),
        Cell::Period(days) => {
            out.push_str("{\"type\":\"period\",\"value\":");
            write_period(out, *days);
            out.push('}');
        }
        Cell::Percent(ratio) => shown_cell(out, "percent", |value| write_percent(value, *ratio)),
        Cell::Number(ratio) => shown_cell(out, "number", |value| write!(value, "{ratio}")),
        Cell::Count(count, noun) => {
            out.push_str("{\"type\":\"count\",\"value\":");
            let _ = write!(out, "{count},\"noun\":");
            string(out, noun);
            out.push('}');
        }
        Cell::Trigger(trigger) => shown_cell(out, "trigger", |value| write_trigger_words(value, *trigger)),
        Cell::Source(loc) => {
            out.push_str("{\"type\":\"source\",");
            location(out, *loc, sources);
            out.push('}');
        }
        Cell::Join(between, parts) => {
            out.push_str("{\"type\":\"sentence\",\"separator\":");
            string(out, between);
            out.push_str(",\"parts\":");
            array(out, parts, |out, part| write_cell(out, part, sources));
            out.push('}');
        }
    }
}

/// `{"type":KIND,"value":TEXT}`.
fn text_cell(out: &mut String, kind: &str, text: &str) {
    out.push_str("{\"type\":");
    string(out, kind);
    out.push_str(",\"value\":");
    string(out, text);
    out.push('}');
}

/// `{"type":KIND,"value":"…"}`, the value being what `write` says, escaped.
fn shown_cell(out: &mut String, kind: &str, write: impl FnOnce(&mut Escaped<'_>) -> std::fmt::Result) {
    start_cell(out, kind, write);
    out.push('}');
}

/// `{"type":KIND,"value":"…"`: a cell whose value is what `write` says, escaped, and which goes on.
fn start_cell(out: &mut String, kind: &str, write: impl FnOnce(&mut Escaped<'_>) -> std::fmt::Result) {
    out.push_str("{\"type\":");
    string(out, kind);
    out.push_str(",\"value\":\"");
    let _ = write(&mut Escaped(out));
    out.push('"');
}

fn plain_string(out: &mut String, cell: &Cell<'_>, sources: &dyn SourceProvider) {
    out.push('"');
    let _ = write_plain(&mut Escaped(out), cell, sources);
    out.push('"');
}

/// Writes a cell's human-readable form directly into an escaped JSON string.
/// In particular, nested sentence parts never allocate a temporary `String`.
fn write_plain(out: &mut impl std::fmt::Write, cell: &Cell<'_>, sources: &dyn SourceProvider) -> std::fmt::Result {
    match cell {
        Cell::Blank => Ok(()),
        Cell::Word(word) => out.write_str(word),
        Cell::Text(text) | Cell::Said(text) => out.write_str(text),
        Cell::Name(name) => out.write_str(name),
        Cell::Code(code) => write!(out, "^{code}"),
        Cell::Purpose(purpose) => write!(out, "#{purpose}"),
        Cell::Amount { qty, scale, unit } => write!(out, "{} {unit}", qty.show(*scale)),
        Cell::Day(day) => write!(out, "{day}"),
        Cell::Span(span) => write!(out, "{span}"),
        Cell::Period(days) => write_period_words(out, *days),
        Cell::Percent(ratio) => write_percent(out, *ratio),
        Cell::Number(ratio) => write!(out, "{ratio}"),
        Cell::Count(count, noun) if noun.is_empty() => {
            write!(out, "{}", Qty(*count as i64).show(0))
        }
        Cell::Count(count, noun) => {
            write!(out, "{count} {noun}{}", if *count == 1 { "" } else { "s" })
        }
        Cell::Trigger(trigger) => write_trigger_words(out, *trigger),
        Cell::Source(loc) => {
            if let Some(position) = SourceProvider::describe(sources, *loc) {
                write!(out, "{}:{}", position.path, position.line)
            } else {
                Ok(())
            }
        }
        Cell::Join(separator, parts) => {
            let mut any = false;
            for part in parts.iter().filter(|part| plain_visible(part, sources)) {
                if any && !(*separator == " " && starts_with_punctuation(part, sources)) {
                    out.write_str(separator)?;
                }
                write_plain(out, part, sources)?;
                any = true;
            }
            Ok(())
        }
    }
}

fn plain_visible(cell: &Cell<'_>, sources: &dyn SourceProvider) -> bool {
    match cell {
        Cell::Blank => false,
        Cell::Text(text) | Cell::Said(text) => !text.is_empty(),
        Cell::Word(text) | Cell::Name(text) | Cell::Purpose(text) => !text.is_empty(),
        Cell::Source(loc) => SourceProvider::describe(sources, *loc).is_some_and(|pos| !pos.path.is_empty()),
        Cell::Join(_, parts) => parts.iter().any(|part| plain_visible(part, sources)),
        _ => true,
    }
}

fn starts_with_punctuation(cell: &Cell<'_>, sources: &dyn SourceProvider) -> bool {
    let first = match cell {
        Cell::Text(text) | Cell::Said(text) => text.chars().next(),
        Cell::Word(text) | Cell::Name(text) => text.chars().next(),
        Cell::Purpose(text) => text.chars().next().or(Some('#')),
        Cell::Join(_, parts) => {
            return parts
                .iter()
                .find(|part| plain_visible(part, sources))
                .is_some_and(|part| starts_with_punctuation(part, sources));
        }
        Cell::Source(loc) => SourceProvider::describe(sources, *loc).and_then(|pos| pos.path.chars().next()),
        _ => None,
    };
    first.is_some_and(|ch| matches!(ch, ',' | ';' | ':' | '.' | ')'))
}

struct Escaped<'a>(&'a mut String);

impl std::fmt::Write for Escaped<'_> {
    fn write_str(&mut self, text: &str) -> std::fmt::Result {
        escaped(self.0, text);
        Ok(())
    }
}

fn write_fact(out: &mut String, fact: &Fact<'_>) {
    out.push('{');
    out.push_str("\"concept\":");
    string(out, fact.concept);
    out.push_str(",\"of\":");
    optional_string(out, fact.of);
    out.push_str(",\"entity\":");
    string(out, fact.entity);
    out.push_str(",\"period\":");
    match fact.when {
        When::Instant(day) => {
            out.push_str("{\"instant\":");
            out.push('"');
            let _ = write!(Escaped(out), "{day}");
            out.push_str("\"}");
        }
        When::During(days) => write_period(out, days),
    }
    out.push_str(",\"unit\":");
    string(out, fact.value.unit);
    out.push_str(",\"value\":");
    digits(out, fact.value.qty, fact.value.scale);
    out.push('}');
}

fn digits(out: &mut String, qty: Qty, scale: u8) {
    out.push('"');
    let mut escaped = Escaped(out);
    let mut ungrouped = WithoutCommas(&mut escaped);
    let _ = write!(ungrouped, "{}", qty.show(scale));
    out.push('"');
}

fn write_period(out: &mut String, days: Days) {
    let end = |day: axiom_core::Day| {
        if day == axiom_core::Day::MIN || day == axiom_core::Day::MAX { None } else { Some(day) }
    };
    out.push('{');
    out.push_str("\"from\":");
    optional_day(out, end(days.first()));
    out.push_str(",\"to\":");
    optional_day(out, end(days.last()));
    out.push('}');
}

fn optional_day(out: &mut String, day: Option<axiom_core::Day>) {
    match day {
        Some(day) => {
            out.push('"');
            let _ = write!(Escaped(out), "{day}");
            out.push('"');
        }
        None => out.push_str("null"),
    }
}

fn write_period_words(out: &mut impl std::fmt::Write, days: Days) -> std::fmt::Result {
    match (Window::exactly(days), days.single()) {
        (Some(window), _) => write!(out, "{window}"),
        (None, Some(day)) => write!(out, "on {day}"),
        (None, None) if days == Days::ALWAYS => out.write_str("ever"),
        (None, None) => write!(out, "{}..{}", days.first(), days.last()),
    }
}

fn write_trigger_words(out: &mut impl std::fmt::Write, trigger: Trigger) -> std::fmt::Result {
    match trigger {
        Trigger::In => out.write_str("on in"),
        Trigger::Out => out.write_str("on out"),
        Trigger::Gain => out.write_str("on gain"),
        Trigger::Spend => out.write_str("on spend"),
        Trigger::Flow => out.write_str("on flow"),
        Trigger::Each(Period::Month, _) => out.write_str("each month"),
        Trigger::Each(Period::Year, None) => out.write_str("each year"),
        Trigger::Each(Period::Year, Some(Closing { month, day })) => {
            write!(out, "each year closing {month:02}-{day:02}")
        }
        Trigger::By(_) => out.write_str("by a date"),
        Trigger::Always => out.write_str("always"),
    }
}

fn write_percent(out: &mut impl std::fmt::Write, ratio: axiom_core::Ratio) -> std::fmt::Result {
    let Some(hundredths) = Qty(10_000).scale(ratio) else {
        return write!(out, "{ratio}");
    };
    let mut shown = StackText::new();
    write!(&mut shown, "{}", hundredths.show(2))?;
    while shown.len > 0 && shown.bytes[shown.len - 1] == b'0' {
        shown.len -= 1;
    }
    if shown.len > 0 && shown.bytes[shown.len - 1] == b'.' {
        shown.len -= 1;
    }
    out.write_str(shown.as_str())?;
    out.write_char('%')
}

struct StackText {
    bytes: [u8; 64],
    len: usize,
}

impl StackText {
    fn new() -> StackText {
        StackText { bytes: [0; 64], len: 0 }
    }

    fn as_str(&self) -> &str {
        std::str::from_utf8(&self.bytes[..self.len]).expect("formatted quantities are UTF-8")
    }
}

impl std::fmt::Write for StackText {
    fn write_str(&mut self, text: &str) -> std::fmt::Result {
        let Some(end) = self.len.checked_add(text.len()).filter(|&end| end <= self.bytes.len()) else {
            return Err(std::fmt::Error);
        };
        self.bytes[self.len..end].copy_from_slice(text.as_bytes());
        self.len = end;
        Ok(())
    }
}

struct WithoutCommas<W>(W);

impl<W: std::fmt::Write> std::fmt::Write for WithoutCommas<W> {
    fn write_str(&mut self, text: &str) -> std::fmt::Result {
        for part in text.split(',') {
            self.0.write_str(part)?;
        }
        Ok(())
    }
}

fn location(out: &mut String, loc: Loc, sources: &dyn SourceProvider) {
    let position = SourceProvider::describe(sources, loc);
    out.push_str("\"file\":");
    optional_string(out, position.map(|position| position.path));
    out.push_str(",\"line\":");
    optional_number(out, position.map(|position| position.line));
    out.push_str(",\"column\":");
    optional_number(out, position.map(|position| position.column));
    let _ = write!(out, ",\"file_id\":{},\"start_byte\":{},\"end_byte\":{}", loc.file.0, loc.start, loc.end);
}

fn optional_number(out: &mut String, number: Option<usize>) {
    match number {
        Some(number) => {
            let _ = write!(out, "{number}");
        }
        None => out.push_str("null"),
    }
}

fn optional_string(out: &mut String, text: Option<&str>) {
    match text {
        Some(text) => string(out, text),
        None => out.push_str("null"),
    }
}

fn comma(out: &mut String, index: usize) {
    if index > 0 {
        out.push(',');
    }
}

/// `text` as a JSON string, quotes included: the one escaper every JSON the program writes goes through.
pub fn string(out: &mut String, text: &str) {
    out.push('"');
    escaped(out, text);
    out.push('"');
}

fn escaped(out: &mut String, text: &str) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch <= '\u{1f}' => {
                let byte = ch as u8;
                out.push_str("\\u00");
                out.push(HEX[usize::from(byte >> 4)] as char);
                out.push(HEX[usize::from(byte & 0x0f)] as char);
            }
            ch => out.push(ch),
        }
    }
}

#[cfg(test)]
mod tests {
    use axiom_core::{Day, FileId, Loc, Qty, Ratio};

    use super::*;
    use crate::{Align, Column, Row, Section, SourcePosition};

    struct InMemorySources {
        path: String,
        text: String,
    }

    impl SourceProvider for InMemorySources {
        fn locate(&self, path: &str, line: usize) -> Option<Loc> {
            if path != self.path {
                return None;
            }
            let starts =
                std::iter::once(0).chain(self.text.match_indices('\n').map(|(at, _)| at + 1)).collect::<Vec<_>>();
            let start = *starts.get(line.checked_sub(1)?)?;
            let end = starts.get(line).copied().unwrap_or(self.text.len());
            Some(Loc::new(FileId(0), start as u32, end.min(self.text.len()) as u32))
        }

        fn describe(&self, loc: Loc) -> Option<SourcePosition<'_>> {
            let (start, end) = (loc.start as usize, loc.end as usize);
            if loc.file != FileId(0)
                || start > end
                || end > self.text.len()
                || !self.text.is_char_boundary(start)
                || !self.text.is_char_boundary(end)
            {
                return None;
            }
            Some({
                let offset = start;
                let line = self.text[..offset].bytes().filter(|byte| *byte == b'\n').count();
                let start = self.text[..offset].rfind('\n').map_or(0, |at| at + 1);
                let column =
                    self.text[start..].char_indices().take_while(|(relative, _)| start + *relative < offset).count()
                        + 1;
                SourcePosition { path: &self.path, line: line + 1, column }
            })
        }
    }

    #[test]
    fn report_json_preserves_typed_cells_and_escapes_text() {
        let sources = InMemorySources { path: "ledger.ax".to_string(), text: "α\tchecking\n".to_string() };
        let report = Report {
            title: Cell::text("Balance \"sheet\"\n🧾"),
            sections: vec![Section {
                heading: Some(Cell::Word("Assets")),
                columns: vec![
                    Column::left("Place"),
                    Column::right("Balance"),
                    Column { title: Cell::Word("Since"), align: Align::Left },
                    Column::left("Source"),
                ],
                rows: vec![Row::new([
                    Cell::text("Checking\\savings"),
                    Cell::Amount { qty: Qty(-123_450), scale: 2, unit: "U\"D" },
                    Cell::Day(Day::from_ymd(2026, 3, 4).unwrap()),
                    Cell::Source(Loc::new(FileId(0), 0, 2)),
                ])],
                notes: vec![Cell::text("line one\nline two\t✓")],
                facts: Vec::new(),
            }],
        };
        let json = render(&report, &sources);
        assert!(json.contains("Balance \\\"sheet\\\"\\n🧾"));
        assert!(json.contains("-1,234.50 U\\\"D"));
        assert!(json.contains("2026-03-04"));
        assert!(json.contains("line one\\nline two\\t✓"));
        assert!(json.contains("\"type\":\"amount\""));
        assert!(json.contains("\"file\":\"ledger.ax\",\"line\":1,\"column\":1"));
    }

    #[test]
    fn a_string_escapes_exactly_what_json_requires_and_keeps_the_rest() {
        let mut out = String::new();
        string(&mut out, "q\" b\\ \u{8}\u{c}\n\r\t \u{1}\u{1f} é🧾/");
        assert_eq!(out, "\"q\\\" b\\\\ \\b\\f\\n\\r\\t \\u0001\\u001f é🧾/\"");
    }

    #[test]
    fn json_source_and_empty_report_keep_stable_shapes() {
        let sources = InMemorySources { path: "journal/one.ax".to_string(), text: "α\nnext\n".to_string() };
        let pos = SourceProvider::describe(&sources, Loc::new(FileId(0), 3, 5)).unwrap();
        assert_eq!((pos.path, pos.line, pos.column), ("journal/one.ax", 2, 1));
        assert_eq!(
            SourceProvider::describe(&sources, Loc::new(FileId(0), 1, 2)),
            None,
            "offsets inside UTF-8 characters are rejected"
        );
        assert_eq!(
            SourceProvider::describe(&sources, Loc::new(FileId(0), 0, 99)),
            None,
            "out-of-bounds locations are rejected"
        );
        assert_eq!(SourceProvider::describe(&sources, Loc::new(FileId(0), 5, 4)), None, "reversed ranges are rejected");
        assert_eq!(
            render(&Report { title: Cell::text(""), sections: Vec::new() }, &sources),
            "{\"title\":\"\",\"sections\":[],\"facts\":[]}\n"
        );
        assert_eq!(crate::percent(Ratio::percent(25, 2).unwrap()), "0.25%");
    }

    #[test]
    fn diagnostics_keep_messages_notes_edits_and_exact_ranges() {
        let sources = InMemorySources { path: "journal/one.ax".to_string(), text: "a\tβc\nlast\n".to_string() };
        let loc = Loc::new(FileId(0), 2, 9);
        let diagnostic = Diagnostic::error("bad\"entry", "headline\nsecond line")
            .label(loc, "near \"beta\"")
            .note("keep\nline breaks")
            .help("replace it")
            .fix("use a name", loc, "owner");
        let output = diagnostics(&[&diagnostic], &sources);
        assert!(output.contains("\"headline\":\"headline\",\"message\":\"headline\\nsecond line\""));
        assert!(output.contains("\"file\":\"journal/one.ax\",\"line\":1,\"column\":3"));
        assert!(output.contains("\"start_byte\":2,\"end_byte\":9"));
        assert!(output.contains("\"end_line\":2,\"end_column\":4"));
        assert!(output.contains("\"replacement\":\"owner\""));
    }
}
