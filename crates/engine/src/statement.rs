//! A statement's split, solved when its first flow lands.
//!
//! The model solves a statement whose every amount is written out, and its flows say what they come to. One with a
//! computed amount, an `=`, an `all` or a `?` is open: its flows carry the zero they carry until the fold can say,
//! and the fold, at the first of them to land, solves the whole group with the same [`solve`] a promise's
//! occurrence is made by, reading the book as it stands. What each flow came to is then remembered in
//! `Record::resolved`, where a posting, a later flow of the group and a reversal read it, as they read an `=` or a
//! computed amount already.
//!
//! The group is solved once, at its first landing, and not flow by flow, because a remainder is what every other
//! leg and item leaves: the legs must all be known before it can be. A leg that reads the book (an `=`, an `all`)
//! reads it then. This module also reads one flow's computed amounts, which a flow that is in no group needs.

use axiom_core::{Day, Diagnostic, Id, Qty, Run};
use axiom_model::{
    Amount, Answer, Book, Drawn, End, Env, Expr, Failed, Fault, Flow, FlowExpressions, FlowSide, Heading, Infer, Line,
    Made, Program, PurposeRoot, Quantity, Remainder, Remaining, Resolved, RuntimeTxn, Sign, Statement, Txn, Value,
    solve,
};

use crate::Cause;
use crate::evaluate::{Binds, Evaluating};
use crate::explain;
use crate::ledger::Ledger;
use crate::motion::Amounts;

impl<'p, 'b, 's> Ledger<'p, 'b, 's> {
    /// What a node of a journal program computes for `flow`, or the diagnostic that says why it cannot be.
    pub(crate) fn amount_of(
        &mut self,
        id: Id<Flow>,
        flow: &Flow,
        program: &Program,
        root: axiom_model::NodeId,
        day: Day,
    ) -> Result<Amount, Diagnostic> {
        let book = self.plan.book;
        let txn = RuntimeTxn::journal(flow.txn).expect("a journal expression cannot use the template sentinel");
        let ordinal = book.txns.get(flow.txn).and_then(|txn| txn.offset(id)).unwrap_or(0);
        let reading = Evaluating {
            program,
            inputs: book.txn_inputs(flow.txn),
            txn,
            cause: Cause::Flow(id),
            ordinal,
            day: flow.day,
            binds: Binds::Flow,
        };
        let fault = match self.lent().value(flow, root, &reading) {
            Value::Amount(amount) => return Ok(amount),
            Value::Fault(fault) => fault,
            _ => Fault::InvalidProgram,
        };
        Err(explain::journal_expression_fault(book, flow, program, root, fault, day))
    }

    /// The flow with its computed out and arrive read and set. A single written quantity supplies both ends of an
    /// ordinary transfer, and a computed `=` has the balance it asks for refreshed. True if anything was computed.
    pub(crate) fn read_quantities(
        &mut self,
        id: Id<Flow>,
        flow: &mut Flow,
        roots: FlowExpressions,
        program: &Program,
        day: Day,
    ) -> Result<bool, Diagnostic> {
        let mut read = false;
        for (root, end) in [(roots.out, End::From), (roots.arrive, End::To)] {
            if let Some(root) = root {
                *flow.amount_at_mut(end) = self.amount_of(id, flow, program, root, day)?;
                read = true;
            }
        }
        // The model stores a root on the written side only, while the literal lowering has already mirrored the
        // placeholder.
        if !flow.is_exchange() {
            match (roots.out.is_some(), roots.arrive.is_some()) {
                (true, false) => flow.arrive = flow.out,
                (false, true) => flow.out = flow.arrive,
                _ => {}
            }
        }
        if let (Some(_), Infer::Target { end, .. }) = (roots.out.or(roots.arrive), flow.infer) {
            flow.infer = Infer::Target { end, balance: flow.amount_at(end).qty };
        }
        Ok(read)
    }

    /// What an exchange's cost flows add to its cost, in the base currency: the purpose-bearing `Less` items of an exchange
    /// header and the spending legs beside the exchange leg of a split are the exchange's cost evidence
    /// ([`exchange_costs_of`]). They remain ordinary posted flows for cash and purpose totals; the exchange flow carries their
    /// aggregate cost so a sale's realized proceeds shrink and a purchase's parcel basis grows. `own` is the cost the flow
    /// already says. None when there is none to say. A cost that is computed is in an open group, which was solved when its
    /// first flow landed.
    pub(crate) fn exchange_costs(
        &mut self,
        group: &Made,
        (flows, loc): (Run<Flow>, axiom_core::Loc),
        own: Option<Amount>,
        day: Day,
    ) -> Result<Option<Amount>, Diagnostic> {
        let book = self.plan.book;
        let price = |amount: Amount, at| {
            book.convert(amount, book.base, day).ok_or_else(|| {
                Diagnostic::error(
                    "exchange-cost-price",
                    format!("cannot value the exchange cost {}", book.show(amount)),
                )
                .label(at, "needed to value this exchange cost")
            })
        };
        let mut total = own.map_or(Ok(0), |cost| price(cost, loc).map(|base| base.qty.0))?;
        let mut has_cost = own.is_some();
        for offset in exchange_costs_of(book, &book.flows[flows], group) {
            let id = Id::new(flows.start().index() as u32 + offset);
            let flow = &book.flows[id];
            // What a share or a computed cost came to is in the flow it made, as the model or the first landing put it.
            let amount =
                self.record.resolved.get(&id).map_or(flow.out, |cached| Amount::new(cached.out, flow.out.unit));
            total = total.checked_add(price(amount, flow.loc)?.qty.0).ok_or_else(|| {
                Diagnostic::error("exchange-cost-overflow", "exchange costs exceed the supported amount range")
                    .label(flow.loc, "these costs do not fit in one amount")
            })?;
            has_cost = true;
        }
        Ok(has_cost.then(|| Amount::new(Qty(total), book.base)))
    }

    /// Whether the flow `id` of an open statement may post: its group is solved, now if it was not yet, and could
    /// be. A group that could not be is said once, and none of its flows posts.
    pub(crate) fn group_ready(&mut self, txn: Id<Txn>, id: Id<Flow>, day: Day) -> bool {
        if self.record.unsolved.contains(&txn) {
            return false;
        }
        if self.record.resolved.contains_key(&id) {
            return true;
        }
        match self.solve_group(txn, day) {
            Ok(()) => true,
            Err(problem) => {
                self.record.report(problem);
                self.record.unsolved.insert(txn);
                false
            }
        }
    }

    /// Solves the open group of a statement against the book as it stands, and remembers what each of its flows
    /// came to.
    fn solve_group(&mut self, txn_id: Id<Txn>, day: Day) -> Result<(), Diagnostic> {
        let book = self.plan.book;
        let txn = &book.txns[txn_id];
        let program = &book.journal_programs[txn.program.expect("an open group has a program")];
        let group = program.group.as_deref().expect("an open program has its group");
        let statement = Statement { group, flows: &book.flows[txn.flows], loc: txn.loc };
        let first = txn.flows.start();
        let at = |offset: u32| Id::new(first.index() as u32 + offset);
        let header = self.header_of(&statement, program, first, day)?;
        let (draws, bears) = statement.asked(header, book.base);
        let mut env = Reads { ledger: self, statement: &statement, program, first, header, day };
        let solved = match solve(header, &draws, &bears, Remainder::AfterItems, &mut env) {
            Ok(solved) => solved,
            Err(Failed::Env(problem)) => return Err(problem),
            Err(Failed::Fault { at, fault }) => return Err(statement.fault(book, header, at, fault)),
            Err(Failed::TwoRests { at }) => return Err(statement.two_remainders(book, header, at)),
        };
        if let Some(problem) =
            header.filter(|_| solved.exact).and_then(|header| statement.imbalance(book, header, &solved))
        {
            return Err(problem);
        }
        let said = |amount: Amount| Amounts { out: amount.qty, arrive: amount.qty };
        for (index, (leg, drawn)) in group.legs.iter().zip(solved.legs.iter()).enumerate() {
            let amount = match drawn {
                Drawn::Value(resolved) => resolved.amount,
                Drawn::Rest(amount) => *amount,
                Drawn::Omitted => continue,
            };
            let flow = &book.flows[at(leg.flow)];
            let (out, arrive) = axiom_model::balance::moved(flow, group.takes_from(), amount);
            if (out.unit, arrive.unit) != (flow.out.unit, flow.arrive.unit) {
                let fault = Fault::UnitMismatch { found: amount.unit, expected: flow.out.unit };
                return Err(statement.fault(book, header, Line::Leg(index), fault));
            }
            self.record.resolved.insert(at(leg.flow), Amounts { out: out.qty, arrive: arrive.qty });
        }
        for (item, amount) in group.items.iter().zip(solved.items.iter()) {
            if let (Some(offset), Some(amount)) = (item.flow, *amount) {
                self.record.resolved.insert(at(offset), said(amount));
            }
        }
        if let (Heading::Flow(offset), Some(left)) = (group.header, solved.header) {
            self.record.resolved.insert(at(offset), Amounts { out: left.out.qty, arrive: left.arrive.qty });
        }
        Ok(())
    }

    /// What the header says it moves, read now: a header flow's own computed and `=` amounts, or a split's total.
    fn header_of(
        &mut self,
        statement: &Statement<'_>,
        program: &Program,
        first: Id<Flow>,
        day: Day,
    ) -> Result<Option<Remaining>, Diagnostic> {
        let at = |offset: u32| Id::new(first.index() as u32 + offset);
        let counted = |amount: Amount| Remaining { out: amount, arrive: amount };
        let total = match statement.group.header {
            Heading::Flow(offset) => {
                let (id, mut flow) = (at(offset), statement.flows[offset as usize].clone());
                flow.day = day;
                if let Some(roots) = program.roots_of(offset) {
                    self.read_quantities(id, &mut flow, roots, program, day)?;
                }
                let amounts = self.amounts(&flow, Some(id));
                return Ok(Some(Remaining {
                    out: Amount::new(amounts.out, flow.out.unit),
                    arrive: Amount::new(amounts.arrive, flow.arrive.unit),
                }));
            }
            Heading::Source { total, .. } => total,
        };
        Ok(match total {
            Some(Quantity::Amount(Expr::Literal(amount)) | Quantity::Pending(Expr::Literal(amount))) => {
                Some(counted(amount))
            }
            Some(Quantity::Amount(Expr::Computed(root)) | Quantity::Pending(Expr::Computed(root))) => {
                let mut flow = statement.flows[0].clone();
                flow.day = day;
                Some(counted(self.amount_of(at(0), &flow, program, root, day)?))
            }
            Some(Quantity::All(_)) => self.all_of(&statement, day),
            _ => None,
        })
    }

    /// A split header's `all`: everything its source holds, which the way its first leg's flow leaves the source says
    /// how to read (the unit, the parcels it selects).
    fn all_of(&mut self, statement: &Statement<'_>, day: Day) -> Option<Remaining> {
        let leg = statement.group.legs.first().filter(|_| statement.group.side == FlowSide::Out)?;
        let mut flow = statement.flows[leg.flow as usize].clone();
        flow.day = day;
        let held = self.everything(&flow, Amounts::written(&flow));
        let amount = Amount::new(held.out, flow.out.unit);
        Some(Remaining { out: amount, arrive: amount })
    }
}

/// What a statement's expressions are evaluated against: the flow of each part, with the header's amount as the
/// `amount` of what is read (a share of a total is a share of that), on the day the first flow lands.
struct Reads<'a, 'p, 'b, 's> {
    ledger: &'a mut Ledger<'p, 'b, 's>,
    statement: &'a Statement<'a>,
    program: &'a Program,
    first: Id<Flow>,
    header: Option<Remaining>,
    day: Day,
}

impl Reads<'_, '_, '_, '_> {
    /// Where the flow of a part is in the transaction: a leg's own, an item's if it makes one, or else the header's
    /// (a header flow's, or the first of a split's).
    fn offset(&self, at: Line) -> u32 {
        let group = self.statement.group;
        let header = match group.header {
            Heading::Flow(offset) => offset,
            Heading::Source { .. } => group.legs.first().map_or(0, |leg| leg.flow),
        };
        match at {
            Line::Leg(index) => group.legs[index].flow,
            Line::Item(index) => group.items[index].flow.unwrap_or(header),
            Line::Header(_) => header,
        }
    }

    /// The flow of a part as the expression reads it: with the day it lands, and, where the flow's own amount is the
    /// zero it carries meanwhile, the header's `amount` in the flow's commodity: what a share of the total is a
    /// share of. An exchange header has an amount of each commodity, and the flow reads the one it counts in.
    fn flow(&self, offset: u32) -> (Id<Flow>, Flow) {
        let id = Id::new(self.first.index() as u32 + offset);
        let mut flow = self.statement.flows[offset as usize].clone();
        flow.day = self.day;
        if let Some(header) = self.header {
            let counted = if header.arrive.unit == flow.out.unit && header.out.unit != flow.out.unit {
                header.arrive
            } else {
                header.out
            };
            (flow.out, flow.arrive) = (counted, counted);
        }
        (id, flow)
    }
}

impl Env for Reads<'_, '_, '_, '_> {
    type Failure = Diagnostic;

    fn amount(&mut self, at: Line, _left: Option<&Remaining>, expr: Expr) -> Result<Answer, Self::Failure> {
        match expr {
            Expr::Literal(amount) => Ok(Answer::Amount(amount)),
            Expr::Computed(root) => {
                let (id, flow) = self.flow(self.offset(at));
                self.ledger.amount_of(id, &flow, self.program, root, self.day).map(Answer::Amount)
            }
        }
    }

    fn lands(&mut self, at: Line, marker: &Resolved) -> Result<Option<Amount>, Self::Failure> {
        let Line::Leg(index) = at else { return Ok(None) };
        let offset = self.statement.group.legs[index].flow;
        let id = Id::new(self.first.index() as u32 + offset);
        let mut flow = self.statement.flows[offset as usize].clone();
        flow.day = self.day;
        let written = Amounts::written(&flow);
        let (amounts, end) = match marker.infer {
            Infer::Target { end, balance } => (self.ledger.resolve_target(&flow, end, balance, written), end),
            Infer::All => (self.ledger.everything(&flow, written), End::From),
            Infer::Unknown => match self.ledger.plan.amounts.get(&id) {
                Some(&solved) => (solved, End::From),
                None => return Ok(None),
            },
            Infer::Known => return Ok(None),
        };
        let qty = if end == End::From { amounts.out } else { amounts.arrive };
        Ok(Some(Amount::new(qty, flow.out.unit)))
    }
}

/// The flow of a statement's group that is its exchange, by offset in the record: the header flow, or the leg of a split that
/// exchanges.
pub(crate) fn exchange_of(flows: &[Flow], group: &Made) -> Option<u32> {
    let exchanges = |offset: &u32| flows[*offset as usize].is_exchange();
    match group.header {
        Heading::Flow(header) => Some(header).filter(exchanges),
        Heading::Source { .. } => group.legs.iter().map(|leg| leg.flow).find(exchanges),
    }
}

/// The flows of a statement's group that cost its exchange, by offset in the record (LANGUAGE §3, "Pairing": legs and items
/// whose purpose is a cost): the `Less` items under an exchange header, and the other legs of a split whose source pays and
/// whose one leg exchanges, when they are spendings (a fee paid beside the exchange). [`exchange_of`] is the flow that carries them.
pub(crate) fn exchange_costs_of<'a>(
    book: &'a Book,
    flows: &'a [Flow],
    group: &'a Made,
) -> impl Iterator<Item = u32> + 'a {
    let exchange = exchange_of(flows, group);
    let spends = move |offset: &u32| {
        let purpose = flows[*offset as usize].purpose;
        purpose.is_some_and(|purpose| book.purposes[purpose.purpose].root == PurposeRoot::Spending)
    };
    let (headed, paying) = (matches!(group.header, Heading::Flow(_)), group.side == FlowSide::Out);
    let items = group.items.iter().filter(move |item| headed && item.sign == Sign::Less).filter_map(|item| item.flow);
    let legs = group.legs.iter().map(|leg| leg.flow).filter(move |&leg| !headed && paying && Some(leg) != exchange);
    items.chain(legs).filter(move |_| exchange.is_some()).filter(spends)
}
