//! Who a memo is. The `known-as` patterns are compiled once and looked up by
//! first byte, so a record costs one pass over its memo and no allocation.

use std::cmp::Reverse;

use axiom_core::glob::glob;
use axiom_core::par;
use memchr::memmem;

use crate::Record;

/// Records handed to one worker at a time: enough to reuse a scratch buffer.
const CHUNK: usize = 4096;

/// An entity or an account, and how it appears on statements.
pub struct Known<'a> {
    pub name: &'a str,
    /// An account rather than a party: money with it is a transfer.
    pub account: bool,
    /// `known-as` globs, in any case. `*` is any run of text and `?` one
    /// character; a pattern may match anywhere in the memo.
    pub patterns: Vec<&'a str>,
}

/// The other end of a record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Who<'a> {
    pub name: &'a str,
    pub account: bool,
}

/// What a memo names. Nobody when nothing is known as any of it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Recognized<'a> {
    pub who: Option<Who<'a>>,
    /// The intermediary the money passed through (`PAYPAL *ETSY SELLER` is
    /// etsy-seller via paypal).
    pub via: Option<&'a str>,
}

/// Two patterns of different entities that match equally well.
#[derive(Clone, Copy, Debug)]
pub struct Tie<'a> {
    pub first: (Who<'a>, &'a str),
    pub second: (Who<'a>, &'a str),
}

/// A memo, read: who it is, and the codes it names, lowercased.
pub struct Reading<'a> {
    pub who: Result<Recognized<'a>, Tie<'a>>,
    pub codes: Vec<String>,
}

/// One `known-as` glob, taken apart.
struct Pattern<'a> {
    owner: usize,
    source: &'a str,
    /// The text between the `*`s, lowercased. A `?` in it stands for a character.
    pieces: Vec<Vec<u8>>,
    /// How many characters are not wildcards: what "the longest match wins" counts.
    literal: usize,
}

impl Pattern<'_> {
    /// Where the match ends, if the pattern's first piece stands at `at`.
    fn matches_at(&self, memo: &[u8], at: usize) -> Option<usize> {
        let (first, later) = self.pieces.split_first()?;
        later.iter().try_fold(piece_at(first, memo, at)?, |end, piece| find_piece(piece, memo, end))
    }
}

/// Where `piece` ends, if it stands at `at`.
fn piece_at(piece: &[u8], memo: &[u8], at: usize) -> Option<usize> {
    let mut end = at;
    for &wanted in piece {
        let &found = memo.get(end)?;
        end += match wanted {
            b'?' => char_width(found),
            _ if wanted == found => 1,
            _ => return None,
        };
    }
    (end <= memo.len()).then_some(end)
}

/// Where `piece` ends, at its first place at or after `from`.
fn find_piece(piece: &[u8], memo: &[u8], from: usize) -> Option<usize> {
    if piece.contains(&b'?') {
        return (from..memo.len()).find_map(|at| piece_at(piece, memo, at));
    }
    memmem::find(&memo[from..], piece).map(|at| from + at + piece.len())
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

struct Hit {
    pattern: usize,
    start: usize,
    end: usize,
}

/// Room for reading one memo, kept between memos.
#[derive(Default)]
pub struct Scratch {
    lower: Vec<u8>,
    hits: Vec<Hit>,
}

pub struct Recognizer<'a> {
    known: Vec<Known<'a>>,
    patterns: Vec<Pattern<'a>>,
    /// For each byte, the patterns whose first piece can start with it.
    starts: Vec<Vec<usize>>,
    /// The declared `code` globs, lowercased.
    codes: Vec<String>,
}

impl<'a> Recognizer<'a> {
    pub fn new(known: Vec<Known<'a>>, codes: &[&str]) -> Recognizer<'a> {
        let mut patterns = Vec::new();
        for (owner, entry) in known.iter().enumerate() {
            for &source in &entry.patterns {
                let pieces: Vec<Vec<u8>> = source
                    .to_ascii_lowercase()
                    .split('*')
                    .filter(|piece| !piece.is_empty())
                    .map(|piece| piece.as_bytes().to_vec())
                    .collect();
                let literal = pieces.iter().flatten().filter(|&&byte| byte != b'?').count();
                // A pattern of only wildcards would recognize everything as everyone.
                if literal > 0 {
                    patterns.push(Pattern { owner, source, pieces, literal });
                }
            }
        }
        let mut starts = vec![Vec::new(); 256];
        for (at, pattern) in patterns.iter().enumerate() {
            match pattern.pieces[0][0] {
                b'?' => starts.iter_mut().for_each(|bucket| bucket.push(at)),
                byte => starts[byte as usize].push(at),
            }
        }
        let codes = codes.iter().map(|code| code.to_ascii_lowercase()).collect();
        Recognizer { known, patterns, starts, codes }
    }

    /// Reads every record's memo, in parallel.
    pub fn read_all(&self, records: &[Record]) -> Vec<Reading<'a>> {
        let chunks: Vec<&[Record]> = records.chunks(CHUNK).collect();
        let read = |chunk: &&[Record]| {
            let mut scratch = Scratch::default();
            chunk.iter().map(|record| self.read(&record.memo, &mut scratch)).collect::<Vec<_>>()
        };
        par::map_each(&chunks, read).into_iter().flatten().collect()
    }

    pub fn read(&self, memo: &str, scratch: &mut Scratch) -> Reading<'a> {
        let Scratch { lower, hits } = scratch;
        lower.clear();
        lower.extend(memo.bytes().map(|byte| byte.to_ascii_lowercase()));
        hits.clear();
        for (at, &byte) in lower.iter().enumerate() {
            for &pattern in &self.starts[byte as usize] {
                if let Some(end) = self.patterns[pattern].matches_at(lower, at) {
                    hits.push(Hit { pattern, start: at, end });
                }
            }
        }
        Reading { who: self.decide(hits, lower), codes: self.codes_in(lower) }
    }

    fn owner(&self, hit: &Hit) -> usize {
        self.patterns[hit.pattern].owner
    }

    fn literal(&self, hit: &Hit) -> usize {
        self.patterns[hit.pattern].literal
    }

    fn who(&self, owner: usize) -> Who<'a> {
        Who { name: self.known[owner].name, account: self.known[owner].account }
    }

    /// The best-matching entity: the longest literal wins, and two that tie
    /// are an error. A party found inside another's (after only punctuation)
    /// is who it was for, and the other the way it went.
    fn decide(&self, hits: &mut Vec<Hit>, lower: &[u8]) -> Result<Recognized<'a>, Tie<'a>> {
        hits.sort_unstable_by_key(|hit| (self.owner(hit), Reverse(self.literal(hit)), hit.start));
        hits.dedup_by_key(|hit| self.owner(hit));
        if let Some((outer, inner)) = self.enclosed(hits, lower) {
            let (who, via) = (self.who(self.owner(inner)), self.known[self.owner(outer)].name);
            return Ok(Recognized { who: Some(who), via: Some(via) });
        }
        hits.sort_unstable_by_key(|hit| (Reverse(self.literal(hit)), hit.start));
        let candidate = |hit: &Hit| (self.who(self.owner(hit)), self.patterns[hit.pattern].source);
        match hits.as_slice() {
            [] => Ok(Recognized::default()),
            [best, next, ..] if self.literal(best) == self.literal(next) => {
                Err(Tie { first: candidate(best), second: candidate(next) })
            }
            [best, ..] => Ok(Recognized { who: Some(self.who(self.owner(best))), via: None }),
        }
    }

    fn enclosed<'h>(&self, hits: &'h [Hit], lower: &[u8]) -> Option<(&'h Hit, &'h Hit)> {
        let party = |hit: &&Hit| !self.known[self.owner(hit)].account;
        let pairs = hits.iter().filter(party).flat_map(|outer| hits.iter().filter(party).map(move |inner| (outer, inner)));
        // Joined by punctuation such as `*`, never by words, or by spaces alone.
        let joined = |outer: &Hit, inner: &Hit| {
            let gap = lower.get(outer.end..inner.start).unwrap_or_default();
            gap.iter().all(|byte| !byte.is_ascii_alphanumeric()) && gap.iter().any(|byte| !byte.is_ascii_whitespace())
        };
        pairs
            .filter(|(outer, inner)| self.owner(outer) != self.owner(inner) && joined(outer, inner))
            .max_by_key(|(outer, inner)| (self.literal(inner), self.literal(outer)))
    }

    /// The words of the memo that a declared `code` glob matches, in order.
    fn codes_in(&self, lower: &[u8]) -> Vec<String> {
        if self.codes.is_empty() {
            return Vec::new();
        }
        const PUNCTUATION: &[u8] = b"_:./-";
        let in_code = |byte: &u8| byte.is_ascii_lowercase() || byte.is_ascii_digit() || PUNCTUATION.contains(byte);
        let words = lower.split(|byte| !in_code(byte));
        let trimmed = words.map(|word| {
            let end = word.iter().rposition(|byte| !PUNCTUATION.contains(byte));
            &word[..end.map_or(0, |last| last + 1)]
        });
        let names = trimmed.filter(|word| word.first().is_some_and(u8::is_ascii_alphanumeric));
        let texts = names.filter_map(|word| std::str::from_utf8(word).ok());
        texts.filter(|text| self.codes.iter().any(|code| glob(code, text))).map(String::from).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn party<'a>(name: &'a str, patterns: &[&'a str]) -> Known<'a> {
        Known { name, account: false, patterns: patterns.to_vec() }
    }

    fn recognizer() -> Recognizer<'static> {
        let known = vec![
            party("trader-joes", &["TRADER JOE*"]),
            party("shell", &["SHELL"]),
            party("shell-oil", &["SHELL OIL*"]),
            party("paypal", &["PAYPAL"]),
            party("etsy-seller", &["ETSY SELLER*"]),
            party("uber", &["UBER"]),
            party("halcyon", &["HALCYON*"]),
            party("blue-bottle", &["SQ *BL?E BOTTLE*"]),
            party("dup-a", &["SAME"]),
            party("dup-b", &["*same*"]),
            Known { name: "visa", account: true, patterns: vec!["CHASE CARD PAYMENT"] },
        ];
        Recognizer::new(known, &["inv-*", "CHECK-????"])
    }

    fn who(memo: &str) -> (Option<&'static str>, Option<&'static str>) {
        let reading = recognizer().read(memo, &mut Scratch::default());
        let found = reading.who.unwrap_or_else(|tie| panic!("{memo}: a tie between {:?} and {:?}", tie.first.0, tie.second.0));
        (found.who.map(|who| who.name), found.via)
    }

    #[test]
    fn a_memo_is_recognized_in_any_case_and_anywhere() {
        assert_eq!(who("TRADER JOE'S #634 SAN FRANCISCO CA"), (Some("trader-joes"), None));
        assert_eq!(who("pos debit trader joe's"), (Some("trader-joes"), None));
        assert_eq!(who("SQ *BLUE BOTTLE 1234"), (Some("blue-bottle"), None));
        assert_eq!(who("SQ *BLAE BOTTLE 1234"), (Some("blue-bottle"), None));
        assert_eq!(who("SQ *BLUUE BOTTLE"), (None, None), "`?` is one character");
        assert_eq!(who("SOMETHING ELSE"), (None, None));
        assert_eq!(who("CAFÉ ☕ SHELL"), (Some("shell"), None));
        assert_eq!(who(""), (None, None));
    }

    #[test]
    fn the_longest_literal_wins_and_a_tie_is_an_error() {
        assert_eq!(who("SHELL 1234"), (Some("shell"), None));
        assert_eq!(who("SHELL OIL 1234"), (Some("shell-oil"), None));
        let reading = recognizer().read("SAME THING", &mut Scratch::default());
        let tie = reading.who.err().expect("a tie");
        let names = [tie.first.0.name, tie.second.0.name];
        assert!(names.contains(&"dup-a") && names.contains(&"dup-b"), "{names:?}");
    }

    #[test]
    fn a_party_inside_another_is_who_it_was_for() {
        assert_eq!(who("PAYPAL *ETSY SELLER"), (Some("etsy-seller"), Some("paypal")));
        assert_eq!(who("PAYPAL *UBER"), (Some("uber"), Some("paypal")), "whichever is longer");
        assert_eq!(who("PAYPAL TRANSFER"), (Some("paypal"), None));
        assert_eq!(who("PAYPAL ETSY SELLER"), (Some("etsy-seller"), None), "words alone relate nothing");
        assert_eq!(who("UBER  PAYPAL"), (Some("paypal"), None));
    }

    #[test]
    fn an_account_is_the_other_end_of_a_transfer() {
        let reading = recognizer().read("CHASE CARD PAYMENT 0105", &mut Scratch::default());
        let who = reading.who.ok().and_then(|found| found.who).expect("recognized");
        assert_eq!((who.name, who.account), ("visa", true));
    }

    #[test]
    fn codes_are_found_by_the_declared_globs_and_lowercased() {
        let codes = |memo: &str| recognizer().read(memo, &mut Scratch::default()).codes;
        assert_eq!(codes("HALCYON PAYMENT INV-2026-01 THANKS"), ["inv-2026-01"]);
        assert_eq!(codes("payment for inv-2026-01."), ["inv-2026-01"]);
        assert_eq!(codes("INV-1 AND INV-2, CHECK-1041"), ["inv-1", "inv-2", "check-1041"]);
        assert!(codes("CHECK-12 INVOICE 2026").is_empty());
    }

    #[test]
    fn reading_in_parallel_is_reading_one_by_one() {
        let memos = ["TRADER JOE'S", "SHELL OIL", "PAYPAL *UBER", "nothing", "INV-9"];
        let records: Vec<Record> = (0..10_000)
            .map(|at| Record {
                day: axiom_core::Day(0),
                qty: axiom_core::Qty(0),
                memo: memos[at % memos.len()].into(),
                balance: None,
                pending: false,
                at: axiom_core::Loc::default(),
            })
            .collect();
        let recognizer = recognizer();
        let read = recognizer.read_all(&records);
        let mut scratch = Scratch::default();
        for (record, reading) in records.iter().zip(&read) {
            let alone = recognizer.read(&record.memo, &mut scratch);
            assert_eq!(alone.who.ok(), reading.who.ok());
            assert_eq!(alone.codes, reading.codes);
        }
    }
}
