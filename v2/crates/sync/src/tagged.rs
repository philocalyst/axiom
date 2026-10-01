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
    /// A CDATA text segment, not an element boundary.
    cdata: bool,
}

/// The tags of a text, skipping declarations, comments and processing
/// instructions. Attributes are not read.
fn tags(text: &str) -> impl Iterator<Item = Result<Tag<'_>, Broken>> {
    let bytes = text.as_bytes();
    let mut from = 0;
    std::iter::from_fn(move || {
        loop {
            let open = from + memchr(b'<', &bytes[from..])?;
            if bytes[open..].starts_with(b"<!--") {
                let Some(end) = text[open + 4..].find("-->") else {
                    from = bytes.len();
                    return Some(Err(Broken {
                        row: 0,
                        span: Span { start: open, end: bytes.len() },
                        what: "a comment is never closed",
                    }));
                };
                from = open + 4 + end + 3;
                continue;
            }
            if bytes[open..].starts_with(b"<![CDATA[") {
                let body = open + 9;
                let Some(end) = text[body..].find("]]>") else {
                    from = bytes.len();
                    return Some(Err(Broken {
                        row: 0,
                        span: Span { start: open, end: bytes.len() },
                        what: "a CDATA section is never closed",
                    }));
                };
                let stop = body + end;
                from = stop + 3;
                return Some(Ok(Tag {
                    name: "",
                    closing: false,
                    empty: false,
                    at: Span { start: open, end: from },
                    value: &text[body..stop],
                    span: Span { start: body, end: stop },
                    cdata: true,
                }));
            }
            if bytes[open..].starts_with(b"<?") {
                let Some(end) = text[open + 2..].find("?>") else {
                    from = bytes.len();
                    return Some(Err(Broken {
                        row: 0,
                        span: Span { start: open, end: bytes.len() },
                        what: "a processing instruction is never closed",
                    }));
                };
                from = open + 2 + end + 2;
                continue;
            }
            if bytes[open..].get(..9).is_some_and(|head| head.eq_ignore_ascii_case(b"<!DOCTYPE")) {
                let Some(close) = declaration_end(bytes, open + 9) else {
                    from = bytes.len();
                    return Some(Err(Broken {
                        row: 0,
                        span: Span { start: open, end: bytes.len() },
                        what: "a document type declaration is never closed",
                    }));
                };
                from = close + 1;
                continue;
            }
            if bytes[open..].starts_with(b"<!") {
                let Some(close) = tag_end(bytes, open + 2) else {
                    from = bytes.len();
                    return Some(Err(Broken {
                        row: 0,
                        span: Span { start: open, end: bytes.len() },
                        what: "a markup declaration is malformed or never closed",
                    }));
                };
                from = close + 1;
                continue;
            }
            let Some(close_rel) = tag_end(bytes, open + 1) else {
                from = bytes.len();
                return Some(Err(Broken {
                    row: 0,
                    span: Span { start: open, end: bytes.len() },
                    what: "a tag is never closed",
                }));
            };
            let close = close_rel;
            let next =
                close + 1 + memchr(b'<', &bytes[close + 1..]).unwrap_or(bytes.len() - close - 1);
            from = next;
            let raw = text[open + 1..close].trim();
            let (closing, raw) = raw
                .strip_prefix('/')
                .map_or((false, raw), |name| (true, name));
            let (empty, raw) = raw
                .strip_suffix('/')
                .map_or((false, raw), |name| (true, name));
            let name = raw.split_whitespace().next().unwrap_or("");
            let (all, value) = (&text[close + 1..next], text[close + 1..next].trim());
            let start = close + 1 + all.len() - all.trim_start().len();
            return Some(Ok(Tag {
                name,
                closing,
                empty,
                at: Span {
                    start: open,
                    end: close + 1,
                },
                value,
                span: Span {
                    start,
                    end: start + value.len(),
                },
                cdata: false,
            }));
        }
    })
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
            "lt" => '<', "gt" => '>', "quot" => '"', "apos" => '\'', "amp" => '&',
            name if name.starts_with("#x") || name.starts_with("#X") => {
                let code = u32::from_str_radix(&name[2..], 16)
                    .map_err(|_| "a numeric XML character reference is invalid")?;
                char::from_u32(code).ok_or("a numeric XML character reference is invalid")?
            }
            name if name.starts_with('#') => {
                let code = name[1..].parse::<u32>()
                    .map_err(|_| "a numeric XML character reference is invalid")?;
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
        && open[open.len() - wanted.len()..]
            .iter()
            .zip(wanted)
            .all(|(a, b)| a.eq_ignore_ascii_case(b))
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
    let wanted: Vec<Vec<&str>> = paths.iter().map(|path| path.split('/').collect()).collect();
    let (mut count, mut seen) = (0, false);
    let mut cells: Vec<Cell<'t>> = (0..paths.len())
        .map(|_| Cell {
            text: Cow::Borrowed(""),
            span: ABSENT,
        })
        .collect();
    let mut stack: Vec<&str> = Vec::new();
    let mut record_start = None;
    // The leaf just read, whose closing tag (XML) says nothing.
    let mut leaf: Option<&str> = None;
    for tag in tags(text) {
        let tag = match tag {
            Ok(tag) => tag,
            Err(mut broken) => {
                broken.row = count;
                each(Err(broken));
                return;
            }
        };
        seen = true;
        if tag.cdata {
            let Some(_) = record_start else {
                continue;
            };
            for (slot, path) in wanted.iter().enumerate() {
                if ends_with(&stack, path) && cells[slot].span == ABSENT {
                    cells[slot] = Cell {
                        text: Cow::Borrowed(tag.value),
                        span: tag.span,
                    };
                }
            }
            continue;
        }
        let Some(begin) = record_start else {
            if !tag.closing && tag.name.eq_ignore_ascii_case(records) && !tag.empty {
                count += 1;
                record_start = Some(tag.at);
                for cell in &mut cells {
                    *cell = Cell {
                        text: Cow::Borrowed(""),
                        span: ABSENT,
                    };
                }
                stack.clear();
                stack.push(tag.name);
            }
            continue;
        };
        let after_leaf = leaf.take();
        if tag.closing {
            if tag.name.eq_ignore_ascii_case(records) {
                let whole = Span { start: begin.start, end: tag.at.end };
                record_start = None;
                if !each(Ok(Found {
                    number: count,
                    whole,
                    cells: &cells,
                })) {
                    return;
                }
                stack.clear();
            } else if after_leaf != Some(tag.name) {
                // Also closes what an unclosed empty element (SGML) left open inside it.
                if let Some(depth) = stack
                    .iter()
                    .rposition(|name| name.eq_ignore_ascii_case(tag.name))
                {
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
                if ends_with(&stack, path) && cells[slot].span == ABSENT {
                    let text = match decode(tag.value) {
                        Ok(text) => text,
                        Err(what) => {
                            each(Err(Broken {
                                row: count,
                                span: tag.span,
                                what,
                            }));
                            return;
                        }
                    };
                    cells[slot] = Cell {
                        text,
                        span: tag.span,
                    };
                }
            }
            stack.pop();
        }
    }
    if let Some(begin) = record_start {
        each(Err(Broken {
            row: count,
            span: begin,
            what: "the record is never closed",
        }));
    } else if !seen {
        each(Err(Broken {
            row: 0,
            span: Span {
                start: 0,
                end: text.len().min(1),
            },
            what: "there are no tags in it",
        }));
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
                        .map(|cell| {
                            if cell.span == ABSENT {
                                "-".into()
                            } else {
                                cell.text.to_string()
                            }
                        })
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
            read(
                text,
                "STMTTRN",
                &["DTPOSTED", "TRNAMT", "NAME", "MEMO", "CHECKNUM"]
            ),
            [
                ["20260105120000[-5:EST]", "-84.20", "TRADER JOE'S", "-", "-"],
                ["20260106", "-1.00", "B & C", "-", "-"],
            ],
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
            read(
                text,
                "ntry",
                &[
                    "Amt",
                    "cdtdbtind",
                    "BookgDt/Dt",
                    "Dt",
                    "RmtInf/Ustrd",
                    "Nope/Amt"
                ]
            ),
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
        assert_eq!(
            read("<A><R><x>1</x></A>", "R", &["x"]),
            [["broken: the record is never closed"]]
        );
        assert_eq!(
            read("a plain text file", "R", &["x"]),
            [["broken: there are no tags in it"]]
        );
        assert!(
            read("<A></A>", "R", &["x"]).is_empty(),
            "no records is an empty statement, not a broken one"
        );
        assert_eq!(read("<R><x>1</x></R><R><x>2</x></R>", "R", &["x"]).len(), 2);
    }

    #[test]
    fn the_reader_stops_when_asked() {
        let mut count = 0;
        scan(
            "<R/><R><x>1</x></R><R><x>2</x></R><R><x>3</x></R>",
            "R",
            &["x"],
            |_| {
                count += 1;
                count < 2
            },
        );
        assert_eq!(count, 2);
    }

    #[test]
    fn garbage_never_panics() {
        for text in [
            "",
            "<",
            ">",
            "<>",
            "</>",
            "<R>",
            "</R>",
            "<R><",
            "<!--",
            "<R><a>1</R>",
            "<R></a></R>",
            "<\u{ff}>",
        ] {
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
        scan(
            &text,
            "STMTTRN",
            &["DTPOSTED", "TRNAMT", "NAME", "MEMO"],
            |_| {
                count += 1;
                true
            },
        );
        tags += super::tags(&text).count();
        eprintln!(
            "found {count} records ({} MB, {tags} tags) in {:?}, tags alone included",
            text.len() >> 20,
            started.elapsed()
        );
    }
}
