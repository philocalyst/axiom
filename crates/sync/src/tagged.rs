//! Tagged statements: OFX version 1 (SGML, where a value runs to the next `<`
//! and only aggregates close), OFX version 2 and ISO 20022 camt.053 (XML). One
//! reader for all: it finds the declared records, and in each the values at the
//! declared paths (`BookgDt/Dt`, or just `DTPOSTED`, which matches at any depth).

use std::borrow::Cow;

use axiom_core::{Groups, Id};
use memchr::memchr;

use crate::Span;
use crate::cell::{ABSENT, Cell};
use crate::csv::Broken;

/// A tag, and the text after it up to the next tag.
struct Tag<'t> {
    name: &'t str,
    closing: bool,
    /// `<Nil/>`: nothing is inside.
    empty: bool,
    at: Span,
    value: &'t str,
    /// Where the value is, with the white space around it.
    span: Span,
    /// A CDATA text segment, not an element boundary.
    cdata: bool,
}

impl Tag<'_> {
    /// Where the text of the tag is, without the white space around it.
    fn written(&self) -> Span {
        let (leading, trailing) =
            (self.value.len() - self.value.trim_start().len(), self.value.len() - self.value.trim_end().len());
        Span { start: self.span.start.saturating_add(leading), end: self.span.end.saturating_sub(trailing) }
    }
}

/// The tags of a text, skipping declarations, comments and processing
/// instructions. Attributes are not read.
fn tags(text: &str) -> Tags<'_> {
    Tags { text, from: 0 }
}

struct Tags<'t> {
    text: &'t str,
    /// Where the next thing starts.
    from: usize,
}

impl<'t> Iterator for Tags<'t> {
    type Item = Result<Tag<'t>, Broken>;

    fn next(&mut self) -> Option<Self::Item> {
        while self.from < self.text.len() {
            if let Some(found) = self.read() {
                return Some(found);
            }
        }
        None
    }
}

impl<'t> Tags<'t> {
    /// What starts at `from`: text, or markup. Markup that carries nothing is stepped over and gives `None`.
    fn read(&mut self) -> Option<Result<Tag<'t>, Broken>> {
        let bytes = self.text.as_bytes();
        let open = memchr(b'<', &bytes[self.from..]).map_or(bytes.len(), |relative| self.from + relative);
        if open > self.from {
            return Some(Ok(self.text_to(open)));
        }
        let markup = &bytes[open..];
        if markup.starts_with(b"<![CDATA[") {
            return Some(self.cdata(open));
        }
        let (end, unclosed) = if markup.starts_with(b"<!--") {
            (self.after(open + 4, "-->"), "a comment is never closed")
        } else if markup.starts_with(b"<?") {
            (self.after(open + 2, "?>"), "a processing instruction is never closed")
        } else if markup.get(..9).is_some_and(|head| head.eq_ignore_ascii_case(b"<!DOCTYPE")) {
            (declaration_end(bytes, open + 9).map(|close| close + 1), "a document type declaration is never closed")
        } else if markup.starts_with(b"<!") {
            (tag_end(bytes, open + 2).map(|close| close + 1), "a markup declaration is malformed or never closed")
        } else {
            return Some(self.element(open));
        };
        match end {
            Some(end) => {
                self.from = end;
                None
            }
            None => Some(Err(self.broken(open, unclosed))),
        }
    }

    /// The end of the first `closer` from `from` on.
    fn after(&self, from: usize, closer: &str) -> Option<usize> {
        self.text[from..].find(closer).map(|at| from + at + closer.len())
    }

    /// The text up to `end`.
    fn text_to(&mut self, end: usize) -> Tag<'t> {
        let at = Span { start: self.from, end };
        self.from = end;
        Tag { name: "", closing: false, empty: false, at, value: &self.text[at.start..end], span: at, cdata: false }
    }

    /// The CDATA section that opens at `open`: its contents are text, whatever they look like.
    fn cdata(&mut self, open: usize) -> Result<Tag<'t>, Broken> {
        let body = open + "<![CDATA[".len();
        let Some(stop) = self.after(body, "]]>").map(|end| end - "]]>".len()) else {
            return Err(self.broken(open, "a CDATA section is never closed"));
        };
        self.from = stop + "]]>".len();
        let (at, span) = (Span { start: open, end: self.from }, Span { start: body, end: stop });
        Ok(Tag { name: "", closing: false, empty: false, at, value: &self.text[body..stop], span, cdata: true })
    }

    /// The element tag that opens at `open`.
    fn element(&mut self, open: usize) -> Result<Tag<'t>, Broken> {
        let Some(close) = tag_end(self.text.as_bytes(), open + 1) else {
            return Err(self.broken(open, "a tag is never closed"));
        };
        self.from = close + 1;
        let raw = self.text[open + 1..close].trim();
        let (closing, raw) = raw.strip_prefix('/').map_or((false, raw), |name| (true, name));
        let (empty, raw) = raw.strip_suffix('/').map_or((false, raw), |name| (true, name));
        let name = raw.split_whitespace().next().unwrap_or("");
        let (at, span) = (Span { start: open, end: close + 1 }, Span { start: close + 1, end: close + 1 });
        Ok(Tag { name, closing, empty, at, value: "", span, cdata: false })
    }

    /// Markup at `open` that never ends: the rest of the text is lost with it.
    fn broken(&mut self, open: usize, what: &'static str) -> Broken {
        self.from = self.text.len();
        Broken { row: 0, span: Span { start: open, end: self.text.len() }, what }
    }
}

fn tag_end(bytes: &[u8], mut at: usize) -> Option<usize> {
    let mut quote = None;
    while let Some(&byte) = bytes.get(at) {
        if let Some(end) = quote {
            if byte == end {
                quote = None;
            }
        } else {
            match byte {
                b'\'' | b'"' => quote = Some(byte),
                b'>' => return Some(at),
                _ => {}
            }
        }
        at += 1;
    }
    None
}

fn declaration_end(bytes: &[u8], mut at: usize) -> Option<usize> {
    let (mut quote, mut subset) = (None, 0usize);
    while let Some(&byte) = bytes.get(at) {
        if let Some(end) = quote {
            if byte == end {
                quote = None;
            }
        } else {
            match byte {
                b'\'' | b'"' => quote = Some(byte),
                b'[' => subset += 1,
                b']' => subset = subset.saturating_sub(1),
                b'>' if subset == 0 => return Some(at),
                _ => {}
            }
        }
        at += 1;
    }
    None
}

/// One record: its number, where it is, and a cell for each path asked for.
pub(crate) struct Found<'a, 't> {
    pub number: usize,
    pub whole: Span,
    pub cells: &'a [Cell<'t>],
}

/// The five escapes SGML and XML share.
fn decode(text: &str) -> Result<Cow<'_, str>, &'static str> {
    if !text.contains('&') {
        return Ok(Cow::Borrowed(text));
    }
    let mut decoded = String::with_capacity(text.len());
    let mut from = 0;
    while let Some(relative) = text[from..].find('&') {
        let open = from + relative;
        decoded.push_str(&text[from..open]);
        let Some(end) = text[open + 1..].find(';').map(|end| open + 1 + end) else {
            return Err("an entity reference is never closed");
        };
        let entity = &text[open + 1..end];
        let value = match entity {
            "lt" => '<',
            "gt" => '>',
            "quot" => '"',
            "apos" => '\'',
            "amp" => '&',
            name if name.starts_with("#x") || name.starts_with("#X") => {
                let code =
                    u32::from_str_radix(&name[2..], 16).map_err(|_| "a numeric XML character reference is invalid")?;
                char::from_u32(code).ok_or("a numeric XML character reference is invalid")?
            }
            name if name.starts_with('#') => {
                let code = name[1..].parse::<u32>().map_err(|_| "a numeric XML character reference is invalid")?;
                char::from_u32(code).ok_or("a numeric XML character reference is invalid")?
            }
            _ => return Err("the export uses an unknown entity reference"),
        };
        decoded.push(value);
        from = end + 1;
    }
    decoded.push_str(&text[from..]);
    Ok(Cow::Owned(decoded))
}

/// Whether the element path `open` (outermost first) ends with `wanted`, in
/// any case.
fn ends_with(open: &[&str], wanted: &[&str]) -> bool {
    open.len() >= wanted.len()
        && open[open.len() - wanted.len()..].iter().zip(wanted).all(|(a, b)| a.eq_ignore_ascii_case(b))
}

/// `text` without the white space around it, borrowed from where it was if it was.
fn trimmed(text: Cow<'_, str>) -> Cow<'_, str> {
    match text {
        Cow::Borrowed(text) => Cow::Borrowed(text.trim()),
        Cow::Owned(mut text) => {
            let start = text.len() - text.trim_start().len();
            let end = text.trim_end().len();
            text.truncate(end);
            text.drain(..start);
            Cow::Owned(text)
        }
    }
}

/// `text`, then `separator`, then `more`.
fn joined<'t>(text: Cow<'t, str>, separator: &str, more: &str) -> Cow<'t, str> {
    let mut joined = match text {
        Cow::Borrowed(text) => {
            let mut joined = String::with_capacity(text.len() + more.len() + 1);
            joined.push_str(text);
            joined
        }
        Cow::Owned(text) => text,
    };
    joined.push_str(separator);
    joined.push_str(more);
    Cow::Owned(joined)
}

impl Reading {
    /// Appends one text node to the first value found at an element path. XML text split by comments or CDATA
    /// stays one value; a boundary that carried spaces contributes one separator while an adjacent boundary
    /// contributes none.
    fn add<'t>(&mut self, cell: &mut Cell<'t>, tag: &Tag<'t>) -> Result<(), &'static str> {
        let decoded = if tag.cdata { Cow::Borrowed(tag.value) } else { decode(tag.value)? };
        if decoded.trim().is_empty() {
            self.trailing_space = cell.span != ABSENT;
            return Ok(());
        }
        let leading_space = decoded.len() != decoded.trim_start().len();
        let trailing_space = decoded.len() != decoded.trim_end().len();
        if cell.span == ABSENT {
            (cell.text, cell.span) = (trimmed(decoded), tag.written());
        } else {
            let spaced = (self.trailing_space || leading_space) && !cell.text.is_empty();
            let separator = if spaced { " " } else { "" };
            cell.text = joined(std::mem::take(&mut cell.text), separator, decoded.trim());
            cell.span.end = tag.span.end;
        }
        self.trailing_space = trailing_space;
        Ok(())
    }
}

/// Calls `each` with every `records` element of `text`, until it says stop.
/// `paths` are the values wanted, as `A/B` element paths; each record has one
/// cell for each, empty and [`ABSENT`] where the record has no such element.
/// The first value at a path is the one kept.
pub(crate) fn scan<'t>(
    text: &'t str,
    records: &str,
    paths: &[&str],
    mut each: impl for<'a> FnMut(Result<Found<'a, 't>, Broken>) -> bool,
) {
    let mut reader = Reader::new(records, paths);
    for tag in tags(text) {
        let step = match tag {
            Ok(tag) => reader.read(tag),
            Err(broken) => Step::Broken(Broken { row: reader.count, ..broken }),
        };
        let more = match step {
            Step::Continue => true,
            Step::Record(found) => each(Ok(found)),
            Step::Broken(broken) => {
                each(Err(broken));
                false
            }
        };
        if !more {
            return;
        }
    }
    if let Some(broken) = reader.ended(text) {
        each(Err(broken));
    }
}

/// What a tag did to the record being read.
enum Step<'a, 't> {
    Continue,
    /// The record closed: its cells are ready.
    Record(Found<'a, 't>),
    Broken(Broken),
}

/// Marks a row of [`Reader::wanted`]: the names of one path.
struct Wanted;

/// Where reading one wanted value stands.
#[derive(Clone, Copy, Default)]
struct Reading {
    /// Text is still being added to it.
    capturing: bool,
    /// What was added last ended in a space, which the next text joins with.
    trailing_space: bool,
}

/// The tags of one text, read as records.
struct Reader<'p, 't> {
    records: &'p str,
    /// Each wanted path as the names of its elements, outermost first.
    wanted: Groups<Wanted, &'p str>,
    /// The value at each wanted path, in the order they were asked for.
    cells: Vec<Cell<'t>>,
    reading: Vec<Reading>,
    /// How many records have begun.
    count: usize,
    /// Where the record being read began: `None` between records.
    record: Option<Span>,
    /// The elements open at this point of the record, outermost first.
    stack: Vec<&'t str>,
    /// The leaf just read, whose closing tag (XML) says nothing.
    leaf: Option<&'t str>,
    seen: bool,
}

impl<'p, 't> Reader<'p, 't> {
    fn new(records: &'p str, paths: &[&'p str]) -> Reader<'p, 't> {
        let names =
            paths.iter().enumerate().flat_map(|(at, path)| path.split('/').map(move |name| (Id::new(at as u32), name)));
        Reader {
            records,
            wanted: Groups::build(paths.len(), names),
            cells: paths.iter().map(|_| Cell::absent()).collect(),
            reading: vec![Reading::default(); paths.len()],
            count: 0,
            record: None,
            stack: Vec::new(),
            leaf: None,
            seen: false,
        }
    }

    fn read(&mut self, tag: Tag<'t>) -> Step<'_, 't> {
        if tag.name.is_empty() {
            return self.text(&tag);
        }
        self.seen = true;
        let Some(begin) = self.record else {
            self.begin(&tag);
            return Step::Continue;
        };
        let after_leaf = self.leaf.take();
        if tag.closing {
            return self.close(begin, &tag, after_leaf);
        }
        self.open(&tag, after_leaf);
        Step::Continue
    }

    /// Text inside the record goes to every wanted value the open elements lead to.
    fn text(&mut self, tag: &Tag<'t>) -> Step<'_, 't> {
        if self.record.is_none() {
            return Step::Continue;
        }
        for (path, names) in self.wanted.iter() {
            let (cell, reading) = (&mut self.cells[path.index()], &mut self.reading[path.index()]);
            if ends_with(&self.stack, names) && (cell.span == ABSENT || reading.capturing) {
                if let Err(what) = reading.add(cell, tag) {
                    return Step::Broken(Broken { row: self.count, span: tag.span, what });
                }
                reading.capturing = true;
            }
        }
        if !tag.value.trim().is_empty() && self.wanted.iter().any(|(_, names)| ends_with(&self.stack, names)) {
            self.leaf = self.stack.last().copied();
        }
        Step::Continue
    }

    /// Outside a record, only the opening of one matters.
    fn begin(&mut self, tag: &Tag<'t>) {
        if tag.closing || tag.empty || !tag.name.eq_ignore_ascii_case(self.records) {
            return;
        }
        self.count += 1;
        self.record = Some(tag.at);
        self.cells.iter_mut().for_each(|cell| *cell = Cell::absent());
        self.reading.fill(Reading::default());
        self.stack.clear();
        self.stack.push(tag.name);
    }

    fn open(&mut self, tag: &Tag<'t>, after_leaf: Option<&'t str>) {
        // An SGML leaf has no closing tag: the next opening one ends it.
        if let Some(previous) = after_leaf
            && self.stack.last().is_some_and(|name| name.eq_ignore_ascii_case(previous))
        {
            self.release();
            self.stack.pop();
        }
        if !tag.empty {
            self.stack.push(tag.name);
        }
    }

    fn close(&mut self, begin: Span, tag: &Tag<'t>, after_leaf: Option<&'t str>) -> Step<'_, 't> {
        if tag.name.eq_ignore_ascii_case(self.records) {
            self.record = None;
            self.stack.clear();
            self.reading.fill(Reading::default());
            let whole = Span { start: begin.start, end: tag.at.end };
            return Step::Record(Found { number: self.count, whole, cells: &self.cells });
        }
        if after_leaf == Some(tag.name) {
            self.release();
            if self.stack.last().is_some_and(|name| name.eq_ignore_ascii_case(tag.name)) {
                self.stack.pop();
            }
        // Also closes what an unclosed empty element (SGML) left open inside it.
        } else if let Some(depth) = self.stack.iter().rposition(|name| name.eq_ignore_ascii_case(tag.name)) {
            self.release();
            self.stack.truncate(depth);
        }
        Step::Continue
    }

    /// Stops reading the values whose element is closing.
    fn release(&mut self) {
        for (path, _) in self.wanted.iter().filter(|(_, names)| ends_with(&self.stack, names)) {
            self.reading[path.index()] = Reading::default();
        }
    }

    /// What is wrong with a text that ended, if anything is.
    fn ended(&self, text: &str) -> Option<Broken> {
        match self.record {
            Some(begin) => Some(Broken { row: self.count, span: begin, what: "the record is never closed" }),
            None if !self.seen => Some(Broken {
                row: 0,
                span: Span { start: 0, end: text.len().min(1) },
                what: "there are no tags in it",
            }),
            None => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The records of `text`: each cell's text, or `-` where a path has none.
    fn read(text: &str, records: &str, paths: &[&str]) -> Vec<Vec<String>> {
        let mut all = Vec::new();
        scan(text, records, paths, |found| {
            match found {
                Ok(found) => all.push(
                    found
                        .cells
                        .iter()
                        .map(|cell| if cell.span == ABSENT { "-".into() } else { cell.text.to_string() })
                        .collect(),
                ),
                Err(broken) => all.push(vec![format!("broken: {}", broken.what)]),
            }
            true
        });
        all
    }

    #[test]
    fn sgml_leaves_have_no_closing_tags_and_aggregates_do() {
        let text = "OFXHEADER:100\n\n<OFX>\n<STMTTRN>\n<TRNTYPE>DEBIT\n<DTPOSTED>20260105120000[-5:EST]\n<TRNAMT>-84.20\n\
                    <NAME>TRADER JOE'S\n<MEMO>\n</STMTTRN>\n<STMTTRN><DTPOSTED>20260106<TRNAMT>-1.00<NAME>B &amp; C</STMTTRN></OFX>";
        assert_eq!(
            read(text, "STMTTRN", &["DTPOSTED", "TRNAMT", "NAME", "MEMO", "CHECKNUM"]),
            [["20260105120000[-5:EST]", "-84.20", "TRADER JOE'S", "-", "-"], ["20260106", "-1.00", "B & C", "-", "-"],],
            "an empty MEMO is nothing, and does not swallow the closing tag of its record"
        );
    }

    #[test]
    fn xml_paths_name_elements_in_the_record_and_any_case_matches() {
        let text = "<?xml version=\"1.0\"?><Document><Stmt><Ntry><Amt Ccy=\"EUR\">100.00</Amt><CdtDbtInd>CRDT</CdtDbtInd>\
                    <BookgDt><Dt>2026-01-05</Dt></BookgDt><Sts>BOOK</Sts>\
                    <NtryDtls><TxDtls><Amt>99.00</Amt><RmtInf><Ustrd>one</Ustrd><Ustrd>two</Ustrd></RmtInf></TxDtls></NtryDtls></Ntry>\
                    <Ntry><Amt>5.00</Amt><Nil/><BookgDt><Dt>2026-01-06</Dt></BookgDt></Ntry></Stmt></Document>";
        assert_eq!(
            read(text, "ntry", &["Amt", "cdtdbtind", "BookgDt/Dt", "Dt", "RmtInf/Ustrd", "Nope/Amt"]),
            [
                ["100.00", "CRDT", "2026-01-05", "2026-01-05", "one", "-"],
                ["5.00", "-", "2026-01-06", "2026-01-06", "-", "-"],
            ],
            "the first value at a path is the record's, not a deeper one's"
        );
    }

    #[test]
    fn comments_do_not_turn_embedded_record_text_into_rows() {
        let text = "<Document><!-- <Ntry><Amt>999</Amt></Ntry> --><Ntry><Amt>5</Amt></Ntry></Document>";
        assert_eq!(read(text, "Ntry", &["Amt"]), [["5"]]);
    }

    #[test]
    fn cdata_and_numeric_xml_references_are_read_as_text() {
        let text = "<Ntry><Ustrd><![CDATA[a < b & c]]></Ustrd><Memo>Smile &#x1F642; &#169;</Memo></Ntry>";
        assert_eq!(read(text, "Ntry", &["Ustrd", "Memo"]), [["a < b & c", "Smile 🙂 ©"]]);
    }

    #[test]
    fn comments_and_cdata_split_one_memo_without_losing_text_or_repeats() {
        let text = "<Ntry><Memo>PAY<!-- separator -->\
                    <![CDATA[PAL]]> CARD</Memo><Memo>ignored duplicate</Memo></Ntry>";
        assert_eq!(read(text, "Ntry", &["Memo"]), [["PAYPAL CARD"]]);

        let spaced = "<Ntry><Memo>PAY <!-- separator --> PAL</Memo></Ntry>";
        assert_eq!(read(spaced, "Ntry", &["Memo"]), [["PAY PAL"]]);
    }

    #[test]
    fn whitespace_between_nested_camt_elements_does_not_close_the_record() {
        let text = "<Document>\n  <Ntry>\n    <NtryDtls>\n      <TxDtls>\n        <RmtInf>\n          <Ustrd>PAYPAL</Ustrd>\n        </RmtInf>\n      </TxDtls>\n    </NtryDtls>\n  </Ntry>\n</Document>";
        assert_eq!(read(text, "Ntry", &["RmtInf/Ustrd"]), [["PAYPAL"]]);
    }

    #[test]
    fn malformed_markup_and_unknown_entities_stop_the_scan() {
        for text in ["<R><!-- no end", "<R><?xml no end", "<R><![CDATA[no end"] {
            let found = read(text, "R", &["x"]);
            assert_eq!(found.len(), 1, "{text}");
            assert!(found[0][0].starts_with("broken:"), "{text}");
        }
        let found = read("<R><x>&unknown;</x></R>", "R", &["x"]);
        assert!(found[0][0].starts_with("broken:"));
    }

    #[test]
    fn a_doctype_with_an_internal_subset_is_skipped_without_finding_fake_tags() {
        let text = "<!DOCTYPE Document [<!ENTITY fake '<Ntry>'>]><Document><Ntry><Amt>5</Amt></Ntry></Document>";
        assert_eq!(read(text, "Ntry", &["Amt"]), [["5"]]);
    }

    #[test]
    fn records_that_cannot_be_read_are_said_so() {
        assert_eq!(read("<A><R><x>1</x></A>", "R", &["x"]), [["broken: the record is never closed"]]);
        assert_eq!(read("a plain text file", "R", &["x"]), [["broken: there are no tags in it"]]);
        assert!(read("<A></A>", "R", &["x"]).is_empty(), "no records is an empty statement, not a broken one");
        assert_eq!(read("<R><x>1</x></R><R><x>2</x></R>", "R", &["x"]).len(), 2);
    }

    #[test]
    fn the_reader_stops_when_asked() {
        let mut count = 0;
        scan("<R/><R><x>1</x></R><R><x>2</x></R><R><x>3</x></R>", "R", &["x"], |_| {
            count += 1;
            count < 2
        });
        assert_eq!(count, 2);
    }

    #[test]
    fn garbage_never_panics() {
        for text in ["", "<", ">", "<>", "</>", "<R>", "</R>", "<R><", "<!--", "<R><a>1</R>", "<R></a></R>", "<\u{ff}>"]
        {
            let _ = read(text, "R", &["a", "a/b"]);
        }
    }

    #[test]
    #[ignore = "a timing, alone: cargo test -p axiom-sync --release -- --ignored --test-threads=1"]
    fn finding_a_hundred_thousand_records_alone() {
        let mut text = String::from("<OFX><BANKTRANLIST>\n");
        for at in 0..100_000u32 {
            text += &format!(
                "<STMTTRN><TRNTYPE>DEBIT<DTPOSTED>20260105120000<TRNAMT>-{}.{:02}<FITID>{at}<NAME>SHELL OIL {at}<MEMO>SAN FRANCISCO CA</STMTTRN>\n",
                at % 900,
                at % 100
            );
        }
        let started = std::time::Instant::now();
        let (mut count, mut tags) = (0, 0);
        scan(&text, "STMTTRN", &["DTPOSTED", "TRNAMT", "NAME", "MEMO"], |_| {
            count += 1;
            true
        });
        tags += super::tags(&text).count();
        eprintln!(
            "found {count} records ({} MB, {tags} tags) in {:?}, tags alone included",
            text.len() >> 20,
            started.elapsed()
        );
    }
}
