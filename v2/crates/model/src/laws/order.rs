//! The order laws run in: a law that reads `tally(x)` runs after every law that
//! counts into `x`, whichever file either is written in.
//!
//! Only laws that fire on the same occasion depend on one another: what one
//! flow triggers, or what one period's end triggers. Everything else is
//! ordered by time. Among laws with no dependency between them the order laws
//! were declared in decides, and a set of laws that each wait for another is an
//! error naming them.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use axiom_core::{Diagnostic, Map, Sym};

use crate::book::Book;
use crate::law::{Effect, Func, Law, Op, StepKind, Trigger};

/// Which laws fire together: each trigger of a flow is its own occasion, and
/// every law of a period's end is one.
fn occasion(trigger: Trigger) -> u8 {
    match trigger {
        Trigger::In => 0,
        Trigger::Out => 1,
        Trigger::Gain => 2,
        Trigger::Spend => 3,
        Trigger::Always => 4,
        Trigger::Flow => 5,
        Trigger::Each(..) | Trigger::By(_) => 6,
    }
}

/// The tallies a law reads.
fn reads(law: &Law) -> Vec<Sym> {
    let tallies = law.nodes.iter().filter_map(|node| match node.op {
        Op::Call(Func::Tally(name), _) => Some(name),
        _ => None,
    });
    tallies.collect()
}

/// The tallies a law counts into. An `owe`, a `consume` and a `carry` count into nothing.
fn writes(law: &Law) -> Vec<Sym> {
    let counted = |effect: &Effect| match effect {
        Effect::Count { name, .. } => Some(*name),
        Effect::Owe { .. } | Effect::Consume { .. } | Effect::Carry { .. } => None,
    };
    let names = law.steps.iter().filter_map(|step| match &step.kind {
        StepKind::Effect(effect) | StepKind::Require { otherwise: Some(effect), .. } => counted(effect),
        _ => None,
    });
    names.collect()
}

/// Each law's place in the order, by index: dependencies first, declaration
/// order deciding the rest.
pub(crate) fn rank(book: &Book, diags: &mut Vec<Diagnostic>) -> Vec<u32> {
    let laws = book.laws.as_slice();
    let (reads, writes): (Vec<_>, Vec<_>) = laws.iter().map(|law| (reads(law), writes(law))).unzip();
    let mut writers: Map<Sym, Vec<usize>> = Map::default();
    for (at, names) in writes.iter().enumerate() {
        names.iter().for_each(|&name| writers.entry(name).or_default().push(at));
    }
    // `after[a]` are the laws that must run after law `a`.
    let mut after: Vec<Vec<usize>> = vec![Vec::new(); laws.len()];
    let mut waiting = vec![0usize; laws.len()];
    for (reader, names) in reads.iter().enumerate() {
        for writer in names.iter().flat_map(|name| writers.get(name).into_iter().flatten().copied()) {
            let together = occasion(laws[writer].trigger) == occasion(laws[reader].trigger);
            if together && writer != reader && !after[writer].contains(&reader) {
                after[writer].push(reader);
                waiting[reader] += 1;
            }
        }
    }
    let mut ready: BinaryHeap<Reverse<usize>> = (0..laws.len()).filter(|&law| waiting[law] == 0).map(Reverse).collect();
    let mut rank = vec![u32::MAX; laws.len()];
    let mut placed = 0u32;
    while let Some(Reverse(law)) = ready.pop() {
        (rank[law], placed) = (placed, placed + 1);
        for &next in &after[law] {
            waiting[next] -= 1;
            if waiting[next] == 0 {
                ready.push(Reverse(next));
            }
        }
    }
    // What is left waits on itself, or behind something that does. Those laws
    // run last, in the order declared.
    let stuck: Vec<usize> = (0..laws.len()).filter(|&law| rank[law] == u32::MAX).collect();
    for cycle in cycles(&after, stuck.clone()) {
        diags.push(cycle_diagnostic(book, &cycle, &reads, &writes));
    }
    for law in stuck {
        (rank[law], placed) = (placed, placed + 1);
    }
    rank
}

/// The loops among laws that could not be ordered, each in declaration order.
fn cycles(after: &[Vec<usize>], mut stuck: Vec<usize>) -> Vec<Vec<usize>> {
    let mut found = Vec::new();
    loop {
        // A law nothing else here waits for is only behind a loop, not in one.
        while let Some(at) = stuck.iter().position(|&law| after[law].iter().all(|next| !stuck.contains(next))) {
            stuck.remove(at);
        }
        let Some(&start) = stuck.first() else { return found };
        // Every law that is left waits for another, so following them repeats.
        let mut path = vec![start];
        let mut cycle = loop {
            let Some(next) = after[path[path.len() - 1]].iter().copied().find(|next| stuck.contains(next)) else {
                break path;
            };
            match path.iter().position(|&law| law == next) {
                Some(at) => break path.split_off(at),
                None => path.push(next),
            }
        };
        stuck.retain(|law| !cycle.contains(law));
        cycle.sort_unstable();
        found.push(cycle);
    }
}

fn cycle_diagnostic(book: &Book, members: &[usize], reads: &[Vec<Sym>], writes: &[Vec<Sym>]) -> Diagnostic {
    let name = |at: usize| book.name(book.laws.as_slice()[at].name);
    let named: Vec<String> = members.iter().map(|&at| format!("`{}`", name(at))).collect();
    let mut diagnostic = Diagnostic::error(
        "law-cycle",
        format!("laws {} each wait for a tally another counts", named.join(", ")),
    )
    .note("a law that reads `tally(x)` runs after every law that counts into `x`, so laws that count into each other cannot be ordered")
    .help("count into a different tally, or move the reading out of the loop");
    for (n, &at) in members.iter().enumerate() {
        // What this law waits for, and who counts it.
        let waits = reads[at].iter().find_map(|&tally| {
            let writer = members.iter().copied().find(|&other| other != at && writes[other].contains(&tally))?;
            Some((tally, writer))
        });
        let law = &book.laws.as_slice()[at];
        let text = match waits {
            Some((tally, writer)) => {
                format!("`{}` reads `{}`, which `{}` counts", name(at), book.name(tally), name(writer))
            }
            None => format!("`{}` is part of the loop", name(at)),
        };
        diagnostic = if n == 0 { diagnostic.label(law.loc, text) } else { diagnostic.context(law.loc, text) };
    }
    diagnostic
}
