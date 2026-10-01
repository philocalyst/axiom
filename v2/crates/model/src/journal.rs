//! What the journal records: flows grouped into transactions, balance
//! assertions, measures, settlement events, prices, returns as filed, and plans.

use axiom_core::{Day, Days, Id, Loc, Qty, Ratio, Run, Span, Sym};
use std::hash::{Hash, Hasher};

use crate::book::{
    Also, Amount, Asset, Commodity, Contract, Entity, EventState, Kind, On, Place, Policy, Purpose, ScheduleKind,
    System, Text,
};
use crate::law::{Law, Subject};

/// Value moving once, from one place to another. Balanced by construction.
#[derive(Clone, PartialEq, Debug)]
pub struct Flow {
    /// The day value moves: balances, relief and settlement follow it.
    pub day: Day,
    /// The period the flow belongs to: tallies, window totals, budgets and
    /// every report about a period follow it. `day..=day` unless the flow is
    /// spread (`DATE..DATE`) or says `for PERIOD`.
    pub recognized: Days,
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
    pub description: Option<Text>,
    /// Written, an occurrence of a contract, or derived.
    pub origin: Origin,
    /// Lot selectors applied when relieving parcels at `from`.
    pub select: Run<Select>,
    /// The transaction header's codes. This is stored on the flow so forecast
    /// and derived flows retain metadata even when their transaction identity
    /// is synthetic.
    pub header_codes: Run<Sym>,
    /// Codes written on the leg or item. Transaction codes are shared by all
    /// its flows and are read through [`FlowView::codes`].
    pub codes: Run<Sym>,
    /// The leg, or the header for a flow without legs.
    pub loc: Loc,
    /// The `!` that covers this flow: its leg's own, else the transaction's.
    /// Law violations it raises, priced ones included, are accepted and
    /// reported.
    pub waive: Option<Waive>,
    /// An id in the book's rare-detail pool; most flows say nothing here.
    pub detail: Option<Id<Detail>>,
}

impl Flow {
    pub fn is_exchange(&self) -> bool {
        self.out.unit != self.arrive.unit
    }

    /// The immutable pooled identity used to match selectors against source
    /// transaction and local codes, including after a flow is forecast.
    pub fn code_runs(&self) -> FlowCodes {
        FlowCodes { header: self.header_codes, local: self.codes }
    }

}

/// Two ranges in the book-wide code arena: transaction header first, then the
/// originating flow's own codes. Copying this value never copies code text.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FlowCodes {
    pub header: Run<Sym>,
    pub local: Run<Sym>,
}

impl Hash for FlowCodes {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.header.start().hash(state);
        self.header.len().hash(state);
        self.local.start().hash(state);
        self.local.len().hash(state);
    }
}

/// A changed detail owned by an engine's runtime arena, such as a contract
/// occurrence's shifted due day or prorated basis.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct RuntimeDetail(pub Detail);

/// A forecast or derived flow and its optional runtime detail override. With
/// no override, its `Flow::detail` handle still names the source Book detail.
#[derive(Clone, PartialEq, Debug)]
pub struct RuntimeFlow {
    pub flow: Flow,
    pub detail: Option<Id<RuntimeDetail>>,
    /// Identity of the transaction in the runtime. `Flow::txn` is source
    /// metadata and is never used as a Book transaction index for a runtime
    /// flow.
    pub txn: RuntimeTxn,
}

impl RuntimeFlow {
    pub fn source(flow: Flow) -> RuntimeFlow {
        RuntimeFlow { txn: RuntimeTxn::journal(flow.txn), flow, detail: None }
    }
}

/// Transaction identity for a flow applied through the runtime interface.
///
/// Contract occurrence identity is stable whether or not a journal
/// transaction later keeps it. Its optional source transaction is provenance
/// only, not part of the key used to merge lots or deduplicate occurrences.
#[derive(Clone, Copy, Debug)]
pub enum RuntimeTxn {
    /// A transaction recorded in the Book.
    Journal(Id<Txn>),
    /// One occurrence from a contract schedule. The same key is shared by all
    /// grouped headers, legs, and items in that occurrence.
    ContractOccurrence {
        contract: Id<Contract>,
        schedule: ScheduleKind,
        day: Day,
        ordinal: u32,
        source: Option<Id<Txn>>,
    },
}

impl RuntimeTxn {
    pub fn journal(txn: Id<Txn>) -> RuntimeTxn {
        assert_ne!(txn, TEMPLATE_TXN, "template transaction is not a Book transaction");
        RuntimeTxn::Journal(txn)
    }

    /// The actual Book transaction that supplies source location/codes, if
    /// this runtime flow came from one.
    pub fn source_txn(self) -> Option<Id<Txn>> {
        match self {
            RuntimeTxn::Journal(txn) => Some(txn),
            RuntimeTxn::ContractOccurrence { source, .. } => source,
        }
    }
}

impl PartialEq for RuntimeTxn {
    fn eq(&self, other: &Self) -> bool {
        match (*self, *other) {
            (RuntimeTxn::Journal(a), RuntimeTxn::Journal(b)) => a == b,
            (
                RuntimeTxn::ContractOccurrence { contract: ac, schedule: as_, day: ad, ordinal: ao, .. },
                RuntimeTxn::ContractOccurrence { contract: bc, schedule: bs, day: bd, ordinal: bo, .. },
            ) => (ac, as_, ad, ao) == (bc, bs, bd, bo),
            _ => false,
        }
    }
}

impl Eq for RuntimeTxn {}

impl Hash for RuntimeTxn {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match *self {
            RuntimeTxn::Journal(txn) => {
                0u8.hash(state);
                txn.hash(state);
            }
            RuntimeTxn::ContractOccurrence { contract, schedule, day, ordinal, .. } => {
                1u8.hash(state);
                contract.hash(state);
                schedule.hash(state);
                day.hash(state);
                ordinal.hash(state);
            }
        }
    }
}

#[cfg(test)]
mod runtime_txn_tests {
    use super::*;

    #[test]
    fn contract_identity_is_stable_when_a_journal_transaction_keeps_it() {
        let key = |source| RuntimeTxn::ContractOccurrence {
            contract: Id::new(2),
            schedule: ScheduleKind::Standing,
            day: Day(42),
            ordinal: 3,
            source,
        };
        assert_eq!(key(None), key(Some(Id::new(9))));
        assert_eq!(key(None).source_txn(), None);
        assert_eq!(key(Some(Id::new(9))).source_txn(), Some(Id::new(9)));
    }
}

/// A borrowed view of a flow and its pooled metadata. Cloning the `Flow` is
/// constant-size; this view gives readers the original codes, selectors and
/// rare detail without allocating or copying them.
#[derive(Clone, Copy)]
pub struct FlowView<'a> {
    flow: &'a Flow,
    transaction_codes: &'a [Sym],
    local_codes: &'a [Sym],
    selectors: &'a [Select],
    detail: &'a Detail,
}

impl<'a> FlowView<'a> {
    pub(crate) fn new(
        flow: &'a Flow,
        transaction_codes: &'a [Sym],
        local_codes: &'a [Sym],
        selectors: &'a [Select],
        detail: &'a Detail,
    ) -> FlowView<'a> {
        FlowView { flow, transaction_codes, local_codes, selectors, detail }
    }

    /// Codes in source order: transaction header first, then this leg or item.
    pub fn codes(self) -> impl Iterator<Item = Sym> + 'a {
        self.transaction_codes.iter().chain(self.local_codes).copied()
    }

    /// The two pooled ranges in source order, suitable for a compact parcel
    /// selector key.
    pub fn code_runs(self) -> FlowCodes {
        self.flow.code_runs()
    }

    /// The resolved selectors applied at the flow's source.
    pub fn select(self) -> &'a [Select] {
        self.selectors
    }

    /// The flow's rare facts, or the shared empty value.
    pub fn detail(self) -> &'a Detail {
        self.detail
    }
}

impl std::ops::Deref for FlowView<'_> {
    type Target = Flow;
    fn deref(&self) -> &Flow {
        self.flow
    }
}

#[cfg(test)]
mod flow_view_tests {
    use super::*;
    use crate::book::Amount;

    #[test]
    fn pooled_flow_metadata_stays_borrowed_and_codes_keep_source_order() {
        let mut names = axiom_core::Interner::default();
        let header = [names.intern("statement"), names.intern("tax-2026")];
        let local = [names.intern("withheld")];
        let amount = Amount::zero(Id::new(0));
        let flow = Flow {
            day: Day::MIN,
            recognized: Days::on(Day::MIN),
            from: Id::new(0),
            to: Id::new(1),
            out: amount,
            arrive: amount,
            mode: Mode::Actual,
            infer: Infer::Known,
            txn: Id::new(0),
            payee: None,
            owner: Id::new(0),
            purpose: None,
            description: None,
            origin: Origin::Written,
            select: Run::new(Id::new(0), 0),
            header_codes: Run::new(Id::new(0), 0),
            codes: Run::new(Id::new(0), 0),
            loc: Loc::default(),
            waive: None,
            detail: None,
        };

        let view = FlowView::new(&flow, &header, &local, &[], &Detail::NONE);
        assert_eq!(view.codes().collect::<Vec<_>>(), [header[0], header[1], local[0]]);
        assert_eq!(view.code_runs(), FlowCodes { header: flow.header_codes, local: flow.codes });
        let mut by_codes = std::collections::HashMap::new();
        by_codes.insert(view.code_runs(), 1);
        assert_eq!(by_codes.get(&flow.code_runs()), Some(&1));
        assert!(view.select().is_empty());
        assert_eq!(*view.detail(), Detail::NONE);
        assert_eq!(view.from, Id::new(0));
    }
}

/// A flow's purpose, its object, and where it came from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Purposed {
    pub purpose: Id<Purpose>,
    /// `of condo`.
    pub of: Option<Object>,
    pub source: Provenance,
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
pub enum Provenance {
    /// On the leg or its header: the flow's own line says where.
    Written,
    Contract(Id<Contract>),
    /// The party's own `#purpose` (`entity corner-store #groceries`).
    Entity(Id<Entity>),
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
    /// An owner's share of a flow: `business 60% for studio`, declared on
    /// a contract, a party kind or a purpose.
    Share(Sharer),
    /// The tax inside a price paid to a party with `sales-tax`.
    SalesTax(Id<Kind>),
    /// What an exchange rate cost: what was given less what was got.
    ExchangeCost,
    /// A leg between two parties, split into its two halves through the owner.
    PassThrough,
    /// A contract deposit or a missing occurrence: a claim.
    Claim(Id<Contract>),
    /// An `also` line: escrow, an employer's match, a card's cash back.
    Also(Id<Also>),
    /// A deadline's `else`, when it passed (a late fee).
    Otherwise(Id<Contract>),
    /// A law's reparation (`require … else …`).
    Reparation(Id<Law>),
    /// The unused part of a `covers` promise that ended early.
    Refund(Id<Contract>),
    /// `for PARTY` on a payment: the party owes it, and the payment is its,
    /// passed through the owner.
    PaidFor(Id<Entity>),
    /// `^code waived` on a claim: what remained of it is forgiven.
    WriteOff,
    /// `DATE ASSET ends`: the asset leaves the owners for nothing.
    Disposal(Id<Asset>),
}

/// What declared a share, so `why` can point at its line.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sharer {
    Contract(Id<Contract>),
    Kind(Id<Kind>),
    Purpose(Id<Purpose>),
}

/// What a flow says about the parcels it moves, beyond how many.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Detail {
    /// `basis 3_000 USD`: the total basis the arriving parcels take, in
    /// base-currency quanta, overriding the target kind's arrival rule.
    pub basis: Option<Qty>,
    /// `for ENTITY`: the arriving parcels are held for this entity, and its
    /// `on spend` laws govern them. The target's owner means "untie".
    pub hold: Option<Id<Entity>>,
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
    /// `against ^code`: the transaction it refunds or reimburses.
    pub against: Option<Id<Txn>>,
    /// How a computed amount was reckoned (`12% of ^bldg-water`), for `why` and
    /// hints: the share and what it was of, with where that was stated.
    pub reckoned: Option<Reckoning>,
}

/// A computed amount's arithmetic: `rate` of `of`, as stated at `from`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Reckoning {
    pub rate: Ratio,
    pub of: Amount,
    pub from: Loc,
}

impl Detail {
    pub const NONE: Detail = Detail {
        basis: None,
        hold: None,
        since: None,
        spender: None,
        cost: None,
        due: None,
        against: None,
        reckoned: None,
    };

    /// The same detail `days` later: a due day goes with the flow that carries it.
    pub fn moved(&self, days: i32) -> Detail {
        Detail { due: self.due.map(|due| due.add_days(days)), ..*self }
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
    Range(Days),
    /// Parcels acquired by transactions marked with this code.
    Code(Sym),
    Policy(Policy),
}

/// Flows written together.
pub struct Txn {
    pub day: Day,
    /// The flows it produced, in `Book::flows`.
    pub flows: Run<Flow>,
    /// Occurrence input bindings, indexed by the active terms' input order.
    /// Empty for ordinary transactions and occurrences without inputs.
    pub inputs: Run<Option<Amount>>,
    pub codes: Run<Sym>,
    /// `!`: this transaction's law violations are accepted and reported.
    pub waive: Option<Waive>,
    /// The contract this transaction is an occurrence of (`DATE phone`).
    pub contract: Option<Id<Contract>>,
    /// Which independent contract schedule this occurrence keeps.
    pub contract_schedule: Option<ScheduleKind>,
    /// `DATE NAME ends`: it ends the contract, and has no flows.
    pub ends: bool,
    pub doc: Option<Sym>,
    pub loc: Loc,
}

/// An id used only by contract template flows before an occurrence is
/// instantiated. It is deliberately outside the transaction arena; engines
/// must replace it before flow lookup or ledger insertion.
pub const TEMPLATE_TXN: Id<Txn> = Id::new(u32::MAX);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Waive {
    pub loc: Loc,
    pub reason: Option<Text>,
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

/// `12 me worked 6.5 HR for halcyon ^inv-12`, `21 car used 44 MI
/// #business-travel for studio` (LANGUAGE §5): an event that moves nothing.
/// Purpose laws fire on it, and `total` counts it in its unit.
pub struct Measure {
    pub day: Day,
    pub action: Action,
    /// Who worked, or what was used.
    pub subject: Subject,
    /// In a unit of a `measure` kind.
    pub quantity: Amount,
    /// Whom it was for: its owner, as a flow's.
    pub owner: Id<Entity>,
    /// A party it was done for (`for halcyon`).
    pub party: Option<Id<Entity>>,
    pub purpose: Option<Purposed>,
    pub description: Option<Text>,
    pub codes: Box<[Sym]>,
    pub loc: Loc,
}

/// What a measure records.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Work,
    Use,
}

/// `01 ^bldg-water = 155.00 USD`: a named value on a day, for references.
#[derive(Clone, Copy, Debug)]
pub struct Reading {
    pub day: Day,
    pub code: Sym,
    pub amount: Amount,
    pub loc: Loc,
}

/// `2026-04-15 us filed 2025` with its tally lines (LANGUAGE §11).
pub struct Filed {
    pub day: Day,
    pub system: Id<System>,
    pub year: i32,
    pub owner: Id<Entity>,
    pub lines: Box<[(Sym, Amount, Loc)]>,
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
