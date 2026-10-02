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

use axiom_core::{Day, Id, Key, Many, SlotId, Span};
use axiom_syntax::Policy;

use crate::book::{Basis, Commodity};

/// Declares the keys, numbered in the order written.
macro_rules! own_slots {
    ($($(#[$doc:meta])* $key:ident: $ty:ty;)*) => {
        #[allow(non_camel_case_types)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_keys_are_numbered_in_the_order_written_from_zero() {
        assert_eq!(HOLDS.slot(), SlotId(0));
        assert_eq!(CLAIM.slot(), SlotId(COUNT - 1));
        assert_eq!(COUNT, 8);
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
