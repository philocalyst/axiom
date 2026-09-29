//! Entities: who value moves between.
//!
//! They form a path tree (`paypal/john` sits under `paypal`), with `me` always
//! present. Here entities are created and given their kinds; `via` and `lives`
//! wait until places and systems can be named.

use axiom_core::{Id, Tree};

use crate::book::{Entity, Sort};
use crate::catalog::Catalog;
use crate::cx::Cx;
use crate::errors::duplicate;
use crate::kinds::{self, Kinds};
use crate::names::Scoped;
use crate::paths;
use crate::scope::Home;

pub(crate) struct Entities {
    pub tree: Tree<Entity>,
    pub index: Scoped<Entity>,
    pub me: Id<Entity>,
    /// The id of each declaration in the catalog, in catalog order.
    pub declared: Vec<Id<Entity>>,
}

pub(crate) fn declare<'s>(catalog: &Catalog<'_, 's>, kinds: &Kinds, cx: &mut Cx<'_, 's>) -> Entities {
    let written = catalog.entities.iter().map(|written| written.what.name.text).chain(["me"]);
    let (mut tree, by_path) = paths::build(written, |path| Entity {
        path: cx.names.intern(path),
        kind: kinds.roots.entity,
        via: None,
        restricted: false,
        lives: Box::default(),
        member: None,
        props: Box::default(),
        doc: None,
        loc: None,
    });

    let mut homes = vec![Home::Builtin; tree.len()];
    let mut declared = Vec::with_capacity(catalog.entities.len());
    for written in &catalog.entities {
        let decl = written.what;
        let id = by_path[decl.name.text];
        declared.push(id);
        if let Some(first) = tree[id].loc {
            cx.diags.push(duplicate("entity", decl.name, Some(first)));
            continue;
        }
        homes[id.index()] = written.home();
        let thing = format!("the entity `{}`", decl.name.text);
        let kind = kinds.declared(decl, written.home(), Sort::Entity, &thing, cx);
        let entity = &mut tree[id];
        entity.kind = kind;
        entity.restricted = kinds.tree[kind].restricted;
        entity.doc = written.item.doc.map(|doc| cx.names.intern(doc.0));
        entity.loc = Some(decl.name.loc);
    }

    let me = by_path["me"];
    if tree[me].loc.is_none() {
        tree[me].kind = person_or_root(kinds, cx);
        tree[me].restricted = kinds.tree[tree[me].kind].restricted;
    }
    let things: Vec<_> = tree.iter().map(|(id, entity)| (id, cx.names.name(entity.path), homes[id.index()])).collect();
    let index = Scoped::build(cx.names, things);
    Entities { tree, index, me, declared }
}

/// `me` is a `person` when the standard kinds are in scope.
fn person_or_root(kinds: &Kinds, cx: &Cx) -> Id<crate::book::Kind> {
    let scope = cx.scopes.of(Home::Project);
    let found = kinds::find(&kinds.index, cx.names, cx.systems, "person", |home| scope.sees(home));
    match found {
        Ok(person) if kinds.tree[person].sort == Sort::Entity => person,
        _ => kinds.roots.entity,
    }
}
