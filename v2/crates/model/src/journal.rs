//! What the journal records: flows grouped into transactions, balance
//! assertions, settlement events, prices, and plans.

use axiom_core::{Day, Id, Loc, Qty, Ratio, Span, Sym};

use crate::book::{Amount, Asset, Commodity, Contract, Entity, EventState, Kind, On, Place, Policy, Purpose};

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
    /// Who bears it, or earns it: tallies and budgets follow this. The owner
    /// of the flow's account ends by default; a share makes it another.
    pub owner: Id<Entity>,
    /// What it is for, and why the book thinks so.
    pub purpose: Option<Purposed>,
    /// `"food for the routine"`.
    pub description: Option<Sym>,
    /// Written, an occurrence of a contract, or derived.
    pub origin: Origin,
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

    /// Whether quantity crosses `end`. At a `PLACE.basis` end none does: the
    /// flow changes what the place's parcels cost, and nothing arrives there or
    /// leaves it. Everything that reads a flow as money asks this first.
    pub fn moves_quantity(&self, end: End) -> bool {
        self.terms().basis_end != Some(end)
    }
}

/// A flow's purpose, its object, and where it came from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Purposed {
    pub purpose: Id<Purpose>,
    /// `of condo`.
    pub of: Option<Object>,
    pub source: Source,
}

/// What a purpose is `of`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Object {
    Asset(Id<Asset>),
    Place(Id<Place>),
    Entity(Id<Entity>),
}

/// Where a flow's purpose came from, first match winning (LANGUAGE §2).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    /// On the leg or its header.
    Written(Loc),
    Contract(Id<Contract>),
    /// The kind of the party at the flow's other end.
    Party(Id<Kind>),
    /// The kind of the commodity paying, in party position.
    Commodity(Id<Kind>),
    /// A `takes … from …` of an account's kind.
    Account(Id<Kind>),
    /// A derived flow's own purpose (interest, sales tax, a match).
    Derived,
}

/// How a flow came to be.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Origin {
    Written,
    /// An occurrence of a contract, written in the journal as `DATE NAME`.
    Occurrence(Id<Contract>),
    /// Implied by something written; never in the journal.
    Derived(Derivation),
}

/// What a derived flow is, and what it came from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Derivation {
    /// A loan payment's interest, or its principal.
    Interest(Id<Contract>),
    Principal(Id<Contract>),
    Escrow(Id<Contract>),
    Match(Id<Contract>),
    /// An owner's share of a flow: `business 60% for studio`, from a
    /// contract, a party kind or a purpose. `Loc` is the share's declaration.
    Share(Loc),
    /// The tax inside a price paid to a party with `sales-tax`.
    SalesTax(Id<Kind>),
    /// What an exchange rate cost: what was given less what was got.
    ExchangeCost,
    /// A leg between two parties, split into its two halves through the owner.
    PassThrough,
    /// A contract deposit or a missing occurrence: a claim.
    Claim(Id<Contract>),
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
    /// v3 only: the v4 model has no `.basis` places, and this goes with the
    /// v3 model.
    ///
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
    /// `due`: the flow made a claim due on this day. In a `claim` place its
    /// parcel stays apart, and it is this flow's `due` and payee that say who
    /// owes it and by when.
    pub due: Option<Day>,
}

impl Terms {
    pub const NONE: Terms =
        Terms { basis: None, hold: None, basis_end: None, since: None, spender: None, cost: None, due: None };

    /// The same terms `days` later: a due day goes with the flow that carries it.
    pub fn moved(&self, days: i32) -> Terms {
        Terms { due: self.due.map(|due| due.add_days(days)), ..self.clone() }
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
    pub codes: Box<[Sym]>,
    /// `!`: this transaction's law violations are accepted and reported.
    pub waive: Option<Waive>,
    /// The named plan this transaction is an occurrence of (`DATE paycheck`).
    /// v3 only: the v4 model fills `contract`.
    pub plan: Option<Id<Plan>>,
    /// The contract this transaction is an occurrence of (`DATE phone`).
    pub contract: Option<Id<Contract>>,
    /// `DATE NAME ends`: it ends the contract, and has no flows.
    pub ends: bool,
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
