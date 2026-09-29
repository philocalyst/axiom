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
use axiom_core::{Diagnostic, Id, Loc, Sym};
use axiom_syntax::{
    self as ast, BinOp, Effect as WrittenEffect, ExprId, ExprKind, File, StepKind as WrittenStep, UnOp,
};

use super::types::{binary, expected, is_test, mismatch, negate, unify};
use super::vars::When;
use crate::book::{Entity, Param};
use crate::declare::World;
use crate::errors::{Word, article, count, list, suggest};
use crate::law::{
    Closing, Dir, Effect, Field, Func, Law, Node, NodeId, Op, Owner, Step, StepKind, Trigger, Ty, Value, Var, Window,
};
use crate::params::Shape;
use crate::scope::Home;
use crate::values::fits;

const FUNCTIONS: [&str; 8] = ["total", "tally", "min", "max", "abs", "progressive", "value", "date"];

/// The functions whose arguments are all of one type each: what they must be, and what they give.
const FIXED: [(&str, &[Ty], Func, Ty); 3] = [
    ("progressive", &[Ty::Schedule, Ty::Amount], Func::Progressive, Ty::Amount),
    ("value", &[Ty::Amount, Ty::Unit], Func::Value, Ty::Amount),
    ("date", &[Ty::Num, Ty::Num, Ty::Num], Func::Date, Ty::Day),
];

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
}

/// Why a node has no type.
enum Bad {
    /// A diagnostic for it.
    Report(Diagnostic),
    /// A child was already reported.
    Cascade,
}

impl From<Diagnostic> for Bad {
    fn from(diagnostic: Diagnostic) -> Bad {
        Bad::Report(diagnostic)
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
        subject: site.subject,
        law_name,
        first: 0,
        base: 0,
        nodes: Vec::new(),
        poisoned: Vec::new(),
        roles: Vec::new(),
        locals: Vec::new(),
        when: When::of(&law.trigger),
        failed: false,
    };
    compiler.law(site, law)
}

struct Compiler<'w, 'a, 's> {
    world: &'w mut World<'s>,
    diags: &'w mut Vec<Diagnostic>,
    file: &'a File<'s>,
    home: Home,
    subject: Ty,
    law_name: Sym,
    /// The source index of the first node of the run being compiled, and the
    /// index the first node of it got.
    first: usize,
    base: usize,
    nodes: Vec<Node>,
    poisoned: Vec<bool>,
    /// The role of each node of the run being compiled.
    roles: Vec<Role>,
    /// `let` bindings in scope, and the node holding each value.
    locals: Vec<(&'s str, NodeId)>,
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
        Some(Law {
            name: self.law_name,
            doc: law.doc.map(|doc| self.world.book.names.intern(doc.0)),
            owner: site.owner,
            system: if let Home::System(system) = site.home { Some(system) } else { None },
            trigger: trigger?,
            steps: steps.into(),
            nodes: std::mem::take(&mut self.nodes).into(),
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
            ast::Trigger::Each(period) => Trigger::Each(period, None),
            ast::Trigger::Closing { month, day } => Trigger::Each(ast::Period::Year, Some(Closing { month, day })),
            ast::Trigger::Always => Trigger::Always,
            ast::Trigger::By(root) => Trigger::By(self.expression(root, Ty::Day)?),
        })
    }

    fn step(&mut self, step: &ast::Step<'s>) -> Option<Step> {
        let kind = match &step.kind {
            WrittenStep::When(root) => StepKind::When(self.condition(*root)?),
            WrittenStep::Let(name, root) => {
                // Bound even if it failed, so its uses do not report it again.
                let bound = self.compile(*root);
                self.locals.push((name.0, bound));
                StepKind::Let((!self.poisoned[bound.index()]).then_some(bound)?)
            }
            WrittenStep::Require { cond, otherwise, message, warn } => {
                let cond = self.condition(*cond)?;
                let otherwise = match otherwise {
                    Some(effect) => Some(self.effect(effect)?),
                    None => None,
                };
                let message = message.map(|text| self.world.book.names.intern(text.0));
                StepKind::Require { cond, otherwise, message, warn: *warn }
            }
            WrittenStep::Effect(effect) => StepKind::Effect(self.effect(effect)?),
        };
        Some(Step { loc: step.loc, kind })
    }

    fn effect(&mut self, effect: &WrittenEffect<'s>) -> Option<Effect> {
        match effect {
            WrittenEffect::Owe { amount, to, due, name } => {
                let amount = self.expression(*amount, Ty::Amount)?;
                let to = self.owed_to(to.0);
                let due = match due {
                    Some(due) => Some(self.expression(*due, Ty::Day)?),
                    None => None,
                };
                let name = name.map_or(self.law_name, |name| self.world.book.names.intern(name.0));
                Some(Effect::Owe { amount, to: to?, due, name })
            }
            WrittenEffect::Count { amount, name } => {
                let amount = self.expression(*amount, Ty::Amount)?;
                Some(Effect::Count { amount, name: self.world.book.names.intern(name.0) })
            }
        }
    }

    fn owed_to(&mut self, name: &'s str) -> Option<Id<Entity>> {
        let entity = self.world.entity(self.home, Word { text: name, loc: self.file.loc(name) });
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
        (self.first, self.base) = (self.file.exprs[root].first.index(), self.nodes.len());
        self.roles = self.roles_of(subtree);
        for (offset, expr) in subtree.iter().enumerate() {
            self.node(self.first + offset, expr);
        }
        NodeId(self.nodes.len() as u32 - 1)
    }

    /// Which nodes are keywords, param names or `is` alternatives: parents
    /// decide, and children come first, so it is settled before compiling.
    fn roles_of(&self, subtree: &[ast::Expr<'s>]) -> Vec<Role> {
        let mut roles = vec![Role::Normal; subtree.len()];
        let mut mark = |id: ExprId, role: Role| {
            if let Some(slot) = id.index().checked_sub(self.first).and_then(|at| roles.get_mut(at)) {
                *slot = role;
            }
        };
        for expr in subtree {
            match expr.kind {
                ExprKind::Call(name, args) if name.0 == "total" => {
                    self.file[args].iter().take(2).for_each(|&arg| mark(arg, Role::Keyword));
                    self.file[args].iter().skip(2).for_each(|&arg| mark(arg, Role::Pattern));
                }
                ExprKind::Call(name, args) if name.0 == "tally" => {
                    self.file[args].iter().for_each(|&arg| mark(arg, Role::Keyword))
                }
                // Arguments of a function that does not exist mean nothing; the
                // function is the mistake worth reporting.
                ExprKind::Call(name, args) if !FUNCTIONS.contains(&name.0) => {
                    self.file[args].iter().for_each(|&arg| mark(arg, Role::Keyword))
                }
                ExprKind::Index(base, _) => mark(base, Role::ParamBase),
                ExprKind::Is(_, alternatives) => {
                    self.file[alternatives].iter().for_each(|&alt| mark(alt, Role::Pattern))
                }
                _ => {}
            }
        }
        roles
    }

    /// The node of `root`, unless it or something beneath it failed.
    fn value(&mut self, root: ExprId) -> Option<NodeId> {
        let node = self.compile(root);
        (!self.poisoned[node.index()]).then_some(node)
    }

    /// A root that must have the type `want`: an amount or a date.
    fn expression(&mut self, root: ExprId, want: Ty) -> Option<NodeId> {
        let node = self.value(root)?;
        let found = &self.nodes[node.index()];
        if fits(want, found.ty) {
            return Some(node);
        }
        let diagnostic = expected(&article(want.word()), found.ty, found.loc);
        self.report(diagnostic);
        None
    }

    fn condition(&mut self, root: ExprId) -> Option<NodeId> {
        let node = self.value(root)?;
        let found = &self.nodes[node.index()];
        if found.ty == Ty::Bool {
            return Some(node);
        }
        let diagnostic = expected("a condition", found.ty, found.loc)
            .note("a condition compares things, as in `amount <= 500 USD`, or tests them, as in `to is expenses/food`");
        self.report(diagnostic);
        None
    }

    /// The node an expression of the run being compiled became.
    fn node_id(&self, id: ExprId) -> NodeId {
        NodeId((self.base + id.index() - self.first) as u32)
    }

    fn node(&mut self, at: usize, expr: &ast::Expr<'s>) {
        let first = self.node_id(expr.first);
        let (op, ty, ok) = match self.check(at, expr) {
            Ok((op, ty)) => (op, ty, true),
            Err(Bad::Report(diagnostic)) => {
                self.report(diagnostic);
                (Op::Const(Value::Empty), Ty::Empty, false)
            }
            Err(Bad::Cascade) => (Op::Const(Value::Empty), Ty::Empty, false),
        };
        self.nodes.push(Node { op, ty, loc: expr.loc, first });
        self.poisoned.push(!ok);
    }

    /// The typed child, or the reason it has none.
    fn child(&self, id: ExprId) -> Check<(NodeId, Ty)> {
        let node = self.node_id(id);
        if self.poisoned[node.index()] { Err(Bad::Cascade) } else { Ok((node, self.nodes[node.index()].ty)) }
    }

    fn children(&self, ids: &[ExprId]) -> Check<Vec<(NodeId, Ty)>> {
        ids.iter().map(|&id| self.child(id)).collect()
    }

    fn check(&mut self, at: usize, expr: &ast::Expr<'s>) -> Check<(Op, Ty)> {
        if let Some((value, ty)) = self.world.literal(self.file, expr)? {
            return Ok((Op::Const(value), ty));
        }
        let file = self.file;
        match expr.kind {
            ExprKind::Name(name) => self.name(at, Word { text: name.0, loc: expr.loc }),
            ExprKind::Field(receiver, field) => self.field(receiver, Word { text: field.0, loc: file.loc(field.0) }),
            ExprKind::Index(base, keys) => self.lookup(base, &file[keys], expr.loc),
            ExprKind::Call(function, args) => {
                self.call(Word { text: function.0, loc: file.loc(function.0) }, &file[args], expr.loc)
            }
            ExprKind::Unary(op, operand) => self.unary(op, operand),
            ExprKind::Binary(op, left, right) => self.binary(op, left, right),
            ExprKind::Is(subject, alternatives) => self.is(subject, &file[alternatives]),
            ExprKind::If(condition, then, otherwise) => self.conditional(condition, then, otherwise),
            ExprKind::Schedule(_) => Err(Diagnostic::error("schedule-position", "a schedule belongs in a param")
                .label(expr.loc, "write it as a row of a `param`, and look it up here")
                .into()),
            _ => unreachable!("literal() handles every literal kind"),
        }
    }

    // ─── Names ──────────────────────────────────────────────────────────────

    fn name(&mut self, at: usize, word: Word<'s>) -> Check<(Op, Ty)> {
        match self.roles[at - self.first] {
            Role::Keyword | Role::ParamBase => {
                let sym = self.world.book.names.intern(word.text);
                Ok((Op::Const(Value::Name(sym)), Ty::Name))
            }
            Role::Pattern => self.constant(word),
            Role::Normal => {
                if let Some(&(_, bound)) = self.locals.iter().rev().find(|(local, _)| *local == word.text) {
                    return self.local(bound);
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
        if self.poisoned[bound.index()] {
            return Err(Bad::Cascade);
        }
        Ok((Op::Local(bound), self.nodes[bound.index()].ty))
    }

    fn variable(&self, var: Var, word: Word) -> Check<(Op, Ty)> {
        if var.provided_by(self.when) {
            return Ok((Op::Var(var), var.ty(self.subject)));
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
        let (names, lookup) = (&self.world.book.names, &self.world.book.lookup);
        let scope = self.world.scopes.of(self.home);
        known.extend(lookup.params.names.keys(names));
        known.extend(lookup.kinds.names.keys(names).filter(|key| {
            lookup.kinds.names.candidates(names, key).iter().any(|&id| scope.sees(lookup.kinds.home(id)))
        }));
        known.extend(lookup.entities.names.keys(names));
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
            (Ty::Place, "balance") => Some((Field::Balance, Ty::Amount)),
            (Ty::Place, "basis") => Some((Field::Basis, Ty::Amount)),
            (Ty::Amount | Ty::Empty, "unit") => Some((Field::Unit, Ty::Unit)),
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
        if let Some(has) = self.world.props.get(ty, sym) {
            return Ok((Op::Field(node, Field::Prop(sym)), has.ty));
        }
        Err(self.unknown_field(ty, field, receiver).into())
    }

    fn unknown_field(&self, ty: Ty, field: Word<'s>, receiver: ExprId) -> Diagnostic {
        let mut valid: Vec<&str> = match ty {
            Ty::Place => vec!["balance", "basis", "owner", "kind"],
            Ty::Entity => vec!["owner", "kind", "age"],
            Ty::Unit => vec!["kind"],
            Ty::Amount | Ty::Empty => vec!["unit"],
            Ty::Day => vec!["year", "month"],
            _ => Vec::new(),
        };
        valid.extend(self.world.props.names(ty).map(|sym| self.world.book.name(sym)));
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
        let rows = &self.world.book.params[param].rows;
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
                return Err(expected(wanted, ty, self.nodes[node.index()].loc).into());
            }
        }
        Ok(())
    }

    // ─── Calls ──────────────────────────────────────────────────────────────

    fn call(&mut self, function: Word<'s>, args: &[ExprId], loc: Loc) -> Check<(Op, Ty)> {
        let typed = self.children(args)?;
        let arity = |low: usize, high: usize| -> Check<()> {
            if (low..=high).contains(&typed.len()) {
                return Ok(());
            }
            let takes = if low == high { low.to_string() } else { format!("{low} or {high}") };
            Err(Diagnostic::error(
                "call-arity",
                format!("`{}` takes {takes} arguments, but {} were given", function.text, typed.len()),
            )
            .label(loc, "wrong number of arguments")
            .into())
        };
        let nodes: Box<[NodeId]> = typed.iter().map(|&(node, _)| node).collect();
        let ty_at = |at: usize| typed.get(at).map_or(Ty::Empty, |&(_, ty)| ty);
        let arg_loc = |at: usize| self.nodes[typed[at].0.index()].loc;
        let (func, ty) = match function.text {
            "total" => {
                arity(2, 3)?;
                (self.total(&typed)?, Ty::Amount)
            }
            "tally" => {
                arity(1, 1)?;
                (self.tally(args[0])?, Ty::Amount)
            }
            "min" | "max" => {
                arity(2, 2)?;
                let shared = unify(ty_at(0), ty_at(1)).filter(|&shared| binary(BinOp::Lt, shared, shared).is_some());
                let shared =
                    shared.ok_or_else(|| mismatch(BinOp::Lt, (ty_at(0), arg_loc(0)), (ty_at(1), arg_loc(1))))?;
                (if function.text == "min" { Func::Min } else { Func::Max }, shared)
            }
            "abs" => {
                arity(1, 1)?;
                let ty = negate(ty_at(0)).ok_or_else(|| expected("an amount or a number", ty_at(0), arg_loc(0)))?;
                (Func::Abs, ty)
            }
            name => {
                let Some(&(_, wants, func, ty)) = FIXED.iter().find(|entry| entry.0 == name) else {
                    return Err(self.unknown_function(function).into());
                };
                arity(wants.len(), wants.len())?;
                if let Some((at, &want)) = wants.iter().enumerate().find(|&(at, &want)| !fits(want, ty_at(at))) {
                    let phrase = if want == Ty::Unit { "a commodity".into() } else { article(want.word()) };
                    return Err(expected(&phrase, ty_at(at), arg_loc(at)).into());
                }
                (func, ty)
            }
        };
        Ok((Op::Call(func, nodes), ty))
    }

    /// `total(in|out, month|year|ever[, KIND])`
    fn total(&self, args: &[(NodeId, Ty)]) -> Check<Func> {
        let word = |at: usize| match self.nodes[args[at].0.index()].op {
            Op::Const(Value::Name(sym)) => self.world.book.name(sym),
            _ => "",
        };
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
            return Err(expected("a kind", ty, self.nodes[node.index()].loc).into());
        }
        Ok(Func::Total(dir, window))
    }

    fn keyword_error(&self, node: NodeId, function: &str, wanted: &str) -> Diagnostic {
        Diagnostic::error("call-keyword", format!("`{function}` needs {wanted} here"))
            .label(self.nodes[node.index()].loc, format!("expected {wanted}"))
    }

    /// `tally(name)`: the name must be counted by some law.
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
        let diagnostic = Diagnostic::error("unknown-function", format!("there is no function `{}`", function.text))
            .label(function.loc, "not a function")
            .note(format!("the functions are {}", list(&FUNCTIONS)));
        suggest(diagnostic, function.loc, function.text, FUNCTIONS)
    }

    // ─── Operators ──────────────────────────────────────────────────────────

    fn unary(&mut self, op: UnOp, operand: ExprId) -> Check<(Op, Ty)> {
        let (node, ty) = self.child(operand)?;
        let loc = self.nodes[node.index()].loc;
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
                let locs = (self.nodes[l.index()].loc, self.nodes[r.index()].loc);
                Err(mismatch(op, (lt, locs.0), (rt, locs.1)).into())
            }
        }
    }

    fn is(&mut self, subject: ExprId, alternatives: &[ExprId]) -> Check<(Op, Ty)> {
        let (node, ty) = self.child(subject)?;
        let alts = self.children(alternatives)?;
        for &(alt, alt_ty) in &alts {
            if !is_test(ty, alt_ty) {
                let loc = self.nodes[alt.index()].loc;
                return Err(Diagnostic::error("type-mismatch", format!("cannot test {} against {}", article(ty.word()), article(alt_ty.word())))
                    .label(loc, format!("this is {}", article(alt_ty.word())))
                    .context(self.nodes[node.index()].loc, format!("this is {}", article(ty.word())))
                    .note("`is` tests a place, entity or commodity against a kind, a place, an entity or a pattern, and a flow against a `#code`")
                    .into());
            }
        }
        Ok((Op::Is(node, alts.into_iter().map(|(alt, _)| alt).collect()), Ty::Bool))
    }

    fn conditional(&mut self, condition: ExprId, then: ExprId, otherwise: ExprId) -> Check<(Op, Ty)> {
        let ((c, ct), (t, tt), (o, ot)) = (self.child(condition)?, self.child(then)?, self.child(otherwise)?);
        if ct != Ty::Bool {
            return Err(expected("a condition", ct, self.nodes[c.index()].loc).into());
        }
        let Some(ty) = unify(tt, ot) else {
            return Err(Diagnostic::error("type-mismatch", "the two branches of `if` must be of one type")
                .label(self.nodes[o.index()].loc, format!("this is {}", article(ot.word())))
                .context(self.nodes[t.index()].loc, format!("this is {}", article(tt.word())))
                .into());
        };
        Ok((Op::If(c, t, o), ty))
    }
}
