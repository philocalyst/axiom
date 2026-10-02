//! Who can see what.
//!
//! Every declaration was written somewhere, its [`Home`]. A project sees its
//! own declarations and those of the systems it uses; a system sees its own,
//! its ancestors', and those of the systems it uses. The standard `std` system
//! is used by everyone. Every system is compiled, used or not, but only the
//! visible ones take part in name resolution.

use axiom_core::{Id, Tree};

use crate::book::System;

/// Where a declaration was written.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) enum Home {
    /// Built into the language: the root kinds, the class roots.
    Builtin,
    Project,
    System(Id<System>),
}

/// The systems that declare names, and what each home sees of them: what a name is looked up from.
#[derive(Clone, Copy)]
pub(crate) struct Seeing<'a> {
    pub systems: &'a Tree<System>,
    pub scopes: &'a Scopes,
}

/// The point of view of one project or system.
pub(crate) struct Scope {
    /// Where the looking happens, then (for a system) its ancestors, nearest
    /// first: what shadows what.
    nearest: Box<[Home]>,
    visible: Box<[bool]>,
}

impl Scope {
    fn new(home: Home, used: &[Id<System>], systems: &Tree<System>) -> Scope {
        let nearest: Box<[Home]> = match home {
            Home::System(system) => systems.lineage(system).map(Home::System).collect(),
            Home::Project | Home::Builtin => Box::new([home]),
        };
        let mut visible = vec![false; systems.len()];
        let own = nearest.iter().filter_map(|home| if let Home::System(system) = home { Some(*system) } else { None });
        for system in own.chain(used.iter().copied()) {
            for included in systems.lineage(system) {
                visible[included.index()] = true;
            }
        }
        Scope { nearest, visible: visible.into() }
    }

    pub fn sees(&self, home: Home) -> bool {
        match home {
            Home::Builtin => true,
            Home::Project => self.nearest[0] == Home::Project,
            Home::System(system) => self.visible[system.index()],
        }
    }

    /// How close a visible declaration is: 0 for the looker's own, then its
    /// ancestors in turn, then every used system alike.
    pub fn rank(&self, home: Home) -> usize {
        self.nearest.iter().position(|&near| near == home).unwrap_or(self.nearest.len())
    }
}

pub(crate) struct Scopes {
    project: Scope,
    systems: Vec<Scope>,
}

impl Scopes {
    /// `used` lists each home's `use` directives, resolved.
    pub fn new(systems: &Tree<System>, used: impl Fn(Home) -> Vec<Id<System>>) -> Scopes {
        let of = |home| Scope::new(home, &used(home), systems);
        Scopes { project: of(Home::Project), systems: systems.ids().map(|id| of(Home::System(id))).collect() }
    }

    pub fn of(&self, home: Home) -> &Scope {
        match home {
            Home::System(system) => &self.systems[system.index()],
            Home::Project | Home::Builtin => &self.project,
        }
    }
}
