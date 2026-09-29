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
    Amount, ClauseKind, CodeRule, Decl, DeclKind, ExprKind, File, Flow, For, Gap, Item, ItemKind, Law, Leg, Many,
    Param, Place, Plan, Quantity, Select, Setting, Sync, Tail,
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
    /// The index in the source of the run's first item.
    pub first: usize,
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

/// What a look found.
#[derive(Default)]
struct Survey<'a, 's> {
    entries: Vec<Entry<'a, 's>>,
    units: Vec<Seen<'s>>,
    unit_at: Map<&'s str, usize>,
    paths: Vec<&'s str>,
    path_seen: Set<&'s str>,
    texts: Vec<&'s str>,
    diags: Vec<Diagnostic>,
    /// Items that will each make one transaction.
    txns: usize,
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
        .flat_map(|site| {
            let chunks = site.source.file.items.chunks(RUN).enumerate();
            chunks.map(move |(at, items)| Run { site, first: at * RUN, items })
        })
        .collect();
    let surveys = par::map_each(&runs, |run| Survey::of(run));

    let mut merged = Survey::default();
    let mut journal = Vec::new();
    let mut first_txn = 0u32;
    for (run, survey) in runs.iter().zip(surveys) {
        if run.site.home == Home::Project {
            journal.push((*run, first_txn));
        }
        first_txn += survey.txns as u32;
        merged.merge(survey);
    }
    for site in sites {
        merged.expressions(&site.source.file);
    }
    for text in merged.texts {
        names.intern(text);
    }
    Surveyed {
        entries: merged.entries,
        units: merged.units,
        paths: merged.paths,
        journal,
        txns: first_txn,
        diags: merged.diags,
    }
}

impl<'a, 's> Survey<'a, 's> {
    fn of(run: &Run<'a, 's>) -> Survey<'a, 's> {
        let mut survey = Survey::default();
        for (offset, item) in run.items.iter().enumerate() {
            match placement(run.site, run.first + offset, item) {
                Ok(()) => survey.item(run.site, item),
                Err(misplaced) => survey.diags.push(misplaced),
            }
        }
        survey
    }

    /// Adds what `later` saw, which came after everything seen so far.
    fn merge(&mut self, later: Survey<'a, 's>) {
        self.entries.extend(later.entries);
        for seen in later.units {
            self.unit(seen.symbol, seen.places, || seen.first, seen.journal);
        }
        later.paths.into_iter().for_each(|path| self.open(path));
        self.texts.extend(later.texts);
        self.diags.extend(later.diags);
    }

    fn unit(&mut self, symbol: &'s str, places: u8, first: impl FnOnce() -> Loc, journal: bool) {
        match self.unit_at.get(symbol) {
            Some(&at) => {
                let seen = &mut self.units[at];
                (seen.places, seen.journal) = (seen.places.max(places), seen.journal | journal);
            }
            None => {
                self.unit_at.insert(symbol, self.units.len());
                self.units.push(Seen { symbol, places, first: first(), journal });
            }
        }
    }

    /// Amounts and units written inside expressions: properties, params, laws.
    fn expressions(&mut self, file: &File<'s>) {
        for expr in file.exprs.iter() {
            match expr.kind {
                ExprKind::Amount(amount) => {
                    if let Some(unit) = amount.unit() {
                        self.unit(unit.0, places_of(amount), || file.loc(unit.0), false);
                    }
                }
                ExprKind::Unit(unit) => self.unit(unit.0, 0, || file.loc(unit.0), false),
                _ => {}
            }
        }
    }

    fn item(&mut self, site: &'a Site<'a, 's>, item: &'a Item<'s>) {
        let file = &site.source.file;
        if let Some(doc) = item.doc {
            self.texts.push(doc.0);
        }
        match item.kind {
            ItemKind::Decl(id) => {
                let decl = &file[id];
                for prop in &file[decl.props] {
                    for &arg in &file[prop.args] {
                        if let ExprKind::Name(path) = file.exprs[arg].kind {
                            self.open(path.0);
                        }
                    }
                }
                self.entries.push(Entry::Decl(Written { site, item, node: decl }));
            }
            ItemKind::Law(id) => self.entries.push(Entry::Law(Written { site, item, node: &file[id] })),
            ItemKind::Param(id) => self.entries.push(Entry::Param(Written { site, item, node: &file[id] })),
            ItemKind::Code(id) => self.entries.push(Entry::Code(Written { site, item, node: &file[id] })),
            ItemKind::Sync(id) => self.entries.push(Entry::Sync(Written { site, item, node: &file[id] })),
            ItemKind::Setting(id) => self.entries.push(Entry::Setting(site, file[id])),
            ItemKind::Plan(id) => {
                let plan = &file[id];
                self.flow(file, &plan.flow);
                self.entries.push(Entry::Plan(Written { site, item, node: plan }));
            }
            ItemKind::Txn(id) => {
                self.flow(file, &file[id].flow);
                self.txns += 1;
            }
            ItemKind::Occurrence(id) => {
                let occurrence = &file[id];
                occurrence.amount.iter().for_each(|&amount| self.amount(file, amount));
                self.legs(file, occurrence.legs);
                self.txns += 1;
            }
            ItemKind::Opening(id) => {
                self.legs(file, file[id].lines);
                self.txns += 1;
            }
            ItemKind::Assert(id) => {
                let assert = &file[id];
                self.place(file, &assert.place);
                self.amount(file, assert.amount);
                match assert.gap {
                    Gap::Waived(waive) => self.texts.extend(waive.reason),
                    Gap::Via(place) => self.open(place.0),
                    Gap::Refused => {}
                }
            }
            ItemKind::Price(id) => {
                let price = &file[id];
                self.unit(price.unit.0, 0, || file.loc(price.unit.0), true);
                self.amount(file, price.price);
            }
            ItemKind::Split(id) => {
                let unit = file[id].unit.0;
                self.unit(unit, 0, || file.loc(unit), true);
            }
            ItemKind::Event(_) => {}
        }
    }

    fn amount(&mut self, file: &File<'s>, amount: Amount<'s>) {
        if let Some(unit) = amount.unit() {
            self.unit(unit.0, places_of(amount), || file.loc(unit.0), true);
        }
    }

    /// A full path under a class root opens its place.
    fn open(&mut self, path: &'s str) {
        if class_of(path).is_some() && self.path_seen.insert(path) {
            self.paths.push(path);
        }
    }

    fn place(&mut self, file: &File<'s>, place: &Place<'s>) {
        self.open(place.name.0);
        for select in &file[place.select] {
            if let Select::Code(code) = select {
                self.texts.push(code.name());
            }
        }
    }

    fn quantity(&mut self, file: &File<'s>, quantity: &Quantity<'s>) {
        match *quantity {
            Quantity::Fixed(amount) | Quantity::Pending(amount) | Quantity::Target(amount) => self.amount(file, amount),
            Quantity::Unknown(unit) | Quantity::All(Some(unit)) => self.unit(unit.0, 0, || file.loc(unit.0), true),
            Quantity::All(None) | Quantity::Rest => {}
        }
    }

    fn tail(&mut self, file: &File<'s>, tail: &Tail<'s>) {
        for clause in &file[tail.clauses] {
            match clause.kind {
                ClauseKind::Code(code) | ClauseKind::For(For::Code(code)) => self.texts.push(code.name()),
                ClauseKind::Price(amount) | ClauseKind::Basis(amount) => self.amount(file, amount),
                ClauseKind::Waive(waive) => self.texts.extend(waive.reason),
                ClauseKind::For(_) | ClauseKind::Due(_) | ClauseKind::Since(_) => {}
            }
        }
    }

    fn legs(&mut self, file: &File<'s>, legs: Many<Leg<'s>>) {
        for leg in &file[legs] {
            self.place(file, &leg.place);
            self.quantity(file, &leg.amount);
            self.tail(file, &leg.tail);
        }
    }

    fn flow(&mut self, file: &File<'s>, flow: &Flow<'s>) {
        for end in [&flow.from, &flow.to] {
            end.place.iter().for_each(|place| self.place(file, place));
            end.amount.iter().for_each(|quantity| self.quantity(file, quantity));
        }
        self.tail(file, &flow.tail);
        self.legs(file, flow.legs);
    }
}

/// Whether an item belongs where it was written: a system declares kinds,
/// entities, commodities, params, codes and laws, and nothing else.
fn placement(site: &Site, at: usize, item: &Item) -> Result<(), Diagnostic> {
    let file = &site.source.file;
    let in_system = matches!(site.home, Home::System(_));
    let (belongs, what) = match item.kind {
        ItemKind::Setting(id) => match file[id] {
            Setting::System(_) if at > 0 => {
                return Err(Diagnostic::error("system-position", "a system must be the first item of its file")
                    .label(item.loc, "this file's first item is something else")
                    .help("move `system` to the top, or remove it to make this a project file"));
            }
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
