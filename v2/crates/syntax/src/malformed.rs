//! Diagnostics for tokens that are not tokens: impossible dates, glued amounts,
//! currency symbols, unterminated strings, and the like.
//!
//! The lexer only sorts a bad word into a category. Here each category gets the
//! message a person needs: what is wrong, where exactly, and how to write it.

use axiom_core::day::days_in_month;
use axiom_core::{Day, Diagnostic, Loc};

use crate::lex::Malformed;

#[rustfmt::skip]
const MONTHS: [&str; 12] = [
    "January", "February", "March", "April", "May", "June",
    "July", "August", "September", "October", "November", "December",
];

/// The diagnostic for a malformed token whose source text is `text` at `loc`.
pub(crate) fn diagnose(kind: Malformed, loc: Loc, text: &str) -> Diagnostic {
    match kind {
        Malformed::Date => not_a_date(loc, text),
        Malformed::LooseDate => unpadded_date(loc, text),
        Malformed::Number => bad_number(loc, text),
        Malformed::GluedAmount => glued_amount(loc, text),
        Malformed::UnterminatedString => unterminated_string(loc),
        Malformed::Escape(at) => bad_escape(loc, text, at),
        Malformed::Currency => currency_symbol(loc, text),
        Malformed::Code => bad_code(loc),
        Malformed::Word => mixed_case(loc, text),
        Malformed::Character => stray_character(loc, text),
    }
}

/// `2026-02-30`, `2026-13-01`: the right shape, but not on the calendar. The
/// label lands on the offending month or day, not the whole date.
fn not_a_date(loc: Loc, text: &str) -> Diagnostic {
    let field = |from: u32| Loc::new(loc.file, loc.start + from, loc.start + from + 2);
    let number = |from: usize| text.get(from..from + 2).and_then(|digits| digits.parse::<u32>().ok());
    let year: i32 = text[..4].parse().unwrap_or_default();
    let month = number(5).unwrap_or_default();
    let Some(month_name) = month.checked_sub(1).and_then(|index| MONTHS.get(index as usize)) else {
        return Diagnostic::error("bad-date", format!("`{text}` is not a date: there is no month {month}"))
            .label(field(5), "months run from 01 to 12");
    };
    let day = number(8).unwrap_or(1);
    let last = days_in_month(year, month);
    if day == 0 {
        return Diagnostic::error("bad-date", format!("`{text}` is not a date: days start at 01"))
            .label(field(8), "there is no day 00");
    }
    let last_day = format!("{year:04}-{month:02}-{last:02}");
    Diagnostic::error("bad-date", format!("{month_name} {year} has {last} days"))
        .label(field(8), format!("there is no day {day}"))
        .fix(format!("the last day of {month_name} {year} is `{last_day}`"), loc, last_day)
}

/// `2026-1-5`: someone forgot the zero padding.
fn unpadded_date(loc: Loc, text: &str) -> Diagnostic {
    let diag = Diagnostic::error("bad-date", format!("`{text}` is not a date"))
        .label(loc, "dates are written `YYYY-MM-DD`, two digits each for month and day");
    let parts: Vec<String> = text
        .split('-')
        .enumerate()
        .map(|(i, part)| if i == 0 { part.to_string() } else { format!("{part:0>2}") })
        .collect();
    let padded = parts.join("-");
    // A `YYYY-MM` month is checked as the month's first day.
    let on_calendar = match parts.len() {
        2 => Day::parse(format!("{padded}-01").as_bytes()).is_some(),
        _ => Day::parse(padded.as_bytes()).is_some(),
    };
    if on_calendar { diag.fix(format!("write `{padded}`"), loc, padded) } else { diag }
}

fn bad_number(loc: Loc, text: &str) -> Diagnostic {
    let diag = Diagnostic::error("bad-number", format!("`{text}` is not a valid number")).label(loc, "not a number");
    if text.contains('_') {
        diag.note("`_` separates groups of digits and must sit between two digits: `1_000_000`")
    } else {
        diag.note("this number has more digits than an amount can hold")
    }
}

/// `50USD`: the commodity needs a space.
fn glued_amount(loc: Loc, text: &str) -> Diagnostic {
    let diag =
        Diagnostic::error("glued-amount", format!("`{text}` needs a space between the number and its commodity"))
            .label(loc, "number and commodity run together");
    let Some(split) = text.find(|c: char| c.is_ascii_uppercase()) else { return diag };
    let spaced = format!("{} {}", &text[..split], &text[split..]);
    diag.fix(format!("amounts are written `{spaced}`"), loc, spaced)
}

fn unterminated_string(loc: Loc) -> Diagnostic {
    let quote = Loc::new(loc.file, loc.start, loc.start + 1);
    Diagnostic::error("unterminated-string", "this string is never closed")
        .label(quote, "the string starts here")
        .help("a string ends with a closing `\"` on the same line")
}

fn bad_escape(loc: Loc, text: &str, at: u32) -> Diagnostic {
    let offset = (at - loc.start) as usize;
    let escaped = text[offset + 1..].chars().next().map_or(1, char::len_utf8);
    let escape = Loc::new(loc.file, at, at + 1 + escaped as u32);
    Diagnostic::error("bad-escape", format!("unknown escape `{}`", &text[offset..offset + 1 + escaped]))
        .label(escape, "not an escape")
        .note("strings understand `\\\"`, `\\\\`, `\\n` and `\\t`")
}

/// `$50`, `€20`: amounts name their commodity, after the number.
fn currency_symbol(loc: Loc, text: &str) -> Diagnostic {
    let symbol = text.chars().next().unwrap_or('$');
    let digits = &text[symbol.len_utf8()..];
    let code = match symbol {
        '€' => "EUR",
        '£' => "GBP",
        '¥' => "JPY",
        _ => "USD",
    };
    if digits.is_empty() {
        return Diagnostic::error(
            "currency-symbol",
            format!("`{symbol}` is not used; name the commodity after the number"),
        )
        .label(loc, "write the commodity, like `50 USD`");
    }
    Diagnostic::error("currency-symbol", format!("amounts are written `{digits} {code}`, not `{text}`"))
        .label(loc, "a currency symbol")
        .note("the commodity comes after the number and is spelled out, so `$` and `CAD` cannot be confused")
        .fix(format!("write `{digits} {code}`"), loc, format!("{digits} {code}"))
}

fn bad_code(loc: Loc) -> Diagnostic {
    Diagnostic::error("bad-code", "`#` must be followed by a code")
        .label(loc, "a code is lowercase letters, digits, `-`, `_`, `:`, `.` or `/`")
        .help("for example `#check-1041` or `#house`")
}

/// A word that mixes cases, or uses characters names and commodities lack.
fn mixed_case(loc: Loc, text: &str) -> Diagnostic {
    if let Some(code) = text.strip_prefix('#') {
        let lower = format!("#{}", code.to_ascii_lowercase());
        return Diagnostic::error("mixed-case", format!("codes are lowercase, but `{text}` is not"))
            .label(loc, "uppercase in a code")
            .fix(format!("write `{lower}`"), loc, lower);
    }
    let mut diag = Diagnostic::error("mixed-case", format!("`{text}` is neither a name nor a commodity"))
        .label(loc, "names are lowercase and commodities are uppercase");
    let lower = text.to_ascii_lowercase();
    let upper = text.to_ascii_uppercase();
    if lower.bytes().all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-' | b'*' | b'?' | b'/')) {
        diag = diag.fix(format!("as a place or entity, write `{lower}`"), loc, lower);
    }
    if upper.bytes().all(|b| matches!(b, b'A'..=b'Z' | b'0'..=b'9' | b'_' | b'.')) {
        diag = diag.fix(format!("as a commodity, write `{upper}`"), loc, upper);
    }
    diag
}

fn stray_character(loc: Loc, text: &str) -> Diagnostic {
    let diag = Diagnostic::error("unexpected-character", format!("unexpected character '{text}'"))
        .label(loc, "not part of the language");
    match text {
        ";" => diag.help("comments start with `//`"),
        "'" => diag.help("strings are written with double quotes: `\"like this\"`"),
        "%" => diag.help("a percent sign follows its number directly: `10%`"),
        _ => diag,
    }
}
