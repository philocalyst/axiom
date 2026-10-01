//! Runtime recognition over the book's borrowed entities, accounts and flat
//! model pattern programs. The trie stores only entry indices and lowercase
//! literal prefixes; it does not copy or recompile a pattern.

use std::cmp::Reverse;

use axiom_core::Id;
use axiom_core::par;
use axiom_model::sync::{Capture, Op, Pattern};
use axiom_model::{Book, Entity, Place, Role};

use crate::Record;
use crate::peg::{Found, Patterns, Run};

const CHUNK: usize = 4096;
const BUILTINS: [Capture; 5] = [
    Capture::Payee,
    Capture::Code,
    Capture::Amount,
    Capture::Original,
    Capture::Date,
];
type Parts = [Option<(usize, usize)>; 5];

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum KnownId {
    Entity(Id<Entity>),
    Place(Id<Place>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Who<'a> {
    pub id: KnownId,
    pub name: &'a str,
    pub account: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Recognized<'a> {
    pub who: Option<Who<'a>>,
    pub via: Option<&'a str>,
}

#[derive(Clone, Debug)]
pub struct Tie<'a> {
    pub first: Who<'a>,
    pub second: Who<'a>,
}

pub struct Reading<'t, 's> {
    pub who: Result<Recognized<'s>, Tie<'s>>,
    pub codes: Vec<&'t str>,
    pub amount: Option<&'t str>,
    pub original: Option<&'t str>,
    pub date: Option<&'t str>,
}

struct Known<'b, 's> {
    id: KnownId,
    name: &'s str,
    account: bool,
    patterns: &'b [Id<Pattern>],
    aliases: Vec<&'s str>,
}

struct Entry {
    owner: usize,
    pattern: Option<Id<Pattern>>,
    /// The own-name spelling, stored only for the search index. Explicit
    /// declarations always execute their model-owned program by id.
    own: Option<Box<[u8]>>,
    whole: bool,
}

#[derive(Clone, Copy)]
struct Hit {
    entry: usize,
    found: Found,
    parts: Parts,
}

impl Hit {
    fn part(&self, capture: Capture) -> Option<(usize, usize)> {
        let slot = BUILTINS.iter().position(|known| *known == capture)?;
        self.parts[slot]
    }
}

#[derive(Default)]
pub struct Scratch {
    lower: Vec<u8>,
    run: Run,
    hits: Vec<Hit>,
}

struct Trie {
    nodes: Vec<TrieNode>,
    first: [u32; 256],
}

#[derive(Default)]
struct TrieNode {
    next: Vec<(u8, u32)>,
    entries: Vec<usize>,
}

impl Trie {
    fn new() -> Trie {
        Trie {
            nodes: vec![TrieNode::default()],
            first: [0; 256],
        }
    }

    fn insert(&mut self, literal: &[u8], entry: usize) {
        if literal.is_empty() {
            return;
        }
        let mut at = 0;
        for (depth, &byte) in literal.iter().enumerate() {
            let existing = match depth {
                0 => Some(self.first[byte as usize]).filter(|&node| node != 0),
                _ => self.nodes[at as usize]
                    .next
                    .iter()
                    .find(|(next, _)| *next == byte)
                    .map(|&(_, node)| node),
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
        self.nodes[at as usize].entries.push(entry);
    }

    fn walk(&self, hay: &[u8], at: usize, mut visit: impl FnMut(usize)) {
        let Some(&lead) = hay.get(at) else { return };
        let (mut node, mut cursor) = (self.first[lead as usize], at + 1);
        while node != 0 {
            self.nodes[node as usize]
                .entries
                .iter()
                .for_each(|&entry| visit(entry));
            let Some(&byte) = hay.get(cursor) else { return };
            node = self.nodes[node as usize]
                .next
                .iter()
                .find(|(next, _)| *next == byte)
                .map_or(0, |&(_, node)| node);
            cursor += 1;
        }
    }
}

pub struct Recognizer<'b, 's> {
    known: Vec<Known<'b, 's>>,
    entries: Vec<Entry>,
    starts: Trie,
    floating: Vec<usize>,
    codes: Vec<Id<Pattern>>,
    patterns: Patterns<'b, 's>,
}

impl<'b, 's> Recognizer<'b, 's> {
    pub fn new(book: &'b Book<'s>) -> Recognizer<'b, 's> {
        let patterns = Patterns::new(&book.patterns, &book.names);
        let mut known: Vec<Known<'b, 's>> = Vec::new();
        for (id, entity) in book.entities.iter() {
            if id != book.roots.me {
                let name = book.names.name(entity.path);
                known.push(Known {
                    id: KnownId::Entity(id),
                    name,
                    account: false,
                    patterns: &entity.known_as,
                    aliases: aliases(name),
                });
            }
        }
        for (id, place) in book.places.iter() {
            if matches!(place.role, Role::Account { .. }) {
                let name = book.names.name(place.path);
                known.push(Known {
                    id: KnownId::Place(id),
                    name,
                    account: true,
                    patterns: &place.known_as,
                    aliases: aliases(name),
                });
            }
        }
        let mut entries = Vec::new();
        let mut starts = Trie::new();
        let mut floating = Vec::new();
        for (owner, item) in known.iter().enumerate() {
            for &pattern in item.patterns {
                let entry = entries.len();
                let whole = matches!(book.patterns[pattern].program.as_ref(), [Op::Name(_)]);
                entries.push(Entry {
                    owner,
                    pattern: Some(pattern),
                    own: None,
                    whole,
                });
                match patterns.starts(pattern) {
                    Some(literals) => literals
                        .iter()
                        .for_each(|literal| starts.insert(literal, entry)),
                    None => floating.push(entry),
                }
            }
            for alias in &item.aliases {
                let has_model_name = item.patterns.iter().any(|&pattern| {
                    matches!(book.patterns[pattern].program.as_ref(), [Op::Name(name)] if book.name(*name) == *alias)
                });
                if has_model_name {
                    continue;
                }
                let own = alias
                    .bytes()
                    .map(|byte| match byte {
                        b'-' | b'/' => b' ',
                        other => other.to_ascii_lowercase(),
                    })
                    .collect::<Vec<_>>()
                    .into_boxed_slice();
                let entry = entries.len();
                let first = own.split(|byte| *byte == b' ').next().unwrap_or(&own);
                starts.insert(first, entry);
                entries.push(Entry {
                    owner,
                    pattern: None,
                    own: Some(own),
                    whole: true,
                });
            }
        }
        let codes = book
            .code_rules
            .iter()
            .flat_map(|rule| rule.known_as.iter().copied())
            .collect();
        Recognizer {
            known,
            entries,
            starts,
            floating,
            codes,
            patterns,
        }
    }

    pub fn account(&self, name: &str) -> Option<&'s str> {
        self.known
            .iter()
            .find(|known| {
                known.account
                    && known
                        .aliases
                        .iter()
                        .any(|alias| alias.eq_ignore_ascii_case(name))
            })
            .map(|known| known.name)
    }

    pub fn read_all<'t>(&self, records: &[&Record<'t>]) -> Vec<Reading<'t, 's>> {
        let chunks: Vec<&[&Record]> = records.chunks(CHUNK).collect();
        let read = |chunk: &&[&Record]| {
            let mut scratch = Scratch::default();
            chunk
                .iter()
                .map(|record| self.read(&record.memo, &mut scratch))
                .collect::<Vec<_>>()
        };
        par::map_each(&chunks, read).into_iter().flatten().collect()
    }

    pub fn read<'t>(&self, memo: &'t str, scratch: &mut Scratch) -> Reading<'t, 's> {
        let Scratch { lower, run, hits } = scratch;
        lower.clear();
        lower.extend(memo.bytes().map(|byte| byte.to_ascii_lowercase()));
        self.find_hits(lower, run, hits);
        let mut codes = self.codes_in(memo, lower, run);
        for hit in hits.iter() {
            if let Some((start, end)) = hit.part(Capture::Code) {
                if let Some(text) = memo.get(start..end) {
                    codes.push(text);
                }
            }
        }
        let (who, parts) = match self.decide(lower, hits, 0) {
            Ok((who, parts)) => (Ok(who), parts),
            Err(tie) => (Err(tie), [None; 5]),
        };
        let capture = |kind| {
            BUILTINS
                .iter()
                .position(|known| *known == kind)
                .and_then(|slot| parts[slot])
                .and_then(|(start, end)| memo.get(start..end))
        };
        Reading {
            who,
            codes,
            amount: capture(Capture::Amount),
            original: capture(Capture::Original),
            date: capture(Capture::Date),
        }
    }

    fn find_hits(&self, hay: &[u8], run: &mut Run, hits: &mut Vec<Hit>) {
        hits.clear();
        for at in 0..hay.len() {
            let mut try_entry = |entry_index: usize| {
                let entry = &self.entries[entry_index];
                let found = match (entry.pattern, entry.own.as_deref()) {
                    (Some(pattern), _) => run.matches_at(pattern, hay, at, &self.patterns),
                    (None, Some(literal)) => at
                        .checked_add(literal.len())
                        .and_then(|end| hay.get(at..end).map(|candidate| (end, candidate)))
                        .filter(|(_, candidate)| {
                            candidate.iter().zip(literal).all(|(&actual, &expected)| {
                                if expected == b' ' {
                                    matches!(actual, b' ' | b'-' | b'/')
                                } else {
                                    actual.eq_ignore_ascii_case(&expected)
                                }
                            })
                        })
                        .map(|(end, _)| Found {
                            start: at,
                            end,
                            literal: literal.len(),
                        }),
                    _ => None,
                }
                .filter(|found| found.end > found.start);
                let word = |at: usize| hay.get(at).is_some_and(|byte| byte.is_ascii_alphanumeric());
                let bounded = |found: &Found| {
                    !entry.whole || !(found.start > 0 && word(found.start - 1) || word(found.end))
                };
                if let Some(found) = found.filter(bounded) {
                    let mut parts = [None; 5];
                    if entry.pattern.is_some() {
                        for (slot, capture) in BUILTINS.iter().copied().enumerate() {
                            parts[slot] = run.capture(capture);
                        }
                    }
                    hits.push(Hit {
                        entry: entry_index,
                        found,
                        parts,
                    });
                }
            };
            self.starts.walk(hay, at, &mut try_entry);
            self.floating.iter().for_each(|&entry| try_entry(entry));
        }
    }

    fn owner(&self, hit: &Hit) -> usize {
        self.entries[hit.entry].owner
    }

    fn who(&self, owner: usize) -> Who<'s> {
        let known = &self.known[owner];
        Who {
            id: known.id,
            name: known.name,
            account: known.account,
        }
    }

    fn decide<'a>(
        &self,
        hay: &[u8],
        hits: &mut [Hit],
        depth: usize,
    ) -> Result<(Recognized<'s>, Parts), Tie<'s>> {
        if depth > 32 {
            return Ok((Recognized::default(), [None; 5]));
        }
        hits.sort_unstable_by_key(|hit| {
            (self.owner(hit), Reverse(hit.found.literal), hit.found.start)
        });
        let payee = |hit: &Hit| {
            hit.part(Capture::Payee)
                .filter(|_| !self.known[self.owner(hit)].account)
        };
        let inside_a_payee = |hit: &Hit| {
            hits.iter().any(|other| {
                payee(other).is_some_and(|(start, end)| {
                    self.owner(other) != self.owner(hit)
                        && start <= hit.found.start
                        && hit.found.end <= end
                })
            })
        };
        let mut best_of_each = Vec::new();
        for hit in hits.iter().filter(|hit| !inside_a_payee(hit)) {
            if best_of_each
                .last()
                .is_none_or(|last: &&Hit| self.owner(last) != self.owner(hit))
            {
                best_of_each.push(hit);
            }
        }
        best_of_each.sort_by_key(|hit| (Reverse(hit.found.literal), hit.found.start));
        match best_of_each.as_slice() {
            [] => Ok((Recognized::default(), [None; 5])),
            [best, next, ..] if best.found.literal == next.found.literal => Err(Tie {
                first: self.who(self.owner(best)),
                second: self.who(self.owner(next)),
            }),
            [best, ..] => Ok((self.through(best, payee(best), hay, depth)?, best.parts)),
        }
    }

    fn through(
        &self,
        best: &Hit,
        payee: Option<(usize, usize)>,
        hay: &[u8],
        depth: usize,
    ) -> Result<Recognized<'s>, Tie<'s>> {
        let outer = self.who(self.owner(best));
        let Some((start, end)) = payee else {
            return Ok(Recognized {
                who: Some(outer),
                via: None,
            });
        };
        let Some(inner) = hay.get(start..end) else {
            return Ok(Recognized {
                who: Some(outer),
                via: None,
            });
        };
        let mut run = Run::default();
        let mut hits = Vec::new();
        self.find_hits(inner, &mut run, &mut hits);
        Ok(match self.decide(inner, &mut hits, depth + 1)?.0.who {
            Some(inner) if inner.id != outer.id => Recognized {
                who: Some(inner),
                via: Some(outer.name),
            },
            _ => Recognized {
                who: Some(outer),
                via: None,
            },
        })
    }

    fn codes_in<'t>(&self, memo: &'t str, hay: &[u8], run: &mut Run) -> Vec<&'t str> {
        let mut codes = Vec::new();
        for &pattern in &self.codes {
            let mut from = 0;
            while let Some(found) = run.find(pattern, hay, from, &self.patterns) {
                let (start, end) = run
                    .capture(Capture::Code)
                    .unwrap_or((found.start, found.end));
                if let Some(code) = memo.get(start..end) {
                    codes.push(code);
                }
                from = found.end.max(found.start + 1);
            }
        }
        codes
    }
}

fn aliases(name: &str) -> Vec<&str> {
    let mut names = vec![name];
    names.extend(name.match_indices('/').map(|(slash, _)| &name[slash + 1..]));
    names.sort_unstable();
    names.dedup();
    names
}

#[cfg(test)]
mod tests;
