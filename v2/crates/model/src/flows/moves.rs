//! From a shape to flows: the pairing rules of LANGUAGE §2 applied to what a
//! header and its legs say, and the rules every flow must keep.
//!
//! A header naming both places is one flow. A header naming one is a split: each
//! leg is a flow between the named place and the leg's own. A header that also
//! states what the named place gives up or takes (`house 1 HOME -> 431_500 USD`)
//! is one exchange whose proceeds the legs allocate: the exchange runs through
//! one leg's place, and the others are paid to or from it.

use axiom_core::glob::glob;
use axiom_core::{Day, Diagnostic, Id, Loc, Sym};

use super::faults::{self, Written};
use super::pairing::{self, Share};
use super::shape::{Elab, Leg, Placed, Shape, Slot, Stated, Tail};
use crate::book::{Amount, CodeScope, Commodity, Entity, Place};
use crate::errors::{count, iso, list, list_and};
use crate::journal::{End, Flow, Infer, Mode, Recognition, Terms};

/// One flow, before its transaction's facts are added.
pub(super) struct Move {
    pub from: Placed,
    pub to: Placed,
    pub out: Amount,
    pub arrive: Amount,
    pub infer: Infer,
    pub pending: bool,
    pub tail: Tail,
    pub loc: Loc,
}

impl Move {
    /// A flow from `from` to `to`, with what leaves and what arrives, whether
    /// its quantity is known and whether it is pending, and what its tail says.
    pub fn between(
        from: &Placed,
        to: &Placed,
        amounts: (Amount, Amount),
        how: (Infer, bool),
        tail: Tail,
        loc: Loc,
    ) -> Move {
        let (infer, pending) = how;
        Move { from: from.clone(), to: to.clone(), out: amounts.0, arrive: amounts.1, infer, pending, tail, loc }
    }
}

pub(super) struct Moves {
    pub moves: Vec<Move>,
    /// The entity written in place position on the header: the counterparty of
    /// the whole transaction.
    pub counterparty: Option<Id<Entity>>,
}

/// The one amount a split allocates, and what the named place itself gives up
/// or takes when the header says so.
enum Total {
    /// Whatever the legs sum to.
    Sum,
    Stated(Amount),
    Exchange {
        own: Amount,
        total: Amount,
    },
}

impl Elab<'_, '_> {
    pub fn moves(&mut self, shape: &Shape) -> Option<Moves> {
        match (&shape.from.placed, &shape.to.placed, shape.legs.is_empty()) {
            (Some(from), Some(to), true) => self.plain(shape, from, to),
            (Some(named), None, false) => self.split(shape, named, true),
            (None, Some(named), false) => self.split(shape, named, false),
            // The parser has refused every other shape.
            _ => None,
        }
    }

    fn path(&self, placed: &Placed) -> &str {
        self.world.book.name(self.world.book.places[placed.end.place].path)
    }

    /// Lot selectors choose which parcels leave, so they belong on a source, or
    /// on the basis of a place.
    fn refuse_selectors(&mut self, placed: &Placed) -> Option<()> {
        if placed.select.is_empty() || placed.basis {
            return Some(());
        }
        self.fail(
            Diagnostic::error("selector-target", "a selector chooses which parcels leave, so it belongs on a source")
                .label(placed.loc, format!("`{}` receives here", self.path(placed)))
                .help("move the selector to the place the value comes from"),
        )
    }

    // ─── One flow ───────────────────────────────────────────────────────────

    /// `checking -> food 84.20 USD`
    fn plain(&mut self, shape: &Shape, from: &Placed, to: &Placed) -> Option<Moves> {
        let selectors = self.refuse_selectors(to);
        if from.basis && to.basis {
            return self.fail(
                Diagnostic::error("basis-both", "a flow moves the basis of one place, not two")
                    .label(to.loc, "this end is a basis too")
                    .context(from.loc, "and this one")
                    .help("write `.basis` on the place whose basis changes"),
            );
        }
        selectors?;
        let price = shape.tail.price;
        let at = Written {
            flow: shape.arrow,
            price: price.map(|price| price.loc),
            from: self.written(from),
            to: self.written(to),
        };
        let (out, arrive, infer, pending) = self.settle(shape.from.slot, shape.to.slot, price, at)?;
        let counterparty = to.end.entity.or(from.end.entity);
        let tail = Tail { payee: shape.tail.payee.or(counterparty), ..shape.tail.clone() };
        let mv = Move::between(from, to, (out, arrive), (infer, pending), tail, shape.loc);
        Some(Moves { moves: vec![mv], counterparty })
    }

    /// The quantities of a flow whose two sides state what they state.
    fn settle(
        &mut self,
        out: Option<Slot>,
        arrive: Option<Slot>,
        price: Option<super::shape::Priced>,
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
        let price = price.map(|price| pairing::Price { rate: price.rate, quote: price.quote });
        match pairing::pair(commodities, fixed(out), fixed(arrive), price) {
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
        price: Option<super::shape::Priced>,
        at: Written,
    ) -> Option<(Amount, Amount, Infer)> {
        let base = self.world.book.base;
        if let Some(price) = price {
            return self.fail(
                Diagnostic::error("price-inferred", "a price needs stated amounts to convert")
                    .label(price.loc, "this flow's amount is inferred")
                    .help("state the amount, or remove the price"),
            );
        }
        let stated = |slot: Option<Slot>| slot.map(|slot| slot.stated);
        let arrive_loc = arrive.map_or(at.flow, |slot| slot.loc);
        match (stated(out), stated(arrive)) {
            (_, Some(Stated::All(_))) => self.fail(
                Diagnostic::error("all-target", "`all` empties a source, and this is the target")
                    .label(arrive_loc, "nothing to empty here")
                    .help("write `all` on the side the value leaves from"),
            ),
            (Some(Stated::All(unit)), None) => {
                let unit = unit.unwrap_or(base);
                Some((Amount::zero(unit), Amount::zero(unit), Infer::All))
            }
            (Some(Stated::All(unit)), Some(Stated::Fixed(amount))) => {
                Some((Amount::zero(unit.unwrap_or(amount.unit)), amount, Infer::All))
            }
            (Some(Stated::All(_)), Some(_)) => self.fail(
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

    // ─── One side split ─────────────────────────────────────────────────────

    /// The header names one place; the legs are the other side.
    fn split(&mut self, shape: &Shape, named: &Placed, named_is_from: bool) -> Option<Moves> {
        let (own, other) =
            if named_is_from { (shape.from.slot, shape.to.slot) } else { (shape.to.slot, shape.from.slot) };
        let total = self.split_total(own, other, shape.loc);
        let mut kept = if named_is_from { Some(()) } else { self.refuse_selectors(named) };
        for leg in shape.legs.iter().filter(|_| named_is_from) {
            kept = kept.and(self.refuse_selectors(&leg.placed));
            if matches!(leg.slot.stated, Stated::All(_)) {
                kept = self.fail(
                    Diagnostic::error("all-target", "`all` empties a source, and this leg is a target")
                        .label(leg.slot.loc, "nothing to empty here")
                        .help("write `all` on a leg that pays out"),
                );
            }
        }
        let (total, _) = (total?, kept?);
        let unit = match total {
            Total::Stated(amount) => amount.unit,
            Total::Exchange { total, .. } => total.unit,
            Total::Sum => self.world.book.base,
        };
        let stated = match total {
            Total::Stated(amount) => Some(amount),
            Total::Exchange { total, .. } => Some(total),
            Total::Sum => None,
        };
        let legs: Vec<pairing::Leg> = shape.legs.iter().map(|leg| for_pairing(leg, unit)).collect();
        let shares = match pairing::split(&self.world.book.commodities, stated, &legs) {
            Ok(shares) => shares,
            Err(why) => {
                let locs: Vec<_> = shape.legs.iter().map(|leg| leg.loc).collect();
                let ends: Vec<(&str, &str)> = shape
                    .legs
                    .iter()
                    .map(|leg| match named_is_from {
                        true => (self.path(named), self.path(&leg.placed)),
                        false => (self.path(&leg.placed), self.path(named)),
                    })
                    .collect();
                // The total is what to point at: it is what the legs fail to make.
                let header = other.or(own).map_or(shape.loc, |slot| slot.loc);
                return self.fail(faults::split(self.world, why, header, &locs, &ends));
            }
        };
        let pending = own.into_iter().chain(other).any(|slot| slot.pending);
        // A closing statement: what the named place gives up or takes is exchanged for the whole total through
        // one leg's place, and the other legs are paid to or from it. The proceeds are then exactly what the
        // header says, however the legs allocate them.
        let exchange = match total {
            Total::Exchange { own, total } => {
                let rest = shape.legs.iter().position(|leg| matches!(leg.slot.stated, Stated::Rest));
                let principal = rest.or_else(|| shares.iter().position(|share| share.leg.unit == total.unit));
                principal.map(|at| (at, Share { header: own, leg: total }))
            }
            _ => None,
        };
        let mut moves: Vec<Move> = Vec::with_capacity(shape.legs.len());
        for (at, (leg, share)) in shape.legs.iter().zip(&shares).enumerate() {
            let (hub, share) = match exchange {
                Some((principal, exchanged)) if at == principal => (named, exchanged),
                Some((principal, _)) => (&shape.legs[principal].placed, *share),
                None => (named, *share),
            };
            moves.push(self.leg_move(shape, leg, share, hub, named_is_from, pending));
        }
        if let Some((principal, _)) = exchange {
            // Paying into the exchange comes first, and paying out of it last.
            let exchanged = moves.remove(principal);
            match named_is_from {
                true => moves.insert(0, exchanged),
                false => moves.push(exchanged),
            }
        }
        Some(Moves { moves, counterparty: named.end.entity })
    }

    /// The single amount a split header states, and what the named place itself
    /// states, if it also does.
    fn split_total(&mut self, named: Option<Slot>, other: Option<Slot>, loc: Loc) -> Option<Total> {
        let fixed = |slot: Slot| match slot.stated {
            Stated::Fixed(amount) => Some(amount),
            _ => None,
        };
        let not_a_total = |this: &mut Self, slot: Slot| {
            this.fail(
                Diagnostic::error("split-total", "a split's total is a stated amount")
                    .label(slot.loc, "the legs are divided out of a known total")
                    .help("state the total, or leave it out and let the legs sum"),
            )
        };
        match (named, other) {
            (None, None) => Some(Total::Sum),
            (Some(slot), None) | (None, Some(slot)) => match fixed(slot) {
                Some(amount) => Some(Total::Stated(amount)),
                None => not_a_total(self, slot),
            },
            (Some(own), Some(total)) => match (fixed(own), fixed(total)) {
                (Some(own), Some(total)) if own.unit != total.unit => Some(Total::Exchange { own, total }),
                (Some(own), Some(total)) if own.qty == total.qty => Some(Total::Stated(total)),
                (Some(own), Some(total)) => self.fail(
                    Diagnostic::error("amounts-differ", "the two sides of this split state different amounts")
                        .label(
                            loc,
                            format!(
                                "{} on one side, {} on the other",
                                self.world.book.show(own),
                                self.world.book.show(total)
                            ),
                        )
                        .help("state the total once, or state both in different commodities to make it an exchange"),
                ),
                (None, _) => not_a_total(self, own),
                (_, None) => not_a_total(self, total),
            },
        }
    }

    /// One leg of a split, as a flow between the `hub` (the named place, or
    /// where a closing statement's proceeds land) and the leg's own place.
    fn leg_move(
        &self,
        shape: &Shape,
        leg: &Leg,
        share: Share,
        hub: &Placed,
        named_is_from: bool,
        pending: bool,
    ) -> Move {
        let (from, to, out, arrive) = match named_is_from {
            true => (hub, &leg.placed, share.header, share.leg),
            false => (&leg.placed, hub, share.leg, share.header),
        };
        let infer = match leg.slot.stated {
            Stated::Unknown(_) => Infer::Unknown,
            Stated::Target(target) => {
                Infer::Target { end: if named_is_from { End::To } else { End::From }, balance: target.qty }
            }
            Stated::All(_) => Infer::All,
            Stated::Fixed(_) | Stated::Rest => Infer::Known,
        };
        let mut tail = shape.tail.over(&leg.tail);
        tail.payee = tail.payee.or(leg.placed.end.entity).or(hub.end.entity);
        Move::between(from, to, (out, arrive), (infer, pending || leg.slot.pending), tail, leg.loc)
    }

    // ─── Flows ──────────────────────────────────────────────────────────────

    /// The flow a move makes on `day`, if it keeps the rules every flow keeps.
    pub fn flow(&mut self, mv: Move, day: Day, mode: Mode) -> Option<Flow> {
        self.check(&mv, day)?;
        let mode = if mv.pending && mode == Mode::Actual { Mode::Pending } else { mode };
        let tail = &mv.tail;
        let mut select = if mv.to.basis { mv.to.select.clone() } else { mv.from.select.clone() };
        select.extend(tail.settles.map(crate::journal::Select::Code));
        let codes: Box<[Sym]> = match tail.codes.is_empty() {
            true => Box::default(),
            false => tail.codes.iter().map(|&(code, _)| code).collect(),
        };
        let basis_end = match (mv.from.basis, mv.to.basis) {
            (true, _) => Some(End::From),
            (_, true) => Some(End::To),
            _ => None,
        };
        let said = tail.basis.is_some() || tail.hold.is_some() || tail.since.is_some() || basis_end.is_some();
        let terms = said.then(|| Box::new(Terms { basis: tail.basis, hold: tail.hold, basis_end, since: tail.since }));
        let recognized = tail.period.map_or(Recognition::on(day), |(from, until)| Recognition { from, until });
        Some(Flow {
            day,
            recognized,
            from: mv.from.end.place,
            to: mv.to.end.place,
            out: mv.out,
            arrive: mv.arrive,
            mode,
            infer: mv.infer,
            txn: Id::new(self.txn),
            payee: tail.payee,
            select: select.into(),
            codes,
            loc: mv.loc,
            waive: tail.waive,
            terms,
        })
    }

    /// The rules a flow must keep once its quantities are known.
    fn check(&mut self, mv: &Move, day: Day) -> Option<()> {
        let mut kept = true;
        if mv.from.end.place == mv.to.end.place && mv.out.unit == mv.arrive.unit && mv.infer == Infer::Known {
            kept = false;
            self.sink.diags.push(
                Diagnostic::error("self-flow", format!("`{}` cannot pay itself", self.path(&mv.from)))
                    .label(mv.from.loc, "the source")
                    .context(mv.to.loc, "and the target")
                    .help("a flow moves value between two places, or between two commodities of one"),
            );
        }
        for (end, unit) in [(&mv.from, mv.out.unit), (&mv.to, mv.arrive.unit)] {
            if mv.infer != Infer::All {
                kept &= self.check_holds(end, unit);
            }
            kept &= self.check_open(end, day);
        }
        kept.then_some(())
    }

    fn check_holds(&mut self, end: &Placed, unit: Id<Commodity>) -> bool {
        let world = self.world;
        let place = &world.book.places[end.end.place];
        let Some(holds) = &place.holds else { return true };
        if holds.contains(&unit) {
            return true;
        }
        let symbol = |unit: Id<Commodity>| world.book.name(world.book.commodities[unit].symbol);
        let held: Vec<&str> = holds.iter().map(|&held| symbol(held)).collect();
        let mut error = Diagnostic::error(
            "not-held",
            format!(
                "`{}` only holds {}, and this flow moves {}",
                world.book.name(place.path),
                list_and(&held),
                symbol(unit)
            ),
        )
        .label(end.loc, format!("{} arrives or leaves here", symbol(unit)));
        if let Some(&line) = world.lines.get(&(end.end.place, "holds")) {
            error = error.context(line, format!("only {} may be held here", list_and(&held)));
        }
        self.sink.diags.push(error.help("change what the account holds, or use another account"));
        false
    }

    fn check_open(&mut self, end: &Placed, day: Day) -> bool {
        let world = self.world;
        let place = &world.book.places[end.end.place];
        let name = world.book.name(place.path);
        let (code, rule, message, label, help) = match (place.opened, place.closed) {
            (Some(opened), _) if day < opened => {
                let early = opened.0 - day.0;
                let message = format!(
                    "`{name}` opened on {}; this flow is {} earlier",
                    iso(opened),
                    count(early as usize, "day")
                );
                let help = format!("move the flow to {} or later, or change `opened`", iso(opened));
                (
                    "place-not-open",
                    "opened",
                    message,
                    format!("{} before it opened", count(early as usize, "day")),
                    help,
                )
            }
            (_, Some(closed)) if day > closed => {
                let late = day.0 - closed.0;
                let message = format!("`{name}` was closed on {}; nothing can move on {}", iso(closed), iso(day));
                let help = "use another account, or, if it reopened, change `closed`".to_string();
                ("place-closed", "closed", message, format!("{} after it closed", count(late as usize, "day")), help)
            }
            _ => return true,
        };
        let mut error = Diagnostic::error(code, message).label(end.loc, label);
        if let Some(&line) = world.lines.get(&(end.end.place, rule)) {
            error = error.context(line, format!("{rule} here"));
        }
        self.sink.diags.push(error.help(help));
        false
    }

    /// A code may only mark a transaction that touches, in some flow, the places
    /// its rule names.
    pub fn check_codes(&mut self, flows: &[Flow]) {
        let book = &self.world.book;
        let touches = |rule: &crate::book::CodeRule, place: Id<Place>| {
            let place = &book.places[place];
            rule.on.iter().any(|scope| match *scope {
                CodeScope::Places(pattern) => glob(book.name(pattern), book.name(place.path)),
                CodeScope::Kind(kind) => book.kinds.covers(kind, place.kind),
            })
        };
        let mut reported = Vec::new();
        for &(code, loc) in &self.coded {
            let text = book.name(code);
            for rule in book.codes.iter().filter(|rule| glob(book.name(rule.pattern), text)) {
                if flows.iter().any(|flow| touches(rule, flow.from) || touches(rule, flow.to))
                    || reported.contains(&(code, loc))
                {
                    continue;
                }
                reported.push((code, loc));
                let allowed: Vec<&str> = rule
                    .on
                    .iter()
                    .map(|scope| match *scope {
                        CodeScope::Places(pattern) => book.name(pattern),
                        CodeScope::Kind(kind) => book.name(book.kinds[kind].name),
                    })
                    .collect();
                self.sink.diags.push(
                    Diagnostic::error("code-placement", format!("`#{text}` may not mark this transaction"))
                        .label(loc, "this code")
                        .context(rule.loc, "the rule for codes like it")
                        .note(format!("it may only mark transactions touching {}", list(&allowed)))
                        .help("use the code where the rule allows it, or change the rule"),
                );
            }
        }
    }
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

/// What the pairing rules need to know of a leg.
fn for_pairing(leg: &Leg, header_unit: Id<Commodity>) -> pairing::Leg {
    let price = leg.tail.price.map(|price| pairing::Price { rate: price.rate, quote: price.quote });
    match leg.slot.stated {
        Stated::Fixed(amount) => pairing::Leg::Fixed(amount, price),
        Stated::Rest => pairing::Leg::Rest,
        Stated::Unknown(unit) => pairing::Leg::Inferred(unit),
        Stated::Target(target) => pairing::Leg::Inferred(target.unit),
        Stated::All(unit) => pairing::Leg::Inferred(unit.unwrap_or(header_unit)),
    }
}
