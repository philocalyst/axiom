//! Parser for Axiom's intentionally small, line-oriented source ledger.
//!
//! The parser does not try to be clever.  A source ledger is a durable piece of
//! evidence, so preserving where a value came from is more useful than
//! accepting a vaguely similar spelling.  Every node therefore carries its
//! source line and diagnostics point at the first character that made a line
//! invalid.

use std::collections::HashMap;
use std::fmt;

/// A one-based source location.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Location {
    pub line: usize,
    pub column: usize,
}

impl Location {
    const fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }
}

/// A parser diagnostic.  `line` and `column` are one based and are stable
/// even when comments or blank lines are present in the source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseError {
    pub location: Location,
    pub message: String,
}

impl ParseError {
    fn new(line: usize, column: usize, message: impl Into<String>) -> Self {
        Self {
            location: Location::new(line, column),
            message: message.into(),
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "line {}, column {}: {}",
            self.location.line, self.location.column, self.message
        )
    }
}

impl std::error::Error for ParseError {}

/// The kind of a typed existential hole.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum HoleKind {
    Account,
    Amount,
    Asset,
    Book,
    Date,
    Lot,
    Policy,
    Text,
    Unit,
}

impl fmt::Display for HoleKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Account => "account",
            Self::Amount => "amount",
            Self::Asset => "asset",
            Self::Book => "book",
            Self::Date => "date",
            Self::Lot => "lot",
            Self::Policy => "policy",
            Self::Text => "text",
            Self::Unit => "unit",
        };
        f.write_str(name)
    }
}

/// A named (`?lot`) or anonymous (`?`) typed hole.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Hole {
    pub name: Option<String>,
    pub kind: HoleKind,
    pub location: Location,
}

impl Hole {
    fn parse(token: &str, kind: HoleKind, line: usize, column: usize) -> Option<Self> {
        if !token.starts_with('?') {
            return None;
        }
        let name = token[1..].to_owned();
        if !name.is_empty() && !valid_name(&name) {
            return None;
        }
        Some(Self {
            name: (!name.is_empty()).then_some(name),
            kind,
            location: Location::new(line, column),
        })
    }
}

/// A symbol that is either known in the source or remains a typed hole.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Symbol {
    Known(String),
    Hole(Hole),
}

impl Symbol {
    pub fn as_known(&self) -> Option<&str> {
        match self {
            Self::Known(value) => Some(value),
            Self::Hole(_) => None,
        }
    }
}

/// Exact decimal source spelling.  It is deliberately not an `f64` (or any
/// other binary floating point number).  Consumers can convert this canonical
/// decimal string to their arbitrary-precision numeric type without loss.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Decimal(String);

impl Decimal {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn parse(token: &str, line: usize, column: usize) -> Result<Self, ParseError> {
        if token.is_empty() {
            return Err(ParseError::new(line, column, "expected a decimal number"));
        }
        let bytes = token.as_bytes();
        let mut index = 0;
        if matches!(bytes.first(), Some(b'+') | Some(b'-')) {
            index = 1;
        }
        let start_integer = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        let integer_digits = index - start_integer;
        let mut fractional_digits = 0;
        if bytes.get(index) == Some(&b'.') {
            index += 1;
            let start_fraction = index;
            while index < bytes.len() && bytes[index].is_ascii_digit() {
                index += 1;
            }
            fractional_digits = index - start_fraction;
        }
        if integer_digits == 0 || (bytes.get(start_integer) == Some(&b'0') && false) {
            return Err(ParseError::new(
                line,
                column,
                format!("invalid decimal `{token}`"),
            ));
        }
        if index != bytes.len() || (token.contains('.') && fractional_digits == 0) {
            return Err(ParseError::new(
                line,
                column,
                format!("invalid decimal `{token}`"),
            ));
        }
        // Keep exact spelling apart from insignificant leading pluses and
        // leading zeroes.  Trailing fractional zeroes are retained because
        // scale can be meaningful to a caller displaying source values.
        let negative = token.starts_with('-');
        let unsigned = token
            .strip_prefix('+')
            .or_else(|| token.strip_prefix('-'))
            .unwrap_or(token);
        let (integer, fraction) = unsigned
            .split_once('.')
            .map_or((unsigned, None), |(whole, fraction)| {
                (whole, Some(fraction))
            });
        let trimmed_integer = integer.trim_start_matches('0');
        let normalized_integer = if trimmed_integer.is_empty() {
            "0"
        } else {
            trimmed_integer
        };
        let mut normalized = String::new();
        if negative && normalized_integer != "0" {
            normalized.push('-');
        }
        normalized.push_str(normalized_integer);
        if let Some(fraction) = fraction {
            normalized.push('.');
            normalized.push_str(fraction);
        }
        Ok(Self(normalized))
    }
}

impl fmt::Display for Decimal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A quantity with a mandatory unit for non-zero values.  A unit can itself
/// be a hole, which is useful for importers that know the number but not its
/// commodity yet.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Quantity {
    pub amount: QuantityAmount,
    pub unit: Option<Symbol>,
    pub location: Location,
}

/// The numeric side of a quantity is either exact or an explicitly typed
/// amount hole.  Keeping the hole separate from a decimal prevents a source
/// `?` from being confused with zero.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum QuantityAmount {
    Exact(Decimal),
    Hole(Hole),
}

impl Quantity {
    fn parse(
        amount_token: &str,
        unit_token: Option<&str>,
        line: usize,
        column: usize,
        unit_column: usize,
    ) -> Result<Self, ParseError> {
        let amount = if amount_token.starts_with('?') {
            QuantityAmount::Hole(
                Hole::parse(amount_token, HoleKind::Amount, line, column).ok_or_else(|| {
                    ParseError::new(line, column, "invalid amount hole; use `?` or `?name`")
                })?,
            )
        } else {
            QuantityAmount::Exact(Decimal::parse(amount_token, line, column)?)
        };
        let amount_is_zero = matches!(&amount, QuantityAmount::Exact(value) if value
            .as_str()
            .chars()
            .all(|character| matches!(character, '0' | '.' | '-')));
        let unit = match unit_token {
            Some(token) if token.starts_with('?') => Some(Symbol::Hole(
                Hole::parse(token, HoleKind::Unit, line, unit_column).ok_or_else(|| {
                    ParseError::new(line, unit_column, "invalid unit hole; use `?` or `?name`")
                })?,
            )),
            Some(token) if valid_name(token) => Some(Symbol::Known(token.to_owned())),
            Some(_) => {
                return Err(ParseError::new(
                    line,
                    unit_column,
                    "unit must be a name or a typed hole",
                ));
            }
            None if amount_is_zero => None,
            None => {
                return Err(ParseError::new(
                    line,
                    unit_column,
                    "non-zero quantities require an explicit unit",
                ));
            }
        };
        Ok(Self {
            amount,
            unit,
            location: Location::new(line, column),
        })
    }
}

/// An ISO calendar date.  Dates are kept calendar-aware rather than accepted
/// as arbitrary strings so malformed dates fail at their source line.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Date {
    pub year: u16,
    pub month: u8,
    pub day: u8,
}

impl Date {
    fn parse(token: &str, line: usize, column: usize) -> Result<Self, ParseError> {
        let parts: Vec<_> = token.split('-').collect();
        if parts.len() != 3
            || parts[0].len() != 4
            || parts[1].len() != 2
            || parts[2].len() != 2
            || parts
                .iter()
                .any(|part| !part.bytes().all(|byte| byte.is_ascii_digit()))
        {
            return Err(ParseError::new(
                line,
                column,
                format!("date must use YYYY-MM-DD, got `{token}`"),
            ));
        }
        let year = parts[0].parse::<u16>().expect("four ASCII digits");
        let month = parts[1].parse::<u8>().expect("two ASCII digits");
        let day = parts[2].parse::<u8>().expect("two ASCII digits");
        let leap =
            year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
        let days_in_month = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if leap => 29,
            2 => 28,
            _ => 0,
        };
        if days_in_month == 0 || day == 0 || day > days_in_month {
            return Err(ParseError::new(
                line,
                column,
                format!("invalid calendar date `{token}`"),
            ));
        }
        Ok(Self { year, month, day })
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

/// The parser's lossless-ish intermediate ledger.  The public
/// [`parse_ledger`] entry point lowers this source representation into the
/// domain model; keeping this intermediate is useful for diagnostics and for
/// tools that want to preserve typed holes which are not yet resolved.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParsedLedger {
    pub book: Book,
    pub statements: Vec<Statement>,
}

/// Short compatibility name for consumers of the parser module.
pub type Ledger = ParsedLedger;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Book {
    pub name: Symbol,
    pub location: Location,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Statement {
    Buy(Buy),
    Sell(Sell),
    Quote(Quote),
    Observe(Observation),
    Use(UsePolicy),
    Decide(Decision),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Buy {
    pub id: String,
    pub date: Date,
    pub holding: Holding,
    pub consideration: Quantity,
    pub fee: Option<Quantity>,
    pub location: Location,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Sell {
    pub id: String,
    pub date: Date,
    pub holding: Holding,
    pub proceeds: Quantity,
    pub lot: Option<Hole>,
    pub location: Location,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Holding {
    pub quantity: Quantity,
    pub account: Symbol,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Quote {
    pub id: String,
    pub date: Date,
    pub base: Quantity,
    pub quote: Quantity,
    pub location: Location,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Observation {
    Position {
        account: Symbol,
        quantity: Quantity,
        location: Location,
    },
    Settlement {
        reference: String,
        quantity: Quantity,
        into: Option<Symbol>,
        location: Location,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UsePolicy {
    pub policy: Symbol,
    pub book: Symbol,
    pub location: Location,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Decision {
    pub sale: String,
    pub lot: Symbol,
    pub location: Location,
}

#[derive(Debug)]
enum Pending {
    Buy {
        id: String,
        date: Date,
        location: Location,
        holding: Option<Holding>,
        consideration: Option<Quantity>,
        fee: Option<Quantity>,
    },
    Sell {
        id: String,
        date: Date,
        location: Location,
        holding: Option<Holding>,
        proceeds: Option<Quantity>,
        lot: Option<Hole>,
    },
    Quote {
        id: String,
        date: Date,
        location: Location,
        base: Option<Quantity>,
        quote: Option<Quantity>,
    },
}

/// Parse one complete source ledger into the parser-facing representation.
pub fn parse_source(source: &str) -> Result<ParsedLedger, ParseError> {
    let mut book: Option<Book> = None;
    let mut statements = Vec::new();
    let mut pending: Option<Pending> = None;
    let mut form_ids: HashMap<String, (String, Location)> = HashMap::new();

    for (zero_line, raw_line) in source.lines().enumerate() {
        let line = zero_line + 1;
        let trimmed = raw_line.trim();
        if trimmed.is_empty() || trimmed.starts_with(';') || trimmed.starts_with('#') {
            continue;
        }
        let indentation = raw_line.len() - raw_line.trim_start_matches([' ', '\t']).len();
        if indentation != 0 {
            let pending_ref = pending.as_mut().ok_or_else(|| {
                ParseError::new(line, indentation + 1, "indented line has no form header")
            })?;
            parse_continuation(pending_ref, trimmed, line, indentation + 1)?;
            continue;
        }

        if let Some(form) = pending.take() {
            statements.push(finish_pending(form, line.saturating_sub(1))?);
        }
        let tokens = words(trimmed);
        if tokens.is_empty() {
            continue;
        }
        let keyword = tokens[0].0.as_str();
        if keyword != "book" && book.is_none() {
            return Err(ParseError::new(
                line,
                1,
                "ledger must begin with a `book NAME` declaration",
            ));
        }
        match keyword {
            "book" => {
                if book.is_some() {
                    return Err(ParseError::new(
                        line,
                        1,
                        "ledger may contain only one `book` declaration",
                    ));
                }
                if tokens.len() != 2 {
                    return Err(ParseError::new(line, 1, "book syntax is `book NAME`"));
                }
                let (name, column) = &tokens[1];
                let name = parse_symbol(name, HoleKind::Book, line, *column)?;
                book = Some(Book {
                    name,
                    location: Location::new(line, 1),
                });
            }
            "buy" => {
                let parsed = parse_header_buy(&tokens, line)?;
                register_form_id(&mut form_ids, "buy", &tokens, line)?;
                pending = Some(parsed);
            }
            "sell" => {
                let parsed = parse_header_sell(&tokens, line)?;
                register_form_id(&mut form_ids, "sell", &tokens, line)?;
                pending = Some(parsed);
            }
            "quote" => {
                let parsed = parse_header_quote(&tokens, line)?;
                register_form_id(&mut form_ids, "quote", &tokens, line)?;
                pending = Some(parsed);
            }
            "observe" => statements.push(parse_observe(&tokens, line)?),
            "use" => statements.push(parse_use(&tokens, line)?),
            "decide" => statements.push(parse_decide(&tokens, line)?),
            other => {
                return Err(ParseError::new(
                    line,
                    1,
                    format!(
                        "unknown directive `{other}`; expected book, buy, sell, quote, observe, use, or decide"
                    ),
                ));
            }
        }
    }
    if let Some(form) = pending.take() {
        statements.push(finish_pending(form, source.lines().count().max(1))?);
    }
    let book = book
        .ok_or_else(|| ParseError::new(1, 1, "ledger must begin with a `book NAME` declaration"))?;
    Ok(ParsedLedger { book, statements })
}

/// Parse and lower a source ledger into the shared domain model used by the
/// resolver and renderer.  The parser-facing form remains available through
/// [`parse_source`], which is the right boundary for tools that need to keep
/// unresolved typed holes and source locations.
pub fn parse_ledger(source: &str) -> Result<crate::model::Ledger, ParseError> {
    let parsed = parse_source(source)?;
    lower_to_model(parsed)
}

fn lower_to_model(parsed: ParsedLedger) -> Result<crate::model::Ledger, ParseError> {
    use crate::model as domain;

    let book_name = known_symbol(&parsed.book.name, parsed.book.location, "book")?;
    let mut ledger = domain::Ledger::new(book_name);

    for statement in parsed.statements {
        let form = match statement {
            Statement::Buy(Buy {
                id,
                date,
                location,
                holding,
                consideration,
                fee,
            }) => {
                let account = known_symbol(&holding.account, location, "account")?.to_owned();
                domain::LedgerForm::Buy(domain::Buy::new(
                    id,
                    lower_date(date, location)?,
                    lower_positive_quantity(holding.quantity, "buy quantity")?,
                    account,
                    lower_non_negative_quantity(consideration, "buy cost")?,
                    fee.map(|fee| lower_non_negative_quantity(fee, "buy fee"))
                        .transpose()?,
                ))
            }
            Statement::Sell(sell) => {
                let Sell {
                    id,
                    date,
                    location,
                    holding,
                    proceeds,
                    lot,
                } = sell;
                let lot = lot.ok_or_else(|| {
                    ParseError::new(
                        location.line,
                        1,
                        "sell form is missing its `lot ?name` line",
                    )
                })?;
                let account = known_symbol(&holding.account, location, "account")?.to_owned();
                domain::LedgerForm::Sell(domain::Sell::new(
                    id,
                    lower_date(date, location)?,
                    lower_positive_quantity(holding.quantity, "sell quantity")?,
                    account,
                    lower_non_negative_quantity(proceeds, "sell proceeds")?,
                    domain::LotSelector::Hole(lower_hole(lot)?),
                ))
            }
            Statement::Quote(quote) => domain::LedgerForm::Quote(domain::Quote::new(
                quote.id,
                lower_date(quote.date, quote.location)?,
                lower_quantity(quote.base)?,
                lower_quantity(quote.quote)?,
            )),
            Statement::Observe(Observation::Position {
                account,
                quantity,
                location,
            }) => domain::LedgerForm::ObservePosition(domain::PositionObservation {
                account: known_symbol(&account, location, "account")?.into(),
                quantity: lower_quantity(quantity)?,
            }),
            Statement::Observe(Observation::Settlement {
                reference,
                quantity,
                into,
                location,
            }) => domain::LedgerForm::ObserveSettlement(domain::SettlementObservation {
                sale: domain::OccurrenceId::new(reference),
                amount: lower_quantity(quantity)?,
                into: into
                    .map(|account| known_symbol(&account, location, "account").map(Into::into))
                    .transpose()?,
            }),
            Statement::Use(UsePolicy {
                policy,
                book,
                location,
            }) => domain::LedgerForm::UsePolicy(domain::PolicyUse {
                book: known_symbol(&book, location, "book")?.into(),
                policy: known_symbol(&policy, location, "policy")?.into(),
            }),
            Statement::Decide(Decision {
                sale,
                lot,
                location,
            }) => domain::LedgerForm::Decide(domain::Decision {
                sale: domain::OccurrenceId::new(sale),
                lot: known_symbol(&lot, location, "lot")?.into(),
            }),
        };
        ledger.push(form);
    }
    Ok(ledger)
}

fn known_symbol<'a>(
    symbol: &'a Symbol,
    location: Location,
    kind: &str,
) -> Result<&'a str, ParseError> {
    symbol.as_known().ok_or_else(|| {
        ParseError::new(
            location.line,
            location.column,
            format!("{kind} holes must be resolved before lowering the ledger model"),
        )
    })
}

fn lower_date(date: Date, location: Location) -> Result<crate::model::Date, ParseError> {
    crate::model::Date::new(date.year as i32, date.month, date.day)
        .map_err(|error| ParseError::new(location.line, location.column, error.to_string()))
}

fn lower_hole(hole: Hole) -> Result<crate::model::Hole, ParseError> {
    match hole.name {
        Some(name) => crate::model::Hole::named(name).map_err(|error| {
            ParseError::new(hole.location.line, hole.location.column, error.to_string())
        }),
        None => Ok(crate::model::Hole::Anonymous),
    }
}

fn lower_quantity(quantity: Quantity) -> Result<crate::model::Quantity, ParseError> {
    let number = match quantity.amount {
        QuantityAmount::Exact(decimal) => {
            decimal
                .as_str()
                .parse()
                .map_err(|error: crate::exact::ExactError| {
                    ParseError::new(
                        quantity.location.line,
                        quantity.location.column,
                        error.to_string(),
                    )
                })?
        }
        QuantityAmount::Hole(hole) => {
            return Err(ParseError::new(
                hole.location.line,
                hole.location.column,
                "amount holes are retained by `parse_source` and cannot lower to a resolved quantity",
            ));
        }
    };
    let unit = match quantity.unit {
        Some(Symbol::Known(value)) => Some(crate::model::Unit::new(value).map_err(|error| {
            ParseError::new(
                quantity.location.line,
                quantity.location.column,
                error.to_string(),
            )
        })?),
        Some(Symbol::Hole(hole)) => {
            return Err(ParseError::new(
                hole.location.line,
                hole.location.column,
                "unit holes are retained by `parse_source` and cannot lower to a resolved quantity",
            ));
        }
        None => None,
    };
    crate::model::Quantity::new(number, unit).map_err(|error| {
        ParseError::new(
            quantity.location.line,
            quantity.location.column,
            error.to_string(),
        )
    })
}

fn lower_positive_quantity(
    quantity: Quantity,
    label: &str,
) -> Result<crate::model::Quantity, ParseError> {
    let location = quantity.location;
    let lowered = lower_quantity(quantity)?;
    if lowered.number.is_zero() || lowered.number.is_negative() {
        return Err(ParseError::new(
            location.line,
            location.column,
            format!("{label} must be greater than zero"),
        ));
    }
    Ok(lowered)
}

fn lower_non_negative_quantity(
    quantity: Quantity,
    label: &str,
) -> Result<crate::model::Quantity, ParseError> {
    let location = quantity.location;
    let lowered = lower_quantity(quantity)?;
    if lowered.number.is_negative() {
        return Err(ParseError::new(
            location.line,
            location.column,
            format!("{label} must be non-negative"),
        ));
    }
    Ok(lowered)
}

fn validate_positive_quantity(quantity: &Quantity, label: &str) -> Result<(), ParseError> {
    let Some(number) = exact_quantity_number(quantity)? else {
        return Ok(());
    };
    if number.is_zero() || number.is_negative() {
        return Err(ParseError::new(
            quantity.location.line,
            quantity.location.column,
            format!("{label} must be greater than zero"),
        ));
    }
    Ok(())
}

fn validate_non_negative_quantity(quantity: &Quantity, label: &str) -> Result<(), ParseError> {
    let Some(number) = exact_quantity_number(quantity)? else {
        return Ok(());
    };
    if number.is_negative() {
        return Err(ParseError::new(
            quantity.location.line,
            quantity.location.column,
            format!("{label} must be non-negative"),
        ));
    }
    Ok(())
}

fn exact_quantity_number(
    quantity: &Quantity,
) -> Result<Option<crate::exact::ExactNumber>, ParseError> {
    let QuantityAmount::Exact(decimal) = &quantity.amount else {
        return Ok(None);
    };
    decimal
        .as_str()
        .parse()
        .map(Some)
        .map_err(|error: crate::exact::ExactError| {
            ParseError::new(
                quantity.location.line,
                quantity.location.column,
                error.to_string(),
            )
        })
}

fn register_form_id(
    form_ids: &mut HashMap<String, (String, Location)>,
    kind: &str,
    tokens: &[(String, usize)],
    line: usize,
) -> Result<(), ParseError> {
    // The corresponding header parser has already checked the token count and
    // ID spelling, so this access cannot fail here.
    let (id, column) = &tokens[1];
    let location = Location::new(line, *column);
    if let Some((first_kind, first_location)) = form_ids.get(id) {
        return Err(ParseError::new(
            line,
            *column,
            format!(
                "duplicate {kind} ID `{id}`; first declared as {first_kind} at line {}, column {}",
                first_location.line, first_location.column
            ),
        ));
    }
    form_ids.insert(id.clone(), (kind.to_owned(), location));
    Ok(())
}

fn parse_header_buy(tokens: &[(String, usize)], line: usize) -> Result<Pending, ParseError> {
    let (id, date) = parse_form_header(tokens, "buy", line)?;
    Ok(Pending::Buy {
        id,
        date,
        location: Location::new(line, 1),
        holding: None,
        consideration: None,
        fee: None,
    })
}

fn parse_header_sell(tokens: &[(String, usize)], line: usize) -> Result<Pending, ParseError> {
    let (id, date) = parse_form_header(tokens, "sell", line)?;
    Ok(Pending::Sell {
        id,
        date,
        location: Location::new(line, 1),
        holding: None,
        proceeds: None,
        lot: None,
    })
}

fn parse_header_quote(tokens: &[(String, usize)], line: usize) -> Result<Pending, ParseError> {
    let (id, date) = parse_form_header(tokens, "quote", line)?;
    Ok(Pending::Quote {
        id,
        date,
        location: Location::new(line, 1),
        base: None,
        quote: None,
    })
}

fn parse_form_header(
    tokens: &[(String, usize)],
    keyword: &str,
    line: usize,
) -> Result<(String, Date), ParseError> {
    if tokens.len() != 4 || tokens[2].0.as_str() != "on" {
        return Err(ParseError::new(
            line,
            1,
            format!("{keyword} syntax is `{keyword} ID on YYYY-MM-DD`"),
        ));
    }
    let (id, id_column) = &tokens[1];
    if !valid_name(id) {
        return Err(ParseError::new(
            line,
            *id_column,
            "form ID must be a name (letters, digits, `/`, `_`, or `-`)",
        ));
    }
    let (date, date_column) = &tokens[3];
    Ok((id.clone(), Date::parse(date, line, *date_column)?))
}

fn parse_continuation(
    pending: &mut Pending,
    line_text: &str,
    line: usize,
    column: usize,
) -> Result<(), ParseError> {
    let tokens = words_at(line_text, column);
    match pending {
        Pending::Buy {
            holding,
            consideration,
            fee,
            ..
        } => {
            if tokens.len() >= 4 && tokens[2].0 == "into" && tokens.len() == 4 {
                if holding.is_some() {
                    return Err(ParseError::new(
                        line,
                        column,
                        "buy form has duplicate holding line",
                    ));
                }
                let quantity = parse_quantity_tokens(&tokens, 0, 1, line)?;
                validate_positive_quantity(&quantity, "buy quantity")?;
                let account = parse_symbol(&tokens[3].0, HoleKind::Account, line, tokens[3].1)?;
                *holding = Some(Holding { quantity, account });
            } else if tokens.len() >= 2 && tokens[0].0 == "for" {
                if tokens.len() < 2 || tokens.len() > 3 {
                    return Err(ParseError::new(
                        line,
                        column,
                        "buy consideration syntax is `for AMOUNT UNIT`",
                    ));
                }
                if consideration.is_some() {
                    return Err(ParseError::new(
                        line,
                        column,
                        "buy form has duplicate `for` line",
                    ));
                }
                let value = parse_quantity_tokens(&tokens, 1, 2, line)?;
                validate_non_negative_quantity(&value, "buy cost")?;
                *consideration = Some(value);
            } else if tokens.len() >= 2 && tokens[0].0 == "fee" {
                if tokens.len() < 2 || tokens.len() > 3 {
                    return Err(ParseError::new(
                        line,
                        column,
                        "buy fee syntax is `fee AMOUNT UNIT`",
                    ));
                }
                if fee.is_some() {
                    return Err(ParseError::new(
                        line,
                        column,
                        "buy form has duplicate `fee` line",
                    ));
                }
                let value = parse_quantity_tokens(&tokens, 1, 2, line)?;
                validate_non_negative_quantity(&value, "buy fee")?;
                *fee = Some(value);
            } else {
                return Err(ParseError::new(
                    line,
                    column,
                    "buy lines must be `AMOUNT UNIT into ACCOUNT`, `for AMOUNT UNIT`, or `fee AMOUNT UNIT`",
                ));
            }
        }
        Pending::Sell {
            holding,
            proceeds,
            lot,
            ..
        } => {
            if tokens.len() == 4 && tokens[2].0 == "from" {
                if holding.is_some() {
                    return Err(ParseError::new(
                        line,
                        column,
                        "sell form has duplicate holding line",
                    ));
                }
                let quantity = parse_quantity_tokens(&tokens, 0, 1, line)?;
                validate_positive_quantity(&quantity, "sell quantity")?;
                let account = parse_symbol(&tokens[3].0, HoleKind::Account, line, tokens[3].1)?;
                *holding = Some(Holding { quantity, account });
            } else if (tokens.len() == 2 || tokens.len() == 3) && tokens[0].0 == "for" {
                if proceeds.is_some() {
                    return Err(ParseError::new(
                        line,
                        column,
                        "sell form has duplicate `for` line",
                    ));
                }
                let value = parse_quantity_tokens(&tokens, 1, 2, line)?;
                validate_non_negative_quantity(&value, "sell proceeds")?;
                *proceeds = Some(value);
            } else if tokens.len() == 2 && tokens[0].0 == "lot" {
                if lot.is_some() {
                    return Err(ParseError::new(
                        line,
                        column,
                        "sell form has duplicate `lot` line",
                    ));
                }
                let hole = Hole::parse(&tokens[1].0, HoleKind::Lot, line, tokens[1].1).ok_or_else(
                    || {
                        ParseError::new(
                            line,
                            tokens[1].1,
                            "lot must be a typed hole such as `?lot` or `?`",
                        )
                    },
                )?;
                *lot = Some(hole);
            } else {
                return Err(ParseError::new(
                    line,
                    column,
                    "sell lines must be `AMOUNT UNIT from ACCOUNT`, `for AMOUNT UNIT`, or `lot ?name`",
                ));
            }
        }
        Pending::Quote { base, quote, .. } => {
            if tokens.len() != 5 || tokens[2].0 != "=" {
                return Err(ParseError::new(
                    line,
                    column,
                    "quote syntax is `AMOUNT UNIT = AMOUNT UNIT`",
                ));
            }
            if base.is_some() || quote.is_some() {
                return Err(ParseError::new(
                    line,
                    column,
                    "quote form has duplicate rate line",
                ));
            }
            *base = Some(parse_quantity_tokens(&tokens, 0, 1, line)?);
            *quote = Some(parse_quantity_tokens(&tokens, 3, 4, line)?);
        }
    }
    Ok(())
}

fn finish_pending(pending: Pending, line: usize) -> Result<Statement, ParseError> {
    match pending {
        Pending::Buy {
            id,
            date,
            location,
            holding,
            consideration,
            fee,
        } => Ok(Statement::Buy(Buy {
            id,
            date,
            holding: holding
                .ok_or_else(|| ParseError::new(line, 1, "buy form is missing its holding line"))?,
            consideration: consideration
                .ok_or_else(|| ParseError::new(line, 1, "buy form is missing its `for` line"))?,
            fee,
            location,
        })),
        Pending::Sell {
            id,
            date,
            location,
            holding,
            proceeds,
            lot,
        } => Ok(Statement::Sell(Sell {
            id,
            date,
            holding: holding
                .ok_or_else(|| ParseError::new(line, 1, "sell form is missing its holding line"))?,
            proceeds: proceeds
                .ok_or_else(|| ParseError::new(line, 1, "sell form is missing its `for` line"))?,
            lot,
            location,
        })),
        Pending::Quote {
            id,
            date,
            location,
            base,
            quote,
        } => Ok(Statement::Quote(Quote {
            id,
            date,
            base: base
                .ok_or_else(|| ParseError::new(line, 1, "quote form is missing its rate line"))?,
            quote: quote
                .ok_or_else(|| ParseError::new(line, 1, "quote form is missing its rate line"))?,
            location,
        })),
    }
}

fn parse_quantity_tokens(
    tokens: &[(String, usize)],
    amount: usize,
    unit: usize,
    line: usize,
) -> Result<Quantity, ParseError> {
    let (amount_token, amount_column) = tokens
        .get(amount)
        .ok_or_else(|| ParseError::new(line, 1, "expected an amount"))?;
    let unit_token = tokens.get(unit).map(|(token, _)| token.as_str());
    let unit_column = tokens
        .get(unit)
        .map_or(*amount_column + amount_token.len(), |(_, column)| *column);
    Quantity::parse(amount_token, unit_token, line, *amount_column, unit_column)
}

fn parse_observe(tokens: &[(String, usize)], line: usize) -> Result<Statement, ParseError> {
    if tokens.len() < 2 {
        return Err(ParseError::new(
            line,
            1,
            "observe syntax is `observe position ACCOUNT AMOUNT UNIT` or `observe settlement ID AMOUNT UNIT [into ACCOUNT]`",
        ));
    }
    match tokens[1].0.as_str() {
        "position" if tokens.len() == 4 || tokens.len() == 5 => {
            let account = parse_symbol(&tokens[2].0, HoleKind::Account, line, tokens[2].1)?;
            let quantity = parse_quantity_tokens(tokens, 3, 4, line)?;
            Ok(Statement::Observe(Observation::Position {
                account,
                quantity,
                location: Location::new(line, 1),
            }))
        }
        "settlement" if matches!(tokens.len(), 4..=7) => {
            let reference =
                parse_known_name(&tokens[2].0, line, tokens[2].1, "settlement reference")?;
            let (quantity, into) = match tokens.len() {
                4 => (parse_quantity_tokens(tokens, 3, tokens.len(), line)?, None),
                5 if tokens[4].0 == "into" => {
                    return Err(ParseError::new(
                        line,
                        tokens[4].1,
                        "settlement syntax is `observe settlement ID AMOUNT UNIT [into ACCOUNT]`",
                    ));
                }
                5 => (parse_quantity_tokens(tokens, 3, 4, line)?, None),
                6 if tokens[4].0 == "into" => (
                    parse_quantity_tokens(tokens, 3, tokens.len(), line)?,
                    Some(parse_symbol(
                        &tokens[5].0,
                        HoleKind::Account,
                        line,
                        tokens[5].1,
                    )?),
                ),
                7 if tokens[5].0 == "into" => (
                    parse_quantity_tokens(tokens, 3, 4, line)?,
                    Some(parse_symbol(
                        &tokens[6].0,
                        HoleKind::Account,
                        line,
                        tokens[6].1,
                    )?),
                ),
                _ => {
                    return Err(ParseError::new(
                        line,
                        1,
                        "settlement syntax is `observe settlement ID AMOUNT UNIT [into ACCOUNT]`",
                    ));
                }
            };
            Ok(Statement::Observe(Observation::Settlement {
                reference,
                quantity,
                into,
                location: Location::new(line, 1),
            }))
        }
        "position" | "settlement" => Err(ParseError::new(
            line,
            1,
            "observe directive has the wrong number of fields",
        )),
        other => Err(ParseError::new(
            line,
            tokens[1].1,
            format!("unknown observation `{other}`; expected `position` or `settlement`"),
        )),
    }
}

fn parse_use(tokens: &[(String, usize)], line: usize) -> Result<Statement, ParseError> {
    if tokens.len() != 4 || tokens[2].0 != "for" {
        return Err(ParseError::new(
            line,
            1,
            "use syntax is `use POLICY for BOOK`",
        ));
    }
    let policy = parse_symbol(&tokens[1].0, HoleKind::Policy, line, tokens[1].1)?;
    let book = parse_symbol(&tokens[3].0, HoleKind::Book, line, tokens[3].1)?;
    Ok(Statement::Use(UsePolicy {
        policy,
        book,
        location: Location::new(line, 1),
    }))
}

fn parse_decide(tokens: &[(String, usize)], line: usize) -> Result<Statement, ParseError> {
    if tokens.len() != 4 || tokens[2].0 != "lot" {
        return Err(ParseError::new(
            line,
            1,
            "decide syntax is `decide SALE_ID lot LOT_ID`",
        ));
    }
    let sale = parse_known_name(&tokens[1].0, line, tokens[1].1, "sale ID")?;
    let lot = parse_symbol(&tokens[3].0, HoleKind::Lot, line, tokens[3].1)?;
    Ok(Statement::Decide(Decision {
        sale,
        lot,
        location: Location::new(line, 1),
    }))
}

fn parse_symbol(
    token: &str,
    kind: HoleKind,
    line: usize,
    column: usize,
) -> Result<Symbol, ParseError> {
    if token.starts_with('?') {
        let hole = Hole::parse(token, kind, line, column).ok_or_else(|| {
            ParseError::new(
                line,
                column,
                format!("invalid {kind} hole; use `?` or `?name`"),
            )
        })?;
        Ok(Symbol::Hole(hole))
    } else {
        Ok(Symbol::Known(parse_known_name(
            token, line, column, "name",
        )?))
    }
}

fn parse_known_name(
    token: &str,
    line: usize,
    column: usize,
    what: &str,
) -> Result<String, ParseError> {
    if valid_name(token) {
        Ok(token.to_owned())
    } else {
        Err(ParseError::new(
            line,
            column,
            format!("{what} `{token}` is not a valid name"),
        ))
    }
}

fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'_' | b'-' | b'.'))
        && value.as_bytes()[0].is_ascii_alphanumeric()
}

fn words(line: &str) -> Vec<(String, usize)> {
    words_at(line, 1)
}

fn words_at(line: &str, offset: usize) -> Vec<(String, usize)> {
    line.split_whitespace()
        .scan(offset, |column, word| {
            let current = *column;
            *column += word.len() + 1;
            Some((word.to_owned(), current))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = r#";
book tax-us

buy buy/one on 2026-01-04
  10 ABC into brokerage
  for 200 USD
  fee 1 USD

buy buy/two on 2026-02-01
  10 ABC into brokerage
  for 300 USD
  fee 1 USD

sell sell on 2026-09-20
  10 ABC from brokerage
  for 500 USD
  lot ?lot

quote quote/one on 2026-09-20
  1 ABC = 52 USD

observe position brokerage 10 ABC
observe settlement sell 500 USD into checking
"#;

    #[test]
    fn parses_the_canonical_fixture_shape_and_keeps_holes_typed() {
        let ledger = parse_source(BASE).expect("canonical source parses");
        assert_eq!(ledger.book.name.as_known(), Some("tax-us"));
        assert_eq!(ledger.statements.len(), 6);
        let sell = ledger
            .statements
            .iter()
            .find_map(|statement| match statement {
                Statement::Sell(sell) => Some(sell),
                _ => None,
            })
            .expect("sell");
        assert_eq!(
            sell.lot.as_ref().and_then(|hole| hole.name.as_deref()),
            Some("lot")
        );
        assert_eq!(sell.lot.as_ref().map(|hole| hole.kind), Some(HoleKind::Lot));
    }

    #[test]
    fn ignores_blank_lines_and_comments_without_changing_locations() {
        let source = "# heading\n\nbook tax-us\n; spacer\nobserve position brokerage 10 ABC\n";
        let ledger = parse_source(source).expect("source parses");
        let Statement::Observe(Observation::Position { location, .. }) = &ledger.statements[0]
        else {
            panic!("position observation")
        };
        assert_eq!(location.line, 5);
    }

    #[test]
    fn rejects_nonzero_quantity_without_unit_at_the_offending_line() {
        let error =
            parse_source("book tax-us\nobserve position brokerage 10\n").expect_err("missing unit");
        assert_eq!(error.location.line, 2);
        assert_eq!(error.location.column, 30);
        assert!(error.message.contains("explicit unit"));
    }

    #[test]
    fn rejects_invalid_dates_precisely() {
        let error = parse_source("book tax-us\nbuy one on 2026-02-30\n").expect_err("invalid date");
        assert_eq!(error.location.line, 2);
        assert_eq!(error.location.column, 12);
        assert!(error.message.contains("calendar date"));
    }

    #[test]
    fn accepts_anonymous_typed_holes() {
        let source =
            "book tax-us\nsell one on 2026-09-20\n  ? ? from brokerage\n  for 0\n  lot ?\n";
        let ledger = parse_source(source).expect("holes parse");
        let Statement::Sell(sell) = &ledger.statements[0] else {
            panic!("sell")
        };
        assert!(matches!(sell.holding.quantity.unit, Some(Symbol::Hole(_))));
        assert_eq!(
            sell.lot.as_ref().and_then(|hole| hole.name.as_deref()),
            None
        );
    }

    #[test]
    fn reports_unknown_directives_as_line_errors() {
        let error = parse_source("book tax-us\nwat tax-us\n").expect_err("unknown directive");
        assert_eq!(error.location, Location::new(2, 1));
        assert!(error.message.contains("unknown directive"));
    }

    #[test]
    fn canonical_fixtures_are_all_source_ledgers() {
        for fixture in [
            include_str!("../examples/ambiguous.axm"),
            include_str!("../examples/fifo.axm"),
            include_str!("../examples/fifo-conflict.axm"),
        ] {
            parse_source(fixture).expect("fixture parses");
        }
    }

    #[test]
    fn rejects_duplicate_form_ids_at_the_second_id() {
        let source = r#"book tax-us
buy one on 2026-01-01
  1 ABC into checking
  for 1 USD
buy one on 2026-01-02
"#;
        let error = parse_source(source).expect_err("duplicate ID");
        assert_eq!(error.location, Location::new(5, 5));
        assert!(error.message.contains("duplicate buy ID `one`"));
    }

    #[test]
    fn decisions_name_the_sale_they_constrain() {
        let source = "book tax-us\n\
decide sale/two lot buy/one\n";
        let ledger = parse_source(source).expect("decision parses");
        let Statement::Decide(decision) = &ledger.statements[0] else {
            panic!("decision")
        };
        assert_eq!(decision.sale, "sale/two");
        assert_eq!(decision.lot.as_known(), Some("buy/one"));
    }

    #[test]
    fn settlement_can_name_an_account() {
        let source = "book tax-us\nobserve settlement sale 5 USD into checking\n";
        let ledger = parse_source(source).expect("settlement parses");
        let Statement::Observe(Observation::Settlement { into, .. }) = &ledger.statements[0] else {
            panic!("settlement")
        };
        assert_eq!(into.as_ref().and_then(Symbol::as_known), Some("checking"));
    }

    #[test]
    fn lowering_rejects_invalid_trade_signs() {
        let source = r#"book tax-us
buy one on 2026-01-01
  0 ABC into checking
  for 1 USD
"#;
        let error = parse_ledger(source).expect_err("zero buy quantity");
        assert_eq!(error.location, Location::new(3, 3));
        assert_eq!(error.message, "buy quantity must be greater than zero");
    }
}
