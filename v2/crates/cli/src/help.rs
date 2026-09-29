//! The usage screen, drawn from the command and option tables.

use crate::args::{COMMANDS, OPTIONS, OptionSpec};
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
    let mut lines = vec![
        Line::text(&format!("axiom {}", env!("CARGO_PKG_VERSION")), Ink::BOLD),
        Line::text(TAGLINE, Ink::DIM),
        Line::new(),
    ];
    lines.push(Line::text("USAGE", Ink::BOLD));
    lines.push(Line::text("  axiom [OPTIONS] <COMMAND>", Ink::PLAIN));
    lines.push(Line::new());
    lines.push(Line::text("COMMANDS", Ink::BOLD));
    lines.extend(table(
        COMMANDS
            .iter()
            .map(|spec| {
                [
                    (spec.name.to_string(), Ink::CYAN.bold()),
                    (spec.operands.to_string(), Ink::DIM),
                    (spec.about.to_string(), Ink::PLAIN),
                ]
            })
            .collect(),
    ));
    lines.push(Line::new());
    lines.push(Line::text("COMMAND OPTIONS", Ink::BOLD));
    lines.extend(table(OPTIONS.iter().filter(|spec| !spec.global).map(option_row).collect()));
    lines.push(Line::new());
    lines.push(Line::text("GLOBAL OPTIONS", Ink::BOLD));
    lines.extend(table(OPTIONS.iter().filter(|spec| spec.global).map(option_row).collect()));
    lines.iter().map(|line| line.render(terminal.painter) + "\n").collect()
}

/// `-C, --project PATH`, what it does, and which commands take it.
fn option_row(spec: &OptionSpec) -> [(String, Ink); 3] {
    let short = spec.short.map_or_else(|| "    ".to_string(), |letter| format!("-{letter}, "));
    let value = spec.value.map_or_else(String::new, |name| format!(" {name}"));
    let owners: Vec<&str> =
        COMMANDS.iter().filter(|command| command.options.contains(&spec.opt)).map(|command| command.name).collect();
    [
        (format!("{short}--{}{value}", spec.long), Ink::CYAN.bold()),
        (spec.about.to_string(), Ink::PLAIN),
        (owners.join(", "), Ink::DIM),
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
