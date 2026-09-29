//! One look at everything written, before anything is resolved.
//!
//! Declarations are sorted out of the items into [`Entry`]s, in the order they
//! were written. Alongside, the look gathers what needs every source at once
//! and decides how everything else is built: how many decimals a commodity
//! needs (the most any amount written in it uses), which full paths open
//! places, and which texts (codes, docs, reasons) the parallel elaboration will
//! need as symbols, so that they are all interned before it starts and it only
//! ever reads.
//!
//! Each source is looked at in runs of consecutive items, on every core, and
//! the partial results are merged in order: what was written first stays first.

use axiom_core::{Diagnostic, Interner, Loc, Map, Set, par};
use axiom_syntax::{
    Amount, Assert, Clause, ClauseKind, CodeRule, Decl, DeclKind, ExprKind, File, For, Gap, Item, ItemKind, Law, Leg,
    Occurrence, Param, Plan, Price, Prop, Quantity, Select, Setting, Split, Sync, Txn,
};

use crate::book::Class;
use crate::scope::Home;
use crate::sources::Site;

/// An item of kind `T`, with where it was written.
pub(crate) struct Written<'a, 's, T> {
    pub site: &'a Site<'a, 's>,
    pub item: &'a Item<'s>,
    pub node: &'a T,
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

/// A declaration or directive, kept in the order it was written.
pub(crate) enum Entry<'a, 's> {
    Decl(Written<'a, 's, Decl<'s>>),
    Law(Written<'a, 's, Law<'s>>),
    Param(Written<'a, 's, Param<'s>>),
    Code(Written<'a, 's, CodeRule<'s>>),
    Sync(Written<'a, 's, Sync<'s>>),
    Plan(Written<'a, 's, Plan<'s>>),
    Setting(&'a Site<'a, 's>, Setting<'s>),
}

/// Some consecutive items of one source.
#[derive(Clone, Copy)]
pub(crate) struct Run<'a, 's> {
    pub site: &'a Site<'a, 's>,
    pub items: &'a [Item<'s>],
}

/// A commodity that appears in the sources, and how precise it must be.
pub(crate) struct Seen<'s> {
    pub symbol: &'s str,
    /// The most decimal places any amount written in it uses.
    pub places: u8,
    /// Where it was first written.
    pub first: Loc,
    /// Whether the journal (not just a law or a property) says it.
    pub journal: bool,
}

impl Seen<'_> {
    /// Takes in another sighting of the same commodity. Where it was first
    /// written is where the journal first says it, or else the earliest
    /// sighting: `sooner` says whether this one is that.
    fn absorb(&mut self, other: &Seen, sooner: bool) {
        if match (other.journal, self.journal) {
            (true, false) => true,
            (false, true) => false,
            _ => sooner,
        } {
            self.first = other.first;
        }
        (self.places, self.journal) = (self.places.max(other.places), self.journal | other.journal);
    }
}

/// What one run of items declares, and how many transactions it makes.
#[derive(Default)]
struct Declared<'a, 's> {
    entries: Vec<Entry<'a, 's>>,
    diags: Vec<Diagnostic>,
    /// Items that will each make one transaction.
    txns: usize,
}

/// What a source says about commodities, places and texts, found by looking at
/// each kind of node in turn wherever it sits, so that nothing here depends on
/// how the items are nested.
#[derive(Default)]
struct Facts<'s> {
    units: Vec<Seen<'s>>,
    unit_at: Map<&'s str, usize>,
    paths: Vec<&'s str>,
    path_seen: Set<&'s str>,
    texts: Vec<&'s str>,
}

/// Everything the look found, merged.
pub(crate) struct Surveyed<'a, 's> {
    pub entries: Vec<Entry<'a, 's>>,
    pub units: Vec<Seen<'s>>,
    /// Full paths under a class root that the sources write: each opens a
    /// place unless it is a typo of a declared one.
    pub paths: Vec<&'s str>,
    /// The runs of journal items to elaborate, each with the number of the
    /// first transaction it makes.
    pub journal: Vec<(Run<'a, 's>, u32)>,
    /// How many transactions the journal makes in all.
    pub txns: u32,
    pub diags: Vec<Diagnostic>,
}

/// The class a full path belongs to, if it starts at a class root.
pub(crate) fn class_of(path: &str) -> Option<Class> {
    let bytes = path.as_bytes();
    let class = match bytes.first()? {
        b'a' => Class::Asset,
        b'l' => Class::Liability,
        b'i' => Class::Income,
        b'e' if bytes.get(1) == Some(&b'x') => Class::Expense,
        b'e' => Class::Equity,
        _ => return None,
    };
    let rest = path.strip_prefix(class.root())?;
    (rest.is_empty() || rest.starts_with('/')).then_some(class)
}

/// The decimals an amount needs: `84.20` needs one.
pub(crate) fn places_of(amount: Amount) -> u8 {
    let Some(dot) = amount.bytes().position(|byte| byte == b'.') else {
        return 0;
    };
    let fraction = &amount.as_bytes()[dot + 1..];
    let digits = fraction.iter().position(|byte| !byte.is_ascii_digit()).unwrap_or(fraction.len());
    fraction[..digits].iter().rposition(|&byte| byte != b'0').map_or(0, |last| last as u8 + 1)
}

pub(crate) fn survey<'a, 's>(sites: &'a [Site<'a, 's>], names: &mut Interner<'s>) -> Surveyed<'a, 's> {
    // Enough items to be worth a thread, and still many runs per core.
    const RUN: usize = 4096;
    let runs: Vec<Run> = sites
        .iter()
        .flat_map(|site| site.source.file.items.chunks(RUN).map(move |items| Run { site, items }))
        .collect();
    let (declared, facts) =
        par::join(|| par::map_each(&runs, Declared::of), || par::map_each(sites, |site| Facts::of(&site.source.file)));

    let (mut entries, mut diags, mut journal, mut first_txn) = (Vec::new(), Vec::new(), Vec::new(), 0u32);
    for (run, mut found) in runs.iter().zip(declared) {
        if run.site.home == Home::Project {
            journal.push((*run, first_txn));
        }
        first_txn += found.txns as u32;
        entries.append(&mut found.entries);
        diags.append(&mut found.diags);
    }
    let mut merged = Facts::default();
    facts.into_iter().for_each(|facts| merged.merge(facts));
    merged.texts.into_iter().for_each(|text| {
        names.intern(text);
    });
    Surveyed { entries, units: merged.units, paths: merged.paths, journal, txns: first_txn, diags }
}

impl<'a, 's> Declared<'a, 's> {
    fn of(run: &Run<'a, 's>) -> Declared<'a, 's> {
        let (site, file) = (run.site, &run.site.source.file);
        let mut found = Declared::default();
        for item in run.items {
            if let Err(misplaced) = placement(site, item) {
                found.diags.push(misplaced);
                continue;
            }
            match item.kind {
                ItemKind::Decl(id) => found.entries.push(Entry::Decl(Written { site, item, node: &file[id] })),
                ItemKind::Law(id) => found.entries.push(Entry::Law(Written { site, item, node: &file[id] })),
                ItemKind::Param(id) => found.entries.push(Entry::Param(Written { site, item, node: &file[id] })),
                ItemKind::Code(id) => found.entries.push(Entry::Code(Written { site, item, node: &file[id] })),
                ItemKind::Sync(id) => found.entries.push(Entry::Sync(Written { site, item, node: &file[id] })),
                ItemKind::Plan(id) => found.entries.push(Entry::Plan(Written { site, item, node: &file[id] })),
                ItemKind::Setting(id) => found.entries.push(Entry::Setting(site, file[id])),
                ItemKind::Txn(_) | ItemKind::Occurrence(_) | ItemKind::Opening(_) => found.txns += 1,
                ItemKind::Assert(_) | ItemKind::Event(_) | ItemKind::Price(_) | ItemKind::Split(_) => {}
            }
        }
        found
    }
}

impl<'s> Facts<'s> {
    fn of(file: &File<'s>) -> Facts<'s> {
        let mut facts = Facts::default();
        facts.texts.extend(file.items.iter().filter_map(|item| item.doc).map(|doc| doc.0));
        // The header of a transaction or plan; its legs, like those of an
        // occurrence or an opening, are in the table of legs.
        let flows = file.iter::<Txn>().map(|txn| &txn.flow).chain(file.iter::<Plan>().map(|plan| &plan.flow));
        for end in flows.flat_map(|flow| [&flow.from, &flow.to]) {
            end.place.iter().for_each(|place| facts.open(place.name.0));
            end.amount.iter().for_each(|quantity| facts.quantity(file, quantity));
        }
        for leg in file.iter::<Leg>() {
            facts.open(leg.place.name.0);
            facts.quantity(file, &leg.amount);
        }
        let codes = file.iter::<Select>().filter_map(|select| match select {
            Select::Code(code) => Some(code.name()),
            _ => None,
        });
        facts.texts.extend(codes);
        for clause in file.iter::<Clause>() {
            match clause.kind {
                ClauseKind::Code(code) | ClauseKind::For(For::Code(code)) => facts.texts.push(code.name()),
                ClauseKind::Price(amount) | ClauseKind::Basis(amount) => facts.amount(file, amount, true),
                ClauseKind::Waive(waive) => facts.texts.extend(waive.reason),
                ClauseKind::For(_) | ClauseKind::Due(_) | ClauseKind::Since(_) => {}
            }
        }
        for assert in file.iter::<Assert>() {
            facts.open(assert.place.name.0);
            facts.amount(file, assert.amount, true);
            match assert.gap {
                Gap::Waived(waive) => facts.texts.extend(waive.reason),
                Gap::Via(place) => facts.open(place.0),
                Gap::Refused => {}
            }
        }
        for price in file.iter::<Price>() {
            facts.unit(price.unit.0, 0, file.loc(price.unit.0), true);
            facts.amount(file, price.price, true);
        }
        for split in file.iter::<Split>() {
            facts.unit(split.unit.0, 0, file.loc(split.unit.0), true);
        }
        for occurrence in file.iter::<Occurrence>() {
            occurrence.amount.iter().for_each(|&amount| facts.amount(file, amount, true));
        }
        for prop in file.iter::<Prop>() {
            for &arg in &file[prop.args] {
                if let ExprKind::Name(path) = file.exprs[arg].kind {
                    facts.open(path.0);
                }
            }
        }
        // Amounts and units written inside expressions: properties, params, laws.
        for expr in file.exprs.iter() {
            match expr.kind {
                ExprKind::Amount(amount) => facts.amount(file, amount, false),
                ExprKind::Unit(unit) => facts.unit(unit.0, 0, file.loc(unit.0), false),
                _ => {}
            }
        }
        facts.units.sort_by_key(|seen| seen.first.start);
        facts
    }

    /// Adds what a later source saw: what was written first stays first.
    fn merge(&mut self, later: Facts<'s>) {
        for seen in later.units {
            match self.unit_at.get(seen.symbol) {
                Some(&at) => self.units[at].absorb(&seen, false),
                None => {
                    self.unit_at.insert(seen.symbol, self.units.len());
                    self.units.push(seen);
                }
            }
        }
        later.paths.into_iter().for_each(|path| self.open(path));
        self.texts.extend(later.texts);
    }

    /// `first` is where `symbol` is written here: the earliest is kept.
    fn unit(&mut self, symbol: &'s str, places: u8, first: Loc, journal: bool) {
        let seen = Seen { symbol, places, first, journal };
        match self.unit_at.get(symbol) {
            Some(&at) => {
                let earlier = &mut self.units[at];
                earlier.absorb(&seen, first.start < earlier.first.start);
            }
            None => {
                self.unit_at.insert(symbol, self.units.len());
                self.units.push(seen);
            }
        }
    }

    fn amount(&mut self, file: &File<'s>, amount: Amount<'s>, journal: bool) {
        if let Some(unit) = amount.unit() {
            self.unit(unit.0, places_of(amount), file.loc(unit.0), journal);
        }
    }

    /// A full path under a class root opens its place.
    fn open(&mut self, path: &'s str) {
        if class_of(path).is_some() && self.path_seen.insert(path) {
            self.paths.push(path);
        }
    }

    fn quantity(&mut self, file: &File<'s>, quantity: &Quantity<'s>) {
        match *quantity {
            Quantity::Fixed(amount) | Quantity::Pending(amount) | Quantity::Target(amount) => {
                self.amount(file, amount, true)
            }
            Quantity::Unknown(unit) | Quantity::All(Some(unit)) => self.unit(unit.0, 0, file.loc(unit.0), true),
            Quantity::All(None) | Quantity::Rest => {}
        }
    }
}

/// Whether an item belongs where it was written: a system declares kinds,
/// entities, commodities, params, codes and laws, and nothing else.
fn placement(site: &Site, item: &Item) -> Result<(), Diagnostic> {
    let file = &site.source.file;
    let in_system = matches!(site.home, Home::System(_));
    let (belongs, what) = match item.kind {
        ItemKind::Setting(id) => match file[id] {
            Setting::System(_) | Setting::Use(_) => (true, ""),
            _ => (!in_system, "a project setting"),
        },
        ItemKind::Decl(id) if file[id].what == DeclKind::Account => (!in_system, "an account"),
        ItemKind::Decl(_) | ItemKind::Code(_) | ItemKind::Param(_) | ItemKind::Law(_) => (true, ""),
        ItemKind::Txn(_) | ItemKind::Occurrence(_) | ItemKind::Opening(_) => (!in_system, "a transaction"),
        ItemKind::Assert(_) => (!in_system, "a balance assertion"),
        ItemKind::Event(_) => (!in_system, "a settlement event"),
        ItemKind::Price(_) | ItemKind::Split(_) => (!in_system, "a price"),
        ItemKind::Plan(_) => (!in_system, "a plan"),
        ItemKind::Sync(_) => (!in_system, "a sync"),
    };
    if belongs {
        return Ok(());
    }
    Err(Diagnostic::error("system-item", format!("a system cannot contain {what}"))
        .label(item.loc, "this belongs in the project")
        .note("systems declare kinds, entities, commodities, params, codes and laws; the project keeps accounts and the journal"))
}

/// The declarations of one kind, in the order written.
pub(crate) fn decls<'e, 'a, 's>(
    entries: &'e [Entry<'a, 's>],
    what: DeclKind,
) -> impl Iterator<Item = Written<'a, 's, Decl<'s>>> + 'e {
    entries.iter().filter_map(move |entry| match entry {
        Entry::Decl(written) if written.node.what == what => Some(*written),
        _ => None,
    })
}
