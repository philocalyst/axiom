//! A first look at everything written, before anything is resolved.
//!
//! Some facts need every source at once and decide how everything else is
//! built: how many decimals a commodity needs (the most any amount written in
//! it uses), which full paths open places, and which names a `tally(...)` may
//! read. Names that the parallel elaboration will need as symbols (codes,
//! docs, waiver reasons) are collected here, so that they can all be interned
//! before that stage starts and it only ever reads.
//!
//! A survey is built by many workers, each looking at a run of items, and the
//! partial surveys are merged in order.

use axiom_core::{Interner, Map, Set, Sym};
use axiom_syntax::{
    Amount, Doc, Effect, ExprId, ExprKind, Flow, Item, ItemKind, Law, PlaceRef, Quantity, Select, StepKind, Tail,
};

use crate::Source;
use crate::book::Class;
use crate::paths::root_of;

/// A commodity that appears in the sources, and how precise it must be.
pub(crate) struct Seen<'s> {
    pub symbol: &'s str,
    /// The most decimal places any amount written in it uses.
    pub places: u8,
}

#[derive(Default)]
pub(crate) struct Survey<'s> {
    /// Every commodity written anywhere, in order of first appearance.
    units: Vec<Seen<'s>>,
    unit_at: Map<&'s str, usize>,
    /// Full paths under a class root written anywhere: each opens a place.
    place_paths: Vec<&'s str>,
    path_seen: Set<&'s str>,
    /// Names some law counts.
    tallies: Set<&'s str>,
    /// Codes that mark a transaction or a leg.
    written_codes: Vec<&'s str>,
    /// Everything else elaboration will need as a symbol.
    texts: Vec<&'s str>,
}

/// A survey with its names interned: what the later stages read.
pub(crate) struct Facts<'s> {
    pub units: Vec<Seen<'s>>,
    pub place_paths: Vec<&'s str>,
    pub tallies: Set<&'s str>,
    /// Codes that mark a transaction or a leg.
    pub codes: Set<Sym>,
}

/// `#house` and `house` are the same code; the interned form drops the `#`.
pub(crate) fn code_text(written: &str) -> &str {
    written.strip_prefix('#').unwrap_or(written)
}

/// The class a full path belongs to, if it starts at a class root.
pub(crate) fn class_of(path: &str) -> Option<Class> {
    let root = root_of(path);
    Class::ALL.into_iter().find(|class| class.root() == root)
}

impl<'s> Survey<'s> {
    /// Adds what `other` saw, which came after everything seen so far.
    pub fn merge(&mut self, other: Survey<'s>) {
        for seen in other.units {
            self.unit(seen.symbol, seen.places);
        }
        for path in other.place_paths {
            self.open(path);
        }
        self.tallies.extend(other.tallies);
        self.written_codes.extend(other.written_codes);
        self.texts.extend(other.texts);
    }

    /// Interns everything elaboration will look up.
    pub fn intern(self, names: &mut Interner<'s>) -> Facts<'s> {
        self.texts.into_iter().for_each(|text| {
            names.intern(text);
        });
        let codes = self.written_codes.into_iter().map(|code| names.intern(code)).collect();
        Facts { units: self.units, place_paths: self.place_paths, tallies: self.tallies, codes }
    }

    fn unit(&mut self, symbol: &'s str, places: u8) {
        match self.unit_at.get(symbol) {
            Some(&at) => self.units[at].places = self.units[at].places.max(places),
            None => {
                self.unit_at.insert(symbol, self.units.len());
                self.units.push(Seen { symbol, places });
            }
        }
    }

    /// Amounts and units written inside expressions: props, params, laws.
    pub fn expressions(&mut self, source: &Source<'s>) {
        let exprs = &source.file.exprs;
        for at in 0..exprs.len() {
            match exprs[ExprId(at as u32)].kind {
                ExprKind::Amount(number, unit) => self.unit(unit.text, number.places()),
                ExprKind::Unit(symbol) => self.unit(symbol, 0),
                _ => {}
            }
        }
    }

    pub fn item(&mut self, source: &Source<'s>, item: &Item<'s>) {
        self.doc(item.doc);
        match &item.kind {
            ItemKind::Txn(txn) => self.flow(&txn.flow),
            ItemKind::Plan(plan) => self.flow(&plan.flow),
            ItemKind::Assert(assert) => {
                self.place(&assert.place);
                self.amount(&assert.amount);
                self.text(assert.waive.and_then(|waive| waive.reason));
            }
            ItemKind::Event(event) => self.texts.push(code_text(event.code.text)),
            ItemKind::Price(price) => {
                self.unit(price.unit.text, 0);
                self.amount(&price.price);
            }
            ItemKind::Decl(decl) => {
                self.property_places(source, decl);
                decl.laws.iter().for_each(|law| self.law(law));
            }
            ItemKind::Law(law) => self.law(law),
            ItemKind::Code(rule) => self.texts.push(code_text(rule.pattern.text)),
            ItemKind::Param(_) | ItemKind::Sync(_) | ItemKind::Setting(_) => {}
        }
    }

    fn doc(&mut self, doc: Option<Doc<'s>>) {
        if let Some(Doc(text)) = doc {
            self.texts.push(text);
        }
    }

    fn text(&mut self, text: Option<&'s str>) {
        self.texts.extend(text);
    }

    fn amount(&mut self, amount: &Amount<'s>) {
        if let Some(unit) = amount.unit {
            self.unit(unit.text, amount.num.places());
        }
    }

    fn quantity(&mut self, quantity: &Quantity<'s>) {
        match quantity {
            Quantity::Fixed(amount) | Quantity::Pending(amount) | Quantity::Target(amount) => self.amount(amount),
            Quantity::Unknown { unit, .. } => self.unit(unit.text, 0),
            Quantity::Rest(_) | Quantity::All(_) => {}
        }
    }

    /// A full path under a class root opens its place.
    fn open(&mut self, path: &'s str) {
        if class_of(path).is_some() && self.path_seen.insert(path) {
            self.place_paths.push(path);
        }
    }

    fn place(&mut self, place: &PlaceRef<'s>) {
        self.open(place.name.text);
        for select in &place.select {
            if let Select::Code(code) = select {
                self.texts.push(code_text(code.text));
            }
        }
    }

    fn tail(&mut self, tail: &Tail<'s>) {
        self.written_codes.extend(tail.codes.iter().map(|code| code_text(code.text)));
        self.text(tail.waive.and_then(|waive| waive.reason));
    }

    fn flow(&mut self, flow: &Flow<'s>) {
        for side in [&flow.from, &flow.to] {
            side.place.iter().for_each(|place| self.place(place));
            side.amount.iter().for_each(|quantity| self.quantity(quantity));
        }
        flow.price.iter().for_each(|price| self.amount(price));
        self.tail(&flow.tail);
        for leg in &flow.legs {
            self.doc(leg.doc);
            self.place(&leg.place);
            self.quantity(&leg.amount);
            leg.price.iter().for_each(|price| self.amount(price));
            self.tail(&leg.tail);
        }
    }

    /// A full path written as a property's argument (`via assets/bank/paypal`)
    /// opens that place, just as one written in a flow does.
    fn property_places(&mut self, source: &Source<'s>, decl: &axiom_syntax::Decl<'s>) {
        for prop in &decl.props {
            for &arg in &prop.args {
                if let ExprKind::Name(path) = source.file.exprs[arg].kind {
                    self.open(path);
                }
            }
        }
    }

    fn law(&mut self, law: &Law<'s>) {
        let counts = law.steps.iter().flat_map(|step| match &step.kind {
            StepKind::Effect(Effect::Count { name, .. })
            | StepKind::Require { otherwise: Some(Effect::Count { name, .. }), .. } => Some(name.text),
            _ => None,
        });
        self.tallies.extend(counts);
    }
}
