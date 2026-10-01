//! The frame around a diagnostic: the gutter of line numbers, the corners that
//! open each file's panel, and the `= note:` lines under them.
//!
//! ```text
//!    ╭─[journal/2026/11.ax:4:3]
//!    │
//!  4 │   retirement   2_600 USD
//!    │
//!    ├─[us/401k.ax:12:11] (built in)
//!    │
//! 12 │   require total(in, year) <= limit[year]
//!    │
//!    = note: Elective deferrals are capped per calendar year.
//! ```

use super::snippet::{Gutter, Panel, Row};
use crate::style::{Ink, Line};
use crate::text::wrap;

/// Lines, numbers, and corners.
const FRAME: Ink = Ink::DIM;

/// Prose is wrapped to at least this many columns however deep it is indented.
const MIN_PROSE_WIDTH: usize = 20;

/// Geometry shared by every row of one diagnostic.
pub struct Page {
    /// Columns taken by the widest line number.
    pub gutter: usize,
    /// The terminal's width.
    pub width: usize,
}

impl Page {
    /// The column of every `│`, corner, and `=`: after the numbers and a space.
    fn spine(&self) -> usize {
        self.gutter + 1
    }

    /// `╭─[path:line:col]` for a diagnostic's first file, `├─[…]` for the rest;
    /// a source shipped with Axiom says so, since its reader cannot edit it.
    pub fn frame(&self, first: bool, panel: &Panel) -> Line {
        let mut line = Line::new();
        line.put(self.spine(), if first { "╭─[" } else { "├─[" }, FRAME);
        line.push(&format!("{}:{}:{}", panel.file.path, panel.lead.line, panel.lead.column), Ink::PLAIN);
        line.push("]", FRAME);
        if panel.file.embedded {
            line.push(" (built in)", Ink::DIM);
        }
        line
    }

    /// A row with its gutter: `12 │ code`, `   │ marks`, `   ⋮`, `12 - old` or
    /// `12 + new`.
    pub fn row(&self, row: &Row) -> Line {
        let (number, mark, ink) = match row.gutter {
            Gutter::Number(number) => (Some(number), "│", FRAME),
            Gutter::Added(number) => (Some(number), "+", Ink::GREEN.bold()),
            Gutter::Removed(number) => (Some(number), "-", Ink::RED.bold()),
            Gutter::Bar => (None, "│", FRAME),
            Gutter::Gap => (None, "⋮", FRAME),
        };
        let mut line = Line::new();
        if let Some(number) = number {
            let number = number.to_string();
            line.put(self.gutter.saturating_sub(number.len()), &number, FRAME);
        }
        line.put(self.spine(), mark, ink);
        line.pad_to(self.spine() + 2);
        line.append(&row.content);
        line
    }

    /// An empty `│` row, to give a panel room.
    pub fn bar(&self) -> Line {
        self.row(&Row { gutter: Gutter::Bar, content: Line::new() })
    }

    /// `= note: text`, wrapped, with continuation lines under the text.
    pub fn remark(&self, kind: &str, ink: Ink, text: &str) -> Vec<Line> {
        let label = format!("{kind}: ");
        let indent = self.spine() + 2 + label.len();
        let room = self.width.saturating_sub(indent).max(MIN_PROSE_WIDTH);
        wrap(text, room)
            .iter()
            .enumerate()
            .map(|(at, part)| {
                let mut line = Line::new();
                if at == 0 {
                    line.put(self.spine(), "=", FRAME);
                    line.put(self.spine() + 2, &label, ink);
                }
                line.put(indent, part, Ink::PLAIN);
                line
            })
            .collect()
    }
}
