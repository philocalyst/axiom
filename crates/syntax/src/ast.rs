//! The syntax tree.
//!
//! # Shape
//!
//! A [`File`] is flat. Its [`Item`]s are small (48 bytes at most), and nothing
//! inside any node owns a `Vec` or a `Box`. What varies in length lives in the
//! file's *tables*, one per node type, and a node reaches its variable part
//! through a typed reference:
//!
//! - [`Many<T>`] is a run of `T`s in their table: `&file[flow.body.legs]` is a
//!   `&[Leg]`.
//! - [`Ref<T>`] is one large, rare node in its own table: `&file[id]` is a
//!   `&Txn`, a `&Statement`, a `&Decl`, a `&Law`.
//! - [`ExprId`] is a node of the [`Exprs`] arena: `file.exprs[id]`.
//!
//! An [`ItemKind`] holds the `Ref` of the node it is, so reading a file is one
//! `match` per item:
//!
//! ```ignore
//! for item in &file.items {
//!     match item.kind {
//!         ItemKind::Txn(id) => {
//!             let txn = &file[id];
//!             for leg in &file[txn.flow.body.legs] { /* … */ }
//!         }
//!         ItemKind::Statement(id) => match file[id].verb { /* … */ },
//!         _ => {}
//!     }
//! }
//! ```
//!
//! References are opaque: use them only to index the file they came from. (A
//! large file is parsed in pieces, each with its own tables, and a reference
//! says which piece it belongs to as well as where.)
//!
//! # The two kinds of journal line
//!
//! A dated line is a **flow** (`->`, a [`Txn`]) or a **statement** (a
//! [`Statement`]): one thing said about one thing on a day. What a statement
//! says is read from the shape of what follows its subject, so the parser knows
//! that `01 flat` is an occurrence and `07-01 flat now 3_050 USD monthly` new
//! terms without knowing what a `flat` is; that is for the model.
//!
//! # Text and locations
//!
//! The tree borrows the source and copies no text: [`Name`], [`Code`],
//! [`Literal`], [`Text`] and [`Doc`] are slices of it. A slice knows where it was
//! written, so none of them stores a location: [`File::loc`] recovers it from
//! the slice's address. Nodes that are not a single slice (a leg, a step, a
//! clause) store a [`Loc`].
//!
//! # Expressions
//!
//! Expressions live in one post-order arena per file, [`Exprs`]: children
//! precede their parent, so a subtree is the contiguous run of nodes
//! `first..=root`. Later phases keep this shape, which makes type checking and
//! evaluation a single forward scan in which every subexpression's result is
//! already at hand.

use std::ops::{Deref, Index};

use axiom_core::{Day, Dec, FileId, Loc, Span};

pub use crate::refs::{Many, Ref};

// The calendar words a plan and a law's `each` trigger are written in are core's.
pub use axiom_core::{On, Period};

// ─── Text ───────────────────────────────────────────────────────────────────

/// A written name, path, glob or commodity: the slice of the source it is.
/// Accounts, owners, parties, assets, purposes, kinds and properties are
/// lowercase (`checking`, `trader-joes`, `joint/savings`, `food/*`);
/// commodities are uppercase (`USD`).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Name<'s>(pub &'s str);

/// A written `^code`, `^` included. Its location covers the `^` too.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Code<'s>(pub &'s str);

/// A raw block of consecutive `///` lines, prefixes included. [`Doc::lines`]
/// strips them without allocating.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Doc<'s>(pub &'s str);

/// A written quantity, exactly as it stands in the source: `84.20 USD`,
/// `empty`, or, after `=`, `-50 USD`. Storing the text keeps it at 16 bytes;
/// [`Literal::num`] and [`Literal::unit`] read it back.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Literal<'s>(pub &'s str);

/// A string's contents between the quotes, escapes not yet processed, or a
/// line of raw text (a command, a path): what was written and nothing more.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Text<'s>(pub &'s str);

/// The text of each of these is what was written: `&*name` is a `&str`.
macro_rules! written {
    ($($ty:ident),+) => { $(impl Deref for $ty<'_> { type Target = str; fn deref(&self) -> &str { self.0 } })+ };
}
written!(Name, Code, Doc, Literal, Text);

impl<'s> Code<'s> {
    /// The code without its `^`: `check-1041`.
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

impl<'s> Literal<'s> {
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

/// What a file's place in its project says of the dates written in it (§10): a
/// file in `journal/2026/03.ax` holds March 2026, so its items may be dated
/// `15`. A heading (`2026-04`) gives the lines below it a year and month of its
/// own. The tree holds whole days, so nothing after the parser sees the
/// difference. The default, where nothing is given, is a file that writes every
/// date in full.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Folder {
    /// The year the file holds.
    pub year: Option<i32>,
    /// The month of that year, when the file holds one.
    pub month: Option<u8>,
}

impl Folder {
    /// What the path of a file, relative to its project's root (`journal/2026/03.ax`),
    /// gives its dates: a folder or file named `YYYY` gives the year, and `MM`
    /// directly beneath it (a folder, or `MM.ax`), or a file `YYYY-MM.ax`, gives the month.
    pub fn of(path: &str) -> Folder {
        let (mut folder, mut year_at) = (Folder::default(), 0);
        for (at, segment) in path.strip_suffix(".ax").unwrap_or(path).split('/').enumerate() {
            match (folder.year, Named::of(segment)) {
                (None, Some(Named::YearMonth(year, month))) => return Folder { year: Some(year), month: Some(month) },
                (None, Some(Named::Year(year))) => (folder.year, year_at) = (Some(year), at),
                (Some(_), Some(Named::Month(month))) if at == year_at + 1 => folder.month = Some(month),
                (Some(_), _) if at == year_at + 1 => folder.month = None,
                _ => {}
            }
        }
        folder
    }
}

/// What one segment of a path says of the dates written below it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Named {
    /// `2026`: four digits.
    Year(i32),
    /// `03`: two digits, of a month.
    Month(u8),
    /// `2026-03`.
    YearMonth(i32, u8),
}

impl Named {
    /// The period a segment names, if it names one.
    pub fn of(segment: &str) -> Option<Named> {
        let digits = |text: &str, width: usize| {
            let all = text.len() == width && text.bytes().all(|byte| byte.is_ascii_digit());
            all.then(|| text.parse::<i32>().ok()).flatten()
        };
        let month = |text: &str| digits(text, 2).filter(|month| (1..=12).contains(month)).map(|month| month as u8);
        match segment.split_once('-') {
            Some((year, text)) => Some(Named::YearMonth(digits(year, 4)?, month(text)?)),
            None => digits(segment, 4).map(Named::Year).or_else(|| month(segment).map(Named::Month)),
        }
    }
}

// ─── The file and its tables ────────────────────────────────────────────────

/// One parsed source file: its items in source order, and the tables that hold
/// what the items are made of.
#[derive(Default, Debug)]
pub struct File<'s> {
    /// Which file this is, as its locations say.
    pub id: FileId,
    /// The source text. Every [`Name`], [`Code`], [`Literal`] and [`Doc`] is a
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

    /// This file laid out in the house style, using the source it borrows.
    pub fn format(&self) -> String {
        crate::style::format(self.src, self)
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

/// A type that lives in one of a piece's tables, so that a [`Many`] or a
/// [`Ref`] of it can index the [`File`]. Sealed: the tree's own nodes are the
/// only ones, and only the parser adds to the tables.
pub trait Stored<'s>: Sized + Table<'s> {}

/// Where a [`Stored`] type is kept. Nothing outside this crate can name it.
pub(crate) use table::Table;

mod table {
    use super::Tables;

    pub trait Table<'s>: Sized {
        /// The table of this type in a piece's tables.
        fn table<'t>(tables: &'t Tables<'s>) -> &'t Vec<Self>;
        /// The same, to add to it while parsing.
        fn table_mut<'t>(tables: &'t mut Tables<'s>) -> &'t mut Vec<Self>;
    }
}

impl<'s, T: Stored<'s>> Index<Many<T>> for File<'s> {
    type Output = [T];
    fn index(&self, many: Many<T>) -> &[T] {
        &T::table(&self.tables[many.piece()])[many.range()]
    }
}

impl<'s, T: Stored<'s>> Index<Ref<T>> for File<'s> {
    type Output = T;
    fn index(&self, id: Ref<T>) -> &T {
        &T::table(&self.tables[id.piece()])[id.local()]
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

        $(
            impl<'s> Table<'s> for $ty {
                fn table<'t>(tables: &'t Tables<'s>) -> &'t Vec<Self> { &tables.$field }
                fn table_mut<'t>(tables: &'t mut Tables<'s>) -> &'t mut Vec<Self> { &mut tables.$field }
            }
            impl<'s> Stored<'s> for $ty {}
        )+
    };
}

tables! {
    /// [`ItemKind::Txn`]
    txns: Txn<'s>,
    /// [`ItemKind::Statement`], and the claims of openings.
    statements: Statement<'s>,
    /// [`Change::Terms`]
    terms: Terms<'s>,
    /// [`Change::Budget`], and the budgets of purposes.
    allowances: Allowance<'s>,
    /// [`ItemKind::Opening`]
    openings: Opening<'s>,
    /// [`ItemKind::Contract`]
    contracts: Contract<'s>,
    /// The days of schedules: [`Terms::on`].
    days: On,
    /// [`ItemKind::Decl`]
    decls: Decl<'s>,
    /// [`ItemKind::Budget`]
    budgets: Budget<'s>,
    /// [`ItemKind::Code`]
    rules: CodeRule<'s>,
    /// [`ItemKind::Param`]
    params: Param<'s>,
    /// [`ItemKind::Sync`]
    syncs: Sync<'s>,
    /// [`ItemKind::Format`], and the formats of syncs.
    formats: Format<'s>,
    /// The lines of formats: [`Format::lines`].
    format_lines: FormatLine<'s>,
    /// The arguments of format lines: [`FormatLine::args`].
    format_args: FormatArg<'s>,
    /// [`ItemKind::Pattern`]
    named_patterns: NamedPattern<'s>,
    /// The patterns of `known-as` lines: [`Decl::known_as`], [`CodeRule::known_as`].
    patterns: Pattern<'s>,
    /// The choices of patterns: [`Pattern::choices`].
    sequences: Sequence<'s>,
    /// The terms of sequences: [`Sequence::terms`].
    pattern_terms: PatternTerm<'s>,
    /// [`ItemKind::Setting`]
    settings: Setting<'s>,
    /// Top-level laws ([`ItemKind::Law`]) and the laws nested in declarations.
    laws: Law<'s>,
    /// The legs of flows, occurrences and openings: [`Body::legs`].
    legs: Leg<'s>,
    /// The items under flows, occurrences, claims and contracts: [`Body::items`].
    items: LineItem<'s>,
    /// The selectors of ends: [`End::select`].
    selects: Select<'s>,
    /// The clauses of tails: [`Flow::tail`].
    clauses: Clause<'s>,
    /// The `also` lines of declarations: [`Contract::alsos`], [`Decl::alsos`].
    alsos: Also<'s>,
    /// The property lines of declarations, contracts and statements:
    /// [`Decl::props`], [`Contract::props`].
    props: Prop<'s>,
    /// The property lines under a property: [`Prop::lines`].
    nested: Nested<'s>,
    /// The rows of parameters: [`Param::rows`].
    rows: ParamRow<'s>,
    /// The keys of parameter rows: [`ParamRow::keys`].
    keys: Key<'s>,
    /// The steps of laws: [`Law::steps`].
    steps: Step<'s>,
    /// The effects of `else` chains: [`StepKind::Require`].
    effects: Effect<'s>,
    /// The rows of schedules: [`ExprKind::Schedule`].
    brackets: Bracket,
    /// Runs of expression roots: property arguments, call arguments, index
    /// keys and the alternatives of `is`.
    ids: ExprId,
    /// The globs and kinds of a code rule's `on` lines: [`CodeRule::on`]. The kinds or words of a slot's range:
    /// [`Takes`].
    names: Name<'s>,
    /// The slots of declarations: [`Decl::slots`].
    slots: Has<'s>,
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
/// (A heading, a line of only a year or a month, is not an item: it says what
/// the dates below it leave out, and the tree holds whole days.)
#[derive(Clone, Copy, Debug)]
pub enum ItemKind<'s> {
    /// `DATE FLOW`
    Txn(Ref<Txn<'s>>),
    /// `DATE SUBJECT VERB …`: one thing said about one thing on a day.
    Statement(Ref<Statement<'s>>),
    /// `opening DATE`
    Opening(Ref<Opening<'s>>),
    /// `contract NAME [with PARTY]`
    Contract(Ref<Contract<'s>>),
    /// `account`, `entity`, `asset`, `purpose`, `commodity` or `kind`. One line
    /// naming several entities is one `Decl` per entity, all sharing the same
    /// properties.
    Decl(Ref<Decl<'s>>),
    /// `budget PURPOSE LIMIT monthly|yearly [carries] [funded from H into H]`
    Budget(Ref<Budget<'s>>),
    /// `code GLOB`. A line with several globs is one rule per glob, all
    /// sharing the same places.
    Code(Ref<CodeRule<'s>>),
    /// `param NAME [UNIT]`
    Param(Ref<Param<'s>>),
    /// A top-level `law NAME`. Its doc is also the item's.
    Law(Ref<Law<'s>>),
    /// `sync NAME`
    Sync(Ref<Sync<'s>>),
    /// `pattern NAME = PATTERN`
    Pattern(Ref<NamedPattern<'s>>),
    /// `format NAME`
    Format(Ref<Format<'s>>),
    /// `system`, `use`, `base`, `relaxed`, `currency` or `rates`.
    Setting(Ref<Setting<'s>>),
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
    /// `currency UNIT`: what a system's laws count in.
    Currency(Name<'s>),
    /// `rates spot` or `rates param NAME`: how a system converts between units.
    Rates(Rates<'s>),
}

/// How a system converts one unit to another.
#[derive(Clone, Copy, Debug)]
pub enum Rates<'s> {
    /// `spot`: the day's price.
    Spot,
    /// `param NAME`: a param's rows, such as the IRS's yearly averages.
    Param(Name<'s>),
}

// ─── Amounts ────────────────────────────────────────────────────────────────

/// An amount as LANGUAGE §4 writes it. The commonest by far is a literal, which
/// takes no node of the expression arena (and keeps an amount two words wide);
/// every other form does.
#[derive(Clone, Copy, Debug)]
pub enum Amount<'s> {
    /// `84.20 USD`, `empty`, or, after `=`, `-50 USD`.
    Literal(Literal<'s>),
    /// Anything else, as the root of an expression: `12%` (an [`ExprKind::Pct`]
    /// alone is of the amount of the header it is under), `12% of ^bldg-water`,
    /// `1/3 of ^pge-jan`, `^inv-12[HR] @ 150 USD/HR`, `X up to Y`; and, in a
    /// declaration, any expression of the law grammar.
    Computed(ExprId),
}

// ─── Flows ──────────────────────────────────────────────────────────────────

/// `DATE FLOW`.
#[derive(Debug)]
pub struct Txn<'s> {
    /// The day value moves.
    pub date: Day,
    /// What moves, from where to where, and why.
    pub flow: Flow<'s>,
}

/// `SOURCE -> TARGET TAIL` with optional indented legs and items.
///
/// When both sides name an end there are no legs. When exactly one side does,
/// the legs are the other side (a "one side split"), and the header may state
/// an amount on either or both sides: `house 1 HOME -> 431_500 USD`. With no
/// legs, a source and both amounts are an exchange that stays at the source:
/// `fidelity 20 VTI -> 5_940 USD`. A flow with no date is what an `also` line
/// of a declaration says.
#[derive(Debug)]
pub struct Flow<'s> {
    /// What leaves: the left of the arrow.
    pub from: Side<'s>,
    /// What arrives: the right of the arrow.
    pub to: Side<'s>,
    /// What the header says about the whole flow, which applies to every leg:
    /// `&file[flow.tail]`. A `DATE..DATE` spread is the clause `for DATE..DATE`.
    pub tail: Many<Clause<'s>>,
    /// The indented lines under the header.
    pub body: Body<'s>,
}

/// The indented lines under a header that says a flow, an occurrence or new
/// terms: the legs that spell out a side, and the items that add to the
/// header's amount, carve it up or take from it (LANGUAGE §3).
#[derive(Clone, Copy, Debug, Default)]
pub struct Body<'s> {
    /// `&file[body.legs]`
    pub legs: Many<Leg<'s>>,
    /// `&file[body.items]`
    pub items: Many<LineItem<'s>>,
}

/// One side of a header: `checking`, `checking 2_000 USD`, `7 VTI`, or nothing.
#[derive(Debug)]
pub struct Side<'s> {
    /// The end, or `None` when the side is left to the legs or to inference.
    pub end: Option<End<'s>>,
    /// How much, or `None` when the side states none.
    pub amount: Option<Quantity<'s>>,
}

/// One end of a flow as written: `brokerage[fifo, 2024]`, `trader-joes`, `VTI`.
/// The parser cannot tell an account, an owner, a party or a promise apart:
/// all are names. A commodity in party position (`VTI -> fidelity 198.12 USD`,
/// a fund that pays) is a name in capitals, and `?` is the unknown party.
#[derive(Clone, Copy, Debug)]
pub struct End<'s> {
    pub name: Name<'s>,
    /// Which parcels of it the flow addresses: `&file[end.select]`.
    pub select: Many<Select<'s>>,
}

/// A selector: what narrows an account (which parcels leave it) or a reference
/// (which parts of a fact). Days, months and years are normalized to inclusive
/// ranges.
#[derive(Clone, Copy, Debug)]
pub enum Select<'s> {
    /// What happened or was acquired on these days, first and last, with where
    /// it was written: `2024`, `2026-01`, `2026-01-22`, `2026-01..2026-06`.
    Range(Day, Day, Loc),
    /// What a code names: `[^inv-12]`.
    Code(Code<'s>),
    /// A lot policy, with where it was written: `[fifo]`.
    Policy(Policy, Loc),
    /// The parts of a purpose: `[#design]`.
    Purpose(Name<'s>),
    /// The parts in a unit: `[HR]`.
    Unit(Name<'s>),
    /// The leg to an end: `[retirement]`, a name that is no policy.
    End(Name<'s>),
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
    /// The parcel that holds exactly what is asked, else the oldest first: how a claim is settled.
    Exact,
}

impl Policy {
    /// Every policy, by the word that names it: the one table of them.
    pub const WORDS: [(&'static str, Policy); 5] = [
        ("fifo", Policy::Fifo),
        ("lifo", Policy::Lifo),
        ("hifo", Policy::Hifo),
        ("prorata", Policy::Prorata),
        ("exact", Policy::Exact),
    ];
}

/// How much moves on one side or leg.
#[derive(Clone, Copy, Debug)]
pub enum Quantity<'s> {
    /// `84.20 USD`, `6%`, `12% of ^bldg-water`: an amount of §4.
    Amount(Amount<'s>),
    /// `(350 USD)`: written, not yet real. The amount's location excludes the
    /// parentheses.
    Pending(Amount<'s>),
    /// `= 5_000 USD` (legs only): whatever makes the end's balance equal this
    /// after the flow.
    Target(Amount<'s>),
    /// `? USD`: inferred from surrounding values. The unit.
    Unknown(Name<'s>),
    /// `all` or `all VXUS`: everything the selected parcels hold, of that unit
    /// if one is named.
    All(Option<Name<'s>>),
    /// `...` (legs only): whatever balances the transaction.
    Rest,
    /// No amount at all (opening lines only): the thing itself, an asset held
    /// at its `basis`.
    Whole,
}

/// An indented line under a flow that names an end: `retirement 800 USD ^pretax`.
/// The same line is a leg of an occurrence's overrides (where `NAME = AMOUNT`
/// states an input, or a target balance: the model tells them apart), of a
/// contract's template, of an [`Opening`] (`fidelity 210 VTI basis 48_300 USD
/// since 2021-06-01`) and of a return (`wages 124_200.00 USD`).
#[derive(Debug)]
pub struct Leg<'s> {
    /// The `///` block above it.
    pub doc: Option<Doc<'s>>,
    /// Where the leg's value goes (or, in an opening, what holds it).
    pub end: End<'s>,
    /// How much: an amount, pending, a target balance, or the remainder.
    pub amount: Quantity<'s>,
    /// The leg's own tail, in addition to the header's.
    pub tail: Many<Clause<'s>>,
    /// The whole line, trailing comment excluded.
    pub loc: Loc,
}

/// An indented line under a flow that names no end (LANGUAGE §3): a part of the
/// header's amount between the same two ends, with its own purpose, description
/// and codes. `32.10 USD #groceries` is carved out of the header's amount,
/// `+ 12% of ^bldg-water #utilities` comes on top of it, and `- 6%
/// #selling-costs` is taken off it.
#[derive(Debug)]
pub struct LineItem<'s> {
    /// The `///` block above it.
    pub doc: Option<Doc<'s>>,
    /// How it relates to the header's amount.
    pub sign: Sign,
    /// How much.
    pub amount: Amount<'s>,
    /// The item's own tail: what it is for, in words and in codes.
    pub tail: Many<Clause<'s>>,
    /// The whole line, trailing comment excluded.
    pub loc: Loc,
}

/// How a [`LineItem`] relates to the amount of the header above it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sign {
    /// No sign: carved out of the header's amount.
    Carve,
    /// `+`: comes on top of it.
    Add,
    /// `-`: taken off it.
    Less,
}

/// One clause of a tail, and where it was written (keyword through value). A
/// tail is what may follow a header's, leg's, item's or statement's amounts, in
/// any order: `#repair of condo "sink" ^inv-12 for 2025 due 30d against ^inv-11
/// via paypal basis 3_000 USD @ 2 USD !`. A header's tail applies to every leg.
/// The parser rejects a repeated clause, except a code, so no other kind occurs
/// twice.
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
    /// `#groceries`, `#repair of condo`: what the event is for.
    Purpose(Purpose<'s>),
    /// `"food for the routine"`: says why in words, and means nothing to the book.
    Description(Text<'s>),
    /// `^inv-12`: names the flow, leg, item or statement so that others can
    /// refer to it. Any number.
    Code(Code<'s>),
    /// `for WHAT`: what the parser cannot tell apart is the model's to say.
    For(For<'s>),
    /// `due WHEN`: the flow is a claim due then.
    Due(Due),
    /// `against ^inv-11`: the flow is about that earlier one: it refunds it, or
    /// reimburses it.
    Against(Code<'s>),
    /// `via paypal`: the intermediary the money passed through. The party at
    /// the flow's end is who it was for.
    Via(Name<'s>),
    /// `basis 3_000 USD`: the total basis the arriving parcels take.
    Basis(Amount<'s>),
    /// `@ 285.70 USD`: the price of one unit of the commodity that arrives.
    Price(Literal<'s>),
    /// `since 2023-06-15`: when an opening line's parcels were acquired.
    Since(Day),
    /// `until 2026-05-31`: the last day a change holds. Only a statement's tail
    /// has one, and only a change (`now`) or a waiver.
    Until(Day),
    /// `!` or `! "reason"`
    Waive(Waive<'s>),
}

/// `#NAME [of THING]`: what an event is for, and what it is for it to have.
#[derive(Clone, Copy, Debug)]
pub struct Purpose<'s> {
    /// The purpose, without its `#`.
    pub name: Name<'s>,
    /// `of condo`: the thing a purpose takes as its object (`#improvement`).
    pub of: Option<Name<'s>>,
}

/// What an event is on account of.
#[derive(Clone, Copy, Debug)]
pub enum For<'s> {
    /// `for 2025`, `for 2026-03`, `for 2026-03-15`, `for 2026-01-01..2026-12-31`:
    /// the period the event is recognized over, as first and last day.
    Period(Day, Day),
    /// `for last month`: the period before the day of the line.
    Last(Relative),
    /// `for car-fund`, `for lumen`, `for halcyon`: an entity or an envelope,
    /// which the model tells apart.
    Whom(Name<'s>),
}

/// The calendar period before a day: `last month`, `last quarter`, `last year`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Relative {
    Month,
    Quarter,
    Year,
}

/// When a claim falls due.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Due {
    /// `due 2026-04-15`, or `due 04-15` (the first such day on or after the
    /// line's own).
    On(Day),
    /// `due 30d`: this long after the day of the line.
    After(Span),
}

/// `!` or `! "reason"`: accept this item's law violations (or, on a value, its
/// gap) explicitly.
#[derive(Clone, Copy, Debug)]
pub struct Waive<'s> {
    /// The `!`, and the string after it if there is one.
    pub at: Loc,
    /// The string's contents.
    pub reason: Option<Text<'s>>,
}

// ─── Statements ─────────────────────────────────────────────────────────────

/// `DATE SUBJECT VERB …` (LANGUAGE §5): one thing said about one thing on a
/// day. It moves no value itself; what follows from it is derived.
///
/// The word after the subject says what kind of line it is, and the parser
/// reads the rest by that word and never by what the subject's name means: `01
/// flat` is an occurrence whether or not `flat` is a contract.
#[derive(Debug)]
pub struct Statement<'s> {
    /// The day it says it on.
    pub date: Day,
    /// What it is said about.
    pub subject: Subject<'s>,
    /// What is said.
    pub verb: Verb<'s>,
    /// What else the line says, in any order: a description, codes, and for
    /// the verbs that take them a purpose, `for`, `due`, `against`, `via`, `!`,
    /// and `until` (of a change or a waiver): `&file[statement.tail]`.
    pub tail: Many<Clause<'s>>,
    /// The indented lines under it: legs and items under an occurrence or new
    /// terms, items under a claim, an amendment or a waiver, tally lines under
    /// a return, and nothing under anything else.
    pub body: Body<'s>,
}

/// What a statement is about.
#[derive(Clone, Copy, Debug)]
pub enum Subject<'s> {
    /// An account, owner, party, promise, asset or system: the parser cannot tell.
    Name(Name<'s>),
    /// `^promo`: what a code names.
    Code(Code<'s>),
    /// `#food`: a purpose.
    Purpose(Name<'s>),
    /// A unit: `VTI`.
    Unit(Name<'s>),
}

/// The word after a statement's subject, and what follows it: one variant for
/// each kind of line of LANGUAGE §2 that is not a flow.
#[derive(Debug)]
pub enum Verb<'s> {
    /// No verb: `01 flat`, `08 phone 47.30 USD`. The promise was kept once, for
    /// this amount instead if one is written (for a `buy`, it is what was
    /// bought). The tail may say more of it (`15 estimates 8_800 USD for 2025`).
    Occurrence(Option<Amount<'s>>),
    /// `= 8_828.87 USD`: what the subject is worth or holds at the end of the
    /// day: a balance, a price (`VTI = 280.14 USD`), a named measure (`^bldg-water
    /// = 155.00 USD`). A `via` or `!` clause of the tail says where a gap goes.
    Value(Amount<'s>),
    /// `owes studio 3_800 USD`: a claim the subject has on `creditor`, which is
    /// the sum of the items under it when no amount is written. Which of the two
    /// is an owner is the model's to say.
    Owes { creditor: Name<'s>, amount: Option<Amount<'s>> },
    /// `now …`: the subject's declaration, restated from this day.
    Now(Change<'s>),
    /// `worked 6.5 HR`: work done. The tail says what for and for whom.
    Worked(Literal<'s>),
    /// `used 44 MI`: a thing used. The tail says what for and for whom.
    Used(Literal<'s>),
    /// `waived`: the occurrence due that day, or with `until` every one in the
    /// span, is kept at nothing; a claim is forgiven, for a purpose and items if
    /// part of it is recoverable.
    Waived,
    /// `ends`: an account, promise or asset is done.
    Ends,
    /// `settled`, `void` or `returned`, of what a code names.
    Event(EventState),
    /// `split 2 for 1`, of a commodity: every parcel is multiplied by
    /// `numerator / denominator`, keeping basis and acquisition day.
    Split { numerator: Dec, denominator: Dec },
    /// `basis 12_000 USD since 2019-03-01`: an asset arriving unbought.
    Basis { amount: Amount<'s>, since: Option<Day> },
    /// `filed 2025`, of a system: a return as filed, with the tally lines that
    /// are the body.
    Filed(i32),
}

/// What follows `now`: part of the subject's declaration.
#[derive(Debug)]
pub enum Change<'s> {
    /// `3_050 USD monthly`: what the contract promises from this day on. What
    /// it leaves out carries over. Legs and items under it are the new template.
    Terms(Ref<Terms<'s>>),
    /// `NAME ARG*`, as a declaration's property line is: `share 20% for studio`,
    /// `at 6.25%`, `lives us/ny`, `due 05-15`, `until 08-31`.
    Property(Prop<'s>),
    /// `budget 1_200 USD monthly`, of a purpose.
    Budget(Ref<Allowance<'s>>),
    /// Nothing: the items under it amend a claim, as a credit note does.
    Amendment,
}

/// The terms a contract promises, on its schedule line or restated by a
/// change: `[about] AMOUNT CADENCE [on DAY, …] [(from | into) NAME]`, or `buy
/// UNIT for AMOUNT CADENCE …` for a standing order.
#[derive(Clone, Copy, Debug)]
pub struct Terms<'s> {
    /// `about`: the amount varies (a utility bill): each occurrence states its
    /// own, and the forecast uses this one.
    pub about: bool,
    /// What each occurrence pays; `None` when the contract's `loan` says.
    pub payment: Option<Payment<'s>>,
    pub cadence: Cadence,
    /// The days of each period: `&file[terms.on]`. None means the period's own start.
    pub on: Many<On>,
    /// Where the money goes or comes from. A contract's schedule always says;
    /// terms that restate it may leave it as it was.
    pub holding: Option<Holding<'s>>,
}

/// What an occurrence pays.
#[derive(Clone, Copy, Debug)]
pub enum Payment<'s> {
    /// `45 USD`
    Fixed(Amount<'s>),
    /// `buy VTI for 500 USD`: what it spends each time is fixed and what it
    /// buys is not, so the occurrence says (`20 vti-monthly 1.620 VTI`).
    Buy { unit: Name<'s>, spend: Amount<'s> },
}

/// `from checking`, `into checking`: an account, or an owner's own hand (the
/// parser cannot tell), and which way the money goes.
#[derive(Clone, Copy, Debug)]
pub struct Holding<'s> {
    pub direction: Direction,
    pub name: Name<'s>,
}

/// How often. `daily` is `every 1d`, `weekly` `every 7d`, `monthly` `every 1m`,
/// `quarterly` `every 3m` and `yearly` `every 12m`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cadence {
    Every(Span),
    /// `twice monthly`: two days in each month, `on 15, last`.
    TwiceMonthly,
}

/// Which way a schedule's money goes, for the holding.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    /// `from checking`: the holding pays.
    From,
    /// `into checking`: the holding receives.
    Into,
}

/// What a `DATE ^code STATE` line does to the flows carrying that code.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum EventState {
    /// Pending becomes actual on this day.
    Settled,
    /// Pending never happens.
    Void,
    /// Actual is reversed on this day.
    Returned,
}

/// `LIMIT monthly|yearly [carries] [funded from HOLDING into HOLDING]`: what a
/// purpose may spend in each window.
#[derive(Clone, Copy, Debug)]
pub struct Allowance<'s> {
    pub limit: Limit<'s>,
    /// The month or year the limit is for.
    pub per: Period,
    /// `carries`: what one window leaves unspent the next may spend, and what
    /// one overspends the next must make up.
    /// `Some(true)` means `carries` was written. `None` leaves a prior
    /// allowance's carry setting unchanged in a dated restatement.
    pub carries: Option<bool>,
    /// `funded from H into H`: the limit moves each window into money held for
    /// it, which the purpose's spending is drawn from first.
    pub funded: Option<Funding<'s>>,
}

/// `from HOLDING into HOLDING`
#[derive(Clone, Copy, Debug)]
pub struct Funding<'s> {
    pub from: Name<'s>,
    pub into: Name<'s>,
}

/// How much a budget allows.
#[derive(Clone, Copy, Debug)]
pub enum Limit<'s> {
    /// `900 USD`
    Amount(Amount<'s>),
    /// `10% of #income`: a share of another purpose's total in the same window.
    Share { percent: Dec, of: Name<'s> },
}

/// `opening DATE` and its indented lines: what the owners hold from that day.
/// Each line is a [`Leg`] with an amount, or an asset's `basis`, and `basis`
/// and `since` clauses; or a claim that is already open.
#[derive(Debug)]
pub struct Opening<'s> {
    /// The day the balances are stated: they exist from then on.
    pub date: Day,
    /// One line per holding: `&file[opening.lines]`.
    pub lines: Many<Leg<'s>>,
    /// One line per open claim, each an `owes` statement dated with the
    /// opening's own day: `&file[opening.claims]`.
    pub claims: Many<Statement<'s>>,
}

// ─── Contracts ──────────────────────────────────────────────────────────────

/// `contract NAME [with PARTY]` and its indented lines: a promise of flows with
/// one party. The schedule lines come first; the rest come in any order.
#[derive(Debug)]
pub struct Contract<'s> {
    pub name: Name<'s>,
    /// Who the promise is with: `None` is the entity of the contract's own name.
    pub party: Option<Name<'s>>,
    /// How often and for how much. `None` when the line is missing or did not
    /// parse, which its own diagnostic says; the contract is kept all the same,
    /// so that its occurrences are not errors too.
    pub schedule: Option<Schedule<'s>>,
    /// A standing order: `buy VTI for 500 USD monthly on 20 from checking`.
    pub standing: Option<Schedule<'s>>,
    /// `#rent of condo`: what each flow is for, if not what the party's is.
    /// Written on the schedule line or on a line of its own.
    pub purpose: Option<Purpose<'s>>,
    /// A description of its flows, where the purpose may be written.
    pub description: Option<Text<'s>>,
    /// The deadline after each due day: `due 5d else + 5% #late-fee`.
    pub deadline: Option<Deadline<'s>>,
    /// What every occurrence implies: `also -> escrow 410 USD #escrow`.
    /// `&file[contract.alsos]`
    pub alsos: Many<Also<'s>>,
    /// The other property lines: `from`, `until`, `grace`, `for`, `covers`,
    /// `prorated`, `rising`, `indexed`, `share`, `input`, `deposit`, `area`,
    /// `loan`, which the model reads with the properties of other declarations
    /// (a party's or a purpose's `share` is the same line): `&file[contract.props]`.
    pub props: Many<Prop<'s>>,
    /// The template: legs as in a split flow, of which an occurrence overrides
    /// those of the same end, and items that add to it.
    pub body: Body<'s>,
    /// The nested laws.
    pub laws: Many<Law<'s>>,
    /// A line of its body did not parse and is left out, so what remains is
    /// not the whole contract: anything that only follows from the missing line
    /// is not worth another diagnostic.
    pub damaged: bool,
}

/// A contract's schedule line, whose [`Terms`] always have a holding.
#[derive(Clone, Copy, Debug)]
pub struct Schedule<'s> {
    /// The whole line.
    pub at: Loc,
    pub terms: Terms<'s>,
}

/// `due SPAN [else ITEM]`: how long after a due day a promise is still kept,
/// and what is added when it passes.
#[derive(Debug)]
pub struct Deadline<'s> {
    pub span: Span,
    pub otherwise: Option<LineItem<'s>>,
}

/// `also ITEM | FLOW [when EXPR]`: what always comes with something. A flow
/// with no date, or an item, whose amounts are expressions of the law grammar.
#[derive(Debug)]
pub struct Also<'s> {
    /// What comes with it.
    pub line: AlsoLine<'s>,
    /// `when EXPR`: only where this holds.
    pub when: Option<ExprId>,
    /// The whole line.
    pub loc: Loc,
}

/// What an [`Also`] adds.
#[derive(Debug)]
pub enum AlsoLine<'s> {
    /// `- 2.9% + 0.30 USD #fees`: an item of every flow.
    Item(LineItem<'s>),
    /// `-> escrow 410 USD #escrow`, `lumen -> retirement 50% of amount #match`.
    Flow(Flow<'s>),
}

// ─── Declarations ───────────────────────────────────────────────────────────

/// `account|entity|asset|purpose|commodity|kind NAME [: KIND]` with indented
/// properties and laws. (`account NAME : KIND at NAME` and `entity NAME : KIND
/// #PURPOSE` are the ones with more.)
#[derive(Debug)]
pub struct Decl<'s> {
    /// Which keyword introduced it.
    pub what: DeclKind,
    /// The name, symbol or kind it declares.
    pub name: Name<'s>,
    /// After `:`: the kind (or, for `purpose` and `kind`, the parent).
    pub kind: Option<Name<'s>>,
    /// `account NAME : KIND at NAME`: the institution the account is with.
    pub at: Option<Name<'s>>,
    /// `entity NAME #PURPOSE`: what flows with this party are for.
    pub purpose: Option<Name<'s>>,
    /// `budget 900 USD monthly`, under a purpose.
    pub budget: Option<Ref<Allowance<'s>>>,
    /// `known-as "TRADER JOE"`: how it appears on statements. `&file[decl.known_as]`
    pub known_as: Many<Pattern<'s>>,
    /// What every event of it implies: `also issuer -> self 2% of amount #rebate`.
    pub alsos: Many<Also<'s>>,
    /// The indented property lines: `&file[decl.props]`.
    pub props: Many<Prop<'s>>,
    /// The slots a kind declares: `has employer employer`. `&file[decl.slots]`
    pub slots: Many<Has<'s>>,
    /// The nested laws.
    pub laws: Many<Law<'s>>,
}

/// Which keyword introduced a declaration.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum DeclKind {
    /// `account`: a position with an institution.
    Account,
    /// `entity`: an owner or a party.
    Entity,
    /// `asset`: an identified thing.
    Asset,
    /// `purpose`: a node of the tree of what events are for.
    Purpose,
    /// `commodity`: a currency, a security or a measure.
    Commodity,
    /// `kind`: a class that accounts, entities, assets or commodities belong to.
    Kind,
}

/// `budget PURPOSE LIMIT monthly|yearly [carries] [funded from H into H]`: a
/// warning on the purpose's total, from the book's first day. A change `DAY
/// #PURPOSE now budget …` restates it.
#[derive(Debug)]
pub struct Budget<'s> {
    pub purpose: Name<'s>,
    pub allowance: Allowance<'s>,
}

/// `NAME ARG*`: arguments are expressions, commas skipped. `has born date`,
/// `holds USD, EUR`, `lives us/ca from 2026-01-01`, `share 120 SQFT for studio`.
#[derive(Debug)]
pub struct Prop<'s> {
    /// The property's name: `has`, `lives`, `share`.
    pub name: Name<'s>,
    /// The roots of the argument expressions: `&file[prop.args]`.
    pub args: Many<ExprId>,
    /// Indented property lines under it: what `loan` says of its `resets` and
    /// `prepay`. `&file[prop.lines]`
    pub lines: Many<Nested<'s>>,
    /// The whole line.
    pub loc: Loc,
}

/// A property line under a property.
#[derive(Debug)]
pub struct Nested<'s>(pub Prop<'s>);

/// `has NAME RANGE [MULT] [by WEIGHT]`: a slot that the things of a kind have, what it takes, how many it takes and
/// what weighs them. `has beneficiary person`, `has coverage one of self-only | family`, `has owners person some by
/// share`.
#[derive(Debug)]
pub struct Has<'s> {
    pub name: Name<'s>,
    pub takes: Takes<'s>,
    pub mult: Mult,
    pub weight: Option<Weight<'s>>,
    /// The whole line.
    pub loc: Loc,
}

/// What a slot takes.
#[derive(Clone, Copy, Debug)]
pub enum Takes<'s> {
    /// `person | household`, or the one word of a value type such as `date`: which a name is, is for the model, which
    /// knows the kinds.
    Names(Many<Name<'s>>),
    /// `one of self-only | family`.
    Words(Many<Name<'s>>),
    /// `USD` or `USD/MI`: a quantity counted in a commodity, or in a rate between two.
    Unit(Name<'s>),
}

/// How many values a slot takes. Written after its range: none means one.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Mult {
    /// Exactly one.
    One,
    /// `optional`: none or one.
    Optional,
    /// `some`: one or more.
    Some,
    /// `many`: any number.
    Many,
}

/// `by share`, `by rent USD`, `by area SQFT`: what the values of a slot that holds several are weighed by.
#[derive(Clone, Copy, Debug)]
pub struct Weight<'s> {
    pub name: Name<'s>,
    /// The commodity a weight is an amount of, if it is one.
    pub unit: Option<Name<'s>>,
}

/// `code GLOB` with indented `on PLACE-GLOB | KIND` lines.
#[derive(Debug)]
pub struct CodeRule<'s> {
    /// The code's name without `^`, or a glob such as `trip-*`.
    pub pattern: Name<'s>,
    /// Every glob and kind of every `on` line: the places the code may name.
    pub on: Many<Name<'s>>,
    /// `known-as "INV-" digit+ "-" digit+`: how it appears in memos.
    pub known_as: Many<Pattern<'s>>,
}

/// `param NAME [UNIT]` with indented `KEY+ VALUE` rows.
#[derive(Debug)]
pub struct Param<'s> {
    /// The parameter's name.
    pub name: Name<'s>,
    /// The unit its values are in, when it says.
    pub unit: Option<Name<'s>>,
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

// ─── Sync ───────────────────────────────────────────────────────────────────

/// `sync NAME` and its indented lines: where a book's records come from
/// (LANGUAGE §14).
#[derive(Debug)]
pub struct Sync<'s> {
    /// What it feeds: an account, or a name of its own.
    pub name: Name<'s>,
    /// `read "imports/chase-*.csv"`: the files to read, a glob.
    pub read: Option<Text<'s>>,
    /// `run quotes {units} --since {since}`: a command whose output is read,
    /// as written.
    pub run: Option<Text<'s>>,
    /// `into prices/{year}.ax` or `into param cpi`, as written.
    pub into: Option<Text<'s>>,
    /// `format csv`, with the lines that declare it here, or none.
    pub format: Option<Ref<Format<'s>>>,
}

/// `format NAME` and its indented lines: the shape of a source's records.
#[derive(Debug)]
pub struct Format<'s> {
    /// `csv`, `ofx`, `camt053`.
    pub name: Name<'s>,
    /// The lines that say what each field is: `&file[format.lines]`. A format
    /// named and not declared, as a sync's `format ofx` is, has none.
    pub lines: Many<FormatLine<'s>>,
}

/// `KEY ARG*` of a format: `date "Posting Date" "MM/DD/YYYY"`, `records STMTTRN`,
/// `amount Amt, sign CdtDbtInd CRDT`. A record's field names are the source's
/// own (`BookgDt/Dt`), so words are kept as written and commas are dropped.
#[derive(Debug)]
pub struct FormatLine<'s> {
    pub key: Name<'s>,
    /// `&file[line.args]`
    pub args: Many<FormatArg<'s>>,
    /// The whole line.
    pub loc: Loc,
}

/// One argument of a [`FormatLine`].
#[derive(Clone, Copy, Debug)]
pub enum FormatArg<'s> {
    /// A word, as written: `flipped`, `DTPOSTED`, `NtryDtls/TxDtls/Ref`.
    Word(Text<'s>),
    /// A string's contents: `"Posting Date"`.
    Quoted(Text<'s>),
}

/// `pattern NAME = PATTERN`: a named pattern that others may use.
#[derive(Debug)]
pub struct NamedPattern<'s> {
    pub name: Name<'s>,
    pub pattern: Pattern<'s>,
}

/// A parsing expression that recognizes memos (§14): `SEQ ( / SEQ )*`, ordered
/// choice of sequences.
#[derive(Clone, Copy, Debug)]
pub struct Pattern<'s> {
    /// `&file[pattern.choices]`
    pub choices: Many<Sequence<'s>>,
}

/// One choice of a pattern: what follows what.
#[derive(Clone, Copy, Debug)]
pub struct Sequence<'s> {
    /// `&file[sequence.terms]`
    pub terms: Many<PatternTerm<'s>>,
}

/// `[NAME:] ATOM [? | * | +]`
#[derive(Clone, Copy, Debug)]
pub struct PatternTerm<'s> {
    /// `payee:rest`: the name that captures what the atom matched.
    pub capture: Option<Name<'s>>,
    pub atom: PatternAtom<'s>,
    pub repeat: Repeat,
}

/// What a [`PatternTerm`] matches.
#[derive(Clone, Copy, Debug)]
pub enum PatternAtom<'s> {
    /// `"TRADER JOE"`, in any case.
    Literal(Text<'s>),
    /// `digit`, `space`, `rest`: a class of characters.
    Class(Class),
    /// Another pattern, by name.
    Named(Name<'s>),
    /// `( PATTERN )`
    Group(Pattern<'s>),
}

/// The classes of characters a pattern names.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Class {
    Digit,
    Letter,
    Space,
    Alnum,
    /// Any one character.
    Any,
    /// Everything up to the end of the memo.
    Rest,
    /// The start of the memo.
    Start,
    /// The end of the memo.
    End,
}

impl Class {
    /// Every class, by the word that names it: the one table of them.
    pub const WORDS: [(&'static str, Class); 8] = [
        ("digit", Class::Digit),
        ("letter", Class::Letter),
        ("space", Class::Space),
        ("alnum", Class::Alnum),
        ("any", Class::Any),
        ("rest", Class::Rest),
        ("start", Class::Start),
        ("end", Class::End),
    ];
}

/// How often a [`PatternTerm`] matches.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Repeat {
    /// Once.
    One,
    /// `?`: once or not at all.
    Optional,
    /// `*`: any number of times.
    Many,
    /// `+`: at least once.
    Some,
}

// ─── Laws ───────────────────────────────────────────────────────────────────

/// `law NAME` with an indented trigger and steps.
#[derive(Debug)]
pub struct Law<'s> {
    /// For a top-level law this is also the [`Item::doc`].
    pub doc: Option<Doc<'s>>,
    /// The law's name, which its diagnostics carry.
    pub name: Name<'s>,
    /// `overrides NAME`: the law this one wins over where both apply.
    pub overrides: Option<Name<'s>>,
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
    /// `on in`: value arrives at the governed thing.
    In,
    /// `on out`: value leaves it.
    Out,
    /// `on gain`: parcels leaving it realize a gain.
    Gain,
    /// `on spend`: money tied to a restricted entity leaves its owner.
    Spend,
    /// `on flow`: a flow of the governed purpose (under an asset, one whose
    /// purpose is `of` it).
    Flow,
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
    /// `always`: after any change to the governed thing.
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
    /// `unless EXPR`: an exception the law itself knows: it does not apply when
    /// this holds.
    Unless(ExprId),
    /// `let NAME = EXPR`: binds a name for the steps below.
    Let(Name<'s>, ExprId),
    /// `require EXPR [else EFFECT]* ["message"]`, or `warn EXPR ["message"]`.
    Require {
        /// What must hold.
        cond: ExprId,
        /// `else EFFECT else EFFECT`: what is owed instead when it does not
        /// hold, and if that is not met, what is owed instead of that
        /// (`require` only): `&file[…]`.
        otherwise: Many<Effect<'s>>,
        /// The string after it, contents only.
        message: Option<Text<'s>>,
        /// `warn`, not `require`: a failure is a warning, not an error.
        warn: bool,
    },
    /// `owe …` or `count …`.
    Effect(Effect<'s>),
}

/// What a law does to the world: an obligation, a tally, or a basis.
#[derive(Debug)]
pub enum Effect<'s> {
    /// `owe EXPR to ENTITY [by EXPR] [as NAME]`: `amount` is owed to the entity
    /// `to`, due on the day `due` says (none: no due day), and called `name` so
    /// a later `for` can settle it.
    Owe { amount: ExprId, to: Name<'s>, due: Option<ExprId>, name: Option<Name<'s>> },
    /// `count EXPR as NAME`: adds `amount` to the tally `name`.
    Count { amount: ExprId, name: Name<'s> },
    /// `consume EXPR`: lowers the basis of the governed asset (or its part)
    /// by `amount`: depreciation, depletion.
    Consume(ExprId),
    /// `carry EXPR to UNIT_EXPR within SPAN`: holds a disallowed loss and
    /// adds it to the basis of the nearest acquisition of the commodity named
    /// by `to` within `within`, before or after (a wash sale). The model checks
    /// its unit type in context; the expression's `Loc` preserves provenance.
    Carry { amount: ExprId, to: ExprId, within: Span },
    /// `derive ITEM | FLOW`: when the law fires, an item of the flow that fired it, or a flow of its own. It is a
    /// step of a law, never a reparation: `require … else` does not parse it.
    Derive(AlsoLine<'s>),
}

// ─── Expressions ────────────────────────────────────────────────────────────

/// Index of an expression in its file's [`Exprs`]: `file.exprs[id]`. (The
/// lifetime is the arena's element type's, and no expression is ever borrowed
/// through it.)
pub type ExprId = Ref<Expr<'static>>;

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
        let part = &self.parts[root.piece()];
        &part[part[root.local()].first.local()..=root.local()]
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
        &self.parts[id.piece()][id.local()]
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
    Amount(Literal<'s>),
    /// `2026-04-15`
    Date(Day),
    /// `2026-03`, as the first day of the month: a selector.
    Month(Day),
    /// `2026` as a calendar-year selector key, distinct from `Num`.
    Year(i32),
    /// `1/3`: numerator and denominator, the latter not zero.
    Fraction(u32, u32),
    /// `30d`, `2w`, `3m`, `1y`
    Span(Span),
    /// A string, contents between the quotes.
    Str(Text<'s>),
    /// `empty`: the zero of every commodity.
    Empty,
    /// Lowercase identifiers, paths and globs: `year`, `self`, `wages`,
    /// `expenses/food/*`, `401k`.
    Name(Name<'s>),
    /// `USD`
    Unit(Name<'s>),
    /// `#groceries`
    Purpose(Name<'s>),
    /// `^inv-12`
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
    /// `A of B`: a purpose and the object it takes (`repair of self`), or a share
    /// and what it is a share of (`12% of ^bldg-water`, `1/3 of ^pge-jan`).
    Of(ExprId, ExprId),
    /// `AMOUNT @ PRICE`: a quantity at a price per unit (`^inv-12[HR] @ 150 USD/HR`).
    At(ExprId, ExprId),
    /// `[retirement]`, with nothing before it: the parts of the flow at hand that
    /// the keys pick.
    Select(Many<ExprId>),
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
    /// `up to`: the smaller of the two (`X up to Y`).
    UpTo,
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
            BinOp::UpTo => "up to",
        }
    }
}
