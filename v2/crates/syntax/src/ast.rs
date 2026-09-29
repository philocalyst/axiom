//! The syntax tree.
//!
//! # Shape
//!
//! A [`File`] is flat. Its [`Item`]s are small (48 bytes at most), and nothing
//! inside any node owns a `Vec` or a `Box`. What varies in length lives in the
//! file's *tables*, one per node type, and a node reaches its variable part
//! through a typed range:
//!
//! - [`Many<T>`] is a run of `T`s in their table: `&file[flow.legs]` is a
//!   `&[Leg]`.
//! - [`Id<T>`] (from `axiom-core`) is one large, rare node in its own table:
//!   `&file[id]` is a `&Txn`, a `&Decl`, a `&Law`.
//! - [`ExprId`] is a node of the [`Exprs`] arena: `file.exprs[id]`.
//!
//! An [`ItemKind`] holds the `Id` of the node it is, so reading a file is one
//! `match` per item:
//!
//! ```ignore
//! for item in &file.items {
//!     match item.kind {
//!         ItemKind::Txn(id) => {
//!             let txn = &file[id];
//!             for leg in &file[txn.flow.legs] { /* … */ }
//!         }
//!         ItemKind::Decl(id) => { /* … */ }
//!         _ => {}
//!     }
//! }
//! ```
//!
//! Ranges, ids and expression ids are opaque: use them only to index the file
//! they came from. (A large file is parsed in pieces, each with its own
//! tables, and an index says which piece it belongs to as well as where.)
//!
//! # Text and locations
//!
//! The tree borrows the source and copies no text: [`Name`], [`Code`],
//! [`Amount`] and [`Doc`] are slices of it. A slice knows where it was written,
//! so none of them stores a location: [`File::loc`] recovers it from the
//! slice's address. Nodes that are not a single slice (a leg, a step, a clause)
//! store a [`Loc`].
//!
//! # Expressions
//!
//! Expressions live in one post-order arena per file, [`Exprs`]: children
//! precede their parent, so a subtree is the contiguous run of nodes
//! `first..=root`. Later phases keep this shape, which makes type checking and
//! evaluation a single forward scan in which every subexpression's result is
//! already at hand.

use std::fmt;
use std::marker::PhantomData;
use std::ops::{Deref, Index, Range};

use axiom_core::{Day, Dec, FileId, Id, Loc, Span};

// The calendar words a plan and a law's `each` trigger are written in are core's.
pub use axiom_core::{On, Period};

// ─── Text ───────────────────────────────────────────────────────────────────

/// A written name, path, glob or commodity: the slice of the source it is.
/// Places, entities, kinds and properties are lowercase (`assets/bank/checking`,
/// `trader-joes`, `expenses/food/*`); commodities are uppercase (`USD`).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Name<'s>(pub &'s str);

/// A written `#code`, `#` included. Its location covers the `#` too.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Code<'s>(pub &'s str);

/// A raw block of consecutive `///` lines, prefixes included. [`Doc::lines`]
/// strips them without allocating.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Doc<'s>(pub &'s str);

/// A written amount, exactly as it stands in the source: `84.20 USD`, `empty`,
/// or, after `=` in an assertion, `-50 USD`. Storing the text keeps an amount
/// at 16 bytes; [`Amount::num`] and [`Amount::unit`] read it back.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Amount<'s>(pub &'s str);

/// The text of each of these is what was written: `&*name` is a `&str`.
macro_rules! written {
    ($($ty:ident),+) => { $(impl Deref for $ty<'_> { type Target = str; fn deref(&self) -> &str { self.0 } })+ };
}
written!(Name, Code, Doc, Amount);

impl<'s> Code<'s> {
    /// The code without its `#`: `check-1041`.
    pub fn name(self) -> &'s str {
        &self.0[1..]
    }
}

impl<'s> Doc<'s> {
    /// The text of each line, `///` and one following space removed.
    pub fn lines(self) -> impl Iterator<Item = &'s str> {
        self.0.lines().map(|line| {
            let text = line.trim_start().trim_start_matches("///");
            text.strip_prefix(' ').unwrap_or(text)
        })
    }
}

/// The blanks that may separate a number from its commodity.
fn is_blank(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t')
}

impl<'s> Amount<'s> {
    /// The written number, sign included. The parser has validated it, so this
    /// cannot fail; `empty` is zero.
    pub fn num(self) -> Dec {
        let (negative, text) = match self.0.strip_prefix('-') {
            Some(unsigned) => (true, unsigned),
            None => (false, self.0),
        };
        let number = text.bytes().position(is_blank).map_or(text, |end| &text[..end]);
        let dec = Dec::parse(number.as_bytes()).unwrap_or_default();
        if negative { dec.neg() } else { dec }
    }

    /// The commodity, or `None` for `empty`, the zero of every commodity.
    pub fn unit(self) -> Option<Name<'s>> {
        self.0.bytes().rposition(is_blank).map(|blank| Name(&self.0[blank + 1..]))
    }
}

// ─── Where a node is ────────────────────────────────────────────────────────

/// How an index says which piece of the file it is in: its top bits. The rest
/// is a position in that piece's table, so a piece has at most 2^24 of any one
/// node, which its size (at most 16 MiB) guarantees: a node takes a byte.
pub(crate) const PIECE_SHIFT: u32 = 24;
const LOCAL: u32 = (1 << PIECE_SHIFT) - 1;

/// The position an index names in its piece.
pub(crate) fn local(raw: u32) -> usize {
    (raw & LOCAL) as usize
}

/// A run of `T`s in their table, addressed by position: `&file[many]`.
pub struct Many<T> {
    first: u32,
    len: u32,
    of: PhantomData<fn() -> T>,
}

impl<T> Many<T> {
    /// Nothing. Every empty range is this one.
    pub const EMPTY: Many<T> = Many { first: 0, len: 0, of: PhantomData };

    /// The `len` nodes from position `first` of a piece's table, as an index
    /// (piece bits included).
    pub(crate) fn new(first: u32, len: usize) -> Many<T> {
        match len {
            0 => Many::EMPTY,
            _ => Many { first, len: len as u32, of: PhantomData },
        }
    }

    /// How many nodes are in the run.
    pub fn len(self) -> usize {
        self.len as usize
    }

    /// Whether the run has no nodes.
    pub fn is_empty(self) -> bool {
        self.len == 0
    }

    /// Where the run is in its piece's table.
    pub(crate) fn range(self) -> Range<usize> {
        local(self.first)..local(self.first) + self.len()
    }
}

impl<T> Clone for Many<T> {
    fn clone(&self) -> Many<T> {
        *self
    }
}

impl<T> Copy for Many<T> {}

impl<T> Default for Many<T> {
    fn default() -> Many<T> {
        Many::EMPTY
    }
}

impl<T> fmt::Debug for Many<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}+{}", self.first >> PIECE_SHIFT, local(self.first), self.len)
    }
}

// ─── The file and its tables ────────────────────────────────────────────────

/// One parsed source file: its items in source order, and the tables that hold
/// what the items are made of.
#[derive(Default, Debug)]
pub struct File<'s> {
    /// Which file this is, as its locations say.
    pub id: FileId,
    /// The source text. Every [`Name`], [`Code`], [`Amount`] and [`Doc`] is a
    /// slice of it.
    pub src: &'s str,
    /// The top-level items, in source order.
    pub items: Vec<Item<'s>>,
    /// Every expression of the file.
    pub exprs: Exprs<'s>,
    /// The tables of each piece the file was parsed in.
    tables: Vec<Tables<'s>>,
}

impl<'s> File<'s> {
    /// A file made of its pieces, in order.
    pub(crate) fn new(id: FileId, src: &'s str, pieces: Vec<Piece<'s>>) -> File<'s> {
        let mut file = File { id, src, ..File::default() };
        let total: usize = pieces.iter().map(|piece| piece.items.len()).sum();
        // The first piece's items are the file's to start with: nearly every
        // file is one piece, and its items are never copied.
        let mut pieces = pieces.into_iter();
        if let Some(first) = pieces.next() {
            file.items = first.items;
            file.exprs.parts.push(first.exprs);
            file.tables.push(first.tables);
        }
        file.items.reserve_exact(total - file.items.len());
        for mut piece in pieces {
            file.items.append(&mut piece.items);
            file.exprs.parts.push(piece.exprs);
            file.tables.push(piece.tables);
        }
        file
    }

    /// Every `T` of the file, in the order it was written.
    pub fn iter<'f, T: Stored<'s> + 'f>(&'f self) -> impl Iterator<Item = &'f T> {
        self.tables.iter().flat_map(|tables| T::table(tables))
    }

    /// Where a slice of the source was written.
    pub fn loc(&self, text: &str) -> Loc {
        locate(self.id, self.src, text)
    }
}

/// Where `text`, a slice of `src`, was written.
pub(crate) fn locate(id: FileId, src: &str, text: &str) -> Loc {
    debug_assert!(src.as_bytes().as_ptr_range().contains(&text.as_ptr()), "not a slice of the source");
    let start = (text.as_ptr() as usize).wrapping_sub(src.as_ptr() as usize);
    Loc::new(id, start as u32, (start + text.len()) as u32)
}

/// A piece of a file as its parser built it: the items of its lines, the
/// expressions, and the tables, with indices that already say which piece.
pub(crate) struct Piece<'s> {
    pub items: Vec<Item<'s>>,
    pub exprs: Vec<Expr<'s>>,
    pub tables: Tables<'s>,
}

/// A type that lives in one of a piece's [`Tables`], so that a [`Many`] or an
/// [`Id`] of it can index the [`File`].
pub trait Stored<'s>: Sized {
    /// The table of this type in a piece's tables.
    fn table<'t>(tables: &'t Tables<'s>) -> &'t Vec<Self>;
    /// The same, to add to it while parsing.
    fn table_mut<'t>(tables: &'t mut Tables<'s>) -> &'t mut Vec<Self>;
}

impl<'s, T: Stored<'s>> Index<Many<T>> for File<'s> {
    type Output = [T];
    fn index(&self, many: Many<T>) -> &[T] {
        &T::table(&self.tables[(many.first >> PIECE_SHIFT) as usize])[many.range()]
    }
}

impl<'s, T: Stored<'s>> Index<Id<T>> for File<'s> {
    type Output = T;
    fn index(&self, id: Id<T>) -> &T {
        &T::table(&self.tables[id.index() >> PIECE_SHIFT])[local(id.index() as u32)]
    }
}

/// Declares [`Tables`], one `Vec` per node type, and how to find each.
macro_rules! tables {
    ($($(#[$doc:meta])* $field:ident: $ty:ty),+ $(,)?) => {
        /// The nodes of one piece of a file, a table for each type of node.
        #[derive(Default, Debug)]
        pub struct Tables<'s> {
            $($(#[$doc])* pub(crate) $field: Vec<$ty>,)+
        }

        $(impl<'s> Stored<'s> for $ty {
            fn table<'t>(tables: &'t Tables<'s>) -> &'t Vec<Self> { &tables.$field }
            fn table_mut<'t>(tables: &'t mut Tables<'s>) -> &'t mut Vec<Self> { &mut tables.$field }
        })+
    };
}

tables! {
    /// [`ItemKind::Txn`]
    txns: Txn<'s>,
    /// [`ItemKind::Assert`]
    asserts: Assert<'s>,
    /// [`ItemKind::Event`]
    events: Event<'s>,
    /// [`ItemKind::Price`]
    prices: Price<'s>,
    /// [`ItemKind::Split`]
    splits: Split<'s>,
    /// [`ItemKind::Occurrence`]
    occurrences: Occurrence<'s>,
    /// [`ItemKind::Opening`]
    openings: Opening<'s>,
    /// [`ItemKind::Plan`]
    plans: Plan<'s>,
    /// [`ItemKind::Decl`]
    decls: Decl<'s>,
    /// [`ItemKind::Code`]
    rules: CodeRule<'s>,
    /// [`ItemKind::Param`]
    params: Param<'s>,
    /// [`ItemKind::Sync`]
    syncs: Sync<'s>,
    /// [`ItemKind::Setting`]
    settings: Setting<'s>,
    /// Top-level laws ([`ItemKind::Law`]) and the laws nested in declarations.
    laws: Law<'s>,
    /// The legs of flows, occurrences and openings: [`Flow::legs`].
    legs: Leg<'s>,
    /// The selectors of places: [`Place::select`].
    selects: Select<'s>,
    /// The clauses of tails: [`Tail::clauses`].
    clauses: Clause<'s>,
    /// The property lines of declarations: [`Decl::props`].
    props: Prop<'s>,
    /// The rows of parameters: [`Param::rows`].
    rows: ParamRow<'s>,
    /// The keys of parameter rows: [`ParamRow::keys`].
    keys: Key<'s>,
    /// The steps of laws: [`Law::steps`].
    steps: Step<'s>,
    /// The rows of schedules: [`ExprKind::Schedule`].
    brackets: Bracket,
    /// Runs of expression roots: property arguments, call arguments, index
    /// keys and the alternatives of `is`.
    ids: ExprId,
    /// The globs and kinds of a code rule's `on` lines: [`CodeRule::on`].
    names: Name<'s>,
}

// ─── Items ──────────────────────────────────────────────────────────────────

/// A top-level item: a column-0 line and the indented block under it.
#[derive(Debug)]
pub struct Item<'s> {
    /// The `///` block directly above it.
    pub doc: Option<Doc<'s>>,
    /// The header line only, up to the end of its last token: a trailing
    /// comment and the item's block are not part of it.
    pub loc: Loc,
    /// What the item is, and where its node is.
    pub kind: ItemKind<'s>,
}

/// What an item is, by its first word, and where its node is: `&file[id]`.
#[derive(Clone, Copy, Debug)]
pub enum ItemKind<'s> {
    /// `DATE FLOW`
    Txn(Id<Txn<'s>>),
    /// `DATE PLACE = AMOUNT`
    Assert(Id<Assert<'s>>),
    /// `DATE #code settled|void|returned`
    Event(Id<Event<'s>>),
    /// `DATE UNIT AMOUNT`
    Price(Id<Price<'s>>),
    /// `DATE UNIT split N for M`
    Split(Id<Split<'s>>),
    /// `DATE PLAN [AMOUNT]`: one occurrence of a named plan.
    Occurrence(Id<Occurrence<'s>>),
    /// `opening DATE`
    Opening(Id<Opening<'s>>),
    /// `every …` or `plan NAME every …`
    Plan(Id<Plan<'s>>),
    /// `account`, `entity`, `commodity` or `kind`. One line naming several
    /// entities is one `Decl` per entity, all sharing the same properties.
    Decl(Id<Decl<'s>>),
    /// `code GLOB`. A line with several globs is one rule per glob, all
    /// sharing the same places.
    Code(Id<CodeRule<'s>>),
    /// `param NAME`
    Param(Id<Param<'s>>),
    /// A top-level `law NAME`. Its doc is also the item's.
    Law(Id<Law<'s>>),
    /// `sync FILE`
    Sync(Id<Sync<'s>>),
    /// `system`, `use`, `base`, `relaxed` or `layout free`.
    Setting(Id<Setting<'s>>),
}

/// One-line directives.
#[derive(Clone, Copy, Debug)]
pub enum Setting<'s> {
    /// `system PATH`: this file defines a system. Must be the first item.
    System(Name<'s>),
    /// `use PATH`
    Use(Name<'s>),
    /// `base UNIT`
    Base(Name<'s>),
    /// `relaxed`: law violations become warnings.
    Relaxed,
    /// `layout free`: folder names stop constraining dates.
    LayoutFree,
}

// ─── Journal ────────────────────────────────────────────────────────────────

/// `DATE FLOW`.
#[derive(Debug)]
pub struct Txn<'s> {
    /// The day value moves.
    pub date: Day,
    /// What moves, from where to where, and why.
    pub flow: Flow<'s>,
}

/// `SOURCE -> TARGET TAIL` with optional indented legs: what a transaction and
/// a plan both say.
///
/// When both sides name a place there are no legs. When exactly one side does,
/// the legs are the other side (a "one side split"), and the header may state
/// an amount on either or both sides: `house 1 HOME -> 431_500 USD`.
#[derive(Debug)]
pub struct Flow<'s> {
    /// What leaves: the left of the arrow.
    pub from: End<'s>,
    /// What arrives: the right of the arrow.
    pub to: End<'s>,
    /// What the header says about the whole flow, which applies to every leg.
    pub tail: Tail<'s>,
    /// The indented lines under the header: `&file[flow.legs]`.
    pub legs: Many<Leg<'s>>,
}

/// One side of a header: `checking`, `checking 2_000 USD`, `7 VTI`, or nothing.
#[derive(Debug)]
pub struct End<'s> {
    /// The place, or `None` when the side is left to the legs or to inference.
    pub place: Option<Place<'s>>,
    /// How much, or `None` when the side states none.
    pub amount: Option<Quantity<'s>>,
}

/// A place as written: `brokerage[fifo, 2024]`, `house[#roof].basis`.
#[derive(Debug)]
pub struct Place<'s> {
    /// A path, a unique suffix of one, an alias, an entity, or `?` (the
    /// unknown place).
    pub name: Name<'s>,
    /// Which parcels of the place the flow addresses, and whether it moves
    /// their quantity or their basis: `&file[place.select]`.
    pub select: Many<Select<'s>>,
}

impl<'s> Place<'s> {
    /// Whether the flow moves the basis of the place's parcels rather than
    /// their quantity: the place is written `PLACE.basis`.
    pub fn is_basis(&self, file: &File<'s>) -> bool {
        matches!(file[self.select].last(), Some(Select::Basis))
    }
}

/// A lot selector. Days, months and years are normalized to inclusive ranges.
#[derive(Clone, Copy, Debug)]
pub enum Select<'s> {
    /// Parcels acquired on these days, first and last, with where it was
    /// written: `2024`, `2026-01`, `2026-01-22`, `2026-01..2026-06`.
    Range(Day, Day, Loc),
    /// Parcels a transaction marked with this code.
    Code(Code<'s>),
    /// A lot policy, with where it was written: `[fifo]`.
    Policy(Policy, Loc),
    /// `.basis`, always last: what the selectors chose is moved by its basis,
    /// not its quantity. (It is written after the brackets, and costs a
    /// selector instead of a flag on every place.)
    Basis,
}

/// How parcels are chosen when several could leave.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Policy {
    /// Oldest parcels first.
    Fifo,
    /// Newest parcels first.
    Lifo,
    /// Highest-basis parcels first.
    Hifo,
    /// Every parcel in proportion to what it holds.
    Prorata,
}

/// How much moves on one side or leg.
#[derive(Clone, Copy, Debug)]
pub enum Quantity<'s> {
    /// `84.20 USD`, or `empty`.
    Fixed(Amount<'s>),
    /// `(350 USD)`: written, not yet real. The amount's location excludes the
    /// parentheses.
    Pending(Amount<'s>),
    /// `= 5_000 USD` (legs only): whatever makes the place's balance equal
    /// this after the flow.
    Target(Amount<'s>),
    /// `? USD`: inferred from surrounding balance assertions. The commodity.
    Unknown(Name<'s>),
    /// `all` or `all VXUS`: everything the selected parcels hold, of that
    /// commodity if one is named.
    All(Option<Name<'s>>),
    /// `...` (legs only): whatever balances the transaction.
    Rest,
}

/// An indented line under a flow: `retirement 800 USD #pretax`. Also a line of
/// an [`Opening`] (`house 1 HOME basis 540_000 USD since 2023-06-15`) and an
/// override under a plan [`Occurrence`].
#[derive(Debug)]
pub struct Leg<'s> {
    /// The `///` block above it.
    pub doc: Option<Doc<'s>>,
    /// Where the leg's value goes (or, in an opening, what holds it).
    pub place: Place<'s>,
    /// How much: fixed, pending, a target balance, or the remainder.
    pub amount: Quantity<'s>,
    /// The leg's own tail, in addition to the header's.
    pub tail: Tail<'s>,
    /// The whole line, trailing comment excluded.
    pub loc: Loc,
}

/// What may follow a header's or leg's amounts, in any order:
/// `/ payee #code for 2025 due 30d basis 3_000 USD @ 285.70 USD ! "reason"`.
/// A header's tail applies to every leg.
#[derive(Debug, Default)]
pub struct Tail<'s> {
    /// `/ payee`: a declared entity.
    pub payee: Option<Name<'s>>,
    /// The rest, in the order written: `&file[tail.clauses]`. A `DATE..DATE`
    /// spread is the `for` clause `for DATE..DATE`.
    pub clauses: Many<Clause<'s>>,
}

/// One clause of a [`Tail`], and where it was written (keyword through value).
/// The parser rejects a repeated clause, so each kind occurs at most once.
#[derive(Clone, Copy, Debug)]
pub struct Clause<'s> {
    /// Where the clause was written.
    pub at: Loc,
    /// What it says.
    pub kind: ClauseKind<'s>,
}

/// What a [`Clause`] says.
#[derive(Clone, Copy, Debug)]
pub enum ClauseKind<'s> {
    /// `#code`: marks the transaction or leg. Any number.
    /// `#code`: marks the transaction or leg. Any number.
    Code(Code<'s>),
    /// `@ 285.70 USD`: the price of one unit of the commodity that arrives.
    Price(Amount<'s>),
    /// `for WHAT`
    For(For<'s>),
    /// `due WHEN`: the flow is a claim due then.
    Due(Due),
    /// `basis 3_000 USD`: the total basis the arriving parcels take.
    Basis(Amount<'s>),
    /// `since 2023-06-15`: when an opening line's parcels were acquired.
    Since(Day),
    /// `!` or `! "reason"`
    Waive(Waive<'s>),
}

/// What a flow is on account of.
#[derive(Clone, Copy, Debug)]
pub enum For<'s> {
    /// `for #inv-12`: settles the claim the code marks.
    Code(Code<'s>),
    /// `for 2025`, `for 2026-03`, `for 2026-03-15`, `for 2026-01-01..2026-12-31`:
    /// the period the flow is recognized over, as first and last day.
    Period(Day, Day),
    /// `for car-fund`: the parcels are held for that entity.
    Entity(Name<'s>),
}

/// When a claim falls due.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Due {
    /// `due 2026-04-15`
    On(Day),
    /// `due 30d`: this long after the payment day.
    After(Span),
}

/// `!` or `! "reason"`: accept this item's law violations (or, on an
/// assertion, its gap) explicitly.
#[derive(Clone, Copy, Debug)]
pub struct Waive<'s> {
    /// The `!`, and the string after it if there is one.
    pub at: Loc,
    /// The string's contents, escapes not yet processed.
    pub reason: Option<&'s str>,
}

/// `DATE PLACE = [-]AMOUNT [! [STRING] | via PLACE]`, checked at the end of the day.
#[derive(Debug)]
pub struct Assert<'s> {
    /// The day the balance is checked, at its end.
    pub date: Day,
    /// The place whose balance is stated.
    pub place: Place<'s>,
    /// In the place's display sign. Negative (`= -50 USD`) for an overdraft.
    pub amount: Amount<'s>,
    /// What becomes of a difference between the statement and the ledger.
    pub gap: Gap<'s>,
}

/// Where an assertion's difference goes.
#[derive(Clone, Copy, Debug)]
pub enum Gap<'s> {
    /// Nowhere: a difference is an error.
    Refused,
    /// `!`: an explicit flow from `equity/unknown`.
    Waived(Waive<'s>),
    /// `via PLACE`: a flow from or to that place.
    Via(Name<'s>),
}

/// `DATE #code settled|void|returned`
#[derive(Debug)]
pub struct Event<'s> {
    /// The day the state takes effect.
    pub date: Day,
    /// The code of the flows the event concerns.
    pub code: Code<'s>,
    /// What became of them.
    pub state: EventState,
    /// Where the state word was written.
    pub state_loc: Loc,
}

/// What a `DATE #code STATE` line does to the flows carrying that code.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum EventState {
    /// Pending becomes actual on this day.
    Settled,
    /// Pending never happens.
    Void,
    /// Actual is reversed on this day.
    Returned,
}

/// `DATE UNIT PRICE`: one `unit` costs `price` on that day.
#[derive(Debug)]
pub struct Price<'s> {
    /// The day the price holds.
    pub date: Day,
    /// The commodity priced.
    pub unit: Name<'s>,
    /// What one unit costs.
    pub price: Amount<'s>,
}

/// `DATE UNIT split N for M`: every parcel of `unit`, in every place, is
/// multiplied by `numerator / denominator`.
#[derive(Debug)]
pub struct Split<'s> {
    /// The day the split takes effect.
    pub date: Day,
    /// The commodity split.
    pub unit: Name<'s>,
    /// The new number of units: `2` in `split 2 for 1`.
    pub numerator: Dec,
    /// The old number of units: `1` in `split 2 for 1`.
    pub denominator: Dec,
}

/// `DATE PLAN [AMOUNT]` with optional override legs: the journal says the named
/// plan happened on `date`.
#[derive(Debug)]
pub struct Occurrence<'s> {
    /// The day the plan happened.
    pub date: Day,
    /// The plan's name.
    pub plan: Name<'s>,
    /// Replaces the plan's header amount.
    pub amount: Option<Amount<'s>>,
    /// Replace the plan's legs of the same place.
    pub legs: Many<Leg<'s>>,
}

/// `opening DATE` and its indented lines: holdings that exist from that day,
/// each line a [`Leg`] with a fixed amount and `basis` and `since` clauses.
#[derive(Debug)]
pub struct Opening<'s> {
    /// The day the balances are stated: they exist from then on.
    pub date: Day,
    /// One line per holding: `&file[opening.lines]`.
    pub lines: Many<Leg<'s>>,
}

/// `every CADENCE [on DAY] [from DATE] [until DATE|MONTH] FLOW`, or
/// `plan NAME every …`.
#[derive(Debug)]
pub struct Plan<'s> {
    /// The name of a named plan, which the journal can instantiate.
    pub name: Option<Name<'s>>,
    /// `month` is one month, `2w` fourteen days, `quarter` three months.
    pub every: Span,
    /// `on DAY`: which day of each period. `None` means the period's own start.
    pub on: Option<On>,
    /// `from DATE`: when the plan starts, if it says.
    pub from: Option<Day>,
    /// Inclusive. A month bound is normalized to that month's last day.
    pub until: Option<Day>,
    /// What happens each time, with its legs.
    pub flow: Flow<'s>,
}

// ─── Declarations ───────────────────────────────────────────────────────────

/// `account|entity|commodity|kind NAME [: KIND]` with indented properties and laws.
#[derive(Debug)]
pub struct Decl<'s> {
    /// Which keyword introduced it.
    pub what: DeclKind,
    /// The path, entity, symbol or kind it declares.
    pub name: Name<'s>,
    /// `account PATH as ALIAS`.
    pub alias: Option<Name<'s>>,
    /// After `:`: the kind (or, for `kind`, the parent kind).
    pub kind: Option<Name<'s>>,
    /// The indented property lines: `&file[decl.props]`.
    pub props: Many<Prop<'s>>,
    /// The nested laws.
    pub laws: Many<Law<'s>>,
}

/// Which keyword introduced a declaration.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum DeclKind {
    /// `account`: a place that holds value.
    Account,
    /// `entity`: a person, business or authority.
    Entity,
    /// `commodity`: a currency, security or asset.
    Commodity,
    /// `kind`: a class that accounts, entities or commodities belong to.
    Kind,
}

/// `NAME ARG*`: arguments are primary expressions, commas skipped.
/// `has born date`, `budget 500 USD monthly`, `lives us/ca from 2026-01-01`.
#[derive(Debug)]
pub struct Prop<'s> {
    /// The property's name: `has`, `budget`, `lives`.
    pub name: Name<'s>,
    /// The roots of the argument expressions: `&file[prop.args]`.
    pub args: Many<ExprId>,
    /// The whole line.
    pub loc: Loc,
}

/// `code GLOB` with indented `on PLACE-GLOB | KIND` lines.
#[derive(Debug)]
pub struct CodeRule<'s> {
    /// The code's name without `#`, or a glob such as `trip-*`.
    pub pattern: Name<'s>,
    /// Every glob and kind of every `on` line: the places the code may mark.
    pub on: Many<Name<'s>>,
}

/// `param NAME` with indented `KEY+ VALUE` rows.
#[derive(Debug)]
pub struct Param<'s> {
    /// The parameter's name.
    pub name: Name<'s>,
    /// Its rows, in order: `&file[param.rows]`.
    pub rows: Many<ParamRow<'s>>,
}

/// `KEY+ VALUE`: the value is a schedule or any expression.
#[derive(Debug)]
pub struct ParamRow<'s> {
    /// What the row is looked up by: `&file[row.keys]`.
    pub keys: Many<Key<'s>>,
    /// The value: an expression, or a [`ExprKind::Schedule`].
    pub value: ExprId,
    /// The whole line.
    pub loc: Loc,
}

/// One key of a parameter row.
#[derive(Clone, Copy, Debug)]
pub enum Key<'s> {
    /// Step lookup: the latest year at or before the one asked for.
    Year(i32, Loc),
    /// An exact date.
    Date(Day, Loc),
    /// A name: a filing status, a kind.
    Name(Name<'s>),
}

/// `sync FILE` with an indented `run COMMAND…` line (raw text).
#[derive(Debug)]
pub struct Sync<'s> {
    /// The file to write, as written.
    pub file: Name<'s>,
    /// The command that produces it, as written.
    pub run: Name<'s>,
}

// ─── Laws ───────────────────────────────────────────────────────────────────

/// `law NAME` with an indented trigger and steps.
#[derive(Debug)]
pub struct Law<'s> {
    /// For a top-level law this is also the [`Item::doc`].
    pub doc: Option<Doc<'s>>,
    /// The law's name, which its diagnostics carry.
    pub name: Name<'s>,
    /// What makes it apply.
    pub trigger: Trigger,
    /// The trigger line.
    pub trigger_loc: Loc,
    /// The steps in order: `&file[law.steps]`. `on in from X | Y` is written
    /// as the trigger and a first step `when from is X | Y`, its expression
    /// nodes located at what was written.
    pub steps: Many<Step<'s>>,
    /// A line of its body did not parse and is left out, so what remains is
    /// not the whole law: its own diagnostic says why, and anything that only
    /// follows from the missing line is not worth another.
    pub damaged: bool,
    /// The `law NAME` line.
    pub loc: Loc,
}

/// When a law applies.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Trigger {
    /// `on in`: value arrives in a governed place.
    In,
    /// `on out`: value leaves a governed place.
    Out,
    /// `on gain`: parcels leaving a governed place realize a gain.
    Gain,
    /// `on spend`: money tied to a restricted entity leaves its owner's places.
    Spend,
    /// `each month` or `each year`: a period of the governed thing ends.
    Each(Period),
    /// `each year closing 04-15`: the year is judged on that day of the next.
    Closing {
        /// 1 to 12.
        month: u8,
        /// The day of that month.
        day: u8,
    },
    /// `by EXPR`: the journal reaches that date.
    By(ExprId),
    /// `always`: after any change to a governed place.
    Always,
}

/// One line of a law's body after its trigger. Steps run top to bottom.
#[derive(Debug)]
pub struct Step<'s> {
    /// The whole line.
    pub loc: Loc,
    /// What it does.
    pub kind: StepKind<'s>,
}

/// What a [`Step`] does.
#[derive(Debug)]
pub enum StepKind<'s> {
    /// A filter: the law stops silently when false.
    When(ExprId),
    /// `let NAME = EXPR`: binds a name for the steps below.
    Let(Name<'s>, ExprId),
    /// `require EXPR [else EFFECT] ["message"]`, or `warn EXPR ["message"]`.
    Require {
        /// What must hold.
        cond: ExprId,
        /// `else EFFECT`: what happens when it does not (`require` only).
        otherwise: Option<Effect<'s>>,
        /// The string after it, contents only.
        message: Option<Name<'s>>,
        /// `warn`, not `require`: a failure is a warning, not an error.
        warn: bool,
    },
    /// `owe …` or `count …`.
    Effect(Effect<'s>),
}

/// What a law does to the world: an obligation, or a tally.
#[derive(Debug)]
pub enum Effect<'s> {
    /// `owe EXPR to ENTITY [by EXPR] [as NAME]`: `amount` is owed to the entity
    /// `to`, due on the day `due` says (none: no due day), and called `name` so
    /// a later `for` can settle it.
    Owe { amount: ExprId, to: Name<'s>, due: Option<ExprId>, name: Option<Name<'s>> },
    /// `count EXPR as NAME`: adds `amount` to the tally `name`.
    Count { amount: ExprId, name: Name<'s> },
}

// ─── Expressions ────────────────────────────────────────────────────────────

/// Index of an expression in its file's [`Exprs`]: `file.exprs[id]`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct ExprId(pub(crate) u32);

impl ExprId {
    /// A number that is unique to the expression and grows through a subtree:
    /// `id.index() - first.index()` is how far into the subtree a node is.
    pub fn index(self) -> usize {
        self.0 as usize
    }

    /// The position in the piece's arena.
    fn position(self) -> usize {
        local(self.0)
    }
}

/// A file's expressions in post-order: children precede their parent, so every
/// subtree is the contiguous run `first..=root`. Index it with an [`ExprId`].
#[derive(Default, Debug)]
pub struct Exprs<'s> {
    /// The arena of each piece of the file.
    pub(crate) parts: Vec<Vec<Expr<'s>>>,
}

/// One node of an expression: what it is, where it was written, and where its
/// subtree starts.
#[derive(Debug)]
pub struct Expr<'s> {
    /// What the node is.
    pub kind: ExprKind<'s>,
    /// Where it was written, operands included.
    pub loc: Loc,
    /// The first node of this expression's subtree.
    pub first: ExprId,
}

impl<'s> Exprs<'s> {
    /// The nodes of `root`'s subtree, in evaluation order, ending with `root`.
    pub fn subtree(&self, root: ExprId) -> &[Expr<'s>] {
        let part = &self.parts[root.0 as usize >> PIECE_SHIFT];
        &part[part[root.position()].first.position()..=root.position()]
    }

    /// Every node of the file, in the order written.
    pub fn iter(&self) -> impl Iterator<Item = &Expr<'s>> {
        self.parts.iter().flatten()
    }

    /// How many nodes the file has in all.
    pub fn len(&self) -> usize {
        self.parts.iter().map(Vec::len).sum()
    }

    /// Whether the file has no expressions.
    pub fn is_empty(&self) -> bool {
        self.parts.iter().all(Vec::is_empty)
    }
}

impl<'s> Index<ExprId> for Exprs<'s> {
    type Output = Expr<'s>;
    fn index(&self, id: ExprId) -> &Expr<'s> {
        &self.parts[id.0 as usize >> PIECE_SHIFT][id.position()]
    }
}

/// What an expression node is. Children are ids of earlier nodes.
#[derive(Debug)]
pub enum ExprKind<'s> {
    /// `24_500`, `0.5`
    Num(Dec),
    /// `10%`, `3.5%`: the written number, not yet divided by 100.
    Pct(Dec),
    /// `24_500 USD`
    Amount(Amount<'s>),
    /// `2026-04-15`
    Date(Day),
    /// `30d`, `2w`, `3m`, `1y`
    Span(Span),
    /// String contents between the quotes, escapes not yet processed.
    Str(&'s str),
    /// `empty`: the zero of every commodity.
    Empty,
    /// Lowercase identifiers, paths and globs: `year`, `self`, `wages`,
    /// `expenses/food/*`, `401k`.
    Name(Name<'s>),
    /// `USD`
    Unit(Name<'s>),
    /// `#house`
    Code(Code<'s>),
    /// `self.purpose`: a field of the first.
    Field(ExprId, Name<'s>),
    /// `limit[year]`, `ordinary[year, owner.filing]`: the keys are `&file[keys]`.
    Index(ExprId, Many<ExprId>),
    /// `total(in, year)`, `progressive(ordinary[year], x)`: the arguments.
    Call(Name<'s>, Many<ExprId>),
    /// `-x`, `not x`.
    Unary(UnOp, ExprId),
    /// `a + b`: the operator, then the left and right operands.
    Binary(BinOp, ExprId, ExprId),
    /// `x is 401k | ira`: true when `x` matches any alternative.
    Is(ExprId, Many<ExprId>),
    /// `if c then a else b`
    If(ExprId, ExprId, ExprId),
    /// `0 USD 10% | 12_400 USD 12% | …`: thresholds and marginal rates.
    Schedule(Many<Bracket>),
}

/// One row of a schedule: from `threshold` up, tax at `rate`.
#[derive(Clone, Copy, Debug)]
pub struct Bracket {
    /// Where the bracket starts.
    pub threshold: ExprId,
    /// The marginal rate, a [`ExprKind::Pct`].
    pub rate: ExprId,
}

/// Prefix operators: `-x` and `not x`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum UnOp {
    /// `-x`
    Neg,
    /// `not x`
    Not,
}

/// Infix operators, loosest binding first.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum BinOp {
    /// `or`
    Or,
    /// `and`
    And,
    /// `==`
    Eq,
    /// `!=`
    Ne,
    /// `<`
    Lt,
    /// `<=`
    Le,
    /// `>`
    Gt,
    /// `>=`
    Ge,
    /// `+`
    Add,
    /// `-`
    Sub,
    /// `*`
    Mul,
    /// `/`
    Div,
}

impl BinOp {
    /// The operator as written.
    pub fn symbol(self) -> &'static str {
        match self {
            BinOp::Or => "or",
            BinOp::And => "and",
            BinOp::Eq => "==",
            BinOp::Ne => "!=",
            BinOp::Lt => "<",
            BinOp::Le => "<=",
            BinOp::Gt => ">",
            BinOp::Ge => ">=",
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
        }
    }
}
