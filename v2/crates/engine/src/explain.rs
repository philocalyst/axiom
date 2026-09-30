//! What the fold says when something is wrong: a law that fails, an assertion
//! that does not hold, lots that cannot be told apart, a sale of more than was
//! held.
//!
//! A failed law is read in three layers, most human first: the fact (what was
//! counted, against what, over by how much), the flows that built it, and the
//! rule's own line with the two values it compared. The law's doc comment is
//! split in two: its first paragraph is a note, and a paragraph that starts
//! `To fix:` is the help.
//!
//! A failed assertion reasons like a bookkeeper: it asks whether a flow was
//! entered backwards, two digits were swapped, the sign is wrong, the wrong
//! commodity was asserted, or a flow is missing, and puts the likeliest in a
//! note before it offers to accept the gap.

use axiom_core::{Day, Diagnostic, Disposition, Id, Loc, Qty, Severity, Sym, calendar};
use axiom_model::{
    Amount, Assert, BinOp, Book, Commodity, Dir, Effect as LawEffect, End, Fault, Flow, Law, NodeId, Op, Param, Place,
    StepKind, Subject, System, Trigger, Value, Waive, Window,
};

use crate::bridge;
use crate::calc::Calc;
use crate::eval::{Context, compared};
use crate::events::Events;
use crate::facts::{Follows, LawFacts, Reads};
use crate::lots::Candidate;
use crate::motion::Motion;
use crate::show;
use crate::{Cause, Effect, Owed, Parcel, Waiver};

/// The most flows an assertion's explanation draws.
const SHOWN: usize = 8;

/// A law's evaluation, frozen at the moment something went wrong.
pub(crate) struct Frame<'a, 's> {
    pub book: &'a Book<'s>,
    pub law: &'a Law,
    /// What is true of the law whatever runs it: what each step reads and compares.
    pub facts: &'a LawFacts,
    pub ctx: &'a Context<'a>,
    /// Every node's value from the run that just ended.
    pub values: &'a [Value],
    /// What laws recorded so far: which flows counted into a tally.
    pub effects: &'a [Effect],
}

impl Frame<'_, '_> {
    fn money(&self, amount: Amount) -> String {
        self.book.show(amount).to_string()
    }

    /// The law's own words for what it is and what to do, from its doc comment:
    /// the first paragraph, and the paragraph that starts `To fix:`.
    fn doc(&self) -> (Option<String>, Option<String>) {
        let Some(doc) = self.law.doc else { return (None, None) };
        let lines: Vec<&str> = self.book.name(doc).lines().map(|l| l.trim().trim_start_matches("///").trim()).collect();
        let (mut what, mut fix) = (None, None);
        for paragraph in lines.split(|line| line.is_empty()).filter(|p| !p.is_empty()).map(|p| p.join(" ")) {
            match paragraph.strip_prefix("To fix:") {
                Some(fixing) => fix = fix.or(Some(fixing.trim().to_owned())),
                None => what = what.or(Some(paragraph)),
            }
        }
        (what, fix)
    }

    /// The primary label: the flow that fired the law, or the law itself for one
    /// fired by time.
    fn cause_label(&self) -> (Loc, String) {
        let (book, ctx) = (self.book, self.ctx);
        let Some(motion) = ctx.motion else {
            let what = format!("checked for {} on {}", show::subject(book, ctx.subject), ctx.day);
            return (self.law.loc, what);
        };
        let moving = ctx.amount.unwrap_or(motion.out);
        let (from, to) = (show::place(book, motion.from), show::place(book, motion.to));
        let what = match (self.law.trigger, ctx.realized) {
            (Trigger::In, _) => format!("{} into {to}", self.money(moving)),
            (Trigger::Out, _) => format!("{} out of {from}", self.money(moving)),
            (Trigger::Gain, Some(r)) => {
                format!("a gain of {} on {}", self.money(Amount::new(r.gain, book.base)), self.money(moving))
            }
            (Trigger::Spend, _) => {
                format!("{} of restricted money leaving {from}", self.money(moving))
            }
            _ => format!("{} from {from} to {to}", self.money(motion.out)),
        };
        (motion.loc, format!("this flow: {what}"))
    }

    /// The comparison a step failed on, when it compares amounts.
    fn comparison(&self, cond: NodeId) -> Option<Comparison> {
        let (cmp, counted, limit) = compared(self.law, self.values, cond)?;
        let upper = matches!(cmp, BinOp::Lt | BinOp::Le);
        let counted_in_limit = Calc { book: self.book, day: self.ctx.day }.convert(counted, limit.unit).ok()?;
        let off = if upper { counted_in_limit.qty - limit.qty } else { limit.qty - counted_in_limit.qty };
        Some(Comparison { counted, limit, upper, off: Amount::new(off, limit.unit) })
    }

    /// The two sides of the failing comparison with their values; for any
    /// other condition, the facts it read (see [`Frame::atoms`]).
    fn operands(&self, cond: NodeId) -> Vec<(Loc, String)> {
        let nodes = &self.law.nodes;
        let at = match nodes[cond.index()].op {
            Op::Bin(BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge | BinOp::Eq | BinOp::Ne, l, r) => {
                vec![l.index(), r.index()]
            }
            _ => self.atoms(cond),
        };
        let unit = at.iter().find_map(|&i| if let Value::Amount(a) = self.values[i] { Some(a.unit) } else { None });
        let mut shown: Vec<(Loc, String)> = Vec::new();
        for i in at.into_iter().filter(|&i| !matches!(nodes[i].op, Op::Const(_))) {
            let text = match (self.values[i], unit) {
                (Value::Empty, Some(unit)) => self.money(Amount::zero(unit)),
                (value, _) => show::value(self.book, self.ctx.day, value),
            };
            if !shown.iter().any(|(loc, _)| *loc == nodes[i].loc) {
                shown.push((nodes[i].loc, text));
            }
        }
        shown
    }

    /// Whose property node `at` read: the entity or place a field was taken from.
    pub fn holder(&self, at: usize) -> Option<Subject> {
        let Op::Field(receiver, _) = self.law.nodes[at].op else { return None };
        match self.values[receiver.index()] {
            Value::Entity(entity) => Some(Subject::Entity(entity)),
            Value::Place(place) => Some(Subject::Place(place)),
            _ => None,
        }
    }

    /// The facts a condition read, outermost first found and in source order:
    /// a variable, a `let`, a field, a param or a call, each once as a whole.
    /// `owner.age` is one fact, not `owner` and its age; the booleans that
    /// combine facts, and the constants, say nothing the line does not.
    fn atoms(&self, cond: NodeId) -> Vec<usize> {
        let nodes = &self.law.nodes;
        let (mut atoms, first) = (Vec::new(), nodes[cond.index()].first.index());
        let mut at = cond.index() + 1;
        while at > first {
            at -= 1;
            if matches!(nodes[at].op, Op::Var(_) | Op::Local(_) | Op::Field(..) | Op::Param(..) | Op::Call(..)) {
                atoms.push(at);
                // Skip the atom's own subtree: a field's receiver, a call's arguments.
                at = nodes[at].first.index();
            }
        }
        atoms.reverse();
        atoms
    }

    /// Up to three flows before this one that built what a limit counted. A
    /// window read as it opened had no flow to fire it: the latest to reach it.
    fn contributors(&self, reads: Reads) -> Vec<Id<Flow>> {
        let (book, ctx) = (self.book, self.ctx);
        let current = match ctx.cause {
            Cause::Flow(id) => Some(id),
            _ if ctx.checking => None,
            _ => return Vec::new(),
        };
        let window = reads.window(ctx);
        let mut found: Vec<Id<Flow>> = match reads {
            Reads::Tally(name) => {
                let of_name = self.effects.iter().rev().filter(|e| e.owner == ctx.owner && e.name == name);
                let counted = of_name.filter(|e| window.contains(e.day)).filter_map(|e| match e.cause {
                    Cause::Flow(id) if Some(id) != current => Some(id),
                    _ => None,
                });
                counted.take(3).collect()
            }
            Reads::Total(dir, _) => {
                let Subject::Place(place) = ctx.subject else { return Vec::new() };
                let flows = &book.touching[place];
                let before = match current {
                    Some(current) => flows.partition_point(|&id| id < current),
                    None => flows.partition_point(|&id| book.flows[id].day <= ctx.day),
                };
                let crosses = |flow: &Flow| {
                    let (here, there) = if dir == Dir::In { (flow.to, flow.from) } else { (flow.from, flow.to) };
                    book.places.covers(place, here) && !book.places.covers(place, there)
                };
                let moves = flows[..before].iter().rev().take(400).filter(|&&id| {
                    let flow = &book.flows[id];
                    crosses(flow) && flow.recognized.overlaps(window)
                });
                moves.copied().take(3).collect()
            }
        };
        found.reverse();
        found
    }

    /// The flow that fired the law, the flows that built what its condition
    /// counted (when it reads a total or tally), and the values it compared.
    fn locate(&self, d: Diagnostic, cond: NodeId, reads: Option<Reads>) -> Diagnostic {
        let (loc, text) = self.cause_label();
        let d = self.contributions(d.label(loc, text), reads);
        self.operands(cond).into_iter().fold(d, |d, (loc, value)| d.context(loc, value))
    }

    /// The flows that built the count, as labels.
    fn contributions(&self, mut d: Diagnostic, reads: Option<Reads>) -> Diagnostic {
        for id in reads.map(|r| self.contributors(r)).unwrap_or_default() {
            let flow = &self.book.flows[id];
            let moved = if matches!(reads, Some(Reads::Total(Dir::Out, _))) { flow.out } else { flow.arrive };
            let text = format!(
                "{}: {} from {} to {}",
                flow.day,
                self.money(moved),
                show::place(self.book, flow.from),
                show::place(self.book, flow.to)
            );
            d = d.context(flow.loc, text);
        }
        d
    }
}

/// What a limit compared: `counted` against `limit`, and by how much it is off.
struct Comparison {
    counted: Amount,
    limit: Amount,
    /// `counted <= limit` (a cap), else `counted >= limit` (a floor).
    upper: bool,
    off: Amount,
}

/// A `require` or `warn` whose condition is false. The headline is the
/// accounting fact; the labels are the flow that crossed the line, the flows
/// that built the count, and the two values compared.
pub(crate) fn broken(f: &Frame, step: usize, warn: bool, waiver: Option<Waiver>) -> Diagnostic {
    let StepKind::Require { cond, message, .. } = f.law.steps[step].kind else {
        unreachable!("only a require or warn step breaks")
    };
    let (what, fix) = f.doc();
    let (bound, reads) = (f.comparison(cond), f.facts.steps[step].reads);
    let headline = match (message.map(|text| f.book.name(text).to_owned()), &bound) {
        (message, Some(bound)) => {
            let lead = message.unwrap_or_else(|| show::subject(f.book, f.ctx.subject).to_owned());
            format!("{lead}: {}", fact(f, bound, reads))
        }
        (Some(message), None) => message,
        (None, None) => what.clone().unwrap_or_else(|| {
            format!("{} is not satisfied by {}", f.book.name(f.law.name), show::subject(f.book, f.ctx.subject))
        }),
    };
    let severity = if warn { Severity::Warning } else { Severity::Error };
    let d = f.locate(Diagnostic::new(severity, f.book.name(f.law.name).to_owned(), headline), cond, reads);
    let d = what.filter(|_| message.is_some() || bound.is_some()).into_iter().fold(d, Diagnostic::note);
    let d = [suggestion(f, step, cond), fix].into_iter().flatten().fold(d, Diagnostic::help);
    accepted(d, f, waiver)
}

/// "27,000.00 USD in 2026 against a limit of 24,500.00 USD, over by 2,500.00 USD"
fn fact(f: &Frame, bound: &Comparison, reads: Option<Reads>) -> String {
    let when = match reads {
        None => String::new(),
        Some(Reads::Total(_, Window::Ever)) => " in total".to_owned(),
        Some(reads) => {
            calendar::Window::exactly(reads.window(f.ctx)).map_or(String::new(), |window| format!(" in {window}"))
        }
    };
    let (bar, past) = if bound.upper { ("limit", "over") } else { ("minimum", "short") };
    let over = if bound.off.qty > Qty::ZERO { format!(", {past} by {}", f.money(bound.off)) } else { String::new() };
    format!("{}{when} against a {bar} of {}{over}", f.money(bound.counted), f.money(bound.limit))
}

/// Marks what was accepted: a `!` names itself and its reason; `relaxed` says why an error is a warning.
fn accepted(d: Diagnostic, f: &Frame, waiver: Option<Waiver>) -> Diagnostic {
    match waiver {
        Some(Waiver::Marked(waive)) => {
            let d = d.context(waive.loc, "waived here").relaxed().disposed(Disposition::Waived);
            match waive.reason {
                Some(reason) => d.note(format!("waived: {}", f.book.name(reason))),
                None => d,
            }
        }
        Some(Waiver::Relaxed) => {
            d.note("shown as a warning because the book is `relaxed`").relaxed().disposed(Disposition::Waived)
        }
        None => d,
    }
}

/// A `require … else owe …` that failed: not an error, a price. It says what
/// is owed, to whom and by when, and why.
pub(crate) fn priced(
    f: &Frame,
    step: usize,
    (name, amount, owed): (Sym, Amount, Owed),
    waive: Option<Waive>,
) -> Diagnostic {
    let StepKind::Require { cond, .. } = f.law.steps[step].kind else { unreachable!("only a require prices") };
    let who = f.book.name(f.book.entities[owed.to].path);
    let (amount, name) = (f.money(amount), f.book.name(name));
    let headline = match waive {
        Some(_) => format!("waived: {amount} would be owed to {who} as {name}"),
        None => format!("{amount} owed to {who} as {name}, due {}", owed.due),
    };
    let d = Diagnostic::info(f.book.name(f.law.name).to_owned(), headline).disposed(Disposition::Priced);
    let (what, fix) = f.doc();
    let d = what.into_iter().fold(f.locate(d, cond, None), Diagnostic::note);
    match waive {
        Some(waive) => accepted(d, f, Some(Waiver::Marked(waive))),
        None => match fix {
            Some(fix) => d.help(fix),
            None => d.help("if an exception applies, keep the flow and mark it `!` with the reason"),
        },
    }
}

/// A fault reached a step: the data the law needs does not exist. A property
/// never set is shown where its holder is declared, since that is where the
/// line that fixes it goes; the law's line that read it is context.
pub(crate) fn faulted(f: &Frame, fault: Fault, origin: Option<usize>, holder: Option<Subject>) -> Diagnostic {
    let (book, law) = (f.book, f.book.name(f.law.name));
    let (what, help) = show::fault(book, fault, f.ctx.day);
    let code = match fault {
        Fault::NoPrice { .. } => "no-price",
        Fault::Unset(_) => "unset-property",
        Fault::NoRow(_) => "no-param-row",
        Fault::DivideByZero | Fault::Overflow => "arithmetic",
    };
    let (cause, text) = f.cause_label();
    let read = origin.map(|at| (f.law.nodes[at].loc, what.clone()));
    if let (Fault::Unset(name), Some(holder)) = (fault, holder) {
        let (name, thing) = (book.name(name), show::subject(book, holder));
        let declared = match holder {
            Subject::Place(place) => book.places[place].loc,
            Subject::Entity(entity) => book.entities[entity].loc,
            Subject::Asset(asset) => Some(book.assets[asset].loc),
        };
        let d = Diagnostic::error(code, format!("`{name}` is not set on `{thing}`, so `{law}` cannot be checked"));
        let d = match declared {
            Some(loc) => d.label(loc, format!("`{thing}` has no `{name}`")).context(cause, text),
            None => d.label(cause, text),
        };
        let d = read.into_iter().fold(d, |d, (loc, _)| d.context(loc, format!("`{law}` reads it here")));
        return d.help(format!("add a `{name} …` line under the declaration of `{thing}`"));
    }
    if let Fault::NoRow(param) = fault
        && let Some(system) = book.params[param].system
    {
        return missing_figures(f, param, system, (cause, text));
    }
    let d = Diagnostic::error(code, format!("cannot check `{law}`: {what}")).label(cause, text);
    let d = read.into_iter().fold(d, |d, (loc, what)| d.context(loc, what));
    help.into_iter().fold(d, Diagnostic::help)
}

/// A system's table has no row for the year a law asked about: the journal
/// is older than the figures the system ships. Every law that needs a figure
/// of that year is skipped, and this is said once.
fn missing_figures(f: &Frame, param: Id<Param>, system: Id<System>, (cause, text): (Loc, String)) -> Diagnostic {
    let (book, year) = (f.book, f.ctx.over.first().year());
    let (param, system) = (&book.params[param], book.name(book.systems[system].path));
    let first = param.rows.iter().filter_map(|row| row.since).min().map(|day| day.year());
    let starts = first.map_or(String::new(), |first| format!(": its figures start in {first}"));
    let d = Diagnostic::error("no-param-row", format!("`{system}` has no figures for {year}{starts}"))
        .label(cause, text)
        .context(param.loc, format!("`{}` has no row for {year}", book.name(param.name)));
    d.note(format!("every law that needs a {year} figure is skipped, and this is reported once"))
        .help(match first {
            Some(first) => format!(
                "start the journal in {first}, or add {year} rows: copy the system's file into `systems/` and write them there"
            ),
            None => "add rows to the param: copy the system's file into `systems/` and write them there".to_owned(),
        })
}

/// Where the value that could not be computed first appeared: the earliest
/// node of the step's expression holding a fault, followed back through any
/// `let` that carried it there.
pub(crate) fn first_fault(f: &Frame, step: usize) -> Option<usize> {
    let root = match &f.law.steps[step].kind {
        StepKind::When(root) | StepKind::Unless(root) | StepKind::Let(root) => *root,
        StepKind::Require { cond, .. } => *cond,
        StepKind::Effect(
            LawEffect::Owe { amount, .. }
            | LawEffect::Count { amount, .. }
            | LawEffect::Consume { amount }
            | LawEffect::Carry { amount, .. },
        ) => *amount,
    };
    origin(f, root)
}

fn origin(f: &Frame, root: NodeId) -> Option<usize> {
    let at = f.law.range(root).find(|&at| matches!(f.values[at], Value::Fault(_)))?;
    match f.law.nodes[at].op {
        Op::Local(bound) => origin(f, bound).or(Some(at)),
        _ => Some(at),
    }
}

/// When the condition is `lhs <= rhs` (or `<`, `>=`, `>`) and `lhs` moves
/// one-for-one with this flow, how much would satisfy it.
fn suggestion(f: &Frame, step: usize, cond: NodeId) -> Option<String> {
    let bound = f.comparison(cond)?;
    let moves = f.facts.steps[step].follows?;
    let calc = Calc { book: f.book, day: f.ctx.day };
    let flow = calc.convert(f.ctx.amount?, bound.limit.unit).ok()?.qty;
    let show = |qty: Qty| f.money(Amount::new(qty, bound.limit.unit));
    // How far the counted side is beyond the bound, or short of it.
    let off = bound.off.qty;
    // What the counted side is a running sum of, in the words of the advice.
    let of = match moves {
        Follows::Flow => {
            return Some(match (bound.upper, flow > off) {
                (true, true) => format!("lower this flow to at most {}", show(flow - off)),
                (true, false) => "no amount of this flow satisfies it".to_owned(),
                (false, _) => format!("this flow must be at least {}", show(flow + off)),
            });
        }
        Follows::Total(dir, window) => {
            format!("{} {}", if dir == Dir::In { "go in" } else { "come out" }, span(window))
        }
        Follows::Tally(name) => format!("count toward `{}` this year", f.book.name(name)),
    };
    Some(match (bound.upper, flow > off) {
        (true, true) => format!("at most {} more can {of}", show(flow - off)),
        (true, false) => format!("nothing more can {of}: it is already {} over", show(off - flow)),
        (false, _) => format!("at least {} more must {of}", show(off)),
    })
}

fn span(window: Window) -> &'static str {
    match window {
        Window::Month => "this month",
        Window::Year => "this year",
        Window::Ever => "in total",
    }
}

/// What an assertion said and what the ledger held, in the sign the assertion
/// is written in.
#[derive(Clone, Copy)]
struct Disagreement {
    unit: Id<Commodity>,
    stated: Qty,
    held: Qty,
    /// How far the gap has moved since the last assertion on the place was
    /// checked. What the gap already was then is carried, and not explained again.
    new: Qty,
}

impl Disagreement {
    fn carried(self) -> Qty {
        self.stated - self.held - self.new
    }
}

/// The flows on an assertion's place since the last one was checked that moved
/// its commodity, as the assertion reads them.
struct Since {
    /// Flows that moved value by the assertion's day, signed the way the assertion
    /// is written: `+` raises the shown balance.
    real: Vec<(Id<Flow>, Qty)>,
    /// Flows written but not yet real, which count against nothing yet.
    pending: Vec<Id<Flow>>,
}

fn since(book: &Book, events: &Events, (assert, sign): (&Assert, i64), checked: Option<Day>) -> Since {
    let flows = &book.touching[assert.place];
    let (from, to) = (
        flows.partition_point(|&id| checked.is_some_and(|last| book.flows[id].day <= last)),
        flows.partition_point(|&id| book.flows[id].day <= assert.day),
    );
    let mut found = Since { real: Vec::new(), pending: Vec::new() };
    for &id in &flows[from..to] {
        let flow = &book.flows[id];
        let (moved, inflow) = if flow.to == assert.place { (flow.arrive, true) } else { (flow.out, false) };
        let end = if inflow { End::To } else { End::From };
        if moved.unit != assert.amount.unit || !bridge::moves_quantity(flow, end) {
            continue;
        }
        let state = events.state(id, flow);
        let signed = if inflow == (sign > 0) { moved.qty } else { -moved.qty };
        if state.is_real_on(assert.day) {
            found.real.push((id, signed));
        } else if state.is_pending_on(assert.day) {
            found.pending.push(id);
        }
    }
    found
}

/// What most likely went wrong when a statement and the ledger disagree.
enum Suspect {
    /// A flow written the wrong way round: the gap is twice its amount.
    Backwards(Id<Flow>),
    /// Two neighbouring digits swapped between the statement and the ledger.
    Swapped,
    /// The statement says the opposite of what the ledger holds.
    Sign,
    /// The account never held this commodity, but holds another.
    Unit(Id<Commodity>, Qty),
    /// Nothing else explains it.
    Missing,
}

/// Whether two amounts' digits differ only by two neighbouring digits swapped.
fn swapped(a: Qty, b: Qty) -> bool {
    let (a, b) = (a.0.unsigned_abs().to_string().into_bytes(), b.0.unsigned_abs().to_string().into_bytes());
    let at: Vec<usize> = (0..a.len().min(b.len())).filter(|&i| a[i] != b[i]).collect();
    a.len() == b.len() && at.len() == 2 && at[1] == at[0] + 1 && a[at[0]] == b[at[1]] && a[at[1]] == b[at[0]]
}

impl Suspect {
    /// The likeliest cause, given the flows that built the gap and what the
    /// place holds of other commodities. It reads numbers, not the book.
    fn find(gap: Disagreement, real: &[(Id<Flow>, Qty)], others: &[(Id<Commodity>, Qty)]) -> Suspect {
        // A flow written backwards is off by twice its amount.
        if let Some(&(id, _)) = real.iter().rev().find(|&&(_, signed)| signed.0 * 2 == -gap.new.0) {
            return Suspect::Backwards(id);
        }
        if gap.carried().is_zero() && swapped(gap.held, gap.stated) {
            return Suspect::Swapped;
        }
        if !gap.held.is_zero() && gap.stated == -gap.held {
            return Suspect::Sign;
        }
        let elsewhere = others.iter().filter(|&&(other, qty)| other != gap.unit && !qty.is_zero());
        let alike = elsewhere.clone().find(|&&(_, qty)| qty == gap.stated).or_else(|| elsewhere.clone().next());
        match alike {
            Some(&(other, qty)) if gap.held.is_zero() => Suspect::Unit(other, qty),
            _ => Suspect::Missing,
        }
    }

    /// What the diagnostic says about the suspect: a note on why, the flow it
    /// points at, and the edit that would fix it.
    fn advise(self, d: Diagnostic, book: &Book, gap: Disagreement) -> Diagnostic {
        let money = |qty: Qty| book.show(Amount::new(qty, gap.unit)).to_string();
        match self {
            Suspect::Backwards(id) => {
                let flow = &book.flows[id];
                let (from, to) = (show::place(book, flow.from), show::place(book, flow.to));
                d.note(format!(
                    "the gap is exactly twice this flow (2 × {}): it is probably written backwards",
                    money(Qty(gap.new.0.abs() / 2))
                ))
                .context(flow.loc, "probably written the wrong way round")
                .help(format!("write it the other way: `{to} -> {from}`"))
            }
            Suspect::Swapped => d
                .note(format!(
                    "{} and {} differ only by two neighbouring digits swapped",
                    money(gap.held),
                    money(gap.stated)
                ))
                .help(format!("if the statement says {}, correct the amount", money(gap.held))),
            Suspect::Sign => d
                .note(format!(
                    "the ledger holds {}, the opposite of {}: the sign may be wrong",
                    money(gap.held),
                    money(gap.stated)
                ))
                .help(format!(
                    "if the balance is {}, write it with its sign: `= {}`",
                    money(gap.held),
                    money(gap.held)
                )),
            Suspect::Unit(other, qty) => {
                let (symbol, shown) = (book.name(book.commodities[other].symbol), book.show(Amount::new(qty, other)));
                d.help(format!("assert in {symbol}: {shown}"))
            }
            Suspect::Missing => d
                .note(format!(
                    "{} is neither twice a flow nor a transposition: most likely a flow is missing",
                    money(gap.new.abs())
                ))
                .help("record the missing flow"),
        }
    }
}

/// An assertion that does not hold. `held` is the place's balance and `new`
/// how far the gap has moved since `checked`, the last time an assertion on the
/// place was checked (both in the sign the assertion is written in); what
/// the gap already was then is carried, and not explained again. `others` is
/// what the place holds of other commodities.
pub(crate) fn mismatch(
    book: &Book,
    events: &Events,
    (assert, sign): (&Assert, i64),
    (held, new): (Qty, Qty),
    checked: Option<Day>,
    others: &[(Id<Commodity>, Qty)],
) -> Diagnostic {
    let gap = Disagreement { unit: assert.amount.unit, stated: assert.amount.qty, held, new };
    let money = |qty: Qty| book.show(Amount::new(qty, gap.unit)).to_string();
    let place = show::place(book, assert.place);
    let flows = since(book, events, (assert, sign), checked);
    let suspect = Suspect::find(gap, &flows.real, others);

    let headline = match suspect {
        Suspect::Unit(other, qty) => {
            let (was, is) = (book.name(book.commodities[gap.unit].symbol), book.show(Amount::new(qty, other)));
            format!("{place} has never held {was}; it holds {is}")
        }
        _ => format!("{place} holds {}, not {}", money(held), money(gap.stated)),
    };
    let direction = if new > Qty::ZERO { "more" } else { "less" };
    let label = match gap.carried().is_zero() {
        true => format!("{} {direction} than the ledger holds", money(new.abs())),
        false => format!("another {} {direction} than the ledger holds", money(new.abs())),
    };
    let mut d = Diagnostic::error("assertion", headline).label(assert.loc, label);
    if !gap.carried().is_zero() {
        d = d.note(format!(
            "the {} gap reported at an earlier assertion is carried; only what is new is explained here",
            money(gap.carried().abs())
        ));
    }
    let hidden = flows.real.len().saturating_sub(SHOWN);
    for &(id, signed) in &flows.real[hidden..] {
        let flow = &book.flows[id];
        let (peer, direction) = if flow.to == assert.place { (flow.from, "from") } else { (flow.to, "to") };
        let plus = if signed >= Qty::ZERO { '+' } else { '-' };
        d = d.context(flow.loc, format!("{plus}{} {direction} {}", money(signed.abs()), show::place(book, peer)));
    }
    let window = checked.map_or("the start of the book".to_owned(), |last| format!("the assertion on {last}"));
    if hidden > 0 {
        d = d.note(format!("{hidden} earlier flows since {window} are not shown"));
    }
    if !flows.pending.is_empty() {
        let names: Vec<String> =
            flows.pending.iter().map(|&id| show::place(book, book.flows[id].to).to_owned()).collect();
        d = d.note(format!(
            "not counted, because still pending: {} flows to {}",
            flows.pending.len(),
            names.join(", ")
        ));
    }
    let end = assert.loc.end;
    suspect.advise(d, book, gap).fix(
        "or accept the gap: it is booked from `equity/unknown` and shown in every report",
        Loc::new(assert.loc.file, end, end),
        " !",
    )
}

/// An assertion that cannot be judged because a flow it depends on has an
/// amount nobody could work out. Reported once; it never suggests `!`.
pub(crate) fn unchecked(book: &Book, assert: &Assert, unknown: Loc) -> Diagnostic {
    let place = show::place(book, assert.place);
    Diagnostic::info("unchecked", format!("{place} is not checked against {}", book.show(assert.amount)))
        .label(assert.loc, "not checked: it depends on an amount that could not be worked out")
        .context(unknown, "this amount is unknown")
        .note("the assertions on this place after it are judged from here, and only a change in the gap is reported")
}

/// An assertion's gap accepted with `!`.
pub(crate) fn padded(book: &Book, assert: &Assert, waive: Waive, amount: Amount) -> Diagnostic {
    let place = show::place(book, assert.place);
    let moved = book.show(Amount::new(amount.qty.abs(), amount.unit));
    let what = if amount.qty > Qty::ZERO {
        format!("{moved} moved from unknown into {place}")
    } else {
        format!("{moved} moved out of {place} into unknown")
    };
    let mut d = Diagnostic::info("pad", format!("accepted a gap on {place}"))
        .label(assert.loc, what)
        .disposed(Disposition::Waived);
    if let Some(reason) = waive.reason {
        d = d.note(format!("accepted because: {}", book.name(reason)));
    }
    d
}

/// Lots that differ, and no rule to choose between them: each candidate, and
/// the gain the flow would realize if it came from that lot alone.
pub(crate) fn ambiguous(book: &Book, m: &Motion, candidates: &[Candidate], proceeds: Option<Qty>) -> Diagnostic {
    let (place, unit) = (show::place(book, m.from), m.out.unit);
    let money = |qty: Qty| book.show(Amount::new(qty, unit)).to_string();
    let base = |qty: Qty| book.show(Amount::new(qty, book.base)).to_string();
    let headline = format!("ambiguous lot: {place} holds {} lots that differ and no policy applies", candidates.len());
    let mut d = Diagnostic::error("ambiguous-lots", headline)
        .label(m.loc, format!("{} could come from any of them", money(m.out.qty)));
    for candidate in candidates.iter().take(SHOWN) {
        let part = candidate.qty.min(m.out.qty);
        let basis = candidate.basis.share(part, candidate.qty).unwrap_or(candidate.basis);
        let gain = proceeds
            .and_then(|total| total.share(part, m.out.qty))
            .map(|worth| format!("; from it alone the gain is {}", base(worth - basis)));
        let text = format!(
            "acquired {}: {}, basis {}{}",
            candidate.acquired,
            money(candidate.qty),
            base(candidate.basis),
            gain.unwrap_or_default()
        );
        d = match candidate.txn.and_then(|id| book.txns.get(id)) {
            Some(txn) => d.context(txn.loc, text),
            None => d.note(format!("plain money: {}", money(candidate.qty))),
        };
    }
    if candidates.len() > SHOWN {
        d = d.note(format!("{} more lots not shown", candidates.len() - SHOWN));
    }
    let lot = candidates
        .iter()
        .find(|c| c.txn.is_some())
        .map_or(String::new(), |c| format!("name the lot, `{place}[{}]`, or ", c.acquired));
    d.note("the sale still moves the oldest lot first, so the lines after it stay consistent")
        .note("later sales from this place are booked the same way, without another report")
        .help(format!("{lot}a policy, `{place}[fifo]` (or lifo, hifo, prorata), or give the account `select fifo`"))
}

/// A `=` target the place has already passed: reaching it would take a flow
/// in the other direction.
pub(crate) fn past_target(
    book: &Book,
    flow: &Flow,
    place: Id<Place>,
    unit: Id<Commodity>,
    (held, target): (Qty, Qty),
    end: End,
) -> Diagnostic {
    let money = |qty: Qty| book.show(Amount::new(qty, unit)).to_string();
    let (place, side) = (show::place(book, place), if end == End::From { "below" } else { "above" });
    Diagnostic::error(
        "past-target",
        format!("{place} holds {}, already {side} the target {}", money(held), money(target)),
    )
    .label(flow.loc, "no flow this way can end there; it moves nothing")
    .help("write the flow the other way round, or correct the target")
}

/// A sale of more than the holding has.
pub(crate) fn shortfall(book: &Book, m: &Motion, held: Qty, admitted: Qty, short: Qty) -> Diagnostic {
    let (place, unit) = (show::place(book, m.from), m.out.unit);
    let money = |qty: Qty| book.show(Amount::new(qty, unit)).to_string();
    let mut d = Diagnostic::error("insufficient-holding", format!("{place} does not hold {}", money(m.out.qty)))
        .label(m.loc, format!("{} more than {place} has", money(short)))
        .note(format!("{place} holds {}", money(held)));
    if !m.select.is_empty() {
        d = d.note(format!("the selectors match {} of it", money(admitted)));
    }
    d.help("record the purchase before this flow, or check the quantity; the missing amount is left as a negative balance so the rest of the ledger stays consistent")
}

/// A flow into or out of `PLACE.basis` that the place cannot carry: it holds
/// no parcel to take the basis (`carried` is false), or less basis than the
/// flow takes off.
pub(crate) fn basis_shortfall(
    book: &Book,
    m: &Motion,
    place: Id<Place>,
    held: Qty,
    amount: Qty,
    carried: bool,
) -> Diagnostic {
    let (name, base) = (show::place(book, place), |qty: Qty| book.show(Amount::new(qty, book.base)).to_string());
    let what = if carried {
        format!("{name} has {} of basis, and this flow takes {} off", base(held), base(amount))
    } else {
        format!("{name} holds nothing to carry a change of basis")
    };
    Diagnostic::error("no-basis", what)
        .label(m.loc, "a basis flow changes the basis of the parcels a place holds, and moves no quantity")
        .help("record what the place holds first, or check the amount and the selectors")
}

/// A claim still open past its day: who owes what, and for how long.
pub(crate) fn overdue(
    book: &Book,
    place: Id<Place>,
    unit: Id<Commodity>,
    lot: &Parcel,
    today: Day,
) -> Option<Diagnostic> {
    let claim = book.paid_into(lot.txn, place)?;
    let due = claim.detail().due.filter(|&due| due <= today)?;
    let who = claim.payee.map_or_else(|| show::place(book, place), |entity| book.name(book.entities[entity].path));
    let owed = book.show(Amount::new(lot.qty, unit));
    let late = today.0 - due.0;
    Some(
        Diagnostic::warning("overdue", format!("{who} still owes {owed}, {late} days past its due day {due}"))
            .label(claim.loc, format!("claimed on {}, due {due}", lot.acquired))
            .note(format!("open for {} since it was made", today.since(lot.acquired)))
            .help("if it has been paid, record the payment `for` the claim's code"),
    )
}

/// A `!` on a transaction none of whose flows raised anything.
pub(crate) fn unused_waiver(at: Loc) -> Diagnostic {
    Diagnostic::warning("unused-waiver", "this `!` waives nothing")
        .label(at, "no law objected to this transaction")
        .help("delete the `!`, or move it to the flow that needs it")
}

#[cfg(test)]
mod tests {
    use super::*;

    const USD: Id<Commodity> = Id::new(0);
    const EUR: Id<Commodity> = Id::new(1);

    /// The ledger held `held` where `stated` was written, with `new` of the gap new.
    fn gap(stated: i64, held: i64, new: i64) -> Disagreement {
        Disagreement { unit: USD, stated: Qty(stated), held: Qty(held), new: Qty(new) }
    }

    fn found(gap: Disagreement, real: &[(u32, i64)], others: &[(Id<Commodity>, i64)]) -> Suspect {
        let real: Vec<_> = real.iter().map(|&(id, signed)| (Id::new(id), Qty(signed))).collect();
        let others: Vec<_> = others.iter().map(|&(unit, qty)| (unit, Qty(qty))).collect();
        Suspect::find(gap, &real, &others)
    }

    #[test]
    fn a_gap_of_twice_a_flow_points_at_the_latest_such_flow() {
        // 50 was entered as -50: 100 short, and three flows of 50.
        let suspect = found(gap(500, 400, 100), &[(1, 50), (2, -50), (3, -50)], &[]);
        assert!(matches!(suspect, Suspect::Backwards(id) if id == Id::new(3)));
    }

    #[test]
    fn two_swapped_digits_are_noticed_only_when_nothing_older_is_carried() {
        assert!(matches!(found(gap(1_234, 1_324, -90), &[], &[]), Suspect::Swapped));
        assert!(matches!(found(gap(1_234, 1_324, -50), &[], &[]), Suspect::Missing), "part of the gap is carried");
    }

    #[test]
    fn the_opposite_of_what_is_held_is_a_sign_slip() {
        assert!(matches!(found(gap(-700, 700, -1_400), &[], &[]), Suspect::Sign));
        assert!(matches!(found(gap(0, 0, 0), &[], &[]), Suspect::Missing), "nothing held has no opposite");
    }

    #[test]
    fn an_account_that_holds_another_commodity_was_asserted_in_the_wrong_one() {
        let others = [(USD, 900), (EUR, 300), (Id::new(2), 700)];
        let suspect = found(gap(700, 0, 700), &[], &others);
        assert!(matches!(suspect, Suspect::Unit(other, qty) if other == Id::new(2) && qty == Qty(700)));
        let suspect = found(gap(500, 0, 500), &[], &others);
        assert!(matches!(suspect, Suspect::Unit(other, _) if other == EUR), "else the first it holds");
        assert!(matches!(found(gap(500, 100, 400), &[], &others), Suspect::Missing), "it did hold this one");
    }
}
