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

use axiom_core::{Qty, Ratio};
use axiom_report::{Align, Cell, Column, Report, Row, Section, Style};

use crate::render::Locator;
use crate::style::{Color, Ink, Line, Terminal};
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
pub fn render(report: &Report, terminal: Terminal, locator: &mut Locator) -> String {
    let mut lines = vec![Line::text(&report.title, Ink::BOLD)];
    for section in &report.sections {
        lines.push(Line::new());
        lines.extend(section_lines(section, terminal.width, locator));
    }
    lines.iter().map(|line| line.render(terminal.painter) + "\n").collect()
}

fn section_lines(section: &Section, width: usize, locator: &mut Locator) -> Vec<Line> {
    let mut lines = Vec::new();
    if let Some(heading) = &section.heading {
        lines.push(Line::text(heading, Ink::BOLD));
    }
    if !section.columns.is_empty() {
        lines.extend(table_lines(section, locator));
    }
    let room = width.saturating_sub(INDENT + 2).max(MIN_NOTE_WIDTH);
    for note in &section.notes {
        for (at, part) in wrap(note, room).iter().enumerate() {
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
fn table_lines(section: &Section, locator: &mut Locator) -> Vec<Line> {
    let units = unit_widths(section);
    let titles: Vec<Line> = section.columns.iter().map(|column| Line::text(&column.title, Ink::DIM)).collect();
    let rows: Vec<Vec<Line>> =
        section.rows.iter().map(|row| row_cells(row, &section.columns, &units, locator)).collect();
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
fn row_cells(row: &Row, columns: &[Column], units: &[usize], locator: &mut Locator) -> Vec<Line> {
    let ink = match row.style {
        Style::Normal => Ink::PLAIN,
        Style::Total => Ink::BOLD,
        Style::Muted => Ink::DIM,
        Style::Alert => Ink::RED,
    };
    let mut cells: Vec<Line> = (0..columns.len())
        .map(|at| row.cells.get(at).map_or(Line::new(), |cell| cell_line(cell, ink, units[at], locator)))
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
        Some(Cell::Amount { unit, .. }) => unit.chars().count(),
        _ => 0,
    };
    (0..section.columns.len())
        .map(|at| section.rows.iter().map(|row| unit(row.cells.get(at))).max().unwrap_or(0))
        .collect()
}

fn cell_line(cell: &Cell, ink: Ink, unit_width: usize, locator: &mut Locator) -> Line {
    match cell {
        Cell::Blank => Line::new(),
        Cell::Text(text) => Line::text(text, ink),
        Cell::Day(day) => Line::text(&day.to_string(), ink),
        Cell::Percent(ratio) => Line::text(&percent(*ratio), ink),
        Cell::Amount { qty, scale, unit } => {
            let ink = if qty.is_negative() { ink.colored(Color::Red) } else { ink };
            let padding = unit_width.saturating_sub(unit.chars().count());
            Line::text(&format!("{} {unit}{}", qty.show(*scale), " ".repeat(padding)), ink)
        }
        Cell::Source(loc) => Line::text(&locator.describe(*loc).unwrap_or_default(), Ink::DIM),
    }
}

/// `12%`, `3.5%`, `0.25%`: to hundredths of a percent, without trailing zeros.
fn percent(ratio: Ratio) -> String {
    let Some(hundredths) = Qty(10_000).scale(ratio) else {
        return ratio.to_string();
    };
    let shown = hundredths.show(2).to_string();
    format!("{}%", shown.trim_end_matches('0').trim_end_matches('.'))
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use axiom_core::Day;

    use super::*;
    use crate::project::Sources;

    fn column(title: &'static str, align: Align) -> Column {
        Column { title: Cow::Borrowed(title), align }
    }

    fn row<'s>(depth: u8, style: Style, cells: Vec<Cell<'s>>) -> Row<'s> {
        Row { depth, style, cells }
    }

    fn amount(qty: i64, unit: &str) -> Cell<'_> {
        Cell::Amount { qty: Qty(qty), scale: 2, unit }
    }

    fn text(text: &'static str) -> Cell<'static> {
        Cell::Text(Cow::Borrowed(text))
    }

    #[test]
    fn a_balance_sheet() {
        let section = Section {
            heading: Some("Assets".to_string()),
            columns: vec![column("Place", Align::Left), column("Balance", Align::Right), column("Since", Align::Left)],
            rows: vec![
                row(0, Style::Normal, vec![text("assets/bank"), Cell::Blank, Cell::Blank]),
                row(
                    1,
                    Style::Normal,
                    vec![text("checking"), amount(792_130, "USD"), Cell::Day(Day::from_ymd(2026, 1, 15).unwrap())],
                ),
                row(1, Style::Normal, vec![text("brokerage"), amount(1_400, "VTI"), Cell::Blank]),
                row(1, Style::Alert, vec![text("visa"), amount(-12_345_600, "USD"), Cell::Blank]),
                row(0, Style::Total, vec![text("Total"), amount(793_530, "USD"), Cell::Blank]),
                row(
                    0,
                    Style::Muted,
                    vec![text("≈ gains"), amount(0, "USD"), Cell::Percent(Ratio::percent(35, 1).unwrap())],
                ),
            ],
            notes: vec!["VTI is not priced, so it is left out of the total.".to_string()],
        };
        let report = Report { title: "Balances at 2026-03-31".to_string(), sections: vec![section] };
        assert_eq!(
            render(&report, Terminal::plain(80), &mut Locator::new(&Sources::default())),
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
