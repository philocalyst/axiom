//! Places: where value can be.
//!
//! The five class roots and `equity/unknown` always exist. Declared accounts
//! add to them, and so does any full path under a class root that is written
//! anywhere: writing `expenses/food/snacks` opens it. Undeclared places are
//! `assets/…`, `expenses/…` and so on; the root of the path is the class.

use axiom_core::diag::closest;
use axiom_core::{Diagnostic, Id, Tree};
use axiom_syntax::Decl;

use crate::book::{Class, Place, Sort};
use crate::catalog::Catalog;
use crate::cx::Cx;
use crate::entities::Entities;
use crate::errors::duplicate;
use crate::kinds::Kinds;
use crate::names::Names;
use crate::paths;
use crate::scope::Home;
use crate::survey::class_of;

/// `?` in a flow: where value of unknown origin comes from and unexplained
/// value goes.
pub(crate) const UNKNOWN: &str = "equity/unknown";

pub(crate) struct Places {
    pub tree: Tree<Place>,
    pub names: Names<Place>,
    pub unknown: Id<Place>,
    /// The id of each declaration in the catalog, in catalog order; none for an
    /// account whose path starts at no class root.
    pub declared: Vec<Option<Id<Place>>>,
}

pub(crate) fn declare<'s>(
    catalog: &Catalog<'_, 's>,
    opened: &[&'s str],
    kinds: &Kinds,
    entities: &Entities,
    cx: &mut Cx<'_, 's>,
) -> Places {
    // An account outside every class root has no class, so it cannot exist.
    let valid: Vec<bool> = catalog.accounts.iter().map(|written| root_is_valid(written.what, cx)).collect();
    let accounts = || catalog.accounts.iter().zip(&valid).filter(|&(_, &valid)| valid).map(|(written, _)| written);
    let roots = Class::ALL.map(Class::root);
    let written = roots.into_iter().chain([UNKNOWN]).chain(opened.iter().copied());
    let written = written.chain(accounts().map(|account| account.what.name.text));
    let (mut tree, by_path) = paths::build(written, |path| {
        let class = class_of(path).expect("only paths under a class root are written");
        Place {
            path: cx.names.intern(path),
            class,
            kind: kinds.roots.of_class(class),
            owner: entities.me,
            holds: None,
            select: None,
            deferred: false,
            liquidity: None,
            opened: None,
            closed: None,
            props: Box::default(),
            doc: None,
            loc: None,
        }
    });

    let mut declared = vec![None; catalog.accounts.len()];
    for (at, written) in catalog.accounts.iter().enumerate().filter(|&(at, _)| valid[at]) {
        let decl = written.what;
        let id = by_path[decl.name.text];
        declared[at] = Some(id);
        if let Some(first) = tree[id].loc {
            cx.diags.push(duplicate("account", decl.name, Some(first)));
            continue;
        }
        let class = tree[id].class;
        let thing = format!("the {} account `{}`", Sort::Place(class).noun(), decl.name.text);
        let kind = kinds.declared(decl, Home::Project, Sort::Place(class), &thing, cx);
        let place = &mut tree[id];
        let kind_facts = &kinds.tree[kind];
        (place.kind, place.deferred) = (kind, kind_facts.deferred);
        (place.select, place.liquidity) = (kind_facts.select, kind_facts.liquidity);
        place.doc = written.item.doc.map(|doc| cx.names.intern(doc.0));
        place.loc = Some(decl.name.loc);
    }

    let mut names = Names::default();
    let paths: Vec<_> = tree.iter().map(|(id, place)| (id, cx.names.name(place.path))).collect();
    for (id, path) in paths {
        names.insert(cx.names, path, id);
    }
    Places { unknown: by_path[UNKNOWN], tree, names, declared }
}

/// Whether `decl` starts at a class root; if not, says so.
fn root_is_valid(decl: &Decl, cx: &mut Cx) -> bool {
    let path = decl.name;
    if class_of(path.text).is_some() {
        return true;
    }
    let roots = Class::ALL.map(Class::root);
    let mut diagnostic = Diagnostic::error("account-root", format!("`{}` does not start at a class root", path.text))
        .label(path.loc, "an account belongs to one of the five classes")
        .note("an account's path starts with `assets`, `liabilities`, `income`, `expenses` or `equity`, and that root is its class");
    let first = paths::root_of(path.text);
    if let Some(near) = closest(first, roots) {
        let fixed = format!("{near}{}", &path.text[first.len()..]);
        diagnostic = diagnostic.fix(format!("did you mean `{fixed}`?"), path.loc, fixed);
    }
    cx.diags.push(diagnostic);
    false
}
