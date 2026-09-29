//! Patterns as the ledger writes them: a small parsing-expression grammar,
//! compiled to a vector of nodes and matched over bytes by a tiny interpreter.
//!
//! ```text
//! "TRADER JOE"                       a literal, in any case
//! "INV-" digit+ "-" digit+           a sequence; `*` `+` `?` repeat what they follow
//! "CHK-" digit+ / "CHECK " digit+    ordered choice: the first that matches
//! "PAYPAL *" payee:(any+)            a named capture around a group
//! ```
//!
//! The classes are `digit`, `letter`, `space` and `any` (one character). A
//! repeat takes all it can and never gives it back, as in every PEG.

use axiom_core::diag::closest;
use axiom_core::{Diagnostic, Loc};
use memchr::memmem;

/// What is wrong with a pattern's text, and where in it.
#[derive(Clone, Debug)]
pub struct PatternError {
    pub at: usize,
    pub len: usize,
    pub message: String,
    pub help: Option<String>,
}

impl PatternError {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Class {
    Digit,
    Letter,
    Space,
    Any,
}

const CLASSES: [(&str, Class); 4] =
    [("digit", Class::Digit), ("letter", Class::Letter), ("space", Class::Space), ("any", Class::Any)];

impl Class {
    /// Where one member of the class at `at` ends. Letters include the bytes of
    /// non-ASCII characters, and `any` takes a whole character.
    fn step(self, hay: &[u8], at: usize) -> Option<usize> {
        let &byte = hay.get(at)?;
        let fits = match self {
            Class::Digit => byte.is_ascii_digit(),
            Class::Letter => byte.is_ascii_alphabetic() || byte >= 0x80,
            Class::Space => byte.is_ascii_whitespace(),
            Class::Any => return Some(at + char_width(byte)).filter(|&end| end <= hay.len()),
        };
        fits.then_some(at + 1)
    }
}

/// The bytes of the character that starts with `lead`.
pub fn char_width(lead: u8) -> usize {
    match lead {
        0xF0.. => 4,
        0xE0.. => 3,
        0xC0.. => 2,
        _ => 1,
    }
}

/// A node of the program; children are positions in the vector.
#[derive(Debug)]
enum Node {
    /// Lowercased: what is matched is lowercased too.
    Literal(Box<[u8]>),
    Class(Class),
    Sequence(Box<[u32]>),
    Choice(Box<[u32]>),
    Repeat { body: u32, min: u32, max: u32 },
    Capture { name: u32, body: u32 },
}

/// A compiled pattern.
#[derive(Debug)]
pub struct Peg {
    nodes: Vec<Node>,
    root: u32,
    names: Vec<String>,
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

impl Peg {
    pub fn new(text: &str) -> Result<Peg, PatternError> {
        let mut parser = Parser { text, at: 0, nodes: Vec::new(), names: Vec::new() };
        let root = parser.choice()?;
        parser.skip_space();
        if parser.at < text.len() {
            return Err(parser.error(1, "there is nothing to close here"));
        }
        let mut peg = Peg { nodes: parser.nodes, root, names: parser.names, starts: None };
        peg.starts = peg.starts_of(root);
        Ok(peg)
    }

    /// The literals a match begins with, one for each way it can. `None` if it
    /// may begin with a class instead, and so has to be tried everywhere.
    pub fn starts(&self) -> Option<&[Vec<u8>]> {
        self.starts.as_deref()
    }

    fn starts_of(&self, node: u32) -> Option<Vec<Vec<u8>>> {
        match &self.nodes[node as usize] {
            Node::Literal(bytes) => Some(vec![bytes.to_vec()]),
            Node::Sequence(children) => self.starts_of(children[0]),
            Node::Capture { body, .. } => self.starts_of(*body),
            Node::Repeat { body, min, .. } if *min > 0 => self.starts_of(*body),
            Node::Choice(alternatives) => {
                let each: Option<Vec<_>> = alternatives.iter().map(|&alternative| self.starts_of(alternative)).collect();
                each.map(|each| each.concat())
            }
            Node::Class(_) | Node::Repeat { .. } => None,
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
        let end = self.step(self.root, hay, at, run)?;
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

    /// Where the capture called `name` was, in the last match `run` made.
    pub fn capture(&self, name: &str, run: &Run) -> Option<(usize, usize)> {
        let index = self.names.iter().position(|known| known == name)? as u32;
        run.captures.iter().rev().find(|capture| capture.0 == index).map(|&(_, start, end)| (start, end))
    }

    /// Where the node ends if it matches at `at`. A node that fails has left
    /// nothing behind.
    fn step(&self, node: u32, hay: &[u8], at: usize, run: &mut Run) -> Option<usize> {
        let (literal, captured) = (run.literal, run.captures.len());
        let ended = match &self.nodes[node as usize] {
            Node::Literal(bytes) => hay[at..].starts_with(bytes).then(|| {
                run.literal += bytes.len();
                at + bytes.len()
            }),
            Node::Class(class) => class.step(hay, at),
            Node::Sequence(children) => children.iter().try_fold(at, |at, &child| self.step(child, hay, at, run)),
            Node::Choice(alternatives) => alternatives.iter().find_map(|&alternative| self.step(alternative, hay, at, run)),
            Node::Repeat { body, min, max } => {
                let (mut end, mut count) = (at, 0);
                while count < *max {
                    match self.step(*body, hay, end, run) {
                        // A body that matches nothing would go on forever.
                        Some(next) if next > end => (end, count) = (next, count + 1),
                        _ => break,
                    }
                }
                (count >= *min).then_some(end)
            }
            Node::Capture { name, body } => self.step(*body, hay, at, run).inspect(|&end| run.captures.push((*name, at, end))),
        };
        if ended.is_none() {
            run.literal = literal;
            run.captures.truncate(captured);
        }
        ended
    }
}

/// The text of a pattern, read into nodes.
struct Parser<'t> {
    text: &'t str,
    at: usize,
    nodes: Vec<Node>,
    names: Vec<String>,
}

impl Parser<'_> {
    fn error(&self, len: usize, message: impl Into<String>) -> PatternError {
        PatternError { at: self.at, len, message: message.into(), help: None }
    }

    fn skip_space(&mut self) {
        self.at += self.text[self.at..].len() - self.text[self.at..].trim_start().len();
    }

    fn peek(&mut self) -> Option<char> {
        self.skip_space();
        self.text[self.at..].chars().next()
    }

    fn push(&mut self, node: Node) -> u32 {
        self.nodes.push(node);
        self.nodes.len() as u32 - 1
    }

    /// One node for several: the node itself for one, else a node that has them.
    fn all_of(&mut self, mut nodes: Vec<u32>, make: fn(Box<[u32]>) -> Node) -> u32 {
        match nodes.pop() {
            Some(only) if nodes.is_empty() => only,
            Some(last) => {
                nodes.push(last);
                self.push(make(nodes.into()))
            }
            None => unreachable!("a choice and a sequence have at least one part"),
        }
    }

    fn choice(&mut self) -> Result<u32, PatternError> {
        let mut alternatives = vec![self.sequence()?];
        while self.peek() == Some('/') {
            self.at += 1;
            alternatives.push(self.sequence()?);
        }
        Ok(self.all_of(alternatives, Node::Choice))
    }

    fn sequence(&mut self) -> Result<u32, PatternError> {
        let mut items = vec![self.item()?];
        while !matches!(self.peek(), None | Some('/' | ')')) {
            items.push(self.item()?);
        }
        Ok(self.all_of(items, Node::Sequence))
    }

    /// An atom, with the name it captures under and the repeat that follows it.
    fn item(&mut self) -> Result<u32, PatternError> {
        self.skip_space();
        let word = self.text[self.at..].split(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_')).next().unwrap_or("");
        let name = self.text[self.at + word.len()..].starts_with(':').then_some(word).filter(|word| !word.is_empty());
        if let Some(name) = name {
            self.at += name.len() + 1;
        }
        let mut node = self.atom()?;
        let repeat = match self.text[self.at..].chars().next() {
            Some('*') => Some((0, u32::MAX)),
            Some('+') => Some((1, u32::MAX)),
            Some('?') => Some((0, 1)),
            _ => None,
        };
        if let Some((min, max)) = repeat {
            self.at += 1;
            node = self.push(Node::Repeat { body: node, min, max });
        }
        let Some(name) = name else { return Ok(node) };
        let index = self.names.iter().position(|known| known == name).unwrap_or_else(|| {
            self.names.push(name.to_string());
            self.names.len() - 1
        });
        Ok(self.push(Node::Capture { name: index as u32, body: node }))
    }

    fn atom(&mut self) -> Result<u32, PatternError> {
        match self.peek() {
            Some('"') => self.literal(),
            Some('(') => {
                self.at += 1;
                let inner = self.choice()?;
                match self.peek() {
                    Some(')') => {
                        self.at += 1;
                        Ok(inner)
                    }
                    _ => Err(self.error(1, "this group is never closed")),
                }
            }
            Some(c) if c.is_ascii_alphabetic() => self.class(),
            Some(c) => Err(self.error(c.len_utf8(), format!("expected a literal, a class or `(`, not `{c}`"))),
            None => Err(self.error(0, "the pattern ends where something was expected")),
        }
    }

    /// `"TEXT"`, where `\"` and `\\` are a quote and a backslash.
    fn literal(&mut self) -> Result<u32, PatternError> {
        let start = self.at;
        let mut bytes = Vec::new();
        let mut chars = self.text[start + 1..].char_indices();
        while let Some((offset, c)) = chars.next() {
            match c {
                '"' if bytes.is_empty() => {
                    self.at = start;
                    return Err(self.error(offset + 2, "a literal cannot be empty"));
                }
                '"' => {
                    self.at = start + offset + 2;
                    return Ok(self.push(Node::Literal(bytes.to_ascii_lowercase().into())));
                }
                '\\' => match chars.next() {
                    Some((_, escaped @ ('"' | '\\'))) => bytes.extend_from_slice(escaped.to_string().as_bytes()),
                    _ => {
                        self.at = start + offset + 1;
                        return Err(self.error(2, "a backslash in a literal is followed by `\"` or `\\`"));
                    }
                },
                c => bytes.extend_from_slice(c.to_string().as_bytes()),
            }
        }
        self.at = start;
        Err(self.error(1, "this literal is never closed"))
    }

    fn class(&mut self) -> Result<u32, PatternError> {
        let word = self.text[self.at..].split(|c: char| !c.is_ascii_alphabetic()).next().unwrap_or("");
        let Some(&(_, class)) = CLASSES.iter().find(|(name, _)| *name == word) else {
            let mut error = self.error(word.len(), format!("`{word}` is not a class"));
            let names = CLASSES.iter().map(|(name, _)| *name);
            error.help = Some(match closest(word, names.clone()) {
                Some(near) => format!("did you mean `{near}`? The classes are digit, letter, space and any"),
                None => "the classes are digit, letter, space and any; write text in quotes".to_string(),
            });
            return Err(error);
        };
        self.at += word.len();
        Ok(self.push(Node::Class(class)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The matched text, and each capture, when `pattern` is searched for in `memo`.
    fn find(pattern: &str, memo: &str) -> Option<(String, usize, Vec<(String, String)>)> {
        let peg = Peg::new(pattern).unwrap_or_else(|error| panic!("{pattern}: {}", error.message));
        let hay = memo.to_ascii_lowercase().into_bytes();
        let mut run = Run::default();
        let found = peg.find(&hay, 0, &mut run)?;
        let text = |(start, end): (usize, usize)| String::from_utf8_lossy(&hay[start..end]).into_owned();
        let captures = peg
            .names
            .iter()
            .filter_map(|name| Some((name.clone(), text(peg.capture(name, &run)?))))
            .collect();
        Some((text((found.start, found.end)), found.literal, captures))
    }

    fn matched(pattern: &str, memo: &str) -> Option<String> {
        find(pattern, memo).map(|(text, ..)| text)
    }

    #[test]
    fn literals_sequences_and_classes() {
        assert_eq!(matched("\"trader joe\"", "POS TRADER JOE'S #634").as_deref(), Some("trader joe"));
        assert_eq!(matched("\"INV-\" digit+ \"-\" digit+", "PAYMENT INV-2026-01 THANKS").as_deref(), Some("inv-2026-01"));
        assert_eq!(matched("\"inv-\" digit+", "inv-x"), None);
        assert_eq!(matched("\"sq *bl\" letter \"e bottle\"", "SQ *BLUE BOTTLE 12").as_deref(), Some("sq *blue bottle"));
        assert_eq!(matched("\"a\" space* \"b\"", "A  \t B").as_deref(), Some("a  \t b"));
        assert_eq!(matched("\"caf\" any \"!\"", "CAFÉ!").as_deref(), Some("cafÉ!"), "case folds ASCII only");
        assert_eq!(matched("\"a\" \"b\"", "xab").as_deref(), Some("ab"), "a search, not an anchor");
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
        let (text, literal, captures) = find("\"PAYPAL *\" payee:(any+)", "paypal *etsy seller").unwrap();
        assert_eq!((text.as_str(), literal), ("paypal *etsy seller", 8));
        assert_eq!(captures, [("payee".to_string(), "etsy seller".to_string())]);
        let (_, literal, captures) = find("code:(\"inv-\" digit+ \"-\" digit+) / \"x\"", "see inv-12-3").unwrap();
        assert_eq!((literal, captures[0].1.as_str()), (5, "inv-12-3"));
        let (_, _, captures) = find("(first:digit \"x\" / \"5\") second:digit", "55").unwrap();
        assert_eq!(captures, [("second".to_string(), "5".to_string())], "a failed branch leaves no capture behind");
    }

    #[test]
    fn a_pattern_says_which_literals_a_match_begins_with() {
        let starts = |pattern: &str| {
            let peg = Peg::new(pattern).unwrap();
            peg.starts().map(|starts| starts.iter().map(|start| String::from_utf8(start.clone()).unwrap()).collect::<Vec<_>>())
        };
        assert_eq!(starts("\"PAYPAL\" \" *\" payee:(any+)"), Some(vec!["paypal".to_string()]));
        assert_eq!(starts("code:(\"inv-\" digit+)"), Some(vec!["inv-".to_string()]));
        assert_eq!(starts("\"a\" digit / (\"b\" / \"c\")+ \"d\""), Some(vec!["a".to_string(), "b".to_string(), "c".to_string()]));
        assert_eq!(starts("digit+ \"-\""), None);
        assert_eq!(starts("\"a\" / digit"), None, "one way to begin with anything is enough");
        assert_eq!(starts("(\"a\"?)  \"b\""), None, "an optional start may be skipped");
        // Found by the one literal there is.
        let (hay, mut run) = (b"xx inv-12 inv-34".to_vec(), Run::default());
        let peg = Peg::new("code:(\"inv-\" digit+)").unwrap();
        assert_eq!(peg.find(&hay, 0, &mut run).map(|found| found.start), Some(3));
        assert_eq!(peg.find(&hay, 4, &mut run).map(|found| found.start), Some(10));
    }

    #[test]
    fn a_bad_pattern_says_where() {
        let bad = |pattern: &str| Peg::new(pattern).err().unwrap_or_else(|| panic!("{pattern} is fine"));
        let error = bad("\"a\" digt+");
        assert_eq!((error.at, error.len, error.message.as_str()), (4, 4, "`digt` is not a class"));
        assert!(error.help.unwrap().starts_with("did you mean `digit`?"));
        assert_eq!(bad("\"never").message, "this literal is never closed");
        assert_eq!(bad("\"\"").message, "a literal cannot be empty");
        assert_eq!(bad("(\"a\"").message, "this group is never closed");
        assert_eq!(bad("\"a\")").message, "there is nothing to close here");
        assert_eq!(bad("\"a\" /").message, "the pattern ends where something was expected");
        assert_eq!(bad("\"a\" $").message, "expected a literal, a class or `(`, not `$`");
        assert_eq!(bad("").message, "the pattern ends where something was expected");
        let at = Loc::new(axiom_core::FileId(2), 100, 110);
        let diagnostic = bad("\"a\" digt+").diagnostic(at);
        assert_eq!(diagnostic.anchor().map(|loc| (loc.file, loc.start, loc.end)), Some((axiom_core::FileId(2), 104, 108)));
    }
}
