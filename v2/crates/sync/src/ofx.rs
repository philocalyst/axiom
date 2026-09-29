//! OFX and QFX, which banks and card issuers let one download. Version 1 is
//! SGML, where a tag has no closing tag and its value runs to the next `<`;
//! version 2 is XML. A transaction is a `<STMTTRN>` in either.

use std::borrow::Cow;

use axiom_core::{Day, Diagnostic, FileId};
use memchr::memchr;

use crate::amount::amount;
use crate::statement::Statement;
use crate::{Record, Span, Unit};

/// A tag, and the text after it up to the next tag.
#[derive(Clone, Copy)]
struct Tag<'t> {
    name: &'t str,
    closing: bool,
    /// The tag itself.
    at: Span,
    value: &'t str,
    /// Where the value is, without the spaces around it.
    span: Span,
}

fn tags(text: &str) -> impl Iterator<Item = Tag<'_>> {
    let bytes = text.as_bytes();
    let mut from = 0;
    std::iter::from_fn(move || {
        let open = from + memchr(b'<', &bytes[from..])?;
        let close = open + memchr(b'>', &bytes[open..])?;
        let next = close + 1 + memchr(b'<', &bytes[close + 1..]).unwrap_or(bytes.len() - close - 1);
        from = next;
        let raw = text[open + 1..close].trim();
        let (closing, name) = raw.strip_prefix('/').map_or((false, raw), |name| (true, name));
        let (all, value) = (&text[close + 1..next], text[close + 1..next].trim());
        let start = close + 1 + all.len() - all.trim_start().len();
        Some(Tag {
            name,
            closing,
            at: Span { start: open, end: close + 1 },
            value,
            span: Span { start, end: start + value.len() },
        })
    })
}

/// What one `<STMTTRN>` says.
#[derive(Default)]
struct Transaction<'t> {
    at: Option<Span>,
    posted: Option<Tag<'t>>,
    amount: Option<Tag<'t>>,
    name: Option<Tag<'t>>,
    memo: Option<Tag<'t>>,
    check: Option<Tag<'t>>,
}

/// The records of a statement, its closing balance, and what is wrong with it.
pub fn read<'t>(text: &'t str, file: FileId, unit: Unit) -> (Statement<'t>, Vec<Diagnostic>) {
    let (mut records, mut problems) = (Vec::new(), Vec::new());
    let (mut card, mut in_balance, mut ledger, mut saw_ofx) = (false, false, (None, None), false);
    let (mut current, mut count) = (None, 0);
    for tag in tags(text) {
        let name = tag.name.to_ascii_uppercase();
        match (name.as_str(), tag.closing, current.as_mut()) {
            ("OFX", false, _) => saw_ofx = true,
            ("CCSTMTRS", false, _) => card = true,
            ("LEDGERBAL", closing, _) => in_balance = !closing,
            ("BALAMT", false, _) if in_balance => ledger.0 = Some(tag),
            ("DTASOF", false, _) if in_balance => ledger.1 = Some(tag),
            ("STMTTRN", false, _) => {
                count += 1;
                current = Some(Transaction { at: Some(tag.at), ..Transaction::default() });
            }
            ("STMTTRN", true, Some(_)) => {
                let done: Transaction = current.take().unwrap_or_default();
                match done.record(count, file, unit) {
                    Ok(record) => records.push(record),
                    Err(problem) => problems.push(problem),
                }
            }
            ("DTPOSTED", false, Some(txn)) => txn.posted = Some(tag),
            ("TRNAMT", false, Some(txn)) => txn.amount = Some(tag),
            ("NAME", false, Some(txn)) => txn.name = Some(tag),
            ("MEMO", false, Some(txn)) => txn.memo = Some(tag),
            ("CHECKNUM", false, Some(txn)) => txn.check = Some(tag),
            _ => {}
        }
    }
    if !saw_ofx {
        let whole = Span { start: 0, end: text.len().min(1) };
        problems.push(
            Diagnostic::error("not-ofx", "this is not an OFX or QFX statement: it has no <OFX>")
                .label(whole.loc(file), "the file starts here"),
        );
    }
    let closing = ledger.0.and_then(|balance| {
        let parsed = amount(balance.value, unit.scale).ok().flatten();
        if parsed.is_none() {
            let place = "the ledger balance";
            let problem = amount(balance.value, unit.scale).err();
            problems.extend(
                problem
                    .map(|why| why.diagnostic(place, balance.span.loc(file), "in BALAMT".into(), balance.value, unit)),
            );
        }
        // A card statement gives what is owed as negative; an assertion states it as positive.
        let qty = parsed.map(|qty| if card { -qty } else { qty })?;
        let day =
            ledger.1.and_then(|asof| date(asof.value)).or_else(|| records.iter().map(|record| record.day).max())?;
        Some((day, qty))
    });
    (Statement { records, closing }, problems)
}

impl<'t> Transaction<'t> {
    fn record(self, number: usize, file: FileId, unit: Unit) -> Result<Record<'t>, Diagnostic> {
        let place = format!("transaction {number}");
        let whole = self.at.unwrap_or(Span { start: 0, end: 1 });
        let missing = |what: &str| {
            Diagnostic::error("missing-tag", format!("{place}: it has no {what}"))
                .label(whole.loc(file), "this transaction")
        };
        let posted = self.posted.ok_or_else(|| missing("DTPOSTED"))?;
        let day = date(posted.value).ok_or_else(|| {
            let headline = format!("{place}: `{}` is not a date", posted.value);
            Diagnostic::error("bad-date", headline)
                .label(posted.span.loc(file), "in DTPOSTED")
                .help("OFX writes dates as YYYYMMDD")
        })?;
        let paid = self.amount.ok_or_else(|| missing("TRNAMT"))?;
        let qty = amount(paid.value, unit.scale)
            .map_err(|why| why.diagnostic(&place, paid.span.loc(file), "in TRNAMT".into(), paid.value, unit))?
            .ok_or_else(|| missing("TRNAMT"))?;
        // What a person would call the memo: the payee, what was said of it, and a check's number.
        let check = self.check.map(|tag| format!("CHECK {}", tag.value));
        let words: Vec<&str> =
            [self.name, self.memo].into_iter().flatten().map(|tag| tag.value).filter(|text| !text.is_empty()).collect();
        let memo = match (words.as_slice(), check) {
            ([only], None) => decode(only),
            (words, check) => Cow::Owned(
                words.iter().map(|word| decode(word).into_owned()).chain(check).collect::<Vec<_>>().join(" "),
            ),
        };
        let at = self.name.or(self.memo).map_or(whole, |tag| tag.span).loc(file);
        Ok(Record { day, qty, memo, balance: None, pending: false, at })
    }
}

/// `YYYYMMDD`, and anything after it: a time and a zone that a day does not need.
fn date(text: &str) -> Option<Day> {
    let part = |from: usize, to: usize| {
        text.get(from..to)?.bytes().all(|b| b.is_ascii_digit()).then(|| text[from..to].parse::<u32>().ok()).flatten()
    };
    Day::from_ymd(part(0, 4)? as i32, part(4, 6)?, part(6, 8)?)
}

/// The five escapes OFX inherits from SGML and XML.
fn decode(text: &str) -> Cow<'_, str> {
    if !text.contains('&') {
        return Cow::Borrowed(text);
    }
    let escapes = [("&lt;", "<"), ("&gt;", ">"), ("&quot;", "\""), ("&apos;", "'"), ("&amp;", "&")];
    Cow::Owned(escapes.iter().fold(text.to_string(), |text, (escape, plain)| text.replace(escape, plain)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const USD: Unit = Unit { name: "USD", scale: 2 };

    /// A version 1 file: SGML, tags unclosed.
    const CHECKING: &str = "OFXHEADER:100\nDATA:OFXSGML\nVERSION:102\n\n\
<OFX>\n<BANKMSGSRSV1><STMTTRNRS><STMTRS><CURDEF>USD\n<BANKTRANLIST>\n\
<STMTTRN>\n<TRNTYPE>DEBIT\n<DTPOSTED>20260105120000[-5:EST]\n<TRNAMT>-84.20\n<FITID>2026010501\n<NAME>TRADER JOE'S #634\n<MEMO>SAN FRANCISCO CA\n</STMTTRN>\n\
<STMTTRN>\n<TRNTYPE>CHECK\n<DTPOSTED>20260106\n<TRNAMT>-350.00\n<FITID>2026010601\n<CHECKNUM>1041\n<NAME>BAY PLUMBING &amp; HEATING\n</STMTTRN>\n\
<STMTTRN>\n<TRNTYPE>CREDIT\n<DTPOSTED>20260108\n<TRNAMT>3800.00\n<FITID>2026010801\n<NAME>HALCYON PAYMENT\n</STMTTRN>\n\
</BANKTRANLIST>\n<LEDGERBAL><BALAMT>3162.55\n<DTASOF>20260112120000\n</LEDGERBAL>\n</STMTRS></STMTTRNRS></BANKMSGSRSV1>\n</OFX>\n";

    #[test]
    fn a_version_1_statement_is_its_transactions_and_its_ledger_balance() {
        let (statement, problems) = read(CHECKING, FileId(0), USD);
        assert!(problems.is_empty(), "{problems:?}");
        let shown: Vec<_> =
            statement.records.iter().map(|r| (r.day.to_string(), r.qty.0, r.memo.to_string())).collect();
        assert_eq!(
            shown,
            [
                ("2026-01-05".to_string(), -8420, "TRADER JOE'S #634 SAN FRANCISCO CA".to_string()),
                ("2026-01-06".to_string(), -35_000, "BAY PLUMBING & HEATING CHECK 1041".to_string()),
                ("2026-01-08".to_string(), 380_000, "HALCYON PAYMENT".to_string()),
            ]
        );
        assert_eq!(
            statement.closing.map(|(day, qty)| (day.to_string(), qty.0)),
            Some(("2026-01-12".to_string(), 316_255))
        );
    }

    #[test]
    fn a_version_2_card_statement_owes_what_it_shows_as_negative() {
        let text = "<?xml version=\"1.0\"?><OFX><CREDITCARDMSGSRSV1><CCSTMTTRNRS><CCSTMTRS><BANKTRANLIST>\
<STMTTRN><TRNTYPE>DEBIT</TRNTYPE><DTPOSTED>20260107</DTPOSTED><TRNAMT>-84.20</TRNAMT><NAME>TRADER JOE'S</NAME></STMTTRN>\
</BANKTRANLIST><LEDGERBAL><BALAMT>-1324.38</BALAMT><DTASOF>20260107</DTASOF></LEDGERBAL></CCSTMTRS></CCSTMTTRNRS></CREDITCARDMSGSRSV1></OFX>";
        let (statement, problems) = read(text, FileId(0), USD);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(statement.records[0].qty.0, -8420);
        assert_eq!(
            statement.closing.map(|(day, qty)| (day.to_string(), qty.0)),
            Some(("2026-01-07".to_string(), 132_438))
        );
    }

    #[test]
    fn what_cannot_be_read_is_reported_at_the_tag() {
        let text = "<OFX><STMTTRN><DTPOSTED>soon<TRNAMT>1.00<NAME>a</STMTTRN>\
<STMTTRN><DTPOSTED>20260105<TRNAMT>12,5<NAME>b</STMTTRN>\
<STMTTRN><DTPOSTED>20260105<NAME>c</STMTTRN>\
<STMTTRN><TRNAMT>1.00</STMTTRN></OFX>";
        let (statement, problems) = read(text, FileId(4), USD);
        assert!(statement.records.is_empty());
        let shown: Vec<_> = problems.iter().map(|problem| problem.message.as_str()).collect();
        assert_eq!(
            shown,
            [
                "transaction 1: `soon` is not a date",
                "transaction 2: `12,5` is not an amount",
                "transaction 3: it has no TRNAMT",
                "transaction 4: it has no DTPOSTED",
            ]
        );
        let at = problems[1].anchor().unwrap();
        assert_eq!((at.file, &text[at.range()]), (FileId(4), "12,5"));
        let (_, problems) = read("Posting Date,Amount\n", FileId(0), USD);
        assert_eq!(problems[0].message, "this is not an OFX or QFX statement: it has no <OFX>");
    }
}
