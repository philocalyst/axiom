//! Every item of every source, sorted by what it is.
//!
//! Later stages want "all the kinds" or "all the transactions", not a walk over
//! files. The catalog does that walk once, keeps where each item was written,
//! and rejects items that do not belong where they were written: a system
//! declares kinds, entities, commodities, params, codes and laws, and nothing
//! else.

use axiom_core::{Diagnostic, Loc};
use axiom_syntax::{
    Assert, CodeRule, Decl, DeclKind, Event, Exprs, Item, ItemKind, Law, Name, Param, Plan, Price, Setting, Sync, Txn,
};

use crate::Source;
use crate::layout::Layout;
use crate::scope::Home;

/// One source and where its declarations live.
pub(crate) struct Site<'a, 's> {
    pub home: Home,
    pub source: &'a Source<'s>,
    /// What the file's place among the folders says it may hold.
    pub layout: Layout<'s>,
}

/// An item of kind `T`, with where it was written.
pub(crate) struct Written<'a, 's, T> {
    pub site: &'a Site<'a, 's>,
    pub item: &'a Item<'s>,
    pub what: &'a T,
}

impl<T> Clone for Written<'_, '_, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Written<'_, '_, T> {}

impl<'a, 's, T> Written<'a, 's, T> {
    pub fn new(site: &'a Site<'a, 's>, item: &'a Item<'s>, what: &'a T) -> Written<'a, 's, T> {
        Written { site, item, what }
    }

    /// The expressions of the file this item was written in.
    pub fn exprs(&self) -> &'a Exprs<'s> {
        &self.site.source.file.exprs
    }

    pub fn home(&self) -> Home {
        self.site.home
    }
}

#[derive(Default)]
pub(crate) struct Catalog<'a, 's> {
    pub uses: Vec<(Home, Name<'s>)>,
    pub kinds: Vec<Written<'a, 's, Decl<'s>>>,
    pub commodities: Vec<Written<'a, 's, Decl<'s>>>,
    pub entities: Vec<Written<'a, 's, Decl<'s>>>,
    pub accounts: Vec<Written<'a, 's, Decl<'s>>>,
    /// Laws written at the top level of a system or the project.
    pub laws: Vec<Written<'a, 's, Law<'s>>>,
    pub params: Vec<Written<'a, 's, Param<'s>>>,
    pub codes: Vec<Written<'a, 's, CodeRule<'s>>>,
    pub txns: Vec<Written<'a, 's, Txn<'s>>>,
    pub asserts: Vec<Written<'a, 's, Assert<'s>>>,
    pub events: Vec<Written<'a, 's, Event<'s>>>,
    pub prices: Vec<Written<'a, 's, Price<'s>>>,
    pub plans: Vec<Written<'a, 's, Plan<'s>>>,
    pub syncs: Vec<Written<'a, 's, Sync<'s>>>,
    pub base: Option<Name<'s>>,
    pub relaxed: bool,
    pub layout_free: bool,
}

impl<'a, 's> Catalog<'a, 's> {
    /// Adds the items of one source from index `first` on: a run of them.
    pub fn read_run(
        &mut self,
        site: &'a Site<'a, 's>,
        first: usize,
        items: &'a [Item<'s>],
        diags: &mut Vec<Diagnostic>,
    ) {
        for (offset, item) in items.iter().enumerate() {
            match self.check_placement(site, first + offset, item) {
                Ok(()) => self.add(site, item, diags),
                Err(misplaced) => diags.push(misplaced),
            }
        }
    }

    /// Adds everything `later` collected from items that come after this one's.
    pub fn merge(&mut self, later: Catalog<'a, 's>, diags: &mut Vec<Diagnostic>) {
        self.uses.extend(later.uses);
        self.kinds.extend(later.kinds);
        self.commodities.extend(later.commodities);
        self.entities.extend(later.entities);
        self.accounts.extend(later.accounts);
        self.laws.extend(later.laws);
        self.params.extend(later.params);
        self.codes.extend(later.codes);
        self.txns.extend(later.txns);
        self.asserts.extend(later.asserts);
        self.events.extend(later.events);
        self.prices.extend(later.prices);
        self.plans.extend(later.plans);
        self.syncs.extend(later.syncs);
        match (self.base, later.base) {
            (Some(first), Some(again)) if first.text != again.text => diags.push(conflicting_base(first.loc, again)),
            (None, base) => self.base = base,
            _ => {}
        }
        self.relaxed |= later.relaxed;
        self.layout_free |= later.layout_free;
    }

    fn check_placement(&self, site: &Site, at: usize, item: &Item) -> Result<(), Diagnostic> {
        let in_system = matches!(site.home, Home::System(_));
        let (belongs, what) = match &item.kind {
            ItemKind::Setting(Setting::System(_)) if at > 0 => {
                return Err(Diagnostic::error("system-position", "a system must be the first item of its file")
                    .label(item.loc, "this file's first item is something else")
                    .help("move `system` to the top, or remove it to make this a project file"));
            }
            ItemKind::Setting(Setting::System(_) | Setting::Use(_)) => (true, ""),
            ItemKind::Decl(decl) if decl.what == DeclKind::Account => (!in_system, "an account"),
            ItemKind::Decl(_) | ItemKind::Code(_) | ItemKind::Param(_) | ItemKind::Law(_) => (true, ""),
            ItemKind::Txn(_) => (!in_system, "a transaction"),
            ItemKind::Assert(_) => (!in_system, "a balance assertion"),
            ItemKind::Event(_) => (!in_system, "a settlement event"),
            ItemKind::Price(_) => (!in_system, "a price"),
            ItemKind::Plan(_) => (!in_system, "a plan"),
            ItemKind::Sync(_) => (!in_system, "a sync"),
            ItemKind::Setting(_) => (!in_system, "a project setting"),
        };
        if belongs {
            return Ok(());
        }
        Err(Diagnostic::error("system-item", format!("a system cannot contain {what}"))
            .label(item.loc, "this belongs in the project")
            .note("systems declare kinds, entities, commodities, params, codes and laws; the project keeps accounts and the journal"))
    }

    fn add(&mut self, site: &'a Site<'a, 's>, item: &'a Item<'s>, diags: &mut Vec<Diagnostic>) {
        match &item.kind {
            ItemKind::Decl(decl) => match decl.what {
                DeclKind::Kind => self.kinds.push(Written::new(site, item, decl)),
                DeclKind::Commodity => self.commodities.push(Written::new(site, item, decl)),
                DeclKind::Entity => self.entities.push(Written::new(site, item, decl)),
                DeclKind::Account => self.accounts.push(Written::new(site, item, decl)),
            },
            ItemKind::Law(law) => self.laws.push(Written::new(site, item, law)),
            ItemKind::Param(param) => self.params.push(Written::new(site, item, param)),
            ItemKind::Code(rule) => self.codes.push(Written::new(site, item, rule)),
            ItemKind::Txn(txn) => self.txns.push(Written::new(site, item, txn)),
            ItemKind::Assert(assert) => self.asserts.push(Written::new(site, item, assert)),
            ItemKind::Event(event) => self.events.push(Written::new(site, item, event)),
            ItemKind::Price(price) => self.prices.push(Written::new(site, item, price)),
            ItemKind::Plan(plan) => self.plans.push(Written::new(site, item, plan)),
            ItemKind::Sync(sync) => self.syncs.push(Written::new(site, item, sync)),
            ItemKind::Setting(setting) => self.setting(site, setting, diags),
        }
    }

    fn setting(&mut self, site: &Site<'a, 's>, setting: &Setting<'s>, diags: &mut Vec<Diagnostic>) {
        match *setting {
            Setting::System(_) => {}
            Setting::Use(name) => self.uses.push((site.home, name)),
            Setting::Base(name) => match self.base {
                Some(first) if first.text != name.text => diags.push(conflicting_base(first.loc, name)),
                _ => self.base = Some(name),
            },
            Setting::Relaxed(_) => self.relaxed = true,
            Setting::LayoutFree(_) => self.layout_free = true,
        }
    }
}

fn conflicting_base(first: Loc, again: Name) -> Diagnostic {
    Diagnostic::error("duplicate-base", "the base currency is set twice")
        .label(again.loc, format!("`base {}` here", again.text))
        .context(first, "and here")
        .help("a book has one base currency: remove one of them")
}
