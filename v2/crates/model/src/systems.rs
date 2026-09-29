//! Systems: the tree of jurisdictions and bodies of law, and the sources that
//! define them.

use axiom_core::diag::closest;
use axiom_core::{Diagnostic, Id, Interner, Map, Tree};
use axiom_syntax::{ExprKind, ItemKind, Name, Setting};

use crate::Source;
use crate::book::System;
use crate::catalog::Catalog;
use crate::paths;
use crate::scope::{Home, Scopes};

/// The path a source defines, if its first item is `system PATH`.
pub(crate) fn defined_by<'s>(source: &Source<'s>) -> Option<Name<'s>> {
    match &source.file.items.first()?.kind {
        ItemKind::Setting(Setting::System(name)) => Some(*name),
        _ => None,
    }
}

/// The sources in declaration order: by path, so the outcome never depends on
/// how the files were found. A project may override an embedded system by
/// defining the same path; two project files may not.
pub(crate) fn arrange<'a, 's>(sources: &'a [Source<'s>], diags: &mut Vec<Diagnostic>) -> Vec<&'a Source<'s>> {
    let mut sorted: Vec<&Source> = sources.iter().collect();
    sorted.sort_by_key(|source| source.path);

    let mut arranged: Vec<Option<&Source>> = Vec::with_capacity(sorted.len());
    let mut defined: Map<&str, usize> = Map::default();
    for source in sorted {
        let Some(system) = defined_by(source) else {
            arranged.push(Some(source));
            continue;
        };
        let Some(&earlier) = defined.get(system.text) else {
            defined.insert(system.text, arranged.len());
            arranged.push(Some(source));
            continue;
        };
        let first = arranged[earlier].expect("a defined system is kept until overridden");
        match (first.embedded, source.embedded) {
            (true, false) => {
                arranged[earlier] = None;
                defined.insert(system.text, arranged.len());
                arranged.push(Some(source));
            }
            (false, false) => diags.push(duplicate(system, first)),
            (_, true) => {}
        }
    }
    arranged.into_iter().flatten().collect()
}

fn duplicate(again: Name, first: &Source) -> Diagnostic {
    let earlier = defined_by(first).expect("only system sources are compared");
    Diagnostic::error("duplicate-system", format!("system `{}` is defined twice", again.text))
        .label(again.loc, "defined again here")
        .context(earlier.loc, "first defined here")
        .note("only an embedded system can be overridden, and the project's file replaces it entirely")
}

/// How to find a system by path.
pub(crate) struct Systems<'s> {
    by_path: Map<&'s str, Id<System>>,
}

impl<'s> Systems<'s> {
    /// Creates every system the sources define, and the ancestors their paths
    /// imply. Also says where each source's declarations live.
    pub fn declare(sources: &[&Source<'s>], names: &mut Interner<'s>) -> (Tree<System>, Systems<'s>, Vec<Home>) {
        let defined: Map<&str, (Name, &Source)> =
            sources.iter().filter_map(|&source| defined_by(source).map(|name| (name.text, (name, source)))).collect();
        let (tree, by_path) = paths::build(defined.keys().copied(), |path| {
            let written = defined.get(path);
            System {
                path: names.intern(path),
                laws: Box::default(),
                doc: written.and_then(|(_, source)| source.file.items[0].doc).map(|doc| names.intern(doc.0)),
                loc: written.map(|(name, _)| name.loc),
            }
        });
        let homes = sources
            .iter()
            .map(|&source| defined_by(source).map_or(Home::Project, |name| Home::System(by_path[name.text])))
            .collect();
        (tree, Systems { by_path }, homes)
    }

    pub fn find(&self, path: &str) -> Option<Id<System>> {
        self.by_path.get(path).copied()
    }

    /// The `unknown-system` diagnostic for a `use` or `lives` naming no system.
    pub fn unknown(&self, name: Name) -> Diagnostic {
        let mut diagnostic = Diagnostic::error("unknown-system", format!("there is no system `{}`", name.text))
            .label(name.loc, "no system has this path");
        if let Some(near) = closest(name.text, self.by_path.keys().copied()) {
            diagnostic = diagnostic.fix(format!("did you mean `{near}`?"), name.loc, near);
        }
        diagnostic
    }
}

/// What each home has brought into scope: its `use` lines, the system of every
/// `lives` line (which implies a `use`), and `std` for everyone.
pub(crate) fn scopes(catalog: &Catalog, systems: &Systems, tree: &Tree<System>, diags: &mut Vec<Diagnostic>) -> Scopes {
    let mut used: Vec<(Home, Id<System>)> = Vec::new();
    for &(home, name) in &catalog.uses {
        match systems.find(name.text) {
            Some(system) => used.push((home, system)),
            None => diags.push(systems.unknown(name)),
        }
    }
    for written in &catalog.entities {
        for prop in written.what.props.iter().filter(|prop| prop.name.text == "lives") {
            let residence = prop.args.first().map(|&arg| &written.exprs()[arg].kind);
            if let Some(&ExprKind::Name(path)) = residence {
                used.extend(systems.find(path).map(|system| (written.home(), system)));
            }
        }
    }
    let std = systems.find("std");
    Scopes::new(tree, |home| {
        let own = used.iter().filter(|&&(user, _)| user == home).map(|&(_, system)| system);
        own.chain(std).collect()
    })
}
