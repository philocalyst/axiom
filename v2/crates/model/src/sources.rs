//! The sources, arranged: which files are systems, which are the project, and
//! the tree of systems they define.

use axiom_core::diag::closest;
use axiom_core::{Diagnostic, Id, Interner, Map, Tree};
use axiom_syntax::{ItemKind, Setting};

use crate::Source;
use crate::book::System;
use crate::errors::{Word, unknown};
use crate::layout::Layout;
use crate::paths;
use crate::scope::Home;

/// One source, and where its declarations live.
pub(crate) struct Site<'a, 's> {
    pub source: &'a Source<'s>,
    pub home: Home,
    /// What the file's place among the folders says it may hold.
    pub layout: Layout<'s>,
}

/// The path a source defines, if its first item is `system PATH`.
fn defined_by<'s>(source: &Source<'s>) -> Option<&'s str> {
    let item = source.file.items.first()?;
    match item.kind {
        ItemKind::Setting(id) => match source.file[id] {
            Setting::System(name) => Some(name.0),
            _ => None,
        },
        _ => None,
    }
}

/// The sources in declaration order: systems first, then the project's own
/// files, each by path, so the outcome never depends on how the files were
/// found and a project can extend a system by counting into what it reads. A
/// project may override an embedded system by defining the same path; two
/// project files may not.
fn arranged<'a, 's>(sources: &'a [Source<'s>], diags: &mut Vec<Diagnostic>) -> Vec<&'a Source<'s>> {
    let mut sorted: Vec<&Source> = sources.iter().collect();
    sorted.sort_by_key(|source| (defined_by(source).is_none(), source.path));
    let mut kept: Vec<Option<&Source>> = Vec::with_capacity(sorted.len());
    let mut defined: Map<&str, usize> = Map::default();
    for source in sorted {
        let Some(path) = defined_by(source) else {
            kept.push(Some(source));
            continue;
        };
        match defined.get(path).map(|&at| (at, kept[at].expect("a defined system is kept until overridden"))) {
            None => {
                defined.insert(path, kept.len());
                kept.push(Some(source));
            }
            Some((at, first)) if first.embedded && !source.embedded => {
                kept[at] = None;
                defined.insert(path, kept.len());
                kept.push(Some(source));
            }
            Some((_, first)) if !first.embedded && !source.embedded => diags.push(duplicate(path, source, first)),
            Some(_) => {}
        }
    }
    kept.into_iter().flatten().collect()
}

fn duplicate(path: &str, again: &Source, first: &Source) -> Diagnostic {
    let name_loc = |source: &Source| {
        let text = defined_by(source).unwrap_or(path);
        source.file.loc(text)
    };
    Diagnostic::error("duplicate-system", format!("system `{path}` is defined twice"))
        .label(name_loc(again), "defined again here")
        .context(name_loc(first), "first defined here")
        .note("only an embedded system can be overridden, and the project's file replaces it entirely")
}

/// How to find a system by path.
pub(crate) struct SystemIndex<'s> {
    by_path: Map<&'s str, Id<System>>,
}

impl SystemIndex<'_> {
    pub fn find(&self, path: &str) -> Option<Id<System>> {
        self.by_path.get(path).copied()
    }

    /// The `unknown-system` diagnostic for a `use` or `lives` naming no system.
    pub fn unknown(&self, word: Word) -> Diagnostic {
        unknown("unknown-system", "system", word, closest(word.text, self.by_path.keys().copied()))
    }
}

/// Arranges the sources, and creates every system they define together with
/// the ancestors their paths imply.
pub(crate) fn arrange<'a, 's>(
    sources: &'a [Source<'s>],
    names: &mut Interner<'s>,
    diags: &mut Vec<Diagnostic>,
) -> (Vec<Site<'a, 's>>, Tree<System>, SystemIndex<'s>) {
    let kept = arranged(sources, diags);
    let written: Map<&str, &Source> =
        kept.iter().filter_map(|&source| defined_by(source).map(|path| (path, source))).collect();
    let (tree, by_path) = paths::build(written.keys().copied(), |path| {
        let source = written.get(path);
        System {
            path: names.intern(path),
            laws: Box::default(),
            doc: source.and_then(|source| source.file.items[0].doc).map(|doc| names.intern(doc.0)),
            loc: source.and_then(|source| defined_by(source).map(|text| source.file.loc(text))),
        }
    });
    let sites = kept
        .into_iter()
        .map(|source| {
            let home = defined_by(source).map_or(Home::Project, |path| Home::System(by_path[path]));
            Site { source, home, layout: Layout::of(source.path) }
        })
        .collect();
    (sites, tree, SystemIndex { by_path })
}
