//! Who a memo is. The ledger's `known-as` patterns are compiled once and looked
//! up by the first byte they can start with, so a record costs one pass over its
//! memo and no allocation.

use std::cmp::Reverse;

use axiom_core::par;

use crate::Record;
use crate::peg::{Found, PatternError, Peg, Run};

/// Records handed to one worker at a time: enough to reuse a scratch buffer.
const CHUNK: usize = 4096;

/// An entity or an account, and how it appears on statements.
pub struct Known<'a> {
    pub name: &'a str,
    /// An account rather than a party: money with it is a transfer.
    pub account: bool,
    /// `known-as` patterns (see [`Peg`]), each matched anywhere in the memo, in
    /// any case. A pattern with a `payee` capture names a go-between: what the
    /// capture holds is who the money was for (`"PAYPAL *" payee:(any+)`).
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
    /// The go-between the money passed through: `PAYPAL *ETSY SELLER` is
    /// etsy-seller via paypal.
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

/// A pattern that does not compile, and whose it is (`code` for a code).
#[derive(Debug)]
pub struct BadPattern<'a> {
    pub owner: &'a str,
    pub pattern: &'a str,
    pub error: PatternError,
}

struct Pattern<'a> {
    owner: usize,
    source: &'a str,
    peg: Peg,
}

struct Hit {
    pattern: usize,
    found: Found,
    /// Where the `payee` capture was, if the pattern has one.
    payee: Option<(usize, usize)>,
}

/// Room for reading one memo, kept between memos.
#[derive(Default)]
pub struct Scratch {
    lower: Vec<u8>,
    run: Run,
    hits: Vec<Hit>,
}

/// The literals patterns begin with, as a trie. Walking it from a byte of the
/// memo follows the literals that fit, and comes out at the patterns worth
/// trying there: most bytes fit none, and patterns that share a beginning
/// share the walk.
struct Trie {
    /// Node 0 is nowhere; a node's patterns are those whose literal ends there.
    nodes: Vec<TrieNode>,
    /// The node each first byte leads to.
    first: [u32; 256],
}

#[derive(Default)]
struct TrieNode {
    next: Vec<(u8, u32)>,
    patterns: Vec<usize>,
}

impl Trie {
    fn new() -> Trie {
        Trie { nodes: vec![TrieNode::default()], first: [0; 256] }
    }

    fn insert(&mut self, literal: &[u8], pattern: usize) {
        let mut at = 0;
        for (depth, &byte) in literal.iter().enumerate() {
            let existing = match depth {
                0 => Some(self.first[byte as usize]).filter(|&node| node != 0),
                _ => self.nodes[at as usize].next.iter().find(|(next, _)| *next == byte).map(|&(_, node)| node),
            };
            at = existing.unwrap_or_else(|| {
                self.nodes.push(TrieNode::default());
                let node = self.nodes.len() as u32 - 1;
                match depth {
                    0 => self.first[byte as usize] = node,
                    _ => self.nodes[at as usize].next.push((byte, node)),
                }
                node
            });
        }
        self.nodes[at as usize].patterns.push(pattern);
    }

    /// Calls `visit` with each pattern whose literal is at `at` in `hay`.
    fn walk(&self, hay: &[u8], at: usize, mut visit: impl FnMut(usize)) {
        let Some(&lead) = hay.get(at) else { return };
        let (mut node, mut cursor) = (self.first[lead as usize], at + 1);
        while node != 0 {
            self.nodes[node as usize].patterns.iter().for_each(|&pattern| visit(pattern));
            let Some(&byte) = hay.get(cursor) else { return };
            node = self.nodes[node as usize].next.iter().find(|(next, _)| *next == byte).map_or(0, |&(_, node)| node);
            cursor += 1;
        }
    }
}

pub struct Recognizer<'a> {
    known: Vec<Known<'a>>,
    patterns: Vec<Pattern<'a>>,
    starts: Trie,
    /// The patterns that may begin with anything, to be tried everywhere.
    floating: Vec<usize>,
    /// The declared `code` patterns. What one captures as `code`, or else all
    /// it matches, is a code.
    codes: Vec<Peg>,
}

impl<'a> Recognizer<'a> {
    pub fn new(known: Vec<Known<'a>>, codes: &[&'a str]) -> Result<Recognizer<'a>, Vec<BadPattern<'a>>> {
        let (mut patterns, mut compiled, mut bad) = (Vec::new(), Vec::new(), Vec::new());
        let owned = known.iter().enumerate().flat_map(|(owner, entry)| entry.patterns.iter().map(move |&source| (Some(owner), entry.name, source)));
        for (owner, name, source) in owned.chain(codes.iter().map(|&source| (None, "code", source))) {
            match (Peg::new(source), owner) {
                (Ok(peg), Some(owner)) => patterns.push(Pattern { owner, source, peg }),
                (Ok(peg), None) => compiled.push(peg),
                (Err(error), _) => bad.push(BadPattern { owner: name, pattern: source, error }),
            }
        }
        if !bad.is_empty() {
            return Err(bad);
        }
        let (mut starts, mut floating) = (Trie::new(), Vec::new());
        for (at, pattern) in patterns.iter().enumerate() {
            match pattern.peg.starts() {
                Some(literals) => literals.iter().for_each(|literal| starts.insert(literal, at)),
                None => floating.push(at),
            }
        }
        Ok(Recognizer { known, patterns, starts, floating, codes: compiled })
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
        let Scratch { lower, run, hits } = scratch;
        lower.clear();
        lower.extend(memo.bytes().map(|byte| byte.to_ascii_lowercase()));
        self.find_hits(lower, run, hits);
        Reading { who: self.decide(lower, hits), codes: self.codes_in(lower, run) }
    }

    /// Every place a pattern matches in `hay`.
    fn find_hits(&self, hay: &[u8], run: &mut Run, hits: &mut Vec<Hit>) {
        hits.clear();
        for at in 0..hay.len() {
            let mut try_pattern = |pattern: usize| {
                let peg = &self.patterns[pattern].peg;
                if let Some(found) = peg.matches_at(hay, at, run) {
                    hits.push(Hit { pattern, found, payee: peg.capture("payee", run) });
                }
            };
            self.starts.walk(hay, at, &mut try_pattern);
            self.floating.iter().for_each(|&pattern| try_pattern(pattern));
        }
    }

    fn owner(&self, hit: &Hit) -> usize {
        self.patterns[hit.pattern].owner
    }

    fn who(&self, owner: usize) -> Who<'a> {
        Who { name: self.known[owner].name, account: self.known[owner].account }
    }

    /// The best-matching entity: the most literal text matched wins, and two
    /// that tie are an error. What lies inside another party's `payee` is not
    /// in the running: that pattern has said what it is.
    fn decide(&self, hay: &[u8], hits: &mut [Hit]) -> Result<Recognized<'a>, Tie<'a>> {
        hits.sort_unstable_by_key(|hit| (self.owner(hit), Reverse(hit.found.literal), hit.found.start));
        let inside_a_payee = |hit: &Hit| {
            hits.iter().any(|other| {
                let payee = other.payee.filter(|_| !self.known[self.owner(other)].account);
                payee.is_some_and(|(start, end)| self.owner(other) != self.owner(hit) && start <= hit.found.start && hit.found.end <= end)
            })
        };
        let mut best_of_each: Vec<&Hit> = Vec::new();
        for hit in hits.iter().filter(|hit| !inside_a_payee(hit)) {
            if best_of_each.last().is_none_or(|last| self.owner(last) != self.owner(hit)) {
                best_of_each.push(hit);
            }
        }
        best_of_each.sort_by_key(|hit| (Reverse(hit.found.literal), hit.found.start));
        let candidate = |hit: &Hit| (self.who(self.owner(hit)), self.patterns[hit.pattern].source);
        match best_of_each.as_slice() {
            [] => Ok(Recognized::default()),
            [best, next, ..] if best.found.literal == next.found.literal => {
                Err(Tie { first: candidate(best), second: candidate(next) })
            }
            [best, ..] => self.through(best, hay),
        }
    }

    /// The best hit as a party, or, if its pattern names a `payee`, the party
    /// that is, with the best hit as the go-between.
    fn through(&self, best: &Hit, hay: &[u8]) -> Result<Recognized<'a>, Tie<'a>> {
        let outer = self.who(self.owner(best));
        let Some((start, end)) = best.payee.filter(|_| !outer.account) else {
            return Ok(Recognized { who: Some(outer), via: None });
        };
        let (mut run, mut hits) = (Run::default(), Vec::new());
        self.find_hits(&hay[start..end], &mut run, &mut hits);
        Ok(match self.decide(&hay[start..end], &mut hits)?.who {
            Some(inner) => Recognized { who: Some(inner), via: Some(outer.name) },
            None => Recognized { who: Some(outer), via: None },
        })
    }

    /// The codes the memo names, by each pattern in turn.
    fn codes_in(&self, hay: &[u8], run: &mut Run) -> Vec<String> {
        let mut codes = Vec::new();
        for peg in &self.codes {
            let mut from = 0;
            while let Some(found) = peg.find(hay, from, run) {
                let (start, end) = peg.capture("code", run).unwrap_or((found.start, found.end));
                codes.extend(std::str::from_utf8(&hay[start..end]).ok().map(String::from));
                from = found.end.max(found.start + 1);
            }
        }
        codes
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
            party("trader-joes", &["\"TRADER JOE\""]),
            party("shell", &["\"SHELL\""]),
            party("shell-oil", &["\"SHELL OIL\""]),
            party("paypal", &["\"PAYPAL\"", "\"PAYPAL *\" payee:(any+)"]),
            party("etsy-seller", &["\"ETSY SELLER\""]),
            party("uber", &["\"UBER\""]),
            party("halcyon", &["\"HALCYON\""]),
            party("blue-bottle", &["\"SQ *BL\" letter \"E BOTTLE\""]),
            party("dup-a", &["\"SAME\""]),
            party("dup-b", &["\"SAME\" any*"]),
            Known { name: "visa", account: true, patterns: vec!["\"CHASE CARD PAYMENT\""] },
        ];
        let codes = ["code:(\"inv-\" digit+ \"-\" digit+)", "\"CHECK \" code:(digit+)"];
        Recognizer::new(known, &codes).unwrap_or_else(|bad| panic!("{}", bad[0].error.message))
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
        assert_eq!(who("SQ *BLUUE BOTTLE"), (None, None));
        assert_eq!(who("SOMETHING ELSE"), (None, None));
        assert_eq!(who("CAFÉ ☕ SHELL"), (Some("shell"), None));
        assert_eq!(who(""), (None, None));
    }

    #[test]
    fn the_most_literal_text_wins_and_a_tie_is_an_error() {
        assert_eq!(who("SHELL 1234"), (Some("shell"), None));
        assert_eq!(who("SHELL OIL 1234"), (Some("shell-oil"), None));
        let reading = recognizer().read("SAME THING", &mut Scratch::default());
        let tie = reading.who.err().expect("a tie");
        let names = [tie.first.0.name, tie.second.0.name];
        assert!(names.contains(&"dup-a") && names.contains(&"dup-b"), "{names:?}");
    }

    #[test]
    fn a_payee_is_who_it_was_for_and_the_pattern_is_the_go_between() {
        assert_eq!(who("PAYPAL *ETSY SELLER"), (Some("etsy-seller"), Some("paypal")));
        assert_eq!(who("PAYPAL *UBER"), (Some("uber"), Some("paypal")), "whichever is longer");
        assert_eq!(who("PAYPAL *SOME SHOP"), (Some("paypal"), None), "a payee nobody knows stays with the go-between");
        assert_eq!(who("PAYPAL TRANSFER"), (Some("paypal"), None));
        assert_eq!(who("PAYPAL ETSY SELLER"), (Some("etsy-seller"), None), "words alone relate nothing");
    }

    #[test]
    fn an_account_is_the_other_end_of_a_transfer() {
        let reading = recognizer().read("CHASE CARD PAYMENT 0105", &mut Scratch::default());
        let who = reading.who.ok().and_then(|found| found.who).expect("recognized");
        assert_eq!((who.name, who.account), ("visa", true));
    }

    #[test]
    fn codes_are_what_the_declared_patterns_capture_lowercased() {
        let codes = |memo: &str| recognizer().read(memo, &mut Scratch::default()).codes;
        assert_eq!(codes("HALCYON PAYMENT INV-2026-01 THANKS"), ["inv-2026-01"]);
        assert_eq!(codes("payment for inv-2026-01."), ["inv-2026-01"]);
        assert_eq!(codes("INV-1-2 AND INV-3-4, CHECK 1041"), ["inv-1-2", "inv-3-4", "1041"]);
        assert!(codes("CHECK X INVOICE 2026").is_empty());
    }

    #[test]
    fn a_pattern_that_does_not_compile_is_reported_with_whose_it_is() {
        let known = vec![party("trader-joes", &["\"TRADER JOE\" digt+"])];
        let bad = Recognizer::new(known, &["\"ok\"", "oops"]).err().expect("refused");
        let shown: Vec<_> = bad.iter().map(|bad| (bad.owner, bad.pattern, bad.error.message.as_str())).collect();
        assert_eq!(shown, [("trader-joes", "\"TRADER JOE\" digt+", "`digt` is not a class"), ("code", "oops", "`oops` is not a class")]);
    }

    #[test]
    #[cfg_attr(debug_assertions, ignore = "timings are for release builds")]
    fn a_million_memos_against_two_hundred_patterns() {
        let names: Vec<String> = (0..200).map(|n| format!("merchant-{n}")).collect();
        let sources: Vec<String> = (0..200).map(|n| format!("\"SHOP {n:03} \" any+ / \"MRCH{n:03}\"")).collect();
        let known = names.iter().zip(&sources).map(|(name, source)| Known { name, account: false, patterns: vec![source] });
        let recognizer = Recognizer::new(known.collect(), &["code:(\"inv-\" digit+)"]).unwrap();
        let mut seed = 7u64;
        let mut next = |bound: u64| {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            (seed >> 33) % bound
        };
        let records: Vec<Record> = (0..1_000_000)
            .map(|_| {
                let memo = match next(4) {
                    0 => format!("POS PURCHASE SHOP {:03} SAN FRANCISCO CA #{}", next(200), next(9999)),
                    1 => format!("ACH DEBIT MRCH{:03} REF {}", next(200), next(99_999)),
                    2 => format!("CHECKCARD {:04} NOBODY IN PARTICULAR {}", next(9999), next(99_999)),
                    _ => format!("TRANSFER TO SOMEWHERE ELSE {}", next(99_999)),
                };
                Record { day: axiom_core::Day(0), qty: axiom_core::Qty(0), memo: memo.into(), balance: None, pending: false, at: axiom_core::Loc::default() }
            })
            .collect();
        let started = std::time::Instant::now();
        let read = recognizer.read_all(&records);
        let took = started.elapsed();
        let known = read.iter().filter(|reading| reading.who.as_ref().is_ok_and(|found| found.who.is_some())).count();
        eprintln!("read {} memos against {} patterns in {took:?}: {known} recognized", records.len(), sources.len());
        assert!((490_000..510_000).contains(&known), "{known}");
        assert!(took.as_millis() < 2000, "{took:?}");
    }

    #[test]
    fn reading_in_parallel_is_reading_one_by_one() {
        let memos = ["TRADER JOE'S", "SHELL OIL", "PAYPAL *UBER", "nothing", "INV-9-9"];
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
