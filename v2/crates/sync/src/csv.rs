//! CSV as banks write it: quoted fields with doubled quotes, CRLF, a byte-order
//! mark, a header row, and columns named by header or counted from 1. What
//! cannot be read is a diagnostic that points at the cell, never a panic.

use std::borrow::Cow;
use std::fmt;

use axiom_core::diag::closest;
use axiom_core::num::{Dec, DecError};
use axiom_core::{Day, Diagnostic, FileId, Loc, Qty};
use memchr::{memchr, memchr2};

use crate::{Record, Unit};

/// A bad column usually fails every row alike; after this many problems the
/// rest of the export is not read.
const MAX_PROBLEMS: usize = 8;

/// Where a value sits in a row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Column {
    /// By the header row's name, in any case.
    Name(String),
    /// By position, counting from 1.
    Index(usize),
}

impl Column {
    fn shown(&self) -> String {
        match self {
            Column::Name(name) => format!("\"{name}\""),
            Column::Index(index) => format!("column {index}"),
        }
    }
}

/// How an export gives amounts. Money into the account is positive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Amounts {
    /// One column, `flipped` for exports that show charges as positive.
    Signed { column: Column, flipped: bool },
    /// Money out and money in, each in a column of its own.
    Split { debit: Column, credit: Column },
}

/// What `csv date … amount … memo …` says about an export.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Csv {
    pub date: Column,
    pub format: DateFormat,
    pub amount: Amounts,
    pub memo: Column,
    pub balance: Option<Column>,
    /// A flag or a status word: `pending`, `true`, `yes`, `y`, `1` or `p` mean it is.
    pub pending: Option<Column>,
}

/// A date pattern: `YYYY-MM-DD`, `MM/DD/YYYY`, `DD.MM.YYYY`, `M/D/YY`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DateFormat(Vec<Part>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Part {
    Year { digits: usize },
    /// `MM` is exactly two digits, `M` one or two.
    Month { min: usize },
    Day { min: usize },
    Literal(char),
}

const WORDS: [(&str, Part); 6] = [
    ("YYYY", Part::Year { digits: 4 }),
    ("YY", Part::Year { digits: 2 }),
    ("MM", Part::Month { min: 2 }),
    ("M", Part::Month { min: 1 }),
    ("DD", Part::Day { min: 2 }),
    ("D", Part::Day { min: 1 }),
];

impl DateFormat {
    pub fn new(pattern: &str) -> Result<DateFormat, String> {
        let mut parts = Vec::new();
        let mut rest = pattern;
        while let Some(next) = rest.chars().next() {
            let word = WORDS.iter().find(|(word, _)| rest.starts_with(word));
            let (length, part) = match word {
                Some(&(word, part)) => (word.len(), part),
                None if next.is_ascii_alphanumeric() => {
                    return Err(format!("`{next}` means nothing in a date pattern: use YYYY, YY, MM, M, DD or D"));
                }
                None => (next.len_utf8(), Part::Literal(next)),
            };
            parts.push(part);
            rest = &rest[length..];
        }
        let named = |is: fn(&Part) -> bool| parts.iter().filter(|part| is(part)).count();
        let once = named(|p| matches!(p, Part::Year { .. })) == 1
            && named(|p| matches!(p, Part::Month { .. })) == 1
            && named(|p| matches!(p, Part::Day { .. })) == 1;
        if !once {
            return Err("a date pattern names the year, the month and the day once each".into());
        }
        Ok(DateFormat(parts))
    }

    /// The day `text` says, if it is exactly this pattern and a real date.
    pub fn read(&self, text: &str) -> Option<Day> {
        let bytes = text.trim().as_bytes();
        let (mut at, mut year, mut month, mut day) = (0, 0, 0, 0);
        for part in &self.0 {
            match *part {
                Part::Year { digits } => {
                    year = number(bytes, &mut at, digits, digits)?;
                    if digits == 2 {
                        year += if year < 70 { 2000 } else { 1900 };
                    }
                }
                Part::Month { min } => month = number(bytes, &mut at, min, 2)?,
                Part::Day { min } => day = number(bytes, &mut at, min, 2)?,
                Part::Literal(c) => {
                    let mut buffer = [0; 4];
                    let wanted = c.encode_utf8(&mut buffer).as_bytes();
                    (bytes.get(at..at + wanted.len())? == wanted).then_some(())?;
                    at += wanted.len();
                }
            }
        }
        (at == bytes.len()).then_some(())?;
        Day::from_ymd(year as i32, month, day)
    }

    /// The same pattern with the month and the day trading places: what a
    /// date that fails to read may have meant.
    fn swapped(&self) -> DateFormat {
        let swap = |part: &Part| match *part {
            Part::Month { min } => Part::Day { min },
            Part::Day { min } => Part::Month { min },
            other => other,
        };
        DateFormat(self.0.iter().map(swap).collect())
    }
}

impl fmt::Display for DateFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for part in &self.0 {
            match part {
                Part::Literal(c) => write!(f, "{c}")?,
                part => f.write_str(WORDS.iter().find(|(_, word)| word == part).map_or("", |(text, _)| text))?,
            }
        }
        Ok(())
    }
}

/// Between `min` and `max` digits at `at`, as many as there are.
fn number(bytes: &[u8], at: &mut usize, min: usize, max: usize) -> Option<u32> {
    let digits = bytes[*at..].iter().take(max).take_while(|b| b.is_ascii_digit()).count();
    (digits >= min).then_some(())?;
    let value = bytes[*at..*at + digits].iter().fold(0u32, |sum, b| sum * 10 + u32::from(b - b'0'));
    *at += digits;
    Some(value)
}

/// A range of the file, for a label.
#[derive(Clone, Copy)]
struct Span {
    start: usize,
    end: usize,
}

/// One cell: its text with the quotes taken off, and its place in the file.
struct Field<'t> {
    text: Cow<'t, str>,
    span: Span,
}

/// A row the reader lost the thread of, and where.
struct Broken {
    row: usize,
    span: Span,
    what: &'static str,
}

/// Rows of fields, borrowed from the text unless a doubled quote forces a copy.
struct Reader<'t> {
    text: &'t str,
    at: usize,
    row: usize,
}

impl<'t> Reader<'t> {
    fn new(text: &'t str) -> Reader<'t> {
        // Spreadsheet exports often start with a byte-order mark.
        Reader { text, at: if text.starts_with('\u{feff}') { 3 } else { 0 }, row: 0 }
    }

    /// The next row that is not blank, into `fields`.
    fn next(&mut self, fields: &mut Vec<Field<'t>>) -> Option<Result<(), Broken>> {
        while self.at < self.text.len() {
            self.row += 1;
            let read = self.read_row(fields);
            if read.is_err() || fields.len() > 1 || !fields[0].text.is_empty() {
                return Some(read);
            }
            self.row -= 1;
        }
        None
    }

    fn read_row(&mut self, fields: &mut Vec<Field<'t>>) -> Result<(), Broken> {
        fields.clear();
        let bytes = self.text.as_bytes();
        loop {
            let field = if bytes.get(self.at) == Some(&b'"') { self.quoted()? } else { self.plain() };
            fields.push(field);
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
    fn plain(&mut self) -> Field<'t> {
        let bytes = self.text.as_bytes();
        let end = memchr2(b',', b'\n', &bytes[self.at..]).map_or(bytes.len(), |found| self.at + found);
        let raw = &self.text[self.at..end];
        let text = raw.trim();
        let start = self.at + raw.len() - raw.trim_start().len();
        self.at = end;
        Field { text: Cow::Borrowed(text), span: Span { start, end: start + text.len() } }
    }

    /// A quoted field, where `""` is one quote. Only such a field is copied.
    fn quoted(&mut self) -> Result<Field<'t>, Broken> {
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
                None | Some(b',' | b'\n') => Ok(Field { text, span: Span { start: open, end: quote + 1 } }),
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


/// A problem in a row of the export, pointing at the cell.
fn problem(file: FileId, row: usize, code: &'static str, headline: String, span: Span, label: impl Into<String>) -> Diagnostic {
    let loc = Loc::new(file, span.start as u32, span.end.max(span.start + 1) as u32);
    Diagnostic::error(code, format!("row {row}: {headline}")).label(loc, label)
}

impl Broken {
    fn report(self, file: FileId) -> Diagnostic {
        problem(file, self.row, "bad-csv", self.what.into(), self.span, "here")
    }
}

/// One row, and where its problems are reported.
struct Row<'r, 't> {
    number: usize,
    file: FileId,
    fields: &'r [Field<'t>],
}

impl<'r, 't> Row<'r, 't> {
    fn error(&self, code: &'static str, headline: String, span: Span, label: impl Into<String>) -> Diagnostic {
        problem(self.file, self.number, code, headline, span, label)
    }

    fn cell(&self, at: At) -> Result<&'r Field<'t>, Diagnostic> {
        self.fields.get(at.index).ok_or_else(|| {
            let end = self.fields.last().map_or(0, |field| field.span.end);
            let headline = format!("has {} columns, but {} is number {}", self.fields.len(), at.column.shown(), at.index + 1);
            self.error("short-row", headline, Span { start: end, end }, "the row ends here")
        })
    }

    fn whole(&self) -> Span {
        let start = self.fields.first().map_or(0, |field| field.span.start);
        Span { start, end: self.fields.last().map_or(start, |field| field.span.end) }
    }

    /// Where `column` is, reading this row as the header.
    fn find<'c>(&self, column: &'c Column) -> Result<At<'c>, Diagnostic> {
        let index = match column {
            Column::Index(0) => return Err(self.error("bad-column", "columns are counted from 1".into(), self.whole(), "this row")),
            Column::Index(index) => index - 1,
            Column::Name(name) => self
                .fields
                .iter()
                .position(|field| field.text.eq_ignore_ascii_case(name))
                .ok_or_else(|| missing_column(self, name))?,
        };
        Ok(At { index, column })
    }
}

/// A column, and its position in the rows.
#[derive(Clone, Copy)]
struct At<'c> {
    index: usize,
    column: &'c Column,
}

struct Cells<'c> {
    date: At<'c>,
    memo: At<'c>,
    amount: Money<'c>,
    balance: Option<At<'c>>,
    pending: Option<At<'c>>,
}

enum Money<'c> {
    Signed { at: At<'c>, flipped: bool },
    Split { debit: At<'c>, credit: At<'c> },
}

/// What has been read so far, and when to stop reading.
#[derive(Default)]
struct Harvest<'t> {
    records: Vec<Record<'t>>,
    problems: Vec<Diagnostic>,
}

impl<'t> Harvest<'t> {
    /// Keeps the record or the problem; false once there are too many problems
    /// to go on.
    fn take(&mut self, read: Result<Record<'t>, Diagnostic>) -> bool {
        match read {
            Ok(record) => self.records.push(record),
            Err(problem) => self.problems.push(problem),
        }
        if self.problems.len() < MAX_PROBLEMS {
            return true;
        }
        let last = self.problems.last_mut().expect("there are problems");
        last.notes.push("the rest of the export was not read".into());
        false
    }
}

impl Csv {
    /// Every record of an export, and everything wrong with it. The first row
    /// is the header when any column is named by it; otherwise it is one only
    /// if its date is not a date.
    pub fn records<'t>(&self, text: &'t str, file: FileId, unit: Unit) -> (Vec<Record<'t>>, Vec<Diagnostic>) {
        let (mut reader, mut fields, mut harvest) = (Reader::new(text), Vec::new(), Harvest::default());
        let Some(first) = reader.next(&mut fields) else { return (Vec::new(), Vec::new()) };
        let row = Row { number: reader.row, file, fields: &fields };
        let located = first.map_err(|broken| broken.report(file)).and_then(|()| self.locate(&row));
        let (cells, header) = match located {
            Ok(found) => found,
            Err(problem) => return (Vec::new(), vec![problem]),
        };
        let mut more = header || harvest.take(self.record(&cells, &row, unit));
        while more {
            let Some(read) = reader.next(&mut fields) else { break };
            let row = Row { number: reader.row, file, fields: &fields };
            let read = read.map_err(|broken| broken.report(file)).and_then(|()| self.record(&cells, &row, unit));
            more = harvest.take(read);
        }
        (harvest.records, harvest.problems)
    }

    /// The positions of the columns, and whether the first row is a header.
    fn locate(&self, first: &Row) -> Result<(Cells<'_>, bool), Diagnostic> {
        let named = self.columns().iter().any(|column| matches!(column, Column::Name(_)));
        let find = |column| first.find(column);
        let amount = match &self.amount {
            Amounts::Signed { column, flipped } => Money::Signed { at: find(column)?, flipped: *flipped },
            Amounts::Split { debit, credit } => Money::Split { debit: find(debit)?, credit: find(credit)? },
        };
        let cells = Cells {
            date: find(&self.date)?,
            memo: find(&self.memo)?,
            amount,
            balance: self.balance.as_ref().map(find).transpose()?,
            pending: self.pending.as_ref().map(find).transpose()?,
        };
        let dateless = first.fields.get(cells.date.index).is_none_or(|cell| self.format.read(&cell.text).is_none());
        Ok((cells, named || dateless))
    }

    fn columns(&self) -> Vec<&Column> {
        let mut all = vec![&self.date, &self.memo];
        match &self.amount {
            Amounts::Signed { column, .. } => all.push(column),
            Amounts::Split { debit, credit } => all.extend([debit, credit]),
        }
        all.extend(self.balance.iter().chain(&self.pending));
        all
    }

    fn record<'t>(&self, cells: &Cells, row: &Row<'_, 't>, unit: Unit) -> Result<Record<'t>, Diagnostic> {
        let day = self.day(row, cells.date)?;
        let qty = match &cells.amount {
            Money::Signed { at, flipped } => {
                let qty = money(row, *at, unit)?.ok_or_else(|| no_amount(row, *at))?;
                if *flipped { -qty } else { qty }
            }
            Money::Split { debit, credit } => {
                let out = money(row, *debit, unit)?.unwrap_or_default().abs();
                let into = money(row, *credit, unit)?.unwrap_or_default().abs();
                if !out.is_zero() && !into.is_zero() {
                    let both = "both the debit and the credit are filled in".to_string();
                    return Err(row.error("bad-amount", both, row.whole(), "a row moves money one way"));
                }
                into - out
            }
        };
        let balance = cells.balance.map(|at| money(row, at, unit)).transpose()?.flatten();
        let pending = cells.pending.and_then(|at| row.fields.get(at.index)).is_some_and(|field| is_pending(&field.text));
        let memo = row.cell(cells.memo)?;
        let at = Loc::new(row.file, memo.span.start as u32, memo.span.end.max(memo.span.start + 1) as u32);
        Ok(Record { day, qty, memo: memo.text.clone(), balance, pending, at })
    }

    fn day(&self, row: &Row, at: At) -> Result<Day, Diagnostic> {
        let field = row.cell(at)?;
        self.format.read(&field.text).ok_or_else(|| {
            let headline = format!("`{}` is not a date written {}", field.text, self.format);
            let error = row.error("bad-date", headline, field.span, format!("in the {} column", at.column.shown()));
            match self.format.swapped().read(&field.text) {
                Some(_) => error.help(format!("if the day comes first, write the pattern as \"{}\"", self.format.swapped())),
                None => error,
            }
        })
    }
}

fn missing_column(first: &Row, name: &str) -> Diagnostic {
    let names: Vec<&str> = first.fields.iter().map(|field| &*field.text).collect();
    let listed = names.iter().map(|name| format!("\"{name}\"")).collect::<Vec<_>>().join(", ");
    let error = first
        .error("no-such-column", format!("the export has no column \"{name}\""), first.whole(), "the header row")
        .note(format!("its columns are {listed}"));
    match closest(name, names.iter().copied()) {
        Some(near) => error.help(format!("did you mean \"{near}\"?")),
        None => error,
    }
}

fn no_amount(row: &Row, at: At) -> Diagnostic {
    let span = row.fields.get(at.index).map_or(row.whole(), |field| field.span);
    row.error("bad-amount", "there is no amount".into(), span, format!("the {} column is empty", at.column.shown()))
}

/// The amount in a cell, if it holds one.
fn money(row: &Row, at: At, unit: Unit) -> Result<Option<Qty>, Diagnostic> {
    let field = row.cell(at)?;
    amount(&field.text, unit.scale).map_err(|why| why.report(row, field, at.column, unit))
}

/// What an export's own "pending" says: a flag or a status word.
fn is_pending(cell: &str) -> bool {
    ["pending", "true", "yes", "y", "1", "p"].iter().any(|word| cell.trim().eq_ignore_ascii_case(word))
}

/// Why a cell is not an amount.
enum Why {
    Malformed { comma_decimal: bool },
    Precision,
    Range,
}

impl Why {
    fn report(self, row: &Row, field: &Field, column: &Column, unit: Unit) -> Diagnostic {
        let label = format!("in the {} column", column.shown());
        let text = field.text.trim();
        match self {
            Why::Malformed { comma_decimal } => {
                let error = row.error("bad-amount", format!("`{text}` is not an amount"), field.span, label);
                match comma_decimal {
                    true => error
                        .note("a comma reads as a thousands separator, so `12,50` is not twelve and a half")
                        .help("ask the bank for an export that writes the decimal mark as a point"),
                    false => error,
                }
            }
            Why::Precision => {
                let headline = format!("`{text}` has more decimal places than {} keeps", unit.name);
                let error = row.error("bad-amount", headline, field.span, label);
                error.note(format!("{} is counted to {} decimal places", unit.name, unit.scale))
            }
            Why::Range => row.error("bad-amount", format!("`{text}` is too large to be an amount"), field.span, label),
        }
    }
}

/// `-1,234.56`, `(12.00)`, `$12`, `-$12`: a leading currency sign, thousands
/// separators, and parentheses or a minus for negatives. `None` for an empty
/// cell.
fn amount(cell: &str, scale: u8) -> Result<Option<Qty>, Why> {
    if cell.trim().is_empty() {
        return Ok(None);
    }
    let (negative, number) = sign(cell);
    let digits = Digits::of(number)?;
    let dec = Dec::parse(digits.as_bytes()).ok_or(Why::Range)?;
    let qty = dec.to_qty(scale).map_err(|why| match why {
        DecError::Inexact => Why::Precision,
        DecError::Range => Why::Range,
    })?;
    Ok(Some(if negative { -qty } else { qty }))
}

/// Whether the cell says negative, and the number without its sign: a minus or
/// parentheses make it so, and a currency sign is only decoration.
fn sign(cell: &str) -> (bool, &str) {
    let cell = cell.trim();
    let (mut negative, mut rest) = match cell.strip_prefix('(').and_then(|inner| inner.strip_suffix(')')) {
        Some(inner) => (true, inner),
        None => (false, cell),
    };
    while let Some(sign) = rest.chars().next().filter(|c| "-+$€£¥ ".contains(*c)) {
        negative |= sign == '-';
        rest = &rest[sign.len_utf8()..];
    }
    (negative, rest)
}

/// A number's digits and its point, with the thousands separators taken out.
struct Digits {
    bytes: [u8; 40],
    len: usize,
}

impl Digits {
    /// `1,234.5` as `1234.5`; a comma that is not between thousands is refused.
    fn of(number: &str) -> Result<Digits, Why> {
        let (whole, fraction) = number.split_once('.').unwrap_or((number, ""));
        let grouped = whole.contains(',');
        let group_ok = |(at, group): (usize, &str)| {
            let size = match (grouped, at) {
                (false, _) => true,
                (true, 0) => (1..=3).contains(&group.len()),
                (true, _) => group.len() == 3,
            };
            size && group.bytes().all(|b| b.is_ascii_digit())
        };
        let sound = whole.split(',').enumerate().all(group_ok)
            && fraction.bytes().all(|b| b.is_ascii_digit())
            && !(whole.is_empty() && fraction.is_empty());
        if !sound {
            let tail = whole.rsplit_once(',').map_or(0, |(_, tail)| tail.len());
            return Err(Why::Malformed { comma_decimal: fraction.is_empty() && matches!(tail, 1 | 2) });
        }
        let mut digits = Digits { bytes: [0; 40], len: 0 };
        if whole.is_empty() {
            digits.push(b'0')?;
        }
        for byte in whole.bytes().filter(|&byte| byte != b',') {
            digits.push(byte)?;
        }
        if !fraction.is_empty() {
            digits.push(b'.')?;
            for byte in fraction.bytes() {
                digits.push(byte)?;
            }
        }
        Ok(digits)
    }

    fn push(&mut self, byte: u8) -> Result<(), Why> {
        *self.bytes.get_mut(self.len).ok_or(Why::Range)? = byte;
        self.len += 1;
        Ok(())
    }

    fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const USD: Unit = Unit { name: "USD", scale: 2 };

    fn named(date: &str, format: &str, amount: &str, memo: &str) -> Csv {
        let name = |text: &str| Column::Name(text.into());
        Csv {
            date: name(date),
            format: DateFormat::new(format).unwrap(),
            amount: Amounts::Signed { column: name(amount), flipped: false },
            memo: name(memo),
            balance: Some(name("Balance")),
            pending: None,
        }
    }

    fn day(text: &str) -> Day {
        Day::parse(text.as_bytes()).unwrap()
    }

    #[test]
    fn a_bank_export_reads_quotes_crlf_and_the_byte_order_mark() {
        let text = "\u{feff}Posting Date,Description,Amount,Balance\r\n\
                    01/05/2026,\"TRADER JOE'S, #634 \"\"SF\"\"\",-84.20,\"8,915.80\"\r\n\
                    \r\n\
                    01/06/2026 , plain memo ,\"1,000.00\",9915.80\r\n";
        let (records, problems) = named("Posting Date", "MM/DD/YYYY", "Amount", "Description").records(text, FileId(0), USD);
        assert!(problems.is_empty(), "{problems:?}");
        let read: Vec<_> = records.iter().map(|r| (r.day, r.qty.0, r.memo.as_ref(), r.balance.map(|b| b.0))).collect();
        assert_eq!(
            read,
            [
                (day("2026-01-05"), -8420, "TRADER JOE'S, #634 \"SF\"", Some(891_580)),
                (day("2026-01-06"), 100_000, "plain memo", Some(991_580)),
            ]
        );
    }

    #[test]
    fn columns_by_position_need_no_header_but_may_have_one() {
        let by_number = |n| Column::Index(n);
        let csv = Csv {
            date: by_number(1),
            format: DateFormat::new("YYYY-MM-DD").unwrap(),
            amount: Amounts::Signed { column: by_number(4), flipped: true },
            memo: by_number(3),
            balance: None,
            pending: Some(by_number(2)),
        };
        for text in ["date,status,memo,amount\n2026-01-05,pending,Coffee,4.50\n", "2026-01-05,Pending,Coffee,4.50"] {
            let (records, problems) = csv.records(text, FileId(0), USD);
            assert!(problems.is_empty(), "{problems:?}");
            assert_eq!((records[0].qty.0, records[0].pending, records[0].memo.as_ref()), (-450, true, "Coffee"), "{text}");
        }
    }

    #[test]
    fn date_patterns() {
        let read = |pattern: &str, text: &str| DateFormat::new(pattern).unwrap().read(text);
        assert_eq!(read("YYYY-MM-DD", "2026-01-05"), Some(day("2026-01-05")));
        assert_eq!(read("MM/DD/YYYY", "01/05/2026"), Some(day("2026-01-05")));
        assert_eq!(read("DD.MM.YYYY", "05.01.2026"), Some(day("2026-01-05")));
        assert_eq!(read("M/D/YY", "1/5/26"), Some(day("2026-01-05")));
        assert_eq!(read("M/D/YY", "12/25/99"), Some(day("1999-12-25")));
        assert_eq!(read("MM/DD/YYYY", "1/5/2026"), None, "MM is two digits");
        assert_eq!(read("MM/DD/YYYY", "13/05/2026"), None);
        assert_eq!(read("YYYY-MM-DD", "2026-02-30"), None);
        assert_eq!(read("YYYY-MM-DD", "2026-01-05 12:00"), None, "the whole cell must be the date");
        assert!(DateFormat::new("yyyy-MM-DD").unwrap_err().contains("`y`"));
        assert!(DateFormat::new("YYYY-MM").unwrap_err().contains("once each"));
        assert_eq!(DateFormat::new("D.M.YYYY").unwrap().to_string(), "D.M.YYYY");
    }

    #[test]
    fn amounts_as_banks_write_them() {
        let qty = |text: &str| amount(text, 2).ok().flatten().map(|q| q.0);
        assert_eq!(qty("1,234.56"), Some(123_456));
        assert_eq!(qty("(12.00)"), Some(-1200));
        assert_eq!(qty("$12"), Some(1200));
        assert_eq!(qty("-$12.50"), Some(-1250));
        assert_eq!(qty("$-12.50"), Some(-1250));
        assert_eq!(qty("+ 7.5"), Some(750));
        assert_eq!(qty(".5"), Some(50));
        assert_eq!(qty("1,234,567"), Some(123_456_700));
        assert_eq!(amount("", 2).ok(), Some(None));
        for bad in ["12,5", "1,23.00", "12.3.4", "abc", "$", "1 000", "1.234,56"] {
            assert!(matches!(amount(bad, 2), Err(Why::Malformed { .. })), "{bad}");
        }
        assert!(matches!(amount("0.005", 2), Err(Why::Precision)));
        assert!(matches!(amount("9".repeat(30).as_str(), 2), Err(Why::Range)));
        assert!(matches!(amount("12,50", 2), Err(Why::Malformed { comma_decimal: true })));
    }

    #[test]
    fn debit_and_credit_columns() {
        let name = |text: &str| Column::Name(text.into());
        let csv = Csv {
            date: name("Date"),
            format: DateFormat::new("YYYY-MM-DD").unwrap(),
            amount: Amounts::Split { debit: name("Debit"), credit: name("Credit") },
            memo: name("Memo"),
            balance: None,
            pending: None,
        };
        let text = "Date,Memo,Debit,Credit\n2026-01-05,rent,2900.00,\n2026-01-06,pay,,3054.70\n2026-01-07,both,1.00,2.00\n";
        let (records, problems) = csv.records(text, FileId(0), USD);
        assert_eq!(records.iter().map(|r| r.qty.0).collect::<Vec<_>>(), [-290_000, 305_470]);
        assert_eq!(problems.len(), 1);
        assert!(problems[0].message.starts_with("row 4: both the debit and the credit"), "{}", problems[0].message);
    }

    #[test]
    fn what_cannot_be_read_is_reported_at_its_cell() {
        let csv = named("Posting Date", "MM/DD/YYYY", "Amount", "Description");
        let text = "Posting Date,Description,Amount,Balance\n\
                    25/12/2026,a,1.00,1\n\
                    01/05/2026,b,twelve,1\n\
                    01/06/2026,\"c\" x,1.00,1\n\
                    01/07/2026,short\n\
                    01/08/2026,\"never closed,1.00,1\n";
        let (records, problems) = csv.records(text, FileId(3), USD);
        assert!(records.is_empty());
        let shown: Vec<_> = problems.iter().map(|p| (p.code.as_ref(), p.message.as_str())).collect();
        assert_eq!(
            shown,
            [
                ("bad-date", "row 2: `25/12/2026` is not a date written MM/DD/YYYY"),
                ("bad-amount", "row 3: `twelve` is not an amount"),
                ("bad-csv", "row 4: text follows the closing quote"),
                ("short-row", "row 5: has 2 columns, but \"Amount\" is number 3"),
                ("bad-csv", "row 6: the quote is never closed"),
            ]
        );
        assert_eq!(problems[0].help[0].text, "if the day comes first, write the pattern as \"DD/MM/YYYY\"");
        let cell = problems[1].anchor().unwrap();
        assert_eq!((cell.file, &text[cell.range()]), (FileId(3), "twelve"));
    }

    #[test]
    fn a_missing_column_says_what_the_export_has() {
        let csv = named("Posting Dat", "MM/DD/YYYY", "Amount", "Description");
        let (records, problems) = csv.records("Posting Date,Description,Amount,Balance\n01/05/2026,a,1.00,1\n", FileId(0), USD);
        assert!(records.is_empty());
        assert_eq!(problems.len(), 1, "one root cause, one problem");
        assert_eq!(problems[0].message, "row 1: the export has no column \"Posting Dat\"");
        assert_eq!(problems[0].notes[0], "its columns are \"Posting Date\", \"Description\", \"Amount\", \"Balance\"");
        assert_eq!(problems[0].help[0].text, "did you mean \"Posting Date\"?");
    }

    #[test]
    fn a_column_that_fails_every_row_stops_the_reading() {
        let csv = named("Date", "YYYY-MM-DD", "Amount", "Memo");
        let text = format!("Date,Memo,Amount,Balance\n{}", "01/05/2026,a,1.00,1\n".repeat(50));
        let (_, problems) = csv.records(&text, FileId(0), USD);
        assert_eq!(problems.len(), MAX_PROBLEMS);
        assert_eq!(problems.last().unwrap().notes, ["the rest of the export was not read"]);
    }

    #[test]
    fn garbage_never_panics() {
        let csv = named("A", "YYYY-MM-DD", "B", "C");
        for text in ["", "\n\n", "\"", "A,B,C\n\"", ",,,\n,,", "A,B,C\n\u{0}\u{1},\u{ff}", "\u{feff}", "A\n,\"\"\"\"\"\n"] {
            let _ = csv.records(text, FileId(0), USD);
        }
    }
}
