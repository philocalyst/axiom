//! `available`: how much can actually be spent, what is coming in, and what it
//! would cost to reach the rest.
//!
//! Money in hand is the easy half: cash in every currency, less what is
//! tied to someone, what is written but not cashed, and what falls due within
//! a month. For everything else the question is asked of the laws: draw the
//! whole holding down into a cash place, on a fork of the ledger, and see what
//! the laws would then owe once the year is judged (its end, or the day its
//! return closes) beyond what they already will. A 401k's penalty, the income
//! it creates and the tax on that income all come out of the rules of the
//! 401k's own system, with no special case here. Liquidity is derived from law.

use std::collections::BTreeMap;

use axiom_core::{Day, Id, Qty, Span, Sym, par};
use axiom_engine::{Effect, Holding, Ledger, Options, Plan, Run, Verdict};
use axiom_model::{Amount, Book, Class, Entity, Place};

use crate::claims::{self, Claim};
use crate::closings;
use crate::history::{Held, postings};
use crate::lens::{Basket, Lens, Liquidity, Whose};
use crate::places::path;
use crate::synth::hypothetical;
use crate::table::{headline, plural};
use crate::{Cell, Column, Report, Row, Section, Style};

/// Obligations falling due within this long count against what can be spent.
const SOON: Span = Span::days(30);

pub fn view<'s>(book: &'s Book<'_>, run: &Run, whose: &Whose, at: Option<Day>) -> Report<'s> {
    let at = at.unwrap_or(run.today);
    let lens = Lens::new(book, whose, at);
    // Deadlines fire up to the day the year is judged, so the laws that figure
    // its tax answer too, whether they run at its end or on a closing day.
    let horizon = closings::judged_through(book, at);
    let plan = Plan::new(book);
    let mut ledger = plan.start(Options { today: horizon.max(run.today), relaxed: book.relaxed });
    // A withdrawal is a fact of `at`, so it comes before what closes that day (a month's or a year's end).
    ledger.advance_to_closing(at);
    from_ledger(lens, run, &ledger, horizon)
}

/// Builds the view from the shared report context's checkpoint fork.
pub(crate) fn from_ledger<'s>(lens: Lens<'s, '_, '_, '_>, run: &Run, ledger: &Ledger, horizon: Day) -> Report<'s> {
    let (book, at) = (lens.book, lens.day);

    let holdings: Vec<&Holding> = ledger.holdings().collect();
    let (mut cash, mut slow) = (Vec::new(), Vec::new());
    for &holding in holdings.iter().filter(|holding| lens.owns(holding.place)) {
        match lens.liquidity(holding.place, holding.unit) {
            Some(Liquidity::Cash) => cash.push(holding),
            Some(Liquidity::Slow(span)) => slow.push((holding, span)),
            Some(Liquidity::Claim) | None => {}
        }
    }
    let claims = claims::open(lens, run, holdings.iter().copied());
    let to = cash.iter().filter(|holding| holding.unit == book.base).max_by_key(|holding| holding.qty());
    let to = to.map(|holding| holding.place);

    let mut baseline = ledger.fork();
    baseline.advance(horizon);
    let baseline = owing(lens, baseline.recorded().effects);
    let reach =
        par::map_each(&slow, |&(holding, span)| Reach::of(&ledger, lens, holding, span, to, &baseline, horizon));

    let mine: Vec<&Claim> = claims.iter().filter(|claim| claim.mine).collect();
    Report::new(format!("Available on {at}"))
        .with(spendable_section(lens, run, &cash, &claims))
        .with(claims::section(lens, "Coming in", &mine))
        .with(reach_section(lens, &reach, to, horizon))
}

// ─── What you can spend ─────────────────────────────────────────────────────

fn spendable_section<'s>(lens: Lens<'s, '_, '_, '_>, run: &Run, cash: &[&Holding], claims: &[Claim]) -> Section<'s> {
    let (book, at) = (lens.book, lens.day);
    let mut section = Section::new([Column::left("In hand"), Column::right("Amount")]).headed("What you can spend");
    let mut unpriced = 0;
    let mut line = |section: &mut Section<'s>, label: String, worth: Option<Qty>, depth: usize| match worth {
        Some(qty) => {
            let style = if depth > 0 { Style::Muted } else { Style::Normal };
            section.push(Row::new([Cell::text(label), Cell::base(book, qty)]).depth(depth).style(style));
        }
        None => unpriced += 1,
    };

    // Money in hand: every currency, each priced as a whole.
    let mut in_hand = Basket::default();
    let mut tied: BTreeMap<Id<Entity>, Basket> = BTreeMap::new();
    for holding in cash {
        in_hand.add(holding.unit, Held { qty: holding.qty(), booked: Qty::ZERO });
        for (lot, entity) in holding.lots.iter().filter_map(|lot| Some((lot, lot.tied?))) {
            tied.entry(entity).or_default().add(holding.unit, Held { qty: lot.qty, booked: Qty::ZERO });
        }
    }
    let hands = in_hand.value(lens, Class::Asset);
    let mut spendable = hands.total;
    line(&mut section, "Money in hand".into(), Some(hands.total), 0);
    for holding in cash {
        line(&mut section, place_label(lens, holding), lens.value(Amount::new(holding.qty(), holding.unit)), 1);
    }

    // What is spoken for: held for someone else, written but not cashed, due soon.
    let held = tied.iter().map(|(&entity, basket)| {
        let whom = book.name(book.entities[entity].path);
        (format!("held for {whom}"), basket.value(lens, Class::Asset).total)
    });
    let pending = postings(book, run)
        .filter(|posting| {
            let from = posting.flow.from;
            posting.is_pending_on(at)
                && lens.owns(from)
                && lens.liquidity(from, posting.flow.out.unit) == Some(Liquidity::Cash)
        })
        .filter_map(|posting| {
            Some((format!("{} to {}", posting.flow.day, path(book, posting.flow.to)), posting.out_in_base(lens)?))
        });
    let spoken_for = [
        ("Held for others", held.collect::<Vec<_>>()),
        ("Pending outflows", pending.collect()),
        ("Due within 30 days", due_soon(lens, run, claims)),
    ];
    for (heading, items) in spoken_for {
        let total: Qty = items.iter().map(|(_, qty)| *qty).sum();
        if !total.is_zero() {
            spendable -= total;
            line(&mut section, heading.into(), Some(-total), 0);
            for (label, qty) in items {
                line(&mut section, label, Some(-qty), 1);
            }
        }
    }
    section.push(Row::new([Cell::text("Available to spend"), Cell::base(book, spendable)]).style(Style::Total));
    if unpriced > 0 {
        section.note(format!("{} have no price and are left out.", plural(unpriced, "amount")));
    }
    section
}

/// A holding's place, with its own amount when it is not in the base currency.
fn place_label(lens: Lens, holding: &Holding) -> String {
    let book = lens.book;
    match holding.unit == book.base {
        true => path(book, holding.place).to_string(),
        false => format!("{} ({})", path(book, holding.place), book.show(Amount::new(holding.qty(), holding.unit))),
    }
}

/// What falls due within a month: obligations the laws recorded, and debts
/// with a due day.
fn due_soon(lens: Lens, run: &Run, claims: &[Claim]) -> Vec<(String, Qty)> {
    let (book, at) = (lens.book, lens.day);
    let soon = |day: Day| day >= at && day <= at.add(SOON);
    let recorded = run.effects.iter().filter(|effect| effect.day <= at && lens.whose.includes(effect.owner));
    let owed = recorded.filter_map(|effect: &Effect| {
        let owed = effect.owed().filter(|owed| soon(owed.due))?;
        let label =
            format!("{}, to {} by {}", book.name(effect.name), book.name(book.entities[owed.to].path), owed.due);
        Some((label, lens.value(effect.amount)?))
    });
    let debts = claims.iter().filter(|claim| !claim.mine && claim.due.is_some_and(soon)).filter_map(|claim| {
        let label = format!("{} by {}", claim.counterparty(book), claim.due?);
        Some((label, lens.value(claim.left)?))
    });
    owed.chain(debts).collect()
}

// ─── Everything else ────────────────────────────────────────────────────────

/// What the laws owe, in the base currency: by name, creditor and due day.
type Owing = BTreeMap<(Sym, Id<Entity>, Day), Qty>;

fn owing(lens: Lens, effects: &[Effect]) -> Owing {
    let mut owing = Owing::new();
    for effect in effects {
        if let (Some(owed), Some(qty)) = (effect.owed(), lens.value(effect.amount)) {
            *owing.entry((effect.name, owed.to, owed.due)).or_default() += qty;
        }
    }
    owing
}

/// A holding that is not spendable cash, and what the laws would take for
/// drawing it down.
struct Reach<'h> {
    holding: &'h Holding,
    /// How long turning it into cash takes.
    liquid_in: Span,
    /// In the base currency at the day's prices.
    value: Option<Qty>,
    /// Penalties and taxes owed beyond what the year would owe anyway.
    cost: Qty,
    /// What drives the cost, or why nothing could be worked out.
    because: String,
    /// A law forbids the withdrawal.
    blocked: bool,
}

impl<'h> Reach<'h> {
    /// Forks the ledger, moves the whole holding into `to` as one flow, runs
    /// on to `horizon` (the day the year is judged), and reads what is owed
    /// that would not be otherwise. The baseline is a fork run out over the
    /// same horizon, so what the year would bring anyway is not the
    /// withdrawal's cost.
    fn of(
        ledger: &Ledger,
        lens: Lens,
        holding: &'h Holding,
        liquid_in: Span,
        to: Option<Id<Place>>,
        baseline: &Owing,
        horizon: Day,
    ) -> Reach<'h> {
        let book = lens.book;
        let held = Amount::new(holding.qty(), holding.unit);
        let value = lens.value(held);
        let mut reach = Reach { holding, liquid_in, value, cost: Qty::ZERO, because: String::new(), blocked: false };
        let Some(cash_in) = value else {
            reach.because = format!("no price for {}: a withdrawal cannot be valued", book.show(held));
            return reach;
        };
        let Some(to) = to else { return reach };
        // The hypothetical flow borrows a real one's transaction and source line.
        let Some(&template) = book.touching[holding.place].last() else { return reach };
        let flow =
            hypothetical(&book.flows[template], lens.day, holding.place, to, held, Amount::new(cash_in, book.base));

        let mut fork = ledger.fork();
        let applied = fork.apply(&flow);
        fork.advance(horizon);
        let recorded = fork.recorded();
        let mut forbidden =
            recorded.violations[applied.violations].iter().filter(|v| v.verdict == Verdict::Blocks);
        if let Some(violation) = forbidden.next() {
            let message = &recorded.diagnostics[violation.diagnostic as usize].message;
            (reach.blocked, reach.because) = (true, format!("blocked: {}", headline(message)));
            return reach;
        }

        // What the laws owe now that they did not owe before, by name, biggest first.
        let mut drivers: BTreeMap<Sym, Qty> = BTreeMap::new();
        for (&(name, to, due), &qty) in &owing(lens, recorded.effects) {
            *drivers.entry(name).or_default() += qty - baseline.get(&(name, to, due)).copied().unwrap_or_default();
        }
        let mut drivers: Vec<(Sym, Qty)> = drivers.into_iter().filter(|(_, qty)| qty.0 > 0).collect();
        drivers.sort_by_key(|&(_, qty)| -qty.0);
        reach.cost = drivers.iter().map(|&(_, qty)| qty).sum();
        let show =
            |&(name, qty): &(Sym, Qty)| format!("{} {}", book.name(name), book.show(Amount::new(qty, book.base)));
        let named: Vec<String> = drivers.iter().take(3).map(show).collect();
        if !named.is_empty() {
            reach.because = format!("driven by {}", named.join(", "));
        }
        reach
    }
}

/// One line per holding that is not cash. `judged` is the day the books were
/// run on to.
fn reach_section<'s>(lens: Lens<'s, '_, '_, '_>, reach: &[Reach], to: Option<Id<Place>>, judged: Day) -> Section<'s> {
    let (book, at) = (lens.book, lens.day);
    let columns = ["Holding", "Liquid in"].map(Column::left).into_iter();
    let columns = columns.chain(["Value", "Cost", "Net"].map(Column::right)).chain([Column::left("Because")]);
    let mut section = Section::new(columns).headed("What it would take to reach the rest");
    let (mut value, mut costs) = (Qty::ZERO, Qty::ZERO);
    for row in reach {
        let liquid_in = if row.liquid_in == Span::default() { "now".to_string() } else { row.liquid_in.to_string() };
        let reachable = row.value.filter(|_| to.is_some() && !row.blocked);
        if let Some(worth) = reachable {
            (value, costs) = (value + worth, costs + row.cost);
        }
        let cells = [
            Cell::text(place_label(lens, row.holding)),
            Cell::text(liquid_in),
            row.value.map_or(Cell::Blank, |worth| Cell::base(book, worth)),
            Cell::base_or_blank(book, row.cost),
            reachable.map_or(Cell::Blank, |worth| Cell::base(book, worth - row.cost)),
            Cell::text(row.because.clone()),
        ];
        let style = if row.blocked {
            Style::Alert
        } else if row.value.is_none() {
            Style::Muted
        } else {
            Style::Normal
        };
        section.push(Row::new(cells).style(style));
    }
    if section.rows.is_empty() {
        return section;
    }
    let total = [Cell::text("If everything were drawn today"), Cell::Blank];
    let sums = [value, costs, value - costs].map(|qty| Cell::base(book, qty));
    section.push(Row::padded(total.into_iter().chain(sums), 6).style(Style::Total));
    match to {
        Some(to) => section.note(format!(
            "Each line withdraws the whole holding into {} on {at} and runs {} through the laws. Cost is \
             what they would then owe beyond what they already will: penalties, and the tax on any income it creates.",
            path(book, to),
            if judged > at.year_end() {
                format!("the books on to {judged}, when the year's return closes,")
            } else {
                "the year out".to_string()
            }
        )),
        None => section.note("No cash place holds money, so there is nowhere to withdraw into."),
    }
    section
}
