//! `available`: how much can actually be spent, and what it would cost to reach the rest.
//!
//! Liquid money is the easy half. For everything else the question is asked
//! of the laws: draw the whole holding down into a liquid place, on a clone of
//! the ledger, and see what the laws do about it by the end of the year. A
//! 401k's penalty, the ordinary income it creates, and the tax that income
//! brings all come out of the rules of the 401k's own system, with no special
//! case here. Liquidity is derived from law.

use std::borrow::Cow;
use std::collections::BTreeMap;

use axiom_core::{Day, Id, Qty, Span, Sym, par};
use axiom_engine::{Holding, Ledger, Options, Run};
use axiom_model::{Amount, Book, Class, Commodity, Entity, Flow, Place};

use crate::history::postings;
use crate::places::{is_liquid, path};
use crate::synth::planned;
use crate::value::Valuer;
use crate::{Cell, Column, Report, Row, Section, Style};

pub fn view<'s>(book: &Book<'s>, run: &Run, at: Option<Day>) -> Report<'s> {
    let at = at.unwrap_or(run.today);
    let year_end = at.year_end();
    // Deadlines fire up to the end of the year, so year-end laws answer too.
    let mut ledger = Ledger::new(book, Options { today: year_end.max(run.today), relaxed: book.relaxed });
    ledger.advance(at);

    let holdings: Vec<&Holding> =
        ledger.holdings().filter(|holding| book.places[holding.place].class == Class::Asset).collect();
    let cash = Cash::gather(book, run, &holdings, at);
    let candidates = candidates(book, &holdings, at);
    let destination = cash.largest_place();
    let baseline = Recorded::by(book, &settled(ledger.clone(), year_end), at);
    let outcomes = par::map(&candidates, |candidate| Outcome::of(&ledger, candidate, destination, &baseline, at));

    let elsewhere = elsewhere_section(book, &candidates, &outcomes, destination, at);
    Report::new(format!("Available on {at}")).with(cash.section(book)).with(elsewhere)
}

/// Runs the ledger on to `day` and hands over everything it recorded.
fn settled(mut ledger: Ledger, day: Day) -> Run {
    ledger.advance(day);
    ledger.finish()
}

fn counts_as_cash(book: &Book, holding: &Holding) -> bool {
    holding.unit == book.base && is_liquid(book, holding.place)
}

// ─── Liquid now ─────────────────────────────────────────────────────────────

/// Money in liquid places, less what is spoken for.
struct Cash {
    places: Vec<(Id<Place>, Qty)>,
    /// Restricted money that landed in a liquid place stays tied to its source.
    tied: BTreeMap<Id<Entity>, Qty>,
    /// Written, not yet cashed: each still reduces what can be spent.
    pending: Vec<Pending>,
    unpriced_pending: usize,
}

struct Pending {
    day: Day,
    with: Id<Place>,
    qty: Qty,
}

impl Cash {
    fn gather(book: &Book, run: &Run, holdings: &[&Holding], at: Day) -> Cash {
        let mut cash = Cash { places: Vec::new(), tied: BTreeMap::new(), pending: Vec::new(), unpriced_pending: 0 };
        for holding in holdings.iter().filter(|holding| counts_as_cash(book, holding)) {
            cash.places.push((holding.place, holding.qty()));
            for lot in &holding.lots {
                if let Some(entity) = lot.tied {
                    *cash.tied.entry(entity).or_default() += lot.qty;
                }
            }
        }
        let outflows =
            postings(book, run).filter(|posting| posting.is_pending_on(at) && is_liquid(book, posting.flow.from));
        for posting in outflows {
            match posting.out_in_base(book) {
                Some(qty) => cash.pending.push(Pending { day: posting.flow.day, with: posting.flow.to, qty }),
                None => cash.unpriced_pending += 1,
            }
        }
        cash
    }

    /// Where a withdrawal from somewhere else would land: the liquid place
    /// holding the most.
    fn largest_place(&self) -> Option<Id<Place>> {
        self.places.iter().max_by_key(|(_, qty)| *qty).map(|&(place, _)| place)
    }

    fn held(&self) -> Qty {
        self.places.iter().map(|&(_, qty)| qty).sum()
    }

    fn tied_total(&self) -> Qty {
        self.tied.values().copied().sum()
    }

    fn pending_total(&self) -> Qty {
        self.pending.iter().map(|pending| pending.qty).sum()
    }

    fn section<'s>(&self, book: &Book<'s>) -> Section<'s> {
        let line = |label: String, qty: Qty| Row::new([Cell::text(label), Cell::base(book, qty)]);
        let mut section =
            Section::new([Column::left("Liquid now"), Column::right("Amount")]).headed("What you can spend");
        section.push(line("Money in liquid places".into(), self.held()));
        for &(place, qty) in &self.places {
            section.push(line(path(book, place).into(), qty).depth(1).style(Style::Muted));
        }
        if !self.tied_total().is_zero() {
            section.push(line("Tied to restricted entities".into(), -self.tied_total()));
            for (&entity, &qty) in &self.tied {
                section.push(
                    line(format!("tied to {}", book.name(book.entities[entity].path)), -qty)
                        .depth(1)
                        .style(Style::Muted),
                );
            }
        }
        if !self.pending_total().is_zero() {
            section.push(line("Pending outflows".into(), -self.pending_total()));
            for pending in &self.pending {
                section.push(
                    line(format!("{} to {}", pending.day, path(book, pending.with)), -pending.qty)
                        .depth(1)
                        .style(Style::Muted),
                );
            }
        }
        let spendable = self.held() - self.tied_total() - self.pending_total();
        section.push(line("Available to spend".into(), spendable).style(Style::Total));
        if self.unpriced_pending > 0 {
            section.note(format!("{} pending outflows have no price and are not subtracted.", self.unpriced_pending));
        }
        section
    }
}

// ─── Everything else ────────────────────────────────────────────────────────

/// A holding that is not spendable cash: an account with a liquidity span,
/// or something that is not the base currency.
struct Candidate {
    place: Id<Place>,
    unit: Id<Commodity>,
    qty: Qty,
    /// In the base currency at the day's prices.
    value: Option<Qty>,
    /// How long turning it into cash takes: the slower of place and commodity.
    liquid_in: Span,
    /// A real flow that touched the place, lending the hypothetical one its
    /// transaction and source line.
    template: Id<Flow>,
}

fn candidates(book: &Book, holdings: &[&Holding], at: Day) -> Vec<Candidate> {
    let valuer = Valuer::new(book, at);
    let others = holdings.iter().filter(|holding| !counts_as_cash(book, holding));
    others
        .filter_map(|holding| {
            let qty = holding.qty();
            let (by_place, by_unit) = (
                book.places[holding.place].liquidity.unwrap_or_default(),
                book.commodities[holding.unit].liquidity.unwrap_or_default(),
            );
            Some(Candidate {
                place: holding.place,
                unit: holding.unit,
                qty,
                value: valuer.qty(Amount::new(qty, holding.unit)),
                liquid_in: if at.add(by_place) >= at.add(by_unit) { by_place } else { by_unit },
                template: *book.touching[holding.place].last()?,
            })
        })
        .collect()
}

/// What the laws have recorded, in the base currency: an obligation to
/// someone by some day, or a tally, under a name.
struct Recorded(BTreeMap<(Sym, Option<(Id<Entity>, Day)>), Qty>);

impl Recorded {
    fn by(book: &Book, run: &Run, at: Day) -> Recorded {
        let valuer = Valuer::new(book, at);
        let mut recorded = BTreeMap::new();
        for effect in &run.effects {
            let Some(qty) = valuer.qty(effect.amount) else { continue };
            *recorded.entry((effect.name, effect.owe.map(|owed| (owed.to, owed.due)))).or_default() += qty;
        }
        Recorded(recorded)
    }

    /// What `self` records beyond `baseline`.
    fn beyond(&self, baseline: &Recorded) -> Vec<Change> {
        let more = |(&(name, owed), &qty): (&(Sym, Option<(Id<Entity>, Day)>), &Qty)| {
            let delta = qty - baseline.0.get(&(name, owed)).copied().unwrap_or_default();
            (!delta.is_zero()).then_some(Change { name, owed, delta })
        };
        self.0.iter().filter_map(more).collect()
    }
}

/// One more obligation or tally that a withdrawal brings about.
struct Change {
    name: Sym,
    owed: Option<(Id<Entity>, Day)>,
    delta: Qty,
}

/// What the laws did about drawing a holding down.
#[derive(Default)]
struct Outcome {
    changes: Vec<Change>,
    /// Base-currency gain realized.
    gain: Qty,
    /// Laws that forbid it, by their diagnostic's message.
    blocked: Vec<String>,
}

impl Outcome {
    /// Clones the ledger, moves the whole holding into `to` as one flow, runs
    /// out the year, and reads what the laws recorded that they would not have
    /// otherwise. Nothing is simulated without a price or a place to receive
    /// the money.
    fn of(ledger: &Ledger, candidate: &Candidate, to: Option<Id<Place>>, baseline: &Recorded, at: Day) -> Outcome {
        let (Some(to), Some(cash_in)) = (to, candidate.value) else { return Outcome::default() };
        let book = ledger.book();
        let flow = Flow {
            from: candidate.place,
            to,
            payee: None,
            select: Box::default(),
            ..planned(
                &book.flows[candidate.template],
                at,
                Amount::new(candidate.qty, candidate.unit),
                Amount::new(cash_in, book.base),
            )
        };

        let mut ledger = ledger.clone();
        let applied = ledger.apply(&flow);
        let run = settled(ledger, at.year_end());
        let blocked = run.violations[applied.violations]
            .iter()
            .filter(|violation| !violation.warn && !violation.waived)
            .map(|violation| run.diagnostics[violation.diagnostic as usize].message.clone())
            .collect();
        Outcome {
            changes: Recorded::by(book, &run, at).beyond(baseline),
            gain: run.gains[applied.gains].iter().map(|gain| gain.gain()).sum(),
            blocked,
        }
    }

    /// What the laws would take: penalties and taxes owed.
    fn costs(&self) -> Qty {
        self.changes.iter().filter(|change| change.owed.is_some()).map(|change| change.delta).sum()
    }
}

fn elsewhere_section<'s>(
    book: &Book<'s>,
    candidates: &[Candidate],
    outcomes: &[Outcome],
    to: Option<Id<Place>>,
    at: Day,
) -> Section<'s> {
    let columns = [
        Column::left("Holding"),
        Column::left("Liquid in"),
        Column::right("Value"),
        Column::right("Costs"),
        Column::right("Net"),
    ];
    let mut section = Section::new(columns).headed("What it would take to reach the rest");
    let (mut value, mut costs) = (Qty::ZERO, Qty::ZERO);
    for (candidate, outcome) in candidates.iter().zip(outcomes) {
        let reachable = candidate.value.filter(|_| to.is_some() && outcome.blocked.is_empty());
        if let Some(worth) = reachable {
            (value, costs) = (value + worth, costs + outcome.costs());
        }
        push_candidate(&mut section, book, candidate, outcome);
    }
    if section.rows.is_empty() {
        return section;
    }
    let total = [
        Cell::text("If everything were drawn today"),
        Cell::Blank,
        Cell::base(book, value),
        Cell::base(book, costs),
        Cell::base(book, value - costs),
    ];
    section.push(Row::new(total).style(Style::Total));
    match to {
        Some(to) => section.note(format!(
            "Each line withdraws the whole holding into {} on {at} and runs the year out through the laws. Costs are what they \
             would owe because of it, as if the year ended today: penalties, and the tax on any income it creates.",
            path(book, to)
        )),
        None => section.note("No liquid place holds money, so there is nowhere to withdraw into."),
    }
    section
}

fn push_candidate<'s>(section: &mut Section<'s>, book: &Book<'s>, candidate: &Candidate, outcome: &Outcome) {
    let held = Amount::new(candidate.qty, candidate.unit);
    let label: Cow<str> = if candidate.unit == book.base {
        path(book, candidate.place).into()
    } else {
        format!("{} ({})", path(book, candidate.place), book.show(held)).into()
    };
    let liquid_in: Cow<str> =
        if candidate.liquid_in == Span::default() { "now".into() } else { candidate.liquid_in.to_string().into() };
    let net = candidate.value.filter(|_| outcome.blocked.is_empty()).map(|worth| worth - outcome.costs());
    let cells = [
        Cell::text(label),
        Cell::text(liquid_in),
        candidate.value.map_or(Cell::Blank, |worth| Cell::base(book, worth)),
        if outcome.costs().is_zero() { Cell::Blank } else { Cell::base(book, outcome.costs()) },
        net.map_or(Cell::Blank, |net| Cell::base(book, net)),
    ];
    let unpriced = candidate.value.is_none();
    section.push(Row::new(cells).style(if unpriced { Style::Muted } else { Style::Normal }));

    if unpriced {
        section.push(detail(format!("no price for {}: a withdrawal cannot be valued", book.show(held)), Style::Muted));
    }
    for change in &outcome.changes {
        let name = book.name(change.name);
        let (text, cost) = match change.owed {
            Some((to, due)) => (
                format!("{name}, owed to {} by {due}", book.name(book.entities[to].path)),
                Cell::base(book, change.delta),
            ),
            None => (format!("counts {} as {name}", book.show(Amount::new(change.delta, book.base))), Cell::Blank),
        };
        section.push(
            Row::new([Cell::text(text), Cell::Blank, Cell::Blank, cost, Cell::Blank]).depth(1).style(Style::Muted),
        );
    }
    if !outcome.gain.is_zero() {
        section.push(detail(
            format!("realizes a gain of {}", book.show(Amount::new(outcome.gain, book.base))),
            Style::Muted,
        ));
    }
    for message in &outcome.blocked {
        section.push(detail(format!("blocked: {message}"), Style::Alert));
    }
}

/// A line of small print under a holding.
fn detail<'s>(text: String, style: Style) -> Row<'s> {
    Row::new([Cell::text(text), Cell::Blank, Cell::Blank, Cell::Blank, Cell::Blank]).depth(1).style(style)
}
