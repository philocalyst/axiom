//! Where things are in a source text: lines, and columns within them.

use axiom_core::{FileId, Loc, Map};

use crate::project::{SourceFile, Sources};

/// Line indexes for the files of a run, each built the first time something
/// points into it. Diagnostics and report tables share one.
pub struct Locator<'a> {
    pub sources: &'a Sources,
    indexes: Map<FileId, LineIndex>,
}

impl<'a> Locator<'a> {
    pub fn new(sources: &'a Sources) -> Locator<'a> {
        Locator { sources, indexes: Map::default() }
    }

    pub fn index(&mut self, file: &SourceFile) -> &LineIndex {
        self.indexes.entry(file.id).or_insert_with(|| LineIndex::new(&file.text))
    }

    /// `journal/2026/01.ax:14`: what `axiom why` accepts back.
    pub fn describe(&mut self, loc: Loc) -> Option<String> {
        let file = self.sources.get(loc.file)?;
        let line = self.index(file).line_of(loc.start as usize) + 1;
        Some(format!("{}:{line}", file.path))
    }

    /// The bytes of line `number` (counted from 1) of the file at `path`.
    pub fn line(&mut self, path: &str, number: usize) -> Option<Loc> {
        let file = self.sources.find(path)?;
        let index = self.index(file);
        let line = number.checked_sub(1).filter(|&line| line < index.len())?;
        let start = index.start(line, &file.text);
        let end = start + index.line(line, &file.text).len();
        Some(Loc::new(file.id, start as u32, end as u32))
    }
}

/// The byte offset at which each line of a text starts.
pub struct LineIndex {
    starts: Vec<usize>,
}

impl LineIndex {
    pub fn new(text: &str) -> LineIndex {
        let newlines = memchr::memchr_iter(b'\n', text.as_bytes()).map(|at| at + 1);
        LineIndex { starts: std::iter::once(0).chain(newlines).collect() }
    }

    /// How many lines the text has.
    pub fn len(&self) -> usize {
        self.starts.len()
    }

    /// The line (counting from 0) that holds byte `offset`. An offset past the
    /// end belongs to the last line.
    pub fn line_of(&self, offset: usize) -> usize {
        self.starts.partition_point(|&start| start <= offset) - 1
    }

    /// Where `line` starts. A line past the end starts at the end.
    pub fn start(&self, line: usize, text: &str) -> usize {
        self.starts.get(line).copied().unwrap_or(text.len())
    }

    /// The text of `line`, without its line ending.
    pub fn line<'t>(&self, line: usize, text: &'t str) -> &'t str {
        let start = self.start(line, text);
        let end = self.start(line + 1, text);
        text[start..end].trim_end_matches(['\n', '\r'])
    }
}

/// How many columns the part of `line` before byte `offset` takes, counting a
/// tab as `tab` columns and every other character as one. An `offset` inside a
/// character counts that character, and one past the end counts the whole line.
pub fn columns_before(line: &str, offset: usize, tab: usize) -> usize {
    line.char_indices().take_while(|&(at, _)| at < offset).map(|(_, ch)| if ch == '\t' { tab } else { 1 }).sum()
}

/// The nearest character boundary at or before `offset`, and never past the end.
pub fn clamp(text: &str, offset: usize) -> usize {
    let mut offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_and_offsets() {
        let text = "ab\ncd\r\n\nlast";
        let index = LineIndex::new(text);
        assert_eq!([0, 2, 3, 5, 6, 7, 8, 100].map(|at| index.line_of(at)), [0, 0, 1, 1, 1, 2, 3, 3]);
        assert_eq!(index.line(1, text), "cd");
        assert_eq!(index.line(2, text), "");
        assert_eq!(index.line(3, text), "last");
        assert_eq!(index.line(9, text), "");
    }

    #[test]
    fn a_final_newline_starts_an_empty_line() {
        let index = LineIndex::new("a\n");
        assert_eq!(index.line_of(2), 1);
        assert_eq!(index.line(1, "a\n"), "");
    }

    #[test]
    fn columns_count_tabs_and_multibyte_characters() {
        let line = "\té→x";
        assert_eq!(columns_before(line, 0, 4), 0);
        assert_eq!(columns_before(line, 1, 4), 4);
        assert_eq!(columns_before(line, 3, 4), 5);
        assert_eq!(columns_before(line, 4, 1), 3);
        assert_eq!(columns_before(line, 99, 1), 4);
        // Inside `é`: the character is counted, nothing panics.
        assert_eq!(columns_before(line, 2, 1), 2);
    }

    #[test]
    fn clamping_lands_on_a_boundary() {
        assert_eq!(clamp("aé", 2), 1);
        assert_eq!(clamp("aé", 99), 3);
    }
}
