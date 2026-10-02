//! Forced placement: the slot each word of a declaration must go in, if every way of placing the words agrees.
//!
//! A declaration's words fill the slots of its kind: `family/529 riley` has three words and the kind three slots. Each
//! word fits the slots whose range admits one of its kinds, and a slot that holds `one` word cannot hold two. A word
//! is placed only if it lands in the same slot in *every* placement of all the words, so the rule never guesses and
//! never takes the first slot that fits. A word that could land in several is ambiguous, and the diagnostic names them
//! and the role word that settles it.
//!
//! There are at most [`MAX_WORDS`] words and sixteen slots, so a set of slots is a `u16` and the whole search is two
//! arrays of eight of them on the stack. First, unit propagation: a word with one candidate slot claims it, and if the
//! slot takes one word, no other word may use it, which can leave another word one candidate in turn. Most
//! declarations are settled there and never branch. The rest take a depth-first search over the words that enumerates
//! every placement and records, for each word, every slot it landed in.
//!
//! Slots that take any number of words never block a word, so landing in one is a single choice for the search: the
//! word lands in all of them at once. Only the slots that take one word are branched on. The search visits a leaf for
//! every way to deal the single slots, so `n` words that all fit all of `k` single slots take `k!/(k-n)!`: 40,320 for
//! eight words and eight slots, which no kind comes near, and the worst case, sixteen, is 5 × 10⁸.

/// The most words one declaration places. A longer one is refused as [`Unplaceable::NoPlacement`], being too many
/// words for any kind to take.
pub const MAX_WORDS: usize = 8;

/// Where one word lands.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Placed {
    /// The slot it lands in whichever way the words are placed.
    Forced(u8),
    /// The slots it lands in under some placement, as a bit set of more than one.
    Ambiguous(u16),
}

/// Why no placement can be made.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Unplaceable {
    /// The word at this index fits no slot.
    NoCandidate(u8),
    /// Each word fits somewhere, but not all at once: too many words for the slots.
    NoPlacement,
}

/// The placed words, in the order they were given.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Placement {
    placed: [Placed; MAX_WORDS],
    words: usize,
}

impl Placement {
    pub fn as_slice(&self) -> &[Placed] {
        &self.placed[..self.words]
    }
}

/// Places the words. `cand[w]` is the set of slots word `w` may fill, one bit a slot, and `single` is the set of slots
/// that take at most one word; the rest take any number.
pub fn place(cand: &[u16], single: u16) -> Result<Placement, Unplaceable> {
    let mut search = Search::new(cand, single)?;
    search.propagate()?;
    if !search.extend(0, 0) {
        return Err(Unplaceable::NoPlacement);
    }
    Ok(search.placement())
}

/// The state of a placement in progress, for every word at once.
struct Search {
    /// The slots each word may still fill; the words are the first `words` entries.
    cand: [u16; MAX_WORDS],
    /// The slots each word was found to land in, under some placement, so far.
    landed: [u16; MAX_WORDS],
    words: usize,
    single: u16,
}

impl Search {
    fn new(cand: &[u16], single: u16) -> Result<Search, Unplaceable> {
        if cand.len() > MAX_WORDS {
            return Err(Unplaceable::NoPlacement);
        }
        if let Some(word) = cand.iter().position(|&slots| slots == 0) {
            return Err(Unplaceable::NoCandidate(word as u8));
        }
        let mut words = [0; MAX_WORDS];
        words[..cand.len()].copy_from_slice(cand);
        Ok(Search { cand: words, landed: [0; MAX_WORDS], words: cand.len(), single })
    }

    /// Takes the slot of every word with only one out of the words that could still use it, until nothing changes.
    fn propagate(&mut self) -> Result<(), Unplaceable> {
        loop {
            let before = self.cand;
            for word in 0..self.words {
                self.claim(word)?;
            }
            if self.cand == before {
                return Ok(());
            }
        }
    }

    /// If `word` has one candidate and it takes one word, every other word loses it. A word left with none is a
    /// conflict the search would find too, but only after enumerating every placement of the words before it.
    fn claim(&mut self, word: usize) -> Result<(), Unplaceable> {
        let slot = self.cand[word];
        if slot.count_ones() != 1 || slot & self.single == 0 {
            return Ok(());
        }
        for other in (0..self.words).filter(|&other| other != word) {
            self.cand[other] &= !slot;
            if self.cand[other] == 0 {
                return Err(Unplaceable::NoPlacement);
            }
        }
        Ok(())
    }

    /// Whether the words from `word` on can be placed when the single slots in `taken` are full. If they can, every
    /// slot each of them can land in, given what came before, is added to `landed`.
    fn extend(&mut self, word: usize, taken: u16) -> bool {
        if word == self.words {
            return true;
        }
        let (any_number, one_each) = (self.cand[word] & !self.single, self.cand[word] & self.single & !taken);
        let mut placeable = false;
        if any_number != 0 && self.extend(word + 1, taken) {
            self.landed[word] |= any_number;
            placeable = true;
        }
        for slot in slots(one_each) {
            if self.extend(word + 1, taken | slot) {
                self.landed[word] |= slot;
                placeable = true;
            }
        }
        placeable
    }

    fn placement(&self) -> Placement {
        let placed = self.landed.map(|landed| match landed.count_ones() {
            1 => Placed::Forced(landed.trailing_zeros() as u8),
            _ => Placed::Ambiguous(landed),
        });
        Placement { placed, words: self.words }
    }
}

/// The slots in `set`, one bit each, lowest first.
fn slots(mut set: u16) -> impl Iterator<Item = u16> {
    std::iter::from_fn(move || {
        let lowest = set & set.wrapping_neg();
        set ^= lowest;
        (lowest != 0).then_some(lowest)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Rng;

    use Placed::{Ambiguous, Forced};

    /// The slots of the 529: `owner` takes a person or household, `beneficiary` a person, `custodian` a broker.
    const OWNER: u16 = 0b001;
    const BENEFICIARY: u16 = 0b010;
    const CUSTODIAN: u16 = 0b100;
    const ALL_TAKE_ONE: u16 = 0b111;

    fn placed(cand: &[u16], single: u16) -> Result<Vec<Placed>, Unplaceable> {
        place(cand, single).map(|placement| placement.as_slice().to_vec())
    }

    #[test]
    fn family_slash_529_with_a_role_word_is_accepted() {
        // The container is a broker, so only a custodian; the household `family` is no person, so only an owner.
        // `beneficiary riley` is a role word and has already filled its slot.
        assert_eq!(placed(&[CUSTODIAN, OWNER], ALL_TAKE_ONE), Ok(vec![Forced(2), Forced(0)]));
    }

    #[test]
    fn riley_slash_529_is_ambiguous_between_owner_and_beneficiary() {
        let riley = OWNER | BENEFICIARY;
        assert_eq!(placed(&[CUSTODIAN, riley], ALL_TAKE_ONE), Ok(vec![Forced(2), Ambiguous(0b011)]));
    }

    #[test]
    fn family_slash_529_riley_is_settled_because_family_takes_owner() {
        let riley = OWNER | BENEFICIARY;
        assert_eq!(placed(&[CUSTODIAN, OWNER, riley], ALL_TAKE_ONE), Ok(vec![Forced(2), Forced(0), Forced(1)]));
        assert_eq!(
            placed(&[riley, OWNER, CUSTODIAN], ALL_TAKE_ONE),
            Ok(vec![Forced(1), Forced(0), Forced(2)]),
            "in any order"
        );
    }

    #[test]
    fn acme_slash_529_fits_no_slot() {
        assert_eq!(placed(&[CUSTODIAN, 0], ALL_TAKE_ONE), Err(Unplaceable::NoCandidate(1)));
        assert_eq!(placed(&[0, 0], ALL_TAKE_ONE), Err(Unplaceable::NoCandidate(0)), "the first is the one named");
    }

    #[test]
    fn two_words_for_one_slot_are_too_many() {
        assert_eq!(placed(&[OWNER, OWNER], ALL_TAKE_ONE), Err(Unplaceable::NoPlacement));
        assert_eq!(
            placed(&[OWNER | CUSTODIAN, OWNER | CUSTODIAN, CUSTODIAN | OWNER], ALL_TAKE_ONE),
            Err(Unplaceable::NoPlacement)
        );
        assert_eq!(placed(&[OWNER; MAX_WORDS + 1], 0), Err(Unplaceable::NoPlacement), "longer than any declaration");
    }

    #[test]
    fn slots_that_take_any_number_never_block() {
        let tags = 0b01; // `some` or `many`: no bit in `single`
        assert_eq!(placed(&[tags, tags, tags], 0b10), Ok(vec![Forced(0); 3]));
        assert_eq!(placed(&[0b11, 0b11], 0b10), Ok(vec![Ambiguous(0b11), Ambiguous(0b11)]));
        assert_eq!(placed(&[0b01, 0b11], 0b10), Ok(vec![Forced(0), Ambiguous(0b11)]));
        assert_eq!(
            placed(&[0b10, 0b11], 0b10),
            Ok(vec![Forced(1), Forced(0)]),
            "the one slot taken, the other goes elsewhere"
        );
    }

    #[test]
    fn no_words_place_trivially() {
        assert_eq!(placed(&[], 0), Ok(vec![]));
    }

    #[test]
    fn eight_words_over_eight_slots_all_ambiguous_take_every_dealing() {
        let all = [0xFF; 8];
        assert_eq!(placed(&all, 0xFF), Ok(vec![Ambiguous(0xFF); 8]));
    }

    /// Every placement, found by trying every slot of every word.
    fn all_placements(cand: &[u16], single: u16) -> Vec<Vec<u8>> {
        let Some((&first, rest)) = cand.split_first() else { return vec![vec![]] };
        let mut found = Vec::new();
        for slot in (0..16).filter(|slot| first >> slot & 1 == 1) {
            for mut tail in all_placements(rest, single) {
                tail.insert(0, slot);
                found.push(tail);
            }
        }
        // Keep those that put at most one word in each single slot.
        found.retain(|placement| {
            (0..16).all(|slot| single >> slot & 1 == 0 || placement.iter().filter(|&&s| s == slot).count() <= 1)
        });
        found
    }

    /// What `place` must say, by looking at every placement.
    fn by_enumeration(cand: &[u16], single: u16) -> Result<Vec<Placed>, Unplaceable> {
        if let Some(word) = cand.iter().position(|&slots| slots == 0) {
            return Err(Unplaceable::NoCandidate(word as u8));
        }
        let placements = all_placements(cand, single);
        if placements.is_empty() {
            return Err(Unplaceable::NoPlacement);
        }
        Ok((0..cand.len())
            .map(|word| {
                let landed = placements.iter().fold(0u16, |landed, placement| landed | 1 << placement[word]);
                if landed.count_ones() == 1 { Forced(landed.trailing_zeros() as u8) } else { Ambiguous(landed) }
            })
            .collect())
    }

    #[test]
    fn every_small_instance_agrees_with_enumerating_the_placements() {
        let mut rng = Rng::new(0x9E37_79B9_7F4A_7C15);
        // How often each kind of answer came up, so that the instances are known to reach all of them.
        let (mut forced_all, mut some_ambiguous, mut no_candidate, mut no_placement) = (0, 0, 0, 0);
        for case in 0..3000 {
            let slots = 1 + rng.below(5);
            let words = rng.below(if slots <= 3 { 9 } else { 7 });
            let mask = (1u16 << slots) - 1;
            let sparse = rng.below(2) == 0;
            let mut candidates = || rng.next() as u16 & if sparse { rng.next() as u16 } else { !0 } & mask;
            let cand: Vec<u16> = (0..words).map(|_| candidates()).collect();
            let single = rng.next() as u16 & mask;

            let answer = placed(&cand, single);
            assert_eq!(answer, by_enumeration(&cand, single), "case {case}: {cand:?} single {single:#b}");
            match answer {
                Ok(words) if words.iter().all(|word| matches!(word, Forced(_))) => forced_all += 1,
                Ok(_) => some_ambiguous += 1,
                Err(Unplaceable::NoCandidate(_)) => no_candidate += 1,
                Err(Unplaceable::NoPlacement) => no_placement += 1,
            }
        }
        assert!(forced_all > 200 && some_ambiguous > 200 && no_candidate > 100 && no_placement > 100);
    }
}
