//! A loaded project as a value: the texts it was read from, the book built from them, and the run of its fold, which
//! answers queries and takes edits. What an MCP server and a GUI are written against; the command line is its first
//! client.
//!
//! | module        | job                                                                                     |
//! |---------------|-----------------------------------------------------------------------------------------|
//! | `texts`       | the owner of every text, which grows through a shared reference and never moves one      |
//! | `sources`     | the files by `FileId`, as a table of borrowed texts: what a `Loc` points into            |
//! | `session`     | parse, build, fold; `query`, `what_if` and `apply`                                       |
//! | `edit`        | an edit as a typed value, why one is refused, and what an applied one changed            |
//! | `transaction` | a typed flow, and the line of the language that says it                                  |
//!
//! # Why the owner is separate from the session
//!
//! A `Book<'s>` borrows its names from the text it was built from, and a `Plan<'b, 's>` borrows the whole `Book`. So no
//! value can own a text, a book built from it and a plan over that book, and a session that did would have to be
//! self-referential (which is `unsafe`, or a crate that is). The text therefore lives in a [`Texts`] the client owns
//! and the session borrows; the session owns the book; and the plan, which cannot sit beside the book, is built for each
//! answer, with the one thing a fold leaves that borrows nothing ([`Folded`](axiom_report::Folded)) kept between them.
//!
//! What the lifetimes then say, and the compiler checks:
//!
//! - [`Session::query`] borrows the session and [`Session::apply`] needs it alone, so **no report outlives the state it
//!   was read from**.
//! - [`Session::what_if`] makes the hypothetical session a local and hands it to a closure that cannot return anything
//!   that mentions it, so **no report of a state that is gone can be returned**.
//! - `apply` builds the next session as a new value and assigns it only when it exists, so an edit that is refused, for
//!   any reason, leaves nothing half done.
//!
//! The price: a struct that holds a [`Texts`] and the `Session` over it is self-referential, so a client keeps the
//! `Texts` in a longer-lived frame (the thread that serves the session; a leaked box for a project that lives as long
//! as the process) and as many sessions over their own as it likes. Nothing is global. A `Session` is `Send` and
//! `Sync`: two threads may query it at once, and the first to need the fold makes it.
//!
//! Each applied edit leaves the old text of its file in the `Texts` until they are dropped, since the book before it
//! may still be being read: bytes of one file per edit, not a book. A server that applies many edits to a large file
//! should now and then open its sources again in a fresh `Texts`.
//!
//! # A session from outside
//!
//! ```
//! use axiom_core::Day;
//! use axiom_report::{Query, json};
//! use axiom_session::{Options, Session, Sources, Texts};
//!
//! const BOOK: &str = "\
//! base USD
//! commodity USD
//!   precision 2
//! entity me
//! entity grocer
//! purpose food : spending
//! account checking : asset
//! opening 2026-01-01
//!   checking 100 USD
//! ";
//!
//! let texts = Texts::default();
//! let sources = Sources::in_memory(&texts, &[("axiom.ax", BOOK)], &[]);
//! let mut session = Session::open(sources, Options { today: Day::parse(b"2026-02-01").unwrap(), relaxed: false });
//! assert_eq!(session.diagnostics().count(), 0);
//!
//! let balance = Query::Balance { globs: vec![], at: None, value: false, monthly: false };
//! let before = json::render(&session.query(&balance, None).unwrap(), session.sources());
//! assert!(before.contains("100.00"));
//! ```

#![forbid(unsafe_code)]

mod edit;
mod session;
mod sources;
mod texts;
mod transaction;

#[cfg(test)]
mod tests;

pub use axiom_engine::Options;
pub use edit::{Applied, Edit, Refused};
pub use session::Session;
pub use sources::{SourceFile, Sources};
pub use texts::Texts;
pub use transaction::{NewTransaction, Unwritable};
