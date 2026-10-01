//! `axiom sync`: read each source, and work out what the book is missing. Each
//! source stands alone: one that fails writes nothing and does not stop the
//! others.

use std::fs;
use std::path::Path;
use std::time::Duration;

use axiom_core::{Day, Diagnostic};

use crate::Insert;
use crate::command::{Failed, run_all, substitute};
use crate::paths::matching_paths;
use crate::sink::{self, Sink};
use crate::world::{Feed, World};
use crate::write::{Change, changes};

/// Where a source's text comes from.
pub enum Input<'a> {
    /// A command, run from the project root, with `{since}`, `{today}` and
    /// `{units}` still in it.
    Run(&'a str),
    /// The files a pattern names, relative to the project root
    /// (`imports/chase/*.csv`): a folder for what a bank lets one download by
    /// hand. They stay where they are; reading them again writes nothing.
    Read(&'a str),
}

/// A declared `sync`.
pub struct Source<'a> {
    pub name: &'a str,
    pub input: Input<'a>,
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
    /// Where commands run from, and patterns start.
    pub root: &'a Path,
    pub today: Day,
    /// The commodities held.
    pub units: &'a [&'a str],
    /// How long a command may take.
    pub timeout: Duration,
}

/// Why a source wrote nothing.
pub enum Failure {
    /// The command could not run, failed, or took too long; or a file could
    /// not be read.
    Command(Failed),
    /// What it gave cannot be used: the text, and what is wrong with it.
    Output {
        text: String,
        problems: Vec<Diagnostic>,
    },
}

pub struct Outcome {
    /// Each source, or each file of one that reads files, and how many lines or
    /// rows it adds.
    pub sources: Vec<(String, Result<usize, Failure>)>,
    /// What every file would be.
    pub changes: Vec<Change>,
    /// A sink path or final formatted source that could not be safely planned.
    pub problems: Vec<Diagnostic>,
}

/// Reads the sources, the commands all at once, and plans their changes one
/// after another: what an earlier source writes, a later one recognizes as
/// written. What prints Axiom (invoices, payouts) goes first, so that the bank's
/// line for the same money is the document's and is not written twice.
pub fn sync<'a>(
    world: &mut World<'a>,
    sources: &[Source<'a>],
    env: &Env,
    read: &dyn Fn(&str) -> Option<String>,
) -> Outcome {
    let commands: Vec<String> = sources
        .iter()
        .filter_map(|source| match source.input {
            Input::Run(command) => Some(substitute(command, source.since, env.today, env.units)),
            Input::Read(_) => None,
        })
        .collect();
    let mut ran = run_all(&commands, env.root, env.timeout).into_iter();
    // The text of each source, or of each file of one that reads files.
    let texts: Vec<Vec<(String, Result<String, Failed>)>> = sources
        .iter()
        .map(|source| match source.input {
            Input::Run(_) => vec![(
                source.name.to_string(),
                ran.next().expect("a result for each command"),
            )],
            Input::Read(pattern) => files(env.root, pattern, source.name),
        })
        .collect();
    let mut order: Vec<usize> = (0..sources.len()).collect();
    order.sort_by_key(|&at| matches!(sources[at].kind, Kind::Feed(_)));
    let mut inserts = Vec::new();
    let mut results: Vec<Vec<(String, Result<usize, Failure>)>> =
        sources.iter().map(|_| Vec::new()).collect();
    for at in order {
        let (source, pieces) = (&sources[at], &texts[at]);
        if pieces.is_empty() {
            results[at].push((source.name.to_string(), Ok(0)));
        }
        for (label, text) in pieces {
            let planned = match text {
                Err(failed) => Err(Failure::Command(failed.clone())),
                Ok(text) => plan(world, source, text, read).map_err(|problems| Failure::Output {
                    text: text.clone(),
                    problems,
                }),
            };
            let counted = planned.map(|added: Vec<Insert>| {
                let count = added.len();
                inserts.extend(added);
                count
            });
            results[at].push((label.clone(), counted));
        }
    }
    let (changes, problems) = match changes(&inserts, read) {
        Ok(changes) => (changes, Vec::new()),
        Err(problems) => (Vec::new(), problems),
    };
    Outcome {
        sources: results.into_iter().flatten().collect(),
        changes,
        problems,
    }
}

fn plan<'a>(
    world: &mut World<'a>,
    source: &Source<'a>,
    text: &str,
    read: &dyn Fn(&str) -> Option<String>,
) -> Result<Vec<Insert>, Vec<Diagnostic>> {
    match &source.kind {
        Kind::Feed(feed) => world.feed(feed, text),
        Kind::Sink(sink) => {
            let added = sink::merge(*sink, text, &world.layout, read)?;
            world.learn(&added);
            Ok(added)
        }
    }
}

/// The text of each file a pattern names, in path order, labelled with its
/// path. A pattern that would leave the project names nothing but a failure.
fn files(root: &Path, pattern: &str, name: &str) -> Vec<(String, Result<String, Failed>)> {
    let paths = match matching_paths(root, pattern) {
        Ok(paths) => paths,
        Err(problem) => {
            return vec![(
                name.to_string(),
                Err(Failed {
                    summary: problem.message,
                    stderr: String::new(),
                }),
            )];
        }
    };
    let root = match fs::canonicalize(root) {
        Ok(root) => root,
        Err(error) => {
            return vec![(
                name.to_string(),
                Err(Failed {
                    summary: format!("could not resolve project root: {error}"),
                    stderr: String::new(),
                }),
            )];
        }
    };
    paths
        .into_iter()
        .map(|path| {
            let text = fs::read_to_string(root.join(&path)).map_err(|error| Failed {
                summary: format!("could not read {path}: {error}"),
                stderr: String::new(),
            });
            (path, text)
        })
        .collect()
}
