//! `derive`: the step of a law that makes a flow.
//!
//! A law judges (`require`, `warn`) or it derives. What it derives is read when a promise's occurrence is made (the
//! fold's `derive.rs`), so a deriving law belongs to a contract, fires on `on flow`, and says nothing a posted flow
//! would judge: a flow of its own, or an item of the occurrence's header, the same two things an `also` implies
//! (LANGUAGE §10). This module compiles one such step: its template, which is data, and its amount, which is a node
//! of the law like any other.

use axiom_core::{Diagnostic, Dim, Loc};
use axiom_syntax as ast;

use super::Compiler;
use crate::book::{Amount, Derived};
use crate::errors::Reported;
use crate::law::{Effect, Node, NodeId, Op, Owner, Step, StepKind, Trigger, Ty, Value};
use crate::lower::also::{lower_selectors, read_line, tail};

impl<'s> Compiler<'_, '_, 's> {
    /// `derive ITEM | FLOW`, or nothing after what is wrong with it has been said.
    pub(super) fn derive(&mut self, line: &ast::AlsoLine<'s>, loc: Loc) -> Option<Effect> {
        if !matches!(self.owner, Some(Owner::Contract(_))) {
            self.report(not_a_contracts(loc));
            return None;
        }
        let said = read_line(self.world, self.home, self.file, line, loc, self.diags);
        let Some(said) = said else {
            self.failed = true;
            return None;
        };
        let amount = self.derived_amount(said.amount)?;
        let errors = self.diags.len();
        let metadata = tail(self.world, self.home, self.file, said.clauses, self.diags);
        let select = match said.selectors {
            Some(selectors) => lower_selectors(self.world, self.home, self.file, selectors, self.diags),
            None => metadata.select,
        };
        if self.diags.len() != errors {
            self.failed = true;
            return None;
        }
        let derived = Derived {
            shape: said.shape,
            purpose: metadata.purpose,
            description: metadata.description,
            codes: metadata.codes,
            select,
            detail: metadata.detail,
            waive: metadata.waive,
            loc,
        };
        Some(Effect::Derive { template: self.world.book.derived.push(derived), amount })
    }

    /// How much a derived flow is: a literal (the parser has already asked for its unit) or an expression.
    fn derived_amount(&mut self, amount: ast::Amount<'s>) -> Option<NodeId> {
        match amount {
            ast::Amount::Computed(root) => self.expression(root, Ty::AMOUNT),
            ast::Amount::Literal(literal) => {
                let read = self.world.literal_amount(self.file, literal, None).or_report(self.diags);
                let Some(amount) = read else {
                    self.failed = true;
                    return None;
                };
                Some(self.amount_node(amount, self.file.loc(literal.0)))
            }
        }
    }

    fn amount_node(&mut self, amount: Amount, loc: Loc) -> NodeId {
        let at = NodeId(self.nodes.len() as u32);
        let ty = Some(Ty::Amount(Dim::Of(amount.unit)));
        self.nodes.push(Node { op: Op::Const(Value::Amount(amount)), ty, loc, first: at });
        at
    }

    /// What a law that derives may say: a contract's, on `on flow`, with no step that judges a posted flow.
    pub(super) fn check_derives(&mut self, trigger: Option<Trigger>, steps: &[Step], loc: Loc) {
        let derives = |step: &Step| matches!(step.kind, StepKind::Effect(Effect::Derive { .. }));
        let Some(first) = steps.iter().find(|step| derives(step)) else { return };
        if trigger.is_some_and(|trigger| trigger != Trigger::Flow) {
            self.report(not_on_flow(first.loc, loc));
        }
        if let Some(judges) = steps.iter().find(|step| {
            !derives(step) && !matches!(step.kind, StepKind::When(_) | StepKind::Unless(_) | StepKind::Let(_))
        }) {
            self.report(also_judges(first.loc, judges.loc));
        }
    }
}

fn not_a_contracts(loc: Loc) -> Diagnostic {
    Diagnostic::error("derive-owner", "a derived flow is made only for the occurrences of a contract")
        .label(loc, "this law is not a contract's")
        .note("a law that fires on a posted flow cannot change what that flow already moved; an occurrence is made before it posts, so a flow can join it")
        .help("write the law inside the contract whose occurrences should carry it")
}

fn not_on_flow(step: Loc, law: Loc) -> Diagnostic {
    Diagnostic::error("derive-trigger", "a law that derives fires on a flow")
        .label(step, "this derives when an occurrence is made")
        .context(law, "this law's trigger is something else")
        .help("write `on flow` as the law's trigger")
}

fn also_judges(derives: Loc, judges: Loc) -> Diagnostic {
    Diagnostic::error("derive-and-judge", "a law either derives or judges")
        .label(judges, "this judges a posted flow")
        .context(derives, "this derives when an occurrence is made")
        .note("what a law derives is made with the occurrence, before any of it posts; what it judges is read as each flow posts")
        .help("split it into two laws")
}
