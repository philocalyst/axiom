//! CSV as banks write it: quoted fields with doubled quotes, CRLF, a byte-order
//! mark. Rows come out as cells that know where they are in the file.

use std::borrow::Cow;

use memchr::{memchr, memchr2};

use crate::Span;
use crate::format::Cell;

/// A row the reader lost the thread of, and where.
pub(crate) struct Broken {
    pub row: usize,
    pub span: Span,
    pub what: &'static str,
}

/// Rows of cells, borrowed from the text unless a doubled quote forces a copy.
pub(crate) struct Reader<'t> {
    text: &'t str,
    at: usize,
    pub row: usize,
}

impl<'t> Reader<'t> {
    pub fn new(text: &'t str) -> Reader<'t> {
        // Spreadsheet exports often start with a byte-order mark.
        Reader { text, at: if text.starts_with('\u{feff}') { 3 } else { 0 }, row: 0 }
    }

    /// The next row that is not blank, into `cells`.
    pub fn next(&mut self, cells: &mut Vec<Cell<'t>>) -> Option<Result<(), Broken>> {
        while self.at < self.text.len() {
            self.row += 1;
            let read = self.read_row(cells);
            if read.is_err() || cells.len() > 1 || !cells[0].text.is_empty() {
                return Some(read);
            }
            self.row -= 1;
        }
        None
    }

    fn read_row(&mut self, cells: &mut Vec<Cell<'t>>) -> Result<(), Broken> {
        cells.clear();
        let bytes = self.text.as_bytes();
        loop {
            let cell = if bytes.get(self.at) == Some(&b'"') { self.quoted()? } else { self.plain() };
            cells.push(cell);
            match bytes.get(self.at) {
                Some(b',') => self.at += 1,
                Some(_) => {
                    self.at += 1;
                    return Ok(());
                }
                None => return Ok(()),
            }
        }
    }

    /// Up to the next comma or line end, without the spaces around it.
    fn plain(&mut self) -> Cell<'t> {
        let bytes = self.text.as_bytes();
        let end = memchr2(b',', b'\n', &bytes[self.at..]).map_or(bytes.len(), |found| self.at + found);
        let raw = &self.text[self.at..end];
        let text = raw.trim();
        let start = self.at + raw.len() - raw.trim_start().len();
        self.at = end;
        Cell { text: Cow::Borrowed(text), span: Span { start, end: start + text.len() } }
    }

    /// A quoted cell, where `""` is one quote. Only such a cell is copied.
    fn quoted(&mut self) -> Result<Cell<'t>, Broken> {
        let bytes = self.text.as_bytes();
        let open = self.at;
        let (mut owned, mut copied, mut from) = (None::<String>, open + 1, open + 1);
        loop {
            let Some(found) = memchr(b'"', &bytes[from..]) else {
                return Err(self.lose(Span { start: open, end: open + 1 }, "the quote is never closed"));
            };
            let quote = from + found;
            if bytes.get(quote + 1) == Some(&b'"') {
                owned.get_or_insert_with(String::new).push_str(&self.text[copied..=quote]);
                (copied, from) = (quote + 2, quote + 2);
                continue;
            }
            let text = match owned {
                Some(mut text) => {
                    text.push_str(&self.text[copied..quote]);
                    Cow::Owned(text)
                }
                None => Cow::Borrowed(&self.text[open + 1..quote]),
            };
            self.at = quote + 1;
            while matches!(bytes.get(self.at), Some(b' ' | b'\t' | b'\r')) {
                self.at += 1;
            }
            return match bytes.get(self.at) {
                None | Some(b',' | b'\n') => Ok(Cell { text, span: Span { start: open, end: quote + 1 } }),
                Some(_) => Err(self.lose(Span { start: quote + 1, end: quote + 2 }, "text follows the closing quote")),
            };
        }
    }

    /// Gives up on the rest of this row, so that the next can be read.
    fn lose(&mut self, span: Span, what: &'static str) -> Broken {
        let bytes = self.text.as_bytes();
        self.at = memchr(b'\n', &bytes[span.start..]).map_or(bytes.len(), |found| span.start + found + 1);
        Broken { row: self.row, span, what }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(text: &str) -> Vec<Vec<String>> {
        let (mut reader, mut cells, mut all) = (Reader::new(text), Vec::new(), Vec::new());
        while let Some(read) = reader.next(&mut cells) {
            match read {
                Ok(()) => all.push(cells.iter().map(|cell| cell.text.to_string()).collect()),
                Err(broken) => all.push(vec![format!("broken: {}", broken.what)]),
            }
        }
        all
    }

    #[test]
    fn quotes_crlf_the_byte_order_mark_and_blank_lines() {
        let text = "\u{feff}a,b\r\n\"x, \"\"y\"\"\" , z \r\n\r\n,\n1,2";
        assert_eq!(rows(text), [["a", "b"], ["x, \"y\"", "z"], ["", ""], ["1", "2"]]);
    }

    #[test]
    fn a_broken_row_is_skipped_and_the_next_is_read() {
        assert_eq!(
            rows("a,\"b\" x\nc,d\n\"never"),
            [
                vec!["broken: text follows the closing quote"],
                vec!["c".into(), "d".into()],
                vec!["broken: the quote is never closed"]
            ]
        );
    }

    #[test]
    fn cells_know_where_they_are() {
        let text = "ab, cd ,\"e\"";
        let (mut reader, mut cells) = (Reader::new(text), Vec::new());
        assert!(reader.next(&mut cells).unwrap().is_ok());
        let spans: Vec<_> = cells.iter().map(|cell| &text[cell.span.start..cell.span.end]).collect();
        assert_eq!(spans, ["ab", "cd", "\"e\""]);
    }

    #[test]
    fn garbage_never_panics() {
        for text in
            ["", "\n\n", "\"", "A,B,C\n\"", ",,,\n,,", "A,B,C\n\u{0}\u{1},\u{ff}", "\u{feff}", "A\n,\"\"\"\"\"\n"]
        {
            let _ = rows(text);
        }
    }

    #[test]
    #[ignore = "a timing, alone: cargo test -p axiom-sync --release -- --ignored --test-threads=1"]
    fn cutting_a_million_rows_into_cells_alone() {
        let mut text = String::from("Posting Date,Description,Amount,Balance\n");
        for row in 0..1_000_000u32 {
            let quoted = if row % 5 == 0 { "\"TRADER JOE'S, #634 \"\"SF\"\"\"" } else { "SHELL OIL 5741" };
            text += &format!(
                "{:02}/{:02}/2026,{quoted},-{}.{:02},\"1,234.56\"\n",
                row % 12 + 1,
                row % 28 + 1,
                row % 900,
                row % 100
            );
        }
        let started = std::time::Instant::now();
        let (mut reader, mut cells, mut count) = (Reader::new(&text), Vec::new(), 0usize);
        while reader.next(&mut cells).is_some() {
            count += cells.len();
        }
        eprintln!("cut {} cells ({} MB) in {:?}", count, text.len() >> 20, started.elapsed());
    }
}
