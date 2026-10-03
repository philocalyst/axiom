//! The owner of every text a session has read: the reason a session can have a text that changes at all.
//!
//! A `Book<'s>` borrows its names from the text it was built from, so the text of the next book, after an edit, has
//! to live as long as that book does, while the old book still reads the old one. A `Vec` cannot hold such texts
//! (pushing may move what the old book points at), and neither can anything behind `&mut` (the book holds a shared
//! borrow). What can is a vector that grows through a *shared* reference and never moves what it holds.
//!
//! That is a vector of doubling buckets of `OnceLock` slots. A bucket is allocated once, when the first text that
//! belongs in it arrives, and a slot is written once, so nothing that has been handed out is ever moved or touched
//! again: [`Texts::keep`] returns a reference that is good for as long as the arena is. An index finds its slot with
//! a shift and a subtraction (no scan, no chain to walk), the one atomic counter hands every text its own slot, and
//! the only synchronisation is the `OnceLock` each slot already is, so the arena is `Sync` and no `unsafe` is needed.
//!
//! Nothing is freed before the arena is: an applied edit leaves the old text of its file here. That costs the bytes
//! of the file, not of a book: the book is dropped when the session replaces it.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::SourceFile;

/// Slots in the first bucket, as a power of two: a project of a few dozen files fills one or two buckets.
const FIRST_BITS: u32 = 3;
/// Each bucket is twice the one before, so this many hold more texts than an index can name.
const BUCKETS: usize = 48;

/// Every text of a session, kept as it was read.
pub struct Texts {
    buckets: [OnceLock<Box<[OnceLock<SourceFile>]>>; BUCKETS],
    /// Slots handed out, and so the slot the next text takes.
    taken: AtomicUsize,
}

impl Default for Texts {
    fn default() -> Texts {
        Texts { buckets: [const { OnceLock::new() }; BUCKETS], taken: AtomicUsize::new(0) }
    }
}

impl Texts {
    /// Adds `file` and gives it back, where it will stay for as long as `self` does. Texts added at the same moment by
    /// several threads each get a slot of their own.
    pub fn keep(&self, file: SourceFile) -> &SourceFile {
        let (bucket, slot) = place(self.taken.fetch_add(1, Ordering::Relaxed));
        let slots = self.buckets[bucket]
            .get_or_init(|| (0..1usize << (FIRST_BITS + bucket as u32)).map(|_| OnceLock::new()).collect());
        slots[slot].get_or_init(|| file)
    }

    /// How many texts have been kept: every text of every project file, appended data file and edit so far.
    pub fn len(&self) -> usize {
        self.taken.load(Ordering::Relaxed)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// The bucket and the slot in it of the text kept `n`th. Counting the first bucket's slots in with the index turns
/// the buckets into the binary numbers of one length: the bucket is how far the number's top bit is from the first
/// bucket's, and the slot is what is left of the number below that bit.
fn place(n: usize) -> (usize, usize) {
    let numbered = n + (1 << FIRST_BITS);
    let top = numbered.ilog2();
    ((top - FIRST_BITS) as usize, numbered - (1 << top))
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use axiom_core::FileId;

    use super::*;

    fn file(id: u16, text: &str) -> SourceFile {
        SourceFile::new(FileId(id), Cow::Borrowed("a.ax"), Cow::Owned(text.to_string()), false)
    }

    #[test]
    fn every_index_has_its_own_slot_and_the_buckets_double() {
        let places: Vec<_> = (0..40).map(place).collect();
        // 8, then 16, then 32 slots; the slots of a bucket count up from nothing.
        assert_eq!(places[0], (0, 0));
        assert_eq!(places[7], (0, 7));
        assert_eq!(places[8], (1, 0));
        assert_eq!(places[23], (1, 15));
        assert_eq!(places[24], (2, 0));
        assert_eq!(places[39], (2, 15));
        let mut all = places.clone();
        all.dedup();
        assert_eq!(all.len(), places.len(), "no two indices share a slot");
        let last = place(usize::MAX >> 16);
        assert!(last.0 < BUCKETS, "an index the arena can be asked for has a bucket: {last:?}");
    }

    #[test]
    fn what_is_kept_stays_where_it_is_while_more_is_added() {
        let texts = Texts::default();
        let first = texts.keep(file(0, "first"));
        let address = first as *const SourceFile;
        // Far past the first two buckets, so that growing has had every chance to move it.
        let later: Vec<&SourceFile> = (1..=200).map(|n| texts.keep(file(n, &format!("text {n}")))).collect();
        assert_eq!(first as *const SourceFile, address);
        assert_eq!(&*first.text, "first");
        for (n, kept) in later.iter().enumerate() {
            assert_eq!(&*kept.text, format!("text {}", n + 1));
        }
        assert_eq!(texts.len(), 201);
    }

    #[test]
    fn texts_kept_by_many_threads_each_keep_a_slot_of_their_own() {
        let texts = Texts::default();
        let kept: Vec<Vec<&SourceFile>> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..4u16)
                .map(|thread| {
                    let texts = &texts;
                    scope.spawn(move || (0..100).map(|n| texts.keep(file(thread * 100 + n, "t"))).collect::<Vec<_>>())
                })
                .collect();
            handles.into_iter().map(|handle| handle.join().unwrap()).collect()
        });
        let mut ids: Vec<u16> = kept.iter().flatten().map(|file| file.id.0).collect();
        ids.sort_unstable();
        assert_eq!(ids, (0..400).collect::<Vec<u16>>(), "every text is there, once");
        assert_eq!(texts.len(), 400);
    }
}
