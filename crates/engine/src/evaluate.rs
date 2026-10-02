//! Reading an expression of a record against one of its flows.
//!
//! A computed amount of a promise, of a kept occurrence and of a journal statement is a node of a program, read
//! with the flow it is for as `self`, the day it lands, and the book as the fold has it. They differ in what they are
//! given, and in what they make of a fault, and each of those is the caller's: what this module does once is the
//! reading itself, so that the three cannot drift into three ways of building a motion, an occasion and a context.

use axiom_core::Day;
use axiom_model::{Amount, Flow, NodeId, Program, RuntimeTxn, Subject, Value};

use crate::Cause;
use crate::eval;
use crate::motion::{Amounts, Motion};
use crate::plan::Plan;
use crate::state::World;

/// What `amount` reads in an expression.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Binds {
    /// Nothing: a promise's expression is read before any amount is settled.
    Nothing,
    /// The flow's own amount, out or, when that is zero, arrive: a statement's expression is about its flow.
    Flow,
}

/// Everything but the flow that an expression is read as.
#[derive(Clone, Copy)]
pub(crate) struct Evaluating<'a> {
    pub program: &'a Program,
    pub inputs: &'a [Option<Amount>],
    pub txn: RuntimeTxn,
    pub cause: Cause,
    /// The flow's place in its transaction or occurrence.
    pub ordinal: u32,
    /// The day the flow lands, which the expression's own days are counted from.
    pub day: Day,
    pub binds: Binds,
}

/// What the fold lends an expression to read: the plan, the world as it stands, and the buffer the nodes compute in.
pub(crate) struct Lent<'a, 'p, 'b, 's> {
    pub plan: &'p Plan<'b, 's>,
    pub world: &'a World,
    pub values: &'a mut Vec<Value>,
}

impl Lent<'_, '_, '_, '_> {
    /// What the node `root` of `reading.program` computes for `flow`.
    pub fn value(&mut self, flow: &Flow, root: NodeId, reading: &Evaluating<'_>) -> Value {
        let book = self.plan.book;
        let amounts = Amounts::written(flow);
        let motion = Motion::from_view_at(
            book,
            book.flow_view(flow),
            reading.txn,
            reading.cause,
            reading.day,
            amounts,
            reading.ordinal,
        );
        let mut occasion = eval::Occasion::flow(&motion);
        if reading.binds == Binds::Flow {
            occasion.amount = Some(if flow.out.qty.is_zero() { flow.arrive } else { flow.out });
        }
        let context =
            eval::Context::new(Subject::Place(flow.from), flow.owner, &occasion).for_flow().with_inputs(reading.inputs);
        eval::program_expression(
            eval::Env { plan: self.plan, world: self.world },
            reading.program,
            root,
            &context,
            self.values,
        )
    }
}
