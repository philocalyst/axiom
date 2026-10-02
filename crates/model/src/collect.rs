//! One look at every item of every source, sorted into typed buckets.
//!
//! Each pass over the declarations wants only some kind of item: the commodities, the contracts, the
//! settings. Walking every item of every file to find them made the model read the whole project about thirty
//! times. The look is made once instead, a file at a time on every core, and the files' buckets are laid end
//! to end in file order, so a bucket holds its kind's items in the order they were written. That is the order
//! the declarations are made in and the diagnostics come in; a pass that depends on the order of two kinds
//! relative to each other (laws and the declarations around them) still walks the items itself.
//!
//! A bucket is a `Vec` of [`Written`]: the node, its item and its source, borrowed. A pass that reads one
//! field of many items still reads one contiguous run.

use axiom_core::par;
use axiom_syntax::{Contract, Decl, DeclKind, File, Format, Item, ItemKind, NamedPattern, Opening, Param, Setting};
use axiom_syntax::{Statement, Sync, Txn};

use crate::scope::Home;
use crate::sources::Site;

/// Where an item stands among the items of every source: sources in the order they were arranged, items in the
/// order they were written. Items of different kinds compare by it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) struct Order {
    site: u32,
    item: u32,
}

/// A node of one kind of item, with the item and the source it was written in.
pub(crate) struct Written<'a, 's, T> {
    pub site: &'a Site<'a, 's>,
    pub item: &'a Item<'s>,
    pub node: &'a T,
    pub order: Order,
}

impl<T> Clone for Written<'_, '_, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Written<'_, '_, T> {}

impl<'a, 's, T> Written<'a, 's, T> {
    pub fn file(&self) -> &'a File<'s> {
        &self.site.source.file
    }

    pub fn home(&self) -> Home {
        self.site.home
    }
}

/// Every item the model lowers one kind at a time, in the order written.
#[derive(Default)]
pub(crate) struct Collected<'a, 's> {
    pub txns: Vec<Written<'a, 's, Txn<'s>>>,
    pub statements: Vec<Written<'a, 's, Statement<'s>>>,
    pub openings: Vec<Written<'a, 's, Opening<'s>>>,
    pub contracts: Vec<Written<'a, 's, Contract<'s>>>,
    pub decls: Vec<Written<'a, 's, Decl<'s>>>,
    pub params: Vec<Written<'a, 's, Param<'s>>>,
    pub patterns: Vec<Written<'a, 's, NamedPattern<'s>>>,
    pub formats: Vec<Written<'a, 's, Format<'s>>>,
    pub syncs: Vec<Written<'a, 's, Sync<'s>>>,
    pub settings: Vec<Written<'a, 's, Setting<'s>>>,
}

impl<'a, 's> Collected<'a, 's> {
    pub fn of(sites: &'a [Site<'a, 's>]) -> Collected<'a, 's> {
        let indexed: Vec<_> = sites.iter().enumerate().collect();
        let mut all = Collected::default();
        for found in par::map_each(&indexed, |&(at, site)| Collected::of_site(at as u32, site)) {
            all.append(found);
        }
        all
    }

    /// The declarations of one kind, in the order written.
    pub fn decls_of(&self, what: DeclKind) -> impl Iterator<Item = &Written<'a, 's, Decl<'s>>> {
        self.decls.iter().filter(move |written| written.node.what == what)
    }

    fn of_site(at: u32, site: &'a Site<'a, 's>) -> Collected<'a, 's> {
        let file = &site.source.file;
        let mut found = Collected::default();
        for (item_at, item) in file.items.iter().enumerate() {
            let order = Order { site: at, item: item_at as u32 };
            match item.kind {
                ItemKind::Txn(id) => found.txns.push(Written { site, item, node: &file[id], order }),
                ItemKind::Statement(id) => found.statements.push(Written { site, item, node: &file[id], order }),
                ItemKind::Opening(id) => found.openings.push(Written { site, item, node: &file[id], order }),
                ItemKind::Contract(id) => found.contracts.push(Written { site, item, node: &file[id], order }),
                ItemKind::Decl(id) => found.decls.push(Written { site, item, node: &file[id], order }),
                ItemKind::Param(id) => found.params.push(Written { site, item, node: &file[id], order }),
                ItemKind::Pattern(id) => found.patterns.push(Written { site, item, node: &file[id], order }),
                ItemKind::Format(id) => found.formats.push(Written { site, item, node: &file[id], order }),
                ItemKind::Sync(id) => found.syncs.push(Written { site, item, node: &file[id], order }),
                ItemKind::Setting(id) => found.settings.push(Written { site, item, node: &file[id], order }),
                ItemKind::Budget(_) | ItemKind::Code(_) | ItemKind::Law(_) => {}
            }
        }
        found
    }

    /// Lays `later` after what is here: its sources were arranged after these.
    fn append(&mut self, mut later: Collected<'a, 's>) {
        self.txns.append(&mut later.txns);
        self.statements.append(&mut later.statements);
        self.openings.append(&mut later.openings);
        self.contracts.append(&mut later.contracts);
        self.decls.append(&mut later.decls);
        self.params.append(&mut later.params);
        self.patterns.append(&mut later.patterns);
        self.formats.append(&mut later.formats);
        self.syncs.append(&mut later.syncs);
        self.settings.append(&mut later.settings);
    }
}

#[cfg(test)]
mod tests {
    use axiom_core::{FileId, Interner};
    use axiom_syntax::{Folder, parse};

    use super::*;
    use crate::Source;
    use crate::sources;

    fn source<'s>(id: u16, path: &'s str, text: &'s str, embedded: bool) -> Source<'s> {
        Source { path, file: parse(FileId(id), text, Folder::default()).0, embedded }
    }

    #[test]
    fn each_bucket_holds_its_items_in_written_order_and_kinds_compare_by_where_they_stand() {
        let std = "system std\nkind person : entity\nentity a : person\n";
        let project =
            "use std\nentity b : person\ncontract c with b\n  10 USD monthly on 1 from checking\nentity d : person\n";
        let sources = [source(0, "std.ax", std, true), source(1, "axiom.ax", project, false)];
        let (sites, _, _) = sources::arrange(&sources, &mut Interner::default(), &mut Vec::new());

        let collected = Collected::of(&sites);

        let declared: Vec<_> = collected.decls.iter().map(|written| written.node.name.0).collect();
        assert_eq!(declared, ["person", "a", "b", "d"], "files in arranged order, items in written order");
        let entities: Vec<_> = collected.decls_of(DeclKind::Entity).map(|written| written.node.name.0).collect();
        assert_eq!(entities, ["a", "b", "d"]);
        assert_eq!(collected.settings.len(), 2, "`system std` and `use std`");
        let (before, contract, after) =
            (collected.decls[2].order, collected.contracts[0].order, collected.decls[3].order);
        assert!(before < contract && contract < after);
        assert!(collected.decls[1].order < before, "a system's items come before the project's");
    }
}
