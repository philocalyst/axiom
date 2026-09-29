//! One transaction, elaborated into flows.
//!
//! The header names a source and a target. With both named the transaction is
//! one flow; with one named, the indented legs are the other side and each
//! becomes a flow. Everything a flow needs is resolved first, so that every
//! independent mistake in the transaction is reported together, and then the
//! pairing rules decide the quantities.

use axiom_core::glob::glob;
use axiom_core::{Day, Diagnostic, Id, Loc, Sym};
use axiom_syntax::{self as ast, Item, PlaceRef, Quantity};

use super::faults::{self, Written};
use super::pairing::{self, Leg, Price, Share};
use crate::args::list;
use crate::book::{Amount, CodeScope, Commodity, Entity, Place};
use crate::errors::iso;
use crate::journal::{End, Flow, Infer, Mode, Quote, Select, Txn, Waive};
use crate::prices::implied_quote;
use crate::resolve::End as Resolved;
use crate::scope::Home;
use crate::survey::code_text;
use crate::world::World;

/// Where a worker leaves what it elaborates: its transactions, their flows,
/// the prices the exchanges imply, and what went wrong. A transaction's flows
/// are the run `first .. first + len` of `flows`, counted from the start of
/// this sink; the merge makes them global.
#[derive(Default)]
pub(super) struct Sink {
    pub txns: Vec<Txn>,
    pub flows: Vec<Flow>,
    pub quotes: Vec<Quote>,
    pub diags: Vec<Diagnostic>,
    /// Some transaction is dated before the one written before it.
    pub unordered: bool,
}

impl Sink {
    /// Room for `txns` transactions, most of which are one flow.
    pub fn with_room_for(txns: usize) -> Sink {
        Sink { txns: Vec::with_capacity(txns), flows: Vec::with_capacity(txns + txns / 8), ..Sink::default() }
    }
}

/// When a transaction's flows happen, and how real they are: the journal's
/// are actual, a plan's are planned.
#[derive(Clone, Copy)]
pub(super) struct Dated {
    pub day: Day,
    pub until: Day,
    pub mode: Mode,
}

/// Elaborates one transaction into `out`. A transaction with a problem leaves
/// its diagnostics and no flows, and nothing else.
pub(super) fn elaborate<'s>(world: &World<'s>, item: &Item<'s>, dated: Dated, flow: &ast::Flow<'s>, out: &mut Sink) {
    let mut elaborator = Elaborator { world, diags: Vec::new(), dated };
    let first = out.flows.len();
    let header = elaborator.tail(&flow.tail);
    let shaped = elaborator.shape(item, flow, &header);
    let payee = header.payee.or(shaped.as_ref().and_then(|shaped| shaped.counterparty));
    if let Some(shaped) = shaped {
        elaborator.assemble(shaped.moves, &header, out);
    }
    if !elaborator.diags.is_empty() {
        out.flows.truncate(first);
        out.diags.append(&mut elaborator.diags);
    }
    out.unordered |= out.txns.last().is_some_and(|last| last.day > dated.day);
    out.txns.push(Txn {
        day: dated.day,
        first: Id::new(first as u32),
        len: (out.flows.len() - first) as u32,
        payee,
        codes: header.codes.iter().map(|&(code, _)| code).collect(),
        waive: header.waive,
        doc: item.doc.map(|doc| world.sym(doc.0)),
        loc: item.loc,
    });
}

/// What follows a header or leg, resolved.
#[derive(Default)]
struct Tail {
    payee: Option<Id<Entity>>,
    codes: Vec<(Sym, Loc)>,
    waive: Option<Waive>,
}

/// A place written in the transaction.
#[derive(Clone, Copy)]
struct Placed {
    end: Resolved,
    loc: Loc,
}

/// How much was written on one side or leg.
#[derive(Clone, Copy)]
enum Stated {
    Fixed(Amount),
    /// `? USD`
    Unknown(Id<Commodity>),
    /// `= 5_000 USD`
    Target(Amount),
    /// `all`
    All,
    /// `...`
    Rest,
}

#[derive(Clone, Copy)]
struct Slot {
    stated: Stated,
    pending: bool,
    loc: Loc,
}

/// One flow, before its transaction's facts are added.
struct Move {
    from: Placed,
    to: Placed,
    out: Amount,
    arrive: Amount,
    infer: Infer,
    pending: bool,
    select: Vec<Select>,
    payee: Option<Id<Entity>>,
    tail: Tail,
    loc: Loc,
}

enum Moves {
    One(Move),
    Many(Vec<Move>),
}

struct Shaped {
    moves: Moves,
    /// The entity written in place position on the header: the counterparty of
    /// the whole transaction.
    counterparty: Option<Id<Entity>>,
}

/// A leg with everything resolved.
struct ResolvedLeg {
    placed: Placed,
    slot: Slot,
    price: Option<(Price, Loc)>,
    select: Vec<Select>,
    tail: Tail,
    loc: Loc,
}

impl ResolvedLeg {
    fn for_pairing(&self, header_unit: Id<Commodity>) -> Leg {
        let price = self.price.map(|(price, _)| price);
        match self.slot.stated {
            Stated::Fixed(amount) => Leg::Fixed(amount, price),
            Stated::Rest => Leg::Rest,
            Stated::Unknown(unit) => Leg::Inferred(unit),
            Stated::Target(target) => Leg::Inferred(target.unit),
            Stated::All => Leg::Inferred(header_unit),
        }
    }
}

/// A side that may be unwritten (`Some(None)`), written well (`Some(Some)`), or
/// written badly, which was already reported (`None`).
fn kept<T>(side: Option<Option<T>>) -> Option<Option<T>> {
    side.map_or(Some(None), |written| written.map(Some))
}

/// One side states its amount and the other is `? UNIT`: a transfer if the
/// commodities agree, and otherwise an exchange whose unwritten side keeps its
/// commodity and has no quantity until the engine solves it.
fn one_unknown(stated: Amount, unknown: Id<Commodity>, unknown_is_out: bool) -> (Amount, Amount, Infer) {
    if stated.unit == unknown {
        return (stated, stated, Infer::Known);
    }
    let blank = Amount::zero(unknown);
    if unknown_is_out { (blank, stated, Infer::Unknown) } else { (stated, blank, Infer::Unknown) }
}

fn quantity_loc(quantity: &Quantity) -> Loc {
    match quantity {
        Quantity::Fixed(amount) | Quantity::Pending(amount) | Quantity::Target(amount) => amount.loc,
        Quantity::Unknown { loc, .. } | Quantity::Rest(loc) | Quantity::All(loc) => *loc,
    }
}

struct Elaborator<'w, 's> {
    world: &'w World<'s>,
    diags: Vec<Diagnostic>,
    dated: Dated,
}

impl<'s> Elaborator<'_, 's> {
    /// Records the diagnostic and gives nothing.
    fn fail<T>(&mut self, diagnostic: Diagnostic) -> Option<T> {
        self.diags.push(diagnostic);
        None
    }

    /// The value, or a recorded failure.
    fn ok<T>(&mut self, result: Result<T, Diagnostic>) -> Option<T> {
        match result {
            Ok(value) => Some(value),
            Err(diagnostic) => self.fail(diagnostic),
        }
    }

    // ─── Reading what was written ───────────────────────────────────────────

    fn tail(&mut self, tail: &ast::Tail<'s>) -> Tail {
        let payee = tail.payee.and_then(|name| {
            let found = self.world.entity(Home::Project, name);
            let found = found
                .map_err(|d| d.note("a payee must be a declared entity, so that it is typed like everything else"));
            self.ok(found)
        });
        let codes = tail.codes.iter().map(|code| (self.world.sym(code_text(code.text)), code.loc)).collect();
        let waive =
            tail.waive.map(|waive| Waive { loc: waive.loc, reason: waive.reason.map(|reason| self.world.sym(reason)) });
        Tail { payee, codes, waive }
    }

    fn place(&mut self, place: &PlaceRef<'s>) -> Option<Placed> {
        let end = self.world.end(place.name);
        self.ok(end).map(|end| Placed { end, loc: place.name.loc })
    }

    fn selectors(&self, place: &PlaceRef<'s>) -> Vec<Select> {
        let sym = |code: &ast::Name| self.world.sym(code_text(code.text));
        let convert = |select: &ast::Select| match select {
            ast::Select::Range(from, to, _) => Select::Range(*from, *to),
            ast::Select::Code(code) => Select::Code(sym(code)),
            ast::Select::Policy(policy, _) => Select::Policy(*policy),
        };
        place.select.iter().map(convert).collect()
    }

    /// Lot selectors choose which parcels leave, so they belong on a source.
    fn refuse_selectors(&mut self, place: &PlaceRef<'s>) {
        let Some(first) = place.select.first() else { return };
        let loc = match first {
            ast::Select::Range(_, _, loc) | ast::Select::Policy(_, loc) => *loc,
            ast::Select::Code(code) => code.loc,
        };
        self.diags.push(
            Diagnostic::error("selector-target", "a selector chooses which parcels leave, so it belongs on a source")
                .label(loc, format!("`{}` receives here", place.name.text))
                .help("move the selector to the place the value comes from"),
        );
    }

    fn quantity(&mut self, quantity: &Quantity<'s>) -> Option<Slot> {
        let (stated, pending) = match quantity {
            Quantity::Fixed(amount) => (self.positive(amount)?, false),
            Quantity::Pending(amount) => (self.positive(amount)?, true),
            Quantity::Unknown { unit, .. } => {
                let unit = self.world.commodity(*unit);
                (Stated::Unknown(self.ok(unit)?), false)
            }
            Quantity::Target(amount) => (Stated::Target(self.amount(amount)?), false),
            Quantity::All(_) => (Stated::All, false),
            Quantity::Rest(_) => (Stated::Rest, false),
        };
        Some(Slot { stated, pending, loc: quantity_loc(quantity) })
    }

    fn amount(&mut self, amount: &ast::Amount<'s>) -> Option<Amount> {
        let Some(unit) = amount.unit else {
            return self.fail(
                Diagnostic::error("empty-amount", "`empty` is the zero of any commodity, and a flow needs a commodity")
                    .label(amount.loc, "write the amount with its commodity")
                    .help("as in `84.20 USD`"),
            );
        };
        let resolved = self.world.amount(amount.num, unit, amount.loc);
        self.ok(resolved)
    }

    /// An amount a flow moves: more than nothing.
    fn positive(&mut self, amount: &ast::Amount<'s>) -> Option<Stated> {
        let resolved = self.amount(amount)?;
        if resolved.qty.is_zero() {
            return self.fail(
                Diagnostic::error("zero-flow", "a flow moves value, and this amount is zero")
                    .label(amount.loc, "moves nothing")
                    .help("remove it, or state what actually moved"),
            );
        }
        Some(Stated::Fixed(resolved))
    }

    /// `@ 285.70 USD`, if written. The outer `None` is a failure.
    fn price(&mut self, price: Option<&ast::Amount<'s>>) -> Option<Option<(Price, Loc)>> {
        let Some(price) = price else { return Some(None) };
        let quote = self.amount(price)?.unit;
        let Some(rate) = price.num.to_ratio().filter(|rate| !rate.is_zero()) else {
            return self
                .fail(Diagnostic::error("price-zero", "a price is more than nothing").label(price.loc, "this price"));
        };
        Some(Some((Price { rate, quote }, price.loc)))
    }

    // ─── Shapes ─────────────────────────────────────────────────────────────

    fn shape(&mut self, item: &Item<'s>, flow: &ast::Flow<'s>, header: &Tail) -> Option<Shaped> {
        match (&flow.from.place, &flow.to.place, flow.legs.is_empty()) {
            (Some(from), Some(to), true) => self.plain(item, flow, from, to, header),
            (Some(named), None, false) => self.split(flow, named, true, header),
            (None, Some(named), false) => self.split(flow, named, false, header),
            (Some(_), Some(_), false) | (None, None, false) => self.fail(many_to_many(flow)),
            (_, _, true) => self.fail(
                Diagnostic::error("missing-place", "this flow names only one of its two places")
                    .label(flow.arrow, "and nothing follows to say where the rest goes")
                    .help("name both places, or indent legs beneath the header for the other side"),
            ),
        }
    }

    /// `checking -> food 84.20 USD`
    fn plain(
        &mut self,
        item: &Item<'s>,
        flow: &ast::Flow<'s>,
        from: &PlaceRef<'s>,
        to: &PlaceRef<'s>,
        header: &Tail,
    ) -> Option<Shaped> {
        let (source, target) = (self.place(from), self.place(to));
        self.refuse_selectors(to);
        let out = flow.from.amount.as_ref().map(|quantity| self.header_quantity(quantity));
        let arrive = flow.to.amount.as_ref().map(|quantity| self.header_quantity(quantity));
        let price = self.price(flow.price.as_ref());
        let (source, target, price) = (source?, target?, price?);
        let (out, arrive) = (kept(out)?, kept(arrive)?);
        let at = Written { flow: flow.arrow, price: price.map(|(_, loc)| loc), from: from.name.text, to: to.name.text };
        let (out_amount, arrive_amount, infer, pending) = self.settle(out, arrive, price, at)?;
        let counterparty = target.end.entity.or(source.end.entity);
        let mv = Move {
            from: source,
            to: target,
            out: out_amount,
            arrive: arrive_amount,
            infer,
            pending,
            select: self.selectors(from),
            payee: header.payee.or(counterparty),
            tail: Tail::default(),
            loc: item.loc,
        };
        Some(Shaped { moves: Moves::One(mv), counterparty })
    }

    /// A header states an amount, `? UNIT` or `all`; `...` and `=` are for legs.
    fn header_quantity(&mut self, quantity: &Quantity<'s>) -> Option<Slot> {
        let slot = self.quantity(quantity)?;
        match slot.stated {
            Stated::Rest | Stated::Target(_) => self.fail(
                Diagnostic::error("header-amount", "a header states an amount, `? UNIT` or `all`")
                    .label(slot.loc, "this belongs on a leg")
                    .help("indent it under the header as a leg"),
            ),
            _ => Some(slot),
        }
    }

    /// The quantities of a flow whose two sides state what they state.
    fn settle(
        &mut self,
        out: Option<Slot>,
        arrive: Option<Slot>,
        price: Option<(Price, Loc)>,
        at: Written,
    ) -> Option<(Amount, Amount, Infer, bool)> {
        let pending = [out, arrive].iter().flatten().any(|slot| slot.pending);
        let solved_later = |slot: Option<Slot>| slot.is_some_and(|slot| !matches!(slot.stated, Stated::Fixed(_)));
        if solved_later(out) || solved_later(arrive) {
            let (out_amount, arrive_amount, infer) = self.inferred(out, arrive, price, at)?;
            return Some((out_amount, arrive_amount, infer, pending));
        }
        let fixed = |slot: Option<Slot>| match slot.map(|slot| slot.stated) {
            Some(Stated::Fixed(amount)) => Some(amount),
            _ => None,
        };
        let commodities = &self.world.book.commodities;
        match pairing::pair(commodities, fixed(out), fixed(arrive), price.map(|(price, _)| price)) {
            Ok((out, arrive)) => Some((out, arrive, Infer::Known, pending)),
            Err(why) => self.fail(faults::mismatch(self.world, why, at)),
        }
    }

    /// Sides with `? UNIT` or `all`: the engine solves them from balances and
    /// parcels, so only the commodities are settled here.
    fn inferred(
        &mut self,
        out: Option<Slot>,
        arrive: Option<Slot>,
        price: Option<(Price, Loc)>,
        at: Written,
    ) -> Option<(Amount, Amount, Infer)> {
        let base = self.world.book.base;
        if let Some((_, loc)) = price {
            return self.fail(
                Diagnostic::error("price-inferred", "a price needs stated amounts to convert")
                    .label(loc, "this flow's amount is inferred")
                    .help("state the amount, or remove the price"),
            );
        }
        let stated = |slot: Option<Slot>| slot.map(|slot| slot.stated);
        let arrive_loc = arrive.map_or(at.flow, |slot| slot.loc);
        match (stated(out), stated(arrive)) {
            (_, Some(Stated::All)) => self.fail(
                Diagnostic::error("all-target", "`all` empties a source, and this is the target")
                    .label(arrive_loc, "nothing to empty here")
                    .help("write `all` on the side the value leaves from"),
            ),
            (Some(Stated::All), None) => Some((Amount::zero(base), Amount::zero(base), Infer::All)),
            (Some(Stated::All), Some(Stated::Fixed(amount))) => Some((Amount::zero(amount.unit), amount, Infer::All)),
            (Some(Stated::All), Some(_)) => self.fail(
                Diagnostic::error(
                    "all-unknown",
                    "`all` moves whatever the parcels hold, so the other side cannot also be unknown",
                )
                .label(arrive_loc, "state this amount"),
            ),
            (Some(Stated::Unknown(unit)), None) | (None, Some(Stated::Unknown(unit))) => {
                Some((Amount::zero(unit), Amount::zero(unit), Infer::Unknown))
            }
            (Some(Stated::Unknown(a)), Some(Stated::Unknown(b))) if a == b => {
                Some((Amount::zero(a), Amount::zero(a), Infer::Unknown))
            }
            (Some(Stated::Unknown(unit)), Some(Stated::Fixed(amount))) => Some(one_unknown(amount, unit, true)),
            (Some(Stated::Fixed(amount)), Some(Stated::Unknown(unit))) => Some(one_unknown(amount, unit, false)),
            _ => self.fail(
                Diagnostic::error("unknown-exchange", "both amounts of an exchange cannot be inferred")
                    .label(
                        at.flow,
                        "the two commodities differ, and neither side states an amount to solve the other from",
                    )
                    .help("state one side's amount, or both amounts"),
            ),
        }
    }

    /// The header names one place; the legs are the other side.
    fn split(
        &mut self,
        flow: &ast::Flow<'s>,
        named: &PlaceRef<'s>,
        named_is_from: bool,
        header: &Tail,
    ) -> Option<Shaped> {
        let named_placed = self.place(named);
        let (header_side, other_side) = if named_is_from { (&flow.from, &flow.to) } else { (&flow.to, &flow.from) };
        let total = self.split_total(header_side.amount.as_ref(), other_side.amount.as_ref());
        if !named_is_from {
            self.refuse_selectors(named);
        }
        let legs = self.legs(flow, named_is_from);
        let (named_placed, total, legs) = (named_placed?, total?, legs?);
        let total_amount = total.map(|slot| match slot.stated {
            Stated::Fixed(amount) => amount,
            _ => unreachable!("split_total keeps only fixed amounts"),
        });
        let header_unit = total_amount.map_or(self.world.book.base, |amount| amount.unit);
        let pairing_legs: Vec<Leg> = legs.iter().map(|leg| leg.for_pairing(header_unit)).collect();
        let shares = match pairing::split(&self.world.book.commodities, total_amount, &pairing_legs) {
            Ok(shares) => shares,
            Err(why) => {
                let locs: Vec<Loc> = legs.iter().map(|leg| leg.loc).collect();
                let ends: Vec<(&str, &str)> = flow
                    .legs
                    .iter()
                    .map(|leg| {
                        let (from, to) = if named_is_from {
                            (named.name.text, leg.place.name.text)
                        } else {
                            (leg.place.name.text, named.name.text)
                        };
                        (from, to)
                    })
                    .collect();
                return self.fail(faults::split(self.world, why, named.name.loc, &locs, &ends));
            }
        };
        let named_select = self.selectors(named);
        let pending_total = total.is_some_and(|slot| slot.pending);
        let moves = legs.into_iter().zip(shares).map(|(leg, share)| {
            let select = if named_is_from { named_select.clone() } else { leg.select.clone() };
            leg_move(leg, share, named_placed, named_is_from, select, pending_total, header)
        });
        Some(Shaped { moves: Moves::Many(moves.collect()), counterparty: named_placed.end.entity })
    }

    /// The single amount a split header states, if any.
    fn split_total(&mut self, named: Option<&Quantity<'s>>, other: Option<&Quantity<'s>>) -> Option<Option<Slot>> {
        let stated = match (named, other) {
            (Some(_), Some(second)) => {
                return self.fail(
                    Diagnostic::error("split-total", "a split's total is stated once")
                        .label(quantity_loc(second), "a second amount")
                        .help("state the total on one side; the legs carry the rest"),
                );
            }
            (Some(quantity), None) | (None, Some(quantity)) => quantity,
            (None, None) => return Some(None),
        };
        let slot = self.quantity(stated)?;
        match slot.stated {
            Stated::Fixed(_) => Some(Some(slot)),
            _ => self.fail(
                Diagnostic::error("split-total", "a split's total is a stated amount")
                    .label(slot.loc, "the legs are divided out of a known total")
                    .help("state the total, or leave it out and let the legs sum"),
            ),
        }
    }

    fn legs(&mut self, flow: &ast::Flow<'s>, named_is_from: bool) -> Option<Vec<ResolvedLeg>> {
        let mut legs = Vec::with_capacity(flow.legs.len());
        let mut whole = true;
        for leg in &flow.legs {
            let placed = self.place(&leg.place);
            let slot = self.quantity(&leg.amount);
            let price = self.price(leg.price.as_ref());
            let tail = self.tail(&leg.tail);
            if named_is_from {
                self.refuse_selectors(&leg.place);
            }
            let (Some(placed), Some(slot), Some(price)) = (placed, slot, price) else {
                whole = false;
                continue;
            };
            if matches!(slot.stated, Stated::All) && named_is_from {
                let all = Diagnostic::error("all-target", "`all` empties a source, and this leg is a target")
                    .label(slot.loc, "nothing to empty here")
                    .help("write `all` on a leg that pays out");
                self.diags.push(all);
                whole = false;
            }
            legs.push(ResolvedLeg { placed, slot, price, select: self.selectors(&leg.place), tail, loc: leg.loc });
        }
        whole.then_some(legs)
    }

    // ─── Flows ──────────────────────────────────────────────────────────────

    fn assemble(&mut self, moves: Moves, header: &Tail, out: &mut Sink) {
        match moves {
            Moves::One(mv) => self.emit(mv, header, out),
            Moves::Many(moves) => moves.into_iter().for_each(|mv| self.emit(mv, header, out)),
        }
    }

    fn emit(&mut self, mv: Move, header: &Tail, out: &mut Sink) {
        let flow = self.flow(mv, header);
        out.quotes.extend(implied_quote(&self.world.book, &flow));
        out.flows.push(flow);
    }

    fn flow(&mut self, mv: Move, header: &Tail) -> Flow {
        self.check(&mv, header);
        let mode = if mv.pending && self.dated.mode == Mode::Actual { Mode::Pending } else { self.dated.mode };
        Flow {
            day: self.dated.day,
            until: self.dated.until,
            from: mv.from.end.place,
            to: mv.to.end.place,
            out: mv.out,
            arrive: mv.arrive,
            mode,
            infer: mv.infer,
            txn: Id::new(0),
            payee: mv.payee,
            select: mv.select.into(),
            codes: header.codes.iter().chain(&mv.tail.codes).map(|&(code, _)| code).collect(),
            loc: mv.loc,
            waived: mv.tail.waive.is_some(),
        }
    }

    /// The rules a flow must keep once its quantities are known.
    fn check(&mut self, mv: &Move, header: &Tail) {
        let world = self.world;
        if mv.from.end.place == mv.to.end.place && mv.out.unit == mv.arrive.unit && mv.infer == Infer::Known {
            let name = world.book.name(world.book.places[mv.from.end.place].path);
            self.diags.push(
                Diagnostic::error("self-flow", format!("`{name}` cannot pay itself"))
                    .label(mv.from.loc, "the source")
                    .context(mv.to.loc, "and the target")
                    .help("a flow moves value between two places, or between two commodities of one"),
            );
        }
        for (end, unit) in [(mv.from, mv.out.unit), (mv.to, mv.arrive.unit)] {
            if mv.infer != Infer::All {
                self.check_holds(end, unit);
            }
            self.check_open(end);
        }
        self.check_codes(mv, header);
    }

    fn check_holds(&mut self, end: Placed, unit: Id<Commodity>) {
        let world = self.world;
        let place = &world.book.places[end.end.place];
        let Some(holds) = &place.holds else { return };
        if holds.contains(&unit) {
            return;
        }
        let symbol = |unit: Id<Commodity>| world.book.name(world.book.commodities[unit].symbol);
        let held: Vec<&str> = holds.iter().map(|&held| symbol(held)).collect();
        self.diags.push(
            Diagnostic::error("not-held", format!("`{}` does not hold {}", world.book.name(place.path), symbol(unit)))
                .label(end.loc, format!("{} arrives or leaves here", symbol(unit)))
                .note(format!("it holds {}", list(&held)))
                .help("change what the account holds, or use another account"),
        );
    }

    fn check_open(&mut self, end: Placed) {
        let place = &self.world.book.places[end.end.place];
        let (day, name) = (self.dated.day, self.world.book.name(place.path));
        let fault = match (place.opened, place.closed) {
            (Some(opened), _) if day < opened => Some(("place-not-open", format!("`{name}` opens on {}", iso(opened)))),
            (_, Some(closed)) if day > closed => Some(("place-closed", format!("`{name}` closed on {}", iso(closed)))),
            _ => None,
        };
        if let Some((code, message)) = fault {
            let label = format!("this flow is dated {}", iso(day));
            self.diags.push(
                Diagnostic::error(code, message)
                    .label(end.loc, label)
                    .help("use another account, or change `opened` or `closed`"),
            );
        }
    }

    /// A code may only mark flows that touch the places its rule names.
    fn check_codes(&mut self, mv: &Move, header: &Tail) {
        let book = &self.world.book;
        for &(code, loc) in header.codes.iter().chain(&mv.tail.codes) {
            let text = book.name(code);
            for rule in book.codes.iter().filter(|rule| glob(book.name(rule.pattern), text)) {
                let touches = |place: Id<Place>| {
                    let place = &book.places[place];
                    rule.on.iter().any(|scope| match *scope {
                        CodeScope::Places(pattern) => glob(book.name(pattern), book.name(place.path)),
                        CodeScope::Kind(kind) => book.kinds.covers(kind, place.kind),
                    })
                };
                if touches(mv.from.end.place) || touches(mv.to.end.place) {
                    continue;
                }
                let allowed: Vec<&str> = rule
                    .on
                    .iter()
                    .map(|scope| match *scope {
                        CodeScope::Places(pattern) => book.name(pattern),
                        CodeScope::Kind(kind) => book.name(book.kinds[kind].name),
                    })
                    .collect();
                self.diags.push(
                    Diagnostic::error("code-placement", format!("`#{text}` may not mark this flow"))
                        .label(loc, "this code")
                        .context(rule.loc, "the rule for codes like it")
                        .note(format!("it may only mark flows touching {}", list(&allowed)))
                        .help("use the code where the rule allows it, or change the rule"),
                );
            }
        }
    }
}

/// One leg of a split, as a flow between the named place and the leg's place.
fn leg_move(
    leg: ResolvedLeg,
    share: Share,
    named: Placed,
    named_is_from: bool,
    select: Vec<Select>,
    pending_total: bool,
    header: &Tail,
) -> Move {
    let (from, to, out, arrive) = if named_is_from {
        (named, leg.placed, share.header, share.leg)
    } else {
        (leg.placed, named, share.leg, share.header)
    };
    let infer = match leg.slot.stated {
        Stated::Unknown(_) => Infer::Unknown,
        Stated::Target(target) => {
            Infer::Target { end: if named_is_from { End::To } else { End::From }, balance: target.qty }
        }
        Stated::All => Infer::All,
        Stated::Fixed(_) | Stated::Rest => Infer::Known,
    };
    let payee = leg.tail.payee.or(header.payee).or(leg.placed.end.entity).or(named.end.entity);
    Move {
        from,
        to,
        out,
        arrive,
        infer,
        pending: leg.slot.pending || pending_total,
        select,
        payee,
        tail: leg.tail,
        loc: leg.loc,
    }
}

fn many_to_many(flow: &ast::Flow) -> Diagnostic {
    let mut diagnostic = Diagnostic::error("many-to-many", "a transaction splits one side, not both")
        .label(flow.arrow, "the header names a place on both sides, or on neither")
        .note("the legs are one side of a flow, so exactly one place is named in the header");
    for leg in &flow.legs {
        diagnostic = diagnostic.context(leg.loc, "a leg with nowhere to go");
    }
    diagnostic.help("write two transactions, one for each side, with a place in between")
}
