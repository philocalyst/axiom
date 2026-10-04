//! Properties: the lines written under kinds, accounts, entities and commodities.
//!
//! A line is read once and what it says goes straight into the facts. The language's own (`owner`, `opened`, `via`,
//! `precision`) are read by the table of [`BUILTINS`], each into a typed key of [`builtin`](crate::builtin); a value
//! for a property that a kind declared with `has NAME TYPE` is checked against its slot's range and count by [`fill`],
//! and said as a datum. A line naming neither is an error with a suggestion. A kind's lines are said of the kind and its
//! things have them through the kind chain, so a thing is never given a copy: nothing is built per thing but what takes
//! every line to say ([`Pending`]).
//!
//! Lines are read kind before thing and sort by sort, so that what a line gives a commodity (its precision) is settled
//! before an amount is written against it.

use std::iter;
use std::ops::{Deref, DerefMut};

use axiom_core::tagless::{Datum, Field};
use axiom_core::{Day, Days, Diagnostic, Id, Key, Loc, Many, Map, Ratio, Set, SlotId, Sym};
use axiom_syntax::{
    Change, ClauseKind, Decl, DeclKind, ExprKind, File, Policy, Prop as Line, Rates, Setting, Statement, Subject, Verb,
};

use crate::args::Args;
use crate::book::{Asset, At, Basis, Books, Commodity, Entity, Kind, Place, Purpose, RatePolicy, Sort, System};
use crate::builtin::{self as slot, Coded};
use crate::collect::{Collected, Written};
use crate::declare::{MAX_SCALE, World};
use crate::errors::{Reported, Word, article, list, suggest};
use crate::fill::{self, Filled, Said};
use crate::holders::Holder;
use crate::law::Ty;
use crate::problem;
use crate::scope::Home;
use crate::slots::{Range, Slot, View};
use crate::spelled;

/// What a property line describes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Target {
    Place,
    Entity,
    Commodity,
    Asset,
    Kind,
    Contract,
}

impl Target {
    fn noun(self) -> &'static str {
        match self {
            Target::Place => "account",
            Target::Entity => "entity",
            Target::Commodity => "commodity",
            Target::Asset => "asset",
            Target::Kind => "kind",
            Target::Contract => "contract",
        }
    }

    /// What the instances of a kind of this sort are.
    fn of(sort: Sort) -> Target {
        match sort {
            Sort::Place(_) => Target::Place,
            Sort::Thing => Target::Asset,
            Sort::Entity => Target::Entity,
            Sort::Commodity => Target::Commodity,
            Sort::Contract => Target::Contract,
        }
    }
}

/// How the arguments of one of the language's properties read, and what they say.
type Reader = fn(&mut Reading<'_, '_, '_>) -> Result<(), Diagnostic>;

/// A line whose argument, as `Args::$read` reads it, is what it says of `$key`: through `$then` first, if the slot
/// holds the word as a number.
macro_rules! says {
    ($key:expr, $read:ident) => {
        |a| {
            let value = a.$read()?;
            a.say($key, value)
        }
    };
    ($key:expr, $read:ident, $then:ident) => {
        |a| {
            let value = a.$read()?;
            a.say($key, value.$then())
        }
    };
}

/// A line whose arguments, as `Args::$read` reads them, are the set it says of `$key`.
macro_rules! says_all {
    ($key:expr, $read:ident) => {
        |a| {
            let members = a.$read()?;
            a.say_set($key, members)
        }
    };
}

/// The properties the language defines itself, what each may be written
/// under, and how it reads.
const BUILTINS: [(&str, &[Target], Reader); 25] = [
    ("holds", &[Target::Place], says_all!(slot::HOLDS, holds)),
    ("owner", &[Target::Kind], |a| {
        let kinds = a.owner_kinds()?;
        let Holder::Kind(kind) = a.thing else { unreachable!("`owner` is a built-in only under kinds") };
        a.world.book.kinds[kind].owners = kinds.into();
        Ok(())
    }),
    ("select", &[Target::Place, Target::Commodity, Target::Asset], says!(slot::SELECT, policy, code)),
    ("opened", &[Target::Place], says!(slot::OPENED, day)),
    ("closed", &[Target::Place], says!(slot::CLOSED, day)),
    ("liquidity", &[Target::Place, Target::Commodity, Target::Asset], says!(slot::LIQUIDITY, span)),
    ("via", &[Target::Entity], says!(slot::VIA, place)),
    ("lives", &[Target::Entity], |a| {
        let residence = a.residence()?;
        a.done()?;
        a.pending.lives.entry(a.thing).or_default().push(residence);
        Ok(())
    }),
    ("member", &[Target::Entity], says!(slot::MEMBER, entity)),
    ("currency", &[Target::Entity], says!(slot::CURRENCY, currency)),
    ("citizen", &[Target::Entity], says_all!(slot::CITIZEN, citizens)),
    ("books", &[Target::Entity], says!(slot::BOOKS, books, code)),
    ("purpose", &[Target::Kind, Target::Entity], |a| {
        let (purpose, loc) = a.purpose()?;
        a.say_at(slot::PURPOSE, purpose, loc)
    }),
    ("pays", &[Target::Kind], |a| {
        let (purpose, loc) = a.purpose()?;
        a.say_at(slot::PAYS, purpose, loc)
    }),
    ("takes", &[Target::Kind], |a| {
        let (to, _) = a.purpose()?;
        a.word(&["from"])?;
        let (from, _) = a.purpose()?;
        a.done()?;
        let take = (from, to, a.line.loc);
        let takes = a.pending.takes.entry(a.thing).or_default();
        // A later line for the same purpose replaces the earlier.
        match takes.iter_mut().find(|held| held.0 == from) {
            Some(held) => *held = take,
            None => takes.push(take),
        }
        Ok(())
    }),
    ("sales-tax", &[Target::Kind], says!(slot::SALES_TAX, percent)),
    ("share", &[Target::Kind], |a| {
        let entity = a.share()?;
        a.done()?;
        a.pending.shares.entry(a.thing).or_default().push(entity);
        Ok(())
    }),
    ("part", &[Target::Asset], |a| {
        let part = a.part_of()?;
        a.done()?;
        let Holder::Asset(asset) = a.thing else { unreachable!("`part` is written under assets") };
        a.world.book.assets[asset].part_of = Some(part);
        Ok(())
    }),
    ("precision", &[Target::Commodity], |a| {
        let scale = a.count(MAX_SCALE)?;
        a.done()?;
        a.pending.precision.insert(a.thing, scale);
        Ok(())
    }),
    ("name", &[Target::Commodity], |a| {
        let title = a.text()?;
        let title = a.world.book.names.intern(title);
        a.say(slot::TITLE, title)
    }),
    ("grows", &[Target::Commodity], |a| {
        let rate = a.percent()?;
        a.word(&["yearly"])?;
        a.say(slot::GROWS, rate)
    }),
    ("restricted", &[Target::Kind], |a| a.say(slot::RESTRICTED, true)),
    ("deferred", &[Target::Kind], |a| a.say(slot::DEFERRED, true)),
    ("basis", &[Target::Kind], says!(slot::BASIS, basis, code)),
    ("claim", &[Target::Kind], |a| a.say(slot::CLAIM, true)),
];

/// What a `takes` line says: what is taken, what it becomes, and where it is written.
type Take = (Id<Purpose>, Id<Purpose>, Loc);

/// What the lines of the language say that is not said as they are read, because it takes all of them: the
/// residences of an entity are its kinds' and its own, whole; a kind's `takes` replace one another by what they take;
/// and a commodity's precision is its own, else its kinds'.
#[derive(Default)]
struct Pending {
    lives: Map<Holder, Vec<Residence>>,
    takes: Map<Holder, Vec<Take>>,
    shares: Map<Holder, Vec<Id<Entity>>>,
    precision: Map<Holder, u8>,
}

/// `lives us/ca from 2026-01-01 until 2026-06-30`: inclusive, and open-ended on either side when unwritten.
#[derive(Clone, Copy, Debug)]
struct Residence {
    days: Days,
    system: Id<System>,
}

impl Pending {
    /// Says it all, now that every line is read.
    fn say(self, world: &mut World<'_>) {
        self.settle_precision(world);
        self.settle_lives(world);
        for (holder, takes) in &self.takes {
            let pairs = takes.iter().map(|&(from, to, _)| (from.index() as u32, to.index() as u32));
            world.say_set(*holder, slot::TAKES, pairs);
            for &(from, _, loc) in takes {
                world.say_site(*holder, slot::TAKES.slot(), from.index() as u32, loc);
            }
        }
        for (holder, entities) in &self.shares {
            world.say_set(*holder, slot::SHARE, entities.iter().copied());
        }
    }

    fn settle_precision(&self, world: &mut World<'_>) {
        for id in world.book.commodities.ids().collect::<Vec<_>>() {
            let kinds = world.book.kinds.lineage(world.book.commodities[id].kind).map(Holder::Kind);
            if let Some(&scale) =
                iter::once(Holder::Commodity(id)).chain(kinds).find_map(|holder| self.precision.get(&holder))
            {
                world.book.commodities[id].scale = scale;
            }
        }
    }

    fn settle_lives(&self, world: &mut World<'_>) {
        for id in world.book.entities.ids().collect::<Vec<_>>() {
            let mut kinds: Vec<_> = world.book.kinds.lineage(world.book.entities[id].kind).collect();
            kinds.reverse();
            let above = kinds.into_iter().map(Holder::Kind);
            let residences: Vec<Residence> = above
                .chain([Holder::Entity(id)])
                .flat_map(|holder| self.lives.get(&holder).into_iter().flatten().copied())
                .collect();
            paint_lives(world, id, &residences);
        }
    }
}

/// Says where an entity lives, day by day: the systems of every residence that holds on a day, and nothing where none
/// does. Residences overlap and the facts hold sets, so the days are cut where one begins or ends.
fn paint_lives(world: &mut World<'_>, entity: Id<Entity>, residences: &[Residence]) {
    let mut cuts: Vec<Day> = residences.iter().map(|residence| residence.days.first()).collect();
    let ends = residences.iter().map(|residence| residence.days.last()).filter(|&last| last < Day::MAX);
    cuts.extend(ends.map(|last| last.add_days(1)));
    cuts.sort_unstable();
    cuts.dedup();
    let stops = cuts.iter().skip(1).map(|&next| next.add_days(-1)).chain([Day::MAX]);
    for (&from, to) in cuts.iter().zip(stops) {
        let living = residences.iter().filter(|residence| residence.days.contains(from));
        let systems: Vec<_> = living.map(|residence| residence.system).collect();
        if let (false, Some(days)) = (systems.is_empty(), Days::new(from, to)) {
            world.say_set_over(entity, slot::LIVES, days, systems);
        }
    }
}

// ─── Reading ────────────────────────────────────────────────────────────────

/// A built-in line being read: its values, and the thing it is written under.
struct Reading<'w, 'a, 's> {
    args: Args<'a, 's>,
    world: &'w mut World<'s>,
    pending: &'w mut Pending,
    home: Home,
    /// The thing the line is written under.
    thing: Holder,
    /// What it says things of: the thing, or its place, for an asset's settings.
    said_of: Holder,
}

impl<'a, 's> Deref for Reading<'_, 'a, 's> {
    type Target = Args<'a, 's>;
    fn deref(&self) -> &Args<'a, 's> {
        &self.args
    }
}

impl DerefMut for Reading<'_, '_, '_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.args
    }
}

impl<'s> Reading<'_, '_, 's> {
    /// The line says `value` of what it is written under.
    fn say<V: Field>(&mut self, key: Key<V>, value: V) -> Result<(), Diagnostic> {
        self.done()?;
        self.world.say(self.said_of, key, value);
        Ok(())
    }

    /// The line says `value`, and where it is written, which a diagnostic may point back to.
    fn say_at<V: Field>(&mut self, key: Key<V>, value: V, loc: Loc) -> Result<(), Diagnostic> {
        self.say(key, value)?;
        self.world.say_site(self.said_of, key.slot(), 0, loc);
        Ok(())
    }

    /// The line says the whole set of `members`.
    fn say_set<V: Field>(&mut self, key: Key<Many<V>>, members: impl IntoIterator<Item = V>) -> Result<(), Diagnostic> {
        self.done()?;
        self.world.say_set(self.said_of, key, members);
        Ok(())
    }

    fn entity(&mut self) -> Result<Id<Entity>, Diagnostic> {
        self.args.entity(self.world, self.home)
    }

    fn place(&mut self) -> Result<Id<Place>, Diagnostic> {
        self.args.place(self.world)
    }

    /// `person, household`: the kinds of entity that may own what a kind of account classifies.
    fn owner_kinds(&mut self) -> Result<Vec<Id<Kind>>, Diagnostic> {
        let mut kinds = Vec::new();
        loop {
            let word = self.name("a kind of entity")?;
            let kind = self.world.kind(self.home, word)?;
            if self.world.book.kinds[kind].sort != Sort::Entity {
                let error = Diagnostic::error("property-type", "`owner` needs a kind of entity");
                return Err(error.label(word.loc, "this is not a kind of entity"));
            }
            kinds.push(kind);
            if self.peek().is_none() {
                return Ok(kinds);
            }
        }
    }

    fn currency(&mut self) -> Result<Id<Commodity>, Diagnostic> {
        self.args.unit(self.world, "a commodity such as `USD`")
    }

    fn citizens(&mut self) -> Result<Vec<Id<System>>, Diagnostic> {
        let mut systems = Vec::new();
        while self.peek().is_some() {
            systems.push(self.args.system(self.world)?);
        }
        if systems.is_empty() {
            Err(Diagnostic::error("property-argument", "`citizen` needs a system")
                .label(self.line.loc, "name a system here"))
        } else {
            Ok(systems)
        }
    }

    fn basis(&mut self) -> Result<Basis, Diagnostic> {
        Ok(if self.word(&["zero", "cost"])? == "zero" { Basis::Zero } else { Basis::Cost })
    }

    fn books(&mut self) -> Result<Books, Diagnostic> {
        Ok(match self.word(&["cash", "accrual"])? {
            "cash" => Books::Cash,
            _ => Books::Accrual,
        })
    }

    /// A purpose, and where it is written.
    fn purpose(&mut self) -> Result<(Id<Purpose>, Loc), Diagnostic> {
        self.args.purpose(self.world, self.home)
    }

    /// `60% for studio`, whose rate is checked and whose entity is the share's.
    fn share(&mut self) -> Result<Id<Entity>, Diagnostic> {
        let expr = &self.file.exprs[self.next_id("a percentage or fraction")?];
        let rate = match expr.kind {
            ExprKind::Pct(number) => Ratio::percent(number.mantissa.into(), number.scale),
            ExprKind::Fraction(top, bottom) => Ratio::new(i128::from(top), i128::from(bottom)),
            _ => None,
        }
        .ok_or_else(|| {
            Diagnostic::error("share-rate", "a share must be a percentage or fraction")
                .label(expr.loc, "write `60%` or `3/5`")
        })?;
        validate_share_rate(rate, expr.loc)?;
        self.word(&["for"])?;
        self.entity()
    }

    fn part_of(&mut self) -> Result<At<Id<Asset>>, Diagnostic> {
        self.word(&["of"])?;
        let (asset, loc) = self.args.asset(self.world)?;
        Ok(At { value: asset, loc })
    }

    fn policy(&mut self) -> Result<Policy, Diagnostic> {
        let words: Vec<&str> = Policy::WORDS.iter().map(|policy| policy.0).collect();
        let word = self.word(&words)?;
        Ok(Policy::WORDS.iter().find(|policy| policy.0 == word).map_or(Policy::Fifo, |policy| policy.1))
    }

    /// `holds USD, VTI`, or `holds any`.
    fn holds(&mut self) -> Result<Vec<Id<Commodity>>, Diagnostic> {
        if self.takes("any") {
            return Ok(Vec::new());
        }
        let mut units = Vec::new();
        while self.peek().is_some() {
            units.push(self.args.unit(self.world, "commodities such as `USD`, or `any`")?);
        }
        match units.is_empty() {
            true => Err(Diagnostic::error("property-argument", "`holds` needs commodities or `any`")
                .label(self.line.loc, "name what it holds")),
            false => Ok(units),
        }
    }

    /// `lives us/ca`, or `lives us/ca from 2026-01-01 until 2026-06-30`.
    fn residence(&mut self) -> Result<Residence, Diagnostic> {
        let system = self.args.system(self.world)?;
        let (mut from, mut until) = (Day::MIN, Day::MAX);
        while self.peek().is_some() {
            match self.word(&["from", "until"])? {
                "from" => from = self.day()?,
                _ => until = self.day()?,
            }
        }
        let Some(days) = Days::new(from, until) else {
            return Err(Diagnostic::error("residence-order", "this residence ends before it begins")
                .label(self.line.loc, "`until` is earlier than `from`")
                .help("swap the two dates"));
        };
        Ok(Residence { days, system })
    }
}

fn validate_share_rate(rate: Ratio, loc: Loc) -> Result<(), Diagnostic> {
    if rate.is_negative() || rate.num() > rate.den() {
        return Err(Diagnostic::error("share-rate-range", "a share must be between 0% and 100%")
            .label(loc, "this share is outside the allowed range"));
    }
    Ok(())
}

/// The lines under one declaration, and where they were written.
struct Lines<'a, 's> {
    home: Home,
    file: &'a File<'s>,
    lines: &'a [Line<'s>],
}

impl<'a, 's> Lines<'a, 's> {
    fn from_native(written: Written<'a, 's, Decl<'s>>) -> Lines<'a, 's> {
        Lines { home: written.home(), file: written.file(), lines: &written.file()[written.node.props] }
    }
}

/// Reads a line: one of the language's for something it is written under (`targets`), and says what it says.
fn read_line<'s>(
    world: &mut World<'s>,
    pending: &mut Pending,
    at: &Lines<'_, 's>,
    line: &Line<'s>,
    under: NativeTarget,
    targets: &[Target],
) -> Result<(), Diagnostic> {
    let word = line.name.0;
    let Some(builtin) = BUILTINS.iter().find(|entry| entry.0 == word && targets.iter().any(|t| entry.1.contains(t)))
    else {
        return Err(unknown_property(world, at.file.loc(word), targets[0], under.kind, word));
    };
    let args = Args::of(at.file, line);
    let mut reading = Reading { args, world, pending, home: at.home, thing: under.holder, said_of: under.said_of };
    (builtin.2)(&mut reading)?;
    reading.done()
}

/// `benificiary` is not a property of an account of kind `529`.
fn unknown_property(world: &World, loc: Loc, target: Target, kind: Id<Kind>, word: &str) -> Diagnostic {
    let kind_name = world.book.name(world.book.kinds[kind].name);
    let mut valid: Vec<&str> = BUILTINS.iter().filter(|entry| entry.1.contains(&target)).map(|entry| entry.0).collect();
    valid.extend(declared_names(world, kind));
    let of = match target {
        Target::Kind => format!("kind `{kind_name}`"),
        target => format!("{} of kind `{kind_name}`", article(target.noun())),
    };
    let error = Diagnostic::error("unknown-property", format!("`{word}` is not a property of {of}"))
        .label(loc, "no such property");
    let error = suggest(error, loc, word, valid.iter().copied());
    match BUILTINS.iter().find(|entry| entry.0 == word) {
        Some(entry) => {
            let owners: Vec<String> = entry.1.iter().map(|owner| format!("{}s", owner.noun())).collect();
            error.note(format!("`{word}` describes {}, not {}", owners.join(" and "), article(target.noun())))
        }
        None => error,
    }
    .note(format!("its properties are {}", list(&valid)))
}

// ─── Native S5 declarations ────────────────────────────────────────────────

#[derive(Clone, Copy)]
struct NativeTarget {
    holder: Holder,
    /// What the settings written under it are said of: itself, but for an asset, whose place they are the settings of.
    said_of: Holder,
    kind: Id<Kind>,
    sort: Sort,
}

/// What a `now PROPERTY VALUE` statement says of a slot, from its day until the day it says, if it says.
struct PropertyChange {
    holder: Holder,
    slot: SlotId,
    days: Days,
    said: Said,
    order: usize,
}

type Seen = Map<(Holder, Sym, Day), Loc>;

/// Resolves the properties written under things, and what each says of the slot it fills, into the facts. A kind's
/// lines are its things' defaults: they are said of the kind, and the facts look up the kind chain when a thing says
/// nothing.
pub(crate) fn declare<'a, 's>(world: &mut World<'s>, collected: &Collected<'a, 's>, diags: &mut Vec<Diagnostic>) {
    native_builtins(world, collected, diags);
    let mut seen = Seen::default();
    let mut filled = Filled::default();
    stage_declared_values(world, collected, &mut seen, &mut filled, diags);
    spelled::place_words(world, collected, &mut filled, diags);
    let changes = property_changes(world, collected, &mut seen, diags);
    paint_changes(world, changes);
    missing_roles(world, collected, &filled, diags);
}

/// The values the property lines of declarations give, from the beginning of time. A slot of several is said once,
/// whole, however many lines fill it.
fn stage_declared_values<'a, 's>(
    world: &mut World<'s>,
    collected: &Collected<'a, 's>,
    seen: &mut Seen,
    filled: &mut Filled,
    diags: &mut Vec<Diagnostic>,
) {
    let mut sets: Map<(Holder, SlotId), Vec<Datum>> = Map::default();
    for &written in &collected.decls {
        if written.node.what == DeclKind::Purpose {
            continue;
        }
        let Some(target) = native_target(world, written) else {
            continue;
        };
        for line in &written.file()[written.node.props] {
            if is_builtin_line(line.name.0) {
                continue;
            }
            let Some(has) = has_named(world, target.kind, line.name.0) else {
                diags.push(unknown_native_property(world, target, line));
                // A near miss of a slot's name is an attempt to fill it: the typo is said, and the slot not again.
                filled.extend(near_slot(world, target.kind, line.name.0).map(|number| (target.holder, number)));
                continue;
            };
            // A line that is wrong is still an attempt to fill the slot: it is said, and the slot not again.
            filled.insert((target.holder, has.number));
            let at = fill::At { home: written.home(), file: written.file(), line };
            let Some(filling) = fill::fill(world, &at, &has.slot).or_report(diags) else {
                continue;
            };
            match filling.said(has.slot.mult) {
                Said::One(datum) => {
                    if note_once(world, seen, target.holder, has.name, Day::MIN, line.loc, diags) {
                        world.paint(target.holder, has.number, Days::ALWAYS, datum);
                    }
                }
                Said::Set(members) => sets.entry((target.holder, has.number)).or_default().extend(members),
            }
        }
    }
    for ((holder, slot), members) in sets {
        world.paint_set(holder, slot, Days::ALWAYS, members);
    }
}

/// Whether a value is the first said of the slot on the day, and said, if not, to be the second.
fn note_once(
    world: &World<'_>,
    seen: &mut Seen,
    holder: Holder,
    name: Sym,
    day: Day,
    loc: Loc,
    diags: &mut Vec<Diagnostic>,
) -> bool {
    let Some(first) = seen.get(&(holder, name, day)).copied() else {
        seen.insert((holder, name, day), loc);
        return true;
    };
    diags.push(problem::filled_twice(world.book.name(name), loc, first));
    false
}

/// The changes `now PROPERTY VALUE` statements make, in the order written.
fn property_changes<'a, 's>(
    world: &mut World<'s>,
    collected: &Collected<'a, 's>,
    seen: &mut Seen,
    diags: &mut Vec<Diagnostic>,
) -> Vec<PropertyChange> {
    let mut changes = Vec::new();
    for written in &collected.statements {
        let Verb::Now(Change::Property(line)) = &written.node.verb else {
            continue;
        };
        if is_builtin_line(line.name.0) || says_a_loans_rate(collected, written.node.subject, line) {
            continue;
        }
        if let Some(change) = property_change(world, written, line, changes.len(), seen, diags) {
            changes.push(change);
        }
    }
    changes
}

/// Whether the statement is `DATE CONTRACT now at 6.25%`: a loan's rate, which the record lowering reads. A contract may be
/// named as a kind is (`mortgage`), and the statement is about the contract then.
fn says_a_loans_rate(collected: &Collected<'_, '_>, subject: Subject<'_>, line: &Line<'_>) -> bool {
    let Subject::Name(name) = subject else { return false };
    line.name.0 == "at" && collected.contracts.iter().any(|contract| contract.node.name.0 == name.0)
}

/// One statement's change of a property a kind declared, or nothing after what is wrong with it is said.
fn property_change<'a, 's>(
    world: &mut World<'s>,
    written: &Written<'a, 's, Statement<'s>>,
    line: &Line<'s>,
    order: usize,
    seen: &mut Seen,
    diags: &mut Vec<Diagnostic>,
) -> Option<PropertyChange> {
    let (file, statement) = (written.file(), written.node);
    let property_name = world.book.names.intern(line.name.0);
    // The statement pass owns unresolved subjects. This pass only consumes a custom property after its target
    // kind is known.
    let target =
        native_statement_target(world, written.home(), statement.subject, property_name, written.item.loc, diags)?;
    let Some(has) = has_named(world, target.kind, line.name.0) else {
        diags.push(unknown_native_property(world, target, line));
        return None;
    };
    let at = fill::At { home: written.home(), file, line };
    let filling = fill::fill(world, &at, &has.slot).or_report(diags)?;
    let until = file[statement.tail].iter().find_map(|clause| match clause.kind {
        ClauseKind::Until(day) => Some(day),
        _ => None,
    });
    let Some(days) = Days::new(statement.date, until.unwrap_or(Day::MAX)) else {
        diags.push(
            Diagnostic::error("property-until-order", "this property change ends before it begins")
                .label(line.loc, "`until` is earlier than the change")
                .help("move the end date to the change date or later"),
        );
        return None;
    };
    let key = (target.holder, has.name, statement.date);
    if let Some(first) = seen.get(&key).copied() {
        diags.push(problem::twice("property change", line.loc, first));
        return None;
    }
    seen.insert(key, line.loc);
    let said = filling.said(has.slot.mult);
    Some(PropertyChange { holder: target.holder, slot: has.number, days, said, order })
}

/// Paints the changes over the declarations: a change overrides what it covers from its day, and what held before
/// resumes when it ends. Day by day, the one begun last is in force, which is the order of painting.
fn paint_changes(world: &mut World<'_>, mut changes: Vec<PropertyChange>) {
    changes.sort_by_key(|change| (change.days.first(), change.order));
    for change in changes {
        match change.said {
            Said::One(datum) => world.paint(change.holder, change.slot, change.days, datum),
            Said::Set(members) => world.paint_set(change.holder, change.slot, change.days, members),
        }
    }
}

/// The built-in properties of everything declared: what a kind says it says of its things, and a thing says its own.
/// Only the first declaration of a thing says them. Kinds are read before the things of their sort, and the sorts in
/// the order their lines need.
fn native_builtins<'a, 's>(world: &mut World<'s>, collected: &Collected<'a, 's>, diags: &mut Vec<Diagnostic>) {
    let mut first = Set::default();
    let mut work: Vec<_> = collected
        .decls
        .iter()
        .filter_map(|&written| native_target(world, written).map(|under| (under, written)))
        .filter(|(under, _)| first.insert(under.holder))
        .collect();
    let rank = |under: &NativeTarget| match Target::of(under.sort) {
        Target::Commodity => 0,
        Target::Entity => 1,
        Target::Place => 2,
        _ => 3,
    };
    work.sort_by_key(|(under, _)| {
        (rank(under), !matches!(under.holder, Holder::Kind(_)), world.book.holders.number(under.holder))
    });
    let mut pending = Pending::default();
    for (under, written) in work {
        read_builtin_lines(world, &mut pending, written, under, diags);
    }
    diagnose_asset_cycles(&mut world.book.assets, &world.book.names, diags);
    native_system_currencies(world, collected, diags);
    pending.say(world);
}

/// The lines a declaration writes for the built-in properties of what it declares, read and said.
fn read_builtin_lines<'a, 's>(
    world: &mut World<'s>,
    pending: &mut Pending,
    written: Written<'a, 's, Decl<'s>>,
    under: NativeTarget,
    diags: &mut Vec<Diagnostic>,
) {
    let at = Lines::from_native(written);
    let own = Target::of(under.sort);
    let targets: &[Target] = if matches!(under.holder, Holder::Kind(_)) { &[Target::Kind, own] } else { &[own] };
    for line in at.lines {
        if line.name.0 == "owner" && !matches!((under.holder, under.sort), (Holder::Kind(_), Sort::Place(_))) {
            owner_line(world, under, line, diags);
            continue;
        }
        if !BUILTINS.iter().any(|(name, _, _)| *name == line.name.0) {
            continue;
        }
        if targets.contains(&Target::Kind) && targets.contains(&Target::Asset) && line.name.0 == "part" {
            diags.push(
                Diagnostic::error("kind-property-target", "`part` is specific to an asset")
                    .label(line.loc, "write this under an `asset`, not its `kind`"),
            );
            continue;
        }
        if line.name.0 == "purpose" {
            diags.extend(purpose_of_its_own(world, &at, under, line));
        }
        if let Err(problem) = read_line(world, pending, &at, line, under, targets) {
            diags.push(problem);
        }
    }
}

/// `purpose education` under a grant: `purpose` says what the flows with a party are for. A party whose kind has a
/// purpose of its own to say (`grant-purpose`, what a grant's money may be spent on) and leaves it unsaid is told so.
fn purpose_of_its_own(world: &World<'_>, at: &Lines<'_, '_>, under: NativeTarget, line: &Line<'_>) -> Option<Diagnostic> {
    let book = &world.book;
    let Holder::Entity(entity) = under.holder else { return None };
    let own = |slot: &&Slot| slot.range == Range::Value(Ty::Purpose) && book.name(slot.name) != "purpose";
    let name = book.name(book.schema.effective(&book.kinds, under.kind).find(own)?.name);
    if at.lines.iter().any(|other| other.name.0 == name) {
        return None;
    }
    let (party, kind) = (book.name(book.entities[entity].path), book.name(book.kinds[under.kind].name));
    let value = match at.file[line.args].first().map(|&arg| &at.file.exprs[arg].kind) {
        Some(ExprKind::Name(written)) => written.0,
        _ => "PURPOSE",
    };
    let warning = Diagnostic::warning(
        "purpose-of-its-own",
        format!("`purpose` is what the flows with `{party}` are for, not the {kind}'s own `{name}`"),
    );
    let at_name = at.file.loc(line.name.0);
    Some(warning.label(at_name, "the purpose of the flows with this party").help(format!(
        "for the {kind}'s own, write `{name} {value}`"
    )))
}

/// An `owner` line under anything but a kind of account: an entity, an account or an asset has its owners read where it is
/// declared, and an account's are held to the kinds its kind says may own it. It is no property of the rest.
fn owner_line(world: &World<'_>, under: NativeTarget, line: &Line<'_>, diags: &mut Vec<Diagnostic>) {
    match under.holder {
        Holder::Place(place) => diags.extend(owners_that_may_not(world, place, line)),
        Holder::Entity(_) | Holder::Asset(_) => {}
        _ => diags.push(
            Diagnostic::error("unknown-property", "`owner` is not a property of this kind")
                .label(line.loc, "owner is set on an entity, account, or asset"),
        ),
    }
}

/// `wrong-kind` for each owner of the account that its kind does not say may own it.
fn owners_that_may_not(world: &World<'_>, place: Id<Place>, line: &Line<'_>) -> Vec<Diagnostic> {
    let book = &world.book;
    let Place { kind, owner, shares, .. } = &book.places[place];
    let range = book.owners_of(*kind);
    if range.is_empty() {
        return Vec::new();
    }
    let owners: Vec<_> = match shares.is_empty() {
        true => vec![(*owner, line.loc)],
        false => shares.iter().map(|share| (share.entity, share.loc)).collect(),
    };
    let (takes, fitting) = (View::Kinds(range).describe(book), fill::fitting(world, range));
    let found = |entity: Id<Entity>| article(book.name(book.kinds[book.entities[entity].kind].name));
    let misfits = owners.into_iter().filter(|&(entity, _)| !book.may_own(*kind, entity));
    let said = |(entity, loc)| {
        let word = Word { text: book.name(book.entities[entity].path), loc };
        problem::wrong_kind("owner", word, &found(entity), &takes, &fitting)
    };
    misfits.map(said).collect()
}

/// Asset `part of` edges are followed by the engine when it walks an asset's
/// ancestry. Diagnose and cut a cycle here so that traversal stays bounded.
fn diagnose_asset_cycles(
    assets: &mut axiom_core::Arena<Asset>,
    names: &axiom_core::Interner<'_>,
    diags: &mut Vec<Diagnostic>,
) {
    let mut state = vec![0u8; assets.len()];
    let ids: Vec<_> = assets.ids().collect();
    for start in ids {
        let mut path = Vec::new();
        let mut current = Some(start);
        while let Some(id) = current {
            match state[id.index()] {
                0 => {
                    state[id.index()] = 1;
                    path.push(id);
                    current = assets[id].part_of.map(|parent| parent.value);
                }
                1 => {
                    let from = path.iter().position(|&member| member == id).unwrap_or(0);
                    let members = &path[from..];
                    let route = members
                        .iter()
                        .chain(members.first())
                        .map(|member| names.name(assets[*member].name))
                        .collect::<Vec<_>>();
                    let edge = *path.last().expect("a cycle has a preceding edge");
                    let part = assets[edge].part_of.take();
                    let mut diagnostic =
                        Diagnostic::error("asset-part-cycle", format!("asset `{}` is part of itself", route[0]))
                            .note(format!("the chain is {}", route.join(" -> ")))
                            .help("make one asset a whole, outside this part-of chain");
                    if let Some(part) = part {
                        diagnostic = diagnostic.label(part.loc, "this part-of relationship closes the cycle");
                    }
                    diags.push(diagnostic);
                    break;
                }
                _ => break,
            }
        }
        for id in path {
            state[id.index()] = 2;
        }
    }
}

fn native_system_currencies<'s>(world: &mut World<'s>, collected: &Collected<'_, 's>, diags: &mut Vec<Diagnostic>) {
    let mut seen: Map<Id<System>, Loc> = Map::default();
    for written in &collected.settings {
        let (Home::System(system), Setting::Currency(unit)) = (written.home(), *written.node) else {
            continue;
        };
        let at = written.item.loc;
        if let Some(first) = seen.insert(system, at) {
            diags.push(problem::twice("currency", at, first));
            continue;
        }
        match world.commodity_of(Word::of(written.file(), unit.0)) {
            Ok(currency) => world.book.systems[system].currency = Some(currency),
            Err(problem) => diags.push(problem),
        }
    }
}

/// Gives each entity the place `via` names, once the facts that say it are frozen: an entity's place is where a
/// flow with it ends, and its name gives one unless a line says another.
pub(crate) fn place_entities(world: &mut World<'_>) {
    for id in world.book.entities.ids().collect::<Vec<_>>() {
        if let Some(place) = world.book.fact(slot::VIA, id) {
            world.book.entities[id].place = Some(place);
        }
    }
}

/// Completes each system's exchange-rate policy after params have been
/// declared, so `rates param NAME` resolves with the system's visibility.
pub(crate) fn system_rates<'s>(world: &mut World<'s>, collected: &Collected<'_, 's>, diags: &mut Vec<Diagnostic>) {
    let mut seen: Map<Id<System>, Loc> = Map::default();
    for written in &collected.settings {
        let (Home::System(system), Setting::Rates(policy)) = (written.home(), *written.node) else {
            continue;
        };
        let at = written.item.loc;
        if let Some(first) = seen.insert(system, at) {
            diags.push(problem::twice("rate policy", at, first));
            continue;
        }
        let policy = match policy {
            Rates::Spot => Some(RatePolicy::Spot),
            Rates::Param(name) => {
                let word = Word::of(written.file(), name.0);
                match world.seek_param(written.home(), word) {
                    Ok(Some(param)) => Some(RatePolicy::Param(param)),
                    Ok(None) => {
                        diags.push(world.missing_param(written.home(), word));
                        None
                    }
                    Err(problem) => {
                        diags.push(problem);
                        None
                    }
                }
            }
        };
        world.book.systems[system].rates = policy;
    }
}

impl NativeTarget {
    fn of(world: &World<'_>, holder: Holder) -> NativeTarget {
        let kind = match holder {
            Holder::Place(id) => world.book.places[id].kind,
            Holder::Entity(id) => world.book.entities[id].kind,
            Holder::Commodity(id) => world.book.commodities[id].kind,
            Holder::Asset(id) => world.book.assets[id].kind,
            Holder::Kind(id) => id,
        };
        let said_of = match holder {
            Holder::Asset(id) => Holder::Place(world.book.assets[id].place),
            other => other,
        };
        NativeTarget { holder, said_of, kind, sort: world.book.kinds[kind].sort }
    }
}

fn native_target(world: &World<'_>, written: Written<'_, '_, Decl<'_>>) -> Option<NativeTarget> {
    let word = Word { text: written.node.name.0, loc: written.item.loc };
    let holder = match written.node.what {
        DeclKind::Account => Holder::Place(world.place(word).ok()?),
        DeclKind::Entity => Holder::Entity(world.entity(written.home(), word).ok()?),
        DeclKind::Commodity => Holder::Commodity(world.commodity_of(word).ok()?),
        DeclKind::Asset => Holder::Asset(world.book.asset(word.text)?),
        DeclKind::Kind => Holder::Kind(world.kind(written.home(), word).ok()?),
        DeclKind::Purpose => return None,
    };
    Some(NativeTarget::of(world, holder))
}

/// What a property statement is about: the one thing with a property of that name among those its subject may
/// name, or the only thing it may name. None, said, when two have such a property.
fn native_statement_target(
    world: &World<'_>,
    home: Home,
    subject: Subject<'_>,
    name: Sym,
    loc: Loc,
    diags: &mut Vec<Diagnostic>,
) -> Option<NativeTarget> {
    let candidates = subject_candidates(world, home, subject, loc);
    let property = world.book.name(name);
    let mut having = candidates.iter().copied().filter(|target| has_named(world, target.kind, property).is_some());
    if let Some(first) = having.next() {
        if having.next().is_some() {
            diags.push(
                Diagnostic::error("ambiguous-property-target", "this property applies to more than one named thing")
                    .label(loc, "qualify the target so the intended thing is clear"),
            );
            return None;
        }
        return Some(first);
    }
    (candidates.len() == 1).then(|| candidates[0])
}

/// Everything a statement's subject may name: a commodity, or, by one name, places, parties, an asset and a kind.
fn subject_candidates(world: &World<'_>, home: Home, subject: Subject<'_>, loc: Loc) -> Vec<NativeTarget> {
    let mut targets = Vec::new();
    match subject {
        Subject::Unit(unit) => {
            targets.extend(world.commodity_of(Word { text: unit.0, loc }).ok().map(Holder::Commodity));
        }
        Subject::Name(subject) => {
            let names = &world.book.names;
            targets.extend(
                world.book.lookup.places.find(names, subject.0, |_| true).into_ids().into_iter().map(Holder::Place),
            );
            let entities = world.book.lookup.entities.find(names, world.scopes.of(home), subject.0);
            targets.extend(entities.into_ids().into_iter().map(Holder::Entity));
            targets.extend(world.book.asset(subject.0).map(Holder::Asset));
            targets.extend(world.kind(home, Word { text: subject.0, loc }).ok().map(Holder::Kind));
        }
        Subject::Code(_) | Subject::Purpose(_) => {}
    }
    targets.into_iter().map(|target| NativeTarget::of(world, target)).collect()
}

/// The slot a property line fills, as the thing's kind declares it.
#[derive(Clone, Copy)]
struct Named {
    name: Sym,
    number: SlotId,
    slot: Slot,
}

fn has_named(world: &World<'_>, kind: Id<Kind>, name: &str) -> Option<Named> {
    let (schema, name) = (&world.book.schema, world.book.names.get(name)?);
    let slot = *schema.find(&world.book.kinds, kind, name)?;
    Some(Named { name, number: schema.number(name)?, slot })
}

/// The slot of things of `kind` whose name `word` is a near miss of.
fn near_slot(world: &World<'_>, kind: Id<Kind>, word: &str) -> Option<SlotId> {
    let near = axiom_core::diag::closest(word, declared_names(world, kind))?;
    world.book.schema.number(world.book.names.get(near)?)
}

/// The names of the slots things of `kind` have.
fn declared_names<'w>(world: &'w World<'_>, kind: Id<Kind>) -> impl Iterator<Item = &'w str> {
    world.book.schema.effective(&world.book.kinds, kind).map(|slot| world.book.name(slot.name))
}

pub(crate) fn is_builtin_line(name: &str) -> bool {
    matches!(
        name,
        "owner"
            | "holds"
            | "select"
            | "opened"
            | "closed"
            | "liquidity"
            | "via"
            | "lives"
            | "member"
            | "precision"
            | "name"
            | "grows"
            | "restricted"
            | "deferred"
            | "basis"
            | "claim"
            | "purpose"
            | "pays"
            | "takes"
            | "sales-tax"
            | "share"
            | "citizen"
            | "books"
            | "currency"
            | "part"
    )
}

fn unknown_native_property(world: &World<'_>, target: NativeTarget, line: &Line<'_>) -> Diagnostic {
    let kind_name = world.book.name(world.book.kinds[target.kind].name);
    let noun = match target.holder {
        Holder::Kind(_) => "kind",
        Holder::Entity(_) => "entity",
        Holder::Commodity(_) => "commodity",
        Holder::Place(_) => "account",
        Holder::Asset(_) => "asset",
    };
    let mut valid: Vec<&str> = BUILTINS
        .iter()
        .filter(|builtin| match (builtin.1.contains(&Target::Kind), target.sort, target.holder) {
            (true, _, Holder::Kind(_)) => true,
            (false, Sort::Place(_), Holder::Place(_)) => builtin.1.contains(&Target::Place),
            (false, Sort::Entity, Holder::Entity(_)) => builtin.1.contains(&Target::Entity),
            (false, Sort::Commodity, Holder::Commodity(_)) => builtin.1.contains(&Target::Commodity),
            _ => false,
        })
        .map(|builtin| builtin.0)
        .collect();
    valid.extend(declared_names(world, target.kind));
    if matches!(target.holder, Holder::Asset(_)) {
        valid.extend(["owner", "known-as", "share", "at", "part", "also"]);
    }
    valid.sort_unstable();
    valid.dedup();
    let text = line.name.0;
    let owner = match target.holder {
        Holder::Kind(_) => format!("kind `{kind_name}`"),
        _ => format!("{} of kind `{kind_name}`", article(noun)),
    };
    let error = Diagnostic::error("unknown-property", format!("`{text}` is not a property of {owner}"))
        .label(line.loc, "no such property");
    let error = suggest(error, line.loc, text, valid.iter().copied());
    error.note(format!("its properties are {}", list(&valid)))
}

/// Required slots that nothing fills: not the thing, and not a kind above it.
fn missing_roles<'a, 's>(
    world: &World<'s>,
    collected: &Collected<'a, 's>,
    filled: &Filled,
    diags: &mut Vec<Diagnostic>,
) {
    let book = &world.book;
    for &written in &collected.decls {
        if matches!(written.node.what, DeclKind::Kind | DeclKind::Purpose) {
            continue;
        }
        let Some(target) = native_target(world, written) else { continue };
        for slot in book.schema.effective(&book.kinds, target.kind).filter(|slot| fill::is_required(slot.mult)) {
            let number = book.schema.number(slot.name).expect("a declared slot is numbered");
            let by_kind = |above: Id<Kind>| filled.contains(&(Holder::Kind(above), number));
            if filled.contains(&(target.holder, number)) || book.kinds.lineage(target.kind).any(by_kind) {
                continue;
            }
            diags.push(missing_role(world, written, target.kind, slot));
        }
    }
}

/// `college` is a 529-plan, which takes a person as its `beneficiary`, and none is written.
fn missing_role(world: &World<'_>, written: Written<'_, '_, Decl<'_>>, kind: Id<Kind>, slot: &Slot) -> Diagnostic {
    let book = &world.book;
    let thing = Word::of(written.file(), written.node.name.0);
    let candidates: Vec<&str> = match slot.range {
        Range::Kinds(run) => fill::fitting(world, book.schema.kinds_of(run)),
        Range::Words(run) => book.schema.words_of(run).iter().map(|&word| book.name(word)).collect(),
        Range::Value(_) => Vec::new(),
    };
    let takes = book.schema.view(slot.range).describe(book);
    let header = written.item.loc;
    let insert = Loc::new(header.file, header.end, header.end);
    problem::missing_role(thing, book.name(book.kinds[kind].name), book.name(slot.name), &takes, &candidates, insert)
}

#[cfg(test)]
mod native_property_tests {
    use super::*;

    #[test]
    fn asset_part_cycles_are_reported_and_cut_before_engine_walks() {
        let mut names = axiom_core::Interner::default();
        let a_name = names.intern("a");
        let b_name = names.intern("b");
        let mut assets = axiom_core::Arena::new();
        let empty = |name| Asset {
            name,
            kind: Id::new(0),
            owner: Id::new(0),
            place: Id::new(0),
            unit: Id::new(0),
            part_of: None,
            doc: None,
            loc: Loc::default(),
        };
        let a = assets.push(empty(a_name));
        let b = assets.push(empty(b_name));
        assets[a].part_of = Some(At { value: b, loc: Loc::default() });
        assets[b].part_of = Some(At { value: a, loc: Loc::default() });

        let mut diagnostics = Vec::new();
        diagnose_asset_cycles(&mut assets, &names, &mut diagnostics);

        assert_eq!(diagnostics.iter().filter(|diag| diag.code == "asset-part-cycle").count(), 1);
        assert!(assets.ids().any(|id| assets[id].part_of.is_none()));
    }

    #[test]
    fn kind_share_rates_are_bounded_and_point_to_the_written_rate() {
        let loc = Loc::new(axiom_core::FileId(2), 8, 11);
        assert!(validate_share_rate(Ratio::ZERO, loc).is_ok());
        assert!(validate_share_rate(Ratio::ONE, loc).is_ok());

        for rate in [Ratio::new(3, 2).unwrap(), Ratio::new(-1, 2).unwrap()] {
            let diagnostic = validate_share_rate(rate, loc).unwrap_err();
            assert_eq!(diagnostic.code, "share-rate-range");
            assert_eq!(diagnostic.anchor(), Some(loc));
        }
    }
}
