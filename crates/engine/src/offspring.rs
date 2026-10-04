//! What a law derives from a flow that has posted: a card's cash back, a processor's fee on every payout.
//!
//! A law that is not a contract's is read as a flow posts, after its value has moved, so what it derives is another
//! flow of its own (or an item that is one: along the same ends or back), never a part of the flow it comes with. The
//! effect handler ([`Ledger::derive_later`]) does not post it. It makes the flow and queues it by value, and
//! [`Ledger::post`] posts the queue after the flow that fired the laws has finished with all of them, so a derived flow
//! sees every law of its cause fire first (`on out`, `on in`, the purpose's, `on spend`, `always`), and fires its own.
//!
//! # Why the data is the one it is
//!
//! - **A `Brood` is a stack, and the stack is the fold's.** What one flow derives is collected in the order its laws
//!   fired (`fresh`) and then laid on `waiting` back to front, so the first is on top: a flow's derived flows post
//!   right after it, before its siblings', and what they derive posts before the next sibling. It is a plain `Vec`
//!   drained by a loop, so the depth of the machine's stack does not depend on the book: `post` never calls `post`.
//! - **A [`Lineage`] is a fixed array, passed by value.** The laws a derived flow descends through, root first,
//!   bounded by [`DEPTH`] and checked for repeats by the one function that can make a longer one. A queued flow
//!   cannot be made without one, and the only way to get a longer lineage is [`Lineage::then`], so the bound and the
//!   cycle check are the same call and cannot be left out. Nothing walks the record to find a chain: the lineage
//!   goes down with the flow and the record (`Offspring::parent`) is the way back up.
//! - **An offspring is numbered by the record it is pushed to**, from the number the record begins at: a checkpoint's
//!   record starts after the last number its parent made (`Record::forked`), so a parcel a checkpoint holds, which
//!   says which flow landed it, is never mistaken for one a later fold derives.
//! - **A flow that will be returned is remembered**, and only that one: the plan knows which journal flows are
//!   returned, so `Record::returnable` holds a few chains and costs nothing for any other. A return posts them backwards.

use axiom_core::{Diagnostic, Id, Loc};
use axiom_model::{
    Amount, Book, Cause, Derived, Fault, Flow, Law, Mode, Offspring, Owner, Place, Rule, RuntimeTxn, Subject,
};

use crate::State;
use crate::eval::Context;
use crate::ledger::Ledger;
use crate::motion::{Amounts, Course, Motion};

/// What a flow derived, each offspring with its number.
pub(crate) type Chain = Box<[(Id<Offspring>, Offspring)]>;

/// How many laws deep a derived flow may be: a law derives from a flow, and what it derives is derived from by others.
pub(crate) const DEPTH: usize = 8;

/// The laws a derived flow descends through, root first. The unused places are always the first law's number, so two
/// lineages of the same laws are equal.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) struct Lineage {
    laws: [Id<Law>; DEPTH],
    len: u8,
}

/// Why a lineage cannot go on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Stopped {
    /// The flow was derived by this very law, which does not watch what it made: a card's cash back is not a charge,
    /// and earns none. Nothing is wrong, and nothing is said.
    Own,
    /// The chain would go on without end, and the book is told.
    Unbounded(Unbounded),
}

/// What a chain that is told of cannot do.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Unbounded {
    /// The law is further back in the chain: it would derive from what another law derived from what it derived.
    Cycle,
    /// It has [`DEPTH`] laws.
    TooDeep,
}

impl Lineage {
    /// What a flow that no law derived descends through.
    pub const ROOT: Lineage = Lineage { laws: [Id::new(0); DEPTH], len: 0 };

    /// The laws, root first.
    pub fn laws(&self) -> &[Id<Law>] {
        &self.laws[..usize::from(self.len)]
    }

    /// The lineage of a flow `law` derives from a flow of this one.
    pub fn then(mut self, law: Id<Law>) -> Result<Lineage, Stopped> {
        if self.laws().last() == Some(&law) {
            return Err(Stopped::Own);
        }
        if self.laws().contains(&law) {
            return Err(Stopped::Unbounded(Unbounded::Cycle));
        }
        *self.laws.get_mut(usize::from(self.len)).ok_or(Stopped::Unbounded(Unbounded::TooDeep))? = law;
        self.len += 1;
        Ok(self)
    }
}

impl Default for Lineage {
    fn default() -> Lineage {
        Lineage::ROOT
    }
}

/// A flow a law derived, waiting its turn to post.
struct Waiting {
    offspring: Offspring,
    /// The step that derived it, which a flow derives from once.
    template: Id<Derived>,
    lineage: Lineage,
}

/// What the flow being posted has derived, and what is waiting to post.
#[derive(Default)]
pub(crate) struct Brood {
    /// Derived by the flow being posted, in the order its laws fired.
    fresh: Vec<Waiting>,
    /// Still to post: the next on top.
    waiting: Vec<Waiting>,
    /// What the flow being posted descends through.
    lineage: Lineage,
    /// The flow that began the chain, which a cycle is said to begin with.
    root: Loc,
}

impl Brood {
    /// A flow no law derived is about to post.
    fn begin(&mut self, root: Loc) {
        (self.lineage, self.root) = (Lineage::ROOT, root);
    }

    /// What the flow that just posted derived goes on top of what waits, the first it derived on top: it posts before
    /// anything that was already waiting.
    fn lay_down(&mut self) {
        let Brood { fresh, waiting, .. } = self;
        waiting.extend(fresh.drain(..).rev());
    }
}

impl Ledger<'_, '_, '_> {
    /// Applies a flow: moves its value and fires every law that watches it, then posts what they derived, each by
    /// the same path. A flow run backwards has the flows it derived run backwards with it, and derives nothing.
    pub(crate) fn post(&mut self, m: &Motion) {
        self.scratch.brood.begin(m.loc);
        self.post_flow(m);
        match m.course {
            Course::Forward => self.post_brood(m),
            Course::Back => self.undo_brood(m),
        }
    }

    /// Posts what the flow derived, and what that derived, in order.
    fn post_brood(&mut self, root: &Motion) {
        if self.scratch.brood.fresh.is_empty() {
            return;
        }
        let first = self.next_offspring();
        self.scratch.brood.lay_down();
        while let Some(next) = self.scratch.brood.waiting.pop() {
            self.post_offspring(next);
        }
        self.remember_for_return(root, first);
    }

    /// One derived flow posts, as a transaction of its own, and goes into the record.
    fn post_offspring(&mut self, Waiting { offspring, lineage, .. }: Waiting) {
        let (book, id) = (self.plan.book, self.next_offspring());
        self.scratch.brood.lineage = lineage;
        self.post_flow(&Motion::derived(book, id, &offspring, offspring.flow.day));
        self.record.offspring.push(offspring);
        self.scratch.brood.lay_down();
    }

    /// The number the next derived flow has: this record's own begin where its parent's ended.
    fn next_offspring(&self) -> Id<Offspring> {
        Id::new(self.record.first_offspring + self.record.offspring.len() as u32)
    }

    /// A flow the plan says will be returned keeps what it derived, which its return posts backwards.
    fn remember_for_return(&mut self, root: &Motion, first: Id<Offspring>) {
        let Cause::Flow(id) = root.cause else { return };
        let book = self.plan.book;
        if !matches!(self.plan.events.state(id, &book.flows[id]), State::Returned(_)) {
            return;
        }
        let made = &self.record.offspring[first.index() - self.record.first_offspring as usize..];
        let numbered = (first.index() as u32..).map(Id::new).zip(made.iter().cloned());
        self.record.returnable.insert(id, numbered.collect());
    }

    /// A flow that is returned posts what it derived backwards, in the order it posted it.
    fn undo_brood(&mut self, returned: &Motion) {
        let Some(chain) = returned.cause.flow().and_then(|id| self.record.returnable.remove(&id)) else { return };
        for (id, offspring) in &chain {
            let forward = Motion::derived(self.plan.book, *id, offspring, returned.day);
            self.post_flow(&forward.reversed());
        }
    }

    /// A law's `derive` step fired for the flow being posted: the flow it makes is queued, if it can be made. A step
    /// derives once for a flow, however many of the places it governs the flow touches.
    pub(crate) fn derive_later(&mut self, rule: &Rule, ctx: &Context, step: u32, made: (Id<Derived>, Amount)) {
        let (template, amount) = made;
        let Some(m) = ctx.motion.filter(|m| m.derives()) else { return };
        let Some(view) = m.view else { return };
        let derived = &self.plan.book.derived[template];
        if !derived.follows_a_posted_flow() {
            return self.fault(rule, ctx, step as usize, Fault::InvalidProgram);
        }
        if self.scratch.brood.fresh.iter().any(|waiting| waiting.template == template) {
            return;
        }
        let Some(lineage) = self.descend(rule.law) else { return };
        let subject = self.subject_place(rule.subject, m);
        let flow = derived.flow_from(&view, rule.law, amount, Some(subject));
        let flow = Flow { day: m.day, mode: Mode::Actual, recognized: ctx.over, ..flow };
        let offspring = Offspring { flow, parent: m.cause, law: rule.law };
        self.scratch.brood.fresh.push(Waiting { offspring, template, lineage });
    }

    /// The lineage a flow `law` derives has, or nothing after the book is told why it cannot.
    fn descend(&mut self, law: Id<Law>) -> Option<Lineage> {
        let lineage = self.scratch.brood.lineage;
        let stopped = match lineage.then(law) {
            Ok(longer) => return Some(longer),
            Err(Stopped::Own) => return None,
            Err(Stopped::Unbounded(stopped)) => stopped,
        };
        if self.record.stopped.insert((lineage, law)) {
            let diagnostic = stopped_chain(self.plan.book, stopped, &lineage, law, self.scratch.brood.root);
            self.record.report(diagnostic);
        }
        None
    }

    /// The place `self` stands for in a law's line: where the flow is at what the law governs.
    fn subject_place(&self, subject: Subject, m: &Motion) -> Id<Place> {
        let book = self.plan.book;
        let stands = |place| match subject {
            Subject::Place(root) => book.places.covers(root, place),
            Subject::Entity(entity) => book.standing_at(place) == entity,
            Subject::Asset(_) | Subject::Contract(_) => false,
        };
        match subject {
            Subject::Asset(asset) => book.assets[asset].place,
            _ if stands(m.from) => m.from,
            _ => m.to,
        }
    }
}

/// What a chain that cannot go on says: each law in the order it derived, the flow that started it, and the one that
/// would not be made.
fn stopped_chain(book: &Book, stopped: Unbounded, lineage: &Lineage, law: Id<Law>, root: Loc) -> Diagnostic {
    let (code, message) = match stopped {
        Unbounded::Cycle => ("derive-cycle", "the laws of this book derive each other's flows without end".to_owned()),
        Unbounded::TooDeep => ("derive-depth", format!("a flow is derived through more than {DEPTH} laws")),
    };
    let mut diagnostic = Diagnostic::error(code, message).context(root, "the flow that started it");
    for (at, &derived) in lineage.laws().iter().enumerate() {
        let (loc, said) = (book.laws[derived].loc, format!("{}. {} derives from it", at + 1, describe(book, derived)));
        diagnostic = diagnostic.context(loc, said);
    }
    let again = match stopped {
        Unbounded::Cycle => format!(
            "{}. and {} would derive again from what that made: not made",
            lineage.laws().len() + 1,
            describe(book, law)
        ),
        Unbounded::TooDeep => {
            format!("{}. and {} would be one law too many: not made", lineage.laws().len() + 1, describe(book, law))
        }
    };
    let note = "a law derives once from a flow, and once from each flow another law derives from it, so a chain that \
                comes back to a law stops where it began";
    diagnostic
        .label(book.laws[law].loc, again)
        .note(note)
        .help("narrow one of the laws with `when`, so that it does not watch the flow that closes the chain")
}

/// A law in the words of the book: the `also` of a kind, or a law by its name and where it is written.
fn describe(book: &Book, law: Id<Law>) -> String {
    let law = &book.laws[law];
    let (owner, name) = (&law.owner, book.name(law.name));
    let of = match *owner {
        Owner::Kind(kind) => format!("kind `{}`", book.name(book.kinds[kind].name)),
        Owner::Place(place) => format!("account `{}`", book.name(book.places[place].path)),
        Owner::Entity(entity) => format!("entity `{}`", book.name(book.entities[entity].path)),
        Owner::Purpose(purpose) => format!("purpose `#{}`", book.name(book.purposes[purpose].name)),
        Owner::Asset(asset) => format!("asset `{}`", book.name(book.assets[asset].name)),
        Owner::Contract(contract) => format!("contract `{}`", book.name(book.contracts[contract].name)),
        Owner::System(_) | Owner::Book => "the project".to_owned(),
    };
    if name == "also" { format!("the `also` of {of}") } else { format!("law `{name}` of {of}") }
}
