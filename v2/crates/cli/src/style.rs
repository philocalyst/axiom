//! Colour, and the only code that knows what an ANSI escape looks like.
//!
//! Everything printed is first laid out as [`Line`]s: rows of cells that each
//! hold one character and one [`Ink`]. Columns are therefore exact whatever the
//! colours, and marks can be placed at any column, which is what underlines and
//! aligned tables need. A [`Painter`] turns a finished line into text, with
//! escapes or without.

use std::io::IsTerminal;

/// How many columns a tab takes, in source lines and in the marks under them.
pub const TAB_WIDTH: usize = 4;

/// The width to wrap at when the terminal's is unknown.
const DEFAULT_WIDTH: usize = 100;

/// Narrower than this and wrapping stops being readable; the terminal's claim
/// is ignored.
const MIN_WIDTH: usize = 40;

/// The colours in use. `Default` is the terminal's own.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Color {
    Default,
    Red,
    Green,
    Yellow,
    Blue,
    Cyan,
}

impl Color {
    fn code(self) -> Option<&'static str> {
        match self {
            Color::Default => None,
            Color::Red => Some("31"),
            Color::Green => Some("32"),
            Color::Yellow => Some("33"),
            // Bright, because plain blue is unreadable on many dark themes.
            Color::Blue => Some("94"),
            Color::Cyan => Some("36"),
        }
    }
}

/// How a run of text looks: a colour, and whether it is bold or dim.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ink {
    color: Color,
    bold: bool,
    dim: bool,
}

impl Ink {
    pub const PLAIN: Ink = Ink { color: Color::Default, bold: false, dim: false };
    pub const BOLD: Ink = Ink::PLAIN.bold();
    pub const DIM: Ink = Ink::PLAIN.dim();
    pub const RED: Ink = Ink::PLAIN.colored(Color::Red);
    pub const GREEN: Ink = Ink::PLAIN.colored(Color::Green);
    pub const YELLOW: Ink = Ink::PLAIN.colored(Color::Yellow);
    pub const BLUE: Ink = Ink::PLAIN.colored(Color::Blue);
    pub const CYAN: Ink = Ink::PLAIN.colored(Color::Cyan);

    /// The same ink, in bold.
    pub const fn bold(self) -> Ink {
        Ink { bold: true, ..self }
    }

    /// The same ink, dimmed.
    pub const fn dim(self) -> Ink {
        Ink { dim: true, ..self }
    }

    /// The same ink in another colour.
    pub const fn colored(self, color: Color) -> Ink {
        Ink { color, ..self }
    }
}

/// The `--color` choice.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColorChoice {
    Auto,
    Always,
    Never,
}

impl ColorChoice {
    /// `auto` colours a terminal unless `NO_COLOR` asks for none.
    pub fn enabled(self, is_terminal: bool, no_color: bool) -> bool {
        match self {
            ColorChoice::Auto => is_terminal && !no_color,
            ColorChoice::Always => true,
            ColorChoice::Never => false,
        }
    }
}

/// Turns styled lines into text.
#[derive(Clone, Copy, Debug)]
pub struct Painter {
    enabled: bool,
}

impl Painter {
    /// Appends `text` to `out`, in `ink` if colour is on.
    fn write(self, out: &mut String, ink: Ink, text: &str) {
        if !self.enabled || ink == Ink::PLAIN {
            out.push_str(text);
            return;
        }
        let codes = [ink.bold.then_some("1"), ink.dim.then_some("2"), ink.color.code()];
        out.push_str("\x1b[");
        out.push_str(&codes.into_iter().flatten().collect::<Vec<_>>().join(";"));
        out.push('m');
        out.push_str(text);
        out.push_str("\x1b[0m");
    }
}

/// Where output goes: whether it is coloured, and how wide it may be.
#[derive(Clone, Copy, Debug)]
pub struct Terminal {
    pub painter: Painter,
    pub width: usize,
}

impl Terminal {
    /// Looks at one output stream, `NO_COLOR`, and `COLUMNS`. A pipe or a file
    /// always gets the default width, so redirected output does not depend on
    /// the window it was made in.
    pub fn detect(choice: ColorChoice, stream: &impl IsTerminal) -> Terminal {
        let is_terminal = stream.is_terminal();
        let no_color = std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty());
        let columns = std::env::var("COLUMNS").ok().and_then(|value| value.parse::<usize>().ok());
        let width = columns.filter(|&columns| is_terminal && columns >= MIN_WIDTH).unwrap_or(DEFAULT_WIDTH);
        Terminal { painter: Painter { enabled: choice.enabled(is_terminal, no_color) }, width }
    }

    /// No colour, whatever the stream.
    #[cfg(test)]
    pub fn plain(width: usize) -> Terminal {
        Terminal { painter: Painter { enabled: false }, width }
    }

    /// Colour, whatever the stream.
    #[cfg(test)]
    pub fn colored(width: usize) -> Terminal {
        Terminal { painter: Painter { enabled: true }, width }
    }
}

#[derive(Clone, Copy)]
struct Cell {
    ch: char,
    ink: Ink,
}

/// One row of styled characters, one per column.
#[derive(Clone, Default)]
pub struct Line {
    cells: Vec<Cell>,
}

impl Line {
    /// An empty line.
    pub fn new() -> Line {
        Line::default()
    }

    /// A line of `text` in one ink.
    pub fn text(text: &str, ink: Ink) -> Line {
        let mut line = Line::new();
        line.push(text, ink);
        line
    }

    /// The number of columns.
    pub fn width(&self) -> usize {
        self.cells.len()
    }

    /// Appends `text`. A tab becomes [`TAB_WIDTH`] spaces and any other control
    /// character a space, so that one character is always one column, a stray
    /// newline cannot break a layout, and text from a file cannot smuggle escape
    /// sequences to the terminal.
    pub fn push(&mut self, text: &str, ink: Ink) {
        for ch in text.chars() {
            match ch {
                '\t' => self.cells.extend(std::iter::repeat_n(Cell { ch: ' ', ink }, TAB_WIDTH)),
                ch if ch.is_control() => self.cells.push(Cell { ch: ' ', ink }),
                ch => self.cells.push(Cell { ch, ink }),
            }
        }
    }

    /// Writes `text` starting at `column`, over whatever is there and padding
    /// with spaces if the line is shorter.
    pub fn put(&mut self, column: usize, text: &str, ink: Ink) {
        let written = Line::text(text, ink);
        self.pad_to(column + written.width());
        self.cells.splice(column..column + written.width(), written.cells);
    }

    /// Pads on the right with spaces to `width`.
    pub fn pad_to(&mut self, width: usize) {
        let padding = width.saturating_sub(self.width());
        self.cells.extend(std::iter::repeat_n(Cell { ch: ' ', ink: Ink::PLAIN }, padding));
    }

    /// Pads on the left with spaces to `width`.
    pub fn right_align(&mut self, width: usize) {
        let padding = width.saturating_sub(self.width());
        self.cells.splice(0..0, std::iter::repeat_n(Cell { ch: ' ', ink: Ink::PLAIN }, padding));
    }

    /// Adds `other` after the last column.
    pub fn append(&mut self, other: &Line) {
        self.cells.extend_from_slice(&other.cells);
    }

    /// The character at `column`, if there is one.
    pub fn glyph_at(&self, column: usize) -> Option<char> {
        self.cells.get(column).map(|cell| cell.ch)
    }

    /// The line as text, with no trailing spaces.
    pub fn render(&self, painter: Painter) -> String {
        let end = self.cells.iter().rposition(|cell| cell.ch != ' ').map_or(0, |last| last + 1);
        let mut out = String::new();
        for run in self.cells[..end].chunk_by(|a, b| a.ink == b.ink) {
            let text: String = run.iter().map(|cell| cell.ch).collect();
            painter.write(&mut out, run[0].ink, &text);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_only_where_ink_differs() {
        let painter = Painter { enabled: true };
        let mut line = Line::text("ab", Ink::RED.bold());
        line.push("  cd", Ink::PLAIN);
        assert_eq!(line.render(painter), "\x1b[1;31mab\x1b[0m  cd");
        assert_eq!(line.render(Terminal::plain(80).painter), "ab  cd");
    }

    #[test]
    fn put_overwrites_and_pads() {
        let mut line = Line::text("abcdef", Ink::PLAIN);
        line.put(2, "XY", Ink::PLAIN);
        line.put(8, "!", Ink::PLAIN);
        assert_eq!(line.render(Terminal::plain(80).painter), "abXYef  !");
    }

    #[test]
    fn tabs_and_control_characters_keep_columns_honest() {
        let line = Line::text("\ta\r\nb", Ink::PLAIN);
        assert_eq!(line.width(), TAB_WIDTH + 4);
    }

    #[test]
    fn colour_choice() {
        assert!(ColorChoice::Auto.enabled(true, false));
        assert!(!ColorChoice::Auto.enabled(true, true));
        assert!(!ColorChoice::Auto.enabled(false, false));
        assert!(ColorChoice::Always.enabled(false, true));
        assert!(!ColorChoice::Never.enabled(true, false));
    }
}
