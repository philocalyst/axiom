//! Runtime readers for the model's canonical format declarations. This module
//! keeps row/tag cells borrowed and only stores resolved column offsets; it
//! does not define or validate a second format schema.

use std::borrow::Cow;

use axiom_core::diag::closest;
use axiom_core::{Day, Diagnostic, FileId, Id, Qty, calendar::DateLayout};
use axiom_model::Text;
use axiom_model::sync::{Column, Fetch, Field, Format, Rule, Shape, Source, Spec};
use axiom_model::{Book, Purpose};

use crate::amount::amount;
use crate::cell::{ABSENT, Cell, MemoJoin};
use crate::csv::{Broken, Reader as CsvReader};
use crate::date::iso_day;
use crate::{Facts, Record, Span, Unit, tagged};

const MAX_PROBLEMS: usize = 8;
const FIELDS: usize = 17;
#[derive(Default)]
struct Harvest<'t> {
    records: Vec<Record<'t>>,
    problems: Vec<Diagnostic>,
}

impl<'t> Harvest<'t> {
    fn take(&mut self, read: Result<Record<'t>, RowError>) -> bool {
        match read {
            Ok(record) => self.records.push(record),
            Err(problem) => self.problems.push(*problem),
        }
        if self.problems.len() < MAX_PROBLEMS {
            return true;
        }
        if let Some(last) = self.problems.last_mut() {
            last.notes.push("the rest of the export was not read".into());
        }
        false
    }
}

struct Row<'r, 't, 'n, 's> {
    number: usize,
    file: FileId,
    what: &'static str,
    cells: &'r [Cell<'t>],
    whole: Span,
    book: &'n Book<'s>,
}

/// What is wrong with a row, boxed so that the result of reading a cell or a field stays small.
type RowError = Box<Diagnostic>;

impl<'t> Row<'_, 't, '_, '_> {
    fn error(&self, code: &'static str, headline: String, span: Span, label: impl Into<String>) -> Diagnostic {
        let span = if span == ABSENT { self.whole } else { span };
        Diagnostic::error(code, format!("{} {}: {headline}", self.what, self.number)).label(span.loc(self.file), label)
    }

    fn shown(&self, column: Column) -> String {
        match column {
            Column::Header(name) => format!("the \"{}\" column", self.book.text(name)),
            Column::Index(index) => format!("column {index}"),
            Column::Path(path) => self.book.text(path).to_string(),
        }
    }

    fn cell<'r>(&'r self, bound: &Bound, place: usize) -> Result<&'r Cell<'t>, RowError>
    where
        't: 'r,
    {
        self.cells.get(bound.slots[place]).ok_or_else(move || {
            let end = self.cells.last().map_or(0, |cell| cell.span.end);
            let column = bound.spec.places[place];
            let headline = format!("has {} columns, but {} is missing", self.cells.len(), self.shown(column));
            Box::new(self.error("short-row", headline, Span { start: end, end }, "the row ends here"))
        })
    }

    fn label(&self, bound: &Bound, place: usize) -> String {
        format!("in {}", self.shown(bound.spec.places[place]))
    }

    fn first<'r>(&'r self, bound: &Bound) -> Result<Option<&'r Cell<'t>>, RowError>
    where
        't: 'r,
    {
        for place in 0..bound.slots.len() {
            let cell = self.cell(bound, place)?;
            if !cell.text.is_empty() {
                return Ok(Some(cell));
            }
        }
        Ok(None)
    }

    fn money<'r>(&'r self, bound: &Bound, unit: Unit<'_>) -> Result<Option<Qty>, RowError>
    where
        't: 'r,
    {
        let cell = self.cell(bound, 0)?;
        let span = if cell.span == ABSENT { self.whole } else { cell.span };
        amount(&cell.text, unit.scale).map_err(|why| {
            let place = format!("{} {}", self.what, self.number);
            Box::new(why.diagnostic(&place, span.loc(self.file), self.label(bound, 0), &cell.text, unit))
        })
    }
}

struct Bound<'f> {
    spec: &'f Spec,
    slots: Vec<usize>,
    sign: Option<usize>,
}

struct Plan<'f>([Option<Bound<'f>>; FIELDS]);

impl<'f> Plan<'f> {
    fn of(&self, field: Field) -> Option<&Bound<'f>> {
        self.0[field as usize].as_ref()
    }
}

/// The day a cell says, in the layout its field declares, else as `YYYY-MM-DD`.
fn day_of(cell: &Cell<'_>, spec: Option<&Spec>) -> Option<Day> {
    match spec.and_then(|spec| spec.layout.as_ref()) {
        Some(layout) => layout.read(&cell.text),
        None => iso_day(&cell.text),
    }
}

struct Reader<'f, 'n, 's> {
    format: &'f Format,
    book: &'n Book<'s>,
}

impl<'f, 'n, 's> Reader<'f, 'n, 's> {
    fn spec(&self, field: Field) -> Option<&'f Spec> {
        self.format.specs.iter().find(|spec| spec.field == field)
    }

    fn plan(&self, mut locate: impl FnMut(Column) -> Result<usize, RowError>) -> Result<Plan<'f>, RowError> {
        let mut plan = Plan(std::array::from_fn(|_| None));
        for spec in self.format.specs.iter() {
            let slots = spec.places.iter().copied().map(&mut locate).collect::<Result<Vec<_>, _>>()?;
            let sign = match spec.rule {
                Rule::Sign { place, .. } => Some(locate(place)?),
                _ => None,
            };
            plan.0[spec.field as usize] = Some(Bound { spec, slots, sign });
        }
        Ok(plan)
    }

    fn rows<'t>(&self, text: &'t str, file: FileId, units: Units, out: &mut Harvest<'t>) {
        let mut csv = CsvReader::new(text);
        let mut cells = Vec::new();
        let Some(first) = csv.next(&mut cells) else {
            return;
        };
        let header = Row { number: csv.row, file, what: "row", cells: &cells, whole: whole(&cells), book: self.book };
        if let Err(broken) = first {
            out.problems.push(header.error("bad-csv", broken.what.into(), broken.span, "here"));
            return;
        }
        let plan = match self.plan(|column| self.in_header(&header, column)) {
            Ok(plan) => plan,
            Err(problem) => {
                out.problems.push(*problem);
                return;
            }
        };
        let mut more =
            !self.is_a_record(&plan, &header) || out.take(Fields { plan: &plan, row: &header }.record(units));
        while more {
            let Some(read) = csv.next(&mut cells) else {
                break;
            };
            let row = Row { number: csv.row, file, what: "row", cells: &cells, whole: whole(&cells), book: self.book };
            let record = match read {
                Ok(()) => Fields { plan: &plan, row: &row }.record(units),
                Err(broken) => Err(row.error("bad-csv", broken.what.into(), broken.span, "here").into()),
            };
            more = out.take(record);
        }
    }

    /// Where a column is in an export that has a header row: an index counts from 1, and a header is looked for
    /// in `header`.
    fn in_header(&self, header: &Row, column: Column) -> Result<usize, RowError> {
        match column {
            Column::Index(index) if index > 0 => Ok(usize::from(index) - 1),
            Column::Index(_) => Err(bad_format("columns are counted from 1").into()),
            Column::Header(name) => {
                let name = self.book.text(name);
                let found = header.cells.iter().position(|cell| cell.text.eq_ignore_ascii_case(name));
                found.ok_or_else(|| no_such_column(header, name).into())
            }
            Column::Path(_) => Err(bad_format("a rows format names columns").into()),
        }
    }

    /// Whether the first row of an export is a record, and not the header: no column is found by its header,
    /// and the row has a day where the date goes.
    fn is_a_record(&self, plan: &Plan, first: &Row) -> bool {
        let dated = plan.of(Field::Date).and_then(|bound| first.cell(bound, 0).ok());
        !names_a_header(self.format) && dated.is_some_and(|cell| day_of(cell, self.spec(Field::Date)).is_some())
    }

    fn tagged<'t>(&self, record_name: &str, text: &'t str, file: FileId, units: Units, out: &mut Harvest<'t>) {
        let paths = distinct_paths(self.book, self.columns());
        let locate = |column: Column| -> Result<usize, RowError> {
            match column {
                Column::Path(path) => Ok(path_slot(self.book, &paths, path)),
                _ => Err(bad_format("a tagged format names paths").into()),
            }
        };
        let plan = match self.plan(locate) {
            Ok(plan) => plan,
            Err(problem) => {
                out.problems.push(*problem);
                return;
            }
        };
        tagged::scan(text, record_name, &paths, |found| {
            let record = match found {
                Ok(found) => {
                    let row = Row {
                        number: found.number,
                        file,
                        what: "record",
                        cells: found.cells,
                        whole: found.whole,
                        book: self.book,
                    };
                    Fields { plan: &plan, row: &row }.record(units)
                }
                Err(broken) => Err(bad_tags(&broken, file).into()),
            };
            out.take(record)
        });
    }

    /// Every column the format reads, field by field: the places of a field, then where its sign is.
    fn columns(&self) -> impl Iterator<Item = Column> {
        self.format.specs.iter().flat_map(|spec| {
            let sign = match spec.rule {
                Rule::Sign { place, .. } => Some(place),
                _ => None,
            };
            spec.places.iter().copied().chain(sign)
        })
    }
}

/// What an export's amounts can be in: the unit of the account it is of, and the units the book has, which a
/// record may name instead.
#[derive(Clone, Copy)]
struct Units<'a, 'u> {
    account: Unit<'u>,
    book: &'a [Unit<'u>],
}

/// What the format says of one row or record: where each field is, and the cells to read them from.
struct Fields<'a, 'r, 't, 'n, 's> {
    plan: &'a Plan<'a>,
    row: &'a Row<'r, 't, 'n, 's>,
}

impl<'t> Fields<'_, '_, 't, '_, '_> {
    /// The record the row holds. Each field is read in the order the faults are reported in.
    fn record(&self, units: Units) -> Result<Record<'t>, RowError> {
        let mut facts = Facts::default();
        let currency = self.text(Field::Currency)?.filter(|code| !code.eq_ignore_ascii_case(units.account.name));
        let unit = self.unit(currency.as_deref(), units)?;
        facts.currency = currency.map(uppercase);
        let day = self.day()?;
        let (gross, fee) = (self.money(Field::Gross, unit)?, self.money(Field::Fee, unit)?);
        let qty = self.qty(unit, gross, fee)?;
        (facts.gross, facts.fee) = (gross.map(Qty::abs), fee.map(Qty::abs));
        let balance = self.money(Field::Balance, unit)?;
        let pending = self.pending()?;
        let (memo, memo_span) = self.memo()?;
        let at = if memo_span == ABSENT { self.row.whole } else { memo_span }.loc(self.row.file);
        facts.code = self.text(Field::Code)?.and_then(code_of);
        for (field, slot) in [
            (Field::Id, &mut facts.id),
            (Field::Party, &mut facts.party),
            (Field::Via, &mut facts.via),
            (Field::Category, &mut facts.category),
            (Field::Object, &mut facts.object),
            (Field::Route, &mut facts.route),
        ] {
            *slot = self.text(field)?;
        }
        let facts = (facts != Facts::default()).then(|| Box::new(facts));
        Ok(Record { day, qty, memo, balance, pending, at, facts })
    }

    /// The first non-empty text at a field's places.
    fn text(&self, field: Field) -> Result<Option<Cow<'t, str>>, RowError> {
        let cell = self.plan.of(field).map(|bound| self.row.first(bound)).transpose()?.flatten();
        Ok(cell.map(|cell| cell.text.clone()))
    }

    fn money(&self, field: Field, unit: Unit<'_>) -> Result<Option<Qty>, RowError> {
        self.plan.of(field).map(|bound| self.row.money(bound, unit)).transpose().map(Option::flatten)
    }

    /// What the amounts are in: the account's unit, unless the row names a currency the book has.
    fn unit<'u>(&self, currency: Option<&str>, units: Units<'_, 'u>) -> Result<Unit<'u>, RowError> {
        let Some(code) = currency else { return Ok(units.account) };
        if let Some(&known) = units.book.iter().find(|known| known.name.eq_ignore_ascii_case(code)) {
            return Ok(known);
        }
        let cell = self.plan.of(Field::Currency).map(|bound| self.row.cell(bound, 0)).transpose()?;
        let headline = format!("the book has no unit `{}`", code.to_uppercase());
        Err(self.row.error("bad-currency", headline, cell.map_or(ABSENT, |cell| cell.span), "the currency").into())
    }

    fn day(&self) -> Result<Day, RowError> {
        let Some(date) = self.plan.of(Field::Date) else {
            return Err(Diagnostic::error("bad-format", "the compiled feed format has no date field").into());
        };
        let cell = self.row.cell(date, 0)?;
        day_of(cell, Some(date.spec)).ok_or_else(|| self.bad_date(date, cell).into())
    }

    fn bad_date(&self, date: &Bound, cell: &Cell<'_>) -> Diagnostic {
        let row = self.row;
        let shown = row.shown(date.spec.places[0]);
        if cell.span == ABSENT {
            return row.error("missing-field", format!("it has no {shown}"), ABSENT, "this record");
        }
        let layout = date.spec.layout.as_ref().map_or_else(|| "YYYY-MM-DD".to_string(), ToString::to_string);
        let error = row.error(
            "bad-date",
            format!("`{}` is not a date written {layout}", cell.text),
            cell.span,
            row.label(date, 0),
        );
        match date.spec.layout.as_ref().map(DateLayout::swapped).filter(|swapped| swapped.read(&cell.text).is_some()) {
            Some(swapped) => error.help(format!("if the day comes first, write the pattern as \"{swapped}\"")),
            None => error,
        }
    }

    /// The signed amount into the account: as written, else a debit and a credit, else the gross less the fee.
    fn qty(&self, unit: Unit<'_>, gross: Option<Qty>, fee: Option<Qty>) -> Result<Qty, RowError> {
        if let Some(amount) = self.plan.of(Field::Amount) {
            return self.written(amount, unit);
        }
        if let (Some(debit), Some(credit)) = (self.plan.of(Field::Debit), self.plan.of(Field::Credit)) {
            return self.debited(debit, credit, unit);
        }
        let gross = gross.ok_or_else(|| {
            Box::new(self.row.error("bad-amount", "there is no amount".into(), ABSENT, "this record"))
        })?;
        Ok(gross - fee.unwrap_or_default().abs())
    }

    /// An amount column, and what its rule does to the sign.
    fn written(&self, amount: &Bound, unit: Unit<'_>) -> Result<Qty, RowError> {
        let row = self.row;
        let qty = row.money(amount, unit)?.ok_or_else(|| {
            let cell = row.cell(amount, 0).map_or(ABSENT, |cell| cell.span);
            let label = format!("{} is empty", row.shown(amount.spec.places[0]));
            Box::new(row.error("bad-amount", "there is no amount".into(), cell, label))
        })?;
        match amount.spec.rule {
            Rule::Flipped => Ok(-qty),
            Rule::Sign { into, .. } => {
                let sign = amount.sign.and_then(|slot| row.cells.get(slot)).filter(|cell| !cell.text.is_empty());
                let Some(sign) = sign else {
                    let headline = "it has no sign for its amount".into();
                    return Err(row.error("missing-field", headline, ABSENT, "this record").into());
                };
                Ok(if sign.text.eq_ignore_ascii_case(row.book.text(into)) { qty.abs() } else { -qty.abs() })
            }
            _ => Ok(qty),
        }
    }

    /// Money out of the account in one column and into it in another: a row moves it one way.
    fn debited(&self, debit: &Bound, credit: &Bound, unit: Unit<'_>) -> Result<Qty, RowError> {
        let out = self.row.money(debit, unit)?.unwrap_or_default().abs();
        let into = self.row.money(credit, unit)?.unwrap_or_default().abs();
        if !out.is_zero() && !into.is_zero() {
            let both = "both the debit and the credit are filled in";
            return Err(self.row.error("bad-amount", both.into(), self.row.whole, "a row moves money one way").into());
        }
        Ok(into - out)
    }

    fn pending(&self) -> Result<bool, RowError> {
        let Some(pending) = self.plan.of(Field::Pending) else { return Ok(false) };
        let cell = self.row.first(pending)?;
        let says = |word: &str| cell.is_some_and(|cell| cell.text.eq_ignore_ascii_case(word));
        Ok(match pending.spec.rule {
            Rule::Is(value) => says(self.row.book.text(value)),
            _ => ["pending", "true", "yes", "y", "1", "p"].iter().any(|word| says(word)),
        })
    }

    /// The memo cells joined, and where the first is.
    fn memo(&self) -> Result<(Cow<'t, str>, Span), RowError> {
        let Some(memo) = self.plan.of(Field::Memo) else { return Ok((Cow::Borrowed(""), self.row.whole)) };
        let mut joined = MemoJoin::default();
        for at in 0..memo.slots.len() {
            joined.push(self.row.cell(memo, at)?);
        }
        match joined.finish() {
            Some(found) => Ok(found),
            None => Ok((Cow::Borrowed(""), self.row.cell(memo, 0)?.span)),
        }
    }
}

pub fn read<'t, 'n, 's>(
    book: &'n Book<'s>,
    format: &Format,
    text: &'t str,
    file: FileId,
    unit: Unit<'_>,
    units: &[Unit<'_>],
) -> (Vec<Record<'t>>, Vec<Diagnostic>) {
    let reader = Reader { format, book };
    let (mut out, units) = (Harvest::default(), Units { account: unit, book: units });
    match &format.shape {
        Shape::Rows => reader.rows(text, file, units, &mut out),
        Shape::Tagged { records } => reader.tagged(book.name(*records), text, file, units, &mut out),
    }
    (out.records, out.problems)
}

/// Read just the memo fields from one declared, local `read` source. This is
/// the input to `check`'s unknown-memo suggestions: it deliberately does not
/// run commands, reconcile records, or change the book.
pub fn read_memos<'t, 's>(
    book: &Book<'s>,
    source: &Source,
    text: &'t str,
    file: FileId,
) -> Result<Vec<Cow<'t, str>>, Vec<Diagnostic>> {
    if !matches!(source.fetch, Fetch::Read(_)) {
        return Err(vec![Diagnostic::error(
            "sync-run-memos",
            "memos for a run source are unavailable without running it",
        )]);
    }
    let Some(format_id) = source.format else {
        return Err(vec![Diagnostic::error("sync-no-format", "this source has no record format to read memos from")]);
    };
    let Some(format) = book.formats.get(format_id) else {
        return Err(vec![Diagnostic::error(
            "sync-no-format",
            "this source refers to a format that is not in the book",
        )]);
    };
    let Some(memo) = format.specs.iter().find(|spec| spec.field == Field::Memo) else {
        return Err(vec![Diagnostic::error("sync-no-memo", "this source's format has no memo field")]);
    };
    if memo.places.is_empty() {
        return Err(vec![Diagnostic::error("sync-no-memo", "this source's memo field has no columns or paths")]);
    }
    match &format.shape {
        Shape::Rows => read_row_memos(book, format, memo, text, file),
        Shape::Tagged { records } => read_tagged_memos(book, memo, book.name(*records), text, file),
    }
}

fn read_row_memos<'t>(
    book: &Book<'_>,
    format: &Format,
    memo: &Spec,
    text: &'t str,
    file: FileId,
) -> Result<Vec<Cow<'t, str>>, Vec<Diagnostic>> {
    let (mut csv, mut cells) = (CsvReader::new(text), Vec::new());
    let Some(header) = csv.next(&mut cells) else {
        return Ok(Vec::new());
    };
    if let Err(broken) = header {
        return Err(vec![bad_csv(&broken, file)]);
    }
    let first = Row { number: csv.row, file, what: "row", cells: &cells, whole: whole(&cells), book };
    let slots = memo.places.iter().map(|&column| header_slot(book, &first, column)).collect::<Result<Vec<_>, _>>()?;
    let mut memos = Vec::new();
    if first_row_is_a_record(book, format, &first)? {
        take_row_memo(&first, &slots, &mut memos)?;
    }
    while let Some(read) = csv.next(&mut cells) {
        let row = Row { number: csv.row, file, what: "row", cells: &cells, whole: whole(&cells), book };
        match read {
            Ok(()) => take_row_memo(&row, &slots, &mut memos)?,
            Err(broken) => return Err(vec![bad_csv(&broken, file)]),
        }
    }
    Ok(memos)
}

/// Where a column is in an export whose first row is `first`: an index counts from 1, and a header is looked for
/// in the row.
fn header_slot(book: &Book<'_>, first: &Row, column: Column) -> Result<usize, Vec<Diagnostic>> {
    let problem = |diagnostic: Diagnostic| vec![diagnostic];
    match column {
        Column::Index(index) if index > 0 => Ok(usize::from(index) - 1),
        Column::Index(_) => Err(problem(bad_format("columns are counted from 1"))),
        Column::Header(header) => {
            let name = book.text(header);
            first.cells.iter().position(|cell| cell.text.eq_ignore_ascii_case(name)).ok_or_else(|| {
                let missing = format!("the export has no column \"{name}\"");
                problem(
                    Diagnostic::error("no-such-column", missing).label(first.whole.loc(first.file), "the header row"),
                )
            })
        }
        Column::Path(_) => Err(problem(bad_format("a rows format names columns"))),
    }
}

/// Whether the first row of an export is a record, and not the header: no column is found by its header, and
/// the row has a day where the date goes.
fn first_row_is_a_record(book: &Book<'_>, format: &Format, first: &Row) -> Result<bool, Vec<Diagnostic>> {
    let date = format.specs.iter().find(|spec| spec.field == Field::Date);
    let slot = date.and_then(|spec| spec.places.first().copied()).map(|column| header_slot(book, first, column));
    let slot = slot.transpose()?;
    let dated = slot.and_then(|slot| first.cells.get(slot)).is_some_and(|cell| day_of(cell, date).is_some());
    Ok(!names_a_header(format) && dated)
}

fn take_row_memo<'t>(
    row: &Row<'_, 't, '_, '_>,
    slots: &[usize],
    memos: &mut Vec<Cow<'t, str>>,
) -> Result<(), Vec<Diagnostic>> {
    let mut joined = MemoJoin::default();
    for &slot in slots {
        let Some(cell) = row.cells.get(slot) else {
            return Err(vec![row.error(
                "short-row",
                format!("has {} columns, but the memo column is missing", row.cells.len()),
                row.whole,
                "the row ends here",
            )]);
        };
        joined.push(cell);
    }
    if let Some((memo, _)) = joined.finish() {
        memos.push(memo);
    }
    Ok(())
}

fn read_tagged_memos<'t>(
    book: &Book<'_>,
    memo: &Spec,
    record_name: &str,
    text: &'t str,
    file: FileId,
) -> Result<Vec<Cow<'t, str>>, Vec<Diagnostic>> {
    if !memo.places.iter().all(|place| matches!(place, Column::Path(_))) {
        return Err(vec![bad_format("a tagged format names paths")]);
    }
    let paths = distinct_paths(book, memo.places.iter().copied());
    let slots: Vec<usize> = memo
        .places
        .iter()
        .filter_map(|place| match place {
            Column::Path(path) => Some(path_slot(book, &paths, *path)),
            _ => None,
        })
        .collect();
    let mut memos = Vec::new();
    let mut problems = Vec::new();
    tagged::scan(text, record_name, &paths, |found| match found {
        Ok(found) => match take_tagged_memo(found.cells, &slots, &mut memos) {
            Ok(()) => true,
            Err(problem) => {
                problems.push(problem);
                false
            }
        },
        Err(broken) => {
            problems.push(bad_tags(&broken, file));
            false
        }
    });
    if problems.is_empty() { Ok(memos) } else { Err(problems) }
}

fn take_tagged_memo<'t>(cells: &[Cell<'t>], slots: &[usize], memos: &mut Vec<Cow<'t, str>>) -> Result<(), Diagnostic> {
    let mut joined = MemoJoin::default();
    for &slot in slots {
        let Some(cell) = cells.get(slot) else {
            return Err(Diagnostic::error("bad-tags", "the tagged record has fewer cells than the format requests"));
        };
        joined.push(cell);
    }
    if let Some((memo, _)) = joined.finish() {
        memos.push(memo);
    }
    Ok(())
}

/// Whether any column of the format is found by its header.
fn names_a_header(format: &Format) -> bool {
    format.specs.iter().flat_map(|spec| spec.places.iter()).any(|place| matches!(place, Column::Header(_)))
}

/// The paths among `columns`, each once, in the order they are first named.
fn distinct_paths<'b>(book: &'b Book<'_>, columns: impl Iterator<Item = Column>) -> Vec<&'b str> {
    let mut paths = Vec::new();
    for column in columns {
        if let Column::Path(path) = column {
            let path = book.text(path);
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
    }
    paths
}

/// Where `path` is among the paths a tagged export is read for.
fn path_slot(book: &Book<'_>, paths: &[&str], path: Text) -> usize {
    let path = book.text(path);
    paths.iter().position(|known| *known == path).unwrap_or(0)
}

fn bad_format(message: &'static str) -> Diagnostic {
    Diagnostic::error("bad-format", message)
}

/// A row of an export that could not be split into cells.
fn bad_csv(broken: &Broken, file: FileId) -> Diagnostic {
    Diagnostic::error("bad-csv", broken.what).label(broken.span.loc(file), "here")
}

/// A record of an export that could not be read for its tags.
fn bad_tags(broken: &Broken, file: FileId) -> Diagnostic {
    Diagnostic::error("bad-tags", format!("record {}: {}", broken.row, broken.what))
        .label(broken.span.loc(file), "here")
}

/// The header row has no column called `name`: what it has instead, and the nearest of those.
fn no_such_column(header: &Row, name: &str) -> Diagnostic {
    let choices: Vec<&str> = header.cells.iter().map(|cell| cell.text.as_ref()).collect();
    let listed = choices.iter().map(|name| format!("\"{name}\"")).collect::<Vec<_>>().join(", ");
    let headline = format!("the export has no column \"{name}\"");
    let error = header.error("no-such-column", headline, header.whole, "the header row");
    let error = error.note(format!("its columns are {listed}"));
    match closest(name, choices.iter().copied()) {
        Some(near) => error.help(format!("did you mean \"{near}\"?")),
        None => error,
    }
}

pub fn date_layout(format: &Format) -> Option<&DateLayout> {
    format.specs.iter().find(|spec| spec.field == Field::Date)?.layout.as_ref()
}

pub fn category(format: &Format, book: &Book<'_>, text: &str) -> Option<Id<Purpose>> {
    let text = text.trim();
    format
        .categories
        .iter()
        .find(|(category, _)| book.text(*category).eq_ignore_ascii_case(text))
        .map(|(_, purpose)| *purpose)
}

/// A structured code is canonical as written, except that Axiom codes are case
/// insensitive and are stored lowercase. Do not invent prefixes from rules.
fn code_of<'t>(text: Cow<'t, str>) -> Option<Cow<'t, str>> {
    let valid = |code: &str| !code.is_empty() && code.chars().all(|c| c.is_ascii_alphanumeric() || "_:./-".contains(c));
    match text {
        Cow::Borrowed(text) => {
            let trimmed = text.trim();
            let code = trimmed.strip_prefix('^').unwrap_or(trimmed);
            if !valid(code) {
                None
            } else if code.bytes().any(|byte| byte.is_ascii_uppercase()) {
                Some(Cow::Owned(code.to_ascii_lowercase()))
            } else {
                Some(Cow::Borrowed(code))
            }
        }
        Cow::Owned(mut code) => {
            let leading = code.len() - code.trim_start().len();
            code.drain(..leading);
            let trailing = code.trim_end().len();
            code.truncate(trailing);
            if code.starts_with('^') {
                code.remove(0);
            }
            if !valid(&code) {
                None
            } else {
                code.make_ascii_lowercase();
                Some(Cow::Owned(code))
            }
        }
    }
}

fn uppercase<'t>(text: Cow<'t, str>) -> Cow<'t, str> {
    match text {
        Cow::Borrowed(text) if text.bytes().any(|byte| byte.is_ascii_lowercase()) => {
            Cow::Owned(text.to_ascii_uppercase())
        }
        Cow::Borrowed(text) => Cow::Borrowed(text),
        Cow::Owned(mut text) => {
            text.make_ascii_uppercase();
            Cow::Owned(text)
        }
    }
}

fn whole(cells: &[Cell]) -> Span {
    let start = cells.first().map_or(0, |cell| cell.span.start);
    Span { start, end: cells.last().map_or(start, |cell| cell.span.end) }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod text_tests {
    use super::*;

    #[test]
    fn canonical_codes_stay_borrowed_and_case_is_normalized_only_when_needed() {
        let lower = code_of(Cow::Borrowed("check-1041")).unwrap();
        assert!(matches!(lower, Cow::Borrowed("check-1041")));
        assert_eq!(code_of(Cow::Borrowed("^Check-1041")).unwrap(), "check-1041");
        assert_eq!(code_of(Cow::Owned("  ^Check-1041  ".to_string())).unwrap(), "check-1041");
        assert!(code_of(Cow::Borrowed("  ")).is_none());
        assert!(matches!(uppercase(Cow::Borrowed("EUR")), Cow::Borrowed("EUR")));
        assert_eq!(uppercase(Cow::Borrowed("eur")), "EUR");
    }
}
