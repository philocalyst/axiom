//! The usage screen, drawn from the command and option tables.

use crate::args::{COMMANDS, CommandSpec, OPTIONS, OptionSpec, takers};
use crate::style::{Ink, Line, Terminal};

const TAGLINE: &str = "A typed plain-text ledger: what you own, what you owe, what you can spend.";

/// Columns between the columns of a table.
const GAP: usize = 2;

/// `axiom 0.3.0`
pub fn version() -> String {
    format!("axiom {}\n", env!("CARGO_PKG_VERSION"))
}

/// The usage screen: what `axiom help` prints.
pub fn screen(terminal: Terminal) -> String {
    let options = |chosen: bool| OPTIONS.iter().filter(move |spec| spec.global != chosen).map(option_row).collect();
    let sections = [
        ("COMMANDS", COMMANDS.iter().map(command_row).collect()),
        ("COMMAND OPTIONS", options(true)),
        ("GLOBAL OPTIONS", options(false)),
    ];
    let mut lines = vec![
        Line::text(&format!("axiom {}", env!("CARGO_PKG_VERSION")), Ink::BOLD),
        Line::text(TAGLINE, Ink::DIM),
        Line::new(),
        Line::text("USAGE", Ink::BOLD),
        Line::text("  axiom [OPTIONS] <COMMAND>", Ink::PLAIN),
    ];
    for (heading, rows) in sections {
        lines.extend([Line::new(), Line::text(heading, Ink::BOLD)]);
        lines.extend(table(rows));
    }
    terminal.painter.paint(&lines)
}

/// `balance  [GLOB…]  assets and debts`
fn command_row(spec: &CommandSpec) -> [(String, Ink); 3] {
    [(spec.name.to_string(), Ink::CYAN.bold()), (spec.operands.usage(), Ink::DIM), (spec.about.to_string(), Ink::PLAIN)]
}

/// `-C, --project PATH`, what it does, and which commands take it if not all.
fn option_row(spec: &OptionSpec) -> [(String, Ink); 3] {
    let short = spec.short.map_or_else(|| "    ".to_string(), |letter| format!("-{letter}, "));
    let value = spec.value.map_or_else(String::new, |name| format!(" {name}"));
    [
        (format!("{short}--{}{value}", spec.long), Ink::CYAN.bold()),
        (spec.about.to_string(), Ink::PLAIN),
        (takers(spec), Ink::DIM),
    ]
}

/// Rows of three cells, indented, each column as wide as its widest cell.
fn table(rows: Vec<[(String, Ink); 3]>) -> Vec<Line> {
    let widths: [usize; 3] =
        std::array::from_fn(|column| rows.iter().map(|row| row[column].0.chars().count()).max().unwrap_or(0));
    rows.iter()
        .map(|row| {
            let mut line = Line::text("  ", Ink::PLAIN);
            for ((text, ink), width) in row.iter().zip(widths) {
                let start = line.width();
                line.push(text, *ink);
                line.pad_to(start + width + GAP);
            }
            line
        })
        .collect()
}
