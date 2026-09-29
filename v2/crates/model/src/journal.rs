//! What the journal records: flows grouped into transactions, balance
//! assertions, settlement events, prices, and plans.

use axiom_core::{Day, Id, Loc, Qty, Ratio, Span, Sym};

use crate::book::{Amount, Commodity, Entity, EventState, On, Place, Policy};

/// Value moving once, from one place to another. Balanced by construction.
#[derive(Clone, Debug)]
pub struct Flow {
    pub day: Day,
    /// The last day over which arrival is recognized; `day` unless spread.
    pub until: Day,
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
    /// The leg carries its own `!`, on top of any the transaction has.
    pub waived: bool,
}

impl Flow {
    pub fn is_exchange(&self) -> bool {
        self.out.unit != self.arrive.unit
    }
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
    /// `visa = 1_234.56 USD` says 1,234.56 is owed, and is stored positive.
    /// The engine applies the sign when it compares with the balance.
    pub amount: Amount,
    /// `!`: an unexplained gap becomes an explicit flow from `unknown`.
    pub pad: Option<Waive>,
    pub loc: Loc,
}

/// `2026-02-06 #check-1041 settled`
pub struct Event {
    pub day: Day,
    pub code: Sym,
    pub state: EventState,
    pub loc: Loc,
}

/// `every month on 1 checking -> landlord 2_400 USD until 2027-06`
pub struct Plan {
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
