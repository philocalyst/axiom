//! Tagged statements: OFX version 1 (SGML, where a value runs to the next `<`
//! and only aggregates close), OFX version 2 and ISO 20022 camt.053 (XML). One
//! reader for all: it finds the declared records, and in each the values at the
//! declared paths (`BookgDt/Dt`, or just `DTPOSTED`, which matches at any depth).

use std::borrow::Cow;

use memchr::memchr;

use crate::Span;
use crate::csv::Broken;
use crate::format::{ABSENT, Cell};

/// A tag, and the text after it up to the next tag.
struct Tag<'t> {
    name: &'t str,
    closing: bool,
    /// `<Nil/>`: nothing is inside.
    empty: bool,
    at: Span,
    value: &'t str,
    /// Where the value is, without the spaces around it.
    span: Span,
}

/// The tags of a text, skipping declarations, comments and processing
/// instructions. Attributes are not read.
fn tags(text: &str) -> impl Iterator<Item = Tag<'_>> {
    let bytes = text.as_bytes();
    let mut from = 0;
    std::iter::from_fn(move || {
        loop {
            let open = from + memchr(b'<', &bytes[from..])?;
            let close = open + memchr(b'>', &bytes[open..])?;
            let next = close + 1 + memchr(b'<', &bytes[close + 1..]).unwrap_or(bytes.len() - close - 1);
            from = next;
            let raw = text[open + 1..close].trim();
            if raw.starts_with(['?', '!']) {
                continue;
            }
            let (closing, raw) = raw.strip_prefix('/').map_or((false, raw), |name| (true, name));
            let (empty, raw) = raw.strip_suffix('/').map_or((false, raw), |name| (true, name));
            let name = raw.split_whitespace().next().unwrap_or("");
            let (all, value) = (&text[close + 1..next], text[close + 1..next].trim());
            let start = close + 1 + all.len() - all.trim_start().len();
            return Some(Tag {
                name,
                closing,
                empty,
                at: Span { start: open, end: close + 1 },
                value,
                span: Span { start, end: start + value.len() },
            });
        }
    })
}

/// One record: its number, where it is, and a cell for each path asked for.
pub(crate) struct Found<'t> {
    pub number: usize,
    pub whole: Span,
    pub cells: Vec<Cell<'t>>,
}

/// The five escapes SGML and XML share.
fn decode(text: &str) -> Cow<'_, str> {
    if !text.contains('&') {
        return Cow::Borrowed(text);
    }
    let escapes = [("&lt;", "<"), ("&gt;", ">"), ("&quot;", "\""), ("&apos;", "'"), ("&amp;", "&")];
    Cow::Owned(escapes.iter().fold(text.to_string(), |text, (escape, plain)| text.replace(escape, plain)))
}

/// Whether the element path `open` (outermost first) ends with `wanted`, in
/// any case.
fn ends_with(open: &[&str], wanted: &[&str]) -> bool {
    open.len() >= wanted.len()
        && open[open.len() - wanted.len()..].iter().zip(wanted).all(|(a, b)| a.eq_ignore_ascii_case(b))
}

/// Calls `each` with every `records` element of `text`, until it says stop.
/// `paths` are the values wanted, as `A/B` element paths; each record has one
/// cell for each, empty and [`ABSENT`] where the record has no such element.
/// The first value at a path is the one kept.
pub(crate) fn scan<'t>(
    text: &'t str,
    records: &str,
    paths: &[&str],
    mut each: impl FnMut(Result<Found<'t>, Broken>) -> bool,
) {
    let wanted: Vec<Vec<&str>> = paths.iter().map(|path| path.split('/').collect()).collect();
    let (mut count, mut seen) = (0, false);
    // The record being read: where it began, its cells, and the elements open in it.
    let mut open: Option<(Span, Vec<Cell<'t>>, Vec<&str>)> = None;
    // The leaf just read, whose closing tag (XML) says nothing.
    let mut leaf: Option<&str> = None;
    for tag in tags(text) {
        seen = true;
        let Some((begin, cells, stack)) = open.as_mut() else {
            if !tag.closing && tag.name.eq_ignore_ascii_case(records) && !tag.empty {
                count += 1;
                let nothing = Cell { text: Cow::Borrowed(""), span: ABSENT };
                open = Some((tag.at, vec![nothing; paths.len()], Vec::new()));
            }
            continue;
        };
        let after_leaf = leaf.take();
        if tag.closing {
            if tag.name.eq_ignore_ascii_case(records) {
                let (whole, cells) = (Span { start: begin.start, end: tag.at.end }, std::mem::take(cells));
                open = None;
                if !each(Ok(Found { number: count, whole, cells })) {
                    return;
                }
            } else if after_leaf != Some(tag.name) {
                // Also closes what an unclosed empty element (SGML) left open inside it.
                if let Some(depth) = stack.iter().rposition(|name| name.eq_ignore_ascii_case(tag.name)) {
                    stack.truncate(depth);
                }
            }
        } else if tag.empty {
            continue;
        } else if tag.value.is_empty() {
            stack.push(tag.name);
        } else {
            leaf = Some(tag.name);
            stack.push(tag.name);
            for (slot, path) in wanted.iter().enumerate() {
                if ends_with(stack, path) && cells[slot].span == ABSENT {
                    cells[slot] = Cell { text: decode(tag.value), span: tag.span };
                }
            }
            stack.pop();
        }
    }
    if let Some((begin, ..)) = open {
        each(Err(Broken { row: count, span: begin, what: "the record is never closed" }));
    } else if !seen {
        each(Err(Broken { row: 0, span: Span { start: 0, end: text.len().min(1) }, what: "there are no tags in it" }));
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
}
