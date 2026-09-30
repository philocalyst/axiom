//! Formats are declarations (LANGUAGE §14): which column, or which tag, is the
//! date, the amount, the memo, and the rest of what a record can say. One
//! builder makes a [`Record`] from the cells a row or a tagged record gave; what
//! cannot be read is a diagnostic that points at the cell, never a panic.

use std::borrow::Cow;

use axiom_core::diag::closest;
use axiom_core::{Diagnostic, FileId, Qty};

use crate::amount::amount;
use crate::csv::Reader;
use crate::date::{DateFormat, iso_day};
use crate::{Facts, Record, Span, Unit, tagged};

/// A bad column usually fails every row alike; after this many problems the
/// rest of the export is not read.
const MAX_PROBLEMS: usize = 8;

/// Where a value sits in a row or a tagged record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Place {
    /// A column, by the header row's name, in any case.
    Name(String),
    /// A column, by position, counting from 1.
    Index(usize),
    /// An element, by its path in the record: `BookgDt/Dt`, or `DTPOSTED` at any depth.
    Path(String),
}

impl Place {
    fn shown(&self) -> String {
        match self {
            Place::Name(name) => format!("the \"{name}\" column"),
            Place::Index(index) => format!("column {index}"),
            Place::Path(path) => path.clone(),
        }
    }
}

/// What a column or a tag is of a record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Date,
    /// Money into the account is positive.
    Amount,
    /// Money out, in a column of its own; with [`Field::Credit`].
    Debit,
    Credit,
    Memo,
    /// What the account held after the record.
    Balance,
    Pending,
    Code,
    /// Records that share one are one flow, like the two sides of a conversion.
    Id,
    Party,
    Gross,
    /// The `-` item of a payout, `via` the source.
    Fee,
    Currency,
    /// Mapped to a purpose by the format's `categories`.
    Category,
    /// The purpose's object: `#purchase of laptop`.
    Object,
    /// The account a row belongs to, for an export of several.
    Route,
    /// Who the money was for, when the memo names its go-between.
    Via,
}

const FIELDS: usize = 17;

/// What follows a field's place in its declaration.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Rule {
    #[default]
    None,
    /// `amount "Amount" flipped`: the export shows charges as positive.
    Flipped,
    /// `amount Amt, sign CdtDbtInd CRDT`: the amount has no sign of its own; it
    /// is money into the account when the value at `place` is `into`.
    Sign { place: Place, into: String },
    /// `pending Sts PDNG`: it is when the value is this. Without it, a flag or
    /// a word (`pending`, `true`, `yes`, `y`, `1`, `p`) says so.
    Is(String),
}

/// One line of a format: a field, where it is, and how to read it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spec {
    pub field: Field,
    /// More than one is for a memo, which is what they say in turn (`memo NAME, MEMO`).
    pub places: Vec<Place>,
    /// A date's layout; ISO when absent.
    pub layout: Option<DateFormat>,
    pub rule: Rule,
}

impl Spec {
    pub fn new(field: Field, places: impl IntoIterator<Item = Place>) -> Spec {
        Spec { field, places: places.into_iter().collect(), layout: None, rule: Rule::None }
    }

    pub fn layout(mut self, layout: DateFormat) -> Spec {
        self.layout = Some(layout);
        self
    }

    pub fn rule(mut self, rule: Rule) -> Spec {
        self.rule = rule;
        self
    }
}

/// How the text is cut into records.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Shape {
    /// Rows of comma-separated cells, a header row optional.
    Rows,
    /// Elements named `records`, each with its fields inside.
    Tagged { records: String },
}

/// `format csv …` or `format ofx …`: how a source's records read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Format {
    pub shape: Shape,
    pub specs: Vec<Spec>,
    /// `category "Groceries" is #groceries`: the text a record's category has,
    /// and the purpose it is.
    pub categories: Vec<(String, String)>,
}

/// One cell: its text, and its place in the file.
#[derive(Clone, Debug)]
pub(crate) struct Cell<'t> {
    pub text: Cow<'t, str>,
    pub span: Span,
}

/// Where the cell of an element a record does not have would be.
pub(crate) const ABSENT: Span = Span { start: usize::MAX, end: usize::MAX };

/// One row or tagged record, and where its problems are reported.
struct Row<'r, 't> {
    number: usize,
    file: FileId,
    what: &'static str,
    cells: &'r [Cell<'t>],
    whole: Span,
}

impl<'r, 't> Row<'r, 't> {
    fn error(&self, code: &'static str, headline: String, span: Span, label: impl Into<String>) -> Diagnostic {
        let span = if span == ABSENT { self.whole } else { span };
        Diagnostic::error(code, format!("{} {}: {headline}", self.what, self.number)).label(span.loc(self.file), label)
    }

    /// The cell of the bound field's `place`th place.
    fn cell(&self, bound: &Bound, place: usize) -> Result<&'r Cell<'t>, Diagnostic> {
        self.cells.get(bound.slots[place]).ok_or_else(|| {
            let end = self.cells.last().map_or(0, |cell| cell.span.end);
            let headline = format!(
                "has {} columns, but {} is number {}",
                self.cells.len(),
                bound.spec.places[place].shown(),
                bound.slots[place] + 1
            );
            self.error("short-row", headline, Span { start: end, end }, "the row ends here")
        })
    }

    fn label(&self, bound: &Bound, place: usize) -> String {
        format!("in {}", bound.spec.places[place].shown())
    }

    /// The first cell of the field that is not empty.
    fn first(&self, bound: &Bound) -> Result<Option<&'r Cell<'t>>, Diagnostic> {
        for place in 0..bound.slots.len() {
            let cell = self.cell(bound, place)?;
            if !cell.text.is_empty() {
                return Ok(Some(cell));
            }
        }
        Ok(None)
    }

    /// The amount in the field's cell, if it holds one.
    fn money(&self, bound: &Bound, unit: Unit) -> Result<Option<Qty>, Diagnostic> {
        let cell = self.cell(bound, 0)?;
        let place = format!("{} {}", self.what, self.number);
        let span = if cell.span == ABSENT { self.whole } else { cell.span };
        amount(&cell.text, unit.scale)
            .map_err(|why| why.diagnostic(&place, span.loc(self.file), self.label(bound, 0), &cell.text, unit))
    }
}

/// A field, and where the cells it reads are in the records.
struct Bound<'f> {
    spec: &'f Spec,
    slots: Vec<usize>,
    /// Where the value a `sign` rule reads is.
    sign: Option<usize>,
}

/// The format's fields, by kind.
struct Plan<'f>([Option<Bound<'f>>; FIELDS]);

impl<'f> Plan<'f> {
    fn of(&self, field: Field) -> Option<&Bound<'f>> {
        self.0[field as usize].as_ref()
    }
}

impl Format {
    /// The layout dates are declared in, if any.
    pub fn date_layout(&self) -> Option<&DateFormat> {
        self.spec(Field::Date).and_then(|spec| spec.layout.as_ref())
    }

    /// The purpose a category is mapped to.
    pub fn purpose(&self, category: &str) -> Option<&str> {
        let same = |(text, _): &&(String, String)| text.eq_ignore_ascii_case(category.trim());
        self.categories.iter().find(same).map(|(_, purpose)| purpose.as_str())
    }

    fn spec(&self, field: Field) -> Option<&Spec> {
        self.specs.iter().find(|spec| spec.field == field)
    }

    /// What is wrong with the declaration itself, if anything.
    pub fn check(&self) -> Result<(), String> {
        let mut seen = [false; FIELDS];
        for spec in &self.specs {
            let field = format!("{:?}", spec.field).to_lowercase();
            if std::mem::replace(&mut seen[spec.field as usize], true) {
                return Err(format!("`{field}` is declared twice"));
            }
            if spec.places.is_empty() || (spec.places.len() > 1 && spec.field != Field::Memo) {
                return Err(format!("`{field}` is one column or tag; only `memo` may name several"));
            }
            let sign = if let Rule::Sign { place, .. } = &spec.rule { Some(place) } else { None };
            for place in spec.places.iter().chain(sign) {
                match (&self.shape, place) {
                    (Shape::Rows, Place::Index(0)) => return Err("columns are counted from 1".into()),
                    (Shape::Rows, Place::Name(_) | Place::Index(_)) | (Shape::Tagged { .. }, Place::Path(_)) => {}
                    (Shape::Rows, Place::Path(_)) => {
                        return Err(format!("`{field}` names a tag; a csv format names columns"));
                    }
                    (Shape::Tagged { .. }, _) => {
                        return Err(format!("`{field}` names a column; a tagged format names tags"));
                    }
                }
            }
            match (&spec.rule, spec.field) {
                (Rule::None, _)
                | (Rule::Flipped | Rule::Sign { .. }, Field::Amount)
                | (Rule::Is(_), Field::Pending) => {}
                _ => return Err(format!("`{field}` cannot take that")),
            }
        }
        let has = |field| seen[field as usize];
        if !has(Field::Date) {
            return Err("the format names no date".into());
        }
        if has(Field::Debit) != has(Field::Credit) {
            return Err("`debit` and `credit` go together".into());
        }
        if !(has(Field::Amount) || has(Field::Debit) || has(Field::Gross)) {
            return Err("the format names no amount: `amount`, `debit` and `credit`, or `gross`".into());
        }
        match &self.shape {
            Shape::Tagged { records } if records.trim().is_empty() => Err("a tagged format names its records".into()),
            _ => Ok(()),
        }
    }

    /// Every record of `text`, and everything wrong with it. `unit` is what
    /// the account holds, and `units` every unit the book has, for the records
    /// that name their own currency.
    pub fn read<'t>(
        &self,
        text: &'t str,
        file: FileId,
        unit: Unit,
        units: &[Unit],
    ) -> (Vec<Record<'t>>, Vec<Diagnostic>) {
        if let Err(message) = self.check() {
            return (Vec::new(), vec![Diagnostic::error("bad-format", message)]);
        }
        let mut harvest = Harvest::default();
        match &self.shape {
            Shape::Rows => self.read_rows(text, file, unit, units, &mut harvest),
            Shape::Tagged { records } => self.read_tagged(records, text, file, unit, units, &mut harvest),
        }
        (harvest.records, harvest.problems)
    }

    /// Rows: the first is the header when a column is named by it, or when its
    /// date is not a date.
    fn read_rows<'t>(&self, text: &'t str, file: FileId, unit: Unit, units: &[Unit], harvest: &mut Harvest<'t>) {
        let (mut reader, mut cells) = (Reader::new(text), Vec::new());
        let Some(first) = reader.next(&mut cells) else { return };
        let row = Row { number: reader.row, file, what: "row", cells: &cells, whole: whole(&cells) };
        if let Err(broken) = first {
            return harvest.problems.push(row.error("bad-csv", broken.what.into(), broken.span, "here"));
        }
        let locate = |place: &Place| match place {
            Place::Index(index) => Ok(index - 1),
            Place::Name(name) => {
                row.cells.iter().position(|cell| cell.text.eq_ignore_ascii_case(name)).ok_or_else(|| {
                    let names: Vec<&str> = row.cells.iter().map(|cell| &*cell.text).collect();
                    let listed = names.iter().map(|name| format!("\"{name}\"")).collect::<Vec<_>>().join(", ");
                    let error = row
                        .error(
                            "no-such-column",
                            format!("the export has no column \"{name}\""),
                            row.whole,
                            "the header row",
                        )
                        .note(format!("its columns are {listed}"));
                    match closest(name, names.iter().copied()) {
                        Some(near) => error.help(format!("did you mean \"{near}\"?")),
                        None => error,
                    }
                })
            }
            Place::Path(_) => Err(Diagnostic::error("bad-format", "a csv format names columns")),
        };
        let plan = match self.plan(locate) {
            Ok(plan) => plan,
            Err(problem) => return harvest.problems.push(problem),
        };
        let named = self.specs.iter().flat_map(|spec| &spec.places).any(|place| matches!(place, Place::Name(_)));
        let dated = plan.of(Field::Date).and_then(|bound| row.cell(bound, 0).ok());
        let dateless = dated.is_none_or(|cell| self.day(cell, self.spec(Field::Date)).is_none());
        let mut more = named || dateless || harvest.take(self.record(&plan, &row, unit, units));
        while more {
            let Some(read) = reader.next(&mut cells) else { break };
            let row = Row { number: reader.row, file, what: "row", cells: &cells, whole: whole(&cells) };
            let record = match read {
                Ok(()) => self.record(&plan, &row, unit, units),
                Err(broken) => Err(row.error("bad-csv", broken.what.into(), broken.span, "here")),
            };
            more = harvest.take(record);
        }
    }

    fn read_tagged<'t>(
        &self,
        records: &str,
        text: &'t str,
        file: FileId,
        unit: Unit,
        units: &[Unit],
        harvest: &mut Harvest<'t>,
    ) {
        let mut paths: Vec<&str> = Vec::new();
        for spec in &self.specs {
            let sign = if let Rule::Sign { place, .. } = &spec.rule { Some(place) } else { None };
            for place in spec.places.iter().chain(sign) {
                if let Place::Path(path) = place {
                    if !paths.contains(&path.as_str()) {
                        paths.push(path);
                    }
                }
            }
        }
        let locate = |place: &Place| match place {
            Place::Path(path) => Ok(paths.iter().position(|known| known == path).unwrap_or(0)),
            _ => Err(Diagnostic::error("bad-format", "a tagged format names tags")),
        };
        let plan = match self.plan(locate) {
            Ok(plan) => plan,
            Err(problem) => return harvest.problems.push(problem),
        };
        tagged::scan(text, records, &paths, |found| {
            let record = match found {
                Ok(found) => {
                    let row =
                        Row { number: found.number, file, what: "record", cells: &found.cells, whole: found.whole };
                    self.record(&plan, &row, unit, units)
                }
                Err(broken) => {
                    let (span, headline) = (broken.span.loc(file), format!("record {}: {}", broken.row, broken.what));
                    Err(Diagnostic::error("bad-tags", headline).label(span, "here"))
                }
            };
            harvest.take(record)
        });
    }

    /// Where each field's cells are, given how a place is found.
    fn plan(&self, mut locate: impl FnMut(&Place) -> Result<usize, Diagnostic>) -> Result<Plan<'_>, Diagnostic> {
        let mut plan = Plan(std::array::from_fn(|_| None));
        for spec in &self.specs {
            let slots = spec.places.iter().map(&mut locate).collect::<Result<Vec<_>, _>>()?;
            let sign = if let Rule::Sign { place, .. } = &spec.rule { Some(locate(place)?) } else { None };
            plan.0[spec.field as usize] = Some(Bound { spec, slots, sign });
        }
        Ok(plan)
    }

    /// The day a date cell says, in the format's layout or as ISO.
    fn day(&self, cell: &Cell, spec: Option<&Spec>) -> Option<axiom_core::Day> {
        match spec.and_then(|spec| spec.layout.as_ref()) {
            Some(layout) => layout.read(&cell.text),
            None => iso_day(&cell.text),
        }
    }

    fn record<'t>(&self, plan: &Plan, row: &Row<'_, 't>, unit: Unit, units: &[Unit]) -> Result<Record<'t>, Diagnostic> {
        let bound = |field| plan.of(field);
        let text = |field| -> Result<Option<Cow<'t, str>>, Diagnostic> {
            let cell = bound(field).map(|bound| row.first(bound)).transpose()?.flatten();
            Ok(cell.map(|cell| cell.text.clone()))
        };
        let mut facts = Facts::default();

        // A record in another currency is counted to that currency's places.
        let currency = text(Field::Currency)?.filter(|code| !code.eq_ignore_ascii_case(unit.name));
        let unit = match &currency {
            None => unit,
            Some(code) => match units.iter().find(|known| known.name.eq_ignore_ascii_case(code)) {
                Some(&known) => known,
                None => {
                    let cell = bound(Field::Currency).map(|bound| row.cell(bound, 0)).transpose()?;
                    let headline = format!("the book has no unit `{}`", code.to_uppercase());
                    return Err(row.error(
                        "bad-currency",
                        headline,
                        cell.map_or(ABSENT, |cell| cell.span),
                        "the currency",
                    ));
                }
            },
        };
        facts.currency = currency.map(|code| Cow::Owned(code.to_uppercase()));

        let date = bound(Field::Date).expect("a date was declared");
        let cell = row.cell(date, 0)?;
        let day = self.day(cell, Some(date.spec)).ok_or_else(|| {
            let shown = date.spec.places[0].shown();
            if cell.span == ABSENT {
                return row.error("missing-field", format!("it has no {shown}"), ABSENT, "this record");
            }
            let layout = date.spec.layout.as_ref().map_or("YYYY-MM-DD".to_string(), |layout| layout.to_string());
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
                .map(|layout| layout.swapped())
                .filter(|swapped| swapped.read(&cell.text).is_some())
            {
                Some(swapped) => error.help(format!("if the day comes first, write the pattern as \"{swapped}\"")),
                None => error,
            }
        })?;

        let money = |field: Field| -> Result<Option<Qty>, Diagnostic> {
            bound(field).map(|bound| row.money(bound, unit)).transpose().map(Option::flatten)
        };
        let (gross, fee) = (money(Field::Gross)?, money(Field::Fee)?);
        let qty = if let Some(amount) = bound(Field::Amount) {
            let qty = row.money(amount, unit)?.ok_or_else(|| {
                let cell = row.cell(amount, 0).map_or(ABSENT, |cell| cell.span);
                let label = format!("{} is empty", amount.spec.places[0].shown());
                row.error("bad-amount", "there is no amount".into(), cell, label)
            })?;
            match &amount.spec.rule {
                Rule::Flipped => -qty,
                Rule::Sign { into, .. } => {
                    let sign = amount.sign.and_then(|slot| row.cells.get(slot)).filter(|cell| !cell.text.is_empty());
                    let Some(sign) = sign else {
                        return Err(row.error(
                            "missing-field",
                            "it has no sign for its amount".into(),
                            ABSENT,
                            "this record",
                        ));
                    };
                    if sign.text.eq_ignore_ascii_case(into) { qty.abs() } else { -qty.abs() }
                }
                _ => qty,
            }
        } else if let (Some(debit), Some(credit)) = (bound(Field::Debit), bound(Field::Credit)) {
            let out = row.money(debit, unit)?.unwrap_or_default().abs();
            let into = row.money(credit, unit)?.unwrap_or_default().abs();
            if !out.is_zero() && !into.is_zero() {
                let both = "both the debit and the credit are filled in".to_string();
                return Err(row.error("bad-amount", both, row.whole, "a row moves money one way"));
            }
            into - out
        } else {
            let gross =
                gross.ok_or_else(|| row.error("bad-amount", "there is no amount".into(), ABSENT, "this record"))?;
            gross - fee.unwrap_or_default().abs()
        };
        (facts.gross, facts.fee) = (gross.map(Qty::abs), fee.map(Qty::abs));

        let balance = money(Field::Balance)?;
        let pending = match bound(Field::Pending) {
            None => false,
            Some(pending) => {
                let cell = row.first(pending)?;
                let says = |word: &str| cell.is_some_and(|cell| cell.text.eq_ignore_ascii_case(word));
                match &pending.spec.rule {
                    Rule::Is(value) => says(value),
                    _ => ["pending", "true", "yes", "y", "1", "p"].iter().any(|word| says(word)),
                }
            }
        };

        // What a person would call the memo: each of its cells that says something, in turn.
        let memo = match bound(Field::Memo) {
            None => (Cow::Borrowed(""), row.whole),
            Some(memo) => {
                let says: Vec<&Cell> = (0..memo.slots.len())
                    .map(|at| row.cell(memo, at))
                    .collect::<Result<Vec<_>, _>>()?
                    .into_iter()
                    .filter(|cell| !cell.text.is_empty())
                    .collect();
                match says.as_slice() {
                    [] => (Cow::Borrowed(""), row.cell(memo, 0)?.span),
                    [only] => (only.text.clone(), only.span),
                    [first, ..] => {
                        (Cow::Owned(says.iter().map(|cell| &*cell.text).collect::<Vec<_>>().join(" ")), first.span)
                    }
                }
            }
        };
        let at = if memo.1 == ABSENT { row.whole } else { memo.1 }.loc(row.file);

        facts.code = text(Field::Code)?.and_then(|code| code_of(&code)).map(Cow::Owned);
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
        Ok(Record { day, qty, memo: memo.0, balance, pending, at, facts })
    }
}

/// A code as the language writes one: lowercase, of `a-z 0-9 _ : . / -`, and
/// starting with a letter or a digit.
fn code_of(text: &str) -> Option<String> {
    let lower = text.trim().to_lowercase();
    let mapped: String =
        lower.chars().map(|c| if c.is_ascii_alphanumeric() || "_:./-".contains(c) { c } else { '-' }).collect();
    let code = mapped.trim_start_matches(|c: char| !c.is_ascii_alphanumeric());
    (!code.is_empty()).then(|| code.to_string())
}

fn whole(cells: &[Cell]) -> Span {
    let start = cells.first().map_or(0, |cell| cell.span.start);
    Span { start, end: cells.last().map_or(start, |cell| cell.span.end) }
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

#[cfg(test)]
mod tests;
