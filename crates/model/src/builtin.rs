//! The language's own slots: what its property lines say, as facts.
//!
//! `select fifo`, `liquidity 3m`, `deferred` and the rest are lines the language itself reads, and what they say of a
//! thing is a fact like any other: a kind says it of the things of the kind, a thing says it of itself, and the thing
//! that says nothing has what its nearest kind says. The slots a kind declares with `has` are numbered by the schema
//! from where these end.
//!
//! # Why constants
//!
//! The language's own slots are a closed set that the model writes and the engine reads, so each is a typed
//! [`Key`] that is a constant: a read and the write it reads cannot disagree on the type, and the compiler checks it. A
//! slot a kind declares is another matter, which the schema types and a datum carries.
//!
//! # Words
//!
//! A fact holds numbers, days, ids and sets of them, and `fifo` is none of those. A property that is one of a few
//! words is held as the place of its word in the set, a [`Coded`] value, and read back into its enum here, so that
//! nothing else knows the numbering.

use axiom_core::{Day, Id, Key, Many, Ratio, SlotId, Span, Sym};
use axiom_syntax::Policy;

use crate::book::{Basis, Books, Commodity, Entity, Place, Purpose, System};

/// Declares the keys, numbered in the order written.
macro_rules! own_slots {
    ($($(#[$doc:meta])* $key:ident: $ty:ty;)*) => {
        #[allow(non_camel_case_types, clippy::upper_case_acronyms)]
        enum Number { $($key),* }

        $($(#[$doc])* pub const $key: Key<$ty> = Key::new(SlotId(Number::$key as u32));)*

        /// How many slots the language has of its own: a slot a kind declares is numbered after them.
        pub const COUNT: u32 = [$(stringify!($key)),*].len() as u32;
    };
}

own_slots! {
    /// The commodities a place may hold. A set of none is "any".
    HOLDS: Many<Id<Commodity>>;
    /// How parcels are relieved: a [`Policy`], coded.
    SELECT: u32;
    /// The day a place opened.
    OPENED: Day;
    /// The day a place closed.
    CLOSED: Day;
    /// How long it takes to turn a place or a commodity into money.
    LIQUIDITY: Span;
    /// Gains are not realized inside.
    DEFERRED: bool;
    /// What basis arriving value takes: a [`Basis`], coded.
    BASIS: u32;
    /// A place holds what others owe: its parcels stay apart.
    CLAIM: bool;
    /// An entity's money stays tied to it: spending it is the entity's to govern.
    RESTRICTED: bool;
    /// The household an entity belongs to.
    MEMBER: Id<Entity>;
    /// The currency an entity counts in.
    CURRENCY: Id<Commodity>;
    /// The systems that tax an entity wherever it lives.
    CITIZEN: Many<Id<System>>;
    /// When a claim is income or spending: [`Books`], coded.
    BOOKS: u32;
    /// The systems an entity lives under, on each day.
    LIVES: Many<Id<System>>;
    /// An entity's place as a flow's end, where it is not the one its name gives.
    VIA: Id<Place>;
    /// What a commodity is called in full.
    TITLE: Sym;
    /// How a commodity grows, a year at a time.
    GROWS: Ratio;
    /// What flows with a party or its kind are for.
    PURPOSE: Id<Purpose>;
    /// What a commodity kind's issuer pays is for.
    PAYS: Id<Purpose>;
    /// What an account kind takes a flow of one purpose as another: pairs of the purpose and what it becomes.
    TAKES: Many<(u32, u32)>;
    /// The tax inside every price paid to the parties of a kind.
    SALES_TAX: Ratio;
    /// Whom the flows with the parties of a kind are shared with.
    SHARE: Many<Id<Entity>>;
}

/// A property that is one of a few words, held as the place of its word.
pub trait Coded: Copy + PartialEq + 'static {
    /// The words, in the order that numbers them.
    const SET: &'static [Self];

    /// The number a fact holds.
    fn code(self) -> u32 {
        Self::SET.iter().position(|&word| word == self).expect("a word of its set") as u32
    }

    /// The word a fact's number is.
    fn decode(code: u32) -> Option<Self> {
        Self::SET.get(code as usize).copied()
    }
}

impl Coded for Policy {
    const SET: &'static [Policy] = &[Policy::Fifo, Policy::Lifo, Policy::Hifo, Policy::Prorata];
}

impl Coded for Basis {
    const SET: &'static [Basis] = &[Basis::Cost, Basis::Zero];
}

impl Coded for Books {
    const SET: &'static [Books] = &[Books::Cash, Books::Accrual];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_keys_are_numbered_in_the_order_written_from_zero() {
        assert_eq!(HOLDS.slot(), SlotId(0));
        assert_eq!(SHARE.slot(), SlotId(COUNT - 1));
        assert_eq!(COUNT, 22);
    }

    #[test]
    fn a_word_is_its_place_in_its_set_and_back() {
        for &policy in <Policy as Coded>::SET {
            assert_eq!(Policy::decode(policy.code()), Some(policy));
        }
        assert_eq!(Basis::Zero.code(), 1);
        assert_eq!(Basis::decode(1), Some(Basis::Zero));
        assert_eq!(Basis::decode(2), None, "a number that is no word of the set");
    }
}
