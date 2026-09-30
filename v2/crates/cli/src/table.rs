//! Reports as text tables.
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
//!
//! This is one of the renderers of a report, which is data: every word a
//! reader sees of a typed cell is chosen here.

use axiom_core::calendar::Window;
use axiom_core::{Days, Qty, Ratio};
use axiom_model::{Closing, Period, Trigger};
use axiom_report::{Align, Cell, Column, Report, Row, Section, Style};

use crate::project::Sources;
use crate::style::{Ink, Line, Terminal};
use crate::text::{plural, wrap};

/// Columns before every table row.
const INDENT: usize = 2;
/// Columns between the columns of a table.
const GAP: usize = 2;
/// Columns a tree-shaped table indents each level.
const DEPTH: usize = 2;
/// A note's prose is wrapped to at least this many columns.
const MIN_NOTE_WIDTH: usize = 20;

/// Draws a report: its title, then each section with its table and notes.
pub fn render(report: &Report, terminal: Terminal, sources: &Sources) -> String {
    let mut lines = vec![Line::text(&text(&report.title, sources), Ink::BOLD)];
    for section in &report.sections {
        lines.push(Line::new());
        lines.extend(section_lines(section, terminal.width, sources));
    }
    terminal.painter.paint(&lines)
}

fn section_lines(section: &Section, width: usize, sources: &Sources) -> Vec<Line> {
    let mut lines = Vec::new();
    if let Some(heading) = section.heading {
        lines.push(Line::text(heading, Ink::BOLD));
    }
    if !section.columns.is_empty() {
        lines.extend(table_lines(section, sources));
    }
    let room = width.saturating_sub(INDENT + 2).max(MIN_NOTE_WIDTH);
    for note in &section.notes {
        for (at, part) in wrap(&text(note, sources), room).iter().enumerate() {
            let mut line = Line::new();
            if at == 0 {
                line.put(INDENT, "·", Ink::DIM);
            }
            line.put(INDENT + 2, part, Ink::DIM);
            lines.push(line);
        }
    }
    lines
}

/// The column titles, a rule, and the rows, with a rule above each total.
fn table_lines(section: &Section, sources: &Sources) -> Vec<Line> {
    let units = unit_widths(section);
    let titles: Vec<Line> =
        section.columns.iter().map(|column| Line::text(&text(&column.title, sources), Ink::DIM)).collect();
    let rows: Vec<Vec<Line>> =
        section.rows.iter().map(|row| row_cells(row, &section.columns, &units, sources)).collect();
    let widths: Vec<usize> = (0..section.columns.len())
        .map(|at| titles[at].width().max(rows.iter().map(|cells| cells[at].width()).max().unwrap_or(0)))
        .collect();
    let table_width = widths.iter().sum::<usize>() + GAP * (widths.len() - 1);
    let rule = || {
        let mut rule = Line::text(&"─".repeat(table_width), Ink::DIM);
        rule.right_align(INDENT + table_width);
        rule
    };

    let mut lines = vec![assemble(titles, &widths, &section.columns), rule()];
    for (at, (row, cells)) in section.rows.iter().zip(rows).enumerate() {
        // A total without a label of its own continues the one above it (the
        // same total in another commodity), so it shares that total's rule.
        let continues = matches!(row.cells.first(), None | Some(Cell::Blank))
            && at.checked_sub(1).is_some_and(|before| section.rows[before].style == Style::Total);
        if row.style == Style::Total && at > 0 && !continues {
            lines.push(rule());
        }
        lines.push(assemble(cells, &widths, &section.columns));
    }
    lines
}

/// Cells side by side, each padded to its column's width.
fn assemble(cells: Vec<Line>, widths: &[usize], columns: &[Column]) -> Line {
    let mut line = Line::text(&" ".repeat(INDENT), Ink::PLAIN);
    for ((mut cell, &width), column) in cells.into_iter().zip(widths).zip(columns) {
        match column.align {
            Align::Left => cell.pad_to(width),
            Align::Right => cell.right_align(width),
        }
        line.append(&cell);
        line.pad_to(line.width() + GAP);
    }
    line
}

/// One line per column: the row's cell, in the row's style, and for the first
/// column indented to the row's depth.
fn row_cells(row: &Row, columns: &[Column], units: &[usize], sources: &Sources) -> Vec<Line> {
    let ink = match row.style {
        Style::Normal => Ink::PLAIN,
        Style::Total => Ink::BOLD,
        Style::Muted => Ink::DIM,
        Style::Alert => Ink::RED,
    };
    let mut cells: Vec<Line> = (0..columns.len())
        .map(|at| row.cells.get(at).map_or(Line::new(), |cell| cell_line(cell, ink, units[at], sources)))
        .collect();
    if let Some(first) = cells.first_mut() {
        first.right_align(first.width() + DEPTH * usize::from(row.depth));
    }
    cells
}

/// The widest unit in each column. Amounts pad their unit to it, so that the
/// numbers of `12.00 USD` and `3.5 VTI` end in the same column.
fn unit_widths(section: &Section) -> Vec<usize> {
    let unit = |cell: Option<&Cell>| match cell {
        Some(Cell::Amount(money)) => money.unit.chars().count(),
        _ => 0,
    };
    (0..section.columns.len())
        .map(|at| section.rows.iter().map(|row| unit(row.cells.get(at))).max().unwrap_or(0))
        .collect()
}

fn cell_line(cell: &Cell, ink: Ink, unit_width: usize, sources: &Sources) -> Line {
    match cell {
        Cell::Amount(money) => {
            let ink = if money.qty.is_negative() { ink.colored(Ink::RED) } else { ink };
            let padding = unit_width.saturating_sub(money.unit.chars().count());
            Line::text(&format!("{} {}{}", money.qty.show(money.scale), money.unit, " ".repeat(padding)), ink)
        }
        Cell::Source(loc) => Line::text(&sources.describe(*loc).unwrap_or_default(), Ink::DIM),
        _ => Line::text(&text(cell, sources), ink),
    }
}

/// What a cell says, in words: an amount in a sentence drops the zeros a table keeps to line
/// up. Parts of a sentence are joined by spaces, except
/// that punctuation stays with the word before it.
pub fn text(cell: &Cell, sources: &Sources) -> String {
    match cell {
        Cell::Blank => String::new(),
        Cell::Word(word) => word.to_string(),
        Cell::Text(text) | Cell::Name(text) => text.to_string(),
        Cell::Said(text) => text.clone(),
        // v3 bridge: v4 writes codes `^inv-12`.
        Cell::Code(code) => format!("#{code}"),
        Cell::Amount(money) => format!("{} {}", money.qty.brief(money.scale), money.unit),
        Cell::Day(day) => day.to_string(),
        Cell::Span(span) => span.to_string(),
        Cell::Period(days) => period_words(*days),
        Cell::Percent(ratio) => percent(*ratio),
        Cell::Number(ratio) => ratio.to_string(),
        Cell::Count(count, "") => Qty(*count as i64).show(0).to_string(),
        Cell::Count(count, noun) => plural(*count, noun),
        Cell::Trigger(trigger) => trigger_words(*trigger),
        Cell::Source(loc) => sources.describe(*loc).unwrap_or_default(),
        Cell::Join(between, parts) => {
            let mut said = String::new();
            for part in parts.iter().map(|part| text(part, sources)).filter(|part| !part.is_empty()) {
                let attaches = *between == " " && part.starts_with([',', ';', ':', '.', ')']);
                if !said.is_empty() && !attaches {
                    said.push_str(between);
                }
                said.push_str(&part);
            }
            said
        }
    }
}

/// `2026-03`, `2026`, `on 2026-03-31`, `ever`, or the range itself.
pub fn period_words(days: Days) -> String {
    match (Window::exactly(days), days.single()) {
        (Some(window), _) => window.to_string(),
        (None, Some(day)) => format!("on {day}"),
        (None, None) if days == Days::ALWAYS => "ever".to_string(),
        (None, None) => format!("{}..{}", days.first(), days.last()),
    }
}

/// When a law fires, in the words it is written with.
pub fn trigger_words(trigger: Trigger) -> String {
    match trigger {
        Trigger::In => "on in".to_string(),
        Trigger::Out => "on out".to_string(),
        Trigger::Gain => "on gain".to_string(),
        Trigger::Spend => "on spend".to_string(),
        Trigger::Flow => "on flow".to_string(),
        Trigger::Each(Period::Month, _) => "each month".to_string(),
        Trigger::Each(Period::Year, None) => "each year".to_string(),
        Trigger::Each(Period::Year, Some(Closing { month, day })) => format!("each year closing {month:02}-{day:02}"),
        Trigger::By(_) => "by a date".to_string(),
        Trigger::Always => "always".to_string(),
    }
}

/// `12%`, `3.5%`, `0.25%`: to hundredths of a percent, without trailing zeros.
pub fn percent(ratio: Ratio) -> String {
    let Some(hundredths) = Qty(10_000).scale(ratio) else {
        return ratio.to_string();
    };
    let shown = hundredths.show(2).to_string();
    format!("{}%", shown.trim_end_matches('0').trim_end_matches('.'))
}

#[cfg(test)]
mod tests {
    use axiom_core::Day;
    use axiom_report::Money;

    use super::*;
    use crate::project::Sources;

    fn column(title: &'static str, align: Align) -> Column<'static> {
        Column { title: Cell::Word(title), align }
    }

    fn row<'s>(depth: u8, style: Style, cells: Vec<Cell<'s>>) -> Row<'s> {
        Row { depth, style, cells }
    }

    fn amount(qty: i64, unit: &str) -> Cell<'_> {
        Cell::Amount(Money { qty: Qty(qty), scale: 2, unit })
    }

    fn word(word: &'static str) -> Cell<'static> {
        Cell::Word(word)
    }

    #[test]
    fn a_balance_sheet() {
        let section = Section {
            heading: Some("Assets"),
            columns: vec![column("Place", Align::Left), column("Balance", Align::Right), column("Since", Align::Left)],
            rows: vec![
                row(0, Style::Normal, vec![word("assets/bank"), Cell::Blank, Cell::Blank]),
                row(
                    1,
                    Style::Normal,
                    vec![word("checking"), amount(792_130, "USD"), Cell::Day(Day::from_ymd(2026, 1, 15).unwrap())],
                ),
                row(1, Style::Normal, vec![word("brokerage"), amount(1_400, "VTI"), Cell::Blank]),
                row(1, Style::Alert, vec![word("visa"), amount(-12_345_600, "USD"), Cell::Blank]),
                row(0, Style::Total, vec![word("Total"), amount(793_530, "USD"), Cell::Blank]),
                row(
                    0,
                    Style::Muted,
                    vec![word("≈ gains"), amount(0, "USD"), Cell::Percent(Ratio::percent(35, 1).unwrap())],
                ),
            ],
            notes: vec![word("VTI is not priced, so it is left out of the total.")],
            facts: Vec::new(),
        };
        let report = Report { title: word("Balances at 2026-03-31"), sections: vec![section] };
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
    fn percents_drop_trailing_zeros() {
        let percent_of = |mantissa, scale| percent(Ratio::percent(mantissa, scale).unwrap());
        assert_eq!(
            [percent_of(10, 0), percent_of(35, 1), percent_of(25, 2), percent_of(0, 0), percent_of(1000, 0)],
            ["10%", "3.5%", "0.25%", "0%", "1,000%"]
        );
    }
}
