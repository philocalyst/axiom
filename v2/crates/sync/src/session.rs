//! `axiom sync`: run each source's command, read what it printed, and work out
//! what the book is missing. Each source stands alone: one that fails writes
//! nothing and does not stop the others.

use std::path::Path;
use std::time::Duration;

use axiom_core::{Day, Diagnostic};

use crate::command::{Failed, run_all, substitute};
use crate::sink::{self, Sink};
use crate::world::{Feed, World};
use crate::write::{Change, changes};
use crate::Insert;

/// A declared `sync`.
pub struct Source<'a> {
    pub name: &'a str,
    /// The command, with `{since}`, `{today}` and `{units}` still in it.
    pub run: &'a str,
    /// The day after the last record the book has from this source, or its
    /// first day.
    pub since: Day,
    pub kind: Kind<'a>,
}

pub enum Kind<'a> {
    /// Statements for an account.
    Feed(Feed<'a>),
    /// Axiom to merge.
    Sink(Sink<'a>),
}

/// What commands run against.
pub struct Env<'a> {
    /// Where commands run from.
    pub root: &'a Path,
    pub today: Day,
    /// The commodities held.
    pub units: &'a [&'a str],
    /// How long a command may take.
    pub timeout: Duration,
}

/// Why a source wrote nothing.
pub enum Failure {
    /// The command could not run, failed, or took too long.
    Command(Failed),
    /// What it printed cannot be used: the text, and what is wrong with it.
    Output { text: String, problems: Vec<Diagnostic> },
}

pub struct Outcome<'a> {
    /// Each source, and how many lines or rows it adds.
    pub sources: Vec<(&'a str, Result<usize, Failure>)>,
    /// What every file would be.
    pub changes: Vec<Change>,
}

/// Runs the sources, all at once, and plans their changes one after another:
/// what an earlier source writes, a later one recognizes as written.
pub fn sync<'a>(
    world: &mut World<'a>,
    sources: &[Source<'a>],
    env: &Env,
    read: &dyn Fn(&str) -> Option<String>,
) -> Outcome<'a> {
    let commands: Vec<String> = sources.iter().map(|source| substitute(source.run, source.since, env.today, env.units)).collect();
    let outputs = run_all(&commands, env.root, env.timeout);
    let (mut inserts, mut results) = (Vec::new(), Vec::new());
    for (source, output) in sources.iter().zip(outputs) {
        let planned = output.map_err(Failure::Command).and_then(|text| {
            plan(world, source, &text, read).map_err(|problems| Failure::Output { text, problems })
        });
        results.push((source.name, planned.map(|added: Vec<Insert>| {
            let count = added.len();
            inserts.extend(added);
            count
        })));
    }
    Outcome { sources: results, changes: changes(&inserts, read) }
}

fn plan<'a>(
    world: &mut World<'a>,
    source: &Source<'a>,
    text: &str,
    read: &dyn Fn(&str) -> Option<String>,
) -> Result<Vec<Insert>, Vec<Diagnostic>> {
    match &source.kind {
        Kind::Feed(feed) => world.feed(feed, text),
        Kind::Sink(sink) => sink::merge(*sink, text, &world.layout, read),
    }
}
