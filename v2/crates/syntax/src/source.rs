//! What feeds a book (LANGUAGE §14): `sync` sources, the `format`s of their
//! records, and the `pattern`s that recognize memos.
//!
//! A command and a path are raw text (a command has anything in it, a path has
//! dots and braces), and so is a format line, whose words are the source's own
//! names (`BookgDt/Dt`, `CdtDbtInd`), so these lines are read by splitting the
//! line, not by the lexer. A pattern is read by the lexer, as its atoms are
//! strings and names.

use axiom_core::diag::closest;
use axiom_core::{Diagnostic, Loc};

use crate::ast::*;
use crate::lex::{Malformed, Punct, Tok};
use crate::lines::Line;
use crate::malformed::diagnose;
use crate::parser::{Parse, Parser, Reported};

/// How deeply patterns may group. Far beyond what a memo needs; the limit
/// keeps a hostile file from overflowing the stack.
const MAX_GROUPS: u32 = 100;

/// The lines a sync may have.
const SYNC_WORDS: [&str; 4] = ["read", "run", "into", "format"];

/// What a sync's lines have said so far, each with where.
#[derive(Default)]
struct Found<'s> {
    read: Option<(Text<'s>, Loc)>,
    run: Option<(Text<'s>, Loc)>,
    into: Option<(Text<'s>, Loc)>,
    format: Option<(Ref<Format<'s>>, Loc)>,
}

impl<'s> Parser<'s> {
    // ─── Sync ───────────────────────────────────────────────────────────────

    /// `sync NAME` with `read STRING` and `run COMMAND…` lines (one at least),
    /// perhaps `into PATH`, and perhaps `format NAME` with the lines that
    /// declare it.
    pub fn sync(&mut self, line: &mut Line<'s>) -> Parse<()> {
        let name = self.name("expected-name", "what it feeds, such as `checking`")?;
        if self.at(Punct::Dot) && self.peek().loc.start == self.lexer.prev_end() {
            return self.fail(self.sync_file(name));
        }
        let header = self.end_header(line)?;
        let mut found = Found::default();
        self.children(line, |parser, child| parser.sync_line(child, &mut found))?;
        if found.read.is_none() && found.run.is_none() {
            return self.fail(missing_source(header.loc));
        }
        let text = |line: Option<(Text<'s>, Loc)>| line.map(|(text, _)| text);
        let sync = Sync {
            name,
            read: text(found.read),
            run: text(found.run),
            into: text(found.into),
            format: found.format.map(|(format, _)| format),
        };
        self.emit(&header, sync, ItemKind::Sync);
        Ok(())
    }

    /// One line of a sync.
    fn sync_line(&mut self, line: &mut Line<'s>, found: &mut Found<'s>) -> Parse<()> {
        let keyword = self.peek();
        let Tok::Name(word) = keyword.tok else {
            return Err(self.expected("expected-sync-line", "a line of a sync: `read`, `run`, `into` or `format`"));
        };
        match word {
            "read" => {
                self.bump();
                let Tok::Str(text) = self.tok() else {
                    return Err(self.expected("expected-string", "the files to read, a glob in quotes: `\"imports/*.csv\"`"));
                };
                self.bump();
                self.expect_eol()?;
                self.once(&mut found.read, "read", Text(text), self.loc_from(line.body))
            }
            "run" => self.raw_line(&mut found.run, "run", "expected-command", "the command to run"),
            "into" => self.raw_line(&mut found.into, "into", "expected-path", "where to write, like `prices/{year}.ax`"),
            "format" => {
                self.bump();
                let name = self.name("expected-name", "the format's name, such as `csv`")?;
                self.expect_eol()?;
                let at = self.loc_from(line.body);
                let format = self.format_body(name, line);
                let format = self.push(format);
                self.once(&mut found.format, "format", format, at)
            }
            _ => {
                let diag = Diagnostic::error("unknown-sync-line", format!("a sync has no `{word}` line"))
                    .label(keyword.loc, "not a line of a sync");
                self.fail(match closest(word, SYNC_WORDS) {
                    Some(near) => diag.fix(format!("did you mean `{near}`?"), keyword.loc, near),
                    None => diag.note("a sync's lines are `read`, `run`, `into` and `format`"),
                })
            }
        }
    }

    /// Records the line `what` of a sync, which may be written once.
    fn once<T>(&mut self, slot: &mut Option<(T, Loc)>, what: &str, value: T, at: Loc) -> Parse<()> {
        match slot.replace((value, at)) {
            Some((_, first)) => Err(self.duplicate(&format!("`{what}` line"), at, first)),
            None => Ok(()),
        }
    }

    /// A sync line that is a word and the raw text after it.
    fn raw_line(&mut self, slot: &mut Option<(Text<'s>, Loc)>, word: &str, code: &'static str, what: &str) -> Parse<()> {
        let keyword = self.bump().loc;
        let Some(text) = self.lexer.raw_rest() else { return Err(self.expected(code, what)) };
        let at = keyword.to(self.loc_of(&text));
        self.once(slot, word, Text(text.0), at)
    }

    /// v3's `sync prices/2026.ax`, which named the file it wrote. A sync is
    /// named for what it feeds now, and says where it writes with `into`.
    fn sync_file(&self, name: Name<'s>) -> Diagnostic {
        let start = self.loc_of(&name);
        let len = self.src[start.start as usize..].find([' ', '\t', '\r', '\n']).unwrap_or(self.src.len() - start.start as usize);
        let file = Loc::new(self.id, start.start, start.start + len as u32);
        let written = self.text(file);
        let feeds = name.split('/').next().unwrap_or(&name);
        Diagnostic::error("sync-file", format!("a sync is named for what it feeds, not for the file `{written}` it writes"))
            .label(file, "a file is `into`, on a line of its own")
            .note("`sync prices` names the source, and its lines say `run` a command and `into` a file")
            .fix(format!("name it `{feeds}` and say where it writes"), file, format!("{feeds}\n  into {written}"))
    }

    // ─── Formats ────────────────────────────────────────────────────────────

    /// `format NAME` and the lines that say what each field of a record is.
    pub fn format(&mut self, line: &mut Line<'s>) -> Parse<()> {
        let name = self.name("expected-name", "the format's name, such as `csv`")?;
        let header = self.end_header(line)?;
        let format = self.format_body(name, line);
        self.emit(&header, format, ItemKind::Format);
        Ok(())
    }

    /// The lines under a `format NAME` line, which keeps its good lines when
    /// one is bad, as a declaration does.
    fn format_body(&mut self, name: Name<'s>, parent: &Line<'s>) -> Format<'s> {
        let mark = self.mark::<FormatLine>();
        let _ = self.children(parent, |parser, child| parser.format_line(child));
        Format { name, lines: self.since(mark) }
    }

    /// `KEY ARG*`, where an argument is a word as written or a string.
    fn format_line(&mut self, line: &Line<'s>) -> Parse<()> {
        let Some(text) = self.lexer.raw_rest() else {
            return Err(self.expected("expected-format-line", "a field and where it is found: `date \"Posting Date\"`"));
        };
        let whole = self.loc_of(&text);
        let words = match format_words(text.0) {
            Ok(words) => words,
            Err(quote) => {
                let from = whole.start + quote as u32;
                return self.fail(diagnose(Malformed::UnterminatedString, Loc::new(self.id, from, whole.end), ""));
            }
        };
        let mut words = words.into_iter();
        let key = match words.next() {
            Some(FormatArg::Word(word)) if is_key(word.0) => Name(word.0),
            _ => {
                let diag = Diagnostic::error("expected-format-key", "a format line starts with what the field is")
                    .label(whole, "expected a word such as `date`, `amount` or `memo`");
                return self.fail(diag);
            }
        };
        let mark = self.mark::<FormatArg>();
        for arg in words {
            self.push(arg);
        }
        let args = self.since(mark);
        self.push(FormatLine { key, args, loc: self.loc_from(line.body) });
        Ok(())
    }

    // ─── Patterns ───────────────────────────────────────────────────────────

    /// `pattern NAME = PATTERN`
    pub fn named_pattern(&mut self, line: &mut Line<'s>) -> Parse<()> {
        let name = self.name("expected-name", "the pattern's name, such as `ach`")?;
        self.expect(Punct::Eq, "expected-equals", "`=` and the pattern")?;
        let pattern = self.pattern()?;
        let header = self.end_header(line)?;
        self.emit(&header, NamedPattern { name, pattern }, ItemKind::Pattern);
        Ok(())
    }

    /// The rest of a `known-as` line, after the keyword: `PATTERN, …`.
    pub fn known_as(&mut self) -> Parse<()> {
        self.bump();
        loop {
            let pattern = self.pattern()?;
            self.push(pattern);
            if self.eat(Punct::Comma).is_none() {
                return self.expect_eol();
            }
        }
    }

    /// `PATTERN := SEQ ( / SEQ )*`: an ordered choice.
    ///
    /// A sequence's terms, and a pattern's sequences, are each one run of their
    /// table, so a nested group is read before its outer run is added: the
    /// runs are held here until then.
    pub fn pattern(&mut self) -> Parse<Pattern<'s>> {
        if self.depth == MAX_GROUPS {
            return self.fail(too_deep(self.peek().loc));
        }
        self.depth += 1;
        let choices = self.choices();
        self.depth -= 1;
        let choices = choices?;
        let mark = self.mark::<Sequence>();
        for choice in choices {
            self.push(choice);
        }
        Ok(Pattern { choices: self.since(mark) })
    }

    fn choices(&mut self) -> Parse<Vec<Sequence<'s>>> {
        let mut choices = Vec::new();
        loop {
            let mut terms = Vec::new();
            loop {
                terms.push(self.pattern_term()?);
                if matches!(self.tok(), Tok::Eol | Tok::Punct(Punct::Slash | Punct::RParen | Punct::Comma)) {
                    break;
                }
            }
            let mark = self.mark::<PatternTerm>();
            for term in terms {
                self.push(term);
            }
            choices.push(Sequence { terms: self.since(mark) });
            if self.eat(Punct::Slash).is_none() {
                return Ok(choices);
            }
        }
    }

    /// `[NAME:] ATOM [? | * | +]`
    fn pattern_term(&mut self) -> Parse<PatternTerm<'s>> {
        let capture = match (self.tok(), self.lexer.peek_second().tok) {
            (Tok::Name(name), Tok::Punct(Punct::Colon)) => {
                self.bump();
                self.bump();
                Some(Name(name))
            }
            _ => None,
        };
        let token = self.peek();
        let (atom, mut repeat) = match token.tok {
            Tok::Str(text) => (PatternAtom::Literal(Text(self.bump_as(text))), Repeat::One),
            Tok::Name(word) => self.pattern_word(word)?,
            Tok::Punct(Punct::LParen) => {
                self.bump();
                let inner = self.pattern()?;
                self.close(token.loc, Punct::RParen)?;
                (PatternAtom::Group(inner), Repeat::One)
            }
            _ => return Err(self.pattern_expected()),
        };
        if repeat == Repeat::One {
            repeat = match self.tok() {
                Tok::Punct(Punct::Question) => Repeat::Optional,
                Tok::Punct(Punct::Star) => Repeat::Many,
                Tok::Punct(Punct::Plus) => Repeat::Some,
                _ => Repeat::One,
            };
            if repeat != Repeat::One {
                self.bump();
            }
        }
        Ok(PatternTerm { capture, atom, repeat })
    }

    /// A class or the name of a pattern, and the repeat glued to it: the
    /// lexer reads `digit*` and `space?` as one word.
    fn pattern_word(&mut self, word: &'s str) -> Parse<(PatternAtom<'s>, Repeat)> {
        let token = self.bump();
        let (base, repeat) = match word.as_bytes().last() {
            Some(b'?') => (&word[..word.len() - 1], Repeat::Optional),
            Some(b'*') => (&word[..word.len() - 1], Repeat::Many),
            _ => (word, Repeat::One),
        };
        if base.is_empty() || base.contains(['?', '*']) {
            let diag = Diagnostic::error("bad-pattern-word", format!("`{word}` is not a class or a pattern's name"))
                .label(token.loc, "a name has no `*` or `?` inside");
            return self.fail(diag);
        }
        if base.contains('/') {
            let spaced = base.replace('/', " / ");
            let diag = Diagnostic::error("pattern-slash", "a choice is written with a blank on each side of `/`")
                .label(token.loc, "this reads as one name")
                .fix("write the choice apart", Loc::new(self.id, token.loc.start, token.loc.start + base.len() as u32), spaced);
            return self.fail(diag);
        }
        let atom = match Class::WORDS.iter().find(|(known, _)| *known == base) {
            Some(&(_, class)) => PatternAtom::Class(class),
            None => PatternAtom::Named(Name(base)),
        };
        Ok((atom, repeat))
    }

    fn pattern_expected(&mut self) -> Reported {
        let token = self.peek();
        let mut diag = self.unexpected(token, "expected-pattern", "a string, a class such as `digit`, a pattern's name or `(`");
        if let Tok::Unit(_) = token.tok {
            diag = diag.help("text is written in quotes: `\"INV-\"`");
        }
        self.report(diag)
    }
}

/// The words of a format line: runs of anything but blanks and commas, or a
/// string, whose contents are kept as written. `Err` is where an unterminated
/// string starts.
fn format_words(text: &str) -> Result<Vec<FormatArg<'_>>, usize> {
    let bytes = text.as_bytes();
    let (mut words, mut at) = (Vec::new(), 0);
    while at < bytes.len() {
        match bytes[at] {
            b' ' | b'\t' | b',' => at += 1,
            b'"' => {
                let mut end = at + 1;
                while end < bytes.len() && bytes[end] != b'"' {
                    end += if bytes[end] == b'\\' { 2 } else { 1 };
                }
                if end >= bytes.len() {
                    return Err(at);
                }
                words.push(FormatArg::Quoted(Text(&text[at + 1..end])));
                at = end + 1;
            }
            _ => {
                let len = bytes[at..].iter().position(|b| matches!(b, b' ' | b'\t' | b',')).unwrap_or(bytes.len() - at);
                words.push(FormatArg::Word(Text(&text[at..at + len])));
                at += len;
            }
        }
    }
    Ok(words)
}

/// Whether `word` can name what a format line says: `date`, `amount`, `via`.
fn is_key(word: &str) -> bool {
    word.starts_with(|first: char| first.is_ascii_lowercase())
        && word.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn missing_source(header: Loc) -> Diagnostic {
    Diagnostic::error("missing-source", "this sync says neither what to read nor what to run")
        .label(header, "nothing below says where its records come from")
        .help("add an indented line: `read \"imports/*.csv\"` or `run python3 fetch_prices.py`")
}

fn too_deep(loc: Loc) -> Diagnostic {
    Diagnostic::error("pattern-too-deep", format!("patterns nest at most {MAX_GROUPS} levels"))
        .label(loc, "nested too deeply")
        .help("name an inner part with `pattern NAME = …` and use its name")
}
