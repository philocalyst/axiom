//! Addresses: how the words of a reference find the account they mean.
//!
//! An account's **address** is the entities that fill its slots, owner first and the custodian last, and then its name:
//! `jordan/bluefin/fidelity/401k`. A **reference** is any run of those words, in order, that ends in the name and
//! means one account that is open on the line's day: `jordan/401k` is *the* 401(k) with jordan among its fillers.
//!
//! # Why this layout
//!
//! This is a search engine's inverted index. For each entity there is one posting list, the accounts it fills a slot of,
//! sorted by number, and for each name another, the accounts called it. A reference is the intersection of its words'
//! lists, which `core::postings` makes by galloping from the shortest one, so the cost is that of the shortest list and
//! not of the longest: `me` owns most of a household's accounts and a name belongs to a few. What is left is checked for
//! the order of the words and for being open, and both are a few comparisons on the handful of accounts that survive.
//!
//! The lists hold each account once, however many slots an entity fills, because the merge needs strictly increasing
//! ids; a second list beside the first says, one bit for each place in the address, where the entity stands, which is all
//! the order check reads. Each account's own address is a third table, for the diagnostics and for the shortest unique
//! address: the words that write only this account.

use axiom_core::{Day, Days, Groups, Id, Many, Map, Sym};

use crate::book::{Book, Entity, Place, Role};
use crate::builtin;
use crate::fill::holds_one;
use crate::holders::Holder;
use crate::names::Found;
use crate::slots::Slot;

/// The most words an address holds. The bits that say where an entity stands are a `u16`.
pub(crate) const MAX_WORDS: usize = 16;

/// What a name is the name of: a marker for the key of the lists of accounts by name.
pub(crate) struct Called;

/// One word of an address.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Part {
    /// An entity that fills a slot of the account.
    Filler(Id<Entity>),
    /// The account's own name, the last word.
    Name(Sym),
}

/// An account as the index is built from it: where it is, what it is called and filled by, and when it is open.
pub(crate) struct Account<'a> {
    pub place: Id<Place>,
    /// The words of its address: the fillers in order, then `Part::Name`.
    pub address: &'a [Part],
    /// The days it is open, or none if there are none (it closes before it opens).
    pub open: Option<Days>,
}

/// Every account's address as a set of posting lists, and what a journal's references mean.
#[derive(Default)]
pub(crate) struct Addresses {
    /// For each entity, the accounts it fills a slot of, by place number.
    fills: Groups<Entity, u32>,
    /// Beside `fills`: the places the entity stands in that account's address, one bit each.
    stands: Groups<Entity, u16>,
    /// For each name, the accounts called it.
    called: Groups<Called, u32>,
    /// Each account's address, by place number; empty for a place that is no account.
    address: Groups<Place, Part>,
    /// By place number: the days the account is open.
    open: Vec<Option<Days>>,
    /// The references a journal writes whose meaning no day or home can change, each worked out once, by the text as
    /// written: the account it means. See `World::settle_addresses`.
    once: Map<Sym, Id<Place>>,
    /// Whether the book writes some account with the entities that fill its slots before its name. A book that does not
    /// reads every reference as it always did: the index is asked for none, and no mention is kept from being a party.
    used: bool,
}

impl Addresses {
    /// The index of every account a book declares, once the facts that say what fills their slots are frozen.
    pub fn of(book: &Book) -> Addresses {
        let mut written = Vec::new();
        let mut starts = Vec::new();
        for (place, account) in book.places.iter() {
            // An account is declared, with a line to point at; the places a path makes for its prefixes are not.
            if account.loc.is_none() || !matches!(account.role, Role::Account { .. }) {
                continue;
            }
            // Its name was interned with every suffix of its path, when the places were made.
            let Some(name) = book.name(account.path).rsplit('/').next().and_then(|name| book.names.get(name)) else {
                continue;
            };
            starts.push((place, written.len()));
            written.extend(fillers(book, place).map(Part::Filler));
            written.push(Part::Name(name));
        }
        let ends = starts.iter().skip(1).map(|&(_, start)| start).chain([written.len()]);
        let accounts: Vec<Account<'_>> = starts
            .iter()
            .zip(ends)
            .map(|(&(place, start), end)| Account {
                place,
                address: &written[start..end],
                open: open_days(book, place),
            })
            .collect();
        let used = book.places.iter().any(|(place, _)| book.is_spelled(place));
        Addresses { used, ..Addresses::build(book.entities.len(), book.names.len(), book.places.len(), &accounts) }
    }

    /// Whether the book writes some account as an address; if it does not, nothing is read as one.
    pub fn is_used(&self) -> bool {
        self.used
    }

    /// The index of `accounts`, in the order of their places. `entities`, `names` and `places` are how many of each
    /// there are, which are the lengths of the tables.
    pub fn build(entities: usize, names: usize, places: usize, accounts: &[Account<'_>]) -> Addresses {
        debug_assert!(accounts.windows(2).all(|pair| pair[0].place < pair[1].place), "in the order of the places");
        let mut fills = Vec::new();
        let mut called = Vec::new();
        let mut written = Vec::new();
        let mut open = vec![None; places];
        for account in accounts {
            let Some((&Part::Name(name), fillers)) = account.address.split_last() else {
                unreachable!("an address ends in its name")
            };
            assert!(fillers.len() < MAX_WORDS, "an address has at most {MAX_WORDS} words");
            for entity in distinct(fillers) {
                fills.push((
                    Id::<Entity>::new(entity.index() as u32),
                    account.place.index() as u32,
                    stands(fillers, entity),
                ));
            }
            called.push((Id::<Called>::new(name.index() as u32), account.place.index() as u32));
            written.extend(account.address.iter().map(|&part| (account.place, part)));
            open[account.place.index()] = account.open;
        }
        Addresses {
            fills: Groups::build(entities, fills.iter().map(|&(entity, place, _)| (entity, place))),
            stands: Groups::build(entities, fills.iter().map(|&(entity, _, at)| (entity, at))),
            called: Groups::build(names, called.into_iter()),
            address: Groups::build(places, written.into_iter()),
            open,
            once: Map::default(),
            used: false,
        }
    }

    /// The accounts a reference means: the fillers it names, in order, and the name it ends in, among the accounts
    /// open on `day`, or among all of them with no day.
    pub fn resolve(&self, fillers: &[Id<Entity>], name: Sym, day: Option<Day>) -> Found<Place> {
        let called = &self.called[Id::new(name.index() as u32)];
        let mut found = Vec::new();
        match fillers {
            [] => found.extend_from_slice(called),
            &[only] => axiom_core::postings::intersect(&self.fills[only], called, &mut found),
            many => {
                let mut lists: Vec<&[u32]> = many.iter().map(|&entity| &self.fills[entity]).collect();
                lists.push(called);
                axiom_core::postings::intersect_all(&mut lists, &mut found);
            }
        }
        found.retain(|&place| self.stands_in_order(place, fillers) && self.is_open(Id::new(place), day));
        match found.as_slice() {
            [] => Found::Nothing,
            &[only] => Found::One(Id::new(only)),
            several => Found::Several(several.iter().map(|&place| Id::new(place)).collect()),
        }
    }

    /// Whether `fillers` stand in the address of the account `place`, in the order they are given: the first at its
    /// lowest place, each next at the lowest after that. Every one is in the account's list, by the intersection.
    fn stands_in_order(&self, place: u32, fillers: &[Id<Entity>]) -> bool {
        let mut from = 0;
        for &entity in fillers {
            let at = self.fills[entity].binary_search(&place).expect("the account is in the list of each filler");
            let later = u32::from(self.stands[entity][at]) & (u32::MAX << from);
            if later == 0 {
                return false;
            }
            from = later.trailing_zeros() + 1;
        }
        true
    }

    /// Whether the account is open on every day there is, so that no line's day can rule it out.
    pub fn is_always_open(&self, place: Id<Place>) -> bool {
        self.open[place.index()] == Some(Days::ALWAYS)
    }

    /// What the text of a reference means, if it was worked out once for every day.
    pub fn settled(&self, text: Sym) -> Option<Id<Place>> {
        self.once.get(&text).copied()
    }

    /// Keeps what each text means, worked out once.
    pub fn settle(&mut self, once: Map<Sym, Id<Place>>) {
        self.once = once;
    }

    /// Whether the account is open on `day`; with no day, whether it is ever open.
    fn is_open(&self, place: Id<Place>, day: Option<Day>) -> bool {
        match (self.open[place.index()], day) {
            (Some(days), Some(day)) => days.contains(day),
            (open, None) => open.is_some(),
            (None, Some(_)) => false,
        }
    }

    /// The address of an account, in order, and the name last. Empty for a place that is no account.
    pub fn address(&self, place: Id<Place>) -> &[Part] {
        &self.address[place]
    }

    /// The accounts an entity fills a slot of, by place number.
    pub fn filled_by(&self, entity: Id<Entity>) -> Vec<Id<Place>> {
        self.fills[entity].iter().map(|&place| Id::new(place)).collect()
    }

    /// The shortest reference that `means_it` says is this account and no other: the fewest of its fillers, the leftmost
    /// among equals, then its name. The whole address, if nothing shorter does.
    pub fn shortest_that(&self, place: Id<Place>, means_it: impl Fn(&[Id<Entity>], Sym) -> bool) -> Vec<Part> {
        let address = self.address(place);
        let Some((&Part::Name(name), fillers)) = address.split_last() else { return Vec::new() };
        let entities: Vec<Id<Entity>> = fillers
            .iter()
            .map(|part| match part {
                Part::Filler(entity) => *entity,
                Part::Name(_) => unreachable!("only the last word of an address is its name"),
            })
            .collect();
        // Fewest fillers first, and among as many, the leftmost: the owner before the custodian.
        let mut subsets: Vec<u16> = (0..1u32 << entities.len()).map(|set| set as u16).collect();
        subsets.sort_by_key(|set| (set.count_ones(), !set.reverse_bits()));
        let chosen_by = |set: &u16| -> Vec<Id<Entity>> {
            (0..entities.len()).filter(|&at| set >> at & 1 == 1).map(|at| entities[at]).collect()
        };
        let chosen = subsets.iter().map(chosen_by).find(|chosen| means_it(chosen, name)).unwrap_or(entities);
        chosen.into_iter().map(Part::Filler).chain([Part::Name(name)]).collect()
    }

    /// The shortest reference that the index alone takes to `place` among the accounts open on `day`.
    #[cfg(test)]
    fn shortest(&self, place: Id<Place>, day: Option<Day>) -> Vec<Part> {
        self.shortest_that(
            place,
            |fillers, name| matches!(self.resolve(fillers, name, day), Found::One(only) if only == place),
        )
    }

    /// How many accounts have an address.
    #[cfg(test)]
    fn accounts(&self) -> usize {
        (0..self.address.keys()).filter(|&place| !self.address[Id::new(place as u32)].is_empty()).count()
    }
}

/// The entities that fill the slots of the account `place`, owners first, the custodian last: the words of its address
/// before its name. An entity whose path has a `/` cannot be written as one word of a path, and is left out.
fn fillers<'b>(book: &'b Book, place: Id<Place>) -> impl Iterator<Item = Id<Entity>> + 'b {
    let account = &book.places[place];
    let owners: Vec<Id<Entity>> = match account.shares.is_empty() {
        true => vec![account.owner],
        false => account.shares.iter().map(|share| share.entity).collect(),
    };
    let slots = book.schema.entity_slots(&book.kinds, account.kind);
    let by_slot = slots.into_iter().flat_map(move |(number, slot)| slot_fillers(book, place, number, slot));
    let institution = match account.role {
        Role::Account { institution } => institution,
        _ => None,
    };
    let all = owners.into_iter().chain(by_slot).chain(institution);
    all.filter(|&entity| !book.name(book.entities[entity].path).contains('/')).take(MAX_WORDS - 1)
}

/// What fills a slot of the account: its own line's, else the nearest kind's.
fn slot_fillers(book: &Book, place: Id<Place>, number: axiom_core::SlotId, slot: Slot) -> Vec<Id<Entity>> {
    let kind = book.places[place].kind;
    let holders = std::iter::once(Holder::Place(place)).chain(book.kinds.lineage(kind).map(Holder::Kind));
    let said =
        holders.into_iter().find_map(|holder| book.facts.datum_at(number, book.holders.number(holder), Day::MIN));
    match (said, holds_one(slot.mult)) {
        (Some(datum), true) => datum.read::<Id<Entity>>().into_iter().collect(),
        (Some(datum), false) => {
            datum.read::<Many<Id<Entity>>>().map_or_else(Vec::new, |set| book.facts.members(set).collect())
        }
        (None, _) => Vec::new(),
    }
}

/// The days an account is open: from the day it opens to the day it closes, each as its lines say.
fn open_days(book: &Book, place: Id<Place>) -> Option<Days> {
    let opened = book.fact(builtin::OPENED, place).unwrap_or(Day::MIN);
    Days::new(opened, book.fact(builtin::CLOSED, place).unwrap_or(Day::MAX))
}

/// The entities of `fillers`, each once, in the order they first appear.
fn distinct(fillers: &[Part]) -> impl Iterator<Item = Id<Entity>> + '_ {
    let mut seen: Vec<Id<Entity>> = Vec::new();
    fillers.iter().filter_map(move |part| match *part {
        Part::Filler(entity) if !seen.contains(&entity) => {
            seen.push(entity);
            Some(entity)
        }
        _ => None,
    })
}

/// The places an entity stands in an address, one bit each.
fn stands(address: &[Part], entity: Id<Entity>) -> u16 {
    address
        .iter()
        .enumerate()
        .filter(|&(_, &part)| part == Part::Filler(entity))
        .fold(0, |bits, (at, _)| bits | 1 << at)
}

#[cfg(test)]
mod tests {
    use axiom_core::Interner;

    use super::*;

    const ME: u32 = 0;
    const JORDAN: u32 = 1;
    const BLUEFIN: u32 = 2;
    const FIDELITY: u32 = 3;
    const FAMILY: u32 = 4;
    const ENTITIES: usize = 5;
    const CHECKING: u32 = 0;
    const FOUR01K: u32 = 1;

    fn entity(number: u32) -> Id<Entity> {
        Id::new(number)
    }

    /// The symbol of the `number`th name: interned in the same order every time, so the same number is the same symbol.
    fn name(number: u32) -> Sym {
        let mut names = Interner::default();
        ["n0", "n1", "n2", "n3"].map(|text| names.intern(text))[number as usize]
    }

    /// An account in `place`, with fillers by number and then its name.
    fn account(place: u32, fillers: &[u32], called: u32, open: Option<Days>) -> (Id<Place>, Vec<Part>, Option<Days>) {
        let mut address: Vec<Part> = fillers.iter().map(|&number| Part::Filler(entity(number))).collect();
        address.push(Part::Name(name(called)));
        (Id::new(place), address, open)
    }

    fn index(accounts: &[(Id<Place>, Vec<Part>, Option<Days>)], places: usize) -> Addresses {
        let accounts: Vec<Account<'_>> =
            accounts.iter().map(|(place, address, open)| Account { place: *place, address, open: *open }).collect();
        Addresses::build(ENTITIES, 4, places, &accounts)
    }

    fn found(index: &Addresses, fillers: &[u32], called: u32, day: Option<Day>) -> Vec<u32> {
        let fillers: Vec<Id<Entity>> = fillers.iter().map(|&number| entity(number)).collect();
        match index.resolve(&fillers, name(called), day) {
            Found::One(only) => vec![only.index() as u32],
            Found::Nothing => Vec::new(),
            Found::Several(places) => places.iter().map(|place| place.index() as u32).collect(),
        }
    }

    fn days(first: i32, last: i32) -> Option<Days> {
        Days::new(Day(first), Day(last))
    }

    /// me's checking, me's and jordan's 401(k)s (jordan's sponsored by bluefin, held at fidelity), the family's checking.
    fn household() -> Addresses {
        let always = Some(Days::ALWAYS);
        index(
            &[
                account(1, &[ME], CHECKING, always),
                account(2, &[ME, FIDELITY], FOUR01K, always),
                account(4, &[JORDAN, BLUEFIN, FIDELITY], FOUR01K, always),
                account(5, &[FAMILY], CHECKING, always),
            ],
            6,
        )
    }

    #[test]
    fn a_reference_is_a_run_of_the_address_that_ends_in_the_name() {
        let index = household();
        assert_eq!(found(&index, &[JORDAN, BLUEFIN, FIDELITY], FOUR01K, None), [4], "the whole address");
        assert_eq!(found(&index, &[JORDAN, FIDELITY], FOUR01K, None), [4], "skipping the sponsor");
        assert_eq!(found(&index, &[BLUEFIN], FOUR01K, None), [4], "one filler is enough when it is the only one");
        assert_eq!(found(&index, &[JORDAN], FOUR01K, None), [4]);
        assert_eq!(found(&index, &[], FOUR01K, None), [2, 4], "the name alone is every account called it");
        assert_eq!(found(&index, &[FAMILY], FOUR01K, None), [], "nothing family fills is a 401k");
    }

    #[test]
    fn the_words_of_a_reference_keep_the_order_of_the_address() {
        let index = household();
        assert_eq!(found(&index, &[FIDELITY, JORDAN], FOUR01K, None), [], "the custodian is after the owner");
        assert_eq!(found(&index, &[BLUEFIN, JORDAN], FOUR01K, None), []);
        assert_eq!(found(&index, &[ME, FIDELITY], FOUR01K, None), [2]);
        assert_eq!(found(&index, &[FIDELITY], FOUR01K, None), [2, 4], "both are held at fidelity");
    }

    #[test]
    fn several_accounts_that_share_the_words_are_all_found() {
        let index = household();
        assert_eq!(found(&index, &[], CHECKING, None), [1, 5]);
        assert_eq!(found(&index, &[ME], CHECKING, None), [1]);
        assert_eq!(found(&index, &[FAMILY], CHECKING, None), [5]);
    }

    #[test]
    fn an_entity_that_stands_twice_matches_a_reference_that_names_it_twice() {
        let index = index(&[account(0, &[FAMILY, FIDELITY, FAMILY], FOUR01K, Some(Days::ALWAYS))], 1);
        assert_eq!(found(&index, &[FAMILY, FAMILY], FOUR01K, None), [0]);
        assert_eq!(found(&index, &[FAMILY, FIDELITY, FAMILY], FOUR01K, None), [0]);
        assert_eq!(found(&index, &[FAMILY, FAMILY, FAMILY], FOUR01K, None), [], "it stands only twice");
        assert_eq!(found(&index, &[FIDELITY, FAMILY, FAMILY], FOUR01K, None), [], "and not after the custodian twice");
    }

    #[test]
    fn a_sibling_that_opens_later_does_not_make_the_earlier_days_ambiguous() {
        let index =
            index(&[account(0, &[ME], FOUR01K, days(0, 400)), account(1, &[JORDAN], FOUR01K, days(200, 900))], 2);
        assert_eq!(found(&index, &[], FOUR01K, Some(Day(100))), [0], "only the first exists yet");
        assert_eq!(found(&index, &[], FOUR01K, Some(Day(300))), [0, 1], "both are open");
        assert_eq!(found(&index, &[], FOUR01K, Some(Day(500))), [1], "the first has closed");
        assert_eq!(found(&index, &[], FOUR01K, Some(Day(1000))), [], "neither is open");
        assert_eq!(found(&index, &[], FOUR01K, None), [0, 1], "with no day, every account that is ever open");
    }

    #[test]
    fn an_account_that_is_never_open_is_never_found() {
        let index = index(&[account(0, &[ME], FOUR01K, None)], 1);
        assert_eq!(found(&index, &[ME], FOUR01K, None), []);
        assert_eq!(found(&index, &[ME], FOUR01K, Some(Day(0))), []);
    }

    #[test]
    fn the_shortest_address_is_the_fewest_fillers_and_the_leftmost() {
        let index = household();
        let shortest = |place: u32, day| {
            let parts = index.shortest(Id::new(place), day);
            parts
                .iter()
                .map(|part| match part {
                    Part::Filler(entity) => format!("e{}", entity.index()),
                    Part::Name(sym) => format!("n{}", sym.index()),
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(shortest(4, None), ["e1", "n1"], "jordan alone separates the two 401(k)s, and is leftmost");
        assert_eq!(shortest(2, None), ["e0", "n1"], "me");
        assert_eq!(shortest(1, None), ["e0", "n0"], "me/checking: the name alone is not enough");
        assert_eq!(shortest(5, None), ["e4", "n0"]);
    }

    #[test]
    fn the_shortest_address_may_be_the_name_when_a_sibling_has_not_opened() {
        let index =
            index(&[account(0, &[ME], FOUR01K, days(0, 900)), account(1, &[JORDAN], FOUR01K, days(200, 900))], 2);
        assert_eq!(
            index.shortest(Id::new(0), Some(Day(100))),
            [Part::Name(name(FOUR01K))],
            "on 100 the name is enough"
        );
        assert_eq!(index.shortest(Id::new(0), Some(Day(300))), [Part::Filler(entity(ME)), Part::Name(name(FOUR01K))]);
    }

    /// Accounts made by a generator, and the references that every run of their addresses makes, against what a person
    /// would do with a pencil: look at each account's words.
    #[test]
    fn every_reference_agrees_with_reading_the_addresses_one_by_one() {
        let mut seed = 0x9E37_79B9_7F4A_7C15u64;
        let mut below = move |n: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % n as u64) as usize
        };
        let mut checked = 0;
        for _ in 0..300 {
            let accounts: Vec<_> = (0..1 + below(8))
                .map(|place| {
                    let fillers: Vec<u32> = (0..below(5)).map(|_| below(ENTITIES) as u32).collect();
                    let first = below(300) as i32;
                    account(place as u32, &fillers, below(3) as u32, days(first, first + below(300) as i32))
                })
                .collect();
            let index = index(&accounts, accounts.len());
            for _ in 0..30 {
                let fillers: Vec<u32> = (0..below(4)).map(|_| below(ENTITIES) as u32).collect();
                let (called, day) = (below(3) as u32, [None, Some(Day(below(600) as i32))][below(2)]);
                let expected: Vec<u32> = accounts
                    .iter()
                    .filter(|(_, address, open)| {
                        let (last, words) = address.split_last().expect("an address");
                        let mut rest = words.iter();
                        let in_order =
                            fillers.iter().all(|&number| rest.any(|part| *part == Part::Filler(entity(number))));
                        let open = match (open, day) {
                            (Some(days), Some(day)) => days.contains(day),
                            (open, None) => open.is_some(),
                            (None, Some(_)) => false,
                        };
                        *last == Part::Name(name(called)) && in_order && open
                    })
                    .map(|(place, _, _)| place.index() as u32)
                    .collect();
                assert_eq!(found(&index, &fillers, called, day), expected, "{fillers:?} {called} {day:?}");
                checked += 1;
            }
            assert_eq!(index.accounts(), accounts.len());
        }
        assert_eq!(checked, 9000);
    }
}
