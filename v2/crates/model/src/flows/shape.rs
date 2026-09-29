//! Reading a flow as written: names become ids, amounts become quantities, and
//! what a header and its legs say is gathered into a [`Shape`], which the rest
//! of elaboration reads without looking at the syntax again.
//!
//! Every independent mistake in a transaction is found before any is acted on,
//! so that they are all reported together. A name that resolves to nothing is
//! only noted here: each is explained once, however often it is written.

use axiom_core::{Day, Diagnostic, Id, Loc, Qty, Ratio, Sym};
use axiom_syntax::{self as ast, ClauseKind, Due, File, For, Quantity};

use super::Sink;
use super::moves::Move;
use crate::book::{Amount, Commodity, Entity};
use crate::declare::World;
use crate::journal::{Select, Waive};
use crate::resolve::{Cause, End};
use crate::scope::Home;

/// A place written in a transaction, with what says which of its parcels.
#[derive(Clone, Debug)]
pub(super) struct Placed {
    pub end: End,
    pub loc: Loc,
    pub select: Vec<Select>,
    /// `PLACE.basis`: what moves is the place's basis, not its quantity.
    pub basis: bool,
}

/// How much was written on one side or leg.
#[derive(Clone, Copy, Debug)]
pub(super) enum Stated {
    Fixed(Amount),
    /// `? USD`
    Unknown(Id<Commodity>),
    /// `= 5_000 USD`
    Target(Amount),
    /// `all`, or `all VXUS`
    All(Option<Id<Commodity>>),
    /// `...`
    Rest,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Slot {
    pub stated: Stated,
    pub pending: bool,
    pub loc: Loc,
}

/// `@ 285.70 USD`: whole units of `quote` for one whole unit of what is priced.
#[derive(Clone, Copy, Debug)]
pub(super) struct Priced {
    pub rate: Ratio,
    pub quote: Id<Commodity>,
    pub loc: Loc,
}

/// What follows a header or leg, resolved.
#[derive(Clone, Default, Debug)]
pub(super) struct Tail {
    pub payee: Option<Id<Entity>>,
    /// Codes that mark, and the one `for #code` settles, which also links.
    pub codes: Vec<(Sym, Loc)>,
    pub settles: Option<Sym>,
    pub price: Option<Priced>,
    pub period: Option<(Day, Day)>,
    pub hold: Option<Id<Entity>>,
    pub due: Option<Due>,
    pub basis: Option<Qty>,
    pub since: Option<Day>,
    pub waive: Option<Waive>,
}

impl Tail {
    /// This tail, added to and overridden by a leg's own.
    pub fn over(&self, leg: &Tail) -> Tail {
        Tail {
            payee: leg.payee.or(self.payee),
            codes: self.codes.iter().chain(&leg.codes).copied().collect(),
            settles: leg.settles.or(self.settles),
            price: leg.price.or(self.price),
            period: leg.period.or(self.period),
            hold: leg.hold.or(self.hold),
            due: leg.due.or(self.due),
            basis: leg.basis.or(self.basis),
            since: leg.since.or(self.since),
            waive: leg.waive.or(self.waive),
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct Side {
    pub placed: Option<Placed>,
    pub slot: Option<Slot>,
}

/// One indented line.
#[derive(Clone, Debug)]
pub(super) struct Leg {
    pub placed: Placed,
    pub slot: Slot,
    pub tail: Tail,
    pub loc: Loc,
}

/// A flow with everything resolved: two header sides, the header's tail, and
/// the legs that spell out an unnamed side.
#[derive(Clone, Debug)]
pub(super) struct Shape {
    pub from: Side,
    pub to: Side,
    pub tail: Tail,
    pub legs: Vec<Leg>,
    /// The header line, and the arrow in it.
    pub loc: Loc,
    pub arrow: Loc,
}

/// One elaborator for a run of items: it reads the world and writes its sink.
pub(super) struct Elab<'a, 's> {
    pub world: &'a World<'s>,
    pub file: &'a File<'s>,
    pub sink: &'a mut Sink<'s>,
    /// The commodity written last, since amounts repeat their commodity.
    unit: Option<(&'s str, Id<Commodity>)>,
    /// The global number of the transaction being made.
    pub txn: u32,
    /// The codes the transaction writes, for its code rules.
    pub coded: Vec<(Sym, Loc)>,
    /// The room the last transaction's moves used, kept for the next one's.
    pub spare: Vec<Move>,
}

impl<'a, 's> Elab<'a, 's> {
    pub fn new(world: &'a World<'s>, file: &'a File<'s>, sink: &'a mut Sink<'s>, txn: u32) -> Elab<'a, 's> {
        Elab { world, file, sink, unit: None, txn, coded: Vec::new(), spare: Vec::new() }
    }

    /// Records the diagnostic and gives nothing.
    pub fn fail<T>(&mut self, diagnostic: Diagnostic) -> Option<T> {
        self.sink.diags.push(diagnostic);
        None
    }

    /// The value, or a recorded failure.
    pub fn ok<T>(&mut self, result: Result<T, Diagnostic>) -> Option<T> {
        result.map_err(|diagnostic| self.sink.diags.push(diagnostic)).ok()
    }

    /// Notes a name that resolved to nothing, to be explained once.
    fn miss<T>(&mut self, cause: Cause, text: &'s str) -> Option<T> {
        self.sink.misses.push((cause, text, self.file.loc(text)));
        None
    }

    // ─── Places, amounts and tails ──────────────────────────────────────────

    pub fn commodity(&mut self, symbol: &'s str) -> Option<Id<Commodity>> {
        if let Some((last, id)) = self.unit
            && last == symbol
        {
            return Some(id);
        }
        match self.world.commodity(symbol) {
            Some(id) => {
                self.unit = Some((symbol, id));
                Some(id)
            }
            None => self.miss(Cause::Commodity, symbol),
        }
    }

    pub fn entity(&mut self, name: &'s str) -> Option<Id<Entity>> {
        use crate::names::Found;
        let (lookup, scope) = (&self.world.book.lookup.entities, self.world.scopes.of(Home::Project));
        match lookup.find(&self.world.book.names, scope, name) {
            Found::One(entity) => Some(entity),
            Found::Nothing => self.miss(Cause::Entity, name),
            Found::Several(_) => self.miss(Cause::AmbiguousEntity, name),
        }
    }

    pub fn place(&mut self, place: &ast::Place<'s>) -> Option<Placed> {
        let text = place.name.0;
        let end = match self.world.find_end(text) {
            Ok(end) => end,
            Err(cause) => return self.miss(cause, text),
        };
        let sym = |code: ast::Code| self.world.sym(code.name());
        let selects = &self.file[place.select];
        let select = selects
            .iter()
            .filter_map(|select| match *select {
                ast::Select::Range(first, last, _) => Some(Select::Range(first, last)),
                ast::Select::Code(code) => Some(Select::Code(sym(code))),
                ast::Select::Policy(policy, _) => Some(Select::Policy(policy)),
                ast::Select::Basis => None,
            })
            .collect();
        Some(Placed { end, loc: self.file.loc(text), select, basis: place.is_basis(self.file) })
    }

    #[inline(always)]
    pub fn amount(&mut self, amount: ast::Amount<'s>) -> Option<Amount> {
        let loc = self.file.loc(&amount);
        let Some(unit) = amount.unit() else {
            return self.fail(
                Diagnostic::error("empty-amount", "`empty` is the zero of any commodity, and a flow needs a commodity")
                    .label(loc, "write the amount with its commodity")
                    .help("as in `84.20 USD`"),
            );
        };
        let unit = self.commodity(unit.0)?;
        let resolved = self.world.amount(amount.num(), unit, loc);
        self.ok(resolved)
    }

    /// An amount a flow moves: more than nothing.
    #[inline(always)]
    fn positive(&mut self, amount: ast::Amount<'s>) -> Option<Stated> {
        let resolved = self.amount(amount)?;
        if resolved.qty.is_zero() {
            return self.fail(
                Diagnostic::error("zero-flow", "a flow moves value, and this amount is zero")
                    .label(self.file.loc(&amount), "moves nothing")
                    .help("remove it, or state what actually moved"),
            );
        }
        Some(Stated::Fixed(resolved))
    }

    /// `fallback` is where to point for `...` and `all`, which have no text of
    /// their own.
    #[inline(always)]
    pub fn quantity(&mut self, quantity: &Quantity<'s>, fallback: Loc) -> Option<Slot> {
        let file = self.file;
        let (stated, pending, loc) = match *quantity {
            Quantity::Fixed(amount) => (self.positive(amount)?, false, file.loc(&amount)),
            Quantity::Pending(amount) => (self.positive(amount)?, true, file.loc(&amount)),
            Quantity::Target(amount) => (Stated::Target(self.amount(amount)?), false, file.loc(&amount)),
            Quantity::Unknown(unit) => (Stated::Unknown(self.commodity(unit.0)?), false, file.loc(unit.0)),
            Quantity::All(Some(unit)) => (Stated::All(Some(self.commodity(unit.0)?)), false, file.loc(unit.0)),
            Quantity::All(None) => (Stated::All(None), false, fallback),
            Quantity::Rest => (Stated::Rest, false, fallback),
        };
        Some(Slot { stated, pending, loc })
    }

    /// `@ 285.70 USD`. The price may carry more decimals than its commodity: it
    /// is a rate, and only the amounts computed from it are rounded.
    pub fn price(&mut self, amount: ast::Amount<'s>) -> Option<Priced> {
        let loc = self.file.loc(&amount);
        let Some(unit) = amount.unit() else {
            return self.fail(
                Diagnostic::error("price-unit", "a price is an amount of some commodity")
                    .label(loc, "which commodity?")
                    .help("write the commodity after the number, as in `285.70 USD`"),
            );
        };
        let quote = self.commodity(unit.0)?;
        match amount.num().to_ratio().filter(|rate| !rate.is_zero()) {
            Some(rate) => Some(Priced { rate, quote, loc }),
            None => self.fail(Diagnostic::error("price-zero", "a price is more than nothing").label(loc, "this price")),
        }
    }

    /// What follows a header or leg. A `due` belongs to the transaction, so it
    /// is refused on a leg.
    #[inline(always)]
    pub fn tail(&mut self, tail: &ast::Tail<'s>, on_leg: bool) -> Option<Tail> {
        let mut resolved = Tail::default();
        let mut whole = true;
        if let Some(payee) = tail.payee {
            let payee = self.entity(payee.0);
            (resolved.payee, whole) = (payee, whole && payee.is_some());
        }
        for clause in &self.file[tail.clauses] {
            let ok = self.clause(clause, on_leg, &mut resolved);
            whole &= ok.is_some();
        }
        whole.then_some(resolved)
    }

    fn clause(&mut self, clause: &ast::Clause<'s>, on_leg: bool, tail: &mut Tail) -> Option<()> {
        let code = |this: &mut Self, code: ast::Code| {
            let sym = this.world.sym(code.name());
            this.coded.push((sym, this.file.loc(code.0)));
            (sym, this.file.loc(code.0))
        };
        match clause.kind {
            ClauseKind::Code(written) => tail.codes.push(code(self, written)),
            ClauseKind::Price(amount) => tail.price = Some(self.price(amount)?),
            ClauseKind::For(For::Code(written)) => {
                let (sym, loc) = code(self, written);
                (tail.settles, tail.codes) = (Some(sym), tail.codes.iter().copied().chain([(sym, loc)]).collect());
            }
            ClauseKind::For(For::Period(first, last)) => tail.period = Some((first, last)),
            ClauseKind::For(For::Entity(name)) => tail.hold = Some(self.entity(name.0)?),
            ClauseKind::Due(_) if on_leg => {
                return self.fail(
                    Diagnostic::error("due-on-leg", "a claim falls due as a whole, so `due` belongs on the header")
                        .label(clause.at, "a leg has no due day of its own")
                        .help("move `due` to the header of the transaction"),
                );
            }
            ClauseKind::Due(due) => tail.due = Some(due),
            ClauseKind::Basis(amount) => tail.basis = Some(self.basis(amount)?),
            ClauseKind::Since(day) => tail.since = Some(day),
            ClauseKind::Waive(waive) => {
                tail.waive = Some(Waive { loc: waive.at, reason: waive.reason.map(|reason| self.world.sym(reason)) })
            }
        }
        Some(())
    }

    /// `basis 3_000 USD`: the total basis, which is counted in the base currency.
    fn basis(&mut self, amount: ast::Amount<'s>) -> Option<Qty> {
        let resolved = self.amount(amount)?;
        let base = self.world.book.base;
        if resolved.unit != base {
            let symbol = self.world.book.name(self.world.book.commodities[base].symbol);
            return self.fail(
                Diagnostic::error("basis-unit", format!("a basis counts in the base currency, {symbol}"))
                    .label(self.file.loc(&amount), "in another commodity")
                    .help(format!("write what it cost in {symbol}")),
            );
        }
        Some(resolved.qty)
    }

    // ─── Shapes ─────────────────────────────────────────────────────────────

    #[inline(always)]
    fn side(&mut self, end: &ast::End<'s>, loc: Loc) -> Option<Side> {
        let placed = end.place.as_ref().map(|place| self.place(place));
        let slot = end.amount.as_ref().map(|quantity| self.quantity(quantity, loc));
        match (placed, slot) {
            (Some(None), _) | (_, Some(None)) => None,
            (placed, slot) => Some(Side { placed: placed.flatten(), slot: slot.flatten() }),
        }
    }

    /// One indented line: a place, how much, and its own tail.
    pub fn leg(&mut self, leg: &ast::Leg<'s>) -> Option<Leg> {
        let (placed, slot, tail) =
            (self.place(&leg.place), self.quantity(&leg.amount, leg.loc), self.tail(&leg.tail, true));
        Some(Leg { placed: placed?, slot: slot?, tail: tail?, loc: leg.loc })
    }

    /// A header and its legs, resolved. `loc` is the header line.
    pub fn shape(&mut self, flow: &ast::Flow<'s>, loc: Loc) -> Option<Shape> {
        let (from, to, tail) = (self.side(&flow.from, loc), self.side(&flow.to, loc), self.tail(&flow.tail, false));
        let legs: Vec<Option<Leg>> = self.file[flow.legs].iter().map(|leg| self.leg(leg)).collect();
        let legs: Option<Vec<Leg>> = legs.into_iter().collect();
        Some(Shape { from: from?, to: to?, tail: tail?, legs: legs?, loc, arrow: self.arrow(loc) })
    }

    /// Where the `->` is in a header, which is the first `>` there: no name or
    /// amount contains one. A header that spells it another way is pointed at
    /// whole.
    fn arrow(&self, header: Loc) -> Loc {
        let text = &self.file.src.as_bytes()[header.range()];
        match memchr::memchr(b'>', text).filter(|&at| at > 0 && text[at - 1] == b'-') {
            Some(at) => Loc::new(header.file, header.start + at as u32 - 1, header.start + at as u32 + 1),
            None => header,
        }
    }

    /// A place as its author wrote it, when it was written in this file.
    pub fn written(&self, placed: &Placed) -> &'s str {
        match placed.loc.file == self.file.id {
            true => &self.file.src[placed.loc.range()],
            false => self.world.book.name(self.world.book.places[placed.end.place].path),
        }
    }
}
