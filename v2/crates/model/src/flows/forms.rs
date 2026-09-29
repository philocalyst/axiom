//! The forms a transaction takes: written out, an occurrence of a named plan,
//! or the holdings of an opening statement. Each becomes moves, and the moves
//! become flows under one [`Txn`].

use axiom_core::diag::closest;
use axiom_core::{Day, Diagnostic, Id, Map};
use axiom_syntax::{self as ast, Item, Quantity};

use super::moves::{Move, Moves};
use super::shape::{Elab, Placed, Shape, Slot, Stated, Tail};
use crate::errors::list;
use crate::journal::{Infer, Mode, Plan, Txn};
use crate::prices::implied_quote;
use crate::resolve::{Cause, End};

/// The named plans, and what each says when the journal instantiates it.
pub(super) struct Plans<'s> {
    pub by_name: Map<&'s str, usize>,
    /// Each plan's flow, resolved, and the plan it made; none if it did not
    /// elaborate, which was reported where the plan is written.
    pub built: Vec<Option<(Id<Plan>, Shape)>>,
    pub names: Vec<&'s str>,
}

impl<'a, 's> Elab<'a, 's> {
    /// A transaction as written.
    pub fn transaction(&mut self, item: &Item<'s>, date: Day, flow: &ast::Flow<'s>, mode: Mode) -> Option<Shape> {
        let shape = self.shape(flow, item.loc);
        let moves = shape.as_ref().and_then(|shape| self.moves(shape));
        let header = shape.as_ref().map(|shape| &shape.tail);
        self.finish(item, date, mode, header, moves, None);
        shape
    }

    /// The transaction and its flows, or just the transaction, empty, if
    /// something about it was wrong: every problem is reported together, and
    /// then none of its flows are kept.
    pub fn finish(
        &mut self,
        item: &Item<'s>,
        day: Day,
        mode: Mode,
        header: Option<&Tail>,
        moves: Option<Moves>,
        plan: Option<Id<Plan>>,
    ) {
        let world = self.world;
        let (diags, misses) = (self.sink.diags.len(), self.sink.misses.len());
        let first = self.sink.flows.len();
        let payee = header.and_then(|header| header.payee).or(moves.as_ref().and_then(|moves| moves.counterparty));
        if let Some(mut moves) = moves {
            for mv in &moves.moves {
                if let Some(flow) = self.flow(mv, day, mode) {
                    self.sink.flows.push(flow);
                }
            }
            moves.moves.clear();
            self.spare = moves.moves;
        }
        if !self.coded.is_empty() {
            let flows = std::mem::take(&mut self.sink.flows);
            self.check_codes(&flows[first..]);
            self.sink.flows = flows;
        }
        let made = &self.sink.flows[first..];
        if diags != self.sink.diags.len() || misses != self.sink.misses.len() || made.is_empty() {
            self.sink.flows.truncate(first);
        } else {
            self.sink.quotes.extend(made.iter().filter_map(|flow| implied_quote(&world.book, flow)));
        }
        self.sink.unordered |= self.sink.txns.last().is_some_and(|last| last.day > day);
        self.sink.txns.push(Txn {
            day,
            first: Id::new(first as u32),
            len: (self.sink.flows.len() - first) as u32,
            payee,
            codes: header.map_or_else(Box::default, |header| header.codes.iter().map(|&(code, _)| code).collect()),
            waive: header.and_then(|header| header.waive),
            due: header.and_then(|header| header.due).map(|due| match due {
                ast::Due::On(day) => day,
                ast::Due::After(span) => day.add(span),
            }),
            plan,
            doc: item.doc.map(|doc| world.sym(doc.0)),
            loc: item.loc,
        });
        self.txn += 1;
        self.coded.clear();
    }

    // ─── Occurrences ────────────────────────────────────────────────────────

    /// `DATE PLAN [AMOUNT]` with override legs: the plan's flow, written in
    /// full on that day. The amount replaces the header's, and each leg replaces
    /// the plan's leg for the same place, so that `...` is recomputed.
    pub fn occurrence(&mut self, item: &Item<'s>, occurrence: &ast::Occurrence<'s>, plans: &Plans<'s>) {
        let name = occurrence.plan.0;
        let Some(&at) = plans.by_name.get(name) else {
            self.sink.misses.push((Cause::Plan, name, self.file.loc(name)));
            return self.finish(item, occurrence.date, Mode::Actual, None, None, None);
        };
        let Some((plan, template)) = &plans.built[at] else {
            return self.finish(item, occurrence.date, Mode::Actual, None, None, None);
        };
        let mut shape = template.clone();
        (shape.loc, shape.arrow) = (item.loc, item.loc);
        shape.legs.iter_mut().for_each(|leg| leg.loc = item.loc);
        let mut whole = true;
        if let Some(amount) = occurrence.amount {
            match self.quantity(&Quantity::Fixed(amount), item.loc) {
                Some(slot) => restate(&mut shape, slot),
                None => whole = false,
            }
        }
        for written in &self.file[occurrence.legs] {
            let Some(leg) = self.leg(written) else {
                whole = false;
                continue;
            };
            let same = |own: &&mut super::shape::Leg| own.placed.end.place == leg.placed.end.place;
            match shape.legs.iter_mut().find(same) {
                Some(own) => {
                    own.slot = leg.slot;
                    own.tail = own.tail.over(&leg.tail);
                    own.loc = leg.loc;
                }
                None => {
                    whole = false;
                    self.not_a_leg(&shape, &leg.placed, name, plans.names[at]);
                }
            }
        }
        let moves = if whole { self.moves(&shape) } else { None };
        self.finish(item, occurrence.date, Mode::Actual, Some(&shape.tail), moves, Some(*plan));
    }

    fn not_a_leg(&mut self, shape: &Shape, placed: &Placed, name: &str, plan: &str) {
        let world = self.world;
        let path = |placed: &Placed| world.book.name(world.book.places[placed.end.place].path);
        let legs: Vec<&str> = shape.legs.iter().map(|leg| path(&leg.placed)).collect();
        let mut diagnostic = Diagnostic::error("plan-leg", format!("plan `{plan}` has no leg for `{}`", path(placed)))
            .label(placed.loc, "no leg of the plan goes here")
            .note(format!("`{name}` has legs for {}", list(&legs)));
        if let Some(near) = closest(path(placed), legs.iter().copied()) {
            diagnostic = diagnostic.help(format!("did you mean `{near}`?"));
        }
        self.sink.diags.push(diagnostic);
    }

    // ─── Openings ───────────────────────────────────────────────────────────

    /// `opening DATE` and its lines: holdings from `equity/opening`, one flow
    /// for each, in the place's display sign.
    pub fn opening(&mut self, item: &Item<'s>, opening: &ast::Opening<'s>) {
        let source = self.world.book.roots.opening;
        let mut moves = Some(Vec::with_capacity(opening.lines.len()));
        for written in &self.file[opening.lines] {
            let Some(line) = self.leg(written) else {
                moves = None;
                continue;
            };
            let Stated::Fixed(amount) = line.slot.stated else {
                continue;
            };
            if line.placed.basis || !line.placed.select.is_empty() {
                self.sink.diags.push(
                    Diagnostic::error("opening-place", "an opening line says what a whole place holds")
                        .label(line.placed.loc, "a selector or `.basis` has no meaning here")
                        .help("write the place, and how much it holds"),
                );
                moves = None;
                continue;
            }
            let equity = Placed {
                end: End { place: source, entity: None },
                loc: line.placed.loc,
                select: Vec::new(),
                basis: false,
            };
            // A liability, income or equity place is shown by what it owes or
            // has earned, so the value flows out of it into the opening equity.
            let from_equity = self.world.book.places[line.placed.end.place].class.display_sign() > 0;
            let (from, to) = if from_equity { (equity, line.placed) } else { (line.placed, equity) };
            let mv = Move::between(&from, &to, (amount, amount), (Infer::Known, false), line.tail, line.loc);
            if let Some(moves) = moves.as_mut() {
                moves.push(mv);
            }
        }
        let moves = moves.map(|moves| Moves { moves, counterparty: None });
        self.finish(item, opening.date, Mode::Opening, None, moves, None);
    }
}

/// The header amount of a plan's flow, replaced: the side with no place if the
/// header splits, otherwise the target's.
fn restate(shape: &mut Shape, slot: Slot) {
    let side = match (shape.from.placed.is_none(), shape.to.placed.is_none()) {
        (true, _) => &mut shape.from,
        (false, true) => &mut shape.to,
        (false, false) if shape.to.slot.is_none() && shape.from.slot.is_some() => &mut shape.from,
        (false, false) => &mut shape.to,
    };
    side.slot = Some(slot);
}
