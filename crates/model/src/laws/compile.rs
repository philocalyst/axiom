//! Compiling one law.
//!
//! The expressions of a law are contiguous runs of its file's expression arena,
//! in post-order. The compiler walks each run once, front to back, and produces
//! one node for each, so children are typed before their parents and the
//! engine's power-assert display can show every subexpression under its
//! source.
//!
//! An error poisons its own node and everything built from it, silently, so a
//! typo is reported once and not again at every operator above it. A law with
//! any error is dropped whole.

use axiom_core::glob::is_pattern;
use axiom_core::{Arena, Days, Diagnostic, Dim, Id, Loc, Period, Ratio, Severity, Sym};
use axiom_syntax::{
    self as ast, BinOp, Effect as WrittenEffect, ExprId, ExprKind, File, StepKind as WrittenStep, UnOp,
};

use super::types::{binary, expected, is_test, mismatch, negate, unify};
mod derive;

use super::vars::When;
use crate::book::{Entity, Input, Param};
use crate::declare::World;
use crate::errors::{Word, article, count, list, suggest};
use crate::journal::Program;
use crate::law::{
    Closing, Dir, Effect, Field, Func, Law, Node, NodeId, Op, Owner, Rank, SelectKey, Step, StepKind, Trigger, Ty,
    Value, Var, Window,
};
use crate::params::Shape;
use crate::scope::Home;
use crate::values::fits;

/// How the arguments of a call and a name in a pattern are read.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Normal,
    /// `in`, `year`: a word the function reads itself.
    Keyword,
    /// The `limit` of `limit[year]`: the param the lookup names.
    ParamBase,
    /// An alternative of `is`: a kind, place, entity or pattern, never a variable.
    Pattern,
    /// The `subject.lives` syntax node inside the special residence predicate.
    ResidenceField,
}

#[derive(Clone, Copy)]
enum Signature {
    Total,
    Tally,
    Min,
    Max,
    Abs,
    Progressive,
    Value,
    Date,
    StraightLine,
    Open,
    Peak,
    Low,
    Days,
}

/// One source of truth for each function's arity, argument roles, and typed
/// compiler handler. The role table is also used before name resolution, so
/// keywords and patterns are never mistaken for variables.
struct FunctionSpec {
    name: &'static str,
    min: usize,
    max: usize,
    roles: &'static [Role],
    signature: Signature,
}

const FUNCTION_SPECS: &[FunctionSpec] = &[
    FunctionSpec { name: "abs", min: 1, max: 1, roles: &[], signature: Signature::Abs },
    FunctionSpec { name: "date", min: 3, max: 3, roles: &[], signature: Signature::Date },
    FunctionSpec { name: "days", min: 2, max: 2, roles: &[Role::Normal, Role::Keyword], signature: Signature::Days },
    FunctionSpec { name: "low", min: 2, max: 2, roles: &[Role::Normal, Role::Keyword], signature: Signature::Low },
    FunctionSpec { name: "max", min: 2, max: 2, roles: &[], signature: Signature::Max },
    FunctionSpec { name: "min", min: 2, max: 2, roles: &[], signature: Signature::Min },
    FunctionSpec { name: "open", min: 1, max: 1, roles: &[Role::Pattern], signature: Signature::Open },
    FunctionSpec { name: "peak", min: 2, max: 2, roles: &[Role::Normal, Role::Keyword], signature: Signature::Peak },
    FunctionSpec { name: "progressive", min: 2, max: 2, roles: &[], signature: Signature::Progressive },
    FunctionSpec {
        name: "straight-line",
        min: 4,
        max: 5,
        roles: &[Role::Normal, Role::Normal, Role::Normal, Role::Keyword, Role::Keyword],
        signature: Signature::StraightLine,
    },
    FunctionSpec { name: "tally", min: 1, max: 2, roles: &[Role::Keyword], signature: Signature::Tally },
    FunctionSpec {
        name: "total",
        min: 1,
        max: 3,
        roles: &[Role::Keyword, Role::Keyword, Role::Pattern],
        signature: Signature::Total,
    },
    FunctionSpec {
        name: "value",
        min: 2,
        max: 3,
        roles: &[Role::Normal, Role::Normal, Role::Keyword],
        signature: Signature::Value,
    },
];

fn function_spec(name: &str) -> Option<&'static FunctionSpec> {
    FUNCTION_SPECS.iter().find(|spec| spec.name == name)
}

fn function_names() -> Vec<&'static str> {
    FUNCTION_SPECS.iter().map(|spec| spec.name).collect()
}

/// A call being checked: the function, and each argument as written, as a node and where it is.
struct Call<'c, 's> {
    function: Word<'s>,
    args: &'c [ExprId],
    typed: Vec<(NodeId, Ty)>,
    locs: Vec<Loc>,
}

impl Call<'_, '_> {
    /// The type of argument `at`, which is empty for one not given.
    fn ty(&self, at: usize) -> Ty {
        self.typed.get(at).map_or(Ty::Empty, |&(_, ty)| ty)
    }

    fn loc(&self, at: usize) -> Loc {
        self.locs[at]
    }

    fn nodes(&self) -> Box<[NodeId]> {
        self.typed.iter().map(|&(node, _)| node).collect()
    }

    /// Argument `at` is `wanted`, which `fits` says its type is.
    fn want(&self, at: usize, wanted: &str, fits: impl Fn(Ty) -> bool) -> Check<()> {
        match self.ty(at) {
            ty if fits(ty) => Ok(()),
            ty => Err(expected(wanted, ty, self.loc(at)).into()),
        }
    }

    fn check_arity(&self, spec: &FunctionSpec, loc: Loc) -> Check<()> {
        if (spec.min..=spec.max).contains(&self.typed.len()) {
            return Ok(());
        }
        let takes = if spec.min == spec.max { spec.min.to_string() } else { format!("{} to {}", spec.min, spec.max) };
        Err(Diagnostic::error(
            "call-arity",
            format!("`{}` takes {takes} arguments, but {} were given", self.function.text, self.typed.len()),
        )
        .label(loc, "wrong number of arguments")
        .into())
    }

    /// `min(a, b)` and `max(a, b)`: both of one type that is ordered.
    fn extremum(&self, func: Func) -> Check<(Func, Ty)> {
        let shared = unify(self.ty(0), self.ty(1)).filter(|&shared| binary(BinOp::Lt, shared, shared).is_some());
        let shared = shared.ok_or_else(|| mismatch(BinOp::Lt, (self.ty(0), self.loc(0)), (self.ty(1), self.loc(1))))?;
        Ok((func, shared))
    }

    fn abs(&self) -> Check<(Func, Ty)> {
        let ty = negate(self.ty(0)).ok_or_else(|| expected("an amount or a number", self.ty(0), self.loc(0)))?;
        Ok((Func::Abs, ty))
    }

    /// `date(year, month, day)`.
    fn date(&self) -> Check<(Func, Ty)> {
        for at in 0..3 {
            self.want(at, "a number", |ty| ty == Ty::Num)?;
        }
        Ok((Func::Date, Ty::Day))
    }

    /// `straight_line(amount, span, start[, option, option])`.
    fn straight_line(&self) -> Check<(Func, Ty)> {
        let wants = [Ty::Amount(Dim::Any), Ty::Span, Ty::Day, Ty::Name, Ty::Name];
        for (at, &want) in wants.iter().take(self.args.len()).enumerate() {
            let phrase = if want == Ty::Name { "a `mid-month` option".into() } else { article(want.word()) };
            self.want(at, &phrase, |ty| fits(want, ty))?;
        }
        Ok((Func::StraightLine, self.ty(0)))
    }
}

/// Why a node has no type.
enum Bad {
    /// A diagnostic for it, boxed because every step of the compiler returns a `Check` and a diagnostic is large.
    Report(Box<Diagnostic>),
    /// A child was already reported.
    Cascade,
}

impl From<Diagnostic> for Bad {
    fn from(diagnostic: Diagnostic) -> Bad {
        Bad::Report(Box::new(diagnostic))
    }
}

type Check<T> = Result<T, Bad>;

/// Where a law was written, and what it governs.
pub(crate) struct Placement<'a, 's> {
    pub file: &'a File<'s>,
    pub home: Home,
    pub owner: Owner,
    /// What `self` is in it.
    pub subject: Ty,
}

pub(crate) fn compile<'s>(
    world: &mut World<'s>,
    diags: &mut Vec<Diagnostic>,
    site: &Placement<'_, 's>,
    law: &ast::Law<'s>,
) -> Option<Law> {
    let law_name = world.book.names.intern(law.name.0);
    let mut compiler = Compiler {
        world,
        diags,
        file: site.file,
        home: site.home,
        owner: Some(site.owner),
        subject: site.subject,
        law_name,
        first: None,
        base: 0,
        nodes: Arena::new(),
        roles: Vec::new(),
        locals: Vec::new(),
        inputs: &[],
        when: When::of(&law.trigger),
        failed: false,
    };
    compiler.law(site, law)
}

/// Compiles the expressions used by a contract term's flow templates into one
/// shared arena. Roots retain their declaration order and inputs resolve by
/// their stable index in `inputs`.
pub(crate) fn compile_template<'s>(
    world: &mut World<'s>,
    diags: &mut Vec<Diagnostic>,
    file: &File<'s>,
    home: Home,
    subject: Ty,
    name: Sym,
    inputs: &[Input],
    roots: &[(ExprId, Ty)],
) -> Option<(Program, Box<[NodeId]>)> {
    let mut compiler = Compiler {
        world,
        diags,
        file,
        home,
        owner: None,
        subject,
        law_name: name,
        first: None,
        base: 0,
        nodes: Arena::new(),
        roles: Vec::new(),
        locals: Vec::new(),
        inputs,
        when: When::Template,
        failed: false,
    };
    let compiled: Vec<NodeId> = roots.iter().filter_map(|&(root, want)| compiler.expression(root, want)).collect();
    if compiler.failed || compiled.len() != roots.len() {
        return None;
    }
    Some((Program::of(std::mem::take(&mut compiler.nodes)), compiled.into()))
}

/// Compiles a dated budget limit in its purpose-owner context. Unlike a
/// contract template, a budget is not triggered by a transaction, so amount,
/// from and to are unavailable while its formula is read.
pub(crate) fn compile_budget_limit<'s>(
    world: &mut World<'s>,
    diags: &mut Vec<Diagnostic>,
    file: &File<'s>,
    home: Home,
    purpose: axiom_core::Id<crate::book::Purpose>,
    root: ExprId,
) -> Option<(Program, NodeId)> {
    let name = world.book.purposes[purpose].name;
    let mut compiler = Compiler {
        world,
        diags,
        file,
        home,
        owner: Some(Owner::Purpose(purpose)),
        subject: Ty::Entity,
        law_name: name,
        first: None,
        base: 0,
        nodes: Arena::new(),
        roles: Vec::new(),
        locals: Vec::new(),
        inputs: &[],
        when: When::Each,
        failed: false,
    };
    let root = compiler.expression(root, Ty::AMOUNT)?;
    if compiler.failed {
        return None;
    }
    Some((Program::of(std::mem::take(&mut compiler.nodes)), root))
}

struct Compiler<'w, 'a, 's> {
    world: &'w mut World<'s>,
    diags: &'w mut Vec<Diagnostic>,
    file: &'a File<'s>,
    home: Home,
    owner: Option<Owner>,
    subject: Ty,
    law_name: Sym,
    /// The source index of the first node of the run being compiled, and the
    /// index the first node of it got.
    first: Option<ExprId>,
    base: usize,
    nodes: Arena<Node>,
    /// The role of each node of the run being compiled.
    roles: Vec<Role>,
    /// `let` bindings in scope, and the node holding each value.
    locals: Vec<(&'s str, NodeId)>,
    inputs: &'a [Input],
    when: When,
    failed: bool,
}

impl<'s> Compiler<'_, '_, 's> {
    fn law(&mut self, site: &Placement<'_, 's>, law: &ast::Law<'s>) -> Option<Law> {
        let trigger = self.trigger(&law.trigger);
        self.when = When::of(&law.trigger);
        let written = &self.file[law.steps];
        let steps: Vec<Step> = written.iter().filter_map(|step| self.step(step)).collect();
        if self.failed || steps.len() != written.len() {
            return None;
        }
        self.check_derives(trigger, &steps, law.loc);
        if self.failed {
            return None;
        }
        Some(Law {
            name: self.law_name,
            doc: law.doc.map(|doc| self.world.book.names.intern(doc.0)),
            owner: site.owner,
            system: if let Home::System(system) = site.home { Some(system) } else { None },
            trigger: trigger?,
            budget: None,
            overrides: None,
            override_name: law.overrides.map(|name| self.world.book.names.intern(name.0)),
            rank: Rank::ZERO,
            steps: steps.into(),
            nodes: std::mem::take(&mut self.nodes),
            loc: law.loc,
        })
    }

    fn trigger(&mut self, trigger: &ast::Trigger) -> Option<Trigger> {
        self.when = When::Deadline;
        Some(match *trigger {
            ast::Trigger::In => Trigger::In,
            ast::Trigger::Out => Trigger::Out,
            ast::Trigger::Gain => Trigger::Gain,
            ast::Trigger::Spend => Trigger::Spend,
            ast::Trigger::Flow => Trigger::Flow,
            ast::Trigger::Each(period) => Trigger::Each(period, None),
            ast::Trigger::Closing { month, day } => Trigger::Each(ast::Period::Year, Some(Closing { month, day })),
            ast::Trigger::Always => Trigger::Always,
            ast::Trigger::By(root) => Trigger::By(self.expression(root, Ty::Day)?),
        })
    }

    fn step(&mut self, step: &ast::Step<'s>) -> Option<Step> {
        let kind = match &step.kind {
            WrittenStep::When(root) => StepKind::When(self.condition(*root)?),
            WrittenStep::Unless(root) => StepKind::Unless(self.condition(*root)?),
            WrittenStep::Let(name, root) => {
                // Bound even if it failed, so its uses do not report it again.
                let bound = self.compile(*root);
                self.locals.push((name.0, bound));
                StepKind::Let(self.nodes[bound].typed_ty().map(|_| bound)?)
            }
            WrittenStep::Require { cond, otherwise, message, warn } => {
                let cond = self.condition(*cond)?;
                let otherwise: Box<[Effect]> = self.file[*otherwise]
                    .iter()
                    .map(|effect| self.effect(effect, step.loc))
                    .collect::<Option<Vec<_>>>()?
                    .into();
                let message = message.map(|text| self.world.book.names.intern(text.0));
                let severity = if *warn { Severity::Warning } else { Severity::Error };
                StepKind::Require { cond, otherwise, message, severity }
            }
            WrittenStep::Effect(effect) => StepKind::Effect(self.effect(effect, step.loc)?),
        };
        Some(Step { loc: step.loc, kind })
    }

    fn effect(&mut self, effect: &WrittenEffect<'s>, loc: Loc) -> Option<Effect> {
        match effect {
            WrittenEffect::Owe { amount, to, due, name } => {
                let amount = self.expression(*amount, self.owner_amount_ty())?;
                let to = self.owed_to(to.0);
                let due = match due {
                    Some(due) => Some(self.expression(*due, Ty::Day)?),
                    None => None,
                };
                let name = name.map_or(self.law_name, |name| self.world.book.names.intern(name.0));
                Some(Effect::Owe { amount, to: to?, due, name })
            }
            WrittenEffect::Count { amount, name } => {
                let amount = self.expression(*amount, self.owner_amount_ty())?;
                Some(Effect::Count { amount, name: self.world.book.names.intern(name.0) })
            }
            WrittenEffect::Consume(amount) => {
                Some(Effect::Consume { amount: self.expression(*amount, self.owner_amount_ty())? })
            }
            WrittenEffect::Derive(line) => self.derive(line, loc),
            WrittenEffect::Carry { amount, to, within } => {
                let amount = self.expression(*amount, self.owner_amount_ty())?;
                let unit = self.expression(*to, Ty::Unit)?;
                let node = NodeId(self.nodes.len() as u32);
                self.nodes.push(Node { op: Op::Const(Value::Span(*within)), ty: Some(Ty::Span), loc, first: node });
                Some(Effect::Carry { amount, unit, within: node })
            }
        }
    }

    fn owed_to(&mut self, name: &'s str) -> Option<Id<Entity>> {
        let entity = self.world.entity(self.home, Word::of(self.file, name));
        entity.map_err(|diagnostic| self.report(diagnostic)).ok()
    }

    fn report(&mut self, diagnostic: Diagnostic) {
        self.failed = true;
        self.diags.push(diagnostic);
    }

    // ─── Nodes ──────────────────────────────────────────────────────────────

    /// Compiles the expression at `root`, all of its subtree, and returns its
    /// node.
    fn compile(&mut self, root: ExprId) -> NodeId {
        let subtree = self.file.exprs.subtree(root);
        (self.first, self.base) = (Some(self.file.exprs[root].first), self.nodes.len());
        self.roles = self.roles_of(subtree);
        for (offset, expr) in subtree.iter().enumerate() {
            self.node(offset, expr);
        }
        NodeId(self.nodes.len() as u32 - 1)
    }

    /// Which nodes are keywords, param names or `is` alternatives: parents
    /// decide, and children come first, so it is settled before compiling.
    fn roles_of(&self, subtree: &[ast::Expr<'s>]) -> Vec<Role> {
        let mut roles = vec![Role::Normal; subtree.len()];
        let first = self.first.expect("roles are assigned while compiling an expression");
        let mut mark = |id: ExprId, role: Role| {
            if let Some(slot) = roles.get_mut(id.offset_from(first)) {
                *slot = role;
            }
        };
        for expr in subtree {
            match expr.kind {
                ExprKind::Call(name, args) => {
                    let written = &self.file[args];
                    if name.0 == "total"
                        && matches!(written.first(), Some(id) if matches!(self.file.exprs[*id].kind, ExprKind::Purpose(_)))
                    {
                        mark(written[0], Role::Pattern);
                        written.iter().skip(1).for_each(|&arg| mark(arg, Role::Keyword));
                    } else if let Some(spec) = function_spec(name.0) {
                        for (at, &arg) in written.iter().enumerate() {
                            if let Some(&role) = spec.roles.get(at) {
                                mark(arg, role);
                            }
                        }
                    } else {
                        // The function name is the useful diagnostic. Do not
                        // cascade unknown-name errors from arguments.
                        written.iter().for_each(|&arg| mark(arg, Role::Keyword));
                    }
                }
                ExprKind::Index(base, _) => mark(base, Role::ParamBase),
                ExprKind::Select(keys) => self.file[keys].iter().for_each(|&key| mark(key, Role::Keyword)),
                ExprKind::Is(subject, alternatives) => {
                    if matches!(self.file.exprs[subject].kind, ExprKind::Field(_, field) if field.0 == "lives") {
                        mark(subject, Role::ResidenceField);
                        self.file[alternatives].iter().for_each(|&alt| mark(alt, Role::Keyword));
                    } else {
                        self.file[alternatives].iter().for_each(|&alt| mark(alt, Role::Pattern));
                    }
                }
                _ => {}
            }
        }
        roles
    }

    /// The node of `root`, unless it or something beneath it failed.
    fn value(&mut self, root: ExprId) -> Option<NodeId> {
        let node = self.compile(root);
        self.nodes[node].typed_ty().map(|_| node)
    }

    /// A root that must have the type `want`: an amount or a date.
    fn expression(&mut self, root: ExprId, want: Ty) -> Option<NodeId> {
        let node = self.value(root)?;
        let found = &self.nodes[node];
        let found_ty = found.typed_ty()?;
        if fits(want, found_ty) {
            return Some(node);
        }
        let diagnostic = expected(&article(want.word()), found_ty, found.loc);
        self.report(diagnostic);
        None
    }

    fn condition(&mut self, root: ExprId) -> Option<NodeId> {
        let node = self.value(root)?;
        let found = &self.nodes[node];
        let found_ty = found.typed_ty()?;
        if found_ty == Ty::Bool {
            return Some(node);
        }
        let diagnostic = expected("a condition", found_ty, found.loc)
            .note("a condition compares things, as in `amount <= 500 USD`, or tests them, as in `to is expenses/food`");
        self.report(diagnostic);
        None
    }

    /// The node an expression of the run being compiled became.
    fn node_id(&self, id: ExprId) -> NodeId {
        NodeId((self.base + id.offset_from(self.first.expect("expression node has an active root"))) as u32)
    }

    fn node(&mut self, at: usize, expr: &ast::Expr<'s>) {
        let first = self.node_id(expr.first);
        let (op, ty) = match self.check(at, expr) {
            Ok((op, ty)) => (op, Some(ty)),
            Err(Bad::Report(diagnostic)) => {
                self.report(*diagnostic);
                (Op::Const(Value::Empty), None)
            }
            Err(Bad::Cascade) => (Op::Const(Value::Empty), None),
        };
        self.nodes.push(Node { op, ty, loc: expr.loc, first });
    }

    /// The typed child, or the reason it has none.
    fn child(&self, id: ExprId) -> Check<(NodeId, Ty)> {
        let node = self.node_id(id);
        self.nodes[node].typed_ty().map(|ty| (node, ty)).ok_or(Bad::Cascade)
    }

    fn children(&self, ids: &[ExprId]) -> Check<Vec<(NodeId, Ty)>> {
        ids.iter().map(|&id| self.child(id)).collect()
    }

    fn check(&mut self, at: usize, expr: &ast::Expr<'s>) -> Check<(Op, Ty)> {
        if self.roles[at] == Role::ResidenceField {
            // This Field node is only the syntactic marker for the special
            // `entity.lives is SYSTEM` predicate. Its parent compiles directly
            // to Op::Resides and does not evaluate this child.
            return Ok((Op::Const(Value::Empty), Ty::Bool));
        }
        if let Some((value, ty)) = self.world.literal(self.home, self.file, expr)? {
            return Ok((Op::Const(value), ty));
        }
        let file = self.file;
        match expr.kind {
            ExprKind::Year(year) => Ok((Op::Const(Value::Num(Ratio::int(i64::from(year)))), Ty::Num)),
            ExprKind::Name(name) => self.name(at, Word { text: name.0, loc: expr.loc }),
            ExprKind::Field(receiver, field) => self.field(receiver, Word::of(file, field.0)),
            ExprKind::Index(base, keys) => self.lookup(base, &file[keys], expr.loc),
            ExprKind::Call(function, args) => self.call(Word::of(file, function.0), &file[args], expr.loc),
            ExprKind::Unary(op, operand) => self.unary(op, operand),
            ExprKind::Binary(op, left, right) => self.binary(op, left, right),
            ExprKind::Of(purpose, object) => self.of(purpose, object),
            ExprKind::At(quantity, price) => self.at(quantity, price),
            ExprKind::Is(subject, alternatives) => self.is(subject, &file[alternatives]),
            ExprKind::If(condition, then, otherwise) => self.conditional(condition, then, otherwise),
            ExprKind::Schedule(_) => Err(Diagnostic::error("schedule-position", "a schedule belongs in a param")
                .label(expr.loc, "write it as a row of a `param`, and look it up here")
                .into()),
            ExprKind::Purpose(name) => {
                let word = Word { text: name.0, loc: expr.loc };
                let purpose = self.world.purpose(self.home, word)?;
                Ok((Op::Const(Value::Purpose(purpose, None)), Ty::Purpose))
            }
            ExprKind::Select(keys) => self.select(&file[keys]),
            ExprKind::Month(_) | ExprKind::Fraction(..) => {
                Err(Diagnostic::error("law-expression", "this expression is not supported here")
                    .label(expr.loc, "use a date, amount, name or supported law expression")
                    .into())
            }
            _ => Err(Diagnostic::error("law-expression", "this expression is not supported here")
                .label(expr.loc, "use a supported law expression")
                .into()),
        }
    }

    // ─── Names ──────────────────────────────────────────────────────────────

    fn name(&mut self, at: usize, word: Word<'s>) -> Check<(Op, Ty)> {
        match self.roles[at] {
            Role::Keyword | Role::ParamBase => {
                let sym = self.world.book.names.intern(word.text);
                Ok((Op::Const(Value::Name(sym)), Ty::Name))
            }
            Role::Pattern => self.constant(word),
            Role::ResidenceField => Err(Bad::Cascade),
            Role::Normal => {
                if let Some(&(_, bound)) = self.locals.iter().rev().find(|(local, _)| *local == word.text) {
                    return self.local(bound);
                }
                if let Some((index, input)) =
                    self.inputs.iter().enumerate().find(|(_, input)| self.world.book.name(input.name) == word.text)
                {
                    let index = u16::try_from(index).map_err(|_| {
                        Diagnostic::error("too-many-inputs", "a contract has too many inputs")
                            .label(input.loc, "input index exceeds the language limit")
                    })?;
                    let ty = input.unit.map_or(Dim::Any, Dim::Of);
                    return Ok((Op::Var(Var::Input(index)), Ty::Amount(ty)));
                }
                if let Some(var) = Var::parse(word.text) {
                    return self.variable(var, word);
                }
                match self.world.seek_param(self.home, word)? {
                    Some(param) => self.bare_param(param, word),
                    None => self.constant(word),
                }
            }
        }
    }

    fn local(&self, bound: NodeId) -> Check<(Op, Ty)> {
        self.nodes[bound].typed_ty().map(|ty| (Op::Local(bound), ty)).ok_or(Bad::Cascade)
    }

    fn variable(&self, var: Var, word: Word) -> Check<(Op, Ty)> {
        if var.provided_by(self.when) {
            let ty = match var {
                Var::Input(index) => self
                    .inputs
                    .get(index as usize)
                    .map_or(Ty::AMOUNT, |input| Ty::Amount(input.unit.map_or(Dim::Any, Dim::Of))),
                Var::Amount => self.flow_amount_ty(),
                Var::Gain | Var::Proceeds | Var::Basis | Var::Balance | Var::Remaining => self.owner_amount_ty(),
                _ => var.ty(self.subject),
            };
            return Ok((Op::Var(var), ty));
        }
        let mut diagnostic = Diagnostic::error("law-variable", format!("`{}` is not available in this law", word.text));
        if self.when == When::Deadline {
            diagnostic = diagnostic
                .label(word.loc, "not known yet")
                .note("the expression after `by` computes the deadline, so it cannot read what happens at the deadline")
                .help("it may read `self` and `owner`");
        } else {
            let suppliers: Vec<&str> = var.suppliers().map(When::phrase).collect();
            diagnostic = diagnostic
                .label(word.loc, format!("this law's trigger does not provide `{}`", word.text))
                .help(format!("`{}` is provided by {} laws", word.text, suppliers.join(" and ")));
        }
        Err(diagnostic.into())
    }

    /// A kind, place, entity, or pattern written where a value is expected.
    fn constant(&mut self, word: Word<'s>) -> Check<(Op, Ty)> {
        if is_pattern(word.text) {
            return Ok((Op::Const(Value::Glob(self.world.book.names.intern(word.text))), Ty::Glob));
        }
        if let Some(kind) = self.world.seek_kind(self.home, word)? {
            return Ok((Op::Const(Value::Kind(kind)), Ty::Kind));
        }
        // A system knows nothing of the project's places.
        if self.home == Home::Project
            && let Some(place) = self.world.seek_place(word)?
        {
            return Ok((Op::Const(Value::Place(place)), Ty::Place));
        }
        if let Some(asset) = self.world.book.asset(word.text) {
            return Ok((Op::Const(Value::Asset(asset)), Ty::Asset));
        }
        match self.world.seek_entity(self.home, word)? {
            Some(entity) => Ok((Op::Const(Value::Entity(entity)), Ty::Entity)),
            None => Err(self.unknown_constant(word).into()),
        }
    }

    /// Nothing is called `word`. A kind that exists in a system this law's
    /// system does not use is the better explanation, when there is one.
    fn unknown_constant(&self, word: Word<'s>) -> Diagnostic {
        match self.world.kind(self.home, word) {
            Err(diagnostic) if !diagnostic.notes.is_empty() => diagnostic,
            _ => self.unknown_name(word),
        }
    }

    fn unknown_name(&self, word: Word<'s>) -> Diagnostic {
        let mut known: Vec<&str> =
            Var::words().filter(|name| Var::parse(name).is_some_and(|var| var.provided_by(self.when))).collect();
        known.extend(self.locals.iter().map(|(local, _)| *local));
        known.extend(self.inputs.iter().map(|input| self.world.book.name(input.name)));
        let (names, lookup) = (&self.world.book.names, &self.world.book.lookup);
        let scope = self.world.scopes.of(self.home);
        known.extend(lookup.params.names.keys(names));
        known.extend(lookup.kinds.names.keys(names).filter(|key| {
            lookup.kinds.names.candidates(names, key).iter().any(|&id| scope.sees(lookup.kinds.home(id)))
        }));
        known.extend(lookup.entities.names.keys(names));
        known.extend(self.world.book.assets.values().map(|asset| self.world.book.name(asset.name)));
        known.extend(
            self.world.book.purposes.ids().map(|purpose| self.world.book.name(self.world.book.purposes[purpose].name)),
        );
        if self.home == Home::Project {
            known.extend(lookup.places.keys(names));
        }
        let diagnostic = Diagnostic::error("unknown-name", format!("`{}` means nothing in this law", word.text))
            .label(word.loc, "not a variable, param, kind, place or entity here")
            .note("a name in a law is a variable of the trigger (`amount`, `from`, `date`, …), a `let`, a param, or a kind, place or entity");
        suggest(diagnostic, word.loc, word.text, known)
    }

    // ─── Fields and lookups ─────────────────────────────────────────────────

    fn field(&mut self, receiver: ExprId, field: Word<'s>) -> Check<(Op, Ty)> {
        let (node, ty) = self.child(receiver)?;
        let built_in = match (ty, field.text) {
            (Ty::Place, "balance") => Some((Field::Balance, self.value_amount_ty(node))),
            (Ty::Place, "basis") => Some((Field::Basis, self.value_amount_ty(node))),
            (Ty::Asset, "cost") => Some((Field::Cost, self.value_amount_ty(node))),
            (Ty::Asset, "basis") => Some((Field::Basis, self.value_amount_ty(node))),
            (Ty::Asset, "in-service") => Some((Field::InService, Ty::Day)),
            (Ty::Asset, "parts") => Some((Field::Parts, Ty::Num)),
            (Ty::Purpose, "of") => Some((Field::Of, Ty::Asset)),
            (Ty::Amount(_) | Ty::Empty, "unit") => Some((Field::Unit, Ty::Unit)),
            (Ty::Place | Ty::Entity, "owner") => Some((Field::Owner, Ty::Entity)),
            (Ty::Place | Ty::Entity | Ty::Unit, "kind") => Some((Field::Kind, Ty::Kind)),
            (Ty::Entity, "age") => Some((Field::Age, Ty::Span)),
            (Ty::Day, "year") => Some((Field::Year, Ty::Num)),
            (Ty::Day, "month") => Some((Field::Month, Ty::Num)),
            _ => None,
        };
        if let Some((field, ty)) = built_in {
            if matches!(field, Field::Age) {
                // Age counts from `born`, which the evaluator finds by name even
                // when no entity wrote one.
                self.world.book.names.intern("born");
            }
            return Ok((Op::Field(node, field), ty));
        }
        let sym = self.world.book.names.intern(field.text);
        if let Some(ty) = self.world.book.schema.field(ty, sym) {
            return Ok((Op::Field(node, Field::Prop(sym)), ty));
        }
        Err(self.unknown_field(ty, field, receiver).into())
    }

    /// Compiles the selector on an expression such as `50% of [retirement]`.
    /// The ids and keys are fixed now; the engine applies them to the borrowed,
    /// materialized occurrence groups when the template runs.
    fn select(&mut self, keys: &[ExprId]) -> Check<(Op, Ty)> {
        let mut resolved = Vec::with_capacity(keys.len());
        for &key in keys {
            let expr = &self.file.exprs[key];
            let word = |text| Word { text, loc: expr.loc };
            let selected = match expr.kind {
                ExprKind::Purpose(name) => SelectKey::Purpose(self.world.purpose(self.home, word(name.0))?),
                ExprKind::Code(code) => SelectKey::Code(self.world.book.names.intern(code.name())),
                ExprKind::Unit(name) => SelectKey::Unit(self.world.commodity_of(word(name.0))?),
                ExprKind::Date(day) => SelectKey::Range(Days::on(day)),
                ExprKind::Month(day) => {
                    SelectKey::Range(axiom_core::calendar::Window::containing(Period::Month, day).days())
                }
                ExprKind::Year(year) => {
                    let first =
                        axiom_core::Day::from_ymd(year, 1, 1).expect("a four-digit year is a valid calendar year");
                    SelectKey::Range(axiom_core::calendar::Window::containing(Period::Year, first).days())
                }
                ExprKind::Name(name) => SelectKey::End(self.world.end(self.home, word(name.0))?.place),
                _ => {
                    return Err(Diagnostic::error("selector-key", "this expression cannot select parts of a flow")
                        .label(expr.loc, "use a purpose, endpoint, code, unit, date, month or year")
                        .into());
                }
            };
            resolved.push(selected);
        }
        Ok((Op::Select(resolved.into()), Ty::AMOUNT))
    }

    /// Currency used for this law's owner-scoped amounts. A kind, purpose,
    /// project, or unconfigured system can govern many owners with different
    /// currencies, so those contexts remain genuinely dynamic.
    fn owner_amount_ty(&self) -> Ty {
        let currency = match self.owner {
            Some(Owner::Place(place)) => Some(self.world.book.currency(self.world.book.places[place].owner)),
            Some(Owner::Entity(entity)) => Some(self.world.book.currency(entity)),
            Some(Owner::Asset(asset)) => {
                let owner = self.world.book.assets[asset].owner;
                Some(self.world.book.currency(owner))
            }
            Some(Owner::Contract(contract)) => {
                let owner = self.world.book.contracts[contract].owner;
                Some(self.world.book.currency(owner))
            }
            // A system's currency is only the default for its residents;
            // individual entities may set another one.
            Some(Owner::System(_)) => None,
            Some(Owner::Kind(_) | Owner::Purpose(_) | Owner::Book) | None => None,
        };
        currency.map_or(Ty::AMOUNT, |unit| Ty::Amount(Dim::Of(unit)))
    }

    /// Currency of an explicitly named subject, or the law owner's currency
    /// for `self`; a generic expression stays dynamic.
    fn value_amount_ty(&self, node: NodeId) -> Ty {
        let currency = match self.nodes[node].op {
            Op::Const(Value::Place(place)) => {
                let owner = self.world.book.places[place].owner;
                Some(self.world.book.currency(owner))
            }
            Op::Const(Value::Entity(entity)) => Some(self.world.book.currency(entity)),
            Op::Const(Value::Asset(asset)) => {
                let owner = self.world.book.assets[asset].owner;
                Some(self.world.book.currency(owner))
            }
            Op::Var(Var::Subject) => match self.owner {
                Some(Owner::Place(place)) => {
                    let owner = self.world.book.places[place].owner;
                    Some(self.world.book.currency(owner))
                }
                Some(Owner::Entity(entity)) => Some(self.world.book.currency(entity)),
                Some(Owner::Asset(asset)) => {
                    let owner = self.world.book.assets[asset].owner;
                    Some(self.world.book.currency(owner))
                }
                Some(Owner::Contract(contract)) => {
                    let owner = self.world.book.contracts[contract].owner;
                    Some(self.world.book.currency(owner))
                }
                Some(Owner::System(_)) => None,
                _ => None,
            },
            _ => None,
        };
        currency.map_or(Ty::AMOUNT, |unit| Ty::Amount(Dim::Of(unit)))
    }

    /// A flow amount has a static unit only when its governing place declares
    /// exactly one accepted commodity. Otherwise the expression must state a
    /// conversion with `value(amount, UNIT)` before comparing unlike units.
    fn flow_amount_ty(&self) -> Ty {
        let unit = match self.owner {
            Some(Owner::Place(place)) => self.world.book.holds_only(place),
            Some(Owner::Asset(asset)) => Some(self.world.book.assets[asset].unit),
            _ => None,
        };
        unit.map_or(Ty::AMOUNT, |unit| Ty::Amount(Dim::Of(unit)))
    }

    fn unknown_field(&self, ty: Ty, field: Word<'s>, receiver: ExprId) -> Diagnostic {
        let mut valid: Vec<&str> = match ty {
            Ty::Place => vec!["balance", "basis", "owner", "kind"],
            Ty::Asset => vec!["basis", "cost", "in-service", "parts"],
            Ty::Purpose => vec!["of"],
            Ty::Entity => vec!["owner", "kind", "age"],
            Ty::Unit => vec!["kind"],
            Ty::Amount(_) | Ty::Empty => vec!["unit"],
            Ty::Day => vec!["year", "month"],
            _ => Vec::new(),
        };
        // In the order of the alphabet, not of the hash of their symbols, which every new name the book interns shuffles.
        let mut declared: Vec<&str> = self.world.book.schema.fields(ty).map(|sym| self.world.book.name(sym)).collect();
        declared.sort_unstable();
        valid.extend(declared);
        let mut diagnostic =
            Diagnostic::error("unknown-field", format!("{} has no `{}`", article(ty.word()), field.text))
                .label(field.loc, "no such field")
                .context(self.file.exprs[receiver].loc, format!("this is {}", article(ty.word())));
        diagnostic = suggest(diagnostic, field.loc, field.text, valid.iter().copied());
        if valid.is_empty() {
            diagnostic.note(format!("{} has no fields", article(ty.word())))
        } else {
            diagnostic.note(format!("it has {}", list(&valid)))
        }
    }

    /// `limit[year]`, `ordinary[year, owner.filing]`
    fn lookup(&mut self, base: ExprId, keys: &[ExprId], loc: Loc) -> Check<(Op, Ty)> {
        let ExprKind::Name(name) = self.file.exprs[base].kind else {
            return Err(Diagnostic::error("param-lookup", "only a param can be looked up with `[…]`")
                .label(self.file.exprs[base].loc, "this is not a param name")
                .into());
        };
        let word = Word { text: name.0, loc: self.file.exprs[base].loc };
        let param = self.world.seek_param(self.home, word)?.ok_or_else(|| self.world.missing_param(self.home, word))?;
        let keys = self.children(keys)?;
        self.check_keys(param, word, &keys, loc)?;
        Ok((Op::Param(param, keys.into_iter().map(|(node, _)| node).collect()), self.param_ty(param)))
    }

    /// `catch-up`: a param looked up at the day the law runs.
    fn bare_param(&mut self, param: Id<Param>, word: Word<'s>) -> Check<(Op, Ty)> {
        let shape = Shape::of(&self.world.book.params[param].rows[0]);
        if shape != (Shape { timed: true, names: 0 }) {
            let what = if shape.timed { "a date and names" } else { "names" };
            return Err(Diagnostic::error("param-lookup", format!("`{}` needs keys", word.text))
                .label(word.loc, format!("`{}` has {what} to look up", word.text))
                .help(format!("write `{}[year]`, naming each key", word.text))
                .into());
        }
        if !Var::Date.provided_by(self.when) {
            return Err(Diagnostic::error(
                "law-variable",
                format!("`{}` is looked up at the law's date, which is not known here", word.text),
            )
            .label(word.loc, "write the day to look up: `[…]`")
            .into());
        }
        Ok((Op::Param(param, Box::new([])), self.param_ty(param)))
    }

    /// Every row of a param holds one type; `empty` rows adopt the amounts'.
    fn param_ty(&self, param: Id<Param>) -> Ty {
        let definition = &self.world.book.params[param];
        if let Some(unit) = definition.unit {
            return Ty::Amount(unit);
        }
        let rows = &definition.rows;
        let tys = rows.iter().filter_map(|row| row.value.ty());
        tys.reduce(|a, b| unify(a, b).unwrap_or(a)).unwrap_or(Ty::Empty)
    }

    fn check_keys(&self, param: Id<Param>, word: Word, keys: &[(NodeId, Ty)], loc: Loc) -> Check<()> {
        let shape = Shape::of(&self.world.book.params[param].rows[0]);
        if keys.len() != shape.keys() {
            let takes = count(shape.keys(), "key");
            return Err(Diagnostic::error(
                "param-lookup",
                format!("`{}` takes {takes}, not {}", word.text, keys.len()),
            )
            .label(loc, "wrong number of keys")
            .context(self.world.book.params[param].loc, "the param")
            .into());
        }
        for (at, &(node, ty)) in keys.iter().enumerate() {
            let timed = shape.timed && at == 0;
            let fine = if timed { matches!(ty, Ty::Num | Ty::Day) } else { matches!(ty, Ty::Name | Ty::Text) };
            if !fine {
                let wanted = if timed { "a year or a date" } else { "a name" };
                return Err(expected(wanted, ty, self.nodes[node].loc).into());
            }
        }
        Ok(())
    }

    // ─── Calls ──────────────────────────────────────────────────────────────

    /// A call to one of the functions: its arguments compiled, their number and types checked against what the
    /// function takes.
    fn call(&mut self, function: Word<'s>, args: &[ExprId], loc: Loc) -> Check<(Op, Ty)> {
        let Some(spec) = function_spec(function.text) else {
            return Err(self.unknown_function(function).into());
        };
        let typed = self.children(args)?;
        let locs = typed.iter().map(|&(node, _)| self.nodes[node].loc).collect();
        let call = Call { function, args, typed, locs };
        call.check_arity(spec, loc)?;
        let (func, ty) = self.signed(spec.signature, &call)?;
        // `open` and a purpose's total carry what they read in the function; no argument is evaluated for them.
        let nodes =
            if matches!(func, Func::Open(_) | Func::PurposeTotal { .. }) { Box::default() } else { call.nodes() };
        Ok((Op::Call(func, nodes), ty))
    }

    /// The function a call is, and the type it gives, once each argument is the type its signature wants.
    fn signed(&mut self, signature: Signature, call: &Call<'_, 's>) -> Check<(Func, Ty)> {
        let currency = self.owner_amount_ty();
        match signature {
            Signature::Total => Ok((self.total(&call.typed)?, currency)),
            Signature::Tally => Ok((self.tally_of(call)?, currency)),
            Signature::Open => Ok((self.open(call)?, currency)),
            Signature::Min => call.extremum(Func::Min),
            Signature::Max => call.extremum(Func::Max),
            Signature::Abs => call.abs(),
            Signature::Date => call.date(),
            Signature::StraightLine => call.straight_line(),
            Signature::Progressive => self.progressive(call),
            Signature::Value => self.value_call(call),
            Signature::Peak => self.in_window(call, Func::Peak),
            Signature::Low => self.in_window(call, Func::Low),
            Signature::Days => self.days(call),
        }
    }

    /// `tally(name[, year])`.
    fn tally_of(&mut self, call: &Call<'_, 's>) -> Check<Func> {
        if call.args.len() == 2 {
            call.want(1, "a year or a date", |ty| matches!(ty, Ty::Num | Ty::Day))?;
        }
        self.tally(call.args[0])
    }

    /// `open(^code)`: the code is read as written, not as a value.
    fn open(&mut self, call: &Call<'_, 's>) -> Check<Func> {
        let ExprKind::Code(code) = self.file.exprs[call.args[0]].kind else {
            return Err(expected("a source code such as `^rent`", call.ty(0), call.loc(0)).into());
        };
        call.want(0, "a source code", |ty| ty == Ty::Code)?;
        Ok(Func::Open(self.world.book.names.intern(code.name())))
    }

    /// `progressive(schedule, amount)`: the amount is in the commodity the schedule is, when that is known.
    fn progressive(&self, call: &Call<'_, 's>) -> Check<(Func, Ty)> {
        call.want(0, "a schedule", |ty| ty == Ty::Schedule)?;
        call.want(1, "an amount", |ty| matches!(ty, Ty::Amount(_) | Ty::Empty))?;
        let schedule_unit = match self.nodes[call.typed[0].0].op {
            Op::Const(Value::Schedule(schedule)) => Some(self.world.book.schedules[schedule].unit),
            _ => None,
        };
        if let Some(unit) = schedule_unit
            && let Ty::Amount(Dim::Of(input)) = call.ty(1)
            && unit != input
        {
            return Err(Diagnostic::error("unit-mismatch", "the amount and tax schedule use different commodities")
                .label(call.loc(1), "convert the amount to the schedule's commodity")
                .context(call.loc(0), "the schedule is declared in another commodity")
                .help("use `value(amount, UNIT)` to make the conversion explicit")
                .into());
        }
        Ok((Func::Progressive, schedule_unit.map_or(call.ty(1), |unit| Ty::Amount(Dim::Of(unit)))))
    }

    /// `value(amount, UNIT[, policy])`: the amount in another commodity.
    fn value_call(&self, call: &Call<'_, 's>) -> Check<(Func, Ty)> {
        call.want(0, "an amount", |ty| matches!(ty, Ty::Amount(_) | Ty::Empty))?;
        call.want(1, "a commodity", |ty| ty == Ty::Unit)?;
        if call.args.len() == 3 {
            call.want(2, "a rate policy name", |ty| ty == Ty::Name)?;
        }
        let target = match self.nodes[call.typed[1].0].op {
            Op::Const(Value::Unit(unit)) => Ty::Amount(Dim::Of(unit)),
            _ => Ty::AMOUNT,
        };
        Ok((Func::Value, target))
    }

    /// `peak(value, window)` and `low(value, window)`: the most or least an ordered value was in the window.
    fn in_window(&self, call: &Call<'_, 's>, func: Func) -> Check<(Func, Ty)> {
        call.want(0, "an ordered value", |ty| matches!(ty, Ty::Amount(_) | Ty::Num | Ty::Day | Ty::Span))?;
        self.want_window(call)?;
        Ok((func, call.ty(0)))
    }

    /// `days(condition, window)`: how many days the condition held in the window.
    fn days(&self, call: &Call<'_, 's>) -> Check<(Func, Ty)> {
        call.want(0, "a condition", |ty| ty == Ty::Bool)?;
        self.want_window(call)?;
        Ok((Func::Days, Ty::Num))
    }

    /// The second argument is `month`, `year` or `ever`.
    fn want_window(&self, call: &Call<'_, 's>) -> Check<()> {
        if self.keyword(call.args[1]).is_none() {
            return Err(self.keyword_error(call.typed[1].0, call.function.text, "`month`, `year` or `ever`").into());
        }
        Ok(())
    }

    /// `total(in|out, month|year|ever[, KIND])`
    fn total(&self, args: &[(NodeId, Ty)]) -> Check<Func> {
        let word = |at: usize| match self.nodes[args[at].0].op {
            Op::Const(Value::Name(sym)) => self.world.book.name(sym),
            _ => "",
        };
        if args.len() == 1 {
            let window = self.window_word(word(0), args[0].0, "total")?;
            let Some(Owner::Purpose(_)) = self.owner else {
                return Err(self
                    .keyword_error(args[0].0, "total", "`total(#PURPOSE, window)` outside a purpose law")
                    .into());
            };
            return Ok(Func::PurposeTotal { purpose: None, window });
        }
        if args.len() == 2 && args[0].1 == Ty::Purpose {
            let purpose = match self.nodes[args[0].0].op {
                Op::Const(Value::Purpose(purpose, None)) => purpose,
                _ => {
                    return Err(self.keyword_error(args[0].0, "total", "a declared purpose").into());
                }
            };
            let window = self.window_word(word(1), args[1].0, "total")?;
            return Ok(Func::PurposeTotal { purpose: Some(purpose), window });
        }
        let dir = match word(0) {
            "in" => Dir::In,
            "out" => Dir::Out,
            _ => {
                return Err(self.keyword_error(args[0].0, "total", "`in` or `out`").into());
            }
        };
        let window = match word(1) {
            "month" => Window::Month,
            "year" => Window::Year,
            "ever" => Window::Ever,
            _ => {
                return Err(self.keyword_error(args[1].0, "total", "`month`, `year` or `ever`").into());
            }
        };
        if let Some(&(node, ty)) = args.get(2)
            && ty != Ty::Kind
        {
            return Err(expected("a kind", ty, self.nodes[node].loc).into());
        }
        Ok(Func::Total(dir, window))
    }

    fn window_word(&self, text: &str, node: NodeId, function: &str) -> Check<Window> {
        match text {
            "month" => Ok(Window::Month),
            "year" => Ok(Window::Year),
            "ever" => Ok(Window::Ever),
            _ => Err(self.keyword_error(node, function, "`month`, `year` or `ever`").into()),
        }
    }

    fn keyword(&self, expr: ExprId) -> Option<Window> {
        let ExprKind::Name(name) = self.file.exprs[expr].kind else {
            return None;
        };
        match name.0 {
            "month" => Some(Window::Month),
            "year" => Some(Window::Year),
            "ever" => Some(Window::Ever),
            _ => None,
        }
    }

    fn keyword_error(&self, node: NodeId, function: &str, wanted: &str) -> Diagnostic {
        Diagnostic::error("call-keyword", format!("`{function}` needs {wanted} here"))
            .label(self.nodes[node].loc, format!("expected {wanted}"))
    }

    /// `tally(name)` or `tally(name, year)`: the name must be counted by some law.
    fn tally(&mut self, arg: ExprId) -> Check<Func> {
        let expr = &self.file.exprs[arg];
        let ExprKind::Name(name) = expr.kind else {
            return Err(expected("the name of a tally", Ty::Num, expr.loc).into());
        };
        if !self.world.tallies.contains(name.0) {
            let diagnostic = Diagnostic::error("unknown-tally", format!("no law counts `{}`", name.0))
                .label(expr.loc, "nothing is tallied under this name")
                .note("`tally(NAME)` reads what `count … as NAME` lines add up");
            return Err(suggest(diagnostic, expr.loc, name.0, self.world.tallies.iter().copied()).into());
        }
        Ok(Func::Tally(self.world.book.names.intern(name.0)))
    }

    fn unknown_function(&self, function: Word) -> Diagnostic {
        let functions = function_names();
        let diagnostic = Diagnostic::error("unknown-function", format!("there is no function `{}`", function.text))
            .label(function.loc, "not a function")
            .note(format!("the functions are {}", list(&functions)));
        suggest(diagnostic, function.loc, function.text, functions)
    }

    // ─── Operators ──────────────────────────────────────────────────────────

    fn unary(&mut self, op: UnOp, operand: ExprId) -> Check<(Op, Ty)> {
        let (node, ty) = self.child(operand)?;
        let loc = self.nodes[node].loc;
        match op {
            UnOp::Neg => {
                let ty = negate(ty).ok_or_else(|| expected("an amount or a number", ty, loc))?;
                Ok((Op::Neg(node), ty))
            }
            UnOp::Not if ty == Ty::Bool => Ok((Op::Not(node), Ty::Bool)),
            UnOp::Not => Err(expected("a condition", ty, loc).into()),
        }
    }

    fn binary(&mut self, op: BinOp, left: ExprId, right: ExprId) -> Check<(Op, Ty)> {
        let ((l, lt), (r, rt)) = (self.child(left)?, self.child(right)?);
        match binary(op, lt, rt) {
            Some(ty) => Ok((Op::Bin(op, l, r), ty)),
            None => {
                let locs = (self.nodes[l].loc, self.nodes[r].loc);
                Err(mismatch(op, (lt, locs.0), (rt, locs.1)).into())
            }
        }
    }

    /// A purpose can carry an identified object; a fraction can take a share
    /// of an amount. The AST uses `of` for both forms, so resolve by typed
    /// operands rather than reparsing the source text.
    fn of(&mut self, left: ExprId, right: ExprId) -> Check<(Op, Ty)> {
        let ((l, lt), (r, rt)) = (self.child(left)?, self.child(right)?);
        match (lt, rt) {
            (Ty::Purpose, Ty::Asset) => Ok((Op::Of(l, r), Ty::Purpose)),
            (Ty::Purpose, Ty::Place | Ty::Entity) => Ok((Op::Of(l, r), Ty::Purpose)),
            (Ty::Num, Ty::Amount(_)) | (Ty::Amount(Dim::Number), Ty::Amount(_)) => {
                let ty = binary(BinOp::Mul, lt, rt).expect("the number dimension multiplies an amount");
                Ok((Op::Bin(BinOp::Mul, l, r), ty))
            }
            _ => {
                let locs = (self.nodes[l].loc, self.nodes[r].loc);
                Err(Diagnostic::error("type-mismatch", "`of` needs a purpose and object, or a share and amount")
                    .label(locs.1, format!("this is {}", article(rt.word())))
                    .context(locs.0, format!("this is {}", article(lt.word())))
                    .note("write `repair of self` for an identified purpose, or `10% of amount` for a share")
                    .into())
            }
        }
    }

    /// A quantity priced in a unit per that quantity's unit.
    fn at(&mut self, quantity: ExprId, price: ExprId) -> Check<(Op, Ty)> {
        let ((q, qt), (p, pt)) = (self.child(quantity)?, self.child(price)?);
        match binary(BinOp::Mul, qt, pt) {
            Some(ty @ Ty::Amount(_)) => Ok((Op::At(q, p), ty)),
            _ => {
                let locs = (self.nodes[q].loc, self.nodes[p].loc);
                Err(Diagnostic::error("type-mismatch", "a quantity needs a price per its unit")
                    .label(locs.1, format!("this is {}", article(pt.word())))
                    .context(locs.0, format!("this is {}", article(qt.word())))
                    .note("for example, `44 MI @ 0.70 USD/MI` is an amount in USD")
                    .into())
            }
        }
    }

    fn is(&mut self, subject: ExprId, alternatives: &[ExprId]) -> Check<(Op, Ty)> {
        if let ExprKind::Field(receiver, field) = self.file.exprs[subject].kind
            && field.0 == "lives"
        {
            let (entity, found) = self.child(receiver)?;
            if found != Ty::Entity {
                return Err(expected("an entity", found, self.nodes[entity].loc).into());
            }
            let mut systems = Vec::with_capacity(alternatives.len());
            for &alternative in alternatives {
                let expr = &self.file.exprs[alternative];
                let ExprKind::Name(name) = expr.kind else {
                    return Err(Diagnostic::error("type-mismatch", "a residence can only be tested against a system")
                        .label(expr.loc, "name a system")
                        .into());
                };
                systems.push(self.world.system(Word::of(self.file, name.0))?);
            }
            return Ok((Op::Resides(entity, systems.into()), Ty::Bool));
        }
        let (node, ty) = self.child(subject)?;
        let alts = self.children(alternatives)?;
        for &(alt, alt_ty) in &alts {
            if !is_test(ty, alt_ty) {
                let loc = self.nodes[alt].loc;
                return Err(Diagnostic::error("type-mismatch", format!("cannot test {} against {}", article(ty.word()), article(alt_ty.word())))
                    .label(loc, format!("this is {}", article(alt_ty.word())))
                    .context(self.nodes[node].loc, format!("this is {}", article(ty.word())))
                    .note("`is` tests a place, entity or commodity against a kind, a place, an entity or a pattern, and a flow against a `#code`")
                    .into());
            }
        }
        Ok((Op::Is(node, alts.into_iter().map(|(alt, _)| alt).collect()), Ty::Bool))
    }

    fn conditional(&mut self, condition: ExprId, then: ExprId, otherwise: ExprId) -> Check<(Op, Ty)> {
        let ((c, ct), (t, tt), (o, ot)) = (self.child(condition)?, self.child(then)?, self.child(otherwise)?);
        if ct != Ty::Bool {
            return Err(expected("a condition", ct, self.nodes[c].loc).into());
        }
        let Some(ty) = unify(tt, ot) else {
            return Err(Diagnostic::error("type-mismatch", "the two branches of `if` must be of one type")
                .label(self.nodes[o].loc, format!("this is {}", article(ot.word())))
                .context(self.nodes[t].loc, format!("this is {}", article(tt.word())))
                .into());
        };
        Ok((Op::If(c, t, o), ty))
    }
}
