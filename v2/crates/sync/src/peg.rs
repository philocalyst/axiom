//! Patterns as the ledger writes them (LANGUAGE §14): a small parsing-expression
//! grammar, compiled once to a program of ops and matched over bytes by a tiny
//! interpreter.
//!
//! ```text
//! "TRADER JOE"                       a literal, in any case
//! "INV-" digit+ "-" digit+           a sequence; `*` `+` `?` repeat what they follow
//! "CHK-" digit+ / "CHECK " digit+    ordered choice: the first that matches
//! "PAYPAL *" payee:rest              a named capture around an atom or a group
//! ach space+                         a pattern the ledger declared, by name
//! ```
//!
//! The atoms are a string; the classes `digit`, `letter`, `alnum`, `space`, `any`
//! (one character) and `rest` (all that is left); the anchors `start` and `end`;
//! a named pattern; and a group. A repeat takes all it can and never gives it
//! back, as in every PEG. A capture named `payee`, `code`, `amount` or `date`
//! fills that part of the record ([`Capture`]).

use axiom_core::diag::closest;
use axiom_core::{Diagnostic, Loc};
use memchr::memmem;

/// Groups nest no deeper than this: a pattern is read by recursion, and it is
/// the ledger's, which may be anything.
const MAX_DEPTH: usize = 32;

/// What is wrong with a pattern's text, and where in it.
#[derive(Clone, Debug)]
pub struct PatternError {
    pub at: usize,
    pub len: usize,
    pub message: String,
    pub help: Option<String>,
    /// The name of a pattern it calls that there is none of.
    unknown: Option<String>,
}

impl PatternError {
    fn new(at: usize, len: usize, message: impl Into<String>) -> PatternError {
        PatternError { at, len, message: message.into(), help: None, unknown: None }
    }

    /// The diagnostic, given where the pattern's text is in its file.
    pub fn diagnostic(&self, pattern: Loc) -> Diagnostic {
        let start = pattern.start + self.at as u32;
        let at = Loc::new(pattern.file, start, start + self.len.max(1) as u32);
        let error = Diagnostic::error("bad-pattern", self.message.clone()).label(at, "here");
        match &self.help {
            Some(help) => error.help(help.clone()),
            None => error,
        }
    }
}

/// The parts of a record that a capture fills.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Capture {
    /// Who the money was for, recognized in turn: the pattern's own entity is the
    /// go-between (`via`).
    Payee,
    /// A code the memo names.
    Code,
    /// The amount, when the memo says it better than a column.
    Amount,
    /// The day, likewise.
    Date,
}

const CAPTURES: [(&str, Capture); 4] =
    [("payee", Capture::Payee), ("code", Capture::Code), ("amount", Capture::Amount), ("date", Capture::Date)];

/// Where each capture was, in [`Capture`] order.
pub type Parts = [Option<(usize, usize)>; 4];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CharClass {
    Digit,
    Letter,
    Alnum,
    Space,
    /// One character.
    Any,
    /// Everything up to the end, which may be nothing.
    Rest,
    /// Nowhere: the beginning of the memo.
    Start,
    /// Nowhere: the end of it.
    End,
}

const CLASSES: [(&str, CharClass); 8] = [
    ("digit", CharClass::Digit),
    ("letter", CharClass::Letter),
    ("alnum", CharClass::Alnum),
    ("space", CharClass::Space),
    ("any", CharClass::Any),
    ("rest", CharClass::Rest),
    ("start", CharClass::Start),
    ("end", CharClass::End),
];

impl CharClass {
    /// Where the class ends if it matches at `at`. Letters include the bytes of
    /// non-ASCII characters, and `any` takes a whole character.
    fn step(self, hay: &[u8], at: usize) -> Option<usize> {
        let one = |fits: fn(u8) -> bool| hay.get(at).copied().filter(|&byte| fits(byte)).map(|_| at + 1);
        match self {
            CharClass::Digit => one(|byte| byte.is_ascii_digit()),
            CharClass::Letter => one(|byte| byte.is_ascii_alphabetic() || byte >= 0x80),
            CharClass::Alnum => one(|byte| byte.is_ascii_alphanumeric() || byte >= 0x80),
            CharClass::Space => one(|byte| byte.is_ascii_whitespace()),
            CharClass::Any => hay.get(at).map(|&lead| at + char_width(lead)).filter(|&end| end <= hay.len()),
            CharClass::Rest => Some(hay.len()),
            CharClass::Start => (at == 0).then_some(at),
            CharClass::End => (at == hay.len()).then_some(at),
        }
    }
}

/// The bytes of the character that starts with `lead`.
fn char_width(lead: u8) -> usize {
    match lead {
        0xF0.. => 4,
        0xE0.. => 3,
        0xC0.. => 2,
        _ => 1,
    }
}

/// One op of a program. A program is a tree written out in order: an op that has
/// others under it says how many ops they are, and those follow it.
#[derive(Clone, Debug)]
enum Op {
    /// Lowercased: what is matched is lowercased too.
    Literal(Box<[u8]>),
    Class(CharClass),
    /// Each of the ops beneath, in order.
    Seq {
        len: u32,
    },
    /// The first of the ops beneath that matches.
    Choice {
        len: u32,
    },
    Repeat {
        min: u32,
        max: u32,
        len: u32,
    },
    Capture {
        name: u32,
        len: u32,
    },
}

/// A compiled pattern.
#[derive(Clone, Debug)]
pub struct Pattern {
    program: Box<[Op]>,
    /// The names its captures go by.
    names: Box<[String]>,
    /// The literals a match begins with, one for each way it can; `None` if it
    /// may begin with anything.
    starts: Option<Vec<Vec<u8>>>,
}

/// What a match has done so far; kept between matches, to be reused.
#[derive(Default)]
pub struct Run {
    literal: usize,
    /// Each capture's name, and the bytes it took.
    captures: Vec<(u32, usize, usize)>,
}

/// Where a match is, and how many of its bytes were literals: the measure of
/// how specific it is.
#[derive(Clone, Copy, Debug)]
pub struct Found {
    pub start: usize,
    pub end: usize,
    pub literal: usize,
}

impl Pattern {
    /// The pattern `source` says, calling the ledger's own `named` patterns.
    pub fn new(source: &str, named: &Patterns) -> Result<Pattern, PatternError> {
        let mut parser = Parser { text: source, at: 0, depth: 0, ops: Vec::new(), names: Vec::new(), named };
        parser.choice()?;
        parser.skip_space();
        if parser.at < source.len() {
            return Err(parser.error(1, "there is nothing to close here"));
        }
        Ok(Pattern::from(parser.ops, parser.names))
    }

    /// The pattern that is exactly this text, in any case.
    pub fn literal(text: &str) -> Pattern {
        Pattern::from(vec![Op::Literal(text.to_ascii_lowercase().into_bytes().into())], Vec::new())
    }

    fn from(program: Vec<Op>, names: Vec<String>) -> Pattern {
        let mut pattern = Pattern { program: program.into(), names: names.into(), starts: None };
        pattern.starts = pattern.starts_of(0);
        pattern
    }

    /// The literals a match begins with, one for each way it can. `None` if it
    /// may begin with a class instead, and so has to be tried everywhere.
    pub fn starts(&self) -> Option<&[Vec<u8>]> {
        self.starts.as_deref()
    }

    /// How many ops the op at `at` takes with those beneath it.
    fn extent(&self, at: usize) -> usize {
        match self.program[at] {
            Op::Literal(_) | Op::Class(_) => 1,
            Op::Seq { len } | Op::Choice { len } | Op::Repeat { len, .. } | Op::Capture { len, .. } => 1 + len as usize,
        }
    }

    /// The ops directly beneath the one at `at`, whose body is `len` ops long.
    fn children(&self, at: usize, len: u32) -> impl Iterator<Item = usize> + '_ {
        let end = at + 1 + len as usize;
        let next = move |child: usize| Some(child).filter(|&child| child < end);
        std::iter::successors(next(at + 1), move |&child| next(child + self.extent(child)))
    }

    fn starts_of(&self, at: usize) -> Option<Vec<Vec<u8>>> {
        match &self.program[at] {
            Op::Literal(bytes) => Some(vec![bytes.to_vec()]),
            Op::Seq { len } => {
                let mut first =
                    self.children(at, *len).skip_while(|&c| matches!(self.program[c], Op::Class(CharClass::Start)));
                first.next().and_then(|child| self.starts_of(child))
            }
            Op::Capture { .. } => self.starts_of(at + 1),
            Op::Repeat { min, .. } if *min > 0 => self.starts_of(at + 1),
            Op::Choice { len } => {
                let each: Option<Vec<_>> = self.children(at, *len).map(|child| self.starts_of(child)).collect();
                each.map(|each| each.concat())
            }
            Op::Class(_) | Op::Repeat { .. } => None,
        }
    }

    /// The literal every match begins with, if there is only one way to.
    fn prefix(&self) -> &[u8] {
        match self.starts() {
            Some([only]) => only,
            _ => &[],
        }
    }

    /// The match that starts exactly at `at` in lowercased `hay`, if there is one.
    pub fn matches_at(&self, hay: &[u8], at: usize, run: &mut Run) -> Option<Found> {
        run.literal = 0;
        run.captures.clear();
        let end = self.step(0, hay, at, run)?;
        Some(Found { start: at, end, literal: run.literal })
    }

    /// The first match at or after `from`, looking for the leading literal.
    pub fn find(&self, hay: &[u8], from: usize, run: &mut Run) -> Option<Found> {
        let (mut at, prefix) = (from, self.prefix());
        loop {
            if !prefix.is_empty() {
                at += memmem::find(hay.get(at..)?, prefix)?;
            }
            if let Some(found) = self.matches_at(hay, at, run) {
                return Some(found);
            }
            at += 1;
            if at > hay.len() {
                return None;
            }
        }
    }

    /// Where the capture that fills `part` was, in the last match `run` made.
    pub fn capture(&self, part: Capture, run: &Run) -> Option<(usize, usize)> {
        let word = CAPTURES.iter().find(|(_, known)| *known == part)?.0;
        let index = self.names.iter().position(|name| name == word)? as u32;
        run.captures.iter().rev().find(|capture| capture.0 == index).map(|&(_, start, end)| (start, end))
    }

    /// Where every capture was, in the last match `run` made.
    pub fn parts(&self, run: &Run) -> Parts {
        CAPTURES.map(|(_, part)| self.capture(part, run))
    }

    /// Where the op at `at` ends if it matches at position `pos`. An op that
    /// fails has left nothing behind.
    fn step(&self, at: usize, hay: &[u8], pos: usize, run: &mut Run) -> Option<usize> {
        let (literal, captured) = (run.literal, run.captures.len());
        let ended = match &self.program[at] {
            Op::Literal(bytes) => hay[pos..].starts_with(bytes).then(|| {
                run.literal += bytes.len();
                pos + bytes.len()
            }),
            Op::Class(class) => class.step(hay, pos),
            Op::Seq { len } => self.children(at, *len).try_fold(pos, |pos, child| self.step(child, hay, pos, run)),
            Op::Choice { len } => self.children(at, *len).find_map(|child| self.step(child, hay, pos, run)),
            Op::Repeat { min, max, .. } => {
                let (mut end, mut count) = (pos, 0);
                while count < *max {
                    match self.step(at + 1, hay, end, run) {
                        // A body that matches nothing would go on forever.
                        Some(next) if next > end => (end, count) = (next, count + 1),
                        _ => break,
                    }
                }
                (count >= *min).then_some(end)
            }
            Op::Capture { name, .. } => {
                self.step(at + 1, hay, pos, run).inspect(|&end| run.captures.push((*name, pos, end)))
            }
        };
        if ended.is_none() {
            run.literal = literal;
            run.captures.truncate(captured);
        }
        ended
    }
}

/// The patterns a ledger declares by name (`pattern ach = …`), for others to call.
#[derive(Default)]
pub struct Patterns {
    compiled: Vec<(String, Pattern)>,
}

impl Patterns {
    /// Compiles the definitions, each `(name, source)`, in whatever order they
    /// call one another. What cannot be compiled is reported with its position
    /// in `definitions`.
    pub fn new(definitions: &[(&str, &str)]) -> Result<Patterns, Vec<(usize, PatternError)>> {
        let (mut patterns, mut errors) = (Patterns::default(), Vec::new());
        let mut waiting: Vec<usize> = (0..definitions.len()).collect();
        loop {
            let (mut stuck, before) = (Vec::new(), waiting.len());
            for at in waiting {
                let (name, source) = definitions[at];
                let clash = CLASSES.iter().any(|(class, _)| *class == name) || patterns.get(name).is_some();
                let result = match clash {
                    true => {
                        Err(PatternError::new(0, name.len(), format!("`{name}` is already a name a pattern can have")))
                    }
                    false => Pattern::new(source, &patterns),
                };
                match result {
                    Ok(pattern) => patterns.compiled.push((name.to_string(), pattern)),
                    Err(error)
                        if error.unknown.as_ref().is_some_and(|wanted| definitions.iter().any(|d| d.0 == wanted)) =>
                    {
                        stuck.push((at, error))
                    }
                    Err(error) => errors.push((at, error)),
                }
            }
            if stuck.is_empty() {
                break;
            }
            if stuck.len() == before {
                for (at, mut error) in stuck {
                    let wanted = error.unknown.take().unwrap_or_default();
                    error.message =
                        format!("`{}` and `{wanted}` are defined in terms of each other", definitions[at].0);
                    errors.push((at, error));
                }
                break;
            }
            waiting = stuck.into_iter().map(|(at, _)| at).collect();
        }
        if errors.is_empty() { Ok(patterns) } else { Err(errors) }
    }

    fn get(&self, name: &str) -> Option<&Pattern> {
        self.compiled.iter().find(|(known, _)| known == name).map(|(_, pattern)| pattern)
    }
}

/// The text of a pattern, read into ops.
struct Parser<'t, 'n> {
    text: &'t str,
    at: usize,
    /// How many groups deep the parser is.
    depth: usize,
    ops: Vec<Op>,
    names: Vec<String>,
    named: &'n Patterns,
}

impl<'t> Parser<'t, '_> {
    fn error(&self, len: usize, message: impl Into<String>) -> PatternError {
        PatternError::new(self.at, len, message)
    }

    fn skip_space(&mut self) {
        self.at += self.text[self.at..].len() - self.text[self.at..].trim_start().len();
    }

    fn peek(&mut self) -> Option<char> {
        self.skip_space();
        self.text[self.at..].chars().next()
    }

    /// The characters from here that `accept` takes.
    fn word(&self, accept: fn(char) -> bool) -> &'t str {
        let rest = &self.text[self.at..];
        &rest[..rest.find(|c| !accept(c)).unwrap_or(rest.len())]
    }

    /// Puts `op` before the ops written since `start`, over them all.
    fn wrap(&mut self, start: usize, op: impl FnOnce(u32) -> Op) {
        let len = (self.ops.len() - start) as u32;
        self.ops.insert(start, op(len));
    }

    fn name_index(&mut self, name: &str) -> u32 {
        let known = self.names.iter().position(|known| known == name);
        known.unwrap_or_else(|| {
            self.names.push(name.to_string());
            self.names.len() - 1
        }) as u32
    }

    fn choice(&mut self) -> Result<(), PatternError> {
        let start = self.ops.len();
        self.sequence()?;
        let mut alternatives = 1;
        while self.peek() == Some('/') {
            self.at += 1;
            self.sequence()?;
            alternatives += 1;
        }
        if alternatives > 1 {
            self.wrap(start, |len| Op::Choice { len });
        }
        Ok(())
    }

    fn sequence(&mut self) -> Result<(), PatternError> {
        let (start, mut items) = (self.ops.len(), 0);
        loop {
            self.item()?;
            items += 1;
            if matches!(self.peek(), None | Some('/' | ')')) {
                break;
            }
        }
        if items > 1 {
            self.wrap(start, |len| Op::Seq { len });
        }
        Ok(())
    }

    /// An atom, with the repeat that follows it and the name it captures under.
    fn item(&mut self) -> Result<(), PatternError> {
        self.skip_space();
        let start = self.ops.len();
        let word = self.word(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        let name = self.text[self.at + word.len()..].starts_with(':').then_some(word).filter(|word| !word.is_empty());
        if let Some(name) = name {
            self.at += name.len() + 1;
        }
        self.atom()?;
        let repeat = match self.text[self.at..].chars().next() {
            Some('*') => Some((0, u32::MAX)),
            Some('+') => Some((1, u32::MAX)),
            Some('?') => Some((0, 1)),
            _ => None,
        };
        if let Some((min, max)) = repeat {
            self.at += 1;
            self.wrap(start, |len| Op::Repeat { min, max, len });
        }
        if let Some(name) = name {
            let index = self.name_index(name);
            self.wrap(start, |len| Op::Capture { name: index, len });
        }
        Ok(())
    }

    fn atom(&mut self) -> Result<(), PatternError> {
        match self.peek() {
            Some('"') => self.literal(),
            Some('(') => {
                if self.depth == MAX_DEPTH {
                    return Err(self.error(1, format!("groups nest at most {MAX_DEPTH} deep")));
                }
                (self.at, self.depth) = (self.at + 1, self.depth + 1);
                self.choice()?;
                self.depth -= 1;
                match self.peek() {
                    Some(')') => {
                        self.at += 1;
                        Ok(())
                    }
                    _ => Err(self.error(1, "this group is never closed")),
                }
            }
            Some(c) if c.is_ascii_alphabetic() => self.name(),
            Some(c) => Err(self.error(c.len_utf8(), format!("expected a literal, a class or `(`, not `{c}`"))),
            None => Err(self.error(0, "the pattern ends where something was expected")),
        }
    }

    /// `"TEXT"`, where `\"` and `\\` are a quote and a backslash.
    fn literal(&mut self) -> Result<(), PatternError> {
        let start = self.at;
        let mut text = String::new();
        let mut chars = self.text[start + 1..].char_indices();
        while let Some((offset, c)) = chars.next() {
            match c {
                '"' if text.is_empty() => {
                    self.at = start;
                    return Err(self.error(offset + 2, "a literal cannot be empty"));
                }
                '"' => {
                    self.at = start + offset + 2;
                    self.ops.push(Op::Literal(text.to_ascii_lowercase().into_bytes().into()));
                    return Ok(());
                }
                '\\' => match chars.next() {
                    Some((_, escaped @ ('"' | '\\'))) => text.push(escaped),
                    _ => {
                        self.at = start + offset + 1;
                        return Err(self.error(2, "a backslash in a literal is followed by `\"` or `\\`"));
                    }
                },
                c => text.push(c),
            }
        }
        self.at = start;
        Err(self.error(1, "this literal is never closed"))
    }

    /// A class, or the pattern of the ledger that has this name, written in.
    fn name(&mut self) -> Result<(), PatternError> {
        let word = self.word(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if let Some(&(_, class)) = CLASSES.iter().find(|(name, _)| *name == word) {
            self.ops.push(Op::Class(class));
        } else if let Some(callee) = self.named.get(word) {
            for op in callee.program.iter() {
                let op = match op {
                    Op::Capture { name, len } => {
                        let name = self.name_index(&callee.names[*name as usize]);
                        Op::Capture { name, len: *len }
                    }
                    other => other.clone(),
                };
                self.ops.push(op);
            }
        } else {
            let known =
                CLASSES.iter().map(|(name, _)| *name).chain(self.named.compiled.iter().map(|(n, _)| n.as_str()));
            let mut error = self.error(word.len(), format!("there is no class or pattern called `{word}`"));
            error.help = Some(match closest(word, known) {
                Some(near) => format!("did you mean `{near}`?"),
                None => "write text in quotes; the classes are digit, letter, alnum, space, any and rest".to_string(),
            });
            error.unknown = Some(word.to_string());
            return Err(error);
        }
        self.at += word.len();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compile(source: &str) -> Pattern {
        Pattern::new(source, &Patterns::default()).unwrap_or_else(|error| panic!("{source}: {}", error.message))
    }

    /// The matched text, and each capture, when `pattern` is searched for in `memo`.
    fn find(pattern: &str, memo: &str) -> Option<(String, usize, Vec<(String, String)>)> {
        let pattern = compile(pattern);
        let hay = memo.to_ascii_lowercase().into_bytes();
        let mut run = Run::default();
        let found = pattern.find(&hay, 0, &mut run)?;
        let text = |(start, end): (usize, usize)| String::from_utf8_lossy(&hay[start..end]).into_owned();
        let captures = CAPTURES
            .iter()
            .filter_map(|&(name, part)| Some((name.to_string(), text(pattern.capture(part, &run)?))))
            .collect();
        Some((text((found.start, found.end)), found.literal, captures))
    }

    fn matched(pattern: &str, memo: &str) -> Option<String> {
        find(pattern, memo).map(|(text, ..)| text)
    }

    #[test]
    fn literals_sequences_and_classes() {
        assert_eq!(matched("\"trader joe\"", "POS TRADER JOE'S #634").as_deref(), Some("trader joe"));
        assert_eq!(
            matched("\"INV-\" digit+ \"-\" digit+", "PAYMENT INV-2026-01 THANKS").as_deref(),
            Some("inv-2026-01")
        );
        assert_eq!(matched("\"inv-\" digit+", "inv-x"), None);
        assert_eq!(matched("\"sq *bl\" letter \"e bottle\"", "SQ *BLUE BOTTLE 12").as_deref(), Some("sq *blue bottle"));
        assert_eq!(matched("\"a\" space* \"b\"", "A  \t B").as_deref(), Some("a  \t b"));
        assert_eq!(matched("\"caf\" any \"!\"", "CAFÉ!").as_deref(), Some("cafÉ!"), "case folds ASCII only");
        assert_eq!(matched("\"a\" \"b\"", "xab").as_deref(), Some("ab"), "a search, not an anchor");
        assert_eq!(matched("alnum+ \"-\" alnum+", "ref 12ab-9z ok").as_deref(), Some("12ab-9z"));
    }

    #[test]
    fn anchors_and_the_rest() {
        assert_eq!(matched("start \"pos \"", "POS DEBIT"), Some("pos ".to_string()));
        assert_eq!(matched("start \"debit\"", "POS DEBIT"), None);
        assert_eq!(matched("\"debit\" end", "POS DEBIT"), Some("debit".to_string()));
        assert_eq!(matched("\"pos\" end", "POS DEBIT"), None);
        assert_eq!(matched("\"pos \" rest", "POS DEBIT").as_deref(), Some("pos debit"));
        assert_eq!(matched("\"pos debit\" rest", "POS DEBIT").as_deref(), Some("pos debit"), "the rest may be nothing");
        assert_eq!(matched("start start \"pos\"", "POS").as_deref(), Some("pos"), "an anchor matches nowhere, twice");
    }

    #[test]
    fn choice_takes_the_first_that_matches_and_repeats_do_not_give_back() {
        assert_eq!(matched("\"chk-\" digit+ / \"check \" digit+", "CHECK 1041").as_deref(), Some("check 1041"));
        assert_eq!(matched("(\"a\" / \"ab\") \"c\"", "abc"), None, "PEG choice does not backtrack into the second");
        assert_eq!(matched("digit* \"1\"", "0001"), None, "the repeat took the last digit");
        assert_eq!(matched("\"x\" (\"a\" / \"b\")* \"y\"", "xabbay").as_deref(), Some("xabbay"));
        assert_eq!(matched("\"x\" (\"a\"?)* \"y\"", "xy").as_deref(), Some("xy"), "an empty repeat does not loop");
    }

    #[test]
    fn captures_say_where_and_literals_are_counted() {
        let (text, literal, captures) = find("\"PAYPAL *\" payee:rest", "paypal *etsy seller").unwrap();
        assert_eq!((text.as_str(), literal), ("paypal *etsy seller", 8));
        assert_eq!(captures, [("payee".to_string(), "etsy seller".to_string())]);
        let (_, literal, captures) = find("code:(\"inv-\" digit+ \"-\" digit+) / \"x\"", "see inv-12-3").unwrap();
        assert_eq!((literal, captures[0].1.as_str()), (5, "inv-12-3"));
        let (_, _, captures) = find("(payee:digit \"x\" / \"5\") code:digit", "55").unwrap();
        assert_eq!(captures, [("code".to_string(), "5".to_string())], "a failed branch leaves no capture behind");
        let (_, _, captures) =
            find("amount:(digit+ \".\" digit+) \" on \" date:(digit+ \"/\" digit+)", "fx 58.40 on 01/05").unwrap();
        assert_eq!(captures.len(), 2);
        assert_eq!(captures[0], ("amount".to_string(), "58.40".to_string()));
        assert_eq!(captures[1], ("date".to_string(), "01/05".to_string()));
    }

    #[test]
    fn a_pattern_says_which_literals_a_match_begins_with() {
        let starts = |pattern: &str| {
            compile(pattern)
                .starts()
                .map(|starts| starts.iter().map(|start| String::from_utf8(start.clone()).unwrap()).collect::<Vec<_>>())
        };
        assert_eq!(starts("\"PAYPAL\" \" *\" payee:rest"), Some(vec!["paypal".to_string()]));
        assert_eq!(starts("code:(\"inv-\" digit+)"), Some(vec!["inv-".to_string()]));
        assert_eq!(starts("start \"pos\""), Some(vec!["pos".to_string()]), "an anchor does not begin a match");
        assert_eq!(
            starts("\"a\" digit / (\"b\" / \"c\")+ \"d\""),
            Some(vec!["a".to_string(), "b".to_string(), "c".to_string()])
        );
        assert_eq!(starts("digit+ \"-\""), None);
        assert_eq!(starts("\"a\" / digit"), None, "one way to begin with anything is enough");
        assert_eq!(starts("(\"a\"?)  \"b\""), None, "an optional start may be skipped");
        // Found by the one literal there is.
        let (hay, mut run) = (b"xx inv-12 inv-34".to_vec(), Run::default());
        let pattern = compile("code:(\"inv-\" digit+)");
        assert_eq!(pattern.find(&hay, 0, &mut run).map(|found| found.start), Some(3));
        assert_eq!(pattern.find(&hay, 4, &mut run).map(|found| found.start), Some(10));
    }

    #[test]
    fn the_ledgers_named_patterns_are_called_by_name_in_any_order() {
        let named = Patterns::new(&[
            ("card", "\"card \" digits \" \" payee:(letter+)"),
            ("digits", "digit digit digit digit"),
            ("ach", "\"ach \" (\"debit\" / \"credit\") space+"),
        ])
        .unwrap_or_else(|errors| panic!("{}", errors[0].1.message));
        let call = |source: &str, memo: &str| {
            let pattern = Pattern::new(source, &named).unwrap_or_else(|error| panic!("{}", error.message));
            let mut run = Run::default();
            let found = pattern.find(memo.to_ascii_lowercase().as_bytes(), 0, &mut run)?;
            let payee = pattern
                .capture(Capture::Payee, &run)
                .map(|(start, end)| memo.to_ascii_lowercase()[start..end].to_string());
            Some((memo[found.start..found.end].to_string(), found.literal, payee))
        };
        assert_eq!(call("ach", "ACH CREDIT  x"), Some(("ACH CREDIT  ".to_string(), 10, None)));
        assert_eq!(
            call("ach payee:rest", "ach debit ashgrove"),
            Some(("ach debit ashgrove".to_string(), 9, Some("ashgrove".to_string())))
        );
        assert_eq!(call("card \"!\"", "card 1234 x"), None, "a call is the whole pattern, not a piece of it");
        assert_eq!(call("card", "CARD 1234 STORE 9").map(|(text, ..)| text), Some("CARD 1234 STORE".to_string()));
        assert_eq!(
            call("card", "CARD 1234 STORE 9").and_then(|found| found.2),
            Some("store".to_string()),
            "a called pattern's capture is the caller's"
        );
    }

    #[test]
    fn named_patterns_that_cannot_be_built_are_reported() {
        let errors = |definitions: &[(&str, &str)]| {
            Patterns::new(definitions)
                .err()
                .unwrap_or_else(|| panic!("{definitions:?} is fine"))
                .into_iter()
                .map(|(at, error)| (at, error.message))
                .collect::<Vec<_>>()
        };
        assert_eq!(errors(&[("a", "b"), ("b", "a")]).len(), 2, "each says it is part of the cycle");
        assert!(errors(&[("a", "b"), ("b", "a")])[0].1.contains("in terms of each other"));
        assert_eq!(errors(&[("a", "nope")]), [(0, "there is no class or pattern called `nope`".to_string())]);
        assert_eq!(errors(&[("digit", "\"x\"")]), [(0, "`digit` is already a name a pattern can have".to_string())]);
        assert_eq!(
            errors(&[("a", "\"x\""), ("a", "\"y\"")]),
            [(1, "`a` is already a name a pattern can have".to_string())]
        );
        assert_eq!(errors(&[("ok", "\"x\""), ("bad", "\"never")]), [(1, "this literal is never closed".to_string())]);
    }

    #[test]
    fn a_bad_pattern_says_where() {
        let bad = |pattern: &str| {
            Pattern::new(pattern, &Patterns::default()).err().unwrap_or_else(|| panic!("{pattern} is fine"))
        };
        let error = bad("\"a\" digt+");
        assert_eq!((error.at, error.len, error.message.as_str()), (4, 4, "there is no class or pattern called `digt`"));
        assert_eq!(error.help.as_deref(), Some("did you mean `digit`?"));
        assert_eq!(bad("\"never").message, "this literal is never closed");
        assert_eq!(bad("\"\"").message, "a literal cannot be empty");
        assert_eq!(bad("(\"a\"").message, "this group is never closed");
        assert_eq!(bad("\"a\")").message, "there is nothing to close here");
        assert_eq!(bad("\"a\" /").message, "the pattern ends where something was expected");
        assert_eq!(bad("\"a\" $").message, "expected a literal, a class or `(`, not `$`");
        assert_eq!(bad("").message, "the pattern ends where something was expected");
        let deep = format!("{}\"a\"{}", "(".repeat(1000), ")".repeat(1000));
        assert_eq!(bad(&deep).message, "groups nest at most 32 deep");
        assert!(Pattern::new(&format!("{}\"a\"{}", "(".repeat(32), ")".repeat(32)), &Patterns::default()).is_ok());
        let at = Loc::new(axiom_core::FileId(2), 100, 110);
        let diagnostic = bad("\"a\" digt+").diagnostic(at);
        assert_eq!(
            diagnostic.anchor().map(|loc| (loc.file, loc.start, loc.end)),
            Some((axiom_core::FileId(2), 104, 108))
        );
    }
}
