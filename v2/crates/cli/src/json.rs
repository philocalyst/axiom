//! Reports and diagnostics as JSON, written by hand.
//!
//! The second renderer of the data a view builds: it walks the same sections,
//! rows and cells the text tables draw, and never computes anything. Keys are
//! snake_case and always present, amounts are strings with their unit, and days
//! are ISO dates.

use axiom_core::Days;
use axiom_core::diag::{Diagnostic, Disposition, Loc, Severity};
use axiom_report::{Align, Cell, Fact, Money, Report, Row, Section, Style, When};

use crate::project::Sources;
use crate::table::{percent, trigger_words};

/// A JSON value.
pub enum Json {
    Null,
    Bool(bool),
    Number(usize),
    Text(String),
    List(Vec<Json>),
    Object(Vec<(&'static str, Json)>),
}

impl Json {
    fn text(text: impl Into<String>) -> Json {
        Json::Text(text.into())
    }

    fn list<T>(items: impl IntoIterator<Item = T>, each: impl Fn(T) -> Json) -> Json {
        Json::List(items.into_iter().map(each).collect())
    }

    /// One compact line.
    pub fn line(&self) -> String {
        let mut out = String::new();
        self.write(&mut out);
        out.push('\n');
        out
    }

    fn write(&self, out: &mut String) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(yes) => out.push_str(if *yes { "true" } else { "false" }),
            Json::Number(number) => out.push_str(&number.to_string()),
            Json::Text(text) => quote(text, out),
            Json::List(items) => {
                out.push('[');
                for (at, item) in items.iter().enumerate() {
                    if at > 0 {
                        out.push(',');
                    }
                    item.write(out);
                }
                out.push(']');
            }
            Json::Object(fields) => {
                out.push('{');
                for (at, (key, value)) in fields.iter().enumerate() {
                    if at > 0 {
                        out.push(',');
                    }
                    quote(key, out);
                    out.push(':');
                    value.write(out);
                }
                out.push('}');
            }
        }
    }
}

/// `text` as a JSON string.
fn quote(text: &str, out: &mut String) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out.push('"');
}

// ─── Reports ────────────────────────────────────────────────────────────────

/// The whole report: its title, sections and facts, and how many errors the
/// book it rests on has.
pub fn report(report: &Report, sources: &Sources, errors: usize) -> Json {
    let facts = report.sections.iter().flat_map(|section| &section.facts);
    Json::Object(vec![
        ("title", cell(&report.title, sources)),
        ("sections", Json::list(&report.sections, |section| self::section(section, sources))),
        ("facts", Json::list(facts, fact)),
        ("errors", Json::Number(errors)),
    ])
}

fn section(section: &Section, sources: &Sources) -> Json {
    let column = |column: &axiom_report::Column| {
        let align = if column.align == Align::Left { "left" } else { "right" };
        Json::Object(vec![("title", cell(&column.title, sources)), ("align", Json::text(align))])
    };
    Json::Object(vec![
        ("heading", section.heading.map_or(Json::Null, Json::text)),
        ("columns", Json::list(&section.columns, column)),
        ("rows", Json::list(&section.rows, |row| self::row(row, sources))),
        ("notes", Json::list(&section.notes, |note| cell(note, sources))),
    ])
}

fn row(row: &Row, sources: &Sources) -> Json {
    let style = match row.style {
        Style::Normal => "normal",
        Style::Total => "total",
        Style::Muted => "muted",
        Style::Alert => "alert",
    };
    Json::Object(vec![
        ("style", Json::text(style)),
        ("depth", Json::Number(row.depth.into())),
        ("cells", Json::list(&row.cells, |one| cell(one, sources))),
    ])
}

/// Words and names are strings; every other kind of cell says which it is.
fn cell(one: &Cell, sources: &Sources) -> Json {
    let tagged = |key, value| Json::Object(vec![(key, value)]);
    match one {
        Cell::Blank => Json::Null,
        Cell::Word(word) => Json::text(*word),
        Cell::Text(text) | Cell::Name(text) => Json::text(*text),
        Cell::Said(said) => Json::text(said.as_str()),
        Cell::Code(code) => tagged("code", Json::text(*code)),
        Cell::Amount(money) => {
            Json::Object(vec![("amount", Json::text(digits(money))), ("unit", Json::text(money.unit))])
        }
        Cell::Day(day) => tagged("day", Json::text(day.to_string())),
        Cell::Span(span) => tagged("span", Json::text(span.to_string())),
        Cell::Period(days) => tagged("period", period(*days)),
        Cell::Percent(ratio) => tagged("percent", Json::text(percent(*ratio))),
        Cell::Number(ratio) => tagged("number", Json::text(ratio.to_string())),
        Cell::Count(count, noun) => Json::Object(vec![("count", Json::Number(*count)), ("noun", Json::text(*noun))]),
        Cell::Trigger(trigger) => tagged("trigger", Json::text(trigger_words(*trigger))),
        Cell::Source(loc) => tagged("source", sources.describe(*loc).map_or(Json::Null, Json::text)),
        Cell::Join(_, parts) => Json::list(parts, |part| cell(part, sources)),
    }
}

/// A quantity without thousands separators: `-7921.30`.
fn digits(money: &Money) -> String {
    money.qty.show(money.scale).to_string().replace(',', "")
}

/// The days of a period, or `null` for an end that is unbounded.
fn period(days: Days) -> Json {
    let end = |day: axiom_core::Day| {
        if day == axiom_core::Day::MIN || day == axiom_core::Day::MAX {
            Json::Null
        } else {
            Json::text(day.to_string())
        }
    };
    Json::Object(vec![("from", end(days.first())), ("to", end(days.last()))])
}

/// A fact in the shape of an XBRL one: what, of whom, when, in what unit, and its value.
fn fact(fact: &Fact) -> Json {
    let when = match fact.when {
        When::Instant(day) => Json::Object(vec![("instant", Json::text(day.to_string()))]),
        When::During(days) => period(days),
    };
    Json::Object(vec![
        ("concept", Json::text(fact.concept)),
        ("of", fact.of.map_or(Json::Null, Json::text)),
        ("entity", Json::text(fact.entity)),
        ("period", when),
        ("unit", Json::text(fact.value.unit)),
        ("value", Json::text(digits(&fact.value))),
    ])
}

// ─── Diagnostics ────────────────────────────────────────────────────────────

/// One diagnostic: its code, severity and headline, the labels that point at
/// its causes, notes, advice, and the edits that carry advice out.
pub fn diagnostic(diagnostic: &Diagnostic, sources: &Sources) -> Json {
    let severity = match diagnostic.severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Note => "note",
    };
    let disposition = match diagnostic.disposition {
        Disposition::Open => "open",
        Disposition::Priced => "priced",
        Disposition::Waived => "waived",
    };
    let label = |label: &axiom_core::diag::Label| {
        let mut fields = span(label.loc, sources);
        fields.extend([("text", Json::text(label.text.as_str())), ("primary", Json::Bool(label.primary))]);
        Json::Object(fields)
    };
    let fix = |help: &axiom_core::diag::Help| {
        let (loc, replacement) = help.edit.as_ref()?;
        let mut edit = span(*loc, sources);
        edit.push(("replacement", Json::text(replacement.as_str())));
        let edits = Json::List(vec![Json::Object(edit)]);
        Some(Json::Object(vec![("help", Json::text(help.text.as_str())), ("edits", edits)]))
    };
    Json::Object(vec![
        ("code", Json::text(diagnostic.code.as_ref())),
        ("severity", Json::text(severity)),
        ("disposition", Json::text(disposition)),
        ("headline", Json::text(diagnostic.message.lines().next().unwrap_or_default())),
        ("message", Json::text(diagnostic.message.as_str())),
        ("labels", Json::list(&diagnostic.labels, label)),
        ("notes", Json::list(&diagnostic.notes, |note| Json::text(note.as_str()))),
        ("helps", Json::list(&diagnostic.help, |help| Json::text(help.text.as_str()))),
        ("fixes", Json::List(diagnostic.help.iter().filter_map(fix).collect())),
    ])
}

/// Where a range of a source is, by file and by line and column (counted from
/// 1) of each end: the fields of a label, or of an edit.
fn span(loc: Loc, sources: &Sources) -> Vec<(&'static str, Json)> {
    let position = |offset: u32| sources.position(loc.file, offset).unwrap_or_default();
    let ((line, column), (end_line, end_column)) = (position(loc.start), position(loc.end));
    let file = sources.get(loc.file).map_or(Json::Null, |file| Json::text(&*file.path));
    vec![
        ("file", file),
        ("line", Json::Number(line)),
        ("column", Json::Number(column)),
        ("end_line", Json::Number(end_line)),
        ("end_column", Json::Number(end_column)),
    ]
}

#[cfg(test)]
mod tests {
    use axiom_core::{Day, Days, Qty, Span};

    use super::*;

    #[test]
    fn strings_are_escaped() {
        let text = Json::text("a \"quoted\" \\ line\nand\ttab\u{1}");
        assert_eq!(text.line(), "\"a \\\"quoted\\\" \\\\ line\\nand\\ttab\\u0001\"\n");
    }

    #[test]
    fn each_kind_of_cell_says_which_it_is() {
        let sources = Sources::default();
        let day = Day::from_ymd(2026, 3, 31).unwrap();
        let cells = [
            Cell::Blank,
            Cell::Word("Total"),
            Cell::Amount(Money { qty: Qty(-123_456_789), scale: 2, unit: "USD" }),
            Cell::Day(day),
            Cell::Period(Days::new(day, day).unwrap()),
            Cell::Period(Days::ALWAYS),
            Cell::Count(3, "flow"),
            Cell::Join(" ", vec![Cell::Word("late by"), Cell::Span(Span::days(40))]),
        ];
        let written: Vec<String> = cells.iter().map(|one| cell(one, &sources).line()).collect();
        assert_eq!(
            written.concat(),
            "null\n\"Total\"\n{\"amount\":\"-1234567.89\",\"unit\":\"USD\"}\n{\"day\":\"2026-03-31\"}\n\
             {\"period\":{\"from\":\"2026-03-31\",\"to\":\"2026-03-31\"}}\n{\"period\":{\"from\":null,\"to\":null}}\n\
             {\"count\":3,\"noun\":\"flow\"}\n[\"late by\",{\"span\":\"40d\"}]\n"
        );
    }
}
