//! What the journal records: flows grouped into transactions, balance
//! assertions, settlement events, prices, and plans.

use axiom_core::{Day, Id, Loc, Qty, Ratio, Span, Sym};

use crate::book::{Amount, Commodity, Entity, EventState, On, Place, Policy};

/// Value moving once, from one place to another. Balanced by construction.
#[derive(Clone, Debug)]
pub struct Flow {
    /// The day value moves: balances, relief and settlement follow it.
    pub day: Day,
    /// The period the flow belongs to: tallies, window totals, budgets and
    /// every report about a period follow it. `day..=day` unless the flow is
    /// spread (`DATE..DATE`) or says `for PERIOD`.
    pub recognized: Recognition,
    pub from: Id<Place>,
    pub to: Id<Place>,
    /// What leaves `from`.
    pub out: Amount,
    /// What arrives at `to`: the same as `out` for a transfer, another
    /// commodity for an exchange.
    pub arrive: Amount,
    pub mode: Mode,
    /// Whether the quantities are known yet, or still to be solved.
    pub infer: Infer,
    pub txn: Id<Txn>,
    pub payee: Option<Id<Entity>>,
    /// Lot selectors applied when relieving parcels at `from`.
    pub select: Box<[Select]>,
    /// The transaction's codes, then the leg's own.
    pub codes: Box<[Sym]>,
    /// The leg, or the header for a flow without legs.
    pub loc: Loc,
    /// The `!` that covers this flow: its leg's own, else the transaction's.
    /// Law violations it raises, priced ones included, are accepted and
    /// reported.
    pub waive: Option<Waive>,
    /// What few flows say about the parcels they move. Boxed: most flows say
    /// nothing, and a flow is copied into every place that reads the journal.
    pub terms: Option<Box<Terms>>,
}

impl Flow {
    pub fn is_exchange(&self) -> bool {
        self.out.unit != self.arrive.unit
    }

    /// The flow's terms, or the terms of a flow that says nothing.
    pub fn terms(&self) -> &Terms {
        self.terms.as_deref().unwrap_or(&Terms::NONE)
    }
}

/// An inclusive range of days over which a flow is recognized.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Recognition {
    pub from: Day,
    pub until: Day,
}

impl Recognition {
    pub fn on(day: Day) -> Recognition {
        Recognition { from: day, until: day }
    }

    /// Whether the whole flow belongs to one day.
    pub fn is_instant(self) -> bool {
        self.from == self.until
    }
}

/// What a flow says about the parcels it moves, beyond how many.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct Terms {
    /// `basis 3_000 USD`: the total basis the arriving parcels take, in
    /// base-currency quanta, overriding the target kind's arrival rule.
    pub basis: Option<Qty>,
    /// `for ENTITY`: the arriving parcels are held for this entity, and its
    /// `on spend` laws govern them. The target's owner means "untie".
    pub hold: Option<Id<Entity>>,
    /// `PLACE.basis` at one end: that end moves basis, not quantity. Into it,
    /// the place's parcels gain basis; out of it, they lose basis and the
    /// amount is recognized at the other end.
    pub basis_end: Option<End>,
    /// An opening line's `since`: when its parcels were acquired.
    pub since: Option<Day>,
    /// An entity written as the source (`car-fund -> car-repair 150 USD`): the
    /// parcels tied to it leave first, whatever its `on spend` laws make of
    /// the flow, since the flow says whose money it is.
    pub spender: Option<Id<Entity>>,
    /// What the legs of an exchange into expense places cost it, in what the
    /// fee is paid in (a trading fee, a sale's commission): the parcels sold
    /// fetched that much less, and the parcels bought cost that much more.
    pub cost: Option<Amount>,
}

impl Terms {
    pub const NONE: Terms =
        Terms { basis: None, hold: None, basis_end: None, since: None, spender: None, cost: None };
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Mode {
    /// It happened.
    Actual,
    /// `(350 USD)`: written but not yet real, like an uncashed check. It
    /// reduces what can be spent but not what the bank reports.
    Pending,
    /// Generated from a plan; exists only in forecasts.
    Planned,
    /// An `opening` line: holdings from `equity/opening` that no law sees and
    /// that start no period.
    Opening,
}

/// How a flow's quantity is known.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Infer {
    Known,
    /// `? USD`: solved from the balance assertions around it.
    Unknown,
    /// `= 5_000 USD`: whatever makes this end's place hold `balance` after
    /// the flow.
    Target {
        end: End,
        balance: Qty,
    },
    /// `all`: everything the selected parcels at `from` hold.
    All,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum End {
    From,
    To,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Select {
    /// Parcels acquired within these days, inclusive.
    Range(Day, Day),
    /// Parcels acquired by transactions marked with this code.
    Code(Sym),
    Policy(Policy),
}

/// Flows written together.
pub struct Txn {
    pub day: Day,
    /// The flows it produced: `first .. first + len` in `Book::flows`.
    pub first: Id<Flow>,
    pub len: u32,
    pub payee: Option<Id<Entity>>,
    pub codes: Box<[Sym]>,
    /// `!`: this transaction's law violations are accepted and reported.
    pub waive: Option<Waive>,
    /// `due`: the transaction made a claim due on this day. In a `claim` place
    /// its parcels stay apart, remembering this transaction.
    pub due: Option<Day>,
    /// The named plan this transaction is an occurrence of (`DATE paycheck`).
    pub plan: Option<Id<Plan>>,
    pub doc: Option<Sym>,
    pub loc: Loc,
}

#[derive(Clone, Copy, Debug)]
pub struct Waive {
    pub loc: Loc,
    pub reason: Option<Sym>,
}

/// `2026-01-31 checking = 7_921.30 USD`, checked at the end of the day.
pub struct Assert {
    pub day: Day,
    pub place: Id<Place>,
    /// As written, in the place's display sign (`Class::display_sign`):
    /// `visa = 1_234.56 USD` says 1,234.56 is owed, and is stored positive; an
    /// overdrawn `checking = -42.17 USD` is stored negative. The engine applies
    /// the sign when it compares with the balance.
    pub amount: Amount,
    /// What becomes of a difference between the balance and the statement.
    pub gap: Gap,
    pub loc: Loc,
}

/// Where an assertion's gap goes.
#[derive(Clone, Copy, Debug)]
pub enum Gap {
    /// Nowhere: a gap is an error.
    Refused,
    /// `!`: an explicit flow from `equity/unknown`.
    Unexplained(Waive),
    /// `via PLACE`: a flow from or to that place. Into a `market` place it is a
    /// revaluation, not a withdrawal.
    Via { place: Id<Place>, loc: Loc },
}

/// `2026-05-22 FAST split 2 for 1`: every parcel of `unit`, everywhere, is
/// multiplied by `ratio`, keeping its basis and acquisition day.
#[derive(Clone, Copy, Debug)]
pub struct Split {
    pub day: Day,
    pub unit: Id<Commodity>,
    /// New units per old unit: 2 for `2 for 1`, 1/10 for `1 for 10`.
    pub ratio: Ratio,
    pub loc: Loc,
}

/// `2026-02-06 #check-1041 settled`
pub struct Event {
    pub day: Day,
    pub code: Sym,
    pub state: EventState,
    pub loc: Loc,
}

/// `every month on 1 checking -> landlord 2_400 USD until 2027-06`, or a named
/// one, `plan paycheck every 2w …`, which the journal can also instantiate.
pub struct Plan {
    pub name: Option<Sym>,
    pub every: Span,
    pub on: Option<On>,
    pub from: Option<Day>,
    pub until: Option<Day>,
    /// The flows of one occurrence, mode `Planned`, dated at `from` (or the
    /// day the plan was declared relative to). Forecasts re-date copies.
    pub template: Box<[Flow]>,
    pub loc: Loc,
}

/// Prices by commodity pair and day.
#[derive(Default)]
pub struct Prices {
    /// Sorted by `(unit, quote, day)`.
    pub(crate) quotes: Vec<Quote>,
}

#[derive(Clone, Copy, Debug)]
pub struct Quote {
    pub unit: Id<Commodity>,
    pub quote: Id<Commodity>,
    pub day: Day,
    /// Whole `quote` units per whole `unit`.
    pub rate: Ratio,
    /// Derived from an exchange rather than written as a price line. A
    /// written price on the same day wins.
    pub implied: bool,
    pub loc: Loc,
}

impl Prices {
    pub fn quotes(&self) -> &[Quote] {
        &self.quotes
    }
}
