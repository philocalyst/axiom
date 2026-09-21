//! Parser for Axiom's intentionally small, line-oriented source ledger.
//!
//! The parser does not try to be clever.  A source ledger is a durable piece of
//! evidence, so preserving where a value came from is more useful than
//! accepting a vaguely similar spelling.  Every node therefore carries its
//! source line and diagnostics point at the first character that made a line
//! invalid.

use std::collections::HashMap;
use std::fmt;

use crate::surface::{SurfaceFile, Token, TokenKind};

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
    Entity,
    Lot,
    Instrument,
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
            Self::Entity => "entity",
            Self::Lot => "lot",
            Self::Instrument => "instrument",
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
    Obligation(Obligation),
    Settlement(Settlement),
    Satisfy(Satisfy),
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

/// A promised transfer in the obligation authoring form.
///
/// The transfer's instrument is the quantity unit.  Keeping the source
/// quantity intact here lets callers retain typed holes and exact decimal
/// spellings until model elaboration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Performance {
    Transfer { quantity: Quantity },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Obligation {
    pub id: String,
    pub debtor: Symbol,
    pub creditor: Symbol,
    pub performance: Performance,
    pub due: Option<Date>,
    pub location: Location,
}

/// One settlement state observation.  The vector on [`Settlement`] is kept in
/// source order deliberately: state history is evidence, not a set that can
/// be sorted into a valid-looking sequence later.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettlementState {
    pub state: String,
    pub at: Option<Date>,
    pub location: Location,
}

/// A settlement and its ordered state observations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Settlement {
    pub id: String,
    pub kind: String,
    pub from: Symbol,
    pub to: Symbol,
    /// The settlement instrument is normally derived from `amount.unit`.
    /// The optional legacy `instrument` line remains accepted for source
    /// compatibility and is checked against that unit during finishing.
    pub instrument: Symbol,
    pub amount: Quantity,
    pub states: Vec<SettlementState>,
    pub location: Location,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Satisfy {
    pub id: String,
    pub obligation: String,
    pub settlement: String,
    pub amount: Quantity,
    pub state: String,
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
    Obligation {
        id: String,
        location: Location,
        debtor: Option<Symbol>,
        creditor: Option<Symbol>,
        performance: Option<Performance>,
        due: Option<Date>,
    },
    Settlement {
        id: String,
        location: Location,
        kind: Option<String>,
        from: Option<Symbol>,
        to: Option<Symbol>,
        instrument: Option<Symbol>,
        instrument_location: Option<Location>,
        amount: Option<Quantity>,
        states: Vec<SettlementState>,
    },
    Satisfy {
        id: String,
        location: Location,
        obligation: Option<String>,
        settlement: Option<String>,
        amount: Option<Quantity>,
        state: Option<String>,
    },
}

/// Parse one complete source ledger into the parser-facing representation.
pub fn parse_source(source: &str) -> Result<ParsedLedger, ParseError> {
    let surface = SurfaceFile::parse(source);
    parse_surface_file(&surface)
}

/// Parse a lossless authoring surface into the strict parser representation.
///
/// `SurfaceFile` is the one lexical boundary for the project.  This function
/// deliberately consumes its token stream instead of re-scanning source text,
/// while retaining the parser's existing line-oriented grammar and diagnostic
/// wording.  The tolerant surface can therefore be used by editors and the
/// strict result by the semantic elaborator without introducing another AST.
pub fn parse_surface_file(surface: &SurfaceFile) -> Result<ParsedLedger, ParseError> {
    let mut book: Option<Book> = None;
    let mut statements = Vec::new();
    let mut pending: Option<Pending> = None;
    let mut form_ids: HashMap<String, (String, Location)> = HashMap::new();

    for (line, line_tokens) in surface_lines(surface) {
        let tokens = surface_words(&line_tokens);
        if tokens.is_empty() {
            continue;
        }
        // A comment in the first non-trivia position is a source comment,
        // matching the old line parser's `trim().starts_with(...)` rule.  An
        // inline comment remains words below, preserving its historical
        // strict-grammar behavior and diagnostics.
        if first_meaningful(&line_tokens)
            .is_some_and(|token| matches!(token.kind, TokenKind::Comment))
        {
            continue;
        }
        let indentation = tokens
            .first()
            .map_or(0, |(_, column)| column.saturating_sub(1));
        if indentation != 0 {
            let pending_ref = pending.as_mut().ok_or_else(|| {
                ParseError::new(line, indentation + 1, "indented line has no form header")
            })?;
            parse_continuation(pending_ref, &tokens, line, indentation + 1)?;
            continue;
        }

        if let Some(form) = pending.take() {
            statements.push(finish_pending(form, line.saturating_sub(1))?);
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
            "obligation" => {
                let parsed = parse_header_obligation(&tokens, line)?;
                register_form_id(&mut form_ids, "obligation", &tokens, line)?;
                pending = Some(parsed);
            }
            "settlement" => {
                let parsed = parse_header_settlement(&tokens, line)?;
                register_form_id(&mut form_ids, "settlement", &tokens, line)?;
                pending = Some(parsed);
            }
            "satisfy" => {
                let parsed = parse_header_satisfy(&tokens, line)?;
                register_form_id(&mut form_ids, "satisfy", &tokens, line)?;
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
                        "unknown directive `{other}`; expected book, buy, sell, quote, obligation, settlement, satisfy, observe, use, or decide"
                    ),
                ));
            }
        }
    }
    if let Some(form) = pending.take() {
        statements.push(finish_pending(form, source_line_count(surface).max(1))?);
    }
    let book = book
        .ok_or_else(|| ParseError::new(1, 1, "ledger must begin with a `book NAME` declaration"))?;
    Ok(ParsedLedger { book, statements })
}

/// Parse and lower a source ledger into the shared domain model used by the
/// resolver and renderer. The parser-facing form remains available through
/// [`parse_source`], which is the right boundary for tools that need to keep
/// unresolved typed holes and source locations. Production analysis should
/// use [`crate::workspace::Workspace::analyze_commit`] so source bytes are
/// committed before this lowering is evaluated.
pub fn parse_ledger(source: &str) -> Result<crate::model::Ledger, ParseError> {
    let surface = SurfaceFile::parse(source);
    parse_surface_ledger(&surface)
}

/// Strictly parse and elaborate an already-tokenized authoring surface into
/// the shared domain ledger.  Keeping this separate from [`parse_ledger`]
/// makes the single source-to-model path explicit for editor and formatter
/// integrations while preserving the historical string API.
pub fn parse_surface_ledger(surface: &SurfaceFile) -> Result<crate::model::Ledger, ParseError> {
    let parsed = parse_surface_file(surface)?;
    lower_to_model(parsed)
}

/// Build logical source lines from the surface token stream.  Newline tokens
/// are structural, so CRLF and LF share the same strict parser behavior while
/// the original bytes remain available through `SurfaceFile::source()`.
fn surface_lines(surface: &SurfaceFile) -> Vec<(usize, Vec<Token>)> {
    let mut lines = Vec::new();
    let mut current_line = 1;
    let mut current = Vec::new();
    for token in surface.tokens() {
        if token.kind == TokenKind::Newline {
            lines.push((current_line, std::mem::take(&mut current)));
            current_line += 1;
        } else {
            current.push(token.clone());
        }
    }
    // `str::lines` (used by the former parser) does not yield an additional
    // empty line after a trailing newline, so only emit an unterminated final
    // line when there are bytes after the last newline.
    if !current.is_empty() || surface.source().is_empty() {
        lines.push((current_line, current));
    }
    lines
}

fn source_line_count(surface: &SurfaceFile) -> usize {
    surface.tokens().last().map_or(1, |token| token.span.line)
}

fn first_meaningful(tokens: &[Token]) -> Option<&Token> {
    // Comments are trivia to the CST, but they are still the first lexical
    // item needed to recognize a comment-only source line here.
    tokens
        .iter()
        .find(|token| !matches!(token.kind, TokenKind::Whitespace | TokenKind::Newline))
}

/// Convert one surface line into the parser's historical `(word, column)`
/// representation.  Comments are deliberately split into words here because
/// the strict parser historically treated an inline `#`/`;` as ordinary input;
/// comment-only lines are filtered before this helper is called.
fn surface_words(tokens: &[Token]) -> Vec<(String, usize)> {
    let mut words = Vec::new();
    for token in tokens {
        if matches!(token.kind, TokenKind::Whitespace | TokenKind::Newline) {
            continue;
        }
        if token.kind == TokenKind::Comment {
            words.extend(words_at(&token.lexeme, token.span.column));
            continue;
        }
        words.push((token.lexeme.clone(), token.span.column));
    }
    words
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
                domain::Buy::new(
                    id,
                    lower_date(date, location)?,
                    lower_positive_quantity(holding.quantity, "buy quantity")?,
                    account,
                    lower_non_negative_quantity(consideration, "buy cost")?,
                    fee.map(|fee| lower_non_negative_quantity(fee, "buy fee"))
                        .transpose()?,
                )
                .map(domain::LedgerForm::Buy)
                .map_err(|error| {
                    ParseError::new(location.line, location.column, error.to_string())
                })?
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
                domain::Sell::new(
                    id,
                    lower_date(date, location)?,
                    lower_positive_quantity(holding.quantity, "sell quantity")?,
                    account,
                    lower_non_negative_quantity(proceeds, "sell proceeds")?,
                    domain::LotSelector::Hole(lower_hole(lot)?),
                )
                .map(domain::LedgerForm::Sell)
                .map_err(|error| {
                    ParseError::new(location.line, location.column, error.to_string())
                })?
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
                occurrence: domain::OccurrenceId::new(format!("position/{}", location.line)),
                account: known_symbol(&account, location, "account")?.into(),
                quantity: lower_quantity(quantity)?,
            }),
            Statement::Observe(Observation::Settlement {
                reference,
                quantity,
                into,
                location,
            }) => domain::LedgerForm::ObserveSettlement(domain::SettlementObservation {
                occurrence: domain::OccurrenceId::new(format!("settlement/{}", location.line)),
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
            Statement::Obligation(obligation) => {
                let Performance::Transfer { quantity } = obligation.performance;
                domain::LedgerForm::Obligation(domain::SourceObligation {
                    occurrence: domain::OccurrenceId::new(obligation.id),
                    debtor: known_symbol(&obligation.debtor, obligation.location, "debtor")?.into(),
                    creditor: known_symbol(&obligation.creditor, obligation.location, "creditor")?
                        .into(),
                    quantity: lower_quantity(quantity)?,
                    due: obligation
                        .due
                        .map(|date| lower_date(date, obligation.location))
                        .transpose()?,
                })
            }
            Statement::Settlement(settlement) => {
                let kind = match settlement.kind.as_str() {
                    "ach" => domain::SettlementKind::Ach,
                    "card" => domain::SettlementKind::Card,
                    "check" => domain::SettlementKind::Check,
                    other => {
                        return Err(ParseError::new(
                            settlement.location.line,
                            settlement.location.column,
                            format!(
                                "unknown settlement kind `{other}`; expected ach, card, or check"
                            ),
                        ));
                    }
                };
                let history = settlement
                    .states
                    .into_iter()
                    .map(|state| {
                        Ok(domain::SourceSettlementState {
                            state: lower_settlement_state(&state.state, state.location)?,
                            at: state
                                .at
                                .map(|date| lower_date(date, state.location))
                                .transpose()?,
                        })
                    })
                    .collect::<Result<Vec<_>, ParseError>>()?;
                domain::LedgerForm::Settlement(domain::SourceSettlement {
                    occurrence: domain::OccurrenceId::new(settlement.id),
                    kind,
                    from: known_symbol(&settlement.from, settlement.location, "settlement from")?
                        .into(),
                    to: known_symbol(&settlement.to, settlement.location, "settlement to")?.into(),
                    instrument: known_symbol(
                        &settlement.instrument,
                        settlement.location,
                        "instrument",
                    )?
                    .into(),
                    amount: lower_quantity(settlement.amount)?,
                    history,
                })
            }
            Statement::Satisfy(satisfy) => {
                domain::LedgerForm::Satisfaction(domain::SourceSatisfaction {
                    occurrence: domain::OccurrenceId::new(satisfy.id),
                    obligation: domain::OccurrenceId::new(satisfy.obligation),
                    settlement: domain::OccurrenceId::new(satisfy.settlement),
                    amount: lower_quantity(satisfy.amount)?,
                    state: match satisfy.state.as_str() {
                        "proposed" => domain::SatisfactionState::Proposed,
                        "applied" => domain::SatisfactionState::Applied,
                        "reversed" => domain::SatisfactionState::Reversed,
                        _ => unreachable!("parser validates satisfaction states"),
                    },
                })
            }
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

fn lower_settlement_state(
    state: &str,
    location: Location,
) -> Result<crate::model::SettlementStateKind, ParseError> {
    use crate::model::SettlementStateKind as State;
    match state {
        "issued" => Ok(State::Issued),
        "authorized" => Ok(State::Authorized),
        "presented" => Ok(State::Presented),
        "pending" => Ok(State::Pending),
        "settled" => Ok(State::Settled),
        "returned" => Ok(State::Returned),
        "reversed" => Ok(State::Reversed),
        "rejected" => Ok(State::Rejected),
        "cancelled" => Ok(State::Cancelled),
        "refunded" => Ok(State::Refunded),
        "disputed" => Ok(State::Disputed),
        "charged-back" => Ok(State::ChargedBack),
        "represented" => Ok(State::Represented),
        "resolved" => Ok(State::Resolved),
        other => Err(ParseError::new(
            location.line,
            location.column,
            format!("unknown settlement state `{other}`"),
        )),
    }
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

fn validate_observation_quantity(quantity: &Quantity, label: &str) -> Result<(), ParseError> {
    if quantity.unit.is_none()
        && matches!(&quantity.amount, QuantityAmount::Exact(value) if value
            .as_str()
            .chars()
            .all(|character| matches!(character, '0' | '.' | '-')))
    {
        return Err(ParseError::new(
            quantity.location.line,
            quantity.location.column,
            format!("{label} observations require an explicit unit, even for zero"),
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

fn parse_header_obligation(tokens: &[(String, usize)], line: usize) -> Result<Pending, ParseError> {
    let id = parse_block_header(tokens, "obligation", line)?;
    Ok(Pending::Obligation {
        id,
        location: Location::new(line, 1),
        debtor: None,
        creditor: None,
        performance: None,
        due: None,
    })
}

fn parse_header_settlement(tokens: &[(String, usize)], line: usize) -> Result<Pending, ParseError> {
    let id = parse_block_header(tokens, "settlement", line)?;
    Ok(Pending::Settlement {
        id,
        location: Location::new(line, 1),
        kind: None,
        from: None,
        to: None,
        instrument: None,
        instrument_location: None,
        amount: None,
        states: Vec::new(),
    })
}

fn parse_header_satisfy(tokens: &[(String, usize)], line: usize) -> Result<Pending, ParseError> {
    let id = parse_block_header(tokens, "satisfy", line)?;
    Ok(Pending::Satisfy {
        id,
        location: Location::new(line, 1),
        obligation: None,
        settlement: None,
        amount: None,
        state: None,
    })
}

fn parse_block_header(
    tokens: &[(String, usize)],
    keyword: &str,
    line: usize,
) -> Result<String, ParseError> {
    if tokens.len() != 2 {
        return Err(ParseError::new(
            line,
            1,
            format!("{keyword} syntax is `{keyword} ID`"),
        ));
    }
    parse_known_name(&tokens[1].0, line, tokens[1].1, "form ID")
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
    tokens: &[(String, usize)],
    line: usize,
    column: usize,
) -> Result<(), ParseError> {
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
                let quantity = parse_quantity_tokens(tokens, 0, 1, line)?;
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
                let value = parse_quantity_tokens(tokens, 1, 2, line)?;
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
                let value = parse_quantity_tokens(tokens, 1, 2, line)?;
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
                let quantity = parse_quantity_tokens(tokens, 0, 1, line)?;
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
                let value = parse_quantity_tokens(tokens, 1, 2, line)?;
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
            let base_quantity = parse_quantity_tokens(tokens, 0, 1, line)?;
            let quote_quantity = parse_quantity_tokens(tokens, 3, 4, line)?;
            validate_positive_quantity(&base_quantity, "quote base quantity")?;
            validate_positive_quantity(&quote_quantity, "quote counter quantity")?;
            *base = Some(base_quantity);
            *quote = Some(quote_quantity);
        }
        Pending::Obligation {
            debtor,
            creditor,
            performance,
            due,
            ..
        } => match tokens.first().map(|token| token.0.as_str()) {
            Some("debtor") if tokens.len() == 2 => {
                if debtor.is_some() {
                    return Err(ParseError::new(
                        line,
                        column,
                        "obligation form has duplicate `debtor` line",
                    ));
                }
                *debtor = Some(parse_symbol(
                    &tokens[1].0,
                    HoleKind::Entity,
                    line,
                    tokens[1].1,
                )?);
            }
            Some("creditor") if tokens.len() == 2 => {
                if creditor.is_some() {
                    return Err(ParseError::new(
                        line,
                        column,
                        "obligation form has duplicate `creditor` line",
                    ));
                }
                *creditor = Some(parse_symbol(
                    &tokens[1].0,
                    HoleKind::Entity,
                    line,
                    tokens[1].1,
                )?);
            }
            Some("performance") if matches!(tokens.len(), 3 | 4) && tokens[1].0 == "transfer" => {
                if performance.is_some() {
                    return Err(ParseError::new(
                        line,
                        column,
                        "obligation form has duplicate `performance` line",
                    ));
                }
                let quantity = parse_quantity_tokens(tokens, 2, 3, line)?;
                validate_positive_quantity(&quantity, "obligation transfer quantity")?;
                *performance = Some(Performance::Transfer { quantity });
            }
            Some("due") if tokens.len() == 2 => {
                if due.is_some() {
                    return Err(ParseError::new(
                        line,
                        column,
                        "obligation form has duplicate `due` line",
                    ));
                }
                *due = Some(Date::parse(&tokens[1].0, line, tokens[1].1)?);
            }
            Some("performance") => {
                return Err(ParseError::new(
                    line,
                    column,
                    "obligation performance syntax is `performance transfer AMOUNT UNIT`",
                ));
            }
            Some("debtor") | Some("creditor") | Some("due") => {
                return Err(ParseError::new(
                    line,
                    column,
                    "obligation line has the wrong number of fields",
                ));
            }
            _ => {
                return Err(ParseError::new(
                    line,
                    column,
                    "obligation lines must be `debtor ENTITY`, `creditor ENTITY`, `performance transfer AMOUNT UNIT`, or `due YYYY-MM-DD`",
                ));
            }
        },
        Pending::Settlement {
            kind,
            from,
            to,
            instrument,
            instrument_location,
            amount,
            states,
            ..
        } => match tokens.first().map(|token| token.0.as_str()) {
            Some("kind") if tokens.len() == 2 => {
                if kind.is_some() {
                    return Err(ParseError::new(
                        line,
                        column,
                        "settlement form has duplicate `kind` line",
                    ));
                }
                let parsed = parse_known_name(&tokens[1].0, line, tokens[1].1, "settlement kind")?;
                if !matches!(parsed.as_str(), "ach" | "card" | "check") {
                    return Err(ParseError::new(
                        line,
                        tokens[1].1,
                        format!("unknown settlement kind `{parsed}`; expected ach, card, or check"),
                    ));
                }
                *kind = Some(parsed);
            }
            Some("from") if tokens.len() == 2 => {
                if from.is_some() {
                    return Err(ParseError::new(
                        line,
                        column,
                        "settlement form has duplicate `from` line",
                    ));
                }
                *from = Some(parse_symbol(
                    &tokens[1].0,
                    HoleKind::Entity,
                    line,
                    tokens[1].1,
                )?);
            }
            Some("to") if tokens.len() == 2 => {
                if to.is_some() {
                    return Err(ParseError::new(
                        line,
                        column,
                        "settlement form has duplicate `to` line",
                    ));
                }
                *to = Some(parse_symbol(
                    &tokens[1].0,
                    HoleKind::Entity,
                    line,
                    tokens[1].1,
                )?);
            }
            // `amount AMOUNT UNIT` already names the settlement instrument.
            // Keep accepting the original explicit line as a compatibility
            // spelling, but do not make authors repeat the same fact.
            Some("instrument") if tokens.len() == 2 => {
                if instrument.is_some() {
                    return Err(ParseError::new(
                        line,
                        column,
                        "settlement form has duplicate `instrument` line",
                    ));
                }
                *instrument = Some(parse_symbol(
                    &tokens[1].0,
                    HoleKind::Instrument,
                    line,
                    tokens[1].1,
                )?);
                *instrument_location = Some(Location::new(line, column));
            }
            Some("amount") if matches!(tokens.len(), 2 | 3) => {
                if amount.is_some() {
                    return Err(ParseError::new(
                        line,
                        column,
                        "settlement form has duplicate `amount` line",
                    ));
                }
                let value = parse_quantity_tokens(tokens, 1, 2, line)?;
                validate_positive_quantity(&value, "settlement amount")?;
                *amount = Some(value);
            }
            Some("state") if matches!(tokens.len(), 2 | 4) => {
                states.push(parse_settlement_state(tokens, line, column)?);
            }
            Some("kind") | Some("from") | Some("to") | Some("instrument") | Some("amount")
            | Some("state") => {
                return Err(ParseError::new(
                    line,
                    column,
                    "settlement line has the wrong number of fields",
                ));
            }
            _ => {
                return Err(ParseError::new(
                    line,
                    column,
                    "settlement lines must be `kind KIND`, `from ENTITY`, `to ENTITY`, `amount AMOUNT UNIT`, optional `instrument INSTRUMENT`, or `state STATE [at YYYY-MM-DD]`",
                ));
            }
        },
        Pending::Satisfy {
            obligation,
            settlement,
            amount,
            state,
            ..
        } => match tokens.first().map(|token| token.0.as_str()) {
            Some("obligation") if tokens.len() == 2 => {
                if obligation.is_some() {
                    return Err(ParseError::new(
                        line,
                        column,
                        "satisfy form has duplicate `obligation` line",
                    ));
                }
                *obligation = Some(parse_known_name(
                    &tokens[1].0,
                    line,
                    tokens[1].1,
                    "obligation ID",
                )?);
            }
            Some("settlement") if tokens.len() == 2 => {
                if settlement.is_some() {
                    return Err(ParseError::new(
                        line,
                        column,
                        "satisfy form has duplicate `settlement` line",
                    ));
                }
                *settlement = Some(parse_known_name(
                    &tokens[1].0,
                    line,
                    tokens[1].1,
                    "settlement ID",
                )?);
            }
            Some("amount") if matches!(tokens.len(), 2 | 3) => {
                if amount.is_some() {
                    return Err(ParseError::new(
                        line,
                        column,
                        "satisfy form has duplicate `amount` line",
                    ));
                }
                let value = parse_quantity_tokens(tokens, 1, 2, line)?;
                validate_positive_quantity(&value, "satisfaction amount")?;
                *amount = Some(value);
            }
            Some("state") if tokens.len() == 2 => {
                if state.is_some() {
                    return Err(ParseError::new(
                        line,
                        column,
                        "satisfy form has duplicate `state` line",
                    ));
                }
                *state = Some(parse_satisfaction_state(&tokens[1].0, line, tokens[1].1)?);
            }
            Some("obligation") | Some("settlement") | Some("amount") | Some("state") => {
                return Err(ParseError::new(
                    line,
                    column,
                    "satisfy line has the wrong number of fields",
                ));
            }
            _ => {
                return Err(ParseError::new(
                    line,
                    column,
                    "satisfy lines must be `obligation ID`, `settlement ID`, `amount AMOUNT UNIT`, or `state STATE`",
                ));
            }
        },
    }
    Ok(())
}

fn parse_settlement_state(
    tokens: &[(String, usize)],
    line: usize,
    column: usize,
) -> Result<SettlementState, ParseError> {
    let state = parse_known_name(&tokens[1].0, line, tokens[1].1, "settlement state")?;
    if !matches!(
        state.as_str(),
        "issued"
            | "authorized"
            | "presented"
            | "pending"
            | "settled"
            | "returned"
            | "reversed"
            | "rejected"
            | "cancelled"
            | "refunded"
            | "disputed"
            | "charged-back"
            | "represented"
            | "resolved"
    ) {
        return Err(ParseError::new(
            line,
            tokens[1].1,
            format!("unknown settlement state `{state}`"),
        ));
    }
    let at = if tokens.len() == 4 {
        if tokens[2].0 != "at" {
            return Err(ParseError::new(
                line,
                column,
                "settlement state syntax is `state STATE [at YYYY-MM-DD]`",
            ));
        }
        Some(Date::parse(&tokens[3].0, line, tokens[3].1)?)
    } else {
        None
    };
    Ok(SettlementState {
        state,
        at,
        location: Location::new(line, column),
    })
}

fn parse_satisfaction_state(token: &str, line: usize, column: usize) -> Result<String, ParseError> {
    let state = parse_known_name(token, line, column, "satisfaction state")?;
    if !matches!(state.as_str(), "proposed" | "applied" | "reversed") {
        return Err(ParseError::new(
            line,
            column,
            format!(
                "unknown satisfaction state `{state}`; expected proposed, applied, or reversed"
            ),
        ));
    }
    Ok(state)
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
        Pending::Obligation {
            id,
            location,
            debtor,
            creditor,
            performance,
            due,
        } => Ok(Statement::Obligation(Obligation {
            id,
            debtor: debtor.ok_or_else(|| {
                ParseError::new(line, 1, "obligation form is missing its `debtor` line")
            })?,
            creditor: creditor.ok_or_else(|| {
                ParseError::new(line, 1, "obligation form is missing its `creditor` line")
            })?,
            performance: performance.ok_or_else(|| {
                ParseError::new(line, 1, "obligation form is missing its `performance` line")
            })?,
            due,
            location,
        })),
        Pending::Settlement {
            id,
            location,
            kind,
            from,
            to,
            instrument,
            instrument_location,
            amount,
            states,
        } => {
            if states.is_empty() {
                return Err(ParseError::new(
                    line,
                    1,
                    "settlement form is missing at least one `state` line",
                ));
            }
            let amount = amount.ok_or_else(|| {
                ParseError::new(line, 1, "settlement form is missing its `amount` line")
            })?;
            let amount_unit = amount.unit.clone().ok_or_else(|| {
                ParseError::new(
                    amount.location.line,
                    amount.location.column,
                    "settlement amount must name its instrument unit",
                )
            })?;
            let instrument = match instrument {
                Some(instrument) if instrument != amount_unit => {
                    let instrument_location = instrument_location.unwrap_or(location);
                    return Err(ParseError::new(
                        instrument_location.line,
                        instrument_location.column,
                        "settlement instrument must match the amount unit",
                    ));
                }
                Some(instrument) => instrument,
                None => amount_unit,
            };
            Ok(Statement::Settlement(Settlement {
                id,
                kind: kind.ok_or_else(|| {
                    ParseError::new(line, 1, "settlement form is missing its `kind` line")
                })?,
                from: from.ok_or_else(|| {
                    ParseError::new(line, 1, "settlement form is missing its `from` line")
                })?,
                to: to.ok_or_else(|| {
                    ParseError::new(line, 1, "settlement form is missing its `to` line")
                })?,
                amount,
                instrument,
                states,
                location,
            }))
        }
        Pending::Satisfy {
            id,
            location,
            obligation,
            settlement,
            amount,
            state,
        } => Ok(Statement::Satisfy(Satisfy {
            id,
            obligation: obligation.ok_or_else(|| {
                ParseError::new(line, 1, "satisfy form is missing its `obligation` line")
            })?,
            settlement: settlement.ok_or_else(|| {
                ParseError::new(line, 1, "satisfy form is missing its `settlement` line")
            })?,
            amount: amount.ok_or_else(|| {
                ParseError::new(line, 1, "satisfy form is missing its `amount` line")
            })?,
            state: state.ok_or_else(|| {
                ParseError::new(line, 1, "satisfy form is missing its `state` line")
            })?,
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
            validate_observation_quantity(&quantity, "position")?;
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
            validate_observation_quantity(&quantity, "settlement")?;
            validate_positive_quantity(&quantity, "settlement amount")?;
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
    use crate::surface::SurfaceFile;

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
        let source = r#"book tax-us
decide sale/two lot buy/one
"#;
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

    #[test]
    fn strict_parser_consumes_the_lossless_surface() {
        let surface = SurfaceFile::parse(BASE);
        let parsed = parse_surface_file(&surface).expect("surface parses strictly");
        assert_eq!(parsed, parse_source(BASE).expect("source parses strictly"));
        assert_eq!(surface.lossless(), BASE);
        assert_eq!(parse_surface_ledger(&surface), parse_ledger(BASE));
    }

    #[test]
    fn surface_recovery_does_not_replace_strict_diagnostics() {
        let source = "book\nwat tax-us\n";
        let surface = SurfaceFile::parse(source);
        assert!(surface.nodes().iter().any(|node| node.is_error()));
        let error = parse_surface_file(&surface).expect_err("unknown directive");
        assert_eq!(error.location, Location::new(1, 1));
        assert!(error.message.contains("book syntax"));
    }

    #[test]
    fn observations_and_quotes_reject_ambiguous_or_negative_amounts() {
        for source in [
            "book tax-us\nobserve position checking 0\n",
            "book tax-us\nobserve position checking 0.0\n",
            "book tax-us\nobserve settlement sale 0 USD\n",
            "book tax-us\nobserve settlement sale -1 USD\n",
            "book tax-us\nquote q on 2026-01-01\n  0 ABC = 1 USD\n",
        ] {
            assert!(
                parse_ledger(source).is_err(),
                "source should be rejected: {source}"
            );
        }
    }

    #[test]
    fn parses_obligation_settlement_and_satisfaction_forms() {
        let source = r#"book tax-us
obligation invoice/1
  debtor alice
  creditor bob
  performance transfer 100 USD
  due 2026-12-31
settlement payment/1
  kind ach
  from alice
  to bob
  instrument USD
  amount 100 USD
  state issued at 2026-01-01
  state presented at 2026-01-02
  state settled at 2026-01-03
satisfy allocation/1
  obligation invoice/1
  settlement payment/1
  amount 100 USD
  state applied
"#;
        let ledger = parse_source(source).expect("ontology forms parse");
        assert_eq!(ledger.statements.len(), 3);
        let Statement::Obligation(obligation) = &ledger.statements[0] else {
            panic!("obligation")
        };
        assert_eq!(obligation.id, "invoice/1");
        assert_eq!(
            obligation.due,
            Some(Date {
                year: 2026,
                month: 12,
                day: 31
            })
        );
        let Statement::Settlement(settlement) = &ledger.statements[1] else {
            panic!("settlement")
        };
        assert_eq!(
            settlement
                .states
                .iter()
                .map(|state| state.state.as_str())
                .collect::<Vec<_>>(),
            ["issued", "presented", "settled"]
        );
        let Statement::Satisfy(satisfy) = &ledger.statements[2] else {
            panic!("satisfy")
        };
        assert_eq!(satisfy.state, "applied");
    }

    #[test]
    fn lowering_ontology_forms_derives_settlement_instrument_and_keeps_history_order() {
        let source = r#"book tax-us
obligation invoice/1
  debtor alice
  creditor bob
  performance transfer 100 USD
settlement payment/1
  kind ach
  from alice
  to bob
  amount 100 USD
  state settled at 2026-01-03
  state issued at 2026-01-01
satisfy allocation/1
  obligation invoice/1
  settlement payment/1
  amount 100 USD
  state applied
"#;
        let ledger = parse_ledger(source).expect("ontology forms lower");
        assert_eq!(ledger.forms.len(), 3);

        let crate::model::LedgerForm::Obligation(obligation) = &ledger.forms[0] else {
            panic!("obligation form")
        };
        assert_eq!(obligation.debtor.as_str(), "alice");
        assert_eq!(obligation.creditor.as_str(), "bob");
        assert_eq!(
            obligation.quantity.unit().map(|unit| unit.as_str()),
            Some("USD")
        );

        let crate::model::LedgerForm::Settlement(settlement) = &ledger.forms[1] else {
            panic!("settlement form")
        };
        assert_eq!(settlement.instrument.as_str(), "USD");
        assert_eq!(
            settlement.amount.unit().map(|unit| unit.as_str()),
            Some("USD")
        );
        assert_eq!(
            settlement
                .history
                .iter()
                .map(|state| state.state)
                .collect::<Vec<_>>(),
            [
                crate::model::SettlementStateKind::Settled,
                crate::model::SettlementStateKind::Issued,
            ]
        );

        let crate::model::LedgerForm::Satisfaction(satisfaction) = &ledger.forms[2] else {
            panic!("satisfaction form")
        };
        assert_eq!(satisfaction.obligation.as_str(), "invoice/1");
        assert_eq!(satisfaction.settlement.as_str(), "payment/1");
    }

    #[test]
    fn explicit_legacy_settlement_instrument_must_match_amount_unit() {
        let source = r#"book tax-us
settlement payment/1
  kind ach
  from alice
  to bob
  instrument EUR
  amount 100 USD
  state issued
        "#;
        let error = parse_source(source).expect_err("mismatched instrument");
        assert_eq!(error.location.line, 6);
        assert!(error.message.contains("must match the amount unit"));
    }

    #[test]
    fn ontology_lowering_reports_unresolved_quantity_holes() {
        let source = r#"book tax-us
obligation invoice/1
  debtor alice
  creditor bob
  performance transfer ?amount USD
"#;
        let error = parse_ledger(source).expect_err("amount hole cannot lower");
        assert!(error.message.contains("amount holes"));
        assert_eq!(error.location.line, 5);
    }

    #[test]
    fn ontology_forms_reject_missing_units_at_the_source_boundary() {
        for source in [
            "book tax-us\nobligation o\n  debtor alice\n  creditor bob\n  performance transfer 100\n",
            "book tax-us\nsettlement s\n  kind ach\n  from alice\n  to bob\n  amount 100\n  state issued\n",
            "book tax-us\nsatisfy a\n  obligation o\n  settlement s\n  amount 100\n  state applied\n",
        ] {
            let error = parse_source(source).expect_err("non-zero quantity needs a unit");
            assert!(error.message.contains("explicit unit"), "{error}");
        }
    }

    #[test]
    fn lowering_does_not_resolve_cross_record_references() {
        let source = r#"book tax-us
satisfy allocation/1
  obligation missing-obligation
  settlement missing-settlement
  amount 1 USD
  state proposed
"#;
        let ledger = parse_ledger(source).expect("references are checked after parsing");
        assert!(matches!(
            ledger.forms.first(),
            Some(crate::model::LedgerForm::Satisfaction(_))
        ));
    }

    #[test]
    fn ontology_forms_reject_duplicates_and_unknown_states() {
        let duplicate = "book tax-us\nobligation o\n  debtor alice\n  debtor bob\n";
        let error = parse_source(duplicate).expect_err("duplicate field");
        assert!(error.message.contains("duplicate `debtor`"));

        let unknown = "book tax-us\nsettlement s\n  kind ach\n  from alice\n  to bob\n  instrument USD\n  amount 1 USD\n  state mystery\n";
        let error = parse_source(unknown).expect_err("unknown state");
        assert!(error.message.contains("unknown settlement state"));
    }
}
