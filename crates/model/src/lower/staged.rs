//! Writes to the journal's arenas that are undone unless they are kept.
//!
//! Lowering one record appends flows, codes, selectors, details and a journal program, and may find out
//! halfway that the record is wrong. Those arenas only ever grow at their ends, so remembering where each
//! one ended is enough to take a record back: a [`Staged`] does that, and truncates them again when it is
//! dropped, which is every early return.
//!
//! Transactions and input values are not staged. A record pushes its transaction as its last act, when it
//! is sure, and a rejected record pushes an empty placeholder instead so that every dated record still owns
//! one transaction id.

use std::mem;
use std::ops::{Deref, DerefMut};

use axiom_core::{Arena, Id, Run, Sym};

use crate::declare::World;
use crate::journal::{Detail, Flow, Program, Select};

/// Where each staged arena ended when the guard opened: the id its next item would get.
#[derive(Clone, Copy)]
struct Marks {
    flows: Id<Flow>,
    codes: Id<Sym>,
    selectors: Id<Select>,
    details: Id<Detail>,
    programs: Id<Program>,
}

fn end<T>(arena: &Arena<T>) -> Id<T> {
    Id::new(arena.len() as u32)
}

/// The book, with everything written to it since this was opened taken back unless [`commit`](Self::commit)
/// says to keep it. It stands in for the world while a record is lowered.
pub(crate) struct Staged<'w, 's> {
    world: &'w mut World<'s>,
    marks: Marks,
}

impl<'w, 's> Staged<'w, 's> {
    pub fn open(world: &'w mut World<'s>) -> Self {
        let book = &world.book;
        let marks = Marks {
            flows: end(&book.flows),
            codes: end(&book.codes),
            selectors: end(&book.selectors),
            details: end(&book.details),
            programs: end(&book.journal_programs),
        };
        Staged { world, marks }
    }

    /// The flows written since the guard opened: what the record's transaction owns.
    pub fn flows(&self) -> Run<Flow> {
        Run::new(self.marks.flows, (self.world.book.flows.len() - self.marks.flows.index()) as u32)
    }

    /// The codes written since the guard opened.
    pub fn codes(&self) -> Run<Sym> {
        Run::new(self.marks.codes, (self.world.book.codes.len() - self.marks.codes.index()) as u32)
    }

    /// The flow written `offset` flows after the guard opened.
    pub fn flow(&self, offset: u32) -> &Flow {
        &self.world.book.flows[Id::new(self.marks.flows.index() as u32 + offset)]
    }

    /// Keeps everything written. The guard holds only a borrow, so forgetting it skips the rollback and
    /// nothing else; a flag for "committed" could disagree with the arenas, and this cannot.
    pub fn commit(self) {
        mem::forget(self);
    }
}

impl<'s> Deref for Staged<'_, 's> {
    type Target = World<'s>;

    fn deref(&self) -> &World<'s> {
        self.world
    }
}

impl DerefMut for Staged<'_, '_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.world
    }
}

impl Drop for Staged<'_, '_> {
    fn drop(&mut self) {
        let (book, marks) = (&mut self.world.book, self.marks);
        book.flows.truncate(marks.flows.index());
        book.codes.truncate(marks.codes.index());
        book.selectors.truncate(marks.selectors.index());
        book.details.truncate(marks.details.index());
        book.journal_programs.truncate(marks.programs.index());
    }
}
