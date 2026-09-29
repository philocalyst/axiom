//! What the fold says when something is wrong: a law that fails, an assertion
//! that does not hold, lots that cannot be told apart, a sale of more than was
//! held.
//!
//! When a `require` is false, the evaluator's scratch table still holds the
//! value of every subexpression that ran. The diagnostic prints them under the
//! law's own source, power-assert style, points at the flow that tripped it,
//! quotes the law's doc comment, and, when the comparison is a bound on this
//! flow or a window total, computes the amount that would satisfy it.

use axiom_core::{Day, Diagnostic, Id, Loc, Qty, Sym};
use axiom_model::{
    Amount, Assert, BinOp, Book, Commodity, Dir, Effect as Consequence, End, Fault, Flow, Func, Law, NodeId, Op, Place,
    StepKind, Trigger, Value, Var, Waive, Window,
};

use crate::calc::Calc;
use crate::eval::Context;
use crate::events::Events;
use crate::motion::Motion;
use crate::relief::{Candidate, Source};
use crate::{Holding, show};

/// A law's evaluation, frozen at the moment something went wrong.
pub(crate) struct Frame<'a, 's> {
    pub book: &'a Book<'s>,
    pub law: &'a Law,
    pub ctx: &'a Context<'a>,
    /// Every node's value from the run that just ended.
    pub values: &'a [Value],
}

/// Why a violation is not an error.
#[derive(Clone, Copy)]
pub(crate) enum Waiver {
    /// `!` on the transaction.
    Marked(Waive),
    /// The book or the command line is `relaxed`.
    Relaxed,
}

/// A `require` or `warn` whose condition is false.
pub(crate) fn broken(f: &Frame, step: usize, warn: bool, waiver: Option<Waiver>) -> Diagnostic {
    let StepKind::Require { cond, message, .. } = f.law.steps[step].kind else {
        unreachable!("only a require or warn step breaks")
    };
    // The law's own words, best first: the step's message, the doc comment's
    // first line, a sentence built from the law's name. Whatever the headline
    // did not use of the doc comment becomes the note.
    let doc = doc_lines(f);
    let (headline, explanation) = match (message, doc.split_first()) {
        (Some(text), _) => (f.book.name(text).to_owned(), &doc[..]),
        (None, Some((first, rest))) => ((*first).to_owned(), rest),
        (None, None) => {
            let sentence =
                format!("law `{}` does not hold for {}", f.book.name(f.law.name), show::subject(f.book, f.ctx.subject));
            (sentence, &doc[..])
        }
    };
    let mut d = if warn { Diagnostic::warning("law", headline) } else { Diagnostic::error("law", headline) };
    let (loc, text) = cause_label(f);
    d = d.label(loc, text);
    for (loc, value) in parts(f, cond) {
        d = d.context(loc, value);
    }
    if !explanation.is_empty() {
        d = d.note(explanation.join(" "));
    }
    if let Some(help) = suggestion(f, step, cond) {
        d = d.help(help);
    }
    match waiver {
        Some(Waiver::Marked(waive)) => {
            d = d.context(waive.loc, "waived here").relaxed();
            if let Some(reason) = waive.reason {
                d = d.note(format!("waived: {}", f.book.name(reason)));
            }
            d
        }
        Some(Waiver::Relaxed) => d.note("shown as a warning because the book is `relaxed`").relaxed(),
        None => d,
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
    let (loc, text) = cause_label(f);
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

/// The primary label: the flow that fired the law, or the law itself for one
/// fired by time.
fn cause_label(f: &Frame) -> (Loc, String) {
    let book = f.book;
    let Some(motion) = f.ctx.motion else {
        let text = format!("checked for {} on {}", show::subject(book, f.ctx.subject), f.ctx.day);
        return (f.law.loc, text);
    };
    let amount = |a: Amount| book.show(a).to_string();
    let moving = f.ctx.amount.unwrap_or(motion.out);
    let (from, to) = (show::place(book, motion.from), show::place(book, motion.to));
    let what = match (f.law.trigger, f.ctx.realized) {
        (Trigger::In, _) => format!("{} into {to}", amount(moving)),
        (Trigger::Out, _) => format!("{} out of {from}", amount(moving)),
        (Trigger::Gain, Some(r)) => {
            format!("a gain of {} on {}", amount(Amount::new(r.gain, book.base)), amount(moving))
        }
        (Trigger::Spend, _) => format!("{} of restricted money leaving {from}", amount(moving)),
        _ => format!("{} from {from} to {to}", amount(motion.out)),
    };
    (motion.loc, format!("this flow: {what}"))
}

/// The law's doc comment, line by line, without its `///` markers.
fn doc_lines<'a>(f: &Frame<'a, '_>) -> Vec<&'a str> {
    let Some(doc) = f.law.doc else { return Vec::new() };
    let lines = f.book.name(doc).lines().map(|l| l.trim().trim_start_matches("///").trim());
    lines.filter(|l| !l.is_empty()).collect()
}

/// Every non-constant subexpression of the condition with its value, one per
/// source range.
fn parts(f: &Frame, cond: NodeId) -> Vec<(Loc, String)> {
    let mut found: Vec<(Loc, String)> = Vec::new();
    for at in f.law.range(cond) {
        let node = &f.law.nodes[at];
        if matches!(node.op, Op::Const(_)) || found.iter().any(|(loc, _)| *loc == node.loc) {
            continue;
        }
        found.push((node.loc, show::value(f.book, f.ctx.day, f.values[at])));
    }
    found
}

/// When the condition is `lhs <= rhs` (or `<`, `>=`, `>`) and `lhs` moves
/// one-for-one with this flow, how much would satisfy it.
fn suggestion(f: &Frame, step: usize, cond: NodeId) -> Option<String> {
    let Op::Bin(op, lhs, rhs) = f.law.nodes[cond.index()].op else { return None };
    let (upper, strict) = match op {
        BinOp::Le => (true, false),
        BinOp::Lt => (true, true),
        BinOp::Ge => (false, false),
        BinOp::Gt => (false, true),
        _ => return None,
    };
    let moves = follows_flow(f, step, lhs)?;
    let (left, right, flow, unit) = in_one_unit(f, lhs, rhs)?;
    let show = |qty: Qty| f.book.show(Amount::new(qty, unit)).to_string();
    // How far the left side is beyond the bound, or short of it (one quantum
    // more when the bound is strict).
    let off = if upper { left - right } else { right - left } + Qty(i64::from(strict));
    Some(match (moves, upper) {
        (Moves::Flow, true) if flow > off => format!("this flow can be at most {}", show(flow - off)),
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

/// Both sides of the comparison and the flow's amount, in the bound's unit.
fn in_one_unit(f: &Frame, lhs: NodeId, rhs: NodeId) -> Option<(Qty, Qty, Qty, Id<Commodity>)> {
    let (left, right) = match (f.values[lhs.index()], f.values[rhs.index()]) {
        (Value::Amount(l), Value::Amount(r)) => (l, r),
        (Value::Amount(l), Value::Empty) => (l, Amount::zero(l.unit)),
        _ => return None,
    };
    let calc = Calc { book: f.book, day: f.ctx.day };
    let flow = calc.convert(f.ctx.amount?, right.unit).ok()?.qty;
    Some((calc.convert(left, right.unit).ok()?.qty, right.qty, flow, right.unit))
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

/// An assertion that does not hold: the gap, and the flows since the last time
/// it did (`since`, or the start of the book). `held` is the place's balance in
/// the sign the assertion is written in.
pub(crate) fn mismatch(book: &Book, events: &Events, assert: &Assert, held: Qty, since: Option<Day>) -> Diagnostic {
    const SHOWN: usize = 8;
    let (place, unit, day) = (show::place(book, assert.place), assert.amount.unit, assert.day);
    let money = |qty: Qty| book.show(Amount::new(qty, unit)).to_string();
    let gap = assert.amount.qty - held;
    let (size, verdict) = if gap > Qty::ZERO {
        (gap, "missing: the ledger holds less")
    } else {
        (-gap, "too much: the ledger holds more")
    };
    let mut d =
        Diagnostic::error("assertion", format!("{place} holds {}, not {}", money(held), money(assert.amount.qty)))
            .label(assert.loc, format!("{} {verdict} than this", money(size)));

    let flows = &book.touching[assert.place];
    let from = flows.partition_point(|&id| since.is_some_and(|last| book.flows[id].day <= last));
    let to = flows.partition_point(|&id| book.flows[id].day <= day);
    let (mut real, mut pending) = (Vec::new(), Vec::new());
    for &id in &flows[from..to] {
        let flow = &book.flows[id];
        let (moved, peer, inflow) =
            if flow.to == assert.place { (flow.arrive, flow.from, true) } else { (flow.out, flow.to, false) };
        if moved.unit != unit {
            continue;
        }
        // Signed the way the assertion is written: `+` raises the shown balance.
        let sign = if inflow == (book.places[assert.place].class.display_sign() > 0) { '+' } else { '-' };
        let state = events.state(id, flow);
        let text =
            format!("{sign}{} {} {}", money(moved.qty), if inflow { "from" } else { "to" }, show::place(book, peer));
        if state.is_real_on(day) {
            real.push((flow.loc, text));
        } else if state.is_pending_on(day) {
            pending.push(text);
        }
    }
    let hidden = real.len().saturating_sub(SHOWN);
    for (loc, text) in real.into_iter().skip(hidden) {
        d = d.context(loc, text);
    }
    let window = since.map_or("the start of the book".to_owned(), |last| format!("the assertion that held on {last}"));
    if hidden > 0 {
        d = d.note(format!("{hidden} earlier flows since {window} are not shown"));
    }
    if !pending.is_empty() {
        d = d.note(format!("not counted, because still pending: {}", pending.join(", ")));
    }
    let end = assert.loc.end;
    d.fix(
        "if the gap is a genuine externality (a missed transaction), accept it explicitly",
        Loc::new(assert.loc.file, end, end),
        " !",
    )
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
    let mut d = Diagnostic::info("pad", format!("accepted a gap on {place}")).label(assert.loc, what);
    if let Some(reason) = waive.reason {
        d = d.note(format!("accepted because: {}", book.name(reason)));
    }
    d
}

/// Lots that differ, and no rule to choose between them: each candidate, and
/// the gain the flow would realize if it came from that lot alone.
pub(crate) fn ambiguous(
    book: &Book,
    m: &Motion,
    holding: &Holding,
    candidates: &[Candidate],
    proceeds: Option<Qty>,
) -> Diagnostic {
    let (place, unit) = (show::place(book, m.from), m.out.unit);
    let money = |qty: Qty| book.show(Amount::new(qty, unit)).to_string();
    let base = |qty: Qty| book.show(Amount::new(qty, book.base)).to_string();
    let headline = format!("ambiguous lot: {place} holds {} lots that differ and no policy applies", candidates.len());
    let mut d = Diagnostic::error("ambiguous-lots", headline)
        .label(m.loc, format!("{} could come from any of them", money(m.out.qty)));
    for candidate in candidates {
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
        d = match candidate.source {
            Source::Lot(at) => match book.txns.get(holding.lots[at].txn) {
                Some(txn) => d.context(txn.loc, text),
                None => d.note(text),
            },
            Source::Plain => d.note(format!("plain money: {}", money(candidate.qty))),
        };
    }
    let lot = candidates
        .iter()
        .find(|c| matches!(c.source, Source::Lot(_)))
        .map_or(String::new(), |c| format!("name the lot, `{place}[{}]`, or ", c.acquired));
    d.note("the sale still moves the oldest lot first, so the lines after it stay consistent")
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
