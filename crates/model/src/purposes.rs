//! Purposes: what flows are for.
//!
//! The four roots (income, spending, capital, transfer) and every written purpose make one tree, which
//! [`taxonomy`](crate::taxonomy) builds. This module says what is particular to purposes: the object a purpose
//! takes (`of KIND`).

use axiom_core::{Diagnostic, Id, Interner, Loc, Sym};
use axiom_syntax::{Decl, DeclKind};

use crate::args::Args;
use crate::book::{At, Kind, Miss, Purpose, PurposeRoot, PurposeRoots};
use crate::collect::{Collected, Written};
use crate::errors::{Reported, Word};
use crate::kinds;
use crate::names::Scoped;
use crate::problem::{self, Among, Noun};
use crate::scope::{Home, Seeing};
use crate::taxonomy::{Node, Repeated, Taxonomy};

const INCOME: usize = 0;
const SPENDING: usize = 1;
const CAPITAL: usize = 2;
const TRANSFER: usize = 3;

/// Each root's place among the roots of what a purpose is.
const KINDS: [PurposeRoot; 4] =
    [PurposeRoot::Income, PurposeRoot::Spending, PurposeRoot::Capital, PurposeRoot::Transfer];

impl Purpose {
    fn blank(name: Sym, root: PurposeRoot) -> Purpose {
        Purpose { name, root, system: None, of: None, laws: Box::default(), doc: None, loc: None }
    }
}

impl Node for Purpose {
    const NOUN: Noun = Noun::Purpose;
    const DECL: DeclKind = DeclKind::Purpose;
    const ROOTS: &'static [(&'static str, Option<usize>)] =
        &[("income", None), ("spending", None), ("capital", None), ("transfer", None)];
    const ORPHAN: usize = TRANSFER;
    const REPEATED_ROOT: Repeated = Repeated::Allowed;
    const PARENT_QUESTION: &'static str = "what is this purpose a kind of?";

    fn root(name: Sym, at: usize) -> Purpose {
        Purpose::blank(name, KINDS[at])
    }

    fn declared<'s>(at: &Written<'_, 's, Decl<'s>>, names: &mut Interner<'s>) -> Purpose {
        let mut purpose = Purpose::blank(names.intern(at.node.name.0), PurposeRoot::Transfer);
        purpose.system = match at.home() {
            Home::System(system) => Some(system),
            Home::Project | Home::Builtin => None,
        };
        purpose.doc = at.item.doc.map(|doc| names.intern(doc.0));
        purpose.loc = Some(at.file().loc(at.node.name.0));
        purpose
    }

    /// A purpose is of the root it hangs beneath.
    fn adopt(&mut self, parent: &Purpose) {
        self.root = parent.root;
    }

    fn name(&self) -> Sym {
        self.name
    }

    fn loc(&self) -> Option<Loc> {
        self.loc
    }

    fn unresolved(miss: Miss<Purpose>, word: Word, among: &Among<Purpose>, nodes: &[Purpose]) -> Diagnostic {
        let describe = |ids: &[Id<Purpose>]| {
            let (name, loc) = (|id: Id<Purpose>| nodes[id.index()].name, |id: Id<Purpose>| nodes[id.index()].loc);
            problem::shortest(among.names, &among.index.names, ids, name, loc)
        };
        among.failed(miss, Noun::Purpose, word, describe)
    }
}

/// The roots, by the names the rest of the model gives them.
pub(crate) fn roots(ids: &[Id<Purpose>]) -> PurposeRoots {
    PurposeRoots { income: ids[INCOME], spending: ids[SPENDING], capital: ids[CAPITAL], transfer: ids[TRANSFER] }
}

/// `of KIND` is the only property that determines a purpose's object type.
pub(crate) fn attach_objects<'s>(
    purposes: &mut Taxonomy<Purpose>,
    collected: &Collected<'_, 's>,
    names: &Interner<'s>,
    seeing: Seeing<'_>,
    kind_index: &Scoped<Kind>,
    diags: &mut Vec<Diagnostic>,
) {
    for (written, &id) in collected.decls_of(DeclKind::Purpose).zip(&purposes.declarations) {
        let (file, decl) = (written.file(), written.node);
        if purposes.roots.contains(&id) {
            continue;
        }
        for prop in file[decl.props].iter().filter(|prop| prop.name.0 == "of") {
            let mut args = Args::shaped(file, prop, "purpose-object", "of KIND");
            let Some(kind_name) = args.name("a kind").and_then(|word| args.done().map(|()| word)).or_report(diags)
            else {
                continue;
            };
            let scope = seeing.scopes.of(written.home());
            match kinds::find(kind_index, names, seeing.systems, kind_name.text, |visible| scope.sees(visible)) {
                Ok(kind) => purposes.tree[id].of = Some(At { value: kind, loc: prop.loc }),
                Err(_) => diags.push(
                    Diagnostic::error("unknown-kind", format!("kind `{}` is not known here", kind_name.text))
                        .label(kind_name.loc, "not a visible kind"),
                ),
            }
        }
    }
}
