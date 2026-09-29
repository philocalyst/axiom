//! Name tables: a thing is known by every `/`-boundary suffix of its path.
//!
//! `assets/bank/checking` answers to `checking`, `bank/checking` and its full
//! path. A suffix shared by several things is *ambiguous*: the table keeps all
//! of them so the diagnostic can list the candidates. Keys are interned, so a
//! lookup is one hash of the text and one of the symbol.

use axiom_core::diag::closest;
use axiom_core::{Id, Interner, Map, Sym};

use crate::book::Miss;
use crate::scope::{Home, Scope};

/// What a lookup found.
pub(crate) enum Found<T> {
    One(Id<T>),
    Nothing,
    Several(Vec<Id<T>>),
}

pub(crate) struct Names<T> {
    slots: Map<Sym, Slot<T>>,
}

enum Slot<T> {
    One(Id<T>),
    Many(Vec<Id<T>>),
}

impl<T> Slot<T> {
    fn ids(&self) -> &[Id<T>] {
        match self {
            Slot::One(id) => std::slice::from_ref(id),
            Slot::Many(ids) => ids,
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

impl<T> Names<T> {
    /// Registers `id` under every suffix of `path`.
    pub fn insert<'s>(&mut self, names: &mut Interner<'s>, path: &'s str, id: Id<T>) {
        for suffix in suffixes(path) {
            let key = names.intern(suffix);
            match self.slots.get_mut(&key) {
                None => {
                    self.slots.insert(key, Slot::One(id));
                }
                Some(Slot::Many(ids)) => ids.push(id),
                Some(slot @ Slot::One(_)) => {
                    let first = slot.ids()[0];
                    *slot = Slot::Many(vec![first, id]);
                }
            }
        }
    }

    /// Everything that ends with `text`, before any visibility filter.
    pub fn candidates(&self, names: &Interner, text: &str) -> &[Id<T>] {
        names.get(text).and_then(|key| self.slots.get(&key)).map_or(&[], Slot::ids)
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
        let alone = |suffix: &&str| self.candidates(names, suffix) == [id];
        let mut by_length: Vec<&str> = suffixes(path).collect();
        by_length.reverse();
        by_length.into_iter().find(alone).unwrap_or(path)
    }

    /// The known name closest to `text`, among the things `visible` admits.
    fn suggest(&self, names: &Interner, text: &str, visible: &impl Fn(Id<T>) -> bool) -> Option<Sym> {
        let keys = self.slots.iter().filter(|(_, slot)| slot.ids().iter().any(|&id| visible(id)));
        let best = closest(text, keys.map(|(&key, _)| names.name(key)))?;
        names.get(best)
    }

    /// The same table over new ids, after a tree renumbered its nodes.
    pub fn renumbered<U>(self, new_id: &[Id<U>]) -> Names<U> {
        let map = |id: Id<T>| new_id[id.index()];
        let slots = self.slots.into_iter().map(|(key, slot)| {
            let slot = match slot {
                Slot::One(id) => Slot::One(map(id)),
                Slot::Many(ids) => Slot::Many(ids.into_iter().map(map).collect()),
            };
            (key, slot)
        });
        Names { slots: slots.collect() }
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
        let mut table = Scoped { names: Names::default(), homes: Vec::new() };
        for (id, path, home) in things {
            table.names.insert(interner, path, id);
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

    /// The one thing `scope` can see under `text`.
    pub fn resolve(&self, interner: &Interner, scope: &Scope, text: &str) -> Result<Id<T>, Miss<T>> {
        self.names.resolve(interner, text, |id| scope.sees(self.home(id)))
    }

    pub fn renumbered<U>(self, new_id: &[Id<U>]) -> Scoped<U> {
        let mut homes = vec![Home::Builtin; self.homes.len()];
        for (old, &home) in self.homes.iter().enumerate() {
            homes[new_id[old].index()] = home;
        }
        Scoped { names: self.names.renumbered(new_id), homes }
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
            table.insert(names, path, Id::new(i as u32));
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
    fn unknown_names_suggest_the_closest() {
        let mut names = Interner::default();
        let table = table(&mut names, &["assets/bank/checking"]);
        match table.resolve(&names, "chekcing", |_| true) {
            Err(Miss::Unknown { suggestion: Some(s) }) => assert_eq!(names.name(s), "checking"),
            other => panic!("expected a suggestion, got {other:?}"),
        }
    }
}
