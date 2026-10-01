//! Runtime readers for the model's canonical format declarations. This module
//! keeps row/tag cells borrowed and only stores resolved column offsets; it
//! does not define or validate a second format schema.

use std::borrow::Cow;

use axiom_core::diag::closest;
use axiom_core::{Diagnostic, FileId, Id, Interner, Qty, calendar::DateLayout};
use axiom_model::Purpose;
use axiom_model::sync::{Column, Field, Format, Rule, Shape, Spec};

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
    names: &'n Interner<'s>,
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
            Column::Header(name) => format!("the \"{}\" column", self.names.name(name)),
            Column::Index(index) => format!("column {index}"),
            Column::Path(path) => self.names.name(path).to_string(),
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
        let place = format!("{} {}", self.what, self.number);
        let span = if cell.span == ABSENT {
            self.whole
        } else {
            cell.span
        };
        amount(&cell.text, unit.scale).map_err(|why| {
            why.diagnostic(
                &place,
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
    names: &'n Interner<'s>,
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
            names: self.names,
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
                let name = self.names.name(header);
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
                names: self.names,
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
                    let path = self.names.name(path);
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
                let path = self.names.name(path);
                if !paths.contains(&path) {
                    paths.push(path);
                }
            }
        }
        let locate = |column: Column| match column {
            Column::Path(path) => {
                let path = self.names.name(path);
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
                        cells: &found.cells,
                        whole: found.whole,
                        names: self.names,
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
        facts.currency = currency.map(|code| Cow::Owned(code.to_uppercase()));

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
                    let expected = self.names.name(into);
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
                    Rule::Is(value) => says(self.names.name(value)),
                    _ => ["pending", "true", "yes", "y", "1", "p"]
                        .iter()
                        .any(|word| says(word)),
                }
            }
        };

        let (memo, memo_span) = match bound(Field::Memo) {
            None => (Cow::Borrowed(""), row.whole),
            Some(memo) => {
                let mut nonempty = Vec::new();
                for at in 0..memo.slots.len() {
                    let cell = row.cell(memo, at)?;
                    if !cell.text.is_empty() {
                        nonempty.push(cell);
                    }
                }
                match nonempty.as_slice() {
                    [] => (Cow::Borrowed(""), row.cell(memo, 0)?.span),
                    [only] => (only.text.clone(), only.span),
                    [first, ..] => (
                        Cow::Owned(
                            nonempty
                                .iter()
                                .map(|cell| cell.text.as_ref())
                                .collect::<Vec<_>>()
                                .join(" "),
                        ),
                        first.span,
                    ),
                }
            }
        };
        let at = if memo_span == ABSENT {
            row.whole
        } else {
            memo_span
        }
        .loc(row.file);
        facts.code = text(Field::Code)?
            .and_then(|code| code_of(&code))
            .map(Cow::Owned);
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
    format: &Format,
    names: &'n Interner<'s>,
    text: &'t str,
    file: FileId,
    unit: Unit<'_>,
    units: &[Unit<'_>],
) -> (Vec<Record<'t>>, Vec<Diagnostic>) {
    let reader = Reader { format, names };
    let mut out = Harvest::default();
    match &format.shape {
        Shape::Rows => reader.rows(text, file, unit, units, &mut out),
        Shape::Tagged { records } => {
            reader.tagged(names.name(*records), text, file, unit, units, &mut out)
        }
    }
    (out.records, out.problems)
}

pub fn date_layout(format: &Format) -> Option<&DateLayout> {
    format
        .specs
        .iter()
        .find(|spec| spec.field == Field::Date)?
        .layout
        .as_ref()
}

pub fn category(format: &Format, names: &Interner<'_>, text: &str) -> Option<Id<Purpose>> {
    let text = text.trim();
    format
        .categories
        .iter()
        .find(|(category, _)| names.name(*category).eq_ignore_ascii_case(text))
        .map(|(_, purpose)| *purpose)
}

/// A structured code is canonical as written, except that Axiom codes are case
/// insensitive and are stored lowercase. Do not invent prefixes from rules.
fn code_of(text: &str) -> Option<String> {
    let text = text.trim().strip_prefix('^').unwrap_or(text.trim());
    if text.is_empty()
        || !text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_:./-".contains(c))
    {
        return None;
    }
    Some(text.to_ascii_lowercase())
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
