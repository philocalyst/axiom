//! The session: a project read, built and folded, that answers queries and takes edits.
//!
//! Why it is shaped the way it is is the crate's story (see the crate documentation): the text lives in a [`Texts`] the
//! client owns, the book borrows it, and what the fold leaves that borrows nothing ([`Folded`]) is kept, while the
//! plan, which borrows the whole book, is built for each answer.

use std::sync::OnceLock;

use axiom_core::Diagnostic;
use axiom_engine::{Options, Plan, Run};
use axiom_model::Book;
use axiom_report::{Context, Folded, Query, Report, Summary, summary_of};

use crate::Sources;

/// A project's sources, the book built from them, and the run of its fold. Nothing is global and nothing prints:
/// several sessions can be held at once, each over a [`Texts`](crate::Texts) of its own.
pub struct Session<'t> {
    sources: Sources<'t>,
    book: Book<'t>,
    /// What reading and building the book found.
    found: Vec<Diagnostic>,
    options: Options,
    /// The fold, made by the first thing that needs it, with the plan that thing built.
    folded: OnceLock<Folded>,
}

/// Everything a session is made of is shared by reference between threads, so a session is too: a query on one thread
/// and a query on another read the same book, and the fold is made once, by whichever asks first.
const _: () = {
    const fn is_send_and_sync<T: Send + Sync>() {}
    is_send_and_sync::<Session<'static>>();
};

impl<'t> Session<'t> {
    /// Reads `sources` into a book. Parsing is on every core; the fold waits until something asks for it.
    pub fn open(sources: Sources<'t>, options: Options) -> Session<'t> {
        let (parsed, mut found) = sources.parse();
        let (book, built) = axiom_model::build(&parsed);
        // The trees are done with: the book borrows only the text.
        drop(parsed);
        found.extend(built);
        Session { sources, book, found, options, folded: OnceLock::new() }
    }

    /// The files by number: what every diagnostic's location points into, and a `SourceProvider` for the renderers.
    pub fn sources(&self) -> &Sources<'t> {
        &self.sources
    }

    pub fn book(&self) -> &Book<'t> {
        &self.book
    }

    /// The day the fold stands on, and whether violations are only warnings.
    pub fn options(&self) -> Options {
        self.options
    }

    /// The journal as folded. The first call folds it.
    pub fn run(&self) -> &Run {
        self.folded().run()
    }

    /// What reading and building the book found: the parser's, file by file, then the model's.
    pub fn book_diagnostics(&self) -> &[Diagnostic] {
        &self.found
    }

    /// What the fold found. The first call folds the journal.
    pub fn run_diagnostics(&self) -> &[Diagnostic] {
        &self.run().diagnostics
    }

    /// Everything found, in the order `axiom check` lists it: the book's, then the run's.
    pub fn diagnostics(&self) -> impl Iterator<Item = &Diagnostic> + Clone {
        self.book_diagnostics().iter().chain(self.run_diagnostics())
    }

    /// The one-line account of the book that `axiom check` ends with.
    pub fn summary(&self) -> Summary {
        self.with_plan(|plan, folded| summary_of(&plan, folded.run()))
    }

    /// The answer to `query` about the money of `whose` (an entity, a household including its members; everyone's if
    /// `None`). A report runs whatever errors the book has: they are `diagnostics`.
    ///
    pub fn query(&self, query: &Query<'_>, whose: Option<&str>) -> Result<Report<'_>, Diagnostic> {
        self.with_plan(|plan, folded| Context::over(plan, folded, whose)?.report_with_sources(query, &self.sources))
    }

    /// The fold, once, from the plan of whichever answer needs it first, and the plan that answer then uses. The plan
    /// borrows the whole book, so it cannot be kept in the session beside it: it is built per answer, and the first
    /// answer's is also the fold's.
    fn with_plan<'a, R>(&'a self, then: impl FnOnce(Plan<'a, 't>, &'a Folded) -> R) -> R {
        let plan = Plan::new(&self.book);
        let folded = self.folded.get_or_init(|| Folded::of(&plan, self.options));
        then(plan, folded)
    }

    fn folded(&self) -> &Folded {
        match self.folded.get() {
            Some(folded) => folded,
            None => self.with_plan(|_, folded| folded),
        }
    }
}
