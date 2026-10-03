//! `derive`: the step of a law that makes a flow, and the lines of a contract that are such laws.
//!
//! A law judges (`require`, `warn`) or it derives. What it derives is read when a promise's occurrence is made (the
//! fold's `derive.rs`), so a deriving law belongs to a contract, fires on `on flow`, and says nothing a posted flow
//! would judge: a flow of its own, or an item of the occurrence's header (LANGUAGE §10). This module compiles one
//! such step: its template, which is data, and its amount, which is a node of the law like any other.
//!
//! Two lines a contract writes say a law in fewer words, and are compiled to it here, so that nothing else in the
//! book or the fold knows them: `also LINE [when E]` is `on flow`, `when E`, `derive LINE`; and `share 60% for
//! studio` is `on flow`, `derive 60% of amount`, an item carved from the header and borne by the studio.

use axiom_core::{Arena, Diagnostic, Dim, Id, Loc, Run};
use axiom_syntax as ast;

use super::line::{lower_selectors, read_line, tail};
use super::{Compiler, Placement, When};
use crate::book::{Amount, Derived, Shape, Share};
use crate::declare::World;
use crate::errors::Reported;
use crate::law::{BinOp, Effect, Law, Node, NodeId, Op, Owner, Rank, Step, StepKind, Trigger, Ty, Value, Var};
use crate::scope::Home;
use crate::split::Sign;

/// A contract's `also LINE [when E]`, compiled as the law it abbreviates.
pub(crate) fn also<'s>(
    world: &mut World<'s>,
    diags: &mut Vec<Diagnostic>,
    site: &Placement<'_, 's>,
    also: &ast::Also<'s>,
) -> Option<Law> {
    let name = world.book.names.intern("also");
    let mut compiler = Compiler::placed(world, diags, site, name, When::of(&ast::Trigger::Flow));
    // Both are read before either failure stops the line, so both are said.
    let when = also.when.map(|root| compiler.condition(root));
    let effect = compiler.derive(&also.line, also.loc);
    let mut steps = Vec::new();
    if let Some(when) = when {
        steps.push(Step { loc: also.loc, kind: StepKind::When(when?) });
    }
    steps.push(Step { loc: also.loc, kind: StepKind::Effect(effect?) });
    Some(compiler.finished(site, Trigger::Flow, steps, also.loc))
}

/// A contract's `share RATE for ENTITY`, as the law it abbreviates: the entity bears `RATE of amount` of every
/// occurrence, a flow of its own that is carved out of the header and is for what the header is.
pub(crate) fn share(world: &mut World<'_>, owner: Owner, home: Home, share: &Share) -> Law {
    let name = world.book.names.intern("share");
    let loc = share.loc;
    let derived = Derived {
        shape: Shape::Item(Sign::Carve),
        purpose: None,
        owner: Some(share.entity),
        description: None,
        codes: Run::new(Id::new(world.book.codes.len() as u32), 0),
        select: Run::new(Id::new(world.book.selectors.len() as u32), 0),
        detail: None,
        waive: None,
        loc,
    };
    let mut nodes = Arena::new();
    let at = |index: u32| NodeId(index);
    let amount = Ty::AMOUNT;
    nodes.push(Node { op: Op::Const(Value::Num(share.rate)), ty: Some(Ty::Num), loc, first: at(0) });
    nodes.push(Node { op: Op::Var(Var::Amount), ty: Some(amount), loc, first: at(1) });
    nodes.push(Node { op: Op::Bin(BinOp::Mul, at(0), at(1)), ty: Some(amount), loc, first: at(0) });
    let effect = Effect::Derive { template: world.book.derived.push(derived), amount: at(2) };
    Law {
        name,
        doc: None,
        owner,
        system: if let Home::System(system) = home { Some(system) } else { None },
        trigger: Trigger::Flow,
        budget: None,
        overrides: None,
        override_name: None,
        rank: Rank::ZERO,
        steps: Box::new([Step { loc, kind: StepKind::Effect(effect) }]),
        nodes,
        loc,
    }
}

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
            owner: None,
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
