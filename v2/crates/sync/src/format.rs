//! Runtime readers for the model's canonical format declarations. This module
//! keeps row/tag cells borrowed and only stores resolved column offsets; it
//! does not define or validate a second format schema.

use std::borrow::Cow;

use axiom_core::diag::closest;
use axiom_core::{Diagnostic, FileId, Id, Qty, calendar::DateLayout};
use axiom_model::{Book, Purpose};
use axiom_model::sync::{Column, Fetch, Field, Format, Rule, Shape, Source, Spec};

use crate::amount::amount;
use crate::csv::Reader as CsvReader;
use crate::date::iso_day;
use crate::{Facts, Record, Span, Unit, tagged};

const MAX_PROBLEMS: usize = 8;
const FIELDS: usize = 17;
pub(crate) const ABSENT: Span = Span {
    start: usize::MAX,
    end: usize::MAX,
};

pub(crate) struct Cell<'t> {
    pub text: Cow<'t, str>,
    pub span: Span,
}

#[derive(Default)]
struct MemoJoin<'t> {
    first: Option<Cow<'t, str>>,
    joined: Option<String>,
    span: Option<Span>,
}

impl<'t> MemoJoin<'t> {
    fn push(&mut self, cell: &Cell<'t>) {
        if cell.text.is_empty() {
            return;
        }
        if let Some(first) = &self.first {
            let joined = self.joined.get_or_insert_with(|| first.to_string());
            joined.push(' ');
            joined.push_str(&cell.text);
        } else {
            self.first = Some(cell.text.clone());
            self.span = Some(cell.span);
        }
    }

    fn finish(self) -> Option<(Cow<'t, str>, Span)> {
        match (self.first, self.joined) {
            (None, _) => None,
            (Some(first), None) => Some((first, self.span.unwrap_or(ABSENT))),
            (Some(_), Some(joined)) => Some((Cow::Owned(joined), self.span.unwrap_or(ABSENT))),
        }
    }
}

#[derive(Default)]
struct Harvest<'t> {
    records: Vec<Record<'t>>,
    problems: Vec<Diagnostic>,
}

impl<'t> Harvest<'t> {
    fn take(&mut self, read: Result<Record<'t>, Diagnostic>) -> bool {
        match read {
            Ok(record) => self.records.push(record),
            Err(problem) => self.problems.push(problem),
        }
        if self.problems.len() < MAX_PROBLEMS {
            return true;
        }
        if let Some(last) = self.problems.last_mut() {
            last.notes
                .push("the rest of the export was not read".into());
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

impl Row<'_, '_, '_, '_> {
    fn error(
        &self,
        code: &'static str,
        headline: String,
        span: Span,
        label: impl Into<String>,
    ) -> Diagnostic {
        let span = if span == ABSENT { self.whole } else { span };
        Diagnostic::error(code, format!("{} {}: {headline}", self.what, self.number))
            .label(span.loc(self.file), label)
    }

    fn shown(&self, column: Column) -> String {
        match column {
            Column::Header(name) => format!("the \"{}\" column", self.book.text(name)),
            Column::Index(index) => format!("column {index}"),
            Column::Path(path) => self.book.text(path).to_string(),
        }
    }

    fn cell<'r, 't>(&'r self, bound: &Bound, place: usize) -> Result<&'r Cell<'t>, Diagnostic>
    where
        't: 'r,
    {
        self.cells.get(bound.slots[place]).ok_or_else(|| {
            let end = self.cells.last().map_or(0, |cell| cell.span.end);
            let column = bound.spec.places[place];
            let headline = format!(
                "has {} columns, but {} is missing",
                self.cells.len(),
                self.shown(column)
            );
            self.error(
                "short-row",
                headline,
                Span { start: end, end },
                "the row ends here",
            )
        })
    }

    fn label(&self, bound: &Bound, place: usize) -> String {
        format!("in {}", self.shown(bound.spec.places[place]))
    }

    fn first<'r, 't>(&'r self, bound: &Bound) -> Result<Option<&'r Cell<'t>>, Diagnostic>
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

    fn money<'r, 't>(&'r self, bound: &Bound, unit: Unit<'_>) -> Result<Option<Qty>, Diagnostic>
    where
        't: 'r,
    {
        let cell = self.cell(bound, 0)?;
        let span = if cell.span == ABSENT {
            self.whole
        } else {
            cell.span
        };
        amount(&cell.text, unit.scale).map_err(|why| {
            why.diagnostic(
                &format!("{} {}", self.what, self.number),
                span.loc(self.file),
                self.label(bound, 0),
                &cell.text,
                unit,
            )
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

struct Reader<'f, 'n, 's> {
    format: &'f Format,
    book: &'n Book<'s>,
}

impl<'f, 'n, 's> Reader<'f, 'n, 's> {
    fn spec(&self, field: Field) -> Option<&'f Spec> {
        self.format.specs.iter().find(|spec| spec.field == field)
    }

    fn plan(
        &self,
        mut locate: impl FnMut(Column) -> Result<usize, Diagnostic>,
    ) -> Result<Plan<'f>, Diagnostic> {
        let mut plan = Plan(std::array::from_fn(|_| None));
        for spec in self.format.specs.iter() {
            let slots = spec
                .places
                .iter()
                .copied()
                .map(&mut locate)
                .collect::<Result<Vec<_>, _>>()?;
            let sign = match spec.rule {
                Rule::Sign { place, .. } => Some(locate(place)?),
                _ => None,
            };
            plan.0[spec.field as usize] = Some(Bound { spec, slots, sign });
        }
        Ok(plan)
    }

    fn day(&self, cell: &Cell<'_>, spec: Option<&Spec>) -> Option<axiom_core::Day> {
        match spec.and_then(|spec| spec.layout.as_ref()) {
            Some(layout) => layout.read(&cell.text),
            None => iso_day(&cell.text),
        }
    }

    fn rows<'t>(
        &self,
        text: &'t str,
        file: FileId,
        unit: Unit<'_>,
        units: &[Unit<'_>],
        out: &mut Harvest<'t>,
    ) {
        let mut csv = CsvReader::new(text);
        let mut cells = Vec::new();
        let Some(first) = csv.next(&mut cells) else {
            return;
        };
        let header_row = Row {
            number: csv.row,
            file,
            what: "row",
            cells: &cells,
            whole: whole(&cells),
            book: self.book,
        };
        if let Err(broken) = first {
            out.problems
                .push(header_row.error("bad-csv", broken.what.into(), broken.span, "here"));
            return;
        }
        let locate = |column: Column| match column {
            Column::Index(index) if index > 0 => Ok(usize::from(index) - 1),
            Column::Index(_) => Err(Diagnostic::error(
                "bad-format",
                "columns are counted from 1",
            )),
            Column::Header(header) => {
                let name = self.book.text(header);
                header_row
                    .cells
                    .iter()
                    .position(|cell| cell.text.eq_ignore_ascii_case(name))
                    .ok_or_else(|| {
                        let choices: Vec<&str> = header_row
                            .cells
                            .iter()
                            .map(|cell| cell.text.as_ref())
                            .collect();
                        let listed = choices
                            .iter()
                            .map(|name| format!("\"{name}\""))
                            .collect::<Vec<_>>()
                            .join(", ");
                        let error = header_row
                            .error(
                                "no-such-column",
                                format!("the export has no column \"{name}\""),
                                header_row.whole,
                                "the header row",
                            )
                            .note(format!("its columns are {listed}"));
                        match closest(name, choices.iter().copied()) {
                            Some(near) => error.help(format!("did you mean \"{near}\"?")),
                            None => error,
                        }
                    })
            }
            Column::Path(_) => Err(Diagnostic::error(
                "bad-format",
                "a rows format names columns",
            )),
        };
        let plan = match self.plan(locate) {
            Ok(plan) => plan,
            Err(problem) => {
                out.problems.push(problem);
                return;
            }
        };
        let has_headers = self
            .format
            .specs
            .iter()
            .flat_map(|spec| spec.places.iter())
            .any(|place| matches!(*place, Column::Header(_)));
        let dated = plan
            .of(Field::Date)
            .and_then(|bound| header_row.cell(bound, 0).ok());
        let dateless = dated.is_none_or(|cell| self.day(cell, self.spec(Field::Date)).is_none());
        let mut more =
            has_headers || dateless || out.take(self.record(&plan, &header_row, unit, units));
        while more {
            let Some(read) = csv.next(&mut cells) else {
                break;
            };
            let row = Row {
                number: csv.row,
                file,
                what: "row",
                cells: &cells,
                whole: whole(&cells),
                book: self.book,
            };
            let record = match read {
                Ok(()) => self.record(&plan, &row, unit, units),
                Err(broken) => Err(row.error("bad-csv", broken.what.into(), broken.span, "here")),
            };
            more = out.take(record);
        }
    }

    fn tagged<'t>(
        &self,
        record_name: &str,
        text: &'t str,
        file: FileId,
        unit: Unit<'_>,
        units: &[Unit<'_>],
        out: &mut Harvest<'t>,
    ) {
        let mut paths: Vec<&str> = Vec::new();
        for spec in self.format.specs.iter() {
            for place in spec.places.iter().copied() {
                if let Column::Path(path) = place {
                    let path = self.book.text(path);
                    if !paths.contains(&path) {
                        paths.push(path);
                    }
                }
            }
            if let Rule::Sign {
                place: Column::Path(path),
                ..
            } = spec.rule
            {
                let path = self.book.text(path);
                if !paths.contains(&path) {
                    paths.push(path);
                }
            }
        }
        let locate = |column: Column| match column {
            Column::Path(path) => {
                let path = self.book.text(path);
                Ok(paths.iter().position(|known| *known == path).unwrap_or(0))
            }
            _ => Err(Diagnostic::error(
                "bad-format",
                "a tagged format names paths",
            )),
        };
        let plan = match self.plan(locate) {
            Ok(plan) => plan,
            Err(problem) => {
                out.problems.push(problem);
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
                    self.record(&plan, &row, unit, units)
                }
                Err(broken) => Err(Diagnostic::error(
                    "bad-tags",
                    format!("record {}: {}", broken.row, broken.what),
                )
                .label(broken.span.loc(file), "here")),
            };
            out.take(record)
        });
    }

    fn record<'t>(
        &self,
        plan: &Plan,
        row: &Row<'_, 't, 'n, 's>,
        account_unit: Unit<'_>,
        units: &[Unit<'_>],
    ) -> Result<Record<'t>, Diagnostic> {
        let bound = |field| plan.of(field);
        let text = |field| -> Result<Option<Cow<'t, str>>, Diagnostic> {
            let cell = bound(field)
                .map(|bound| row.first(bound))
                .transpose()?
                .flatten();
            Ok(cell.map(|cell| cell.text.clone()))
        };
        let mut facts = Facts::default();
        let currency =
            text(Field::Currency)?.filter(|code| !code.eq_ignore_ascii_case(account_unit.name));
        let unit = match currency.as_deref() {
            None => account_unit,
            Some(code) => match units
                .iter()
                .find(|known| known.name.eq_ignore_ascii_case(code))
            {
                Some(&known) => known,
                None => {
                    let cell = bound(Field::Currency)
                        .map(|bound| row.cell(bound, 0))
                        .transpose()?;
                    return Err(row.error(
                        "bad-currency",
                        format!("the book has no unit `{}`", code.to_uppercase()),
                        cell.map_or(ABSENT, |cell| cell.span),
                        "the currency",
                    ));
                }
            },
        };
        facts.currency = currency.map(|code| {
            uppercase(code)
        });

        let Some(date) = bound(Field::Date) else {
            return Err(Diagnostic::error(
                "bad-format",
                "the compiled feed format has no date field",
            ));
        };
        let cell = row.cell(date, 0)?;
        let day = self.day(cell, Some(date.spec)).ok_or_else(|| {
            let shown = row.shown(date.spec.places[0]);
            if cell.span == ABSENT {
                return row.error(
                    "missing-field",
                    format!("it has no {shown}"),
                    ABSENT,
                    "this record",
                );
            }
            let layout = date
                .spec
                .layout
                .as_ref()
                .map_or_else(|| "YYYY-MM-DD".to_string(), ToString::to_string);
            let error = row.error(
                "bad-date",
                format!("`{}` is not a date written {layout}", cell.text),
                cell.span,
                row.label(date, 0),
            );
            match date
                .spec
                .layout
                .as_ref()
                .map(DateLayout::swapped)
                .filter(|swapped| swapped.read(&cell.text).is_some())
            {
                Some(swapped) => error.help(format!(
                    "if the day comes first, write the pattern as \"{swapped}\""
                )),
                None => error,
            }
        })?;

        let money = |field: Field| -> Result<Option<Qty>, Diagnostic> {
            bound(field)
                .map(|bound| row.money(bound, unit))
                .transpose()
                .map(Option::flatten)
        };
        let (gross, fee) = (money(Field::Gross)?, money(Field::Fee)?);
        let qty = if let Some(amount) = bound(Field::Amount) {
            let qty = row.money(amount, unit)?.ok_or_else(|| {
                let cell = row.cell(amount, 0).map_or(ABSENT, |cell| cell.span);
                let label = format!("{} is empty", row.shown(amount.spec.places[0]));
                row.error("bad-amount", "there is no amount".into(), cell, label)
            })?;
            match amount.spec.rule {
                Rule::Flipped => -qty,
                Rule::Sign { into, .. } => {
                    let expected = self.book.text(into);
                    let sign = amount
                        .sign
                        .and_then(|slot| row.cells.get(slot))
                        .filter(|cell| !cell.text.is_empty());
                    let Some(sign) = sign else {
                        return Err(row.error(
                            "missing-field",
                            "it has no sign for its amount".into(),
                            ABSENT,
                            "this record",
                        ));
                    };
                    if sign.text.eq_ignore_ascii_case(expected) {
                        qty.abs()
                    } else {
                        -qty.abs()
                    }
                }
                _ => qty,
            }
        } else if let (Some(debit), Some(credit)) = (bound(Field::Debit), bound(Field::Credit)) {
            let out = row.money(debit, unit)?.unwrap_or_default().abs();
            let into = row.money(credit, unit)?.unwrap_or_default().abs();
            if !out.is_zero() && !into.is_zero() {
                return Err(row.error(
                    "bad-amount",
                    "both the debit and the credit are filled in".into(),
                    row.whole,
                    "a row moves money one way",
                ));
            }
            into - out
        } else {
            gross.ok_or_else(|| {
                row.error(
                    "bad-amount",
                    "there is no amount".into(),
                    ABSENT,
                    "this record",
                )
            })? - fee.unwrap_or_default().abs()
        };
        (facts.gross, facts.fee) = (gross.map(Qty::abs), fee.map(Qty::abs));

        let balance = money(Field::Balance)?;
        let pending = match bound(Field::Pending) {
            None => false,
            Some(pending) => {
                let cell = row.first(pending)?;
                let says =
                    |word: &str| cell.is_some_and(|cell| cell.text.eq_ignore_ascii_case(word));
                match pending.spec.rule {
                    Rule::Is(value) => says(self.book.text(value)),
                    _ => ["pending", "true", "yes", "y", "1", "p"]
                        .iter()
                        .any(|word| says(word)),
                }
            }
        };

        let (memo, memo_span) = match bound(Field::Memo) {
            None => (Cow::Borrowed(""), row.whole),
            Some(memo) => {
                let mut joined = MemoJoin::default();
                for at in 0..memo.slots.len() {
                    let cell = row.cell(memo, at)?;
                    joined.push(cell);
                }
                joined
                    .finish()
                    .unwrap_or((Cow::Borrowed(""), row.cell(memo, 0)?.span))
            }
        };
        let at = if memo_span == ABSENT {
            row.whole
        } else {
            memo_span
        }
        .loc(row.file);
        facts.code = text(Field::Code)?.and_then(code_of);
        for (field, slot) in [
            (Field::Id, &mut facts.id),
            (Field::Party, &mut facts.party),
            (Field::Via, &mut facts.via),
            (Field::Category, &mut facts.category),
            (Field::Object, &mut facts.object),
            (Field::Route, &mut facts.route),
        ] {
            *slot = text(field)?;
        }
        let facts = (facts != Facts::default()).then(|| Box::new(facts));
        Ok(Record {
            day,
            qty,
            memo,
            balance,
            pending,
            at,
            facts,
        })
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
    let mut out = Harvest::default();
    match &format.shape {
        Shape::Rows => reader.rows(text, file, unit, units, &mut out),
        Shape::Tagged { records } => {
            reader.tagged(book.name(*records), text, file, unit, units, &mut out)
        }
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
        return Err(vec![Diagnostic::error(
            "sync-no-format",
            "this source has no record format to read memos from",
        )]);
    };
    let Some(format) = book.formats.get(format_id) else {
        return Err(vec![Diagnostic::error(
            "sync-no-format",
            "this source refers to a format that is not in the book",
        )]);
    };
    let Some(memo) = format.specs.iter().find(|spec| spec.field == Field::Memo) else {
        return Err(vec![Diagnostic::error(
            "sync-no-memo",
            "this source's format has no memo field",
        )]);
    };
    if memo.places.is_empty() {
        return Err(vec![Diagnostic::error(
            "sync-no-memo",
            "this source's memo field has no columns or paths",
        )]);
    }
    match &format.shape {
        Shape::Rows => read_row_memos(book, format, memo, text, file),
        Shape::Tagged { records } => {
            read_tagged_memos(book, format, memo, book.name(*records), text, file)
        }
    }
}

fn read_row_memos<'t, 's>(
    book: &Book<'s>,
    format: &Format,
    memo: &Spec,
    text: &'t str,
    file: FileId,
) -> Result<Vec<Cow<'t, str>>, Vec<Diagnostic>> {
    let (mut csv, mut cells) = (CsvReader::new(text), Vec::new());
    let Some(header) = csv.next(&mut cells) else {
        return Ok(Vec::new());
    };
    let span = whole(&cells);
    if let Err(broken) = header {
        return Err(vec![Diagnostic::error("bad-csv", broken.what)
            .label(broken.span.loc(file), "here")]);
    }
    let first = Row {
        number: csv.row,
        file,
        what: "row",
        cells: &cells,
        whole: span,
        book,
    };
    let locate = |column: Column| -> Result<usize, Diagnostic> {
        match column {
            Column::Index(index) if index > 0 => Ok(usize::from(index) - 1),
            Column::Index(_) => Err(Diagnostic::error(
                "bad-format",
                "columns are counted from 1",
            )),
            Column::Header(header) => first
                .cells
                .iter()
                .position(|cell| cell.text.eq_ignore_ascii_case(book.text(header)))
                .ok_or_else(|| {
                    Diagnostic::error(
                        "no-such-column",
                        format!("the export has no column \"{}\"", book.text(header)),
                    )
                    .label(first.whole.loc(file), "the header row")
                }),
            Column::Path(_) => Err(Diagnostic::error(
                "bad-format",
                "a rows format names columns",
            )),
        }
    };
    let memo_slots = memo
        .places
        .iter()
        .copied()
        .map(locate)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|problem| vec![problem])?;
    let date_spec = format.specs.iter().find(|spec| spec.field == Field::Date);
    let date_slot = date_spec
        .and_then(|spec| spec.places.first().copied())
        .map(locate)
        .transpose()
        .map_err(|problem| vec![problem])?;
    let has_headers = format
        .specs
        .iter()
        .flat_map(|spec| spec.places.iter())
        .any(|place| matches!(place, Column::Header(_)));
    let first_is_record = !has_headers
        && date_slot
            .and_then(|slot| first.cells.get(slot))
            .is_some_and(|cell| {
                date_spec
                    .and_then(|spec| spec.layout.as_ref())
                    .map_or_else(|| iso_day(&cell.text).is_some(), |layout| layout.read(&cell.text).is_some())
            });
    let mut memos = Vec::new();
    if first_is_record {
        take_row_memo(&first, &memo_slots, &mut memos)?;
    }
    while let Some(read) = csv.next(&mut cells) {
        let row = Row {
            number: csv.row,
            file,
            what: "row",
            cells: &cells,
            whole: whole(&cells),
            book,
        };
        match read {
            Ok(()) => take_row_memo(&row, &memo_slots, &mut memos)?,
            Err(broken) => {
                return Err(vec![Diagnostic::error("bad-csv", broken.what)
                    .label(broken.span.loc(file), "here")]);
            }
        }
    }
    Ok(memos)
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

fn read_tagged_memos<'t, 's>(
    book: &Book<'s>,
    _format: &Format,
    memo: &Spec,
    record_name: &str,
    text: &'t str,
    file: FileId,
) -> Result<Vec<Cow<'t, str>>, Vec<Diagnostic>> {
    let mut paths = Vec::with_capacity(memo.places.len());
    for place in memo.places.iter().copied() {
        let Column::Path(path) = place else {
            return Err(vec![Diagnostic::error(
                "bad-format",
                "a tagged format names paths",
            )]);
        };
        let path = book.text(path);
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    let slots: Vec<usize> = memo
        .places
        .iter()
        .map(|place| {
            let Column::Path(path) = place else {
                return Err(Diagnostic::error(
                    "bad-format",
                    "a tagged format names paths",
                ));
            };
            Ok(paths.iter().position(|known| *known == book.text(*path)).unwrap_or(0))
        })
        .collect::<Result<_, _>>()
        .map_err(|problem| vec![problem])?;
    let mut memos = Vec::new();
    let mut problems = Vec::new();
    tagged::scan(text, record_name, &paths, |found| match found {
        Ok(found) => {
            let mut joined = MemoJoin::default();
            for &slot in &slots {
                let Some(cell) = found.cells.get(slot) else {
                    problems.push(Diagnostic::error(
                        "bad-tags",
                        "the tagged record has fewer cells than the format requests",
                    ));
                    return false;
                };
                joined.push(cell);
            }
            if let Some((memo, _)) = joined.finish() {
                memos.push(memo);
            }
            true
        }
        Err(broken) => {
            problems.push(Diagnostic::error(
                "bad-tags",
                format!("record {}: {}", broken.row, broken.what),
            )
            .label(broken.span.loc(file), "here"));
            false
        }
    });
    if problems.is_empty() {
        Ok(memos)
    } else {
        Err(problems)
    }
}

pub fn date_layout(format: &Format) -> Option<&DateLayout> {
    format
        .specs
        .iter()
        .find(|spec| spec.field == Field::Date)?
        .layout
        .as_ref()
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
    let valid = |code: &str| {
        !code.is_empty()
            && code
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "_:./-".contains(c))
    };
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
    Span {
        start,
        end: cells.last().map_or(start, |cell| cell.span.end),
    }
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
        assert_eq!(
            code_of(Cow::Owned("  ^Check-1041  ".to_string())).unwrap(),
            "check-1041"
        );
        assert!(code_of(Cow::Borrowed("  ")).is_none());
        assert!(matches!(uppercase(Cow::Borrowed("EUR")), Cow::Borrowed("EUR")));
        assert_eq!(uppercase(Cow::Borrowed("eur")), "EUR");
    }
}
