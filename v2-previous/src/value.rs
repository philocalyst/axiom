//! Exact, bounded values shared by every package and rule.

use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{One, Signed, Zero};
use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

const MAX_VALUE_BYTES: usize = 64 * 1024;
const MAX_TEXT_BYTES: usize = 16 * 1024;
const MAX_NUMBER_BYTES: usize = 16 * 1024;
const MAX_NUMBER_DIGITS: usize = 4096;
const MAX_UNIT_BYTES: usize = 256;
const MAX_DEPTH: usize = 64;
const MAX_NODES: usize = 10_000;

/// A normalized arbitrary-precision rational number.
///
/// Source spellings belong to the source document.  In a semantic value the
/// numerator and denominator are always reduced and the denominator is
/// positive, so equal amounts always have one codec spelling.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Number(BigRational);

impl Number {
    pub fn zero() -> Self {
        Self(BigRational::zero())
    }

    pub fn one() -> Self {
        Self(BigRational::one())
    }

    pub fn is_zero(&self) -> bool {
        self.0.is_zero()
    }

    pub fn is_negative(&self) -> bool {
        self.0.is_negative()
    }

    fn checked_result(value: BigRational) -> Result<Self, String> {
        let number = Self(value);
        let numerator_digits = number.0.numer().to_string().trim_start_matches('-').len();
        let denominator_digits = number.0.denom().to_string().len();
        if numerator_digits > MAX_NUMBER_DIGITS || denominator_digits > MAX_NUMBER_DIGITS {
            return Err("arithmetic result exceeds the number size limit".into());
        }
        if number.canonical().len() > MAX_NUMBER_BYTES {
            return Err("arithmetic result exceeds the number encoding limit".into());
        }
        Ok(number)
    }

    pub fn add(&self, other: &Self) -> Result<Self, String> {
        Self::checked_result(&self.0 + &other.0)
    }

    pub fn sub(&self, other: &Self) -> Result<Self, String> {
        Self::checked_result(&self.0 - &other.0)
    }

    pub fn mul(&self, other: &Self) -> Result<Self, String> {
        Self::checked_result(&self.0 * &other.0)
    }

    pub fn div(&self, other: &Self) -> Result<Self, String> {
        if other.is_zero() {
            return Err("division by zero".into());
        }
        Self::checked_result(&self.0 / &other.0)
    }

    fn canonical(&self) -> String {
        if self.0.denom().is_one() {
            self.0.numer().to_string()
        } else {
            format!("{}/{}", self.0.numer(), self.0.denom())
        }
    }

    fn readable(&self) -> String {
        self.finite_decimal().unwrap_or_else(|| self.canonical())
    }

    fn finite_decimal(&self) -> Option<String> {
        if self.0.denom().is_one() {
            return Some(self.0.numer().to_string());
        }

        let mut remainder = self.0.denom().clone();
        let mut twos = 0usize;
        let mut fives = 0usize;
        while (&remainder % 2u8).is_zero() {
            remainder /= 2u8;
            twos += 1;
            if twos > MAX_NUMBER_DIGITS {
                return None;
            }
        }
        while (&remainder % 5u8).is_zero() {
            remainder /= 5u8;
            fives += 1;
            if fives > MAX_NUMBER_DIGITS {
                return None;
            }
        }
        if remainder != BigInt::one() {
            return None;
        }

        let scale = twos.max(fives);
        if scale > MAX_NUMBER_DIGITS {
            return None;
        }
        let coefficient = self.0.numer() * ten_pow(scale) / self.0.denom();
        let negative = coefficient.is_negative();
        let digits = coefficient.abs().to_string();
        let (whole, fraction) = if digits.len() <= scale {
            (
                "0".to_owned(),
                format!("{}{}", "0".repeat(scale - digits.len()), digits),
            )
        } else {
            let point = digits.len() - scale;
            (digits[..point].to_owned(), digits[point..].to_owned())
        };
        let fraction = fraction.trim_end_matches('0');
        let sign = if negative { "-" } else { "" };
        if fraction.is_empty() {
            Some(format!("{sign}{whole}"))
        } else {
            Some(format!("{sign}{whole}.{fraction}"))
        }
    }
}

impl FromStr for Number {
    type Err = String;

    fn from_str(source: &str) -> Result<Self, Self::Err> {
        if source.is_empty() || source.len() > MAX_NUMBER_BYTES {
            return Err("number is empty or exceeds the size limit".into());
        }
        if !source.is_ascii() {
            return Err("numbers must contain only ASCII digits and punctuation".into());
        }

        if let Some((numerator, denominator)) = source.split_once('/') {
            if denominator.contains('/') {
                return Err("a rational number may contain only one slash".into());
            }
            let numerator = parse_integer(numerator)?;
            let denominator = parse_integer(denominator)?;
            if denominator.is_zero() {
                return Err("rational denominator cannot be zero".into());
            }
            return Ok(Self(BigRational::new(numerator, denominator)));
        }

        if let Some(dot) = source.find('.') {
            if source[dot + 1..].contains('.') {
                return Err("a decimal number may contain only one point".into());
            }
            let (whole, fractional_with_point) = source.split_at(dot);
            let fractional = &fractional_with_point[1..];
            let (sign, whole_digits) = signed_digits(whole)?;
            if fractional.is_empty() || !fractional.bytes().all(|b| b.is_ascii_digit()) {
                return Err("decimal point must be followed by digits".into());
            }
            check_digit_count(whole_digits.len() + fractional.len())?;
            let scale = ten_pow(fractional.len());
            let whole = if whole_digits.is_empty() {
                BigInt::zero()
            } else {
                BigInt::from_str(whole_digits).map_err(|_| "invalid integer part")?
            };
            let fraction = BigInt::from_str(fractional).map_err(|_| "invalid fractional part")?;
            let coefficient = whole * &scale + fraction;
            let coefficient = if sign < 0 { -coefficient } else { coefficient };
            return Ok(Self(BigRational::new(coefficient, scale)));
        }

        Ok(Self(BigRational::from_integer(parse_integer(source)?)))
    }
}

fn parse_integer(source: &str) -> Result<BigInt, String> {
    let (sign, digits) = signed_digits(source)?;
    check_digit_count(digits.len())?;
    let value = BigInt::from_str(digits).map_err(|_| "expected an integer".to_owned())?;
    Ok(if sign < 0 { -value } else { value })
}

fn signed_digits(source: &str) -> Result<(i8, &str), String> {
    let (sign, digits) = match source.as_bytes().first() {
        Some(b'-') => (-1, &source[1..]),
        Some(b'+') => (1, &source[1..]),
        _ => (1, source),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err("expected ASCII digits".into());
    }
    Ok((sign, digits))
}

fn check_digit_count(count: usize) -> Result<(), String> {
    if count > MAX_NUMBER_DIGITS {
        Err("number exceeds the digit limit".into())
    } else {
        Ok(())
    }
}

fn ten_pow(power: usize) -> BigInt {
    BigInt::from(10u8).pow(power as u32)
}

impl fmt::Display for Number {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.readable())
    }
}

impl Serialize for Number {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.canonical())
    }
}

impl<'de> Deserialize<'de> for Number {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct NumberVisitor;
        impl<'de> Visitor<'de> for NumberVisitor {
            type Value = Number;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a canonical exact rational string")
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                let number = Number::from_str(value).map_err(E::custom)?;
                if number.canonical() != value {
                    return Err(E::custom("number string is not in canonical form"));
                }
                Ok(number)
            }

            fn visit_string<E: de::Error>(self, value: String) -> Result<Self::Value, E> {
                self.visit_str(&value)
            }
        }
        deserializer.deserialize_str(NumberVisitor)
    }
}

/// A validated Gregorian calendar date in `YYYY-MM-DD` form.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Date(String);

impl FromStr for Date {
    type Err = String;

    fn from_str(source: &str) -> Result<Self, Self::Err> {
        let bytes = source.as_bytes();
        if bytes.len() != 10
            || bytes[4] != b'-'
            || bytes[7] != b'-'
            || !bytes[..4].iter().all(u8::is_ascii_digit)
            || !bytes[5..7].iter().all(u8::is_ascii_digit)
            || !bytes[8..].iter().all(u8::is_ascii_digit)
        {
            return Err("date must use ISO YYYY-MM-DD form".into());
        }
        let year = parse_small_number(&source[..4]);
        let month = parse_small_number(&source[5..7]);
        let day = parse_small_number(&source[8..10]);
        if year == 0 || !(1..=12).contains(&month) {
            return Err("date year or month is out of range".into());
        }
        let leap =
            year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
        let days = match month {
            2 if leap => 29,
            2 => 28,
            4 | 6 | 9 | 11 => 30,
            _ => 31,
        };
        if !(1..=days).contains(&day) {
            return Err("day is out of range for this month".into());
        }
        Ok(Self(source.to_owned()))
    }
}

fn parse_small_number(source: &str) -> u32 {
    source.parse().unwrap_or(0)
}

impl fmt::Display for Date {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Serialize for Date {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Date {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct DateVisitor;
        impl<'de> Visitor<'de> for DateVisitor {
            type Value = Date;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a valid Gregorian date in YYYY-MM-DD form")
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                Date::from_str(value).map_err(E::custom)
            }

            fn visit_string<E: de::Error>(self, value: String) -> Result<Self::Value, E> {
                self.visit_str(&value)
            }
        }
        deserializer.deserialize_str(DateVisitor)
    }
}

/// The single semantic value representation used by all packages.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Value {
    Number(Number),
    Quantity(Number, String),
    Date(Date),
    Text(String),
    Bool(bool),
    Ref(String),
    Hole(String),
    List(Vec<Value>),
    Record(BTreeMap<String, Value>),
}

impl Value {
    /// Charge a logical retention budget before keeping another elaborated value.
    /// This bounds default expansion, not allocator overhead or process RSS.
    pub(crate) fn charge_size(&self, remaining: &mut usize) -> bool {
        fn charge(remaining: &mut usize, bytes: usize) -> bool {
            match remaining.checked_sub(bytes) {
                Some(next) => {
                    *remaining = next;
                    true
                }
                None => false,
            }
        }
        fn number_bytes(number: &Number) -> usize {
            (number.0.numer().bits().div_ceil(8) + number.0.denom().bits().div_ceil(8)) as usize
        }
        if !charge(remaining, std::mem::size_of::<Self>()) {
            return false;
        }
        match self {
            Self::Number(number) => charge(remaining, number_bytes(number)),
            Self::Quantity(number, unit) => charge(remaining, number_bytes(number) + unit.len()),
            Self::Text(text) | Self::Ref(text) | Self::Hole(text) => charge(remaining, text.len()),
            Self::List(values) => values.iter().all(|value| value.charge_size(remaining)),
            Self::Record(fields) => fields.iter().all(|(name, value)| {
                charge(remaining, std::mem::size_of::<String>() + name.len())
                    && value.charge_size(remaining)
            }),
            Self::Date(_) | Self::Bool(_) => true,
        }
    }

    pub fn parse(source: &str) -> Result<Self, String> {
        if source.len() > MAX_VALUE_BYTES {
            return Err("value exceeds the size limit".into());
        }
        let mut parser = ValueParser {
            source,
            offset: 0,
            nodes: 0,
        };
        parser.skip_space();
        let value = parser.value(0)?;
        parser.skip_space();
        if parser.offset != source.len() {
            return Err(parser.error("unexpected text after value"));
        }
        Ok(value)
    }

    /// Validate intrinsic value limits, including values assembled directly
    /// through the public enum variants rather than parsed from source.
    pub fn validate(&self) -> Result<(), String> {
        let mut pending = vec![(self, 0usize)];
        let mut nodes = 0usize;
        while let Some((value, depth)) = pending.pop() {
            if depth >= MAX_DEPTH {
                return Err("value nesting exceeds the depth limit".into());
            }
            nodes += 1;
            if nodes > MAX_NODES {
                return Err("value contains too many elements".into());
            }
            match value {
                Self::Quantity(_, unit) if !valid_unit(unit) => {
                    return Err("invalid quantity unit".into());
                }
                Self::Text(text) if text.len() > MAX_TEXT_BYTES => {
                    return Err("text exceeds the size limit".into());
                }
                Self::Ref(reference) if !valid_ref(reference) => {
                    return Err("invalid reference".into());
                }
                Self::Hole(name) if !valid_hole(name) => {
                    return Err("invalid hole".into());
                }
                Self::List(values) => {
                    pending.extend(values.iter().map(|value| (value, depth + 1)));
                }
                Self::Record(fields) => {
                    for (name, value) in fields {
                        if name.is_empty() || name.len() > MAX_TEXT_BYTES {
                            return Err("record field name is empty or too long".into());
                        }
                        pending.push((value, depth + 1));
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
}

impl fmt::Display for Value {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Number(number) => number.fmt(formatter),
            Self::Quantity(number, unit) => write!(formatter, "{number} {unit}"),
            Self::Date(date) => date.fmt(formatter),
            Self::Text(text) if is_bare_text(text) => formatter.write_str(text),
            Self::Text(text) => write!(formatter, "{}", json_string(text)),
            Self::Bool(value) => value.fmt(formatter),
            Self::Ref(reference) => write!(formatter, "@{reference}"),
            Self::Hole(name) if name.is_empty() => formatter.write_str("?"),
            Self::Hole(name) => write!(formatter, "?{name}"),
            Self::List(values) => {
                formatter.write_str("[")?;
                for (index, value) in values.iter().enumerate() {
                    if index != 0 {
                        formatter.write_str(", ")?;
                    }
                    value.fmt(formatter)?;
                }
                formatter.write_str("]")
            }
            Self::Record(fields) => {
                formatter.write_str("{")?;
                for (index, (name, value)) in fields.iter().enumerate() {
                    if index != 0 {
                        formatter.write_str(", ")?;
                    }
                    if is_field_name(name) {
                        formatter.write_str(name)?;
                    } else {
                        formatter.write_str(&json_string(name))?;
                    }
                    write!(formatter, ": {value}")?;
                }
                formatter.write_str("}")
            }
        }
    }
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".into())
}

fn is_field_name(value: &str) -> bool {
    let mut chars = value.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn is_bare_text(value: &str) -> bool {
    if value.is_empty()
        || value.len() > MAX_TEXT_BYTES
        || value.chars().any(char::is_whitespace)
        || value
            .chars()
            .any(|c| matches!(c, '[' | ']' | '{' | '}' | ',' | '"' | '#'))
        || value.starts_with(['@', '?'])
        || value == "true"
        || value == "false"
        || starts_numberish(value)
        || is_date_shape(value)
    {
        return false;
    }
    true
}

fn starts_numberish(value: &str) -> bool {
    matches!(value.as_bytes().first(), Some(b'0'..=b'9'))
        || (value.starts_with('-') || value.starts_with('+'))
            && value.as_bytes().get(1).is_some_and(u8::is_ascii_digit)
}

fn is_date_shape(value: &str) -> bool {
    value.len() == 10
        && value.as_bytes().get(4) == Some(&b'-')
        && value.as_bytes().get(7) == Some(&b'-')
        && value
            .bytes()
            .enumerate()
            .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit())
}

pub(crate) fn valid_unit(unit: &str) -> bool {
    !unit.is_empty()
        && unit.len() <= MAX_UNIT_BYTES
        && unit.chars().all(|c| {
            !c.is_whitespace()
                && !c.is_control()
                && !matches!(c, ',' | ']' | '}' | '{' | '[' | '"' | '#')
        })
        && !unit.starts_with(['@', '?'])
}

fn valid_ref(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_TEXT_BYTES
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'/' | b'.' | b':'))
}

fn valid_hole(value: &str) -> bool {
    value.is_empty() || (value.len() <= MAX_TEXT_BYTES && is_field_name(value))
}

struct ValueParser<'a> {
    source: &'a str,
    offset: usize,
    nodes: usize,
}

impl ValueParser<'_> {
    fn value(&mut self, depth: usize) -> Result<Value, String> {
        if depth >= MAX_DEPTH {
            return Err(self.error("value nesting exceeds the depth limit"));
        }
        self.nodes += 1;
        if self.nodes > MAX_NODES {
            return Err(self.error("value contains too many elements"));
        }
        self.skip_space();
        match self.peek() {
            Some('[') => self.list(depth),
            Some('{') => self.record(depth),
            Some('"') => self.quoted_text(),
            Some(_) => self.atom(),
            None => Err(self.error("expected a value")),
        }
    }

    fn list(&mut self, depth: usize) -> Result<Value, String> {
        self.offset += 1;
        self.skip_space();
        let mut values = Vec::new();
        if self.consume(']') {
            return Ok(Value::List(values));
        }
        loop {
            values.push(self.value(depth + 1)?);
            self.skip_space();
            if self.consume(']') {
                break;
            }
            if !self.consume(',') {
                return Err(self.error("expected ',' or ']' in list"));
            }
            self.skip_space();
            if self.peek() == Some(']') {
                return Err(self.error("trailing commas are not allowed in lists"));
            }
        }
        Ok(Value::List(values))
    }

    fn record(&mut self, depth: usize) -> Result<Value, String> {
        self.offset += 1;
        self.skip_space();
        let mut fields = BTreeMap::new();
        if self.consume('}') {
            return Ok(Value::Record(fields));
        }
        loop {
            let key = self.record_key()?;
            self.skip_space();
            if !self.consume(':') {
                return Err(self.error("expected ':' after record field name"));
            }
            let value = self.value(depth + 1)?;
            if fields.insert(key.clone(), value).is_some() {
                return Err(self.error(&format!("duplicate record field '{key}'")));
            }
            self.skip_space();
            if self.consume('}') {
                break;
            }
            if !self.consume(',') {
                return Err(self.error("expected ',' or '}' in record"));
            }
            self.skip_space();
            if self.peek() == Some('}') {
                return Err(self.error("trailing commas are not allowed in records"));
            }
        }
        Ok(Value::Record(fields))
    }

    fn record_key(&mut self) -> Result<String, String> {
        self.skip_space();
        if self.peek() == Some('"') {
            return self.quoted_string();
        }
        let start = self.offset;
        while let Some(c) = self.peek() {
            if c.is_whitespace() || c == ':' || c == ',' || c == '}' {
                break;
            }
            self.offset += c.len_utf8();
        }
        let key = &self.source[start..self.offset];
        if !is_field_name(key) {
            return Err(self.error("record field names must be identifiers or quoted strings"));
        }
        Ok(key.to_owned())
    }

    fn quoted_text(&mut self) -> Result<Value, String> {
        self.quoted_string().map(Value::Text)
    }

    fn quoted_string(&mut self) -> Result<String, String> {
        let start = self.offset;
        self.offset += 1;
        let mut escaped = false;
        while let Some(c) = self.peek() {
            self.offset += c.len_utf8();
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                let spelling = &self.source[start..self.offset];
                let value: String = serde_json::from_str(spelling)
                    .map_err(|_| self.error("invalid quoted text"))?;
                if value.len() > MAX_TEXT_BYTES {
                    return Err(self.error("text exceeds the size limit"));
                }
                return Ok(value);
            }
        }
        Err(self.error("unterminated quoted text"))
    }

    fn atom(&mut self) -> Result<Value, String> {
        let start = self.offset;
        while let Some(c) = self.peek() {
            if c.is_whitespace() || matches!(c, ',' | ']' | '}') {
                break;
            }
            self.offset += c.len_utf8();
        }
        if self.offset == start {
            return Err(self.error("expected a value"));
        }
        let token = &self.source[start..self.offset];
        if token.len() > MAX_TEXT_BYTES {
            return Err(self.error("atom exceeds the size limit"));
        }
        if token == "true" {
            return Ok(Value::Bool(true));
        }
        if token == "false" {
            return Ok(Value::Bool(false));
        }
        if let Some(reference) = token.strip_prefix('@') {
            return if valid_ref(reference) {
                Ok(Value::Ref(reference.to_owned()))
            } else {
                Err(self.error("malformed reference literal"))
            };
        }
        if let Some(name) = token.strip_prefix('?') {
            return if valid_hole(name) {
                Ok(Value::Hole(name.to_owned()))
            } else {
                Err(self.error("malformed hole literal"))
            };
        }
        if is_date_shape(token) {
            return Date::from_str(token)
                .map(Value::Date)
                .map_err(|error| self.error(&error));
        }
        if starts_numberish(token) {
            let number = Number::from_str(token).map_err(|error| self.error(&error))?;
            let before_space = self.offset;
            self.skip_space();
            if self.offset == before_space {
                return Ok(Value::Number(number));
            }
            if matches!(self.peek(), None | Some(',' | ']' | '}')) {
                return Ok(Value::Number(number));
            }
            let unit_start = self.offset;
            while let Some(c) = self.peek() {
                if c.is_whitespace() || matches!(c, ',' | ']' | '}') {
                    break;
                }
                self.offset += c.len_utf8();
            }
            let unit = &self.source[unit_start..self.offset];
            if valid_unit(unit) {
                return Ok(Value::Quantity(number, unit.to_owned()));
            }
            return Err(self.error("malformed quantity unit"));
        }
        Ok(Value::Text(token.to_owned()))
    }

    fn skip_space(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            let len = self.peek().map(char::len_utf8).unwrap_or(0);
            self.offset += len;
        }
    }

    fn peek(&self) -> Option<char> {
        self.source[self.offset..].chars().next()
    }

    fn consume(&mut self, expected: char) -> bool {
        if self.peek() == Some(expected) {
            self.offset += expected.len_utf8();
            true
        } else {
            false
        }
    }

    fn error(&self, message: &str) -> String {
        format!("{message} at byte {}", self.offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retention_budget_bounds_repeated_default_expansion() {
        let default = Value::Text("x".repeat(4096));
        let mut remaining = std::mem::size_of::<Value>() + 4096;
        assert!(default.charge_size(&mut remaining));
        assert_eq!(remaining, 0);
        assert!(!default.charge_size(&mut remaining));
        let mut tiny = 1;
        assert!(!Value::List(vec![default]).charge_size(&mut tiny));
    }

    #[test]
    fn number_is_normalized_and_arithmetic_stays_exact() {
        let half = Number::from_str("2.50").unwrap();
        assert_eq!(half.to_string(), "2.5");
        assert_eq!(half.canonical(), "5/2");
        let three_quarters = Number::from_str("6/8").unwrap();
        assert_eq!(three_quarters.to_string(), "0.75");
        assert_eq!(three_quarters.canonical(), "3/4");
        let result = Number::from_str("1/3")
            .unwrap()
            .add(&Number::from_str("1/6").unwrap())
            .unwrap();
        assert_eq!(result.to_string(), "0.5");
        assert_eq!(result.canonical(), "1/2");
        assert_eq!(Number::from_str("1/3").unwrap().to_string(), "1/3");
        let beyond_decimal_cap = Number(BigRational::new(
            BigInt::one(),
            BigInt::from(2u8).pow((MAX_NUMBER_DIGITS + 1) as u32),
        ));
        assert!(beyond_decimal_cap.to_string().starts_with("1/"));
        assert!(Number::one().div(&Number::zero()).is_err());
    }

    #[test]
    fn dates_validate_the_gregorian_calendar() {
        assert!(Date::from_str("2000-02-29").is_ok());
        assert!(Date::from_str("1900-02-29").is_err());
        assert!(Date::from_str("0000-01-01").is_err());
    }

    #[test]
    fn values_round_trip_and_reject_malformed_reserved_syntax() {
        let value = Value::parse(r#"{amount: 12.50 USD, when: 2024-02-29, note: "due # now", items: [@sale/one, ?lot, false]}"#).unwrap();
        assert_eq!(Value::parse(&value.to_string()).unwrap(), value);
        assert!(Value::parse("@ ").is_err());
        assert!(Value::parse("?bad name").is_err());
        assert!(Value::parse("1 @reference").is_err());
        assert!(Value::parse("2023-02-29").is_err());
        assert!(Value::parse("[1,]").is_err());
        assert!(Value::parse("{a: 1, a: 2}").is_err());
    }

    #[test]
    fn directly_constructed_values_still_obey_intrinsic_limits() {
        assert!(
            Value::Quantity(Number::one(), "bad unit".into())
                .validate()
                .is_err()
        );
        assert!(
            Value::Text("x".repeat(MAX_TEXT_BYTES + 1))
                .validate()
                .is_err()
        );
    }

    #[test]
    fn serde_requires_canonical_numbers_and_bounded_values() {
        assert!(serde_json::from_str::<Number>(r#""2/2""#).is_err());
        let encoded =
            serde_json::to_string(&Value::Number(Number::from_str("1.00").unwrap())).unwrap();
        assert_eq!(encoded, r#"{"Number":"1"}"#);
        assert_eq!(
            serde_json::to_string(&Number::from_str("4.50").unwrap()).unwrap(),
            r#""9/2""#
        );
        assert!(serde_json::from_str::<Date>(r#""2025-02-29""#).is_err());
        assert!(serde_json::from_str::<Value>(r#"{"Unknown":"value"}"#).is_err());
        let malformed: Value = serde_json::from_str(r#"{"Quantity":["1","bad unit"]}"#).unwrap();
        assert!(malformed.validate().is_err());
        let nested = format!("{}0{}", "[".repeat(MAX_DEPTH), "]".repeat(MAX_DEPTH));
        assert!(Value::parse(&nested).is_err());
    }
}
