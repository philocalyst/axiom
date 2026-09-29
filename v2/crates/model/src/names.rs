//! Name tables: a thing is known by every `/`-boundary suffix of its path.
//!
//! `assets/bank/checking` answers to `checking`, `bank/checking` and its full
//! path. Some names outrank others: an alias beats a full path, and a full path
//! beats a suffix of some longer path, so a name that is exact never becomes
//! ambiguous because of what was declared later. Among equals, a name shared by
//! several things is *ambiguous*: the table keeps all of them so the diagnostic
//! can list the candidates. Keys are interned, so a lookup is one hash of the
//! text and one of the symbol.

use axiom_core::diag::closest;
use axiom_core::{Id, Interner, Map, Sym};

use crate::book::Miss;
use crate::scope::{Home, Scope};

/// How well a written name fits a thing: higher wins.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) enum Rank {
    Suffix,
    Path,
    Alias,
}

/// What a lookup found.
pub(crate) enum Found<T> {
    One(Id<T>),
    Nothing,
    Several(Vec<Id<T>>),
}

pub(crate) struct Names<T> {
    slots: Map<Sym, Slot<T>>,
}

struct Slot<T> {
    rank: Rank,
    ids: Ids<T>,
}

enum Ids<T> {
    One(Id<T>),
    Many(Vec<Id<T>>),
}

impl<T> Ids<T> {
    fn as_slice(&self) -> &[Id<T>] {
        match self {
            Ids::One(id) => std::slice::from_ref(id),
            Ids::Many(ids) => ids,
        }
    }
}

impl<T> Default for Names<T> {
    fn default() -> Names<T> {
        Names { slots: Map::default() }
    }
}

/// `path` itself, then each suffix that starts after a `/`.
fn suffixes(path: &str) -> impl Iterator<Item = &str> {
    std::iter::once(path).chain(path.match_indices('/').map(|(at, _)| &path[at + 1..]))
}

/// The candidate closest to `name`, considering only those whose length could
/// possibly be close enough: an edit distance costs more than a length check.
pub(crate) fn near<'a>(name: &str, candidates: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    let window = (name.len() / 3).max(1);
    closest(name, candidates.into_iter().filter(|candidate| candidate.len().abs_diff(name.len()) <= window))
}

/// How many single-character edits turn `a` into `b`: insertions, deletions,
/// substitutions, and swaps of two neighbours.
pub(crate) fn edits(a: &str, b: &str) -> usize {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut rows = vec![vec![0usize; b.len() + 1]; 3];
    rows[1].iter_mut().enumerate().for_each(|(j, cell)| *cell = j);
    for i in 1..=a.len() {
        rows.rotate_left(1);
        rows[1][0] = i;
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut best = (rows[0][j] + 1).min(rows[1][j - 1] + 1).min(rows[0][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                best = best.min(rows[2][j - 2] + 1);
            }
            rows[1][j] = best;
        }
    }
    rows[1][b.len()]
}

impl<T> Names<T> {
    /// Registers `id` under `key`, unless something of a higher rank has it.
    pub fn insert<'s>(&mut self, names: &mut Interner<'s>, key: &'s str, rank: Rank, id: Id<T>) {
        let sym = names.intern(key);
        match self.slots.get_mut(&sym) {
            None => {
                self.slots.insert(sym, Slot { rank, ids: Ids::One(id) });
            }
            Some(slot) if rank > slot.rank => *slot = Slot { rank, ids: Ids::One(id) },
            Some(slot) if rank == slot.rank => match &mut slot.ids {
                Ids::Many(ids) => ids.push(id),
                Ids::One(first) => slot.ids = Ids::Many(vec![*first, id]),
            },
            Some(_) => {}
        }
    }

    /// Registers `id` under its full path and every suffix of it.
    pub fn insert_path<'s>(&mut self, names: &mut Interner<'s>, path: &'s str, id: Id<T>) {
        for (at, suffix) in suffixes(path).enumerate() {
            self.insert(names, suffix, if at == 0 { Rank::Path } else { Rank::Suffix }, id);
        }
    }

    /// Everything that best answers to `text`, before any visibility filter.
    pub fn candidates(&self, names: &Interner, text: &str) -> &[Id<T>] {
        names.get(text).and_then(|key| self.slots.get(&key)).map_or(&[], |slot| slot.ids.as_slice())
    }

    /// What is visible under `text`. The success path allocates nothing, and
    /// a miss costs nothing more: suggestions are for [`Names::resolve`].
    pub fn find(&self, names: &Interner, text: &str, visible: impl Fn(Id<T>) -> bool) -> Found<T> {
        let all = self.candidates(names, text);
        let mut seen = all.iter().copied().filter(|&id| visible(id));
        match (seen.next(), seen.next()) {
            (Some(only), None) => Found::One(only),
            (None, _) => Found::Nothing,
            (Some(_), Some(_)) => Found::Several(all.iter().copied().filter(|&id| visible(id)).collect()),
        }
    }

    /// The one visible thing named `text`, or why there is not exactly one,
    /// with the closest name when there is none.
    pub fn resolve(&self, names: &Interner, text: &str, visible: impl Fn(Id<T>) -> bool) -> Result<Id<T>, Miss<T>> {
        match self.find(names, text, &visible) {
            Found::One(only) => Ok(only),
            Found::Nothing => Err(Miss::Unknown { suggestion: self.suggest(names, text, &visible) }),
            Found::Several(ids) => Err(Miss::Ambiguous(ids.into())),
        }
    }

    /// Every name the table answers to.
    pub fn keys<'a>(&'a self, names: &'a Interner) -> impl Iterator<Item = &'a str> {
        self.slots.keys().map(|&key| names.name(key))
    }

    /// The shortest suffix of `path` that names `id` alone: what to write to
    /// disambiguate.
    pub fn shortest_unique<'p>(&self, names: &Interner, path: &'p str, id: Id<T>) -> &'p str {
        let mut by_length: Vec<&str> = suffixes(path).collect();
        by_length.reverse();
        by_length.into_iter().find(|suffix| self.candidates(names, suffix) == [id]).unwrap_or(path)
    }

    /// The known name closest to `text`, among the things `visible` admits.
    fn suggest(&self, names: &Interner, text: &str, visible: &impl Fn(Id<T>) -> bool) -> Option<Sym> {
        let keys = self.slots.iter().filter(|(_, slot)| slot.ids.as_slice().iter().any(|&id| visible(id)));
        names.get(near(text, keys.map(|(&key, _)| names.name(key)))?)
    }
}

/// A name table over things that were written somewhere, so that each reader
/// sees only some of them.
pub(crate) struct Scoped<T> {
    pub names: Names<T>,
    homes: Vec<Home>,
}

impl<T> Default for Scoped<T> {
    fn default() -> Scoped<T> {
        Scoped { names: Names::default(), homes: Vec::new() }
    }
}

impl<T> Scoped<T> {
    /// `things` yields each thing's id, path and home.
    pub fn build<'s>(
        interner: &mut Interner<'s>,
        things: impl IntoIterator<Item = (Id<T>, &'s str, Home)>,
    ) -> Scoped<T> {
        let mut table = Scoped::default();
        for (id, path, home) in things {
            table.names.insert_path(interner, path, id);
            if table.homes.len() <= id.index() {
                table.homes.resize(id.index() + 1, Home::Builtin);
            }
            table.homes[id.index()] = home;
        }
        table
    }

    pub fn home(&self, id: Id<T>) -> Home {
        self.homes[id.index()]
    }

    /// What `scope` can see under `text`.
    pub fn find(&self, interner: &Interner, scope: &Scope, text: &str) -> Found<T> {
        self.names.find(interner, text, |id| scope.sees(self.home(id)))
    }

    /// The one thing `scope` can see under `text`.
    pub fn resolve(&self, interner: &Interner, scope: &Scope, text: &str) -> Result<Id<T>, Miss<T>> {
        self.names.resolve(interner, text, |id| scope.sees(self.home(id)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct Thing;

    fn table<'s>(names: &mut Interner<'s>, paths: &[&'s str]) -> Names<Thing> {
        let mut table = Names::default();
        for (i, path) in paths.iter().enumerate() {
            table.insert_path(names, path, Id::new(i as u32));
        }
        table
    }

    #[test]
    fn suffixes_resolve_when_unique_and_list_candidates_when_not() {
        let mut names = Interner::default();
        let paths = ["assets/bank/checking", "assets/broker/checking", "assets/bank/savings"];
        let table = table(&mut names, &paths);
        let all = |_: Id<Thing>| true;
        assert_eq!(table.resolve(&names, "savings", all).unwrap(), Id::new(2));
        assert_eq!(table.resolve(&names, "bank/checking", all).unwrap(), Id::new(0));
        match table.resolve(&names, "checking", all) {
            Err(Miss::Ambiguous(ids)) => assert_eq!(&*ids, &[Id::new(0), Id::new(1)]),
            other => panic!("expected ambiguity, got {other:?}"),
        }
        assert_eq!(table.shortest_unique(&names, paths[1], Id::new(1)), "broker/checking");
    }

    #[test]
    fn an_alias_beats_a_path_and_a_path_beats_a_suffix() {
        let mut names = Interner::default();
        let mut table = table(&mut names, &["assets/business", "expenses/business", "x/assets/business"]);
        assert_eq!(table.candidates(&names, "assets/business"), [Id::new(0)], "the full path wins over a suffix");
        assert_eq!(table.candidates(&names, "business").len(), 3);
        table.insert(&mut names, "business", Rank::Alias, Id::new(0));
        assert_eq!(table.candidates(&names, "business"), [Id::new(0)], "an alias wins over any suffix");
    }

    #[test]
    fn unknown_names_suggest_the_closest() {
        let mut names = Interner::default();
        let table = table(&mut names, &["assets/bank/checking"]);
        match table.resolve(&names, "chekcing", |_| true) {
            Err(Miss::Unknown { suggestion: Some(s) }) => assert_eq!(names.name(s), "checking"),
            other => panic!("expected a suggestion, got {other:?}"),
        }
    }
}
