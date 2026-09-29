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

use axiom_core::{Day, Days, Diagnostic, Id, Loc, Map, Ratio, Span, Sym};
use axiom_syntax::{Decl, DeclKind, Expr, ExprId, ExprKind, File, Policy, Prop as Line};

use crate::book::{Amount, Basis, Commodity, Entity, Has, Kind, Place, Prop, Residence, Sort};
use crate::collect::{Entry, Written, decls};
use crate::declare::{MAX_SCALE, World};
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
        !matches!(self, Assign::Restricted | Assign::Deferred | Assign::Claim | Assign::Basis(_) | Assign::Has(_))
    }
}

/// How the arguments of one of the language's properties read.
type Reader = fn(&mut Args<'_, '_, '_>) -> Result<Assign, Diagnostic>;

/// The properties the language defines itself, what each may be written
/// under, and how it reads.
const BUILTINS: [(&str, &[Target], Reader); 18] = [
    ("owner", &[Target::Place], |a| a.entity().map(Assign::Owner)),
    ("holds", &[Target::Place], |a| Ok(Assign::Holds(a.holds()?, a.line.loc))),
    ("select", &[Target::Place, Target::Commodity], |a| a.policy().map(Assign::Select)),
    ("opened", &[Target::Place], |a| Ok(Assign::Opened(a.day()?, a.line.loc))),
    ("closed", &[Target::Place], |a| Ok(Assign::Closed(a.day()?, a.line.loc))),
    ("budget", &[Target::Place], |a| {
        let amount = a.amount()?;
        let monthly = a.word(&["monthly", "yearly"])? == "monthly";
        Ok(Assign::Budget(amount, if monthly { Window::Month } else { Window::Year }, a.line.loc))
    }),
    ("liquidity", &[Target::Place, Target::Commodity], |a| a.span().map(Assign::Liquidity)),
    ("via", &[Target::Entity], |a| a.place().map(Assign::Via)),
    ("lives", &[Target::Entity], |a| a.residence()),
    ("member", &[Target::Entity], |a| Ok(Assign::Member(a.entity()?, a.line.loc))),
    ("precision", &[Target::Commodity], |a| a.count(MAX_SCALE).map(Assign::Precision)),
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
        Ok(Assign::Basis(if a.word(&["zero", "cost"])? == "zero" { Basis::Zero } else { Basis::Cost }))
    }),
    ("claim", &[Target::Kind], |_| Ok(Assign::Claim)),
    ("has", &[Target::Kind], |a| a.has()),
];

/// Words that read a value off a thing in a law (`self.balance`), so a
/// declared property may not take them.
const FIELD_WORDS: [&str; 8] = ["balance", "basis", "owner", "kind", "age", "unit", "year", "month"];

const POLICIES: [(&str, Policy); 4] =
    [("fifo", Policy::Fifo), ("lifo", Policy::Lifo), ("hifo", Policy::Hifo), ("prorata", Policy::Prorata)];

const TYPES: [(&str, Ty); 12] = [
    ("date", Ty::Day),
    ("amount", Ty::Amount),
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
        self.declared.keys().filter(move |key| key.0 == family).map(|key| key.1)
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
            Assign::Lives(residence) => self.lives = self.lives.iter().copied().chain([*residence]).collect(),
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
            return Err(Diagnostic::error("property-argument", format!("`{}` needs {wanted}", self.line.name.0))
                .label(self.line.loc, format!("{wanted} should follow here")));
        };
        self.next += 1;
        Ok(id)
    }

    fn wrong(&self, expr: &Expr, wanted: &str) -> Diagnostic {
        Diagnostic::error("property-type", format!("`{}` needs {wanted}", self.line.name.0))
            .label(expr.loc, format!("this is {}", describe(&expr.kind)))
    }

    /// The next argument, as `pick` reads it out of the expression.
    fn arg<T>(&mut self, wanted: &str, pick: impl FnOnce(&Expr<'s>) -> Option<T>) -> Result<T, Diagnostic> {
        let expr = &self.file.exprs[self.next_id(wanted)?];
        pick(expr).ok_or_else(|| self.wrong(expr, wanted))
    }

    fn done(&self) -> Result<(), Diagnostic> {
        let Some(extra) = self.peek() else { return Ok(()) };
        Err(Diagnostic::error("property-argument", format!("`{}` takes no more arguments here", self.line.name.0))
            .label(extra.loc, "unexpected"))
    }

    /// One of the `allowed` words.
    fn word(&mut self, allowed: &[&str]) -> Result<&'s str, Diagnostic> {
        let wanted = if allowed.len() == 1 { format!("`{}`", allowed[0]) } else { format!("one of {}", list(allowed)) };
        let expr = &self.file.exprs[self.next_id(&wanted)?];
        match expr.kind {
            ExprKind::Name(name) if allowed.contains(&name.0) => Ok(name.0),
            ExprKind::Name(name) => {
                let error = self.wrong(expr, &wanted).label(expr.loc, format!("`{}` is not one of them", name.0));
                Err(suggest(error, expr.loc, name.0, allowed.iter().copied()))
            }
            _ => Err(self.wrong(expr, &wanted)),
        }
    }

    fn name(&mut self, wanted: &str) -> Result<Word<'s>, Diagnostic> {
        let name = |expr: &Expr<'s>| match expr.kind {
            ExprKind::Name(name) => Some(Word { text: name.0, loc: expr.loc }),
            _ => None,
        };
        self.arg(wanted, name)
    }

    fn day(&mut self) -> Result<Day, Diagnostic> {
        self.arg("a date", |expr| if let ExprKind::Date(day) = expr.kind { Some(day) } else { None })
    }

    fn span(&mut self) -> Result<Span, Diagnostic> {
        let span = |expr: &Expr| if let ExprKind::Span(span) = expr.kind { Some(span) } else { None };
        self.arg("a span such as `5d` or `1y6m`", span)
    }

    fn text(&mut self) -> Result<&'s str, Diagnostic> {
        self.arg("text in quotes", |expr| if let ExprKind::Str(text) = expr.kind { Some(text) } else { None })
    }

    /// A whole number from zero to `max`.
    fn count(&mut self, max: u8) -> Result<u8, Diagnostic> {
        self.arg(&format!("a whole number up to {max}"), |expr| match expr.kind {
            ExprKind::Num(number) => {
                number.to_qty(0).ok().and_then(|qty| u8::try_from(qty.0).ok()).filter(|&n| n <= max)
            }
            _ => None,
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
        Ok(POLICIES.iter().find(|policy| policy.0 == word).map_or(Policy::Fifo, |policy| policy.1))
    }

    /// `holds USD, VTI`, or `holds any`.
    fn holds(&mut self) -> Result<Option<Box<[Id<Commodity>]>>, Diagnostic> {
        if matches!(self.peek().map(|expr| &expr.kind), Some(ExprKind::Name(name)) if name.0 == "any") {
            self.word(&["any"])?;
            return Ok(None);
        }
        let mut units = Vec::new();
        while let Some(expr) = self.peek() {
            let ExprKind::Unit(symbol) = expr.kind else {
                return Err(self.wrong(expr, "commodities such as `USD`, or `any`"));
            };
            self.next += 1;
            units.push(self.world.commodity_of(Word { text: symbol.0, loc: expr.loc })?);
        }
        match units.is_empty() {
            true => Err(Diagnostic::error("property-argument", "`holds` needs commodities or `any`")
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
            return Err(Diagnostic::error("residence-order", "this residence ends before it begins")
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
        if BUILTINS.iter().any(|builtin| builtin.0 == name.text) || FIELD_WORDS.contains(&name.text) {
            return Err(Diagnostic::error("reserved-property", format!("`{}` is a built-in property", name.text))
                .label(name.loc, "choose another name")
                .note("built-in properties keep their meaning everywhere, so a kind cannot redefine them"));
        }
        Ok(Assign::Has(Has { name: self.world.book.names.intern(name.text), ty, loc: Some(name.loc) }))
    }

    /// The value of a property a kind declared.
    fn declared(&mut self, has: Has) -> Result<Assign, Diagnostic> {
        let id = self.next_id("a value")?;
        let (value, _) = self.world.constant(self.home, self.file, id, Some(has.ty))?;
        Ok(Assign::Prop(Prop { name: has.name, value, loc: Some(self.line.loc) }))
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
        Lines { home: written.home(), file, lines: &file[written.node.props] }
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
    let builtin = BUILTINS.iter().find(|entry| entry.0 == word && targets.iter().any(|t| entry.1.contains(t)));
    let declared = world.book.names.get(word).and_then(|sym| has.iter().find(|has| has.name == sym)).copied();
    let read = match (builtin, declared) {
        (Some(entry), _) => Ok(entry.2),
        (None, Some(has)) => Err(has),
        (None, None) => return Err(unknown_property(world, at.file.loc(word), targets[0], kind, has, word)),
    };
    let mut args = Args { world, file: at.file, ids: &at.file[line.args], line, home: at.home, next: 0 };
    let assign = match read {
        Ok(read) => read(&mut args)?,
        Err(has) => args.declared(has)?,
    };
    args.done()?;
    Ok(assign)
}

/// `benificiary` is not a property of an account of kind `529`.
fn unknown_property(world: &World, loc: Loc, target: Target, kind: Id<Kind>, has: &[Has], word: &str) -> Diagnostic {
    let kind_name = world.book.name(world.book.kinds[kind].name);
    let mut valid: Vec<&str> = BUILTINS.iter().filter(|entry| entry.1.contains(&target)).map(|entry| entry.0).collect();
    valid.extend(has.iter().map(|has| world.book.name(has.name)));
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
    let own = at.lines.iter().filter(|line| line.name.0 != "has" || !targets.contains(&Target::Kind));
    let read = own.map(|line| read_line(world, at, line, targets, has, kind));
    read.filter_map(|read| read.map_err(|error| diags.push(error)).ok()).collect()
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

pub(crate) fn apply<'s>(world: &mut World<'s>, entries: &[Entry<'_, 's>], diags: &mut Vec<Diagnostic>) -> Vec<Budget> {
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
) -> Vec<Vec<Assign>> {
    let ids: Vec<Id<Kind>> =
        world.book.kinds.ids().filter(|&id| Target::of(world.book.kinds[id].sort) == target).collect();
    let written = first_of(decls(entries, DeclKind::Kind), world.declared.kinds.iter().map(|&id| Some(id)));
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
        let inherited =
            world.book.kinds.parent(id).map_or(Box::default(), |parent| world.book.kinds[parent].has.clone());
        let sort = world.book.kinds[id].sort;
        world.declare_props(sort, &own, diags);
        let unshadowed = inherited.iter().filter(|theirs| !own.iter().any(|mine| mine.name == theirs.name));
        world.book.kinds[id].has = own.iter().chain(unshadowed).copied().collect();
    }
    let mut defaults: Vec<Vec<Assign>> = vec![Vec::new(); world.book.kinds.len()];
    for &id in &ids {
        if let Some(parent) = world.book.kinds.parent(id) {
            let above = world.book.kinds[parent].clone();
            world.book.kinds[id].inherit(&above);
            defaults[id.index()] = defaults[parent.index()].clone();
        }
        let Some(w) = written.get(&id) else { continue };
        let has = world.book.kinds[id].has.clone();
        for assign in read_lines(world, &Lines::of(w), &[Target::Kind, target], &has, id, diags) {
            world.book.kinds[id].set(&assign);
            if assign.is_default() {
                defaults[id.index()].push(assign);
            }
        }
    }
    defaults
}

/// The settings of one thing: what its kind hands down, then its own lines.
fn settings<'a, 's>(
    world: &mut World<'s>,
    (kind, target): (Id<Kind>, Target),
    written: Option<&Written<'a, 's, Decl<'s>>>,
    defaults: &[Vec<Assign>],
    diags: &mut Vec<Diagnostic>,
) -> Vec<Assign> {
    let has = world.book.kinds[kind].has.clone();
    let own = written.map_or_else(Vec::new, |w| read_lines(world, &Lines::of(w), &[target], &has, kind, diags));
    defaults[kind.index()].iter().cloned().chain(own).collect()
}

fn commodities<'s>(
    world: &mut World<'s>,
    entries: &[Entry<'_, 's>],
    defaults: &[Vec<Assign>],
    diags: &mut Vec<Diagnostic>,
) {
    let written = first_of(decls(entries, DeclKind::Commodity), world.declared.commodities.iter().map(|&id| Some(id)));
    for id in world.book.commodities.ids().collect::<Vec<_>>() {
        let kind = world.book.commodities[id].kind;
        let assigns = settings(world, (kind, Target::Commodity), written.get(&id), defaults, diags);
        assigns.iter().for_each(|assign| world.book.commodities[id].set(assign));
    }
}

fn entities<'s>(
    world: &mut World<'s>,
    entries: &[Entry<'_, 's>],
    defaults: &[Vec<Assign>],
    diags: &mut Vec<Diagnostic>,
) {
    let written = first_of(decls(entries, DeclKind::Entity), world.declared.entities.iter().map(|&id| Some(id)));
    for id in world.book.entities.ids().collect::<Vec<_>>() {
        let kind = world.book.entities[id].kind;
        let assigns = settings(world, (kind, Target::Entity), written.get(&id), defaults, diags);
        let restricted = world.book.kinds[kind].restricted;
        let entity = &mut world.book.entities[id];
        entity.restricted = restricted;
        for assign in &assigns {
            match assign {
                Assign::Member(member, loc) if *member == id => diags.push(
                    Diagnostic::error("member-self", "an entity cannot be a member of itself")
                        .label(*loc, "name the household this person belongs to"),
                ),
                _ => entity.set(assign),
            }
        }
        let mut lives = std::mem::take(&mut entity.lives).into_vec();
        lives.sort_by_key(|residence| residence.days);
        entity.lives = lives.into();
    }
}

fn places<'s>(
    world: &mut World<'s>,
    entries: &[Entry<'_, 's>],
    defaults: &[Vec<Assign>],
    budgets: &mut Vec<Budget>,
    diags: &mut Vec<Diagnostic>,
) {
    let written = first_of(decls(entries, DeclKind::Account), world.declared.places.iter().copied());
    for id in world.book.places.ids().collect::<Vec<_>>() {
        let kind = world.book.places[id].kind;
        let assigns = settings(world, (kind, Target::Place), written.get(&id), defaults, diags);
        let facts = &world.book.kinds[kind];
        // Money in a `deferred` place is untaxed until it leaves, so unless its kind
        // says otherwise, none of it counts as already accounted for.
        let default = if facts.deferred { Basis::Zero } else { Basis::Cost };
        let (deferred, basis, claim) = (facts.deferred, facts.basis.unwrap_or(default), facts.claim);
        let place = &mut world.book.places[id];
        (place.deferred, place.basis, place.claim) = (deferred, basis, claim);
        for assign in &assigns {
            match *assign {
                Assign::Budget(amount, window, loc) => budgets.push(Budget { place: id, amount, window, loc }),
                Assign::Holds(_, loc) => drop(world.lines.insert((id, "holds"), loc)),
                Assign::Opened(_, loc) => drop(world.lines.insert((id, "opened"), loc)),
                Assign::Closed(_, loc) => drop(world.lines.insert((id, "closed"), loc)),
                _ => {}
            }
            place.set(assign);
        }
        if let (Some(opened), Some(closed), Some(loc)) = (place.opened, place.closed, place.loc)
            && closed < opened
        {
            diags.push(
                Diagnostic::error("closed-before-opened", "this account closes before it opens")
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
