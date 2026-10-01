//! Diagnostics for tokens that are not tokens: impossible dates, glued amounts,
//! currency symbols, unterminated strings, and the like.
//!
//! The lexer only sorts a bad word into a category. Here each category gets the
//! message a person needs: what is wrong, where exactly, and how to write it.
//! A word can be as long as its line, so messages show a clip of it, and a
//! word too long to be a slip of the pen gets no edit.

use axiom_core::day::days_in_month;
use axiom_core::{Day, Diagnostic, Loc};

use crate::dates::MONTHS;
use crate::lex::Malformed;

/// The longest word an edit is offered for.
const MENDABLE: usize = 64;

/// The first `max` characters of `text`, then `…` if there were more.
pub(crate) fn clip(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((cut, _)) => format!("{}…", &text[..cut]),
        None => text.to_string(),
    }
}

/// The diagnostic for a malformed token whose source text is `text` at `loc`.
pub(crate) fn diagnose(kind: Malformed, loc: Loc, text: &str) -> Diagnostic {
    match kind {
        Malformed::Date => malformed_date(loc, text),
        Malformed::LooseDate => unpadded_date(loc, text),
        Malformed::SlashDate => slash_date(loc, text),
        Malformed::Number => bad_number(loc, text),
        Malformed::Span => bad_span(loc, text),
        Malformed::GluedAmount => glued_amount(loc, text),
        Malformed::UnterminatedString => unterminated_string(loc),
        Malformed::Escape(at) => bad_escape(loc, text, at),
        Malformed::Currency => currency_symbol(loc, text),
        Malformed::Mark => bad_mark(loc, text),
        Malformed::Word => mixed_case(loc, text),
        Malformed::Character => stray_character(loc, text),
    }
}

/// `2026-02-30`, `2026-13-01`: the right shape, but not on the calendar.
fn malformed_date(loc: Loc, text: &str) -> Diagnostic {
    let number = |from: usize| text.get(from..from + 2).and_then(|digits| digits.parse::<u32>().ok());
    let year = text[..4].parse().unwrap_or_default();
    not_a_date(loc, text, (year, number(5).unwrap_or_default(), number(8).unwrap_or(1)))
}

/// A date that is not on the calendar, written as `text` at `loc` in one of its
/// three shapes (`2026-02-30`, `02-30`, `30`), which `(year, month, day)` says in
/// full: the shorter ones are completed from their file's place. The label
/// lands on the offending month or day, not the whole date.
pub(crate) fn not_a_date(loc: Loc, text: &str, (year, month, day): (i32, u32, u32)) -> Diagnostic {
    // The shorter shapes are the end of the whole one, and so are its fields.
    let left_out = if text.len() > 5 { 0 } else { 10 - text.len() as u32 };
    // A field the date leaves out was given by its place, so it is the whole date that is wrong.
    let field = |from: u32| match from.checked_sub(left_out) {
        Some(offset) => Loc::new(loc.file, loc.start + offset, loc.start + offset + 2),
        None => loc,
    };
    let Some(month_name) = month.checked_sub(1).and_then(|index| MONTHS.get(index as usize)) else {
        return Diagnostic::error("bad-date", format!("`{text}` is not a date: there is no month {month}"))
            .label(field(5), "months run from 01 to 12");
    };
    let last = days_in_month(year, month);
    if day == 0 {
        return Diagnostic::error("bad-date", format!("`{text}` is not a date: days start at 01"))
            .label(field(8), "there is no day 00");
    }
    let whole = format!("{year:04}-{month:02}-{last:02}");
    let last_day = &whole[whole.len().saturating_sub(text.len())..];
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

/// `01/15/2026`, `15/01/2026`, `2026/01/15`. When the year is last, the day
/// and month can be told apart only if one of them cannot be a month; Axiom
/// never guesses between two readings.
fn slash_date(loc: Loc, text: &str) -> Diagnostic {
    let mut parts = text.split('/').map(|part| (part.len(), part.parse::<u32>().unwrap_or_default()));
    let (Some((width, a)), Some((_, b)), Some((_, c))) = (parts.next(), parts.next(), parts.next()) else {
        return bad_number(loc, text);
    };
    // As (year, month, day): year first, or the year last with the month first or second.
    let readings = if width == 4 { [Some((a, b, c)), None] } else { [Some((c, a, b)), Some((c, b, a))] };
    let mut valid = readings.into_iter().flatten().filter(|&(y, m, d)| Day::from_ymd(y as i32, m, d).is_some());
    let first = valid.next();
    let second = valid.next().filter(|&other| Some(other) != first);
    let diag = Diagnostic::error("bad-date", format!("`{text}` is not a date; dates are written `YYYY-MM-DD`"));
    match (first, second) {
        (Some((y, m, d)), None) => {
            let order = match () {
                _ if width == 4 => "year/month/day",
                _ if Some((y, m, d)) == readings[0] => "month/day/year",
                _ => "day/month/year",
            };
            diag.label(loc, format!("read as {order}: {} {d}, {y}", MONTHS[m as usize - 1])).fix(
                "write the date year first",
                loc,
                format!("{y:04}-{m:02}-{d:02}"),
            )
        }
        (Some((y, m, d)), Some(_)) => diag.label(loc, "month first or day first?").note(format!(
            "`{text}` could be {} {d}, {y} or the other way round, so Axiom never guesses",
            MONTHS[m as usize - 1]
        )),
        _ => diag.label(loc, "not a date on the calendar"),
    }
}

fn bad_number(loc: Loc, text: &str) -> Diagnostic {
    let diag = Diagnostic::error("bad-number", format!("`{}` is not a valid number", clip(text, 40)))
        .label(loc, "not a number");
    if text.contains('_') {
        diag.note("`_` separates groups of digits and must sit between two digits: `1_000_000`")
    } else {
        diag.note("this number has more digits than an amount can hold")
    }
}

/// `50USD`: the commodity needs a space.
fn glued_amount(loc: Loc, text: &str) -> Diagnostic {
    let diag = Diagnostic::error(
        "glued-amount",
        format!("`{}` needs a space between the number and its commodity", clip(text, 40)),
    )
    .label(loc, "number and commodity run together");
    let Some(split) = text.find(|c: char| c.is_ascii_uppercase()).filter(|_| text.len() <= MENDABLE) else {
        return diag;
    };
    let spaced = format!("{} {}", &text[..split], &text[split..]);
    diag.fix(format!("amounts are written `{spaced}`"), loc, spaced)
}

fn unterminated_string(loc: Loc) -> Diagnostic {
    let quote = Loc::new(loc.file, loc.start, loc.start + 1);
    let end = Loc::new(loc.file, loc.end, loc.end);
    Diagnostic::error("unterminated-string", "this string is never closed").label(quote, "the string starts here").fix(
        "close it at the end of the line",
        end,
        "\"",
    )
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
    let diag = Diagnostic::error(
        "currency-symbol",
        format!("amounts are written `{} {code}`, not `{}`", clip(digits, 40), clip(text, 40)),
    )
    .label(loc, "a currency symbol")
    .note("the commodity comes after the number and is spelled out, so `$` and `CAD` cannot be confused");
    match text.len() <= MENDABLE {
        true => diag.fix(format!("write `{digits} {code}`"), loc, format!("{digits} {code}")),
        false => diag,
    }
}

fn bad_span(loc: Loc, text: &str) -> Diagnostic {
    Diagnostic::error("bad-span", format!("`{}` is not a whole number of months", clip(text, 40)))
        .label(loc, "a span counts whole months")
        .help("years may have a fraction that comes to whole months: `27.5y` is `27y6m`")
}

/// A `#` or `^` with no name after it.
fn bad_mark(loc: Loc, text: &str) -> Diagnostic {
    let (what, example) = if text.starts_with('#') { ("purpose", "`#groceries`") } else { ("code", "`^check-1041`") };
    Diagnostic::error("bad-mark", format!("`{text}` must be followed by a {what}"))
        .label(loc, format!("a {what} is lowercase letters, digits and `-`, like {example}"))
}

/// A word that mixes cases, or uses characters names and commodities lack.
fn mixed_case(loc: Loc, text: &str) -> Diagnostic {
    let mendable = text.len() <= MENDABLE;
    if let Some(name) = text.strip_prefix(['#', '^']) {
        let what = if text.starts_with('#') { "purpose" } else { "code" };
        let diag = Diagnostic::error("mixed-case", format!("a {what} is lowercase, but `{}` is not", clip(text, 40)))
            .label(loc, format!("uppercase in a {what}"));
        let lower = format!("{}{}", &text[..1], name.to_ascii_lowercase());
        return if mendable { diag.fix(format!("write `{lower}`"), loc, lower) } else { diag };
    }
    let mut diag = Diagnostic::error("mixed-case", format!("`{}` is neither a name nor a commodity", clip(text, 40)))
        .label(loc, "names are lowercase and commodities are uppercase");
    let lower = text.to_ascii_lowercase();
    let upper = text.to_ascii_uppercase();
    if mendable && lower.bytes().all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-' | b'*' | b'?' | b'/')) {
        diag = diag.fix(format!("as a name, write `{lower}`"), loc, lower);
    }
    if mendable && upper.bytes().all(|b| matches!(b, b'A'..=b'Z' | b'0'..=b'9' | b'_' | b'.')) {
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
