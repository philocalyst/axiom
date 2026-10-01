//! Reports as tables.
//!
//! ```text
//! Balances at 2026-03-31
//!
//! Assets
//!   Place                      Balance
//!   ──────────────────────────────────
//!   assets/bank/checking  7,921.30 USD
//!   assets/brokerage         14.00 VTI
//!   ──────────────────────────────────
//!   Total                 7,935.30 USD
//!   · VTI is not priced, so it is left out.
//! ```

use std::fmt::{self, Write as _};

use axiom_core::calendar::Window;
use axiom_core::{Days, Qty, Ratio};
use axiom_model::{Closing, Period, Trigger};
use axiom_report::{
    Align, Cell, Column, Report, ReportRenderer, Row, Section, SourceProvider, Style, percent,
};

use crate::style::{Ink, Line, Terminal};
use crate::text::wrap;

/// Columns before every table row.
const INDENT: usize = 2;
/// Columns between the columns of a table.
const GAP: usize = 2;
/// Columns a tree-shaped table indents each level.
const DEPTH: usize = 2;
/// A note's prose is wrapped to at least this many columns.
const MIN_NOTE_WIDTH: usize = 20;

/// Draws a report: its title, then each section with its table and notes.
pub fn render(report: &Report<'_>, terminal: Terminal, sources: &dyn SourceProvider) -> String {
    let mut output = String::new();
    let mut title = Line::new();
    write_cell(
        &mut StyledLine::new(&mut title, Ink::BOLD),
        &report.title,
        sources,
        Ink::BOLD,
        0,
    );
    terminal.painter.paint_line(&mut output, &title);
    for section in &report.sections {
        terminal.painter.paint_line(&mut output, &Line::new());
        write_section(&mut output, section, terminal, sources);
    }
    output
}

/// The terminal table renderer, configured with its output width and painter.
pub struct TableRenderer {
    pub terminal: Terminal,
}

impl ReportRenderer for TableRenderer {
    type Output = String;

    fn render<'s>(&self, report: &Report<'s>, sources: &dyn SourceProvider) -> Self::Output {
        render(report, self.terminal, sources)
    }
}

fn write_section(
    output: &mut String,
    section: &Section<'_>,
    terminal: Terminal,
    sources: &dyn SourceProvider,
) {
    if let Some(heading) = &section.heading {
        let mut line = Line::new();
        write_cell(
            &mut StyledLine::new(&mut line, Ink::BOLD),
            heading,
            sources,
            Ink::BOLD,
            0,
        );
        terminal.painter.paint_line(output, &line);
    }
    if !section.columns.is_empty() {
        write_table(output, section, terminal, sources);
    }
    let room = terminal
        .width
        .saturating_sub(INDENT + 2)
        .max(MIN_NOTE_WIDTH);
    for note in &section.notes {
        let text = cell_string(note, sources);
        for (at, part) in wrap(&text, room).iter().enumerate() {
            let mut line = Line::new();
            if at == 0 {
                line.put(INDENT, "·", Ink::DIM);
            }
            line.put(INDENT + 2, part, Ink::DIM);
            terminal.painter.paint_line(output, &line);
        }
    }
}

/// The column titles, a rule, and the rows, with a rule above each total.
fn write_table(
    output: &mut String,
    section: &Section<'_>,
    terminal: Terminal,
    sources: &dyn SourceProvider,
) {
    let units = unit_widths(section);
    let widths: Vec<usize> = (0..section.columns.len())
        .map(|at| {
            let title = measure_cell(&section.columns[at].title, sources, 0);
            section.rows.iter().fold(title, |width, row| {
                let cell = row.cells.get(at).unwrap_or(&Cell::Blank);
                let depth = if at == 0 {
                    DEPTH * usize::from(row.depth)
                } else {
                    0
                };
                width.max(measure_cell(cell, sources, units[at]) + depth)
            })
        })
        .collect();
    let table_width = widths.iter().sum::<usize>() + GAP * (widths.len() - 1);
    let titles = table_row(None, &section.columns, &widths, &units, sources);
    terminal.painter.paint_line(output, &titles);
    write_rule(output, terminal, table_width);
    for (at, row) in section.rows.iter().enumerate() {
        // A total without a label of its own continues the one above it (the
        // same total in another commodity), so it shares that total's rule.
        let continues = matches!(row.cells.first(), None | Some(Cell::Blank))
            && at
                .checked_sub(1)
                .is_some_and(|before| section.rows[before].style == Style::Total);
        if row.style == Style::Total && at > 0 && !continues {
            write_rule(output, terminal, table_width);
        }
        let line = table_row(Some(row), &section.columns, &widths, &units, sources);
        terminal.painter.paint_line(output, &line);
    }
}

fn write_rule(output: &mut String, terminal: Terminal, width: usize) {
    let mut rule = Line::with_capacity(INDENT + width);
    rule.push_repeat('─', width, Ink::DIM);
    rule.right_align(INDENT + width);
    terminal.painter.paint_line(output, &rule);
}

/// Measures and writes a row directly into one output line. The first pass
/// computes widths; this pass doesn't build a temporary line for every cell.
fn table_row(
    row: Option<&Row<'_>>,
    columns: &[Column<'_>],
    widths: &[usize],
    units: &[usize],
    sources: &dyn SourceProvider,
) -> Line {
    let style = row.map_or(Style::Normal, |row| row.style);
    let ink = match style {
        Style::Normal => Ink::PLAIN,
        Style::Total => Ink::BOLD,
        Style::Muted => Ink::DIM,
        Style::Alert => Ink::RED,
    };
    let capacity = INDENT + widths.iter().sum::<usize>() + GAP * columns.len();
    let mut line = Line::with_capacity(capacity);
    line.push_repeat(' ', INDENT, Ink::PLAIN);
    for (at, (column, &width)) in columns.iter().zip(widths).enumerate() {
        let cell = if let Some(row) = row {
            row.cells.get(at).unwrap_or(&Cell::Blank)
        } else {
            &column.title
        };
        let cell_ink = if row.is_none() { Ink::DIM } else { ink };
        let column_start = line.width();
        let depth = if at == 0 {
            row.map_or(0, |row| DEPTH * usize::from(row.depth))
        } else {
            0
        };
        line.push_repeat(' ', depth, Ink::PLAIN);
        write_cell(
            &mut StyledLine::new(&mut line, cell_ink),
            cell,
            sources,
            cell_ink,
            units[at],
        );
        let used = line.width() - column_start;
        let padding = width.saturating_sub(used);
        if column.align == Align::Right {
            line.insert_spaces(column_start, padding);
        } else {
            line.push_repeat(' ', padding, Ink::PLAIN);
        }
        line.push_repeat(' ', GAP, Ink::PLAIN);
    }
    line
}

/// The widest unit in each column. Amounts pad their unit to it, so that the
/// numbers of `12.00 USD` and `3.5 VTI` end in the same column.
fn unit_widths(section: &Section<'_>) -> Vec<usize> {
    let unit = |cell: Option<&Cell>| match cell {
        Some(Cell::Amount { unit, .. }) => unit.chars().count(),
        _ => 0,
    };
    (0..section.columns.len())
        .map(|at| {
            section
                .rows
                .iter()
                .map(|row| unit(row.cells.get(at)))
                .max()
                .unwrap_or(0)
        })
        .collect()
}

fn cell_string(cell: &Cell<'_>, sources: &dyn SourceProvider) -> String {
    let mut text = String::new();
    if let Cell::Amount { qty, scale, unit } = cell {
        let _ = write!(&mut text, "{} {unit}", qty.brief(*scale));
    } else {
        let _ = write_cell(&mut PlainText(&mut text), cell, sources, Ink::PLAIN, 0);
    }
    text
}

fn measure_cell(cell: &Cell<'_>, sources: &dyn SourceProvider, unit_width: usize) -> usize {
    let mut width = Width(0);
    let _ = write_cell(&mut width, cell, sources, Ink::PLAIN, unit_width);
    width.0
}

struct StyledLine<'a> {
    line: &'a mut Line,
    ink: Ink,
}

impl<'a> StyledLine<'a> {
    fn new(line: &'a mut Line, ink: Ink) -> StyledLine<'a> {
        StyledLine { line, ink }
    }
}

impl fmt::Write for StyledLine<'_> {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        self.line.push(text, self.ink);
        Ok(())
    }
}

struct PlainText<'a>(&'a mut String);

impl fmt::Write for PlainText<'_> {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        self.0.push_str(text);
        Ok(())
    }
}

#[derive(Default)]
struct Width(usize);

impl fmt::Write for Width {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        self.0 += text
            .chars()
            .map(|ch| if ch == '\t' { 4 } else { 1 })
            .sum::<usize>();
        Ok(())
    }
}

trait CellSink: fmt::Write {
    fn ink(&mut self, ink: Ink);
}

impl CellSink for StyledLine<'_> {
    fn ink(&mut self, ink: Ink) {
        self.ink = ink;
    }
}

impl CellSink for PlainText<'_> {
    fn ink(&mut self, _: Ink) {}
}

impl CellSink for Width {
    fn ink(&mut self, _: Ink) {}
}

fn write_cell<W: CellSink>(
    out: &mut W,
    cell: &Cell<'_>,
    sources: &dyn SourceProvider,
    ink: Ink,
    unit_width: usize,
) -> bool {
    out.ink(ink);
    match cell {
        Cell::Blank => false,
        Cell::Text(text) | Cell::Said(text) => {
            let _ = out.write_str(text);
            !text.is_empty()
        }
        Cell::Word(word) => {
            let _ = out.write_str(word);
            !word.is_empty()
        }
        Cell::Name(name) => {
            let _ = out.write_str(name);
            !name.is_empty()
        }
        Cell::Code(code) => {
            let _ = write!(out, "^{code}");
            true
        }
        Cell::Purpose(purpose) => {
            let _ = write!(out, "#{purpose}");
            true
        }
        Cell::Day(day) => {
            let _ = write!(out, "{day}");
            true
        }
        Cell::Span(span) => {
            let _ = write!(out, "{span}");
            true
        }
        Cell::Period(days) => {
            let _ = write_period(out, *days);
            true
        }
        Cell::Percent(ratio) => {
            let _ = write_percent(out, *ratio);
            true
        }
        Cell::Number(ratio) => {
            let _ = write!(out, "{ratio}");
            true
        }
        Cell::Count(count, noun) => {
            if noun.is_empty() {
                let _ = write!(out, "{}", Qty(*count as i64).show(0));
            } else {
                let _ = write!(
                    out,
                    "{} {noun}{}",
                    Qty(*count as i64).show(0),
                    if *count == 1 { "" } else { "s" }
                );
            }
            true
        }
        Cell::Trigger(trigger) => {
            let _ = write_trigger(out, *trigger);
            true
        }
        Cell::Amount { qty, scale, unit } => {
            let previous_ink = ink;
            if qty.is_negative() {
                out.ink(ink.colored(Ink::RED));
            }
            let _ = write!(out, "{} {unit}", qty.show(*scale));
            let padding = unit_width.saturating_sub(unit.chars().count());
            for _ in 0..padding {
                let _ = out.write_char(' ');
            }
            out.ink(previous_ink);
            true
        }
        Cell::Source(loc) => {
            let Some(position) = SourceProvider::describe(sources, *loc) else {
                return false;
            };
            out.ink(Ink::DIM);
            let _ = write!(out, "{}:{}", position.path, position.line);
            true
        }
        Cell::Join(separator, parts) => {
            let mut any = false;
            for part in parts {
                if !cell_visible(part, sources) {
                    continue;
                }
                if any && !(*separator == " " && starts_with_punctuation(part, sources)) {
                    let _ = out.write_str(separator);
                }
                any |= write_cell(out, part, sources, ink, unit_width);
            }
            any
        }
    }
}

fn cell_visible(cell: &Cell<'_>, sources: &dyn SourceProvider) -> bool {
    match cell {
        Cell::Blank => false,
        Cell::Text(text) | Cell::Said(text) => !text.is_empty(),
        Cell::Word(text) => !text.is_empty(),
        Cell::Name(text) => !text.is_empty(),
        Cell::Purpose(text) => !text.is_empty(),
        Cell::Source(loc) => {
            SourceProvider::describe(sources, *loc).is_some_and(|p| !p.path.is_empty())
        }
        Cell::Join(_, parts) => parts.iter().any(|part| cell_visible(part, sources)),
        _ => true,
    }
}

fn starts_with_punctuation(cell: &Cell<'_>, sources: &dyn SourceProvider) -> bool {
    let first = match cell {
        Cell::Text(text) | Cell::Said(text) => text.chars().next(),
        Cell::Word(text) => text.chars().next(),
        Cell::Name(text) => text.chars().next(),
        Cell::Purpose(text) => text.chars().next().or(Some('#')),
        Cell::Source(loc) => {
            SourceProvider::describe(sources, *loc).and_then(|p| p.path.chars().next())
        }
        Cell::Join(_, parts) => {
            return parts
                .iter()
                .find(|part| cell_visible(part, sources))
                .is_some_and(|part| starts_with_punctuation(part, sources));
        }
        _ => None,
    };
    first.is_some_and(|ch| matches!(ch, ',' | ';' | ':' | '.' | ')'))
}

fn write_period(out: &mut impl fmt::Write, days: Days) -> fmt::Result {
    match (Window::exactly(days), days.single()) {
        (Some(window), _) => write!(out, "{window}"),
        (None, Some(day)) => write!(out, "on {day}"),
        (None, None) if days == Days::ALWAYS => out.write_str("ever"),
        (None, None) => write!(out, "{}..{}", days.first(), days.last()),
    }
}

fn write_trigger(out: &mut impl fmt::Write, trigger: Trigger) -> fmt::Result {
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

/// Formats a percent through a small stack buffer, then writes the trimmed
/// digits into the renderer without allocating a temporary String.
fn write_percent(out: &mut impl fmt::Write, ratio: Ratio) -> fmt::Result {
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
        StackText {
            bytes: [0; 64],
            len: 0,
        }
    }

    fn as_str(&self) -> &str {
        std::str::from_utf8(&self.bytes[..self.len]).expect("formatted numbers are UTF-8")
    }
}

impl fmt::Write for StackText {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let Some(end) = self
            .len
            .checked_add(text.len())
            .filter(|&end| end <= self.bytes.len())
        else {
            return Err(fmt::Error);
        };
        self.bytes[self.len..end].copy_from_slice(text.as_bytes());
        self.len = end;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use axiom_core::{Day, Qty, Ratio};

    use super::*;
    use crate::project::Sources;

    fn column(title: &'static str, align: Align) -> Column<'static> {
        Column {
            title: Cell::Word(title),
            align,
        }
    }

    fn row<'s>(depth: u8, style: Style, cells: Vec<Cell<'s>>) -> Row<'s> {
        Row {
            depth,
            style,
            cells,
        }
    }

    fn amount(qty: i64, unit: &str) -> Cell<'_> {
        Cell::Amount {
            qty: Qty(qty),
            scale: 2,
            unit,
        }
    }

    fn text(text: &'static str) -> Cell<'static> {
        Cell::Text(Cow::Borrowed(text))
    }

    #[test]
    fn a_balance_sheet() {
        let section = Section {
            heading: Some(Cell::Word("Assets")),
            columns: vec![
                column("Place", Align::Left),
                column("Balance", Align::Right),
                column("Since", Align::Left),
            ],
            rows: vec![
                row(
                    0,
                    Style::Normal,
                    vec![text("assets/bank"), Cell::Blank, Cell::Blank],
                ),
                row(
                    1,
                    Style::Normal,
                    vec![
                        text("checking"),
                        amount(792_130, "USD"),
                        Cell::Day(Day::from_ymd(2026, 1, 15).unwrap()),
                    ],
                ),
                row(
                    1,
                    Style::Normal,
                    vec![text("brokerage"), amount(1_400, "VTI"), Cell::Blank],
                ),
                row(
                    1,
                    Style::Alert,
                    vec![text("visa"), amount(-12_345_600, "USD"), Cell::Blank],
                ),
                row(
                    0,
                    Style::Total,
                    vec![text("Total"), amount(793_530, "USD"), Cell::Blank],
                ),
                row(
                    0,
                    Style::Muted,
                    vec![
                        text("≈ gains"),
                        amount(0, "USD"),
                        Cell::Percent(Ratio::percent(35, 1).unwrap()),
                    ],
                ),
            ],
            notes: vec![Cell::text(
                "VTI is not priced, so it is left out of the total.",
            )],
            facts: Vec::new(),
        };
        let report = Report {
            title: Cell::Word("Balances at 2026-03-31"),
            sections: vec![section],
        };
        assert_eq!(
            render(&report, Terminal::plain(80), &Sources::default()),
            "\
Balances at 2026-03-31

Assets
  Place                Balance  Since
  ────────────────────────────────────────
  assets/bank
    checking      7,921.30 USD  2026-01-15
    brokerage        14.00 VTI
    visa       -123,456.00 USD
  ────────────────────────────────────────
  Total           7,935.30 USD
  ≈ gains             0.00 USD  3.5%
  · VTI is not priced, so it is left out of the total.
"
        );
    }

    #[test]
    fn structured_sentences_write_typed_parts_without_flattening_them_first() {
        let sentence = Cell::Join(
            " ",
            vec![
                Cell::Word("Income"),
                Cell::Join(
                    " ",
                    vec![
                        Cell::Amount {
                            qty: Qty(1_200),
                            scale: 2,
                            unit: "USD",
                        },
                        Cell::Said(Cow::Borrowed(",")),
                    ],
                ),
                Cell::Blank,
                Cell::Word("today"),
            ],
        );
        let report = Report {
            title: sentence,
            sections: Vec::new(),
        };

        assert_eq!(
            render(&report, Terminal::plain(80), &Sources::default()),
            "Income 12.00 USD, today\n"
        );
    }

    #[test]
    fn percents_drop_trailing_zeros() {
        let percent_of = |mantissa, scale| percent(Ratio::percent(mantissa, scale).unwrap());
        assert_eq!(
            [
                percent_of(10, 0),
                percent_of(35, 1),
                percent_of(25, 2),
                percent_of(0, 0),
                percent_of(1000, 0)
            ],
            ["10%", "3.5%", "0.25%", "0%", "1,000%"]
        );
    }
}
