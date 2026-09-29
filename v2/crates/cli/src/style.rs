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

/// What a tab is drawn as, in its first column: a tab that shows as spaces
/// cannot be told from the spaces it was mistaken for.
const TAB_MARK: char = '⇥';

/// The width to wrap at when the terminal's is unknown.
const DEFAULT_WIDTH: usize = 100;

/// Narrower than this and wrapping stops being readable; the terminal's claim
/// is ignored.
const MIN_WIDTH: usize = 40;

/// How a run of text looks: a colour (an SGR code), and whether it is bold or
/// dim.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ink {
    color: Option<u8>,
    bold: bool,
    dim: bool,
}

impl Ink {
    pub const PLAIN: Ink = Ink { color: None, bold: false, dim: false };
    pub const BOLD: Ink = Ink::PLAIN.bold();
    pub const DIM: Ink = Ink::PLAIN.dim();
    pub const RED: Ink = Ink::color(31);
    pub const GREEN: Ink = Ink::color(32);
    pub const YELLOW: Ink = Ink::color(33);
    /// Bright, because plain blue is unreadable on many dark themes.
    pub const BLUE: Ink = Ink::color(94);
    pub const CYAN: Ink = Ink::color(36);

    const fn color(code: u8) -> Ink {
        Ink { color: Some(code), ..Ink::PLAIN }
    }

    /// The same ink, in bold.
    pub const fn bold(self) -> Ink {
        Ink { bold: true, ..self }
    }

    /// The same ink, dimmed.
    pub const fn dim(self) -> Ink {
        Ink { dim: true, ..self }
    }

    /// The same ink in the colour of `other`.
    pub const fn colored(self, other: Ink) -> Ink {
        Ink { color: other.color, ..self }
    }
}

/// The `--color` choice.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColorChoice {
    Auto,
    Always,
    Never,
}

/// Turns styled lines into text.
#[derive(Clone, Copy, Debug)]
pub struct Painter {
    enabled: bool,
}

impl Painter {
    /// The lines as text, each ending in a newline.
    pub fn paint(self, lines: &[Line]) -> String {
        lines.iter().map(|line| line.render(self) + "\n").collect()
    }

    /// Appends `text` to `out`, in `ink` if colour is on.
    fn write(self, out: &mut String, ink: Ink, text: &str) {
        if !self.enabled || ink == Ink::PLAIN {
            out.push_str(text);
            return;
        }
        let codes = [ink.bold.then_some(1), ink.dim.then_some(2), ink.color].into_iter().flatten();
        let codes: Vec<String> = codes.map(|code| code.to_string()).collect();
        out.push_str(&format!("\x1b[{}m{text}\x1b[0m", codes.join(";")));
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
        let enabled = match choice {
            ColorChoice::Auto => is_terminal && !no_color,
            ColorChoice::Always => true,
            ColorChoice::Never => false,
        };
        Terminal { painter: Painter { enabled }, width }
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

/// One character per column: a tab is [`TAB_MARK`] and spaces up to
/// [`TAB_WIDTH`], and any other control character is a space. So a column is a
/// character whatever the text holds, a stray newline cannot break a layout,
/// and text from a file cannot smuggle escape sequences to the terminal.
fn columns(text: &str) -> impl Iterator<Item = char> + '_ {
    text.chars().flat_map(|ch| {
        let width = if ch == '\t' { TAB_WIDTH } else { 1 };
        (0..width).map(move |at| match ch {
            '\t' if at == 0 => TAB_MARK,
            ch if ch.is_control() => ' ',
            ch => ch,
        })
    })
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

    /// Appends `text`, one cell per column (see [`columns`]).
    pub fn push(&mut self, text: &str, ink: Ink) {
        self.cells.extend(columns(text).map(|ch| Cell { ch, ink }));
    }

    /// The columns `from..to` of `text`, without laying out what is outside
    /// them: a line of two megabytes has no business being copied whole.
    pub fn excerpt(text: &str, from: usize, to: usize, ink: Ink) -> Line {
        let cells = columns(text).skip(from).take(to.saturating_sub(from)).map(|ch| Cell { ch, ink });
        Line { cells: cells.collect() }
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
        assert_eq!(Line::text("a\tb", Ink::PLAIN).render(Terminal::plain(80).painter), "a⇥   b");
    }

    #[test]
    fn an_excerpt_takes_the_columns_asked_for() {
        let plain = Terminal::plain(80).painter;
        assert_eq!(Line::excerpt("hello world", 3, 8, Ink::PLAIN).render(plain), "lo wo");
        assert_eq!(Line::excerpt("a\tb", 1, 3, Ink::PLAIN).render(plain), "⇥");
        assert_eq!(Line::excerpt("short", 9, 20, Ink::PLAIN).width(), 0);
    }
}
