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
use axiom_engine::{Effect, Holding, Ledger, Options};
use axiom_model::{Amount, Class, Entity, Place};

use crate::claims::{self, Claim};
use crate::closings;
use crate::history::{Held, postings};
use crate::lens::{Basket, Lens, Liquidity, Priced};
use crate::places::path;
use crate::synth::hypothetical;
use crate::{Cell, Column, Money, Report, Row, Section, Style, When};

/// Obligations falling due within this long count against what can be spent.
const SOON: Span = Span::days(30);

pub fn view<'s>(lens: Lens<'_, 's>, at: Option<Day>) -> Report<'s> {
    let (book, run) = (lens.book, lens.run);
    let at = at.unwrap_or(lens.day);
    let lens = lens.on(at);
    // Deadlines fire up to the day the year is judged, so the laws that figure
    // its tax answer too, whether they run at its end or on a closing day.
    let horizon = closings::judged_through(book, at);
    let mut ledger = Ledger::new(book, Options { today: horizon.max(run.today), relaxed: book.relaxed });
    // A withdrawal is a fact of `at`, so it comes before what closes that day (a month's or a year's end).
    ledger.advance_to_closing(at);

    let holdings: Vec<&Holding> = ledger.holdings().collect();
    let (mut cash, mut slow) = (Vec::new(), Vec::new());
    for &holding in holdings.iter().filter(|holding| lens.owns(holding.place)) {
        match lens.liquidity(holding.place, holding.unit) {
            Some(Liquidity::Cash) => cash.push(holding),
            Some(Liquidity::Slow(span)) => slow.push((holding, span)),
            Some(Liquidity::Claim) | None => {}
        }
    }
    let claims = claims::open(lens, holdings.iter().copied());
    let to = cash.iter().filter(|holding| holding.unit == book.base).max_by_key(|holding| holding.qty());
    let to = to.map(|holding| holding.place);

    let mut baseline = ledger.fork();
    baseline.advance(horizon);
    let baseline = owing(lens, baseline.recorded().effects);
    let reach =
        par::map_each(&slow, |&(holding, span)| Reach::of(&ledger, lens, holding, span, to, &baseline, horizon));

    let mine: Vec<&Claim> = claims.iter().filter(|claim| claim.mine).collect();
    Report::new(["Available on".into(), Cell::Day(at)])
        .with(spendable_section(lens, &cash, &claims))
        .with(claims::section(lens, "Coming in", &mine))
        .with(reach_section(lens, reach, to, horizon))
}

// ─── What you can spend ─────────────────────────────────────────────────────

fn spendable_section<'s>(lens: Lens<'_, 's>, cash: &[&Holding], claims: &[Claim]) -> Section<'s> {
    let (book, run, at) = (lens.book, lens.run, lens.day);
    let mut section = Section::new([Column::left("In hand"), Column::right("Amount")]).headed("What you can spend");
    let mut priced = Priced::default();
    let mut line = |section: &mut Section<'s>, label: Cell<'s>, worth: Option<Qty>, depth: usize| {
        if let Some(qty) = priced.add(worth) {
            let style = if depth > 0 { Style::Muted } else { Style::Normal };
            section.push(Row::new([label, Cell::base(book, qty)]).depth(depth).style(style));
        }
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
        let whom = Cell::Name(book.name(book.entities[entity].path));
        (["held for".into(), whom].into(), basket.value(lens, Class::Asset).total)
    });
    let pending = postings(book, run)
        .filter(|posting| {
            let from = posting.flow.from;
            posting.is_pending_on(at)
                && lens.owns(from)
                && lens.liquidity(from, posting.flow.out.unit) == Some(Liquidity::Cash)
        })
        .filter_map(|posting| {
            let label = [Cell::Day(posting.flow.day), "to".into(), Cell::Name(path(book, posting.flow.to))].into();
            Some((label, posting.out_in_base(lens)?))
        });
    let spoken_for = [
        ("Held for others", held.collect::<Vec<_>>()),
        ("Pending outflows", pending.collect()),
        ("Due within 30 days", due_soon(lens, claims)),
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
    section.total(["Available to spend".into(), Cell::base(book, spendable)]);
    section.fact("available", None, lens.whose.label(book), When::Instant(at), Money::base(book, spendable));
    section.unpriced(priced.missing(), "amount");
    section
}

/// A holding's place, with its own amount when it is not in the base currency.
fn place_label<'s>(lens: Lens<'_, 's>, holding: &Holding) -> Cell<'s> {
    let book = lens.book;
    let place = Cell::Name(path(book, holding.place));
    match holding.unit == book.base {
        true => place,
        false => {
            let own = Cell::amount(book, Amount::new(holding.qty(), holding.unit));
            [place, Cell::Join("", vec!["(".into(), own, ")".into()])].into()
        }
    }
}

/// What falls due within a month: obligations the laws recorded, and debts
/// with a due day.
fn due_soon<'s>(lens: Lens<'_, 's>, claims: &[Claim]) -> Vec<(Cell<'s>, Qty)> {
    let (book, at) = (lens.book, lens.day);
    let soon = |day: Day| day >= at && day <= at.add(SOON);
    let recorded = lens.run.effects.iter().filter(|effect| effect.day <= at && lens.whose.includes(effect.owner));
    let owed = recorded.filter_map(|effect: &Effect| {
        let owed = effect.owe.filter(|owed| soon(owed.due))?;
        let to = Cell::Name(book.name(book.entities[owed.to].path));
        let label = [Cell::Name(book.name(effect.name)), ", to".into(), to, "by".into(), Cell::Day(owed.due)];
        Some((label.into(), lens.value(effect.amount)?))
    });
    let debts = claims.iter().filter(|claim| !claim.mine && claim.due.is_some_and(soon)).filter_map(|claim| {
        let label = [Cell::Name(claim.counterparty(book)), "by".into(), Cell::Day(claim.due?)];
        Some((label.into(), lens.value(claim.left)?))
    });
    owed.chain(debts).collect()
}

// ─── Everything else ────────────────────────────────────────────────────────

/// What the laws owe, in the base currency: by name, creditor and due day.
type Owing = BTreeMap<(Sym, Id<Entity>, Day), Qty>;

fn owing(lens: Lens, effects: &[Effect]) -> Owing {
    let mut owing = Owing::new();
    for effect in effects {
        if let (Some(owed), Some(qty)) = (effect.owe, lens.value(effect.amount)) {
            *owing.entry((effect.name, owed.to, owed.due)).or_default() += qty;
        }
    }
    owing
}

/// A holding that is not spendable cash, and what the laws would take for
/// drawing it down.
struct Reach<'h, 's> {
    holding: &'h Holding,
    /// How long turning it into cash takes.
    liquid_in: Span,
    /// In the base currency at the day's prices.
    value: Option<Qty>,
    /// Penalties and taxes owed beyond what the year would owe anyway.
    cost: Qty,
    /// What drives the cost, or why nothing could be worked out.
    because: Cell<'s>,
    /// A law forbids the withdrawal.
    blocked: bool,
}

impl<'h, 's> Reach<'h, 's> {
    /// Forks the ledger, moves the whole holding into `to` as one flow, runs
    /// on to `horizon` (the day the year is judged), and reads what is owed
    /// that would not be otherwise. The baseline is a fork run out over the
    /// same horizon, so what the year would bring anyway is not the
    /// withdrawal's cost.
    fn of(
        ledger: &Ledger,
        lens: Lens<'_, 's>,
        holding: &'h Holding,
        liquid_in: Span,
        to: Option<Id<Place>>,
        baseline: &Owing,
        horizon: Day,
    ) -> Reach<'h, 's> {
        let book = lens.book;
        let held = Amount::new(holding.qty(), holding.unit);
        let value = lens.value(held);
        let mut reach = Reach { holding, liquid_in, value, cost: Qty::ZERO, because: Cell::Blank, blocked: false };
        let Some(cash_in) = value else {
            reach.because =
                ["no price for".into(), Cell::amount(book, held), ": a withdrawal cannot be valued".into()].into();
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
            recorded.violations[applied.violations].iter().filter(|v| !v.warn && !v.waived && !v.priced);
        if let Some(violation) = forbidden.next() {
            let message = &recorded.diagnostics[violation.diagnostic as usize].message;
            (reach.blocked, reach.because) = (true, ["blocked:".into(), Cell::headline(message)].into());
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
        let show = |&(name, qty): &(Sym, Qty)| [Cell::Name(book.name(name)), Cell::base(book, qty)].into();
        let named = Cell::list_or_blank(", ", drivers.iter().take(3).map(show));
        if !matches!(named, Cell::Blank) {
            reach.because = ["driven by".into(), named].into();
        }
        reach
    }
}

/// One line per holding that is not cash. `judged` is the day the books were
/// run on to.
fn reach_section<'s>(lens: Lens<'_, 's>, reach: Vec<Reach<'_, 's>>, to: Option<Id<Place>>, judged: Day) -> Section<'s> {
    let (book, at) = (lens.book, lens.day);
    let columns = ["Holding", "Liquid in"].map(Column::left).into_iter();
    let columns = columns.chain(["Value", "Cost", "Net"].map(Column::right)).chain([Column::left("Because")]);
    let mut section = Section::new(columns).headed("What it would take to reach the rest");
    let (mut value, mut costs) = (Qty::ZERO, Qty::ZERO);
    for row in reach {
        let liquid_in = if row.liquid_in == Span::default() { "now".into() } else { Cell::Span(row.liquid_in) };
        let reachable = row.value.filter(|_| to.is_some() && !row.blocked);
        if let Some(worth) = reachable {
            (value, costs) = (value + worth, costs + row.cost);
        }
        let cells = [
            place_label(lens, row.holding),
            liquid_in,
            row.value.map_or(Cell::Blank, |worth| Cell::base(book, worth)),
            Cell::base_or_blank(book, row.cost),
            reachable.map_or(Cell::Blank, |worth| Cell::base(book, worth - row.cost)),
            row.because,
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
    let total = ["If everything were drawn today".into(), Cell::Blank];
    let sums = [value, costs, value - costs].map(|qty| Cell::base(book, qty));
    section.total(total.into_iter().chain(sums));
    match to {
        Some(to) => {
            let ran = if judged > at.year_end() {
                ["the books on to".into(), Cell::Day(judged), ", when the year's return closes,".into()].into()
            } else {
                "the year out".into()
            };
            section.note([
                "Each line withdraws the whole holding into".into(),
                Cell::Name(path(book, to)),
                "on".into(),
                Cell::Day(at),
                "and runs".into(),
                ran,
                "through the laws. Cost is what they would then owe beyond what they already will: penalties, \
                 and the tax on any income it creates."
                    .into(),
            ]);
        }
        None => section.note("No cash place holds money, so there is nowhere to withdraw into."),
    }
    section
}
