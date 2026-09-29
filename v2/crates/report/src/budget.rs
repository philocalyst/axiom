//! `budget`: every `warn` law over a window total, spent against its limit.
//!
//! A budget is not a feature of its own: it is a law of the shape
//! `warn total(in, month) <= 500 USD`, and this view finds those laws and
//! evaluates their totals from the flows.

use std::borrow::Cow;

use axiom_core::{Day, Id, Qty, Ratio};
use axiom_engine::Run;
use axiom_model::{Amount, BinOp, Book, Dir, Func, Law, NodeId, Op, Owner, Period, Place, StepKind, Value, Window};

use crate::calendar::Periods;
use crate::history::Posting;
use crate::places::path;
use crate::value::Valuer;
use crate::{Cell, Column, Report, Row, Section, Style};

pub fn view<'s>(book: &Book<'s>, run: &Run, month: Option<Day>) -> Report<'s> {
    let at = month.unwrap_or(run.today);
    let mut section = Section::new([
        Column::left("Place"),
        Column::left("Law"),
        Column::left("Window"),
        Column::right("Spent"),
        Column::right("Limit"),
        Column::right("Left"),
        Column::right("Used"),
    ]);
    let valuer = Valuer::new(book, at);
    let mut unpriced = 0;
    for budget in budgets(book) {
        let spent = budget.spent(book, run, at);
        unpriced += spent.unpriced;
        section.push(row(book, &budget, at, &valuer, spent.total));
    }
    if section.rows.is_empty() {
        section.note("No budgets. A line like `budget 500 USD monthly` under an account makes one.");
    }
    if unpriced > 0 {
        section.note(format!("{unpriced} flows have no price on their day and are not counted."));
    }
    Report::new(format!("Budgets for {at}")).with(section)
}

/// A `warn` law bounding a window total under one place.
struct Budget {
    law: Id<Law>,
    /// The place whose subtree the total covers.
    subject: Id<Place>,
    dir: Dir,
    window: Window,
    /// `None` when the limit is an expression rather than a written amount.
    limit: Option<Amount>,
}

/// The budgets of a book, in tree order.
fn budgets(book: &Book) -> Vec<Budget> {
    let mut found = Vec::new();
    for (law_id, law) in book.laws.iter() {
        let Some((dir, window, limit)) = shape(law) else { continue };
        let subjects: Vec<Id<Place>> = match law.owner {
            Owner::Place(place) => vec![place],
            Owner::Kind(kind) => {
                book.places.iter().filter(|(_, place)| book.is_a(place.kind, kind)).map(|(id, _)| id).collect()
            }
            Owner::Entity(_) | Owner::System(_) | Owner::Book => Vec::new(),
        };
        found.extend(subjects.into_iter().map(|subject| Budget { law: law_id, subject, dir, window, limit }));
    }
    found.sort_by_key(|budget| (budget.subject, budget.law));
    found
}

/// Recognizes `warn total(dir, window) <= limit` (or `limit >= total(…)`).
fn shape(law: &Law) -> Option<(Dir, Window, Option<Amount>)> {
    law.steps.iter().find_map(|step| match step.kind {
        StepKind::Require { cond, warn: true, .. } => bound(law, cond),
        _ => None,
    })
}

fn bound(law: &Law, cond: NodeId) -> Option<(Dir, Window, Option<Amount>)> {
    let Op::Bin(op, left, right) = &law.nodes[cond.index()].op else { return None };
    let (total, limit) = match op {
        BinOp::Le | BinOp::Lt => (left, right),
        BinOp::Ge | BinOp::Gt => (right, left),
        _ => return None,
    };
    let Op::Call(Func::Total(dir, window), _) = &law.nodes[total.index()].op else { return None };
    let limit = match law.nodes[limit.index()].op {
        Op::Const(Value::Amount(amount)) => Some(amount),
        _ => None,
    };
    Some((*dir, *window, limit))
}

struct Spent {
    total: Qty,
    unpriced: usize,
}

impl Budget {
    /// What crossed the boundary of the subject's subtree in the window that
    /// contains `at`, in the base currency. A flow inside the subtree does not
    /// count as entering or leaving it.
    fn spent(&self, book: &Book, run: &Run, at: Day) -> Spent {
        let (start, end) = window_bounds(self.window, at);
        let cutoff = end.min(run.today);
        let mut spent = Spent { total: Qty::ZERO, unpriced: 0 };
        for place in book.places.subtree(self.subject) {
            for &id in &book.touching[place] {
                let posting = Posting::at(book, run, id);
                let flow = posting.flow;
                if !posting.is_real_on(cutoff) || flow.day < start || flow.day > end {
                    continue;
                }
                let (near, far, value) = match self.dir {
                    Dir::In => (flow.to, flow.from, posting.arrive_in_base(book)),
                    Dir::Out => (flow.from, flow.to, posting.out_in_base(book)),
                };
                if near != place || book.places.covers(self.subject, far) {
                    continue;
                }
                match value {
                    Some(qty) => spent.total += qty,
                    None => spent.unpriced += 1,
                }
            }
        }
        spent
    }
}

fn window_bounds(window: Window, at: Day) -> (Day, Day) {
    match window {
        Window::Month => (at.month_start(), at.month_end()),
        Window::Year => (at.year_start(), at.year_end()),
        Window::Ever => (Day(i32::MIN), Day(i32::MAX)),
    }
}

fn window_title(window: Window, at: Day) -> Cow<'static, str> {
    match window {
        Window::Month => Periods::covering(Period::Month, at, at).title(0).into(),
        Window::Year => Periods::covering(Period::Year, at, at).title(0).into(),
        Window::Ever => "all time".into(),
    }
}

fn row<'s>(book: &Book<'s>, budget: &Budget, at: Day, valuer: &Valuer, spent: Qty) -> Row<'s> {
    let limit = budget.limit.and_then(|limit| valuer.qty(limit));
    let over = limit.is_some_and(|limit| spent > limit);
    let used = limit.and_then(|limit| Ratio::new(spent.0.into(), limit.0.into()));
    let cells = [
        Cell::text(path(book, budget.subject)),
        Cell::text(book.name(book.laws[budget.law].name)),
        Cell::text(window_title(budget.window, at)),
        Cell::base(book, spent),
        limit.map_or(Cell::Blank, |limit| Cell::base(book, limit)),
        limit.map_or(Cell::Blank, |limit| Cell::base(book, limit - spent)),
        used.map_or(Cell::Blank, Cell::Percent),
    ];
    Row::new(cells).style(if over { Style::Alert } else { Style::Normal })
}
