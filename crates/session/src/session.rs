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
use axiom_syntax::Folder;

use crate::edit::introduced;
use crate::{Applied, Edit, Refused, SourceFile, Sources};

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
    /// The report borrows the session, and an edit needs the session alone: **no report outlives the state it was
    /// read from.** This does not compile, because the report is still to be used when the session changes:
    ///
    /// ```compile_fail,E0502
    /// # use axiom_core::Day;
    /// # use axiom_report::Query;
    /// # use axiom_session::{Edit, Options, Session, Sources, Texts};
    /// # let texts = Texts::default();
    /// # let sources = Sources::in_memory(&texts, &[("axiom.ax", "base USD\n")], &[]);
    /// # let options = Options { today: Day::parse(b"2026-01-01").unwrap(), relaxed: false };
    /// # let mut session = Session::open(sources, options);
    /// let report = session.query(&Query::Contracts, None).unwrap();
    /// session.apply(Edit::Append { file: axiom_core::FileId(0), text: "commodity USD\n".into() }).unwrap();
    /// println!("{}", report.sections.len());
    /// ```
    ///
    /// and this, the same without the last line, does:
    ///
    /// ```
    /// # use axiom_core::Day;
    /// # use axiom_report::Query;
    /// # use axiom_session::{Edit, Options, Session, Sources, Texts};
    /// # let texts = Texts::default();
    /// # let sources = Sources::in_memory(&texts, &[("axiom.ax", "base USD\n")], &[]);
    /// # let options = Options { today: Day::parse(b"2026-01-01").unwrap(), relaxed: false };
    /// # let mut session = Session::open(sources, options);
    /// let report = session.query(&Query::Contracts, None).unwrap();
    /// println!("{}", report.sections.len());
    /// session.apply(Edit::Append { file: axiom_core::FileId(0), text: "commodity USD\n".into() }).unwrap();
    /// ```
    pub fn query(&self, query: &Query<'_>, whose: Option<&str>) -> Result<Report<'_>, Diagnostic> {
        self.with_plan(|plan, folded| Context::over(plan, folded, whose)?.report_with_sources(query, &self.sources))
    }

    /// What would be true if `edit` were applied, for as long as `ask` looks: `ask` is given the session the edit
    /// would make, and what it returns is yours. Nothing of this session changes, and nothing is kept.
    ///
    /// `ask` cannot return a report of the hypothetical session, because that session is gone when `what_if` returns:
    /// `R` is chosen before the session's lifetime is, so it cannot mention it. Render it inside, or return data you
    /// own.
    ///
    /// ```compile_fail
    /// # use axiom_core::Day;
    /// # use axiom_report::Query;
    /// # use axiom_session::{Edit, Options, Session, Sources, Texts};
    /// # let texts = Texts::default();
    /// # let sources = Sources::in_memory(&texts, &[("axiom.ax", "base USD\n")], &[]);
    /// # let options = Options { today: Day::parse(b"2026-01-01").unwrap(), relaxed: false };
    /// # let session = Session::open(sources, options);
    /// let edit = Edit::Append { file: axiom_core::FileId(0), text: "commodity USD\n".into() };
    /// let report = session.what_if(&edit, |after| after.query(&Query::Contracts, None)).unwrap();
    /// ```
    ///
    /// and what does compile is to say what is wanted of it there:
    ///
    /// ```
    /// # use axiom_core::Day;
    /// # use axiom_session::{Edit, Options, Session, Sources, Texts};
    /// # let texts = Texts::default();
    /// # let sources = Sources::in_memory(&texts, &[("axiom.ax", "base USD\n")], &[]);
    /// # let options = Options { today: Day::parse(b"2026-01-01").unwrap(), relaxed: false };
    /// # let session = Session::open(sources, options);
    /// let edit = Edit::Append { file: axiom_core::FileId(0), text: "commodity USD\n".into() };
    /// let found = session.what_if(&edit, |after| after.diagnostics().count());
    /// assert_eq!(found.unwrap(), 0);
    /// ```
    pub fn what_if<R>(&self, edit: &Edit, ask: impl FnOnce(&Session<'_>) -> R) -> Result<R, Refused> {
        let edited = self.edited(edit)?;
        let hypothetical = Session::open(self.sources.with(&edited), self.options);
        Ok(ask(&hypothetical))
    }

    /// Applies `edit`: the file's text changes, the book is built and folded again, and what the edit introduced and
    /// cleared is returned (which needs what both books found, so both are folded). An edit that is refused changes
    /// nothing: the new session is made as a value beside this one and replaces it only once it exists.
    ///
    /// The old text of the edited file stays in the [`Texts`](crate::Texts) until they are dropped, because the old book
    /// may still be read by whoever holds a report from it; the old book is dropped now.
    pub fn apply(&mut self, edit: Edit) -> Result<Applied, Refused> {
        let texts = self.sources.texts();
        let kept: &'t SourceFile = texts.keep(self.edited(&edit)?);
        let next = Session::open(self.sources.with(kept), self.options);
        let applied = Applied::between(kept.id, self, &next);
        *self = next;
        Ok(applied)
    }

    /// The file `edit` makes: its text after the edit, if the edit is to a file of the project, names a span of its
    /// text, and does not make the file's syntax worse.
    fn edited(&self, edit: &Edit) -> Result<SourceFile, Refused> {
        let file = self.sources.axiom_file(edit.file()).ok_or(Refused::NoSuchFile(edit.file()))?;
        if file.embedded {
            return Err(Refused::Embedded(file.id));
        }
        let edited = file.reading(edit.applied_to(&file.text)?);
        let (after, before) = (syntax_errors(&edited), syntax_errors(file));
        let worse = introduced(&after, &before);
        if worse.is_empty() { Ok(edited) } else { Err(Refused::Unparsable(worse.into_iter().cloned().collect())) }
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

/// The syntax errors of one file, as the parser finds them: all that the parse of this text says is wrong.
fn syntax_errors(file: &SourceFile) -> Vec<Diagnostic> {
    let (_, found) = axiom_syntax::parse(file.id, &file.text, Folder::of(&file.path));
    found.into_iter().filter(Diagnostic::is_error).collect()
}
