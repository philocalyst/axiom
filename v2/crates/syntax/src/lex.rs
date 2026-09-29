//! Tokens: the words, numbers, dates and punctuation of one line.
//!
//! Nothing is copied: names, commodities, purposes, codes and strings are slices
//! of the source, and dates and numbers are converted by the parsers in `axiom-core`.
//! The three commonest words (a path, a plain number, a commodity) have a
//! fast path that reads them in one pass; everything odd about a word is left
//! to the general path, which classifies it byte by byte. A token that is not a
//! token (`2026-02-30`, `$50`) is kept as [`Tok::Invalid`] so the parser can
//! explain it in the context where it turned up. The lexer can look two tokens
//! ahead, which is enough to tell `? USD` (an unknown amount) from `?` (the
//! unknown party), and a year from the number after it.

use axiom_core::{Day, Dec, FileId, Loc, Span};
use memchr::memchr2;

use crate::ast::{BinOp, Code, Name};

/// What a token means: what the parser needs, already converted.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Tok<'s> {
    Date(Day),
    /// `YYYY-MM`, as the first day of the month.
    Month(Day),
    /// `MM-DD` as written, unchecked: the parser says whether it is a date (in
    /// a file whose place gives the year) or a day of every year, and whether
    /// it exists.
    MonthDay(u8, u8),
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
    /// `#groceries`, what a flow is for: the name, without its `#`.
    Purpose(Name<'s>),
    /// `^inv-12`, what marks flows that belong together: `^` included.
    Code(Code<'s>),
    /// A string's contents between the quotes, escapes unprocessed.
    Str(&'s str),
    /// Punctuation. What is meant for the arrow, `=>` and `→`, is read as one,
    /// and the parser can see how it was written from the token's text.
    Punct(Punct),
    /// The end of the line, or of its code when a comment follows.
    Eol,
    /// Not a token. The parser explains why if it ever reaches one.
    Invalid(Malformed),
}

/// The punctuation of the language, lexed by looking at the bytes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Punct {
    Arrow,
    Ellipsis,
    DotDot,
    EqEq,
    Ne,
    Le,
    Ge,
    Slash,
    Comma,
    Dot,
    Eq,
    Bang,
    Lt,
    Gt,
    Plus,
    Minus,
    Star,
    At,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Colon,
    Question,
    Pipe,
}

impl Punct {
    /// The punctuation at the start of `rest`, and how many bytes it takes: the
    /// longest that fits. What is meant for `->` (`=>` and `→`) is read as it.
    pub fn lex(rest: &[u8]) -> Option<(Punct, usize)> {
        Some(match rest {
            [b'-', b'>', ..] | [b'=', b'>', ..] => (Punct::Arrow, 2),
            [0xE2, 0x86, 0x92, ..] => (Punct::Arrow, 3),
            [b'.', b'.', b'.', ..] => (Punct::Ellipsis, 3),
            [b'.', b'.', ..] => (Punct::DotDot, 2),
            [b'=', b'=', ..] => (Punct::EqEq, 2),
            [b'!', b'=', ..] => (Punct::Ne, 2),
            [b'<', b'=', ..] => (Punct::Le, 2),
            [b'>', b'=', ..] => (Punct::Ge, 2),
            [byte, ..] => (
                match byte {
                    b'/' => Punct::Slash,
                    b',' => Punct::Comma,
                    b'.' => Punct::Dot,
                    b'=' => Punct::Eq,
                    b'!' => Punct::Bang,
                    b'<' => Punct::Lt,
                    b'>' => Punct::Gt,
                    b'+' => Punct::Plus,
                    b'-' => Punct::Minus,
                    b'*' => Punct::Star,
                    b'@' => Punct::At,
                    b'(' => Punct::LParen,
                    b')' => Punct::RParen,
                    b'[' => Punct::LBracket,
                    b']' => Punct::RBracket,
                    b':' => Punct::Colon,
                    b'?' => Punct::Question,
                    b'|' => Punct::Pipe,
                    _ => return None,
                },
                1,
            ),
            [] => return None,
        })
    }

    /// As spelled in a message: `->` for the arrow, however it was written.
    pub fn spelling(self) -> &'static str {
        match self {
            Punct::Arrow => "->",
            Punct::Ellipsis => "...",
            Punct::DotDot => "..",
            Punct::EqEq => "==",
            Punct::Ne => "!=",
            Punct::Le => "<=",
            Punct::Ge => ">=",
            Punct::Slash => "/",
            Punct::Comma => ",",
            Punct::Dot => ".",
            Punct::Eq => "=",
            Punct::Bang => "!",
            Punct::Lt => "<",
            Punct::Gt => ">",
            Punct::Plus => "+",
            Punct::Minus => "-",
            Punct::Star => "*",
            Punct::At => "@",
            Punct::LParen => "(",
            Punct::RParen => ")",
            Punct::LBracket => "[",
            Punct::RBracket => "]",
            Punct::Colon => ":",
            Punct::Question => "?",
            Punct::Pipe => "|",
        }
    }

    /// The infix operator this spells, if it is one. (`or`, `and` and `is` are
    /// words.)
    pub fn infix(self) -> Option<BinOp> {
        Some(match self {
            Punct::EqEq => BinOp::Eq,
            Punct::Ne => BinOp::Ne,
            Punct::Lt => BinOp::Lt,
            Punct::Le => BinOp::Le,
            Punct::Gt => BinOp::Gt,
            Punct::Ge => BinOp::Ge,
            Punct::Plus => BinOp::Add,
            Punct::Minus => BinOp::Sub,
            Punct::Star => BinOp::Mul,
            Punct::Slash => BinOp::Div,
            _ => return None,
        })
    }
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
    /// A date written with slashes: `01/15/2026`, `15/01/2026`, `2026/01/15`.
    SlashDate,
    /// `1__000`, or more digits than a number can hold.
    Number,
    /// Years with a fraction that is not a whole number of months: `27.33y`.
    Span,
    /// A commodity stuck to its number: `50USD`.
    GluedAmount,
    UnterminatedString,
    /// A backslash escape other than `\"`, `\\`, `\n` and `\t`, at this offset.
    Escape(u32),
    /// `$50`: a currency symbol instead of a commodity.
    Currency,
    /// `#` or `^` with nothing valid after it.
    Mark,
    /// A word that is neither a name (lowercase) nor a commodity (uppercase).
    Word,
    Character,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Token<'s> {
    pub tok: Tok<'s>,
    pub loc: Loc,
}

/// The tokens of one line, read as they are asked for. Cloning one is how a
/// rule looks further ahead than the two tokens it can peek at.
#[derive(Clone)]
pub(crate) struct Lexer<'s> {
    src: &'s str,
    /// The source cut at the end of the line, so running off it is `None`.
    bytes: &'s [u8],
    file: FileId,
    pos: usize,
    /// Where the line's content begins: a `//` there is a comment.
    body: usize,
    /// The end of the last token scanned: where `Eol` is reported, so a missing
    /// token is pointed at just after the last thing written.
    last_end: usize,
    /// The next token, and the one after it once something has asked for it.
    /// Once the line ends, every token is `Eol`.
    next: Token<'s>,
    after: Option<Token<'s>>,
    /// The end of the last token consumed.
    prev_end: u32,
}

impl<'s> Lexer<'s> {
    /// A lexer over an empty line. Offsets fit `u32`: `parse` refuses larger
    /// files up front.
    pub fn new(src: &'s str, file: FileId) -> Lexer<'s> {
        let eol = Token { tok: Tok::Eol, loc: Loc::default() };
        Lexer { src, bytes: &[], file, pos: 0, body: 0, last_end: 0, next: eol, after: None, prev_end: 0 }
    }

    /// Starts over on the line `src[body..end]`.
    pub fn load(&mut self, body: usize, end: usize) {
        self.bytes = &self.src.as_bytes()[..end];
        (self.pos, self.body, self.last_end, self.prev_end) = (body, body, body, body as u32);
        self.restart();
    }

    /// Scans the first token from `pos`.
    fn restart(&mut self) {
        (self.next, self.after) = (self.scan_token(), None);
    }

    pub fn peek(&self) -> Token<'s> {
        self.next
    }

    pub fn peek_second(&mut self) -> Token<'s> {
        if let Some(token) = self.after {
            return token;
        }
        if matches!(self.next.tok, Tok::Eol) {
            return self.next;
        }
        let token = self.scan_token();
        *self.after.insert(token)
    }

    /// Consumes the next token. The line's end is never consumed: asking for
    /// more keeps returning it.
    pub fn bump(&mut self) -> Token<'s> {
        let token = self.next;
        if !matches!(token.tok, Tok::Eol) {
            self.next = self.after.take().unwrap_or_else(|| self.scan_token());
            self.prev_end = token.loc.end;
        }
        token
    }

    /// Where the last consumed token ended.
    pub fn prev_end(&self) -> u32 {
        self.prev_end
    }

    /// The rest of the line, raw, comments included: what a command is.
    pub fn raw_rest(&mut self) -> Option<Name<'s>> {
        let start = self.raw_start()?;
        Some(self.resume(start, start + self.src[start..self.bytes.len()].trim_end().len()))
    }

    /// Where raw text starts: the next token, unless the line is over.
    fn raw_start(&self) -> Option<usize> {
        (!matches!(self.peek().tok, Tok::Eol)).then_some(self.peek().loc.start as usize)
    }

    /// Takes `src[start..end]` as raw text and lexes on from its end.
    fn resume(&mut self, start: usize, end: usize) -> Name<'s> {
        (self.pos, self.body, self.last_end, self.prev_end) = (end, end, end, end as u32);
        self.restart();
        Name(&self.src[start..end])
    }

    fn scan_token(&mut self) -> Token<'s> {
        let bytes = self.bytes;
        let mut start = self.pos;
        while let Some(b' ' | b'\t') = bytes.get(start) {
            start += 1;
        }
        // `//` starts a comment only at the start of the line or after
        // whitespace: a path never contains `//`, and `a//b` is not a comment.
        let comment = |first: u8| {
            first == b'/' && bytes[start..].starts_with(b"//") && (start == self.body || start > self.pos)
        };
        let Some(&first) = bytes.get(start).filter(|&&first| !comment(first)) else {
            self.pos = bytes.len();
            return Token { tok: Tok::Eol, loc: self.loc(self.last_end, self.last_end) };
        };
        let tok = match first {
            b'0'..=b'9' => self.digit_word(start),
            b'a'..=b'z' => self.name(start),
            b'A'..=b'Z' => self.unit(start),
            b'#' | b'^' => self.mark(start),
            b'"' => self.string(start),
            // A star touching a word is a glob (`*-trip`); alone it multiplies.
            b'*' if bytes.get(start + 1).is_some_and(|&b| CLASS[b as usize] != 0) => self.odd_name(start),
            _ => self.punctuation(start),
        };
        self.last_end = self.pos;
        Token { tok, loc: self.loc(start, self.pos) }
    }

    fn loc(&self, start: usize, end: usize) -> Loc {
        Loc::new(self.file, start as u32, end as u32)
    }

    /// A lowercase name or path, the commonest token by far: segments of name
    /// bytes joined by `/` when another segment follows. A `-` that begins `->`
    /// is not part of it, so `a->b` is three tokens even though `a-b` is one
    /// name. Anything odd (an uppercase letter or `?` in the word) is left to
    /// [`Lexer::odd_name`].
    fn name(&mut self, start: usize) -> Tok<'s> {
        let bytes = self.bytes;
        let mut end = start;
        loop {
            while bytes.get(end).is_some_and(|&b| PLAIN[b as usize]) {
                end += 1;
            }
            match (bytes.get(end), bytes.get(end + 1)) {
                (Some(b'/'), Some(&next)) if CLASS[next as usize] != 0 => end += 1,
                (Some(&b), _) if CLASS[b as usize] != 0 => return self.odd_name(start),
                _ => break,
            }
        }
        if bytes[end - 1] == b'-' && bytes.get(end) == Some(&b'>') {
            end -= 1;
        }
        self.pos = end;
        Tok::Name(&self.src[start..end])
    }

    /// The extent of the word starting at `start`, and which kinds of byte it
    /// holds, one byte at a time: the general path for words the fast one
    /// declines.
    fn scan_word(&self, start: usize) -> Word {
        let (mut end, mut classes) = (start, 0);
        loop {
            let run = self.bytes[end..].iter().take_while(|&&b| CLASS[b as usize] != 0).count();
            classes = self.bytes[end..end + run].iter().fold(classes, |classes, &b| classes | CLASS[b as usize]);
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

    /// A lowercase-initial or glob word with an uppercase letter or `?` in it.
    fn odd_name(&mut self, start: usize) -> Tok<'s> {
        let Word { end, classes } = self.scan_word(start);
        self.pos = end;
        match classes & UPPER {
            0 => Tok::Name(&self.src[start..end]),
            _ => Tok::Invalid(Malformed::Word),
        }
    }

    /// Words that start with a digit: date, month, number, percent, span, or
    /// (`401k`) a name, decided by the shape of the whole word.
    fn digit_word(&mut self, start: usize) -> Tok<'s> {
        // Dates, months and their misspellings have a `-` after the year, which
        // no plain number has.
        if self.bytes.get(start + 4) != Some(&b'-')
            && let Some(number) = self.plain_number(start)
        {
            return number;
        }
        // The commonest of the rest is a date, read in one SWAR parse.
        let date = self.bytes.get(start..start + 10).filter(|chunk| chunk[4] == b'-').and_then(Day::parse);
        let ends_here = self.bytes.get(start + 10).is_none_or(|&b| CLASS[b as usize] == 0 && b != b'/');
        if let Some(day) = date.filter(|_| ends_here) {
            self.pos = start + 10;
            return Tok::Date(day);
        }
        let Word { end, classes } = self.scan_word(start);
        if classes & !(DIGIT | UNDERSCORE) == 0 {
            return self.number(start, end, classes);
        }
        self.pos = end;
        classify_digit_word(&self.src[start..end])
    }

    /// A number or percent that is a word on its own: digits, perhaps a
    /// fraction, and then nothing that would make it a longer word.
    fn plain_number(&mut self, start: usize) -> Option<Tok<'s>> {
        let (number, len) = Dec::prefix(&self.bytes[start..])?;
        let end = start + len;
        match self.bytes.get(end) {
            Some(&b) if CLASS[b as usize] != 0 || b == b'/' => None,
            Some(b'%') => {
                self.pos = end + 1;
                Some(Tok::Percent(number))
            }
            _ => {
                self.pos = end;
                Some(Tok::Number(number))
            }
        }
    }

    /// A number or percent. A point belongs to the number only when digits
    /// follow it, so `2026..2027` is a range and `84.20` a number.
    fn number(&mut self, start: usize, word_end: usize, classes: u8) -> Tok<'s> {
        let end = word_end + fraction(&self.bytes[word_end..]);
        let bytes = self.bytes;
        let ends_word = |at: usize| bytes.get(at).is_none_or(|&b| CLASS[b as usize] == 0);
        if end > word_end && bytes.get(end) == Some(&b'y') && ends_word(end + 1) {
            return self.fractional_years(start, end);
        }
        if end > word_end && !ends_word(end) {
            // `84.20USD`: a fraction leaves no word boundary, unlike `84USD`.
            self.pos = self.scan_word(end).end;
            return Tok::Invalid(if self.bytes[end].is_ascii_uppercase() {
                Malformed::GluedAmount
            } else {
                Malformed::Number
            });
        }
        let text = &self.bytes[start..end];
        let value = Dec::parse(text).filter(|_| classes & UNDERSCORE == 0 || underscores_between_digits(text));
        let percent = self.bytes.get(end) == Some(&b'%');
        self.pos = end + usize::from(percent);
        match (value, percent) {
            (None, _) => Tok::Invalid(Malformed::Number),
            (Some(number), false) => Tok::Number(number),
            (Some(number), true) => Tok::Percent(number),
        }
    }

    /// `27.5y`: years with a fraction, as the whole months they come to.
    fn fractional_years(&mut self, start: usize, end: usize) -> Tok<'s> {
        self.pos = end + 1;
        let months = Dec::parse(&self.bytes[start..end])
            .and_then(|years| Dec { mantissa: years.mantissa.checked_mul(12)?, scale: years.scale }.to_qty(0).ok())
            .and_then(|months| i32::try_from(months.0).ok());
        months.map_or(Tok::Invalid(Malformed::Span), |months| Tok::Span(Span::months(months)))
    }

    fn unit(&mut self, start: usize) -> Tok<'s> {
        // Capitals and then nothing that would make it a longer word: `USD`.
        let capitals = self.bytes[start..].iter().take_while(|byte| byte.is_ascii_uppercase()).count();
        let end = start + capitals;
        if self.bytes.get(end).is_none_or(|&b| CLASS[b as usize] == 0 && !matches!(b, b'.' | b'/')) {
            self.pos = end;
            return Tok::Unit(&self.src[start..end]);
        }
        let Word { mut end, mut classes } = self.scan_word(start);
        // `BRK.B`: a dot joins the parts of a commodity when a letter or digit
        // follows it.
        while self.bytes.get(end) == Some(&b'.') && self.bytes.get(end + 1).is_some_and(u8::is_ascii_alphanumeric) {
            let part = self.scan_word(end + 1);
            (end, classes) = (part.end, classes | part.classes);
        }
        self.pos = end;
        match classes & (LOWER | SYMBOL | SLASH) {
            0 => Tok::Unit(&self.src[start..end]),
            _ => Tok::Invalid(Malformed::Word),
        }
    }

    /// `#purpose` or `^code`: lowercase letters, digits and `-_:./*` behind the mark.
    fn mark(&mut self, start: usize) -> Tok<'s> {
        let is_code_byte = |b: u8| CLASS[b as usize] != 0 || matches!(b, b':' | b'/');
        let mut end = start + 1;
        while let Some(&b) = self.bytes.get(end) {
            let dot_inside = b == b'.' && self.bytes.get(end + 1).is_some_and(|&n| is_code_byte(n));
            if !is_code_byte(b) && !dot_inside {
                break;
            }
            end += 1;
        }
        self.pos = end;
        let text = &self.src[start..end];
        match text.bytes().nth(1) {
            _ if text.bytes().any(|b| b.is_ascii_uppercase()) => Tok::Invalid(Malformed::Word),
            Some(b'a'..=b'z' | b'0'..=b'9') if text.starts_with('#') => Tok::Purpose(Name(&text[1..])),
            Some(b'a'..=b'z' | b'0'..=b'9') => Tok::Code(Code(text)),
            _ => Tok::Invalid(Malformed::Mark),
        }
    }

    /// A string. Escapes are checked but kept raw; the contents are the slice
    /// between the quotes.
    fn string(&mut self, start: usize) -> Tok<'s> {
        let mut at = start + 1;
        self.pos = self.bytes.len();
        loop {
            let Some(offset) = memchr2(b'"', b'\\', &self.bytes[at..]) else {
                return Tok::Invalid(Malformed::UnterminatedString);
            };
            at += offset;
            match self.bytes[at..].get(..2) {
                _ if self.bytes[at] == b'"' => {
                    self.pos = at + 1;
                    return Tok::Str(&self.src[start + 1..at]);
                }
                Some(b"\\\"" | b"\\\\" | b"\\n" | b"\\t") => at += 2,
                Some(_) => {
                    let escaped = self.src[at + 1..].chars().next().map_or(1, char::len_utf8);
                    self.pos = at + 1 + escaped;
                    return Tok::Invalid(Malformed::Escape(at as u32));
                }
                None => return Tok::Invalid(Malformed::UnterminatedString),
            }
        }
    }

    /// Punctuation, or a character the language does not use.
    fn punctuation(&mut self, start: usize) -> Tok<'s> {
        let Some((punct, len)) = Punct::lex(&self.bytes[start..]) else { return self.stray(start) };
        self.pos = start + len;
        Tok::Punct(punct)
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
        self.pos += digits + fraction(&self.bytes[self.pos + digits..]);
        Tok::Invalid(Malformed::Currency)
    }
}

/// The length of the `.fraction` at the start of `bytes`: nothing, unless a
/// point is followed by digits.
fn fraction(bytes: &[u8]) -> usize {
    match bytes {
        [b'.', after @ ..] => match after.iter().take_while(|b| b.is_ascii_digit()).count() {
            0 => 0,
            digits => digits + 1,
        },
        _ => 0,
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

/// The bytes of a plain name, `[a-z0-9_*-]`: what `/` joins into a path, and
/// the only bytes the fast path for names has to look at.
static PLAIN: [bool; 256] = {
    let mut table = [false; 256];
    let mut byte = 0;
    while byte < 256 {
        table[byte] = matches!(byte as u8, b'0'..=b'9' | b'a'..=b'z' | b'_' | b'*' | b'-');
        byte += 1;
    }
    table
};

/// A scanned word: where it ends, and the union of its bytes' classes.
struct Word {
    end: usize,
    classes: u8,
}

// ─── Words that start with a digit ──────────────────────────────────────────

/// Classifies a word whose first byte is a digit, by its exact shape. This is
/// the general path; the common shapes are handled before it is reached.
fn classify_digit_word(text: &str) -> Tok<'_> {
    let bytes = text.as_bytes();
    // A `YYYY-MM` month is checked as its first day.
    if is_shaped(bytes, b"dddd-dd-dd") {
        return Day::parse(bytes).map_or(Tok::Invalid(Malformed::Date), Tok::Date);
    }
    if is_shaped(bytes, b"dddd-dd") {
        return Day::parse(&[bytes, b"-01"].concat()).map_or(Tok::Invalid(Malformed::Date), Tok::Month);
    }
    if is_shaped(bytes, b"dd-dd") {
        return Tok::MonthDay((bytes[0] - b'0') * 10 + bytes[1] - b'0', (bytes[3] - b'0') * 10 + bytes[4] - b'0');
    }
    if let Some(span) = Span::parse(bytes) {
        return Tok::Span(span);
    }
    match () {
        _ if is_unpadded_date(text) => Tok::Invalid(Malformed::LooseDate),
        _ if is_slash_date(text) => Tok::Invalid(Malformed::SlashDate),
        _ if is_glued_amount(bytes) => Tok::Invalid(Malformed::GluedAmount),
        _ if bytes.iter().any(u8::is_ascii_uppercase) => Tok::Invalid(Malformed::Word),
        _ => Tok::Name(text),
    }
}

/// Whether every `_` in `text` sits between two digits.
fn underscores_between_digits(text: &[u8]) -> bool {
    let between = |i: usize| i > 0 && text[i - 1].is_ascii_digit() && text.get(i + 1).is_some_and(u8::is_ascii_digit);
    text.iter().enumerate().all(|(i, &b)| b != b'_' || between(i))
}

/// Whether `bytes` fits `pattern`, where `d` stands for a digit and anything
/// else for itself.
fn is_shaped(bytes: &[u8], pattern: &[u8]) -> bool {
    bytes.len() == pattern.len()
        && bytes.iter().zip(pattern).all(|(&b, &p)| if p == b'd' { b.is_ascii_digit() } else { b == p })
}

/// `YYYY-M[-D]` with one-digit parts: a date someone forgot to pad.
fn is_unpadded_date(text: &str) -> bool {
    let mut parts = text.split('-');
    let (Some(year), Some(month)) = (parts.next(), parts.next()) else { return false };
    let day = parts.next();
    let digits = |part: &str, max: usize| (1..=max).contains(&part.len()) && part.bytes().all(|b| b.is_ascii_digit());
    year.len() == 4 && digits(year, 4) && digits(month, 2) && day.is_none_or(|d| digits(d, 2)) && parts.next().is_none()
}

/// `MM/DD/YYYY`, `DD/MM/YYYY` or `YYYY/MM/DD`, unpadded or not.
fn is_slash_date(text: &str) -> bool {
    let mut widths = text.split('/').map(|part| part.bytes().all(|b| b.is_ascii_digit()).then_some(part.len()));
    let parts = (widths.next(), widths.next(), widths.next(), widths.next());
    matches!(parts, (Some(Some(4)), Some(Some(1..=2)), Some(Some(1..=2)), None))
        || matches!(parts, (Some(Some(1..=2)), Some(Some(1..=2)), Some(Some(4)), None))
}

/// Digits then a commodity with no space between: `50USD`.
fn is_glued_amount(bytes: &[u8]) -> bool {
    let digits = bytes.iter().take_while(|b| matches!(b, b'0'..=b'9' | b'_')).count();
    let unit = &bytes[digits..];
    unit.first().is_some_and(u8::is_ascii_uppercase)
        && unit.iter().all(|b| matches!(b, b'A'..=b'Z' | b'0'..=b'9' | b'_'))
}
