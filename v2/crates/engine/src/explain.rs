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

use axiom_core::{Day, Diagnostic, Disposition, Id, Loc, Qty, Severity, Sym};
use axiom_model::{
    Amount, Assert, BinOp, Book, Commodity, Dir, Effect as Consequence, End, Fault, Flow, Func, Law, NodeId, Op, Place,
    StepKind, Subject, Trigger, Value, Var, Waive, Window,
};

use crate::calc::Calc;
use crate::eval::Context;
use crate::events::Events;
use crate::fire::Reads;
use crate::lots::Candidate;
use crate::motion::Motion;
use crate::show;
use crate::{Cause, Effect, Owed, Parcel};

/// The most flows an assertion's explanation draws.
const SHOWN: usize = 8;

/// A law's evaluation, frozen at the moment something went wrong.
pub(crate) struct Frame<'a, 's> {
    pub book: &'a Book<'s>,
    pub law: &'a Law,
    pub ctx: &'a Context<'a>,
    /// Every node's value from the run that just ended.
    pub values: &'a [Value],
    /// What laws recorded so far: which flows counted into a tally.
    pub effects: &'a [Effect],
}

/// Why a violation is not an error.
#[derive(Clone, Copy)]
pub(crate) enum Waiver {
    /// `!` on the flow's leg or transaction.
    Marked(Waive),
    /// The book or the command line is `relaxed`.
    Relaxed,
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
            (Trigger::Spend, _) => format!("{} of restricted money leaving {from}", self.money(moving)),
            _ => format!("{} from {from} to {to}", self.money(motion.out)),
        };
        (motion.loc, format!("this flow: {what}"))
    }

    /// The comparison a step failed on, when it compares amounts.
    fn bound(&self, cond: NodeId) -> Option<Bound> {
        let Op::Bin(cmp @ (BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge), l, r) = self.law.nodes[cond.index()].op else {
            return None;
        };
        let (counted, limit) = match (self.values[l.index()], self.values[r.index()]) {
            (Value::Amount(a), Value::Amount(b)) => (a, b),
            (Value::Amount(a), Value::Empty) => (a, Amount::zero(a.unit)),
            (Value::Empty, Value::Amount(b)) => (Amount::zero(b.unit), b),
            _ => return None,
        };
        let upper = matches!(cmp, BinOp::Lt | BinOp::Le);
        let counted_in_limit = Calc { book: self.book, day: self.ctx.day }.convert(counted, limit.unit).ok()?;
        let off = if upper { counted_in_limit.qty - limit.qty } else { limit.qty - counted_in_limit.qty };
        Some(Bound { counted, limit, upper, off: Amount::new(off, limit.unit) })
    }

    /// The two sides of the failing comparison with their values, or every
    /// non-constant part of any other condition.
    fn operands(&self, cond: NodeId) -> Vec<(Loc, String)> {
        let nodes = &self.law.nodes;
        let at = match nodes[cond.index()].op {
            Op::Bin(BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge | BinOp::Eq | BinOp::Ne, l, r) => vec![l.index(), r.index()],
            _ => self.law.range(cond).collect(),
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

    /// Up to three flows before this one that built what a limit counted.
    fn contributors(&self, reads: Reads) -> Vec<Id<Flow>> {
        let (book, ctx) = (self.book, self.ctx);
        let Cause::Flow(current) = ctx.cause else { return Vec::new() };
        let window = reads.window(ctx);
        let mut found: Vec<Id<Flow>> = match reads {
            Reads::Tally(name) => {
                let counted = |e: &&Effect| e.owner == ctx.owner && e.name == name && window.from <= e.day && e.day <= window.until;
                self.effects.iter().rev().filter(counted).filter_map(|e| match e.cause {
                    Cause::Flow(id) if id != current => Some(id),
                    _ => None,
                }).collect()
            }
            Reads::Total(dir, _) => {
                let Subject::Place(place) = ctx.subject else { return Vec::new() };
                let flows = &book.touching[place];
                let before = flows.partition_point(|&id| id < current);
                let crosses = |flow: &Flow| {
                    let (here, there) = if dir == Dir::In { (flow.to, flow.from) } else { (flow.from, flow.to) };
                    book.places.covers(place, here) && !book.places.covers(place, there)
                };
                let moves = flows[..before].iter().rev().take(400).filter(|&&id| {
                    let flow = &book.flows[id];
                    crosses(flow) && flow.recognized.until >= window.from && flow.recognized.from <= window.until
                });
                moves.copied().collect()
            }
        };
        found.truncate(3);
        found.reverse();
        found
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
struct Bound {
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
    let (bound, reads) = (f.bound(cond), Reads::of(f.law, cond));
    let headline = match (message.map(|text| f.book.name(text).to_owned()), &bound) {
        (message, Some(bound)) => {
            let lead = message.unwrap_or_else(|| show::subject(f.book, f.ctx.subject).to_owned());
            format!("{lead}: {}", fact(f, bound, reads))
        }
        (Some(message), None) => message,
        (None, None) => what.clone().unwrap_or_else(|| format!("{} is not satisfied by {}", f.book.name(f.law.name), show::subject(f.book, f.ctx.subject))),
    };
    let severity = if warn { Severity::Warning } else { Severity::Error };
    let mut d = Diagnostic::new(severity, f.book.name(f.law.name).to_owned(), headline);
    let (loc, text) = f.cause_label();
    d = f.contributions(d.label(loc, text), reads);
    for (loc, value) in f.operands(cond) {
        d = d.context(loc, value);
    }
    if let Some(what) = what.filter(|_| message.is_some() || bound.is_some()) {
        d = d.note(what);
    }
    if let Some(help) = suggestion(f, step, cond) {
        d = d.help(help);
    }
    if let Some(fix) = fix {
        d = d.help(fix);
    }
    accepted(d, f, waiver)
}

/// "27,000.00 USD in 2026 against a limit of 24,500.00 USD, over by 2,500.00 USD"
fn fact(f: &Frame, bound: &Bound, reads: Option<Reads>) -> String {
    let window = reads.map(|reads| reads.window(f.ctx).from);
    let when = match (reads, window) {
        (Some(Reads::Total(_, Window::Month)), Some(from)) => format!(" in {}-{:02}", from.year(), from.ymd().1),
        (Some(Reads::Total(_, Window::Ever)), _) => " in total".to_owned(),
        (Some(_), Some(from)) => format!(" in {}", from.year()),
        _ => String::new(),
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
        Some(Waiver::Relaxed) => d.note("shown as a warning because the book is `relaxed`").relaxed().disposed(Disposition::Waived),
        None => d,
    }
}

/// A `require … else owe …` that failed: not an error, a price. It says what
/// is owed, to whom and by when, and why.
pub(crate) fn priced(f: &Frame, step: usize, (name, amount, owed): (Sym, Amount, Owed), waive: Option<Waive>) -> Diagnostic {
    let StepKind::Require { cond, .. } = f.law.steps[step].kind else { unreachable!("only a require prices") };
    let who = f.book.name(f.book.entities[owed.to].path);
    let headline = format!("{} owed to {who} by {}: {}", f.money(amount), owed.due, f.book.name(name));
    let mut d = Diagnostic::info(f.book.name(f.law.name).to_owned(), headline).disposed(Disposition::Priced);
    let (loc, text) = f.cause_label();
    d = d.label(loc, text);
    for (loc, value) in f.operands(cond) {
        d = d.context(loc, value);
    }
    let (what, fix) = f.doc();
    if let Some(what) = what {
        d = d.note(what);
    }
    match waive {
        Some(waive) => accepted(d, f, Some(Waiver::Marked(waive))),
        None => match fix {
            Some(fix) => d.help(fix),
            None => d.help("if an exception applies, keep the flow and mark it `!` with the reason"),
        },
    }
}

/// A fault reached a step: the data the law needs does not exist.
pub(crate) fn faulted(f: &Frame, step: usize, fault: Fault) -> Diagnostic {
    let (what, help) = show::fault(f.book, fault, f.ctx.day);
    let code = match fault {
        Fault::NoPrice { .. } => "no-price",
        Fault::Unset(_) => "unset-property",
        Fault::NoRow(_) => "no-param-row",
        Fault::DivideByZero | Fault::Overflow => "arithmetic",
    };
    let mut d = Diagnostic::error(code, format!("cannot check `{}`: {what}", f.book.name(f.law.name)));
    let (loc, text) = f.cause_label();
    d = d.label(loc, text);
    if let Some(origin) = first_fault(f, step) {
        d = d.context(f.law.nodes[origin].loc, what);
    }
    match help {
        Some(help) => d.help(help),
        None => d,
    }
}

/// Where the value that could not be computed first appeared: the earliest
/// node of the step's expression holding a fault.
fn first_fault(f: &Frame, step: usize) -> Option<usize> {
    let root = match &f.law.steps[step].kind {
        StepKind::When(root) | StepKind::Let(root) => *root,
        StepKind::Require { cond, .. } => *cond,
        StepKind::Effect(Consequence::Owe { amount, .. } | Consequence::Count { amount, .. }) => *amount,
    };
    f.law.range(root).find(|&at| matches!(f.values[at], Value::Fault(_)))
}

/// When the condition is `lhs <= rhs` (or `<`, `>=`, `>`) and `lhs` moves
/// one-for-one with this flow, how much would satisfy it.
fn suggestion(f: &Frame, step: usize, cond: NodeId) -> Option<String> {
    let bound = f.bound(cond)?;
    let Op::Bin(_, lhs, _) = f.law.nodes[cond.index()].op else { return None };
    let moves = follows_flow(f, step, lhs)?;
    let calc = Calc { book: f.book, day: f.ctx.day };
    let flow = calc.convert(f.ctx.amount?, bound.limit.unit).ok()?.qty;
    let show = |qty: Qty| f.money(Amount::new(qty, bound.limit.unit));
    // How far the counted side is beyond the bound, or short of it.
    let off = bound.off.qty;
    Some(match (moves, bound.upper) {
        (Moves::Flow, true) if flow > off => format!("lower this flow to at most {}", show(flow - off)),
        (Moves::Flow, true) => "no amount of this flow satisfies it".to_owned(),
        (Moves::Flow, false) => format!("this flow must be at least {}", show(flow + off)),
        (Moves::Total(dir, window), true) if flow > off => {
            format!("at most {} more can {} {}", show(flow - off), verb(dir), span(window))
        }
        (Moves::Total(dir, window), true) => {
            format!("nothing more can {} {}: it is already {} over", verb(dir), span(window), show(off - flow))
        }
        (Moves::Total(dir, window), false) => {
            format!("at least {} more must {} {}", show(off), verb(dir), span(window))
        }
        (Moves::Tally(name), true) if flow > off => {
            format!("at most {} more can count toward `{}` this year", show(flow - off), f.book.name(name))
        }
        (Moves::Tally(name), true) => format!(
            "nothing more can count toward `{}` this year: it is already {} over",
            f.book.name(name),
            show(off - flow)
        ),
        (Moves::Tally(name), false) => {
            format!("at least {} more must count toward `{}` this year", show(off), f.book.name(name))
        }
    })
}

/// What a comparison's left side is a running sum of, so that this flow moves
/// it by its own amount.
#[derive(Clone, Copy)]
enum Moves {
    /// The flow's own amount.
    Flow,
    /// A window total the flow was just added to.
    Total(Dir, Window),
    /// A tally an earlier step of the same law counted this flow's amount into.
    Tally(Sym),
}

fn follows_flow(f: &Frame, step: usize, lhs: NodeId) -> Option<Moves> {
    match (&f.law.nodes[lhs.index()].op, f.law.trigger) {
        (Op::Var(Var::Amount), _) => Some(Moves::Flow),
        (Op::Call(Func::Total(Dir::In, window), _), Trigger::In) => Some(Moves::Total(Dir::In, *window)),
        (Op::Call(Func::Total(Dir::Out, window), _), Trigger::Out) => Some(Moves::Total(Dir::Out, *window)),
        (Op::Call(Func::Tally(name), _), _) => {
            let counts_amount = |kind: &StepKind| match kind {
                StepKind::Effect(Consequence::Count { amount, name: counted }) => {
                    counted == name && matches!(f.law.nodes[amount.index()].op, Op::Var(Var::Amount))
                }
                _ => false,
            };
            f.law.steps[..step].iter().any(|s| counts_amount(&s.kind)).then_some(Moves::Tally(*name))
        }
        _ => None,
    }
}

fn verb(dir: Dir) -> &'static str {
    match dir {
        Dir::In => "go in",
        Dir::Out => "come out",
    }
}

fn span(window: Window) -> &'static str {
    match window {
        Window::Month => "this month",
        Window::Year => "this year",
        Window::Ever => "in total",
    }
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

/// An assertion that does not hold. `held` is the place's balance and `new`
/// how far the gap has moved since `since`, the last time an assertion on the
/// place was checked (both in the sign the assertion is written in); what
/// the gap already was then is carried, and not explained again. `others` is
/// what the place holds of other commodities.
pub(crate) fn mismatch(
    book: &Book,
    events: &Events,
    assert: &Assert,
    (held, new): (Qty, Qty),
    since: Option<Day>,
    others: &[(Id<Commodity>, Qty)],
) -> Diagnostic {
    let (place, unit, day, stated) = (show::place(book, assert.place), assert.amount.unit, assert.day, assert.amount.qty);
    let money = |qty: Qty| book.show(Amount::new(qty, unit)).to_string();
    let sign = book.places[assert.place].class.display_sign();

    // The flows since the last checkpoint that moved this commodity, as the assertion reads them.
    let flows = &book.touching[assert.place];
    let (from, to) = (
        flows.partition_point(|&id| since.is_some_and(|last| book.flows[id].day <= last)),
        flows.partition_point(|&id| book.flows[id].day <= day),
    );
    let (mut real, mut pending) = (Vec::new(), Vec::new());
    for &id in &flows[from..to] {
        let flow = &book.flows[id];
        let (moved, inflow) = if flow.to == assert.place { (flow.arrive, true) } else { (flow.out, false) };
        let state = events.state(id, flow);
        if moved.unit != unit {
            continue;
        }
        // Signed the way the assertion is written: `+` raises the shown balance.
        let signed = if inflow == (sign > 0) { moved.qty } else { -moved.qty };
        if state.is_real_on(day) {
            real.push((id, signed));
        } else if state.is_pending_on(day) {
            pending.push(id);
        }
    }

    let carried = stated - held - new;
    // A flow written backwards is off by twice its amount.
    let backwards = real.iter().rev().find(|&&(_, signed)| signed.0 * 2 == -new.0).map(|&(id, _)| id);
    let elsewhere = others.iter().filter(|&&(other, qty)| other != unit && !qty.is_zero());
    let suspect = match (backwards, elsewhere.clone().find(|&&(_, qty)| qty == stated).or(elsewhere.clone().next())) {
        (Some(id), _) => Suspect::Backwards(id),
        _ if carried.is_zero() && swapped(held, stated) => Suspect::Swapped,
        _ if !held.is_zero() && stated == -held => Suspect::Sign,
        (_, Some(&(other, qty))) if held.is_zero() => Suspect::Unit(other, qty),
        _ => Suspect::Missing,
    };

    let headline = match suspect {
        Suspect::Unit(other, qty) => {
            let (was, is) = (book.name(book.commodities[unit].symbol), book.show(Amount::new(qty, other)));
            format!("{place} has never held {was}; it holds {is}")
        }
        _ => format!("{place} holds {}, not {}", money(held), money(stated)),
    };
    let direction = if new > Qty::ZERO { "more" } else { "less" };
    let label = match carried.is_zero() {
        true => format!("{} {direction} than the ledger holds", money(new.abs())),
        false => format!("another {} {direction} than the ledger holds", money(new.abs())),
    };
    let mut d = Diagnostic::error("assertion", headline).label(assert.loc, label);
    if !carried.is_zero() {
        d = d.note(format!("the {} gap reported at an earlier assertion is carried; only what is new is explained here", money(carried.abs())));
    }

    let hidden = real.len().saturating_sub(SHOWN);
    for &(id, signed) in &real[hidden..] {
        let flow = &book.flows[id];
        let (peer, inflow) = if flow.to == assert.place { (flow.from, "from") } else { (flow.to, "to") };
        let plus = if signed >= Qty::ZERO { '+' } else { '-' };
        d = d.context(flow.loc, format!("{plus}{} {inflow} {}", money(signed.abs()), show::place(book, peer)));
    }
    let window = since.map_or("the start of the book".to_owned(), |last| format!("the assertion on {last}"));
    if hidden > 0 {
        d = d.note(format!("{hidden} earlier flows since {window} are not shown"));
    }
    if !pending.is_empty() {
        let names: Vec<String> = pending.iter().map(|&id| show::place(book, book.flows[id].to).to_owned()).collect();
        d = d.note(format!("not counted, because still pending: {} flows to {}", pending.len(), names.join(", ")));
    }

    d = match suspect {
        Suspect::Backwards(id) => {
            let flow = &book.flows[id];
            let (from, to) = (show::place(book, flow.from), show::place(book, flow.to));
            d.note(format!("the gap is exactly twice this flow (2 × {}): it is probably written backwards", money(Qty(new.0.abs() / 2))))
                .context(flow.loc, "probably written the wrong way round")
                .help(format!("write it the other way: `{to} -> {from}`"))
        }
        Suspect::Swapped => d
            .note(format!("{} and {} differ only by two neighbouring digits swapped", money(held), money(stated)))
            .help(format!("if the statement says {}, correct the amount", money(held))),
        Suspect::Sign => d
            .note(format!("the ledger holds {}, the opposite of {}: the sign may be wrong", money(held), money(stated)))
            .help(format!("if the balance is {}, write it with its sign: `= {}`", money(held), money(held))),
        Suspect::Unit(other, qty) => {
            let (symbol, shown) = (book.name(book.commodities[other].symbol), book.show(Amount::new(qty, other)));
            d.help(format!("assert in {symbol}: {shown}"))
        }
        Suspect::Missing => d
            .note(format!("{} is neither twice a flow nor a transposition: most likely a flow is missing", money(new.abs())))
            .help("record the missing flow"),
    };
    let end = assert.loc.end;
    d.fix("or accept the gap: it is booked from `equity/unknown` and shown in every report", Loc::new(assert.loc.file, end, end), " !")
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
    let mut d = Diagnostic::info("pad", format!("accepted a gap on {place}")).label(assert.loc, what).disposed(Disposition::Waived);
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
pub(crate) fn basis_shortfall(book: &Book, m: &Motion, place: Id<Place>, held: Qty, amount: Qty, carried: bool) -> Diagnostic {
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
pub(crate) fn overdue(book: &Book, place: Id<Place>, unit: Id<Commodity>, lot: &Parcel, today: Day) -> Option<Diagnostic> {
    let txn = book.txns.get(lot.txn)?;
    let due = txn.due.filter(|&due| due <= today)?;
    let who = txn.payee.map_or_else(|| show::place(book, place), |entity| book.name(book.entities[entity].path));
    let owed = book.show(Amount::new(lot.qty, unit));
    let late = today.0 - due.0;
    Some(
        Diagnostic::warning("overdue", format!("{who} still owes {owed}, {late} days past its due day {due}"))
            .label(txn.loc, format!("claimed on {}, due {due}", lot.acquired))
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
