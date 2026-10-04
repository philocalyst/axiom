//! `derive`: the step of a law that makes a flow, and the lines that are such laws.
//!
//! A law judges (`require`, `warn`) or it derives, and a law that derives fires on `on flow` and says nothing a flow
//! would judge (LANGUAGE §10). Where it is written decides when what it derives is made. A contract's law is read when
//! a promise's occurrence is made, before any of it posts (the fold's `occurrence/derive.rs`), so what it derives can
//! be an item carved from the header. Any other law is read as a flow posts, and what it derives is made of a flow that
//! has moved (the fold's `offspring.rs`): a flow of its own, or an item that is a flow along the header's ends or back.
//! This module compiles one such step: its template, which is data, and its amount, which is a node of the law like any
//! other.
//!
//! Lines say a law in fewer words, and are compiled to it here, so that nothing else in the book or the fold knows
//! them: `also LINE [when E]`, under a contract or any declaration, is `on flow`, `when E`, `derive LINE`; and a
//! contract's `share 60% for studio` is `on flow`, `derive 60% of amount`, an item carved from the header and borne by
//! the studio.

use axiom_core::{Arena, Diagnostic, Dim, Id, Loc, Run};
use axiom_syntax as ast;

use super::line::{Positions, lower_selectors, read_line};
use super::{Compiler, Placement, When};
use crate::book::{Amount, Derived, Shape, Share, Stand};
use crate::declare::World;
use crate::errors::Reported;
use crate::journal::Detail;
use crate::law::{BinOp, Effect, Law, Node, NodeId, Op, Owner, Rank, Step, StepKind, Trigger, Ty, Value, Var};
use crate::lower::tail::{Line, no_selectors, read_tail};
use crate::scope::Home;
use crate::split::Sign;

/// A contract's `also LINE [when E]`, compiled as the law it abbreviates.
pub(crate) fn also<'a, 's>(
    world: &mut World<'s>,
    site: &Placement<'a, 's>,
    also: &ast::Also<'s>,
    positions: Positions<'a>,
) -> Option<Law> {
    let name = world.book.names.intern("also");
    let mut compiler = Compiler::placed(world, site, name, When::of(&ast::Trigger::Flow));
    compiler.positions = positions;
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
        select: no_selectors(world),
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
        let said = read_line(self.world, self.home, self.file, line, loc, self.positions);
        let Some(said) = said else {
            self.failed = true;
            return None;
        };
        let amount = self.derived_amount(said.amount)?;
        let errors = self.world.diags.len();
        let (codes, tail) = read_tail(self.world, self.home, self.file, Line::Also, said.clauses);
        let detail = (tail.detail != Detail::NONE).then(|| self.world.book.details.push(tail.detail));
        let select = match said.selectors {
            Some(selectors) => lower_selectors(self.world, self.home, self.file, selectors),
            None => no_selectors(self.world),
        };
        if self.world.diags.len() != errors {
            self.failed = true;
            return None;
        }
        let in_contract = matches!(self.owner, Some(Owner::Contract(_)));
        let derived = Derived {
            shape: if in_contract { flows_own_ends(said.shape) } else { said.shape },
            owner: None,
            purpose: tail.purpose,
            description: tail.description,
            codes,
            select,
            detail,
            waive: tail.waive,
            loc,
        };
        if !in_contract && !derived.follows_a_posted_flow() {
            self.report(changes_a_posted_flow(loc));
            return None;
        }
        Some(Effect::Derive { template: self.world.book.derived.push(derived), amount })
    }

    /// How much a derived flow is: a literal (the parser has already asked for its unit) or an expression.
    fn derived_amount(&mut self, amount: ast::Amount<'s>) -> Option<NodeId> {
        match amount {
            ast::Amount::Computed(root) => self.expression(root, Ty::AMOUNT),
            ast::Amount::Literal(literal) => {
                let read = self.world.literal_amount(self.file, literal, None).or_report(self.world);
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

/// A contract's `self` is the flow itself, so as an end it is the flow's own at that position, as no end at all is.
fn flows_own_ends(shape: Shape) -> Shape {
    let own = |stand| if stand == Stand::Subject { Stand::Flow } else { stand };
    match shape {
        Shape::Flow { from, to } => Shape::Flow { from: own(from), to: own(to) },
        item @ Shape::Item(_) => item,
    }
}

fn changes_a_posted_flow(loc: Loc) -> Diagnostic {
    Diagnostic::error("derive-posted", "a flow that has posted cannot be changed or given away in part")
        .label(loc, "this item is a part of the flow it comes with, or takes from it")
        .note("a law that is not a contract's is read as a flow posts, after its value has moved; only an occurrence is made before it posts, so only a contract's law can carve an item out of it")
        .help("give it a purpose, so that it is a flow of its own along the same ends (`+ 5% of amount #fee`) or back (`- 5% of amount #refund`)")
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
