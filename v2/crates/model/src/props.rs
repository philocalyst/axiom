//! Properties: the lines written under kinds, accounts, entities and
//! commodities.
//!
//! Each line is read once into an [`Assign`], a typed setting: the language's
//! own (`owner`, `opened`, `via`, `precision`), or a value for a property that
//! a kind declared with `has NAME TYPE`. A line naming neither is an error with
//! a suggestion. A kind's lines are defaults for its instances: an instance is
//! what its kind chain assigns, farthest kind first, and then what it says
//! itself. Kinds are read before the things they describe, and the sorts in the
//! order their lines need: an amount is only exact once its commodity's
//! precision is settled.

use axiom_core::{Day, Days, Diagnostic, Dim, Id, Loc, Map, Ratio, Span, Sym, Tree};
use axiom_syntax::{
    BinOp, Change, ClauseKind, Decl, DeclKind, Expr, ExprId, ExprKind, File, ItemKind, Policy,
    Prop as Line, Subject, Verb,
};

use crate::book::{Amount, Basis, Commodity, Entity, Has, Kind, Place, Prop, Residence, Sort};
use crate::collect::{Entry, Written, decls};
use crate::declare::{MAX_SCALE, PropTarget, World};
use crate::errors::{Word, article, list, suggest};
use crate::law::{Ty, Value, Window};
use crate::scope::Home;
use crate::values::describe;

/// What a property line describes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Target {
    Place,
    Entity,
    Commodity,
    Kind,
}

impl Target {
    fn noun(self) -> &'static str {
        match self {
            Target::Place => "account",
            Target::Entity => "entity",
            Target::Commodity => "commodity",
            Target::Kind => "kind",
        }
    }

    /// What the instances of a kind of this sort are.
    fn of(sort: Sort) -> Target {
        match sort {
            // v3 bridge: no v3 thing has properties.
            Sort::Place(_) | Sort::Thing => Target::Place,
            Sort::Entity => Target::Entity,
            Sort::Commodity => Target::Commodity,
        }
    }
}

/// One property line, read: a setting for whatever it is applied to.
#[derive(Clone, Debug)]
enum Assign {
    Owner(Id<Entity>),
    Holds(Option<Box<[Id<Commodity>]>>, Loc),
    Select(Policy),
    Opened(Day, Loc),
    Closed(Day, Loc),
    Budget(Amount, Window, Loc),
    Liquidity(Span),
    Via(Id<Place>),
    Lives(Residence),
    Member(Id<Entity>, Loc),
    Precision(u8),
    Title(Sym),
    Grows(Ratio),
    Restricted,
    Deferred,
    Claim,
    Basis(Basis),
    Has(Has),
    /// A value for a property a kind declared.
    Prop(Prop),
}

impl Assign {
    /// Whether instances inherit it. Kinds keep the rest for themselves.
    fn is_default(&self) -> bool {
        !matches!(
            self,
            Assign::Restricted
                | Assign::Deferred
                | Assign::Claim
                | Assign::Basis(_)
                | Assign::Has(_)
        )
    }
}

/// How the arguments of one of the language's properties read.
type Reader = fn(&mut Args<'_, '_, '_>) -> Result<Assign, Diagnostic>;

/// The properties the language defines itself, what each may be written
/// under, and how it reads.
const BUILTINS: [(&str, &[Target], Reader); 18] = [
    ("owner", &[Target::Place], |a| a.entity().map(Assign::Owner)),
    ("holds", &[Target::Place], |a| {
        Ok(Assign::Holds(a.holds()?, a.line.loc))
    }),
    ("select", &[Target::Place, Target::Commodity], |a| {
        a.policy().map(Assign::Select)
    }),
    ("opened", &[Target::Place], |a| {
        Ok(Assign::Opened(a.day()?, a.line.loc))
    }),
    ("closed", &[Target::Place], |a| {
        Ok(Assign::Closed(a.day()?, a.line.loc))
    }),
    ("budget", &[Target::Place], |a| {
        let amount = a.amount()?;
        let monthly = a.word(&["monthly", "yearly"])? == "monthly";
        Ok(Assign::Budget(
            amount,
            if monthly { Window::Month } else { Window::Year },
            a.line.loc,
        ))
    }),
    ("liquidity", &[Target::Place, Target::Commodity], |a| {
        a.span().map(Assign::Liquidity)
    }),
    ("via", &[Target::Entity], |a| a.place().map(Assign::Via)),
    ("lives", &[Target::Entity], |a| a.residence()),
    ("member", &[Target::Entity], |a| {
        Ok(Assign::Member(a.entity()?, a.line.loc))
    }),
    ("precision", &[Target::Commodity], |a| {
        a.count(MAX_SCALE).map(Assign::Precision)
    }),
    ("name", &[Target::Commodity], |a| {
        let title = a.text()?;
        Ok(Assign::Title(a.world.book.names.intern(title)))
    }),
    ("grows", &[Target::Commodity], |a| {
        let rate = a.percent()?;
        a.word(&["yearly"])?;
        Ok(Assign::Grows(rate))
    }),
    ("restricted", &[Target::Kind], |_| Ok(Assign::Restricted)),
    ("deferred", &[Target::Kind], |_| Ok(Assign::Deferred)),
    ("basis", &[Target::Kind], |a| {
        Ok(Assign::Basis(if a.word(&["zero", "cost"])? == "zero" {
            Basis::Zero
        } else {
            Basis::Cost
        }))
    }),
    ("claim", &[Target::Kind], |_| Ok(Assign::Claim)),
    ("has", &[Target::Kind], |a| a.has()),
];

/// Words that read a value off a thing in a law (`self.balance`), so a
/// declared property may not take them.
const FIELD_WORDS: [&str; 8] = [
    "balance", "basis", "owner", "kind", "age", "unit", "year", "month",
];

const POLICIES: [(&str, Policy); 4] = [
    ("fifo", Policy::Fifo),
    ("lifo", Policy::Lifo),
    ("hifo", Policy::Hifo),
    ("prorata", Policy::Prorata),
];

const TYPES: [(&str, Ty); 14] = [
    ("date", Ty::Day),
    ("amount", Ty::AMOUNT),
    ("number", Ty::Num),
    ("percent", Ty::Num),
    ("span", Ty::Span),
    ("text", Ty::Text),
    ("name", Ty::Name),
    ("entity", Ty::Entity),
    ("place", Ty::Place),
    ("kind", Ty::Kind),
    ("unit", Ty::Unit),
    ("bool", Ty::Bool),
    ("purpose", Ty::Purpose),
    ("asset", Ty::Asset),
];

/// `budget 500 USD monthly`, waiting to become a law once laws can be built.
pub(crate) struct Budget {
    pub place: Id<Place>,
    pub amount: Amount,
    pub window: Window,
    pub loc: Loc,
}

/// Every declared property, typed once for all things of its sort: two kinds
/// that declare `filing` for entities agree on what it is, so `owner.filing`
/// has one type wherever it is written.
#[derive(Default)]
pub(crate) struct PropTable {
    declared: Map<(Ty, Sym), Has>,
}

impl PropTable {
    /// The type family a sort's properties are typed in: places, entities or
    /// commodities (as `Ty::Unit`), matching what a law's receiver is.
    fn family(sort: Sort) -> Ty {
        match sort {
            Sort::Place(_) => Ty::Place,
            Sort::Thing => Ty::Asset,
            Sort::Commodity => Ty::Unit,
            Sort::Entity => Ty::Entity,
        }
    }

    pub fn get(&self, family: Ty, name: Sym) -> Option<Has> {
        self.declared.get(&(family, name)).copied()
    }

    /// The declared property names of one family.
    pub fn names(&self, family: Ty) -> impl Iterator<Item = Sym> {
        self.declared
            .keys()
            .filter(move |key| key.0 == family)
            .map(|key| key.1)
    }
}

// ─── Applying ───────────────────────────────────────────────────────────────

/// Replaces the property `prop` names, or adds it.
fn put(props: &mut Box<[Prop]>, prop: Prop) {
    let mut all = std::mem::take(props).into_vec();
    match all.iter_mut().find(|held| held.name == prop.name) {
        Some(held) => *held = prop,
        None => all.push(prop),
    }
    *props = all.into();
}

impl Kind {
    /// What flows down from `above`, before this kind's own lines say more.
    fn inherit(&mut self, above: &Kind) {
        self.restricted |= above.restricted;
        self.deferred |= above.deferred;
        self.claim |= above.claim;
        self.basis = self.basis.or(above.basis);
        self.select = self.select.or(above.select);
        self.liquidity = self.liquidity.or(above.liquidity);
        self.props = above.props.clone();
    }

    fn set(&mut self, assign: &Assign) {
        match assign {
            Assign::Restricted => self.restricted = true,
            Assign::Deferred => self.deferred = true,
            Assign::Claim => self.claim = true,
            Assign::Basis(basis) => self.basis = Some(*basis),
            Assign::Select(policy) => self.select = Some(*policy),
            Assign::Liquidity(span) => self.liquidity = Some(*span),
            Assign::Prop(prop) => put(&mut self.props, *prop),
            _ => {}
        }
    }
}

/// Defaults written by each kind itself. Ancestors are walked when an instance
/// is built, so a deep kind does not copy every ancestor's assignments.
struct Defaults {
    by_kind: Vec<Vec<Assign>>,
}

impl Defaults {
    fn empty(kinds: usize) -> Defaults {
        Defaults {
            by_kind: vec![Vec::new(); kinds],
        }
    }

    /// Applies defaults from the oldest ancestor through `kind`, reusing one
    /// caller-owned path buffer across every instance of the same target.
    fn apply(
        &self,
        kinds: &Tree<Kind>,
        kind: Id<Kind>,
        path: &mut Vec<Id<Kind>>,
        mut apply: impl FnMut(&Assign),
    ) {
        path.clear();
        path.extend(kinds.lineage(kind));
        path.reverse();
        for ancestor in path.iter().copied() {
            for assign in &self.by_kind[ancestor.index()] {
                apply(assign);
            }
        }
    }
}

impl Commodity {
    fn set(&mut self, assign: &Assign) {
        match assign {
            Assign::Precision(scale) => self.scale = *scale,
            Assign::Title(title) => self.title = Some(*title),
            Assign::Select(policy) => self.select = Some(*policy),
            Assign::Liquidity(span) => self.liquidity = Some(*span),
            Assign::Grows(rate) => self.growth = Some(*rate),
            Assign::Prop(prop) => put(&mut self.props, *prop),
            _ => {}
        }
    }
}

impl Entity {
    fn set(&mut self, assign: &Assign) {
        match assign {
            Assign::Via(place) => self.place = Some(*place),
            Assign::Lives(residence) => {
                self.lives = self.lives.iter().copied().chain([*residence]).collect()
            }
            Assign::Member(entity, _) => self.member = Some(*entity),
            Assign::Prop(prop) => put(&mut self.props, *prop),
            _ => {}
        }
    }
}

impl Place {
    fn set(&mut self, assign: &Assign) {
        match assign {
            Assign::Owner(owner) => self.owner = *owner,
            Assign::Holds(holds, _) => self.holds = holds.clone(),
            Assign::Select(policy) => self.select = Some(*policy),
            Assign::Opened(day, _) => self.opened = Some(*day),
            Assign::Closed(day, _) => self.closed = Some(*day),
            Assign::Liquidity(span) => self.liquidity = Some(*span),
            Assign::Prop(prop) => put(&mut self.props, *prop),
            _ => {}
        }
    }
}

// ─── Reading ────────────────────────────────────────────────────────────────

/// The arguments of one property line, read in order.
struct Args<'w, 'a, 's> {
    world: &'w mut World<'s>,
    file: &'a File<'s>,
    ids: &'a [ExprId],
    line: &'a Line<'s>,
    home: Home,
    next: usize,
}

impl<'a, 's> Args<'_, 'a, 's> {
    fn peek(&self) -> Option<&'a Expr<'s>> {
        self.ids.get(self.next).map(|&id| &self.file.exprs[id])
    }

    /// The next argument, which the property needs to be `wanted`.
    fn next_id(&mut self, wanted: &str) -> Result<ExprId, Diagnostic> {
        let Some(&id) = self.ids.get(self.next) else {
            return Err(Diagnostic::error(
                "property-argument",
                format!("`{}` needs {wanted}", self.line.name.0),
            )
            .label(self.line.loc, format!("{wanted} should follow here")));
        };
        self.next += 1;
        Ok(id)
    }

    fn wrong(&self, expr: &Expr, wanted: &str) -> Diagnostic {
        Diagnostic::error(
            "property-type",
            format!("`{}` needs {wanted}", self.line.name.0),
        )
        .label(expr.loc, format!("this is {}", describe(&expr.kind)))
    }

    /// The next argument, as `pick` reads it out of the expression.
    fn arg<T>(
        &mut self,
        wanted: &str,
        pick: impl FnOnce(&Expr<'s>) -> Option<T>,
    ) -> Result<T, Diagnostic> {
        let expr = &self.file.exprs[self.next_id(wanted)?];
        pick(expr).ok_or_else(|| self.wrong(expr, wanted))
    }

    fn done(&self) -> Result<(), Diagnostic> {
        let Some(extra) = self.peek() else {
            return Ok(());
        };
        Err(Diagnostic::error(
            "property-argument",
            format!("`{}` takes no more arguments here", self.line.name.0),
        )
        .label(extra.loc, "unexpected"))
    }

    /// One of the `allowed` words.
    fn word(&mut self, allowed: &[&str]) -> Result<&'s str, Diagnostic> {
        let wanted = if allowed.len() == 1 {
            format!("`{}`", allowed[0])
        } else {
            format!("one of {}", list(allowed))
        };
        let expr = &self.file.exprs[self.next_id(&wanted)?];
        match expr.kind {
            ExprKind::Name(name) if allowed.contains(&name.0) => Ok(name.0),
            ExprKind::Name(name) => {
                let error = self
                    .wrong(expr, &wanted)
                    .label(expr.loc, format!("`{}` is not one of them", name.0));
                Err(suggest(error, expr.loc, name.0, allowed.iter().copied()))
            }
            _ => Err(self.wrong(expr, &wanted)),
        }
    }

    fn name(&mut self, wanted: &str) -> Result<Word<'s>, Diagnostic> {
        let name = |expr: &Expr<'s>| match expr.kind {
            ExprKind::Name(name) => Some(Word {
                text: name.0,
                loc: expr.loc,
            }),
            _ => None,
        };
        self.arg(wanted, name)
    }

    fn day(&mut self) -> Result<Day, Diagnostic> {
        self.arg("a date", |expr| {
            if let ExprKind::Date(day) = expr.kind {
                Some(day)
            } else {
                None
            }
        })
    }

    fn span(&mut self) -> Result<Span, Diagnostic> {
        let span = |expr: &Expr| {
            if let ExprKind::Span(span) = expr.kind {
                Some(span)
            } else {
                None
            }
        };
        self.arg("a span such as `5d` or `1y6m`", span)
    }

    fn text(&mut self) -> Result<&'s str, Diagnostic> {
        self.arg("text in quotes", |expr| {
            if let ExprKind::Str(text) = expr.kind {
                Some(text)
            } else {
                None
            }
        })
    }

    /// A whole number from zero to `max`.
    fn count(&mut self, max: u8) -> Result<u8, Diagnostic> {
        self.arg(&format!("a whole number up to {max}"), |expr| {
            match expr.kind {
                ExprKind::Num(number) => number
                    .to_qty(0)
                    .ok()
                    .and_then(|qty| u8::try_from(qty.0).ok())
                    .filter(|&n| n <= max),
                _ => None,
            }
        })
    }

    fn percent(&mut self) -> Result<Ratio, Diagnostic> {
        let percent = |expr: &Expr| match expr.kind {
            ExprKind::Pct(number) => Ratio::percent(number.mantissa.into(), number.scale),
            _ => None,
        };
        self.arg("a percentage such as `5%`", percent)
    }

    fn entity(&mut self) -> Result<Id<Entity>, Diagnostic> {
        let word = self.name("an entity")?;
        self.world.entity(self.home, word)
    }

    fn place(&mut self) -> Result<Id<Place>, Diagnostic> {
        let word = self.name("a place")?;
        self.world.place(word)
    }

    fn amount(&mut self) -> Result<Amount, Diagnostic> {
        let expr = &self.file.exprs[self.next_id("an amount")?];
        match self.world.literal(self.file, expr)? {
            Some((Value::Amount(amount), _)) => Ok(amount),
            _ => Err(self.wrong(expr, "an amount such as `500 USD`")),
        }
    }

    fn policy(&mut self) -> Result<Policy, Diagnostic> {
        let words: Vec<&str> = POLICIES.iter().map(|policy| policy.0).collect();
        let word = self.word(&words)?;
        Ok(POLICIES
            .iter()
            .find(|policy| policy.0 == word)
            .map_or(Policy::Fifo, |policy| policy.1))
    }

    /// `holds USD, VTI`, or `holds any`.
    fn holds(&mut self) -> Result<Option<Box<[Id<Commodity>]>>, Diagnostic> {
        if matches!(self.peek().map(|expr| &expr.kind), Some(ExprKind::Name(name)) if name.0 == "any")
        {
            self.word(&["any"])?;
            return Ok(None);
        }
        let mut units = Vec::new();
        while let Some(expr) = self.peek() {
            let ExprKind::Unit(symbol) = expr.kind else {
                return Err(self.wrong(expr, "commodities such as `USD`, or `any`"));
            };
            self.next += 1;
            units.push(self.world.commodity_of(Word {
                text: symbol.0,
                loc: expr.loc,
            })?);
        }
        match units.is_empty() {
            true => Err(Diagnostic::error(
                "property-argument",
                "`holds` needs commodities or `any`",
            )
            .label(self.line.loc, "name what it holds")),
            false => Ok(Some(units.into())),
        }
    }

    /// `lives us/ca`, or `lives us/ca from 2026-01-01 until 2026-06-30`.
    fn residence(&mut self) -> Result<Assign, Diagnostic> {
        let word = self.name("a system")?;
        let system = self.world.system(word)?;
        let (mut from, mut until) = (Day::MIN, Day::MAX);
        while self.peek().is_some() {
            match self.word(&["from", "until"])? {
                "from" => from = self.day()?,
                _ => until = self.day()?,
            }
        }
        let Some(days) = Days::new(from, until) else {
            return Err(Diagnostic::error(
                "residence-order",
                "this residence ends before it begins",
            )
            .label(self.line.loc, "`until` is earlier than `from`")
            .help("swap the two dates"));
        };
        Ok(Assign::Lives(Residence { days, system }))
    }

    /// `has employer entity`
    fn has(&mut self) -> Result<Assign, Diagnostic> {
        let name = self.name("a property name")?;
        let ty = |expr: &Expr| match expr.kind {
            ExprKind::Name(written) => TYPES.iter().find(|ty| ty.0 == written.0).map(|ty| ty.1),
            _ => None,
        };
        let wanted = format!("a type: {}", list(&TYPES.map(|ty| ty.0)));
        let ty = self.arg(&wanted, ty)?;
        if BUILTINS.iter().any(|builtin| builtin.0 == name.text) || FIELD_WORDS.contains(&name.text)
        {
            return Err(Diagnostic::error(
                "reserved-property",
                format!("`{}` is a built-in property", name.text),
            )
            .label(name.loc, "choose another name")
            .note(
                "built-in properties keep their meaning everywhere, so a kind cannot redefine them",
            ));
        }
        Ok(Assign::Has(Has {
            name: self.world.book.names.intern(name.text),
            ty,
            loc: Some(name.loc),
        }))
    }

    /// The value of a property a kind declared.
    fn declared(&mut self, has: Has) -> Result<Assign, Diagnostic> {
        let id = self.next_id("a value")?;
        let (value, _) = self
            .world
            .constant(self.home, self.file, id, Some(has.ty))?;
        // v3 bridge: a property holds from the beginning; statements that change one come with the v4 model.
        Ok(Assign::Prop(Prop {
            name: has.name,
            value,
            since: Day::MIN,
            loc: Some(self.line.loc),
        }))
    }
}

/// The lines under one declaration, and where they were written.
struct Lines<'a, 's> {
    home: Home,
    file: &'a File<'s>,
    lines: &'a [Line<'s>],
}

impl<'a, 's> Lines<'a, 's> {
    fn of(written: &Written<'a, 's, Decl<'s>>) -> Lines<'a, 's> {
        let file = written.file();
        Lines {
            home: written.home(),
            file,
            lines: &file[written.node.props],
        }
    }
}

/// The setting a line makes: one of the language's for something it is
/// written under (`targets`), or one a kind declared (`has`).
fn read_line<'s>(
    world: &mut World<'s>,
    at: &Lines<'_, 's>,
    line: &Line<'s>,
    targets: &[Target],
    has: &[Has],
    kind: Id<Kind>,
) -> Result<Assign, Diagnostic> {
    let word = line.name.0;
    let builtin = BUILTINS
        .iter()
        .find(|entry| entry.0 == word && targets.iter().any(|t| entry.1.contains(t)));
    let declared = world
        .book
        .names
        .get(word)
        .and_then(|sym| has.iter().find(|has| has.name == sym))
        .copied();
    let read = match (builtin, declared) {
        (Some(entry), _) => Ok(entry.2),
        (None, Some(has)) => Err(has),
        (None, None) => {
            return Err(unknown_property(
                world,
                at.file.loc(word),
                targets[0],
                kind,
                has,
                word,
            ));
        }
    };
    let mut args = Args {
        world,
        file: at.file,
        ids: &at.file[line.args],
        line,
        home: at.home,
        next: 0,
    };
    let assign = match read {
        Ok(read) => read(&mut args)?,
        Err(has) => args.declared(has)?,
    };
    args.done()?;
    Ok(assign)
}

/// `benificiary` is not a property of an account of kind `529`.
fn unknown_property(
    world: &World,
    loc: Loc,
    target: Target,
    kind: Id<Kind>,
    has: &[Has],
    word: &str,
) -> Diagnostic {
    let kind_name = world.book.name(world.book.kinds[kind].name);
    let mut valid: Vec<&str> = BUILTINS
        .iter()
        .filter(|entry| entry.1.contains(&target))
        .map(|entry| entry.0)
        .collect();
    valid.extend(has.iter().map(|has| world.book.name(has.name)));
    let of = match target {
        Target::Kind => format!("kind `{kind_name}`"),
        target => format!("{} of kind `{kind_name}`", article(target.noun())),
    };
    let error = Diagnostic::error(
        "unknown-property",
        format!("`{word}` is not a property of {of}"),
    )
    .label(loc, "no such property");
    let error = suggest(error, loc, word, valid.iter().copied());
    match BUILTINS.iter().find(|entry| entry.0 == word) {
        Some(entry) => {
            let owners: Vec<String> = entry
                .1
                .iter()
                .map(|owner| format!("{}s", owner.noun()))
                .collect();
            error.note(format!(
                "`{word}` describes {}, not {}",
                owners.join(" and "),
                article(target.noun())
            ))
        }
        None => error,
    }
    .note(format!("its properties are {}", list(&valid)))
}

/// The settings a declaration's lines make. `has` lines belong to the kind,
/// which reads them first.
fn read_lines<'s>(
    world: &mut World<'s>,
    at: &Lines<'_, 's>,
    targets: &[Target],
    has: &[Has],
    kind: Id<Kind>,
    diags: &mut Vec<Diagnostic>,
) -> Vec<Assign> {
    let own = at
        .lines
        .iter()
        .filter(|line| line.name.0 != "has" || !targets.contains(&Target::Kind));
    let read = own.map(|line| read_line(world, at, line, targets, has, kind));
    read.filter_map(|read| read.map_err(|error| diags.push(error)).ok())
        .collect()
}

/// The first declaration of each thing: a repeat is an error, and its lines
/// are not read.
fn first_of<'a, 's, T>(
    written: impl Iterator<Item = Written<'a, 's, Decl<'s>>>,
    ids: impl Iterator<Item = Option<Id<T>>>,
) -> Map<Id<T>, Written<'a, 's, Decl<'s>>> {
    let mut first = Map::default();
    for (written, id) in written.zip(ids) {
        if let Some(id) = id {
            first.entry(id).or_insert(written);
        }
    }
    first
}

pub(crate) fn apply<'s>(
    world: &mut World<'s>,
    entries: &[Entry<'_, 's>],
    diags: &mut Vec<Diagnostic>,
) -> Vec<Budget> {
    let mut budgets = Vec::new();
    for target in [Target::Commodity, Target::Entity, Target::Place] {
        let defaults = kinds(world, entries, target, diags);
        match target {
            Target::Commodity => commodities(world, entries, &defaults, diags),
            Target::Entity => entities(world, entries, &defaults, diags),
            _ => places(world, entries, &defaults, &mut budgets, diags),
        }
    }
    budgets
}

/// Reads the kinds of one sort, parents first, and returns what each hands
/// down to its instances.
fn kinds<'s>(
    world: &mut World<'s>,
    entries: &[Entry<'_, 's>],
    target: Target,
    diags: &mut Vec<Diagnostic>,
) -> Defaults {
    let ids: Vec<Id<Kind>> = world
        .book
        .kinds
        .ids()
        .filter(|&id| Target::of(world.book.kinds[id].sort) == target)
        .collect();
    let written = first_of(
        decls(entries, DeclKind::Kind),
        world.declared.kinds.iter().map(|&id| Some(id)),
    );
    // What each kind may set on its instances comes first: its own `has`
    // lines, then the ones it inherits.
    for &id in &ids {
        let mut own = Vec::new();
        if let Some(w) = written.get(&id) {
            let at = Lines::of(w);
            for line in at.lines.iter().filter(|line| line.name.0 == "has") {
                match read_line(world, &at, line, &[Target::Kind], &[], id) {
                    Ok(Assign::Has(has)) => own.push(has),
                    Ok(_) => {}
                    Err(error) => diags.push(error),
                }
            }
        }
        let inherited = world
            .book
            .kinds
            .parent(id)
            .map_or(Box::default(), |parent| {
                world.book.kinds[parent].has.clone()
            });
        let sort = world.book.kinds[id].sort;
        world.declare_props(sort, &own, diags);
        let unshadowed = inherited
            .iter()
            .filter(|theirs| !own.iter().any(|mine| mine.name == theirs.name));
        world.book.kinds[id].has = own.iter().chain(unshadowed).copied().collect();
    }
    let mut defaults = Defaults::empty(world.book.kinds.len());
    for &id in &ids {
        if world.book.kinds.parent(id).is_some() {
            let (above, kind) = world
                .book
                .kinds
                .with_parent_mut(id)
                .expect("parent was checked");
            kind.inherit(above);
        }
        let Some(w) = written.get(&id) else { continue };
        let has = world.book.kinds[id].has.clone();
        for assign in read_lines(
            world,
            &Lines::of(w),
            &[Target::Kind, target],
            &has,
            id,
            diags,
        ) {
            world.book.kinds[id].set(&assign);
            if assign.is_default() {
                defaults.by_kind[id.index()].push(assign);
            }
        }
    }
    defaults
}

/// Reads only this thing's own lines. Kind defaults stay borrowed in `Defaults`
/// and are applied directly by each target-specific setter.
fn own_settings<'a, 's>(
    world: &mut World<'s>,
    (kind, target): (Id<Kind>, Target),
    written: Option<&Written<'a, 's, Decl<'s>>>,
    diags: &mut Vec<Diagnostic>,
) -> Vec<Assign> {
    let has = world.book.kinds[kind].has.clone();
    written.map_or_else(Vec::new, |w| {
        read_lines(world, &Lines::of(w), &[target], &has, kind, diags)
    })
}

fn commodities<'s>(
    world: &mut World<'s>,
    entries: &[Entry<'_, 's>],
    defaults: &Defaults,
    diags: &mut Vec<Diagnostic>,
) {
    let written = first_of(
        decls(entries, DeclKind::Commodity),
        world.declared.commodities.iter().map(|&id| Some(id)),
    );
    let mut path = Vec::new();
    for id in world.book.commodities.ids().collect::<Vec<_>>() {
        let kind = world.book.commodities[id].kind;
        let own = own_settings(world, (kind, Target::Commodity), written.get(&id), diags);
        let (kinds, commodities) = (&world.book.kinds, &mut world.book.commodities);
        defaults.apply(kinds, kind, &mut path, |assign| commodities[id].set(assign));
        for assign in &own {
            commodities[id].set(assign);
        }
    }
}

fn entities<'s>(
    world: &mut World<'s>,
    entries: &[Entry<'_, 's>],
    defaults: &Defaults,
    diags: &mut Vec<Diagnostic>,
) {
    let written = first_of(
        decls(entries, DeclKind::Entity),
        world.declared.entities.iter().map(|&id| Some(id)),
    );
    let mut path = Vec::new();
    for id in world.book.entities.ids().collect::<Vec<_>>() {
        let kind = world.book.entities[id].kind;
        let own = own_settings(world, (kind, Target::Entity), written.get(&id), diags);
        let restricted = world.book.kinds[kind].restricted;
        let (kinds, entities) = (&world.book.kinds, &mut world.book.entities);
        let entity = &mut entities[id];
        entity.restricted = restricted;
        let mut apply = |assign: &Assign| match assign {
            Assign::Member(member, loc) if *member == id => diags.push(
                Diagnostic::error("member-self", "an entity cannot be a member of itself")
                    .label(*loc, "name the household this person belongs to"),
            ),
            _ => entity.set(assign),
        };
        defaults.apply(kinds, kind, &mut path, &mut apply);
        for assign in &own {
            apply(assign);
        }
        let mut lives = std::mem::take(&mut entity.lives).into_vec();
        lives.sort_by_key(|residence| residence.days);
        entity.lives = lives.into();
    }
}

fn places<'s>(
    world: &mut World<'s>,
    entries: &[Entry<'_, 's>],
    defaults: &Defaults,
    budgets: &mut Vec<Budget>,
    diags: &mut Vec<Diagnostic>,
) {
    let written = first_of(
        decls(entries, DeclKind::Account),
        world.declared.places.iter().copied(),
    );
    let mut path = Vec::new();
    for id in world.book.places.ids().collect::<Vec<_>>() {
        let kind = world.book.places[id].kind;
        let own = own_settings(world, (kind, Target::Place), written.get(&id), diags);
        let facts = &world.book.kinds[kind];
        // Money in a `deferred` place is untaxed until it leaves, so unless its kind
        // says otherwise, none of it counts as already accounted for.
        let default = if facts.deferred {
            Basis::Zero
        } else {
            Basis::Cost
        };
        let (deferred, basis, claim) =
            (facts.deferred, facts.basis.unwrap_or(default), facts.claim);
        let (kinds, places) = (&world.book.kinds, &mut world.book.places);
        let place = &mut places[id];
        (place.deferred, place.basis, place.claim) = (deferred, basis, claim);
        let lines = &mut world.lines;
        let mut apply = |assign: &Assign| {
            match *assign {
                Assign::Budget(amount, window, loc) => budgets.push(Budget {
                    place: id,
                    amount,
                    window,
                    loc,
                }),
                Assign::Holds(_, loc) => drop(lines.insert((id, "holds"), loc)),
                Assign::Opened(_, loc) => drop(lines.insert((id, "opened"), loc)),
                Assign::Closed(_, loc) => drop(lines.insert((id, "closed"), loc)),
                _ => {}
            }
            place.set(assign);
        };
        defaults.apply(kinds, kind, &mut path, &mut apply);
        for assign in &own {
            apply(assign);
        }
        if let (Some(opened), Some(closed), Some(loc)) = (place.opened, place.closed, place.loc)
            && closed < opened
        {
            diags.push(
                Diagnostic::error(
                    "closed-before-opened",
                    "this account closes before it opens",
                )
                .label(loc, "opened and closed the wrong way round"),
            );
        }
    }
}

impl<'s> World<'s> {
    /// Records the properties a kind declares. Two kinds that declare one name
    /// for one sort of thing must give it the same type.
    fn declare_props(&mut self, sort: Sort, own: &[Has], diags: &mut Vec<Diagnostic>) {
        let family = PropTable::family(sort);
        for &has in own {
            let earlier = *self.props.declared.entry((family, has.name)).or_insert(has);
            if earlier.ty != has.ty {
                diags.push(disagreement(earlier, has, family, self.book.name(has.name)));
            }
        }
    }
}

fn disagreement(first: Has, again: Has, family: Ty, name: &str) -> Diagnostic {
    let noun = family.word();
    let mut diagnostic = Diagnostic::error(
        "property-type",
        format!(
            "`{name}` is declared as {} here, but as {} elsewhere",
            article(again.ty.word()),
            article(first.ty.word())
        ),
    );
    if let Some(loc) = again.loc {
        diagnostic = diagnostic.label(loc, format!("{} here", again.ty.word()));
    }
    if let Some(loc) = first.loc {
        diagnostic = diagnostic.context(loc, format!("{} here", first.ty.word()));
    }
    diagnostic.note(format!(
        "a property has one type for every {noun}, so that `.{name}` means the same thing wherever it is written"
    ))
}

// ─── Native S5 declarations ────────────────────────────────────────────────

#[derive(Clone, Copy)]
struct NativeDecl<'a, 's> {
    home: Home,
    file: &'a File<'s>,
    decl: &'a Decl<'s>,
    loc: Loc,
}

#[derive(Clone, Copy)]
struct NativeTarget {
    target: PropTarget,
    kind: Id<Kind>,
    sort: Sort,
}

#[derive(Clone, Copy)]
struct PropertyChange {
    target: PropTarget,
    name: Sym,
    since: Day,
    until: Option<Day>,
    value: Value,
    loc: Loc,
    order: usize,
}

/// Resolves custom `has` properties and their values directly from the S5 AST.
/// Kind defaults stay on their kind and are inherited by the law evaluator via
/// the kind tree; this pass never clones a default into every instance.
pub(crate) fn declare<'a, 's>(
    world: &mut World<'s>,
    sites: &'a [crate::sources::Site<'a, 's>],
    diags: &mut Vec<Diagnostic>,
) {
    let mut kind_decls: Map<Id<Kind>, NativeDecl<'a, 's>> = Map::default();
    let mut decls = Vec::new();
    for site in sites {
        let file = &site.source.file;
        for item in &file.items {
            let ItemKind::Decl(id) = item.kind else {
                continue;
            };
            let decl = &file[id];
            let native = NativeDecl {
                home: site.home,
                file,
                decl,
                loc: item.loc,
            };
            decls.push(native);
            if decl.what == DeclKind::Kind {
                let word = Word {
                    text: decl.name.0,
                    loc: item.loc,
                };
                if let Ok(kind) = world.kind(site.home, word) {
                    kind_decls.entry(kind).or_insert(native);
                }
            }
        }
    }

    // The kind tree is preordered, so a child's inherited declarations are
    // available before its own declarations are checked.
    let kinds: Vec<Id<Kind>> = world.book.kinds.ids().collect();
    for kind in kinds {
        let own = match kind_decls.get(&kind).copied() {
            Some(written) => read_has_lines(world, written.file, written.decl, diags),
            None => Vec::new(),
        };
        let sort = world.book.kinds[kind].sort;
        world.declare_props(sort, &own, diags);
        let inherited: Vec<Has> = world
            .book
            .kinds
            .parent(kind)
            .map(|parent| world.book.kinds[parent].has.iter().copied().collect())
            .unwrap_or_default();
        let inherited = inherited
            .iter()
            .filter(|above| !own.iter().any(|child| child.name == above.name));
        world.book.kinds[kind].has = own.iter().chain(inherited).copied().collect();
    }

    let mut seen: Map<((u8, u32), Sym, Day), Loc> = Map::default();
    for written in decls {
        if written.decl.what == DeclKind::Purpose {
            continue;
        }
        let Some(target) = native_target(world, written) else {
            continue;
        };
        let lines = &written.file[written.decl.props];
        for line in lines {
            if is_builtin_line(line.name.0) {
                continue;
            }
            let Some(has) = has_named(world, target.kind, line.name.0) else {
                diags.push(unknown_native_property(world, target, line));
                continue;
            };
            let Some(value) = property_value(world, written.home, written.file, line, has, diags)
            else {
                continue;
            };
            stage_unique(
                world,
                &mut seen,
                target.target,
                Prop {
                    name: has.name,
                    value,
                    since: Day::MIN,
                    loc: Some(line.loc),
                },
                diags,
            );
        }
    }

    let mut updates = Vec::new();
    let mut order = 0;
    for site in sites {
        let file = &site.source.file;
        for item in &file.items {
            let ItemKind::Statement(id) = item.kind else {
                continue;
            };
            let statement = &file[id];
            let Verb::Now(Change::Property(line)) = &statement.verb else {
                continue;
            };
            if is_builtin_line(line.name.0) {
                continue;
            }
            let property_name = world.book.names.intern(line.name.0);
            let Some(target) = native_statement_target(
                world,
                site.home,
                statement.subject,
                property_name,
                item.loc,
                diags,
            ) else {
                // The statement pass owns unresolved subjects. This pass only
                // consumes a custom property after its target kind is known.
                continue;
            };
            let Some(has) = has_named(world, target.kind, line.name.0) else {
                diags.push(unknown_native_property(world, target, line));
                continue;
            };
            let Some(value) = property_value(world, site.home, file, line, has, diags) else {
                continue;
            };
            let until = file[statement.tail]
                .iter()
                .find_map(|clause| match clause.kind {
                    ClauseKind::Until(day) => Some(day),
                    _ => None,
                });
            if until.is_some_and(|day| day < statement.date) {
                diags.push(
                    Diagnostic::error(
                        "property-until-order",
                        "this property change ends before it begins",
                    )
                    .label(line.loc, "`until` is earlier than the change")
                    .help("move the end date to the change date or later"),
                );
                continue;
            }
            let key = (target_key(target.target), has.name, statement.date);
            if let Some(first) = seen.get(&key).copied() {
                diags.push(
                    Diagnostic::error(
                        "duplicate-property-change",
                        "this property changes twice on the same day",
                    )
                    .label(line.loc, "change written again here")
                    .context(first, "first change written here"),
                );
                continue;
            }
            seen.insert(key, line.loc);
            updates.push(PropertyChange {
                target: target.target,
                name: has.name,
                since: statement.date,
                until,
                value,
                loc: line.loc,
                order,
            });
            order += 1;
        }
    }
    stage_changes(world, updates, diags);
}

fn read_has_lines<'s>(
    world: &mut World<'s>,
    file: &File<'s>,
    decl: &Decl<'s>,
    diags: &mut Vec<Diagnostic>,
) -> Vec<Has> {
    let mut own = Vec::new();
    for line in &file[decl.props] {
        if line.name.0 != "has" {
            continue;
        }
        let has = match parse_has(world, file, line) {
            Ok(has) => has,
            Err(problem) => {
                diags.push(problem);
                continue;
            }
        };
        if is_builtin_line(world.book.name(has.name))
            || FIELD_WORDS.contains(&world.book.name(has.name))
        {
            diags.push(
                    Diagnostic::error("reserved-property", format!("`{}` is a built-in property", world.book.name(has.name)))
                        .label(has.loc.unwrap_or(line.loc), "choose another name")
                        .note("built-in properties keep their meaning everywhere, so a kind cannot redefine them"),
                );
            continue;
        }
        if own.iter().any(|earlier: &Has| earlier.name == has.name) {
            let earlier = own
                .iter()
                .find(|earlier| earlier.name == has.name)
                .copied()
                .unwrap();
            let mut diagnostic = Diagnostic::error(
                "duplicate-property-declaration",
                format!(
                    "property `{}` is declared twice on this kind",
                    world.book.name(has.name)
                ),
            );
            if let Some(loc) = has.loc {
                diagnostic = diagnostic.label(loc, "declared again here");
            }
            if let Some(loc) = earlier.loc {
                diagnostic = diagnostic.context(loc, "first declared here");
            }
            diags.push(diagnostic);
            continue;
        }
        own.push(has);
    }
    own
}

fn parse_has<'s>(
    world: &mut World<'s>,
    file: &File<'s>,
    line: &Line<'s>,
) -> Result<Has, Diagnostic> {
    let args = &file[line.args];
    if args.len() != 2 {
        return Err(
            Diagnostic::error("has-type", "`has` needs a property name and a type")
                .label(line.loc, "write `has name type` or `has name UNIT`"),
        );
    }
    let name_expr = &file.exprs[args[0]];
    let ExprKind::Name(name) = name_expr.kind else {
        return Err(
            Diagnostic::error("has-name", "a custom property needs a name")
                .label(name_expr.loc, "write a lower-case property name"),
        );
    };
    let type_expr = &file.exprs[args[1]];
    let ty = match type_expr.kind {
        ExprKind::Name(type_name) => match TYPES
            .iter()
            .find(|(name, _)| *name == type_name.0)
            .map(|(_, ty)| *ty)
        {
            Some(ty) => ty,
            None => {
                let valid: Vec<&str> = TYPES.iter().map(|(name, _)| *name).collect();
                return Err(suggest(
                    Diagnostic::error(
                        "has-type",
                        format!("`{}` is not a property type", type_name.0),
                    )
                    .label(type_expr.loc, "unknown type"),
                    type_expr.loc,
                    type_name.0,
                    valid,
                ));
            }
        },
        ExprKind::Unit(unit) => {
            let commodity = world.commodity_of(Word {
                text: unit.0,
                loc: type_expr.loc,
            })?;
            Ty::Amount(Dim::Of(commodity))
        }
        ExprKind::Binary(BinOp::Div, top, bottom) => {
            let numerator = unit_type(world, file, top)?;
            let denominator = unit_type(world, file, bottom)?;
            match numerator.div(denominator) {
                Some(dim) if dim != Dim::Number && dim != Dim::Any => Ty::Amount(dim),
                _ => {
                    return Err(Diagnostic::error(
                        "has-unit",
                        "this is not a supported amount unit",
                    )
                    .label(
                        type_expr.loc,
                        "write one commodity or a rate such as `USD/MI`",
                    ));
                }
            }
        }
        _ => {
            return Err(
                Diagnostic::error("has-type", "a property type must be a name or a unit")
                    .label(type_expr.loc, "write a built-in type or a commodity unit"),
            );
        }
    };
    let name = world.book.names.intern(name.0);
    Ok(Has {
        name,
        ty,
        loc: Some(name_expr.loc),
    })
}

fn unit_type<'s>(
    world: &World<'s>,
    file: &File<'s>,
    id: ExprId,
) -> Result<Dim<Id<Commodity>>, Diagnostic> {
    let expr = &file.exprs[id];
    let (text, loc) = match expr.kind {
        ExprKind::Unit(name) | ExprKind::Name(name) => (name.0, expr.loc),
        _ => {
            return Err(
                Diagnostic::error("has-unit", "this is not a commodity unit")
                    .label(expr.loc, "write a declared unit such as `USD`"),
            );
        }
    };
    world.commodity_of(Word { text, loc }).map(Dim::Of)
}

fn property_value<'s>(
    world: &mut World<'s>,
    home: Home,
    file: &File<'s>,
    line: &Line<'s>,
    has: Has,
    diags: &mut Vec<Diagnostic>,
) -> Option<Value> {
    let args = &file[line.args];
    if args.len() != 1 {
        diags.push(
            Diagnostic::error(
                "property-value",
                format!("`{}` needs one value", line.name.0),
            )
            .label(line.loc, "write one value here"),
        );
        return None;
    }
    if !file[line.lines].is_empty() {
        diags.push(
            Diagnostic::error(
                "property-nested-lines",
                "a custom property value cannot have nested lines",
            )
            .label(line.loc, "remove the nested lines"),
        );
        return None;
    }
    let expr = &file.exprs[args[0]];
    let wanted = match has.ty {
        Ty::Amount(_) if matches!(expr.kind, ExprKind::Num(_) | ExprKind::Pct(_)) => Ty::Num,
        Ty::Amount(_) => Ty::AMOUNT,
        other => other,
    };
    let (value, found) = match world.constant(home, file, args[0], Some(wanted)) {
        Ok(value) => value,
        Err(problem) => {
            diags.push(problem);
            return None;
        }
    };
    if !property_value_fits(has.ty, value, found) {
        let expected = match has.ty {
            Ty::Amount(dim) => format!("an amount in {}", dimension_name(world, dim)),
            ty => article(ty.word()),
        };
        diags.push(
            Diagnostic::error(
                "property-type",
                format!("`{}` needs {expected}", line.name.0),
            )
            .label(expr.loc, format!("this is {}", describe(&expr.kind))),
        );
        return None;
    }
    Some(value)
}

fn property_value_fits(expected: Ty, value: Value, found: Ty) -> bool {
    match (expected, value) {
        (Ty::Amount(Dim::Any), Value::Amount(_)) | (Ty::Amount(Dim::Any), Value::Empty) => true,
        (Ty::Amount(Dim::Of(want)), Value::Amount(amount)) => amount.unit == want,
        (Ty::Amount(Dim::Of(_)), Value::Num(_)) => true,
        (Ty::Amount(Dim::Per(_, _)), Value::Num(_)) => true,
        (Ty::Amount(Dim::Rate(_, _)), Value::Num(_)) => true,
        (Ty::Amount(Dim::Number), Value::Num(_)) => true,
        (Ty::Amount(_), Value::Empty) => true,
        (ty, _) => crate::values::fits(ty, found),
    }
}

fn dimension_name(world: &World<'_>, dim: Dim<Id<Commodity>>) -> String {
    let unit = |unit: Id<Commodity>| {
        world
            .book
            .name(world.book.commodities[unit].symbol)
            .to_owned()
    };
    match dim {
        Dim::Number => "a number".to_owned(),
        Dim::Of(id) => unit(id),
        Dim::Per(top, bottom) => format!("{}/{}", unit(top), unit(bottom)),
        Dim::Rate(id, period) => format!("{} per {period:?}", unit(id)),
        Dim::Any => "an amount".to_owned(),
    }
}

fn native_target(world: &World<'_>, written: NativeDecl<'_, '_>) -> Option<NativeTarget> {
    let word = Word {
        text: written.decl.name.0,
        loc: written.loc,
    };
    let found = match written.decl.what {
        DeclKind::Account => world.place(word).ok().map(|id| NativeTarget {
            target: PropTarget::Place(id),
            kind: world.book.places[id].kind,
            sort: world.book.kinds[world.book.places[id].kind].sort,
        }),
        DeclKind::Entity => world
            .entity(written.home, word)
            .ok()
            .map(|id| NativeTarget {
                target: PropTarget::Entity(id),
                kind: world.book.entities[id].kind,
                sort: world.book.kinds[world.book.entities[id].kind].sort,
            }),
        DeclKind::Commodity => world.commodity_of(word).ok().map(|id| NativeTarget {
            target: PropTarget::Commodity(id),
            kind: world.book.commodities[id].kind,
            sort: world.book.kinds[world.book.commodities[id].kind].sort,
        }),
        DeclKind::Asset => world.book.asset(word.text).map(|id| NativeTarget {
            target: PropTarget::Asset(id),
            kind: world.book.assets[id].kind,
            sort: world.book.kinds[world.book.assets[id].kind].sort,
        }),
        DeclKind::Kind => world.kind(written.home, word).ok().map(|id| NativeTarget {
            target: PropTarget::Kind(id),
            kind: id,
            sort: world.book.kinds[id].sort,
        }),
        DeclKind::Purpose => None,
    };
    found
}

fn native_statement_target(
    world: &World<'_>,
    home: Home,
    subject: Subject<'_>,
    name: Sym,
    loc: Loc,
    diags: &mut Vec<Diagnostic>,
) -> Option<NativeTarget> {
    let mut candidates = Vec::new();
    match subject {
        Subject::Unit(unit) => {
            if let Ok(id) = world.commodity_of(Word { text: unit.0, loc }) {
                let kind = world.book.commodities[id].kind;
                candidates.push(NativeTarget {
                    target: PropTarget::Commodity(id),
                    kind,
                    sort: world.book.kinds[kind].sort,
                });
            }
        }
        Subject::Name(subject) => {
            let place_word = Word {
                text: subject.0,
                loc,
            };
            match world
                .book
                .lookup
                .places
                .find(&world.book.names, subject.0, |_| true)
            {
                crate::names::Found::One(id) => {
                    let kind = world.book.places[id].kind;
                    candidates.push(NativeTarget {
                        target: PropTarget::Place(id),
                        kind,
                        sort: world.book.kinds[kind].sort,
                    });
                }
                crate::names::Found::Several(ids) => candidates.extend(ids.into_iter().map(|id| {
                    let kind = world.book.places[id].kind;
                    NativeTarget {
                        target: PropTarget::Place(id),
                        kind,
                        sort: world.book.kinds[kind].sort,
                    }
                })),
                crate::names::Found::Nothing => {}
            }
            match world.book.lookup.entities.find(
                &world.book.names,
                world.scopes.of(home),
                subject.0,
            ) {
                crate::names::Found::One(id) => {
                    let kind = world.book.entities[id].kind;
                    candidates.push(NativeTarget {
                        target: PropTarget::Entity(id),
                        kind,
                        sort: world.book.kinds[kind].sort,
                    });
                }
                crate::names::Found::Several(ids) => candidates.extend(ids.into_iter().map(|id| {
                    let kind = world.book.entities[id].kind;
                    NativeTarget {
                        target: PropTarget::Entity(id),
                        kind,
                        sort: world.book.kinds[kind].sort,
                    }
                })),
                crate::names::Found::Nothing => {}
            }
            if let Some(id) = world.book.asset(subject.0) {
                let kind = world.book.assets[id].kind;
                candidates.push(NativeTarget {
                    target: PropTarget::Asset(id),
                    kind,
                    sort: world.book.kinds[kind].sort,
                });
            }
            if let Ok(id) = world.kind(home, place_word) {
                candidates.push(NativeTarget {
                    target: PropTarget::Kind(id),
                    kind: id,
                    sort: world.book.kinds[id].sort,
                });
            }
        }
        Subject::Code(_) | Subject::Purpose(_) => {}
    }
    let mut matches = candidates
        .iter()
        .copied()
        .filter(|target| has_named(world, target.kind, world.book.name(name)).is_some());
    if let Some(first) = matches.next() {
        if matches.next().is_some() {
            diags.push(
                Diagnostic::error(
                    "ambiguous-property-target",
                    "this property applies to more than one named thing",
                )
                .label(loc, "qualify the target so the intended thing is clear"),
            );
            return None;
        }
        return Some(first);
    }
    (candidates.len() == 1).then(|| candidates[0])
}

fn has_named(world: &World<'_>, kind: Id<Kind>, name: &str) -> Option<Has> {
    let name = world.book.names.get(name)?;
    world.book.kinds[kind]
        .has
        .iter()
        .find(|has| has.name == name)
        .copied()
}

fn is_builtin_line(name: &str) -> bool {
    matches!(
        name,
        "owner"
            | "holds"
            | "select"
            | "opened"
            | "closed"
            | "budget"
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
            | "has"
            | "purpose"
            | "pays"
            | "takes"
            | "sales-tax"
            | "shares"
            | "share"
            | "known-as"
            | "citizen"
            | "books"
            | "of"
            | "currency"
            | "part"
            | "at"
            | "also"
            | "allowance"
    )
}

fn unknown_native_property(world: &World<'_>, target: NativeTarget, line: &Line<'_>) -> Diagnostic {
    let kind_name = world.book.name(world.book.kinds[target.kind].name);
    let noun = match target.target {
        PropTarget::Kind(_) => "kind",
        PropTarget::Entity(_) => "entity",
        PropTarget::Commodity(_) => "commodity",
        PropTarget::Place(_) => "account",
        PropTarget::Asset(_) => "asset",
    };
    let mut valid: Vec<&str> = BUILTINS
        .iter()
        .filter(|builtin| {
            match (
                builtin.1.contains(&Target::Kind),
                target.sort,
                target.target,
            ) {
                (true, _, PropTarget::Kind(_)) => true,
                (false, Sort::Place(_), PropTarget::Place(_)) => builtin.1.contains(&Target::Place),
                (false, Sort::Entity, PropTarget::Entity(_)) => builtin.1.contains(&Target::Entity),
                (false, Sort::Commodity, PropTarget::Commodity(_)) => {
                    builtin.1.contains(&Target::Commodity)
                }
                _ => false,
            }
        })
        .map(|builtin| builtin.0)
        .collect();
    valid.extend(
        world.book.kinds[target.kind]
            .has
            .iter()
            .map(|has| world.book.name(has.name)),
    );
    if matches!(target.target, PropTarget::Asset(_)) {
        valid.extend(["owner", "known-as", "share", "at", "part", "also"]);
    }
    valid.sort_unstable();
    valid.dedup();
    let text = line.name.0;
    let owner = match target.target {
        PropTarget::Kind(_) => format!("kind `{kind_name}`"),
        _ => format!("{} of kind `{kind_name}`", article(noun)),
    };
    let error = Diagnostic::error(
        "unknown-property",
        format!("`{text}` is not a property of {owner}"),
    )
    .label(line.loc, "no such property");
    let error = suggest(error, line.loc, text, valid.iter().copied());
    error.note(format!("its properties are {}", list(&valid)))
}

fn target_key(target: PropTarget) -> (u8, u32) {
    match target {
        PropTarget::Kind(id) => (0, id.index() as u32),
        PropTarget::Entity(id) => (1, id.index() as u32),
        PropTarget::Commodity(id) => (2, id.index() as u32),
        PropTarget::Place(id) => (3, id.index() as u32),
        PropTarget::Asset(id) => (4, id.index() as u32),
    }
}

fn stage_unique(
    world: &mut World<'_>,
    seen: &mut Map<((u8, u32), Sym, Day), Loc>,
    target: PropTarget,
    prop: Prop,
    diags: &mut Vec<Diagnostic>,
) {
    let key = (target_key(target), prop.name, prop.since);
    if let Some(first) = seen.get(&key).copied() {
        let mut diagnostic = Diagnostic::error(
            "duplicate-property-value",
            format!(
                "property `{}` is assigned twice on the same day",
                world.book.name(prop.name)
            ),
        );
        if let Some(loc) = prop.loc {
            diagnostic = diagnostic.label(loc, "assigned again here");
        }
        diags.push(diagnostic.context(first, "first assigned here"));
        return;
    }
    seen.insert(key, prop.loc.unwrap_or(Loc::default()));
    world.set_prop(target, prop);
}

fn stage_changes(
    world: &mut World<'_>,
    mut updates: Vec<PropertyChange>,
    _diags: &mut Vec<Diagnostic>,
) {
    let mut groups: Map<((u8, u32), Sym), Vec<PropertyChange>> = Map::default();
    for update in updates.drain(..) {
        groups
            .entry((target_key(update.target), update.name))
            .or_default()
            .push(update);
    }
    for ((_target_key, name), mut changes) in groups {
        changes.sort_by_key(|change| (change.since, change.order));
        let target = changes[0].target;
        let base = world
            .prop_writes
            .iter()
            .find(|(candidate, prop)| {
                *candidate == target && prop.name == name && prop.since == Day::MIN
            })
            .map(|(_, prop)| prop.value)
            .unwrap_or(Value::Empty);
        for (day, value, loc) in property_timeline(base, &changes) {
            let prop = Prop {
                name,
                value,
                since: day,
                loc: Some(loc),
            };
            world.set_prop(target, prop);
        }
    }
}

/// The effective rows for a sequence of dated overrides. An expired override
/// uncovers the latest still-active override, then the declaration value (or
/// `empty`, which lets an inherited kind default show through).
fn property_timeline(base: Value, changes: &[PropertyChange]) -> Vec<(Day, Value, Loc)> {
    let mut boundaries = Vec::with_capacity(changes.len() * 2);
    for change in changes {
        boundaries.push(change.since);
        if let Some(until) = change.until.filter(|&until| until < Day::MAX) {
            boundaries.push(until.add_days(1));
        }
    }
    boundaries.sort_unstable();
    boundaries.dedup();
    let mut timeline = Vec::with_capacity(boundaries.len());
    let mut visible = base;
    for day in boundaries {
        let winner = changes
            .iter()
            .filter(|change| change.since <= day && change.until.is_none_or(|until| day <= until))
            .fold(None, |best: Option<&PropertyChange>, change| match best {
                None => Some(change),
                Some(old)
                    if change.since > old.since
                        || (change.since == old.since && change.order < old.order) =>
                {
                    Some(change)
                }
                Some(old) => Some(old),
            });
        let value = winner.map_or(base, |change| change.value);
        if value == visible {
            continue;
        }
        let loc = winner.map_or(changes[0].loc, |change| change.loc);
        timeline.push((day, value, loc));
        visible = value;
    }
    timeline
}

#[cfg(test)]
mod native_property_tests {
    use super::*;

    fn day(year: i32, month: u32, date: u32) -> Day {
        Day::from_ymd(year, month, date).unwrap()
    }

    fn update(
        name: Sym,
        since: Day,
        until: Option<Day>,
        value: bool,
        order: usize,
    ) -> PropertyChange {
        PropertyChange {
            target: PropTarget::Kind(Id::new(0)),
            name,
            since,
            until,
            value: Value::Bool(value),
            loc: Loc::default(),
            order,
        }
    }

    #[test]
    fn an_expiring_property_override_uncovers_the_latest_live_value() {
        let mut names = axiom_core::Interner::default();
        let name = names.intern("flag");
        let start = day(2026, 1, 1);
        let short = day(2026, 1, 5);
        let restore_short = day(2026, 1, 7);
        let restore_outer = day(2026, 1, 11);
        let changes = [
            update(name, start, Some(day(2026, 1, 10)), true, 0),
            update(name, short, Some(day(2026, 1, 6)), false, 1),
        ];
        assert_eq!(
            property_timeline(Value::Bool(false), &changes),
            [
                (start, Value::Bool(true), Loc::default()),
                (short, Value::Bool(false), Loc::default()),
                (restore_short, Value::Bool(true), Loc::default()),
                (restore_outer, Value::Bool(false), Loc::default()),
            ]
        );
    }

    #[test]
    fn an_expiring_property_without_an_instance_value_clears_to_inherit() {
        let mut names = axiom_core::Interner::default();
        let name = names.intern("flag");
        let start = day(2026, 3, 1);
        let restore = day(2026, 3, 8);
        let changes = [update(name, start, Some(day(2026, 3, 7)), true, 0)];
        assert_eq!(
            property_timeline(Value::Empty, &changes),
            [
                (start, Value::Bool(true), Loc::default()),
                (restore, Value::Empty, Loc::default()),
            ]
        );
    }
}
