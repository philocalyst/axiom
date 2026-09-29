//! Tokens: the words, numbers, dates and punctuation of one line.
//!
//! Nothing is copied: names, commodities, codes and strings are slices of the
//! source, and dates and numbers are converted by the SWAR parsers in
//! `axiom-core`. Words are scanned with one table lookup per byte, and a token
//! that is not a token (`2026-02-30`, `$50`) is kept as [`Tok::Invalid`] so the
//! parser can explain it in the context where it turned up.

use axiom_core::{Day, Dec, FileId, Loc, Span};
use memchr::memchr2;

/// What a token means. Punctuation carries nothing; everything else carries
/// what the parser needs, already converted.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Tok<'s> {
    Date(Day),
    /// `YYYY-MM`, as the first day of the month.
    Month(Day),
    /// Digits with optional `_` separators and `.fraction`. A year is a
    /// four-digit number; the parser decides where that matters.
    Number(Dec),
    /// The written number of `10%`, not yet divided by 100.
    Percent(Dec),
    Span(Span),
    /// A lowercase word or `/`-separated path, possibly a glob. Keywords are
    /// names too: only the grammar knows where they count.
    Name(&'s str),
    /// A commodity: `USD`, `BRK.B`.
    Unit(&'s str),
    /// A code without its `#`.
    Code(&'s str),
    /// A string's contents between the quotes, escapes unprocessed.
    Str(&'s str),
    Arrow,
    DotDot,
    Ellipsis,
    Dot,
    Eq,
    EqEq,
    NotEq,
    Lt,
    Le,
    Gt,
    Ge,
    Plus,
    Minus,
    Star,
    Slash,
    At,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
    Colon,
    Bang,
    Question,
    Bar,
    /// The end of the line, or of its code when a comment follows.
    Eol,
    /// Not a token. The parser explains why if it ever reaches one.
    Invalid(Malformed),
}

/// What is wrong with a token that could not be classified. The lexer only
/// records the category; the diagnostic is worded once the parser knows what it
/// was hoping to find.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Malformed {
    /// Shaped like a date or month but not on the calendar: `2026-02-30`.
    Date,
    /// A date without its zero padding: `2026-1-5`.
    LooseDate,
    /// `1__000`, or more digits than a number can hold.
    Number,
    /// A commodity stuck to its number: `50USD`.
    GluedAmount,
    UnterminatedString,
    /// A backslash escape other than `\"`, `\\`, `\n` and `\t`, at this offset.
    Escape(u32),
    /// `$50`: a currency symbol instead of a commodity.
    Currency,
    /// `#` with nothing valid after it.
    Code,
    /// A word that is neither a name (lowercase) nor a commodity (uppercase).
    Word,
    Character,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Token<'s> {
    pub tok: Tok<'s>,
    pub loc: Loc,
}

/// Produces the tokens of one line.
pub(crate) struct Lexer<'s> {
    src: &'s str,
    /// The source cut at the end of the line, so running off it is `None`.
    bytes: &'s [u8],
    file: FileId,
    pos: usize,
    /// Where the line's content begins: a `//` there is a comment.
    body: usize,
    /// The end of the last token: where `Eol` is reported, so a missing token is
    /// pointed at just after the last thing written.
    last_end: usize,
}

impl<'s> Lexer<'s> {
    /// A lexer over `src[body..end]`. Offsets fit `u32`: `parse` refuses larger
    /// files up front.
    pub fn new(src: &'s str, file: FileId, body: usize, end: usize) -> Lexer<'s> {
        Lexer { src, bytes: &src.as_bytes()[..end], file, pos: body, body, last_end: body }
    }

    pub fn next_token(&mut self) -> Token<'s> {
        let after_token = self.pos;
        let blanks = self.bytes[after_token..].iter().take_while(|&&b| b == b' ' || b == b'\t').count();
        let start = after_token + blanks;
        let Some(&first) = self.bytes.get(start) else {
            return self.end_of_line();
        };
        // `//` starts a comment only at the start of the line or after
        // whitespace: a path never contains `//`, and `a//b` is not a comment.
        let starts_comment = first == b'/' && self.bytes[start..].starts_with(b"//");
        if starts_comment && (start == self.body || blanks > 0) {
            return self.end_of_line();
        }
        let tok = self.scan(first, start);
        self.last_end = self.pos;
        Token { tok, loc: self.loc(start, self.pos) }
    }

    fn end_of_line(&mut self) -> Token<'s> {
        self.pos = self.bytes.len();
        Token { tok: Tok::Eol, loc: self.loc(self.last_end, self.last_end) }
    }

    fn loc(&self, start: usize, end: usize) -> Loc {
        Loc::new(self.file, start as u32, end as u32)
    }

    fn scan(&mut self, first: u8, start: usize) -> Tok<'s> {
        match first {
            b'0'..=b'9' => self.digit_word(start),
            b'a'..=b'z' => self.name(start),
            b'A'..=b'Z' => self.unit(start),
            b'#' => self.code(start),
            b'"' => self.string(start),
            // A star touching a word is a glob (`*-trip`); alone it multiplies.
            b'*' if self.bytes.get(start + 1).is_some_and(|&b| CLASS[b as usize] != 0) => self.name(start),
            _ => self.punctuation(start),
        }
    }

    /// The extent of the word starting at `start`, and which kinds of byte it
    /// holds. A word is name bytes, joined into a path by `/` when another
    /// segment follows. A `-` that begins `->` is not part of it, so `a->b` is
    /// three tokens even though `a-b` is one name.
    fn scan_word(&self, start: usize) -> Word {
        let (mut end, mut classes) = (start, 0);
        loop {
            let run = self.bytes[end..]
                .iter()
                .take_while(|&&b| {
                    classes |= CLASS[b as usize];
                    CLASS[b as usize] != 0
                })
                .count();
            end += run;
            if run > 0 && self.bytes[end - 1] == b'-' && self.bytes.get(end) == Some(&b'>') {
                return Word { end: end - 1, classes };
            }
            match (self.bytes.get(end), self.bytes.get(end + 1)) {
                (Some(b'/'), Some(&next)) if CLASS[next as usize] != 0 => {
                    end += 1;
                    classes |= SLASH;
                }
                _ => return Word { end, classes },
            }
        }
    }

    /// Words that start with a digit: date, month, number, percent, span, or
    /// (`401k`) a name, decided by the shape of the whole word.
    fn digit_word(&mut self, start: usize) -> Tok<'s> {
        if let Some(day) = self.date_at(start) {
            return Tok::Date(day);
        }
        let Word { end, classes } = self.scan_word(start);
        if classes & !(DIGIT | UNDERSCORE) == 0 {
            return self.number(start, end, classes);
        }
        self.pos = end;
        classify_digit_word(&self.src[start..end])
    }

    /// A well-formed date, read in one SWAR parse: by far the commonest word
    /// that starts with a digit at the start of a journal line.
    fn date_at(&mut self, start: usize) -> Option<Day> {
        let chunk = self.bytes.get(start..start + 10)?;
        let ends_here = self.bytes.get(start + 10).is_none_or(|&b| CLASS[b as usize] == 0 && b != b'/');
        let day = Day::parse(chunk).filter(|_| ends_here)?;
        self.pos = start + 10;
        Some(day)
    }

    /// A number or percent. A point belongs to the number only when digits
    /// follow it, so `2026..2027` is a range and `84.20` a number.
    fn number(&mut self, start: usize, word_end: usize, classes: u8) -> Tok<'s> {
        let end = self.scan_fraction(word_end);
        if end > word_end && self.bytes.get(end).is_some_and(|&b| CLASS[b as usize] != 0) {
            // `84.20USD`: a fraction leaves no word boundary, unlike `84USD`.
            let rest = self.scan_word(end).end;
            self.pos = rest;
            let glued_unit = self.bytes[end].is_ascii_uppercase();
            return Tok::Invalid(if glued_unit { Malformed::GluedAmount } else { Malformed::Number });
        }
        let text = &self.bytes[start..end];
        let separated = classes & UNDERSCORE == 0 || underscores_between_digits(text);
        let value = if separated { Dec::parse(text) } else { None };
        if self.bytes.get(end) == Some(&b'%') {
            self.pos = end + 1;
            return value.map_or(Tok::Invalid(Malformed::Number), Tok::Percent);
        }
        self.pos = end;
        value.map_or(Tok::Invalid(Malformed::Number), Tok::Number)
    }

    fn scan_fraction(&self, end: usize) -> usize {
        let point = self.bytes.get(end) == Some(&b'.');
        if !point || !self.bytes.get(end + 1).is_some_and(u8::is_ascii_digit) {
            return end;
        }
        end + 1 + self.bytes[end + 1..].iter().take_while(|b| b.is_ascii_digit()).count()
    }

    fn name(&mut self, start: usize) -> Tok<'s> {
        let Word { end, classes } = self.scan_word(start);
        self.pos = end;
        if classes & UPPER != 0 {
            return Tok::Invalid(Malformed::Word);
        }
        Tok::Name(&self.src[start..end])
    }

    fn unit(&mut self, start: usize) -> Tok<'s> {
        let Word { mut end, mut classes } = self.scan_word(start);
        // `BRK.B`: a dot joins the parts of a commodity when a letter or digit
        // follows it.
        while self.bytes.get(end) == Some(&b'.') && self.bytes.get(end + 1).is_some_and(u8::is_ascii_alphanumeric) {
            let part = self.scan_word(end + 1);
            (end, classes) = (part.end, classes | part.classes);
        }
        self.pos = end;
        if classes & (LOWER | SYMBOL | SLASH) != 0 {
            return Tok::Invalid(Malformed::Word);
        }
        Tok::Unit(&self.src[start..end])
    }

    fn code(&mut self, start: usize) -> Tok<'s> {
        let mut end = start + 1;
        while let Some(&b) = self.bytes.get(end) {
            let dot_inside = b == b'.' && self.bytes.get(end + 1).is_some_and(|&n| is_code_byte(n));
            if !is_code_byte(b) && !dot_inside {
                break;
            }
            end += 1;
        }
        self.pos = end;
        let text = &self.src[start + 1..end];
        if text.bytes().any(|b| b.is_ascii_uppercase()) {
            return Tok::Invalid(Malformed::Word);
        }
        match text.bytes().next() {
            Some(b'a'..=b'z' | b'0'..=b'9') => Tok::Code(text),
            _ => Tok::Invalid(Malformed::Code),
        }
    }

    /// A string. Escapes are checked but kept raw; the contents are the slice
    /// between the quotes.
    fn string(&mut self, start: usize) -> Tok<'s> {
        let mut at = start + 1;
        loop {
            let Some(offset) = memchr2(b'"', b'\\', &self.bytes[at..]) else {
                self.pos = self.bytes.len();
                return Tok::Invalid(Malformed::UnterminatedString);
            };
            at += offset;
            if self.bytes[at] == b'"' {
                self.pos = at + 1;
                return Tok::Str(&self.src[start + 1..at]);
            }
            match self.bytes.get(at + 1) {
                Some(b'"' | b'\\' | b'n' | b't') => at += 2,
                Some(_) => {
                    let escaped = self.src[at + 1..].chars().next().map_or(1, char::len_utf8);
                    self.pos = at + 1 + escaped;
                    return Tok::Invalid(Malformed::Escape(at as u32));
                }
                None => {
                    self.pos = self.bytes.len();
                    return Tok::Invalid(Malformed::UnterminatedString);
                }
            }
        }
    }

    fn punctuation(&mut self, start: usize) -> Tok<'s> {
        let (len, tok) = match &self.bytes[start..] {
            [b'-', b'>', ..] => (2, Tok::Arrow),
            [b'.', b'.', b'.', ..] => (3, Tok::Ellipsis),
            [b'.', b'.', ..] => (2, Tok::DotDot),
            [b'=', b'=', ..] => (2, Tok::EqEq),
            [b'!', b'=', ..] => (2, Tok::NotEq),
            [b'<', b'=', ..] => (2, Tok::Le),
            [b'>', b'=', ..] => (2, Tok::Ge),
            [b'.', ..] => (1, Tok::Dot),
            [b'=', ..] => (1, Tok::Eq),
            [b'!', ..] => (1, Tok::Bang),
            [b'<', ..] => (1, Tok::Lt),
            [b'>', ..] => (1, Tok::Gt),
            [b'+', ..] => (1, Tok::Plus),
            [b'-', ..] => (1, Tok::Minus),
            [b'*', ..] => (1, Tok::Star),
            [b'/', ..] => (1, Tok::Slash),
            [b'@', ..] => (1, Tok::At),
            [b'(', ..] => (1, Tok::LParen),
            [b')', ..] => (1, Tok::RParen),
            [b'[', ..] => (1, Tok::LBracket),
            [b']', ..] => (1, Tok::RBracket),
            [b',', ..] => (1, Tok::Comma),
            [b':', ..] => (1, Tok::Colon),
            [b'?', ..] => (1, Tok::Question),
            [b'|', ..] => (1, Tok::Bar),
            _ => return self.stray(start),
        };
        self.pos = start + len;
        tok
    }

    /// A character the language does not use. A currency symbol takes the
    /// digits after it so the diagnostic can show the amount it stood for.
    fn stray(&mut self, start: usize) -> Tok<'s> {
        let ch = self.src[start..].chars().next().expect("a token starts inside the line");
        self.pos = start + ch.len_utf8();
        if !matches!(ch, '$' | '€' | '£' | '¥') {
            return Tok::Invalid(Malformed::Character);
        }
        let digits = self.bytes[self.pos..].iter().take_while(|b| matches!(b, b'0'..=b'9' | b'_')).count();
        self.pos = self.scan_fraction(self.pos + digits);
        Tok::Invalid(Malformed::Currency)
    }
}

// Byte classes for scanning words with one table lookup per byte. A word is
// made of the bytes that have a class: `checking`, `trader-joes`, `check-????`.
// Uppercase is included so `Checking` is one wrong word rather than several
// confusing tokens.
const DIGIT: u8 = 1;
const LOWER: u8 = 2;
const UPPER: u8 = 4;
const UNDERSCORE: u8 = 8;
/// `-`, `*` and `?`: dashes inside names, and the glob characters.
const SYMBOL: u8 = 16;
/// Not a byte class: set in a word's classes when it is a `/`-separated path.
const SLASH: u8 = 32;

static CLASS: [u8; 256] = {
    let mut table = [0; 256];
    let mut byte = 0;
    while byte < 256 {
        table[byte] = match byte as u8 {
            b'0'..=b'9' => DIGIT,
            b'a'..=b'z' => LOWER,
            b'A'..=b'Z' => UPPER,
            b'_' => UNDERSCORE,
            b'-' | b'*' | b'?' => SYMBOL,
            _ => 0,
        };
        byte += 1;
    }
    table
};

/// A scanned word: where it ends, and the union of its bytes' classes.
struct Word {
    end: usize,
    classes: u8,
}

fn is_code_byte(b: u8) -> bool {
    CLASS[b as usize] != 0 || matches!(b, b':' | b'/')
}

/// Classifies a word whose first byte is a digit, by its exact shape. This is
/// the general path; the common shapes are handled before it is reached.
fn classify_digit_word(text: &str) -> Tok<'_> {
    let bytes = text.as_bytes();
    if is_shaped(bytes, b"dddd-dd-dd") {
        return Day::parse(bytes).map_or(Tok::Invalid(Malformed::Date), Tok::Date);
    }
    if is_shaped(bytes, b"dddd-dd") {
        return month_start(bytes).map_or(Tok::Invalid(Malformed::Date), Tok::Month);
    }
    if let Some(span) = Span::parse(bytes) {
        return Tok::Span(span);
    }
    if is_unpadded_date(text) {
        return Tok::Invalid(Malformed::LooseDate);
    }
    if is_glued_amount(bytes) {
        return Tok::Invalid(Malformed::GluedAmount);
    }
    if bytes.iter().any(u8::is_ascii_uppercase) {
        return Tok::Invalid(Malformed::Word);
    }
    Tok::Name(text)
}

/// Whether every `_` in `text` sits between two digits.
fn underscores_between_digits(text: &[u8]) -> bool {
    // A plain scan: numbers are a handful of bytes, too short for `memchr` to pay off.
    let between = |i: usize| i > 0 && text[i - 1].is_ascii_digit() && text.get(i + 1).is_some_and(u8::is_ascii_digit);
    text.iter().enumerate().all(|(i, &b)| b != b'_' || between(i))
}

/// Whether `bytes` fits `pattern`, where `d` stands for a digit and anything
/// else for itself.
fn is_shaped(bytes: &[u8], pattern: &[u8]) -> bool {
    bytes.len() == pattern.len()
        && bytes.iter().zip(pattern).all(|(&b, &p)| if p == b'd' { b.is_ascii_digit() } else { b == p })
}

/// The first day of a `YYYY-MM` month, borrowing the day parser's validation.
fn month_start(month: &[u8]) -> Option<Day> {
    let mut date = *b"0000-00-01";
    date[..7].copy_from_slice(month);
    Day::parse(&date)
}

/// `YYYY-M[-D]` with one-digit parts: a date someone forgot to pad.
fn is_unpadded_date(text: &str) -> bool {
    let mut parts = text.split('-');
    let (Some(year), Some(month)) = (parts.next(), parts.next()) else { return false };
    let day = parts.next();
    let digits = |part: &str, max: usize| (1..=max).contains(&part.len()) && part.bytes().all(|b| b.is_ascii_digit());
    year.len() == 4 && digits(year, 4) && digits(month, 2) && day.is_none_or(|d| digits(d, 2)) && parts.next().is_none()
}

/// Digits then a commodity with no space between: `50USD`.
fn is_glued_amount(bytes: &[u8]) -> bool {
    let digits = bytes.iter().take_while(|b| matches!(b, b'0'..=b'9' | b'_')).count();
    let unit = &bytes[digits..];
    unit.first().is_some_and(u8::is_ascii_uppercase)
        && unit.iter().all(|b| matches!(b, b'A'..=b'Z' | b'0'..=b'9' | b'_'))
}
