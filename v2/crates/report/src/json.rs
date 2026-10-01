//! Machine-readable renderers for reports and diagnostics.
//!
//! The serializers consume the same typed report data as the terminal renderer;
//! they do not recompute balances or interpret view-specific text.

use std::fmt::Write as _;

use axiom_core::{Diagnostic, Loc};
use crate::{Cell, Report, ReportRenderer, SourceProvider};

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
    string(&mut out, &report.title);
    out.push_str(",\"sections\":[");
    for (section_index, section) in report.sections.iter().enumerate() {
        comma(&mut out, section_index);
        out.push_str("{\"heading\":");
        optional_string(&mut out, section.heading.as_deref());
        out.push_str(",\"columns\":[");
        for (column_index, column) in section.columns.iter().enumerate() {
            comma(&mut out, column_index);
            out.push_str("{\"title\":");
            string(&mut out, &column.title);
            out.push_str(",\"align\":");
            string(&mut out, match column.align { crate::Align::Left => "left", crate::Align::Right => "right" });
            out.push('}');
        }
        out.push_str("],\"rows\":[");
        for (row_index, row) in section.rows.iter().enumerate() {
            comma(&mut out, row_index);
            let _ = write!(out, "{{\"depth\":{},\"style\":", row.depth);
            string(&mut out, match row.style {
                crate::Style::Normal => "normal",
                crate::Style::Total => "total",
                crate::Style::Muted => "muted",
                crate::Style::Alert => "alert",
            });
            out.push_str(",\"cells\":[");
            for (cell_index, cell) in row.cells.iter().enumerate() {
                comma(&mut out, cell_index);
                write_cell(&mut out, cell, sources);
            }
            out.push_str("]}");
        }
        out.push_str("],\"notes\":[");
        for (note_index, note) in section.notes.iter().enumerate() {
            comma(&mut out, note_index);
            string(&mut out, note);
        }
        out.push_str("]}");
    }
    out.push_str("]}\n");
    out
}

/// One JSON object per diagnostic, in input order, as required by `check --json`.
pub fn diagnostics(diagnostics: &[&Diagnostic], sources: &dyn SourceProvider) -> String {
    let mut out = String::new();
    for diagnostic in diagnostics {
        out.push('{');
        out.push_str("\"code\":");
        string(&mut out, &diagnostic.code);
        out.push_str(",\"severity\":");
        string(
            &mut out,
            match diagnostic.severity {
                axiom_core::Severity::Error => "error",
                axiom_core::Severity::Warning => "warning",
                axiom_core::Severity::Note => "note",
            },
        );
        out.push_str(",\"headline\":");
        string(&mut out, diagnostic.message.lines().next().unwrap_or_default());
        out.push_str(",\"message\":");
        string(&mut out, &diagnostic.message);
        out.push_str(",\"labels\":[");
        for (index, label) in diagnostic.labels.iter().enumerate() {
            comma(&mut out, index);
            out.push('{');
            location(&mut out, label.loc, sources);
            out.push_str(",\"text\":");
            string(&mut out, &label.text);
            let _ = write!(out, ",\"primary\":{}}}", label.primary);
        }
        out.push_str("],\"notes\":[");
        for (index, note) in diagnostic.notes.iter().enumerate() {
            comma(&mut out, index);
            string(&mut out, note);
        }
        out.push_str("],\"helps\":[");
        for (index, help) in diagnostic.help.iter().enumerate() {
            comma(&mut out, index);
            string(&mut out, &help.text);
        }
        out.push_str("],\"fixes\":[");
        let mut fix_index = 0;
        for help in &diagnostic.help {
            let Some((loc, replacement)) = &help.edit else { continue };
            comma(&mut out, fix_index);
            fix_index += 1;
            out.push('{');
            location(&mut out, *loc, sources);
            // Edits use half-open ranges; the end position is the cursor just
            // after the replaced text, which stays on a UTF-8 boundary.
            let end = Loc { start: loc.end, end: loc.end, ..*loc };
            let end_position = SourceProvider::describe(sources, end);
            out.push_str(",\"end_line\":");
            optional_number(&mut out, end_position.map(|position| position.line));
            out.push_str(",\"end_column\":");
            optional_number(&mut out, end_position.map(|position| position.column));
            out.push_str(",\"replacement\":");
            string(&mut out, replacement);
            out.push('}');
        }
        out.push_str("]}\n");
    }
    out
}

fn write_cell(out: &mut String, cell: &Cell<'_>, sources: &dyn SourceProvider) {
    match cell {
        Cell::Blank => out.push_str("{\"type\":\"blank\"}"),
        Cell::Text(text) => {
            out.push_str("{\"type\":\"text\",\"value\":");
            string(out, text);
            out.push('}');
        }
        Cell::Amount { qty, scale, unit } => {
            out.push_str("{\"type\":\"amount\",\"value\":");
            out.push('"');
            let _ = write!(out, "{} ", qty.show(*scale));
            escaped(out, unit);
            out.push('"');
            out.push_str(",\"unit\":");
            string(out, unit);
            out.push('}');
        }
        Cell::Day(day) => {
            out.push_str("{\"type\":\"day\",\"value\":");
            string(out, &day.to_string());
            out.push('}');
        }
        Cell::Percent(ratio) => {
            out.push_str("{\"type\":\"percent\",\"value\":");
            string(out, &crate::percent(*ratio));
            out.push('}');
        }
        Cell::Source(loc) => {
            out.push_str("{\"type\":\"source\",");
            location(out, *loc, sources);
            out.push('}');
        }
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
        Some(number) => { let _ = write!(out, "{number}"); }
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

fn string(out: &mut String, text: &str) {
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
            if path != self.path { return None; }
            let starts = std::iter::once(0)
                .chain(self.text.match_indices('\n').map(|(at, _)| at + 1))
                .collect::<Vec<_>>();
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
                let column = self.text[start..]
                    .char_indices()
                    .take_while(|(relative, _)| start + *relative < offset)
                    .count()
                    + 1;
                SourcePosition { path: &self.path, line: line + 1, column }
            })
        }
    }

    #[test]
    fn report_json_preserves_typed_cells_and_escapes_text() {
        let sources = InMemorySources { path: "ledger.ax".to_string(), text: "α\tchecking\n".to_string() };
        let report = Report {
            title: "Balance \"sheet\"\n🧾".to_string(),
            sections: vec![Section {
                heading: Some("Assets".to_string()),
                columns: vec![Column::left("Place"), Column::right("Balance"), Column { title: "Since".into(), align: Align::Left }, Column::left("Source")],
                rows: vec![Row::new([
                    Cell::text("Checking\\savings"),
                    Cell::Amount { qty: Qty(-123_450), scale: 2, unit: "U\"D" },
                    Cell::Day(Day::from_ymd(2026, 3, 4).unwrap()),
                    Cell::Source(Loc::new(FileId(0), 0, 2)),
                ])],
                notes: vec!["line one\nline two\t✓".to_string()],
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
    fn json_source_and_empty_report_keep_stable_shapes() {
        let sources = InMemorySources { path: "journal/one.ax".to_string(), text: "α\nnext\n".to_string() };
        let pos = SourceProvider::describe(&sources, Loc::new(FileId(0), 3, 5)).unwrap();
        assert_eq!((pos.path, pos.line, pos.column), ("journal/one.ax", 2, 1));
        assert_eq!(SourceProvider::describe(&sources, Loc::new(FileId(0), 1, 2)), None, "offsets inside UTF-8 characters are rejected");
        assert_eq!(SourceProvider::describe(&sources, Loc::new(FileId(0), 0, 99)), None, "out-of-bounds locations are rejected");
        assert_eq!(SourceProvider::describe(&sources, Loc::new(FileId(0), 5, 4)), None, "reversed ranges are rejected");
        assert_eq!(render(&Report { title: String::new(), sections: Vec::new() }, &sources), "{\"title\":\"\",\"sections\":[]}\n");
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
