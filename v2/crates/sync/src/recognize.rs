//! Who a memo is. The ledger's `known-as` patterns, and every name's own name,
//! are compiled once and found through a trie of the literals they begin with,
//! so a record costs one pass over its memo and no allocation.

use std::cmp::Reverse;

use axiom_core::par;

use crate::Record;
use crate::peg::{Capture, Found, Parts, Pattern, PatternError, Patterns, Run};

/// Records handed to one worker at a time: enough to reuse a scratch buffer.
const CHUNK: usize = 4096;

/// An entity or an account, and how it appears on statements.
pub struct Known<'a> {
    pub name: &'a str,
    /// An account rather than a party: money with it is a transfer.
    pub account: bool,
    /// `known-as` patterns (see [`Pattern`]), each matched anywhere in the memo,
    /// in any case. A pattern with a `payee` capture names a go-between: what the
    /// capture holds is who the money was for (`"PAYPAL *" payee:rest`). The name
    /// itself is a pattern too, with hyphens as spaces, and needs none written.
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
#[derive(Clone, Debug)]
pub struct Tie<'a> {
    pub first: (Who<'a>, String),
    pub second: (Who<'a>, String),
}

/// A memo, read: who it is, the codes it names, lowercased, and the amount and
/// day it says of itself, when a pattern captured them.
pub struct Reading<'a> {
    pub who: Result<Recognized<'a>, Tie<'a>>,
    pub codes: Vec<String>,
    pub amount: Option<String>,
    pub date: Option<String>,
}

/// A pattern that does not compile, and whose it is (`code` for a code).
#[derive(Debug)]
pub struct BadPattern<'a> {
    pub owner: &'a str,
    pub pattern: &'a str,
    pub error: PatternError,
}

struct Entry {
    owner: usize,
    /// As the ledger wrote it, for a tie to name.
    source: String,
    pattern: Pattern,
    /// A name's own pattern: it counts only as a whole word.
    whole: bool,
}

#[derive(Clone, Copy)]
struct Hit {
    entry: usize,
    found: Found,
    /// Where the pattern's captures were.
    parts: Parts,
}

impl Hit {
    fn part(&self, capture: Capture) -> Option<(usize, usize)> {
        self.parts[capture as usize]
    }
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
    entries: Vec<Entry>,
    starts: Trie,
    /// The patterns that may begin with anything, to be tried everywhere.
    floating: Vec<usize>,
    /// The declared `code` patterns. What one captures as `code`, or else all
    /// it matches, is a code.
    codes: Vec<Pattern>,
    /// The ledger's own patterns, which the others call.
    named: Patterns,
}

/// The text between two offsets of the lowercased memo.
fn text(hay: &[u8], start: usize, end: usize) -> Option<String> {
    std::str::from_utf8(&hay[start..end]).ok().map(String::from)
}

impl<'a> Recognizer<'a> {
    /// Compiles every pattern, each able to call the ledger's `named` ones.
    pub fn new(
        known: Vec<Known<'a>>,
        codes: &[&'a str],
        named: &Patterns,
    ) -> Result<Recognizer<'a>, Vec<BadPattern<'a>>> {
        let (mut entries, mut compiled, mut bad) = (Vec::new(), Vec::new(), Vec::new());
        let written = known
            .iter()
            .enumerate()
            .flat_map(|(owner, entry)| entry.patterns.iter().map(move |&source| (Some(owner), entry.name, source)));
        for (owner, name, source) in written.chain(codes.iter().map(|&source| (None, "code", source))) {
            match (Pattern::new(source, named), owner) {
                (Ok(pattern), Some(owner)) => {
                    entries.push(Entry { owner, source: source.to_string(), pattern, whole: false })
                }
                (Ok(pattern), None) => compiled.push(pattern),
                (Err(error), _) => bad.push(BadPattern { owner: name, pattern: source, error }),
            }
        }
        if !bad.is_empty() {
            return Err(bad);
        }
        for (owner, entry) in known.iter().enumerate() {
            let words = entry.name.replace('-', " ");
            let (source, pattern) = (format!("\"{words}\""), Pattern::literal(&words));
            entries.push(Entry { owner, source, pattern, whole: true });
        }
        let (mut starts, mut floating) = (Trie::new(), Vec::new());
        for (at, entry) in entries.iter().enumerate() {
            match entry.pattern.starts() {
                Some(literals) => literals.iter().for_each(|literal| starts.insert(literal, at)),
                None => floating.push(at),
            }
        }
        Ok(Recognizer { known, entries, starts, floating, codes: compiled, named: named.clone() })
    }

    /// The account of the book called `name`, if the recognizer knows one.
    pub fn account(&self, name: &str) -> Option<&'a str> {
        self.known.iter().find(|known| known.account && known.name == name).map(|known| known.name)
    }

    /// Reads every record's memo, in parallel.
    pub fn read_all(&self, records: &[&Record]) -> Vec<Reading<'a>> {
        let chunks: Vec<&[&Record]> = records.chunks(CHUNK).collect();
        let read = |chunk: &&[&Record]| {
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
        let mut codes = self.codes_in(lower, run);
        codes.extend(hits.iter().filter_map(|hit| hit.part(Capture::Code)).filter_map(|(a, b)| text(lower, a, b)));
        let (who, parts) = match self.decide(lower, hits) {
            Ok((who, parts)) => (Ok(who), parts),
            Err(tie) => (Err(tie), Parts::default()),
        };
        let told = |capture: Capture| parts[capture as usize].and_then(|(start, end)| text(lower, start, end));
        Reading { who, codes, amount: told(Capture::Amount), date: told(Capture::Date) }
    }

    /// Every place a pattern matches in `hay`.
    fn find_hits(&self, hay: &[u8], run: &mut Run, hits: &mut Vec<Hit>) {
        hits.clear();
        for at in 0..hay.len() {
            let mut try_entry = |entry: usize| {
                let Entry { pattern, whole, .. } = &self.entries[entry];
                // A match of nothing says nothing about who it was.
                let found = pattern.matches_at(hay, at, run, &self.named).filter(|found| found.end > found.start);
                let word = |at: usize| hay.get(at).is_some_and(|byte| byte.is_ascii_alphanumeric());
                let bounded = |found: &Found| !*whole || !(found.start > 0 && word(found.start - 1) || word(found.end));
                if let Some(found) = found.filter(bounded) {
                    let parts = if pattern.captures() { run.parts() } else { Parts::default() };
                    hits.push(Hit { entry, found, parts });
                }
            };
            self.starts.walk(hay, at, &mut try_entry);
            self.floating.iter().for_each(|&entry| try_entry(entry));
        }
    }

    fn owner(&self, hit: &Hit) -> usize {
        self.entries[hit.entry].owner
    }

    fn who(&self, owner: usize) -> Who<'a> {
        Who { name: self.known[owner].name, account: self.known[owner].account }
    }

    /// The best-matching entity: the most literal text matched wins, and two
    /// that tie are an error. What lies inside another party's `payee` is not
    /// in the running: that pattern has said what it is. What the best pattern
    /// captured comes with it.
    fn decide(&self, hay: &[u8], hits: &mut [Hit]) -> Result<(Recognized<'a>, Parts), Tie<'a>> {
        hits.sort_unstable_by_key(|hit| (self.owner(hit), Reverse(hit.found.literal), hit.found.start));
        let payee = |hit: &Hit| hit.part(Capture::Payee).filter(|_| !self.known[self.owner(hit)].account);
        let inside_a_payee = |hit: &Hit| {
            hits.iter().any(|other| {
                payee(other).is_some_and(|(start, end)| {
                    self.owner(other) != self.owner(hit) && start <= hit.found.start && hit.found.end <= end
                })
            })
        };
        let mut best_of_each: Vec<&Hit> = Vec::new();
        for hit in hits.iter().filter(|hit| !inside_a_payee(hit)) {
            if best_of_each.last().is_none_or(|last| self.owner(last) != self.owner(hit)) {
                best_of_each.push(hit);
            }
        }
        best_of_each.sort_by_key(|hit| (Reverse(hit.found.literal), hit.found.start));
        let candidate = |hit: &Hit| (self.who(self.owner(hit)), self.entries[hit.entry].source.clone());
        match best_of_each.as_slice() {
            [] => Ok((Recognized::default(), Parts::default())),
            [best, next, ..] if best.found.literal == next.found.literal => {
                Err(Tie { first: candidate(best), second: candidate(next) })
            }
            [best, ..] => Ok((self.through(best, payee(best), hay)?, best.parts)),
        }
    }

    /// The best hit as a party, or, if its pattern names a `payee`, the party
    /// that is, with the best hit as the go-between.
    fn through(&self, best: &Hit, payee: Option<(usize, usize)>, hay: &[u8]) -> Result<Recognized<'a>, Tie<'a>> {
        let outer = self.who(self.owner(best));
        let Some((start, end)) = payee else { return Ok(Recognized { who: Some(outer), via: None }) };
        let (mut run, mut hits) = (Run::default(), Vec::new());
        self.find_hits(&hay[start..end], &mut run, &mut hits);
        Ok(match self.decide(&hay[start..end], &mut hits)?.0.who {
            Some(inner) => Recognized { who: Some(inner), via: Some(outer.name) },
            None => Recognized { who: Some(outer), via: None },
        })
    }

    /// The codes the memo names, by each declared code pattern in turn.
    fn codes_in(&self, hay: &[u8], run: &mut Run) -> Vec<String> {
        let mut codes = Vec::new();
        for pattern in &self.codes {
            let mut from = 0;
            while let Some(found) = pattern.find(hay, from, run, &self.named) {
                let (start, end) = run.capture(Capture::Code).unwrap_or((found.start, found.end));
                codes.extend(text(hay, start, end));
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
            party("paypal", &["\"PAYPAL\"", "\"PAYPAL *\" payee:rest"]),
            party("etsy-seller", &["\"ETSY SELLER\""]),
            party("uber", &["\"UBER\""]),
            party("halcyon", &["\"HALCYON\""]),
            party("blue-bottle", &["\"SQ *BL\" letter \"E BOTTLE\""]),
            party("dup-a", &["\"SAME\""]),
            party("dup-b", &["\"SAME\" any*"]),
            Known { name: "visa", account: true, patterns: vec!["\"CHASE CARD PAYMENT\""] },
        ];
        let codes = ["code:(\"inv-\" digit+ \"-\" digit+)", "\"CHECK \" code:(digit+)"];
        Recognizer::new(known, &codes, &Patterns::default()).unwrap_or_else(|bad| panic!("{}", bad[0].error.message))
    }

    fn who(memo: &str) -> (Option<&'static str>, Option<&'static str>) {
        let reading = recognizer().read(memo, &mut Scratch::default());
        let found =
            reading.who.unwrap_or_else(|tie| panic!("{memo}: a tie between {:?} and {:?}", tie.first.0, tie.second.0));
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
    fn a_name_is_known_by_itself_as_a_whole_word() {
        let known = vec![party("ashgrove", &[]), party("trader-joes", &[]), party("uber", &[])];
        let recognizer = Recognizer::new(known, &[], &Patterns::default()).unwrap();
        let who = |memo: &str| recognizer.read(memo, &mut Scratch::default()).who.ok()?.who.map(|who| who.name);
        assert_eq!(who("ach ASHGROVE property"), Some("ashgrove"));
        assert_eq!(who("TRADER JOES #634"), Some("trader-joes"));
        assert_eq!(who("TRADER JOE'S #634"), None, "a spelling the bank chose needs a pattern");
        assert_eq!(who("UBER *TRIP"), Some("uber"));
        assert_eq!((who("SUBERB"), who("UBERALLES"), who("uber2")), (None, None, None));
    }

    #[test]
    fn a_pattern_can_say_the_amount_and_the_day_of_its_memo() {
        let pattern = "\"WISE \" amount:(digit+ \".\" digit+) \" on \" date:(digit+ \"/\" digit+)";
        let recognizer = Recognizer::new(vec![party("wise", &[pattern])], &[], &Patterns::default()).unwrap();
        let reading = recognizer.read("CONVERSION WISE 58.40 ON 01/05 REF 9", &mut Scratch::default());
        assert_eq!((reading.amount.as_deref(), reading.date.as_deref()), (Some("58.40"), Some("01/05")));
        assert_eq!(reading.who.unwrap().who.map(|who| who.name), Some("wise"));
        assert_eq!(recognizer.read("WISE TRANSFER", &mut Scratch::default()).amount, None);
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
        let bad = Recognizer::new(known, &["\"ok\"", "oops"], &Patterns::default()).err().expect("refused");
        let shown: Vec<_> = bad.iter().map(|bad| (bad.owner, bad.pattern, bad.error.message.as_str())).collect();
        assert_eq!(
            shown,
            [
                ("trader-joes", "\"TRADER JOE\" digt+", "there is no class or pattern called `digt`"),
                ("code", "oops", "there is no class or pattern called `oops`")
            ]
        );
    }

    #[test]
    #[ignore = "a timing, alone: cargo test -p axiom-sync --release -- --ignored --test-threads=1"]
    fn a_million_memos_against_two_hundred_patterns() {
        let names: Vec<String> = (0..200).map(|n| format!("merchant-{n}")).collect();
        let sources: Vec<String> = (0..200).map(|n| format!("\"SHOP {n:03} \" any+ / \"MRCH{n:03}\"")).collect();
        let known =
            names.iter().zip(&sources).map(|(name, source)| Known { name, account: false, patterns: vec![source] });
        let recognizer = Recognizer::new(known.collect(), &["code:(\"inv-\" digit+)"], &Patterns::default()).unwrap();
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
                Record::new(axiom_core::Day(0), axiom_core::Qty(0), memo)
            })
            .collect();
        let started = std::time::Instant::now();
        let read = recognizer.read_all(&records.iter().collect::<Vec<_>>());
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
            .map(|at| Record::new(axiom_core::Day(0), axiom_core::Qty(0), memos[at % memos.len()]))
            .collect();
        let recognizer = recognizer();
        let read = recognizer.read_all(&records.iter().collect::<Vec<_>>());
        let mut scratch = Scratch::default();
        for (record, reading) in records.iter().zip(&read) {
            let alone = recognizer.read(&record.memo, &mut scratch);
            assert_eq!(alone.who.as_ref().ok(), reading.who.as_ref().ok());
            assert_eq!(alone.codes, reading.codes);
        }
    }
}
