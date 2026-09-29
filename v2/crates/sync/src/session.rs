//! `axiom sync`: read each source, and work out what the book is missing. Each
//! source stands alone: one that fails writes nothing and does not stop the
//! others.

use std::fs;
use std::path::Path;
use std::time::Duration;

use axiom_core::glob::{glob, is_pattern};
use axiom_core::{Day, Diagnostic};

use crate::Insert;
use crate::command::{Failed, run_all, substitute};
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
    Output { text: String, problems: Vec<Diagnostic> },
}

pub struct Outcome {
    /// Each source, or each file of one that reads files, and how many lines or
    /// rows it adds.
    pub sources: Vec<(String, Result<usize, Failure>)>,
    /// What every file would be.
    pub changes: Vec<Change>,
}

/// Reads the sources, the commands all at once, and plans their changes one
/// after another: what an earlier source writes, a later one recognizes as
/// written.
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
    let (mut inserts, mut results) = (Vec::new(), Vec::new());
    for source in sources {
        let pieces = match source.input {
            Input::Run(_) => vec![(source.name.to_string(), ran.next().expect("a result for each command"))],
            Input::Read(pattern) => files(env.root, pattern, source.name),
        };
        if pieces.is_empty() {
            results.push((source.name.to_string(), Ok(0)));
        }
        for (label, text) in pieces {
            let planned = text.map_err(Failure::Command).and_then(|text| {
                plan(world, source, &text, read).map_err(|problems| Failure::Output { text, problems })
            });
            let counted = planned.map(|added: Vec<Insert>| {
                let count = added.len();
                inserts.extend(added);
                count
            });
            results.push((label, counted));
        }
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

/// The text of each file a pattern names, in path order, labelled with its
/// path. A pattern that would leave the project names nothing but a failure.
fn files(root: &Path, pattern: &str, name: &str) -> Vec<(String, Result<String, Failed>)> {
    let outside = pattern.starts_with('/') || pattern.split('/').any(|part| part == "..");
    if outside {
        let failed = Failed { summary: format!("`{pattern}` leaves the project"), stderr: String::new() };
        return vec![(name.to_string(), Err(failed))];
    }
    let mut found = vec![String::new()];
    for part in pattern.split('/').filter(|part| !part.is_empty()) {
        let mut next = Vec::new();
        for folder in &found {
            let joined = |entry: &str| if folder.is_empty() { entry.to_string() } else { format!("{folder}/{entry}") };
            if !is_pattern(part) {
                next.push(joined(part));
                continue;
            }
            let entries = fs::read_dir(root.join(folder)).into_iter().flatten().flatten();
            let names = entries.map(|entry| entry.file_name().to_string_lossy().into_owned());
            next.extend(names.filter(|entry| !entry.starts_with('.') && glob(part, entry)).map(|entry| joined(&entry)));
        }
        found = next;
    }
    found.retain(|path| root.join(path).is_file());
    found.sort();
    let read = |path: String| {
        let text = fs::read_to_string(root.join(&path))
            .map_err(|error| Failed { summary: format!("could not read {path}: {error}"), stderr: String::new() });
        (path, text)
    };
    found.into_iter().map(read).collect()
}
