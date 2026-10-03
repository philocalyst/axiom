//! Kinds: what things are.
//!
//! Kinds form one tree under the built-in roots: the two classes of account, identified things, commodities, measures,
//! entities and contracts. [`taxonomy`](crate::taxonomy) builds it; this module says what is particular to kinds.

use axiom_core::{Diagnostic, Id, Interner, Loc, Run, Sym, Tree};
use axiom_syntax::{Decl, DeclKind};

use crate::book::{Class, Kind, KindRoots, Miss, Sort, System};
use crate::collect::Written;
use crate::errors::{Candidate, Word};
use crate::names::Scoped;
use crate::problem::{Among, Noun};
use crate::scope::{Home, Scope};
use crate::taxonomy::{Node, Repeated};

impl Kind {
    /// A kind that says nothing yet.
    pub(crate) fn blank(name: Sym, sort: Sort) -> Kind {
        Kind { name, sort, system: None, slots: Run::default(), laws: Box::default(), doc: None, loc: None }
    }
}

const ASSET: usize = 0;
const DEBT: usize = 1;
const THING: usize = 2;
const COMMODITY: usize = 3;
const MEASURE: usize = 4;
const ENTITY: usize = 5;
const CLAIM: usize = 6;
const DEBT_CLAIM: usize = 7;
const CONTRACT: usize = 8;

/// Each root's sort.
const SORTS: [Sort; 9] = [
    Sort::Place(Class::Asset),
    Sort::Place(Class::Debt),
    Sort::Thing,
    Sort::Commodity,
    Sort::Commodity,
    Sort::Entity,
    Sort::Place(Class::Asset),
    Sort::Place(Class::Debt),
    Sort::Contract,
];

impl Node for Kind {
    const NOUN: Noun = Noun::Kind;
    const DECL: DeclKind = DeclKind::Kind;
    const ROOTS: &'static [(&'static str, Option<usize>)] = &[
        ("asset", None),
        ("debt", None),
        ("thing", None),
        ("commodity", None),
        ("measure", Some(COMMODITY)),
        ("entity", None),
        ("claim", Some(ASSET)),
        ("debt-claim", Some(DEBT)),
        ("contract", None),
    ];
    const ORPHAN: usize = THING;
    const REPEATED_ROOT: Repeated = Repeated::Said;
    const PARENT_QUESTION: &'static str = "what kind of thing is this?";

    fn root(name: Sym, at: usize) -> Kind {
        Kind::blank(name, SORTS[at])
    }

    fn declared<'s>(at: &Written<'_, 's, Decl<'s>>, names: &mut Interner<'s>) -> Kind {
        let mut kind = Kind::blank(names.intern(at.node.name.0), Sort::Thing);
        kind.system = match at.home() {
            Home::System(system) => Some(system),
            Home::Project | Home::Builtin => None,
        };
        kind.doc = at.item.doc.map(|doc| names.intern(doc.0));
        kind.loc = Some(at.file().loc(at.node.name.0));
        kind
    }

    /// A kind is of the sort of the kind it is beneath.
    fn adopt(&mut self, parent: &Kind) {
        self.sort = parent.sort;
    }

    fn name(&self) -> Sym {
        self.name
    }

    fn loc(&self) -> Option<Loc> {
        self.loc
    }

    fn resolve(
        index: &Scoped<Kind>,
        names: &Interner,
        systems: &Tree<System>,
        text: &str,
        scope: &Scope,
    ) -> Result<Id<Kind>, Miss<Kind>> {
        find(index, names, systems, text, |visible| scope.sees(visible))
    }

    fn unresolved(miss: Miss<Kind>, word: Word, among: &Among<Kind>, nodes: &[Kind]) -> Diagnostic {
        unresolved(miss, word, among, |id| &nodes[id.index()])
    }
}

/// The roots, by the names the rest of the model gives them.
pub(crate) fn roots(ids: &[Id<Kind>]) -> KindRoots {
    KindRoots {
        asset: ids[ASSET],
        debt: ids[DEBT],
        thing: ids[THING],
        commodity: ids[COMMODITY],
        measure: ids[MEASURE],
        entity: ids[ENTITY],
        claim: ids[CLAIM],
        debt_claim: ids[DEBT_CLAIM],
        contract: ids[CONTRACT],
    }
}

/// A kind by name (`401k`), qualified by its system (`us/401k/401k`), or by a
/// system named for it (`us/401k`). Only bare names depend on what `seen`
/// admits: a qualified name always resolves.
pub(crate) fn find(
    kinds: &Scoped<Kind>,
    names: &Interner,
    systems: &Tree<System>,
    text: &str,
    seen: impl Fn(Home) -> bool,
) -> Result<Id<Kind>, Miss<Kind>> {
    let Some((head, name)) = text.rsplit_once('/') else {
        return kinds.names.resolve(names, text, |id| seen(kinds.home(id)));
    };
    let declared_by = |id: Id<Kind>| match kinds.home(id) {
        Home::System(system) => {
            let path = names.name(systems[system].path);
            path == head || path == text
        }
        Home::Project | Home::Builtin => false,
    };
    kinds.names.resolve(names, name, declared_by)
}

/// Why `word` named no single kind. A kind of a system is written `system/kind`, which is how it is offered.
pub(crate) fn unresolved<'k>(
    miss: Miss<Kind>,
    word: Word,
    among: &Among<Kind>,
    kind_of: impl Fn(Id<Kind>) -> &'k Kind,
) -> Diagnostic {
    let describe = |&id: &Id<Kind>| {
        let (declared, name) = (kind_of(id).loc, among.names.name(kind_of(id).name));
        match (among.index.home(id), among.system_of(id)) {
            (Home::System(_), Some(system)) => {
                let last = system.rsplit('/').next().unwrap_or(system);
                // `us/401k` names the kind `401k` of that system, and is the shorter way to say it.
                let write = if last == word.text { system.to_string() } else { format!("{system}/{}", word.text) };
                Candidate { is: format!("`{}` from `{system}`", word.text), declared, write: Some(write) }
            }
            (Home::Builtin, _) => Candidate { is: format!("the built-in `{name}`"), declared, write: None },
            _ => Candidate { is: format!("the project's `{name}`"), declared, write: None },
        }
    };
    among.failed(miss, Noun::Kind, word, |ids| ids.iter().map(describe).collect())
}
