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

use axiom_core::{Day, Days, Diagnostic, Id, Loc, Map, Ratio, SlotId, Span, Sym, Tree};
use axiom_syntax::{
    Change, ClauseKind, Decl, DeclKind, Expr, ExprId, ExprKind, File, Policy, Prop as Line, Rates, Setting, Statement,
    Subject, Verb,
};

use crate::book::{
    Asset, At, Basis, Books, Commodity, Entity, Kind, Place, Prop, Purpose, RatePolicy, Residence, Role, Share, Sort,
    Take,
};
use crate::collect::{Collected, Written};
use crate::declare::{MAX_SCALE, PropTarget, World};
use crate::errors::{Reported, Word, article, list, suggest};
use crate::fill::{self, Filled};
use crate::law::Value;
use crate::problem;
use crate::scope::Home;
use crate::slots::{Range, Slot};
use crate::values::describe;

/// What a property line describes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Target {
    Place,
    Entity,
    Commodity,
    Asset,
    Kind,
}

impl Target {
    fn noun(self) -> &'static str {
        match self {
            Target::Place => "account",
            Target::Entity => "entity",
            Target::Commodity => "commodity",
            Target::Asset => "asset",
            Target::Kind => "kind",
        }
    }

    /// What the instances of a kind of this sort are.
    fn of(sort: Sort) -> Target {
        match sort {
            Sort::Place(_) => Target::Place,
            Sort::Thing => Target::Asset,
            Sort::Entity => Target::Entity,
            Sort::Commodity => Target::Commodity,
        }
    }
}

/// One property line, read: a setting for whatever it is applied to.
#[derive(Clone, Debug)]
enum Assign {
    Holds(Option<Box<[Id<Commodity>]>>),
    Select(Policy),
    Opened(Day),
    Closed(Day),
    Liquidity(Span),
    Via(Id<Place>),
    Lives(Residence),
    Member(Id<Entity>),
    Currency(Id<Commodity>),
    Citizen(Box<[Id<crate::book::System>]>),
    Books(Books),
    Purpose(At<Id<Purpose>>),
    Pays(At<Id<Purpose>>),
    Takes(At<Take>),
    SalesTax(Ratio),
    Share(Share),
    PartOf(At<Id<Asset>>),
    Precision(u8),
    Title(Sym),
    Grows(Ratio),
    Restricted,
    Deferred,
    Claim,
    Basis(Basis),
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
                | Assign::Purpose(_)
                | Assign::Pays(_)
                | Assign::Takes(_)
                | Assign::SalesTax(_)
                | Assign::Share(_)
                | Assign::PartOf(_)
        )
    }
}

/// How the arguments of one of the language's properties read.
type Reader = fn(&mut Args<'_, '_, '_>) -> Result<Assign, Diagnostic>;

/// The properties the language defines itself, what each may be written
/// under, and how it reads.
const BUILTINS: [(&str, &[Target], Reader); 24] = [
    ("holds", &[Target::Place], |a| Ok(Assign::Holds(a.holds()?))),
    ("select", &[Target::Place, Target::Commodity, Target::Asset], |a| a.policy().map(Assign::Select)),
    ("opened", &[Target::Place], |a| Ok(Assign::Opened(a.day()?))),
    ("closed", &[Target::Place], |a| Ok(Assign::Closed(a.day()?))),
    ("liquidity", &[Target::Place, Target::Commodity, Target::Asset], |a| a.span().map(Assign::Liquidity)),
    ("via", &[Target::Entity], |a| a.place().map(Assign::Via)),
    ("lives", &[Target::Entity], |a| a.residence()),
    ("member", &[Target::Entity], |a| Ok(Assign::Member(a.entity()?))),
    ("currency", &[Target::Entity], |a| a.currency().map(Assign::Currency)),
    ("citizen", &[Target::Entity], |a| a.citizens().map(Assign::Citizen)),
    ("books", &[Target::Entity], |a| a.books().map(Assign::Books)),
    ("purpose", &[Target::Kind, Target::Entity], |a| a.purpose().map(Assign::Purpose)),
    ("pays", &[Target::Kind], |a| a.purpose().map(Assign::Pays)),
    ("takes", &[Target::Kind], |a| a.takes().map(Assign::Takes)),
    ("sales-tax", &[Target::Kind], |a| a.percent().map(Assign::SalesTax)),
    ("share", &[Target::Kind], |a| a.share().map(Assign::Share)),
    ("part", &[Target::Asset], |a| a.part_of().map(Assign::PartOf)),
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
];

const POLICIES: [(&str, Policy); 4] =
    [("fifo", Policy::Fifo), ("lifo", Policy::Lifo), ("hifo", Policy::Hifo), ("prorata", Policy::Prorata)];

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
        self.purpose = self.purpose.or(above.purpose);
        self.pays = self.pays.or(above.pays);
        self.sales_tax = self.sales_tax.or(above.sales_tax);
        self.takes = merge_takes(&above.takes, &self.takes);
        self.shares = merge_shares(&above.shares, &self.shares);
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
            Assign::Purpose(purpose) => self.purpose = Some(*purpose),
            Assign::Pays(pays) => self.pays = Some(*pays),
            Assign::Takes(take) => self.takes = merge_takes(&self.takes, std::slice::from_ref(take)),
            Assign::SalesTax(rate) => self.sales_tax = Some(*rate),
            Assign::Share(share) => self.shares = merge_shares(&self.shares, std::slice::from_ref(share)),
            Assign::Prop(prop) => put(&mut self.props, *prop),
            _ => {}
        }
    }
}

fn merge_takes(inherited: &[At<Take>], own: &[At<Take>]) -> Box<[At<Take>]> {
    let mut merged = inherited.to_vec();
    for take in own {
        match merged.iter_mut().find(|held| held.value.from == take.value.from) {
            Some(held) => *held = *take,
            None => merged.push(*take),
        }
    }
    merged.into_boxed_slice()
}

fn merge_shares(inherited: &[Share], own: &[Share]) -> Box<[Share]> {
    let mut merged = inherited.to_vec();
    for share in own {
        match merged.iter_mut().find(|held| held.entity == share.entity) {
            Some(held) => *held = *share,
            None => merged.push(*share),
        }
    }
    merged.into_boxed_slice()
}

/// Defaults written by each kind itself. Ancestors are walked when an instance
/// is built, so a deep kind does not copy every ancestor's assignments.
struct Defaults {
    by_kind: Vec<Vec<Assign>>,
}

impl Defaults {
    fn empty(kinds: usize) -> Defaults {
        Defaults { by_kind: vec![Vec::new(); kinds] }
    }

    /// Applies defaults from the oldest ancestor through `kind`, reusing one
    /// caller-owned path buffer across every instance of the same target.
    fn apply(&self, kinds: &Tree<Kind>, kind: Id<Kind>, path: &mut Vec<Id<Kind>>, mut apply: impl FnMut(&Assign)) {
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
            Assign::Purpose(purpose) => self.purpose = Some(*purpose),
            Assign::Via(place) => self.place = Some(*place),
            Assign::Lives(residence) => self.lives = self.lives.iter().copied().chain([*residence]).collect(),
            Assign::Member(entity) => self.member = Some(*entity),
            Assign::Currency(currency) => self.currency = *currency,
            Assign::Citizen(citizen) => self.citizen = citizen.clone(),
            Assign::Books(books) => self.books = *books,
            Assign::Prop(prop) => put(&mut self.props, *prop),
            _ => {}
        }
    }
}

impl Place {
    fn set(&mut self, assign: &Assign) {
        match assign {
            Assign::Holds(holds) => self.holds = holds.clone(),
            Assign::Select(policy) => self.select = Some(*policy),
            Assign::Opened(day) => self.opened = Some(*day),
            Assign::Closed(day) => self.closed = Some(*day),
            Assign::Liquidity(span) => self.liquidity = Some(*span),
            Assign::Prop(prop) => put(&mut self.props, *prop),
            _ => {}
        }
    }
}

impl Asset {
    fn set(&mut self, assign: &Assign) {
        if let Assign::PartOf(parent) = assign {
            self.part_of = Some(*parent);
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
        let Some(extra) = self.peek() else {
            return Ok(());
        };
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
        let span = |expr: &Expr| {
            if let ExprKind::Span(span) = expr.kind { Some(span) } else { None }
        };
        self.arg("a span such as `5d` or `1y6m`", span)
    }

    fn text(&mut self) -> Result<&'s str, Diagnostic> {
        self.arg("text in quotes", |expr| if let ExprKind::Str(text) = expr.kind { Some(text.0) } else { None })
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

    fn currency(&mut self) -> Result<Id<Commodity>, Diagnostic> {
        let word = self.name("a commodity")?;
        self.world.commodity_of(word)
    }

    fn citizens(&mut self) -> Result<Box<[Id<crate::book::System>]>, Diagnostic> {
        let mut systems = Vec::new();
        while self.peek().is_some() {
            let word = self.name("a system")?;
            systems.push(self.world.system(word)?);
        }
        if systems.is_empty() {
            Err(Diagnostic::error("property-argument", "`citizen` needs a system")
                .label(self.line.loc, "name a system here"))
        } else {
            Ok(systems.into_boxed_slice())
        }
    }

    fn books(&mut self) -> Result<Books, Diagnostic> {
        Ok(match self.word(&["cash", "accrual"])? {
            "cash" => Books::Cash,
            _ => Books::Accrual,
        })
    }

    fn purpose(&mut self) -> Result<At<Id<Purpose>>, Diagnostic> {
        let word = self.name("a purpose")?;
        Ok(At { value: self.world.purpose(self.home, word)?, loc: word.loc })
    }

    fn takes(&mut self) -> Result<At<Take>, Diagnostic> {
        let to = self.purpose()?;
        self.word(&["from"])?;
        let from = self.purpose()?;
        Ok(At { value: Take { to: to.value, from: from.value }, loc: self.line.loc })
    }

    fn share(&mut self) -> Result<Share, Diagnostic> {
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
        let word = self.name("an entity")?;
        let entity = self.world.entity(self.home, word)?;
        Ok(Share { rate, entity, measure: None, loc: self.line.loc })
    }

    fn part_of(&mut self) -> Result<At<Id<Asset>>, Diagnostic> {
        self.word(&["of"])?;
        let word = self.name("an asset")?;
        let Some(asset) = self.world.book.asset(word.text) else {
            return Err(self.world.missing_asset(word));
        };
        Ok(At { value: asset, loc: word.loc })
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

/// The setting a line makes: one of the language's for something it is
/// written under (`targets`), or one a kind declared (`has`).
fn read_line<'s>(
    world: &mut World<'s>,
    at: &Lines<'_, 's>,
    line: &Line<'s>,
    targets: &[Target],
    kind: Id<Kind>,
) -> Result<Assign, Diagnostic> {
    let word = line.name.0;
    let Some(builtin) = BUILTINS.iter().find(|entry| entry.0 == word && targets.iter().any(|t| entry.1.contains(t)))
    else {
        return Err(unknown_property(world, at.file.loc(word), targets[0], kind, word));
    };
    let mut args = Args { world, file: at.file, ids: &at.file[line.args], line, home: at.home, next: 0 };
    let assign = (builtin.2)(&mut args)?;
    args.done()?;
    Ok(assign)
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
pub(crate) fn declare<'a, 's>(world: &mut World<'s>, collected: &Collected<'a, 's>, diags: &mut Vec<Diagnostic>) {
    native_builtins(world, collected, diags);
    let mut seen: Map<((u8, u32), Sym, Day), Loc> = Map::default();
    let mut filled = Filled::default();
    stage_declared_values(world, collected, &mut seen, &mut filled, diags);
    let changes = property_changes(world, collected, &mut seen, diags);
    stage_changes(world, changes);
    missing_roles(world, collected, &filled, diags);
}

/// The values the property lines of declarations give, from the beginning of time.
fn stage_declared_values<'a, 's>(
    world: &mut World<'s>,
    collected: &Collected<'a, 's>,
    seen: &mut Map<((u8, u32), Sym, Day), Loc>,
    filled: &mut Filled,
    diags: &mut Vec<Diagnostic>,
) {
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
                let key = target_key(target.target);
                filled.extend(near_slot(world, target.kind, line.name.0).map(|number| (key.0, key.1, number)));
                continue;
            };
            // A line that is wrong is still an attempt to fill the slot: it is said, and the slot not again.
            let key = target_key(target.target);
            filled.insert((key.0, key.1, has.number));
            let at = fill::At { home: written.home(), file: written.file(), line };
            let Some(filling) = fill::fill(world, &at, &has.slot).or_report(diags) else {
                continue;
            };
            // A slot that holds several is stored once the facts are (the next change); one value is a row.
            if let (true, [value]) = (fill::holds_one(has.slot.mult), &filling.values[..]) {
                let prop = Prop { name: has.name, value: *value, since: Day::MIN, loc: Some(line.loc) };
                stage_unique(world, seen, target.target, prop, diags);
            }
        }
    }
}

/// The changes `now PROPERTY VALUE` statements make, in the order written.
fn property_changes<'a, 's>(
    world: &mut World<'s>,
    collected: &Collected<'a, 's>,
    seen: &mut Map<((u8, u32), Sym, Day), Loc>,
    diags: &mut Vec<Diagnostic>,
) -> Vec<PropertyChange> {
    let mut changes = Vec::new();
    for written in &collected.statements {
        let Verb::Now(Change::Property(line)) = &written.node.verb else {
            continue;
        };
        if is_builtin_line(line.name.0) {
            continue;
        }
        if let Some(change) = property_change(world, written, line, changes.len(), seen, diags) {
            changes.push(change);
        }
    }
    changes
}

/// One statement's change of a property a kind declared, or nothing after what is wrong with it is said.
fn property_change<'a, 's>(
    world: &mut World<'s>,
    written: &Written<'a, 's, Statement<'s>>,
    line: &Line<'s>,
    order: usize,
    seen: &mut Map<((u8, u32), Sym, Day), Loc>,
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
    let [value] = filling.values[..] else { return None };
    let until = file[statement.tail].iter().find_map(|clause| match clause.kind {
        ClauseKind::Until(day) => Some(day),
        _ => None,
    });
    if until.is_some_and(|day| day < statement.date) {
        diags.push(
            Diagnostic::error("property-until-order", "this property change ends before it begins")
                .label(line.loc, "`until` is earlier than the change")
                .help("move the end date to the change date or later"),
        );
        return None;
    }
    let key = (target_key(target.target), has.name, statement.date);
    if let Some(first) = seen.get(&key).copied() {
        diags.push(problem::twice("property change", line.loc, first));
        return None;
    }
    seen.insert(key, line.loc);
    Some(PropertyChange {
        target: target.target,
        name: has.name,
        since: statement.date,
        until,
        value,
        loc: line.loc,
        order,
    })
}

/// Reads built-in kind defaults and applies them oldest-ancestor first to the
/// native entities, commodities and places. Custom property values are read
/// separately below and remain stored once on their declaring kind.
/// The first declaration written for each thing that has properties.
struct WrittenBy<'a, 's> {
    kinds: Map<Id<Kind>, Written<'a, 's, Decl<'s>>>,
    entities: Map<Id<Entity>, Written<'a, 's, Decl<'s>>>,
    commodities: Map<Id<Commodity>, Written<'a, 's, Decl<'s>>>,
    places: Map<Id<Place>, Written<'a, 's, Decl<'s>>>,
    assets: Map<Id<Asset>, Written<'a, 's, Decl<'s>>>,
}

fn written_by<'a, 's>(world: &World<'s>, collected: &Collected<'a, 's>) -> WrittenBy<'a, 's> {
    let mut by = WrittenBy {
        kinds: Map::default(),
        entities: Map::default(),
        commodities: Map::default(),
        places: Map::default(),
        assets: Map::default(),
    };
    for &written in &collected.decls {
        let Some(target) = native_target(world, written) else {
            continue;
        };
        match target.target {
            PropTarget::Kind(id) => by.kinds.entry(id).or_insert(written),
            PropTarget::Entity(id) => by.entities.entry(id).or_insert(written),
            PropTarget::Commodity(id) => by.commodities.entry(id).or_insert(written),
            PropTarget::Place(id) => by.places.entry(id).or_insert(written),
            PropTarget::Asset(id) => by.assets.entry(id).or_insert(written),
        };
    }
    by
}

/// The built-in properties of everything declared, inherited down the kind tree: a kind's own lines first, then
/// each commodity, entity, place and asset has its kind's defaults and then its own.
fn native_builtins<'a, 's>(world: &mut World<'s>, collected: &Collected<'a, 's>, diags: &mut Vec<Diagnostic>) {
    let written = written_by(world, collected);
    for target in [Target::Commodity, Target::Entity, Target::Place, Target::Asset] {
        let defaults = kind_defaults(world, target, &written.kinds, diags);
        match target {
            Target::Commodity => {
                builtin_commodities(world, &written.commodities, &defaults, diags);
                native_system_currencies(world, collected, diags);
            }
            Target::Entity => builtin_entities(world, &written.entities, &defaults, diags),
            Target::Place => builtin_places(world, &written.places, &defaults, diags),
            Target::Asset => builtin_assets(world, &written.assets, &defaults, diags),
            Target::Kind => unreachable!(),
        }
    }
}

/// What each kind of a sort sets: its parent's settings inherited, then its own lines, and the settings that
/// instances inherit gathered for them.
fn kind_defaults<'a, 's>(
    world: &mut World<'s>,
    target: Target,
    written: &Map<Id<Kind>, Written<'a, 's, Decl<'s>>>,
    diags: &mut Vec<Diagnostic>,
) -> Defaults {
    let kind_ids: Vec<Id<Kind>> =
        world.book.kinds.ids().filter(|&id| Target::of(world.book.kinds[id].sort) == target).collect();
    let mut defaults = Defaults::empty(world.book.kinds.len());
    for kind in kind_ids {
        if world.book.kinds.parent(kind).is_some() {
            let (above, child) = world.book.kinds.with_parent_mut(kind).expect("kind parent exists");
            child.inherit(above);
        }
        if let Some(written) = written.get(&kind).copied() {
            let at = Lines::from_native(written);
            let assigns = read_builtin_lines(world, &at, &[Target::Kind, target], kind, diags);
            for assign in &assigns {
                world.book.kinds[kind].set(assign);
                if assign.is_default() {
                    defaults.by_kind[kind.index()].push(assign.clone());
                }
            }
        }
    }
    defaults
}

/// The lines a declaration writes for the built-in properties of a thing of `kind`, read.
fn own_builtins<'a, 's>(
    world: &mut World<'s>,
    written: Option<Written<'a, 's, Decl<'s>>>,
    target: Target,
    kind: Id<Kind>,
    diags: &mut Vec<Diagnostic>,
) -> Vec<Assign> {
    let Some(written) = written else {
        return Vec::new();
    };
    let at = Lines::from_native(written);
    read_builtin_lines(world, &at, &[target], kind, diags)
}

fn builtin_commodities<'a, 's>(
    world: &mut World<'s>,
    written: &Map<Id<Commodity>, Written<'a, 's, Decl<'s>>>,
    defaults: &Defaults,
    diags: &mut Vec<Diagnostic>,
) {
    let ids: Vec<_> = world.book.commodities.ids().collect();
    let mut path = Vec::new();
    for id in ids {
        let kind = world.book.commodities[id].kind;
        let own = own_builtins(world, written.get(&id).copied(), Target::Commodity, kind, diags);
        let (kinds, commodities) = (&world.book.kinds, &mut world.book.commodities);
        defaults.apply(kinds, kind, &mut path, |assign| commodities[id].set(assign));
        for assign in &own {
            commodities[id].set(assign);
        }
    }
}

fn builtin_entities<'a, 's>(
    world: &mut World<'s>,
    written: &Map<Id<Entity>, Written<'a, 's, Decl<'s>>>,
    defaults: &Defaults,
    diags: &mut Vec<Diagnostic>,
) {
    let ids: Vec<_> = world.book.entities.ids().collect();
    let mut path = Vec::new();
    let mut currency_set = vec![false; world.book.entities.len()];
    for id in ids {
        let kind = world.book.entities[id].kind;
        let own = own_builtins(world, written.get(&id).copied(), Target::Entity, kind, diags);
        let (kinds, entities) = (&world.book.kinds, &mut world.book.entities);
        let entity = &mut entities[id];
        defaults.apply(kinds, kind, &mut path, |assign| {
            if matches!(assign, Assign::Currency(_)) {
                currency_set[id.index()] = true;
            }
            entity.set(assign);
        });
        for assign in &own {
            if matches!(assign, Assign::Currency(_)) {
                currency_set[id.index()] = true;
            }
            entity.set(assign);
        }
        entity.purpose = entity.purpose.or(kinds[kind].purpose);
        entity.restricted = kinds[kind].restricted;
        let mut lives = entity.lives.to_vec();
        lives.sort_by_key(|residence| residence.days.first());
        entity.lives = lives.into_boxed_slice();
    }
    // An entity that sets no currency counts in that of the first system it lives under, else the book's.
    for id in world.book.entities.ids().collect::<Vec<_>>() {
        if !currency_set[id.index()] {
            let residence_currency = world.book.entities[id]
                .lives
                .iter()
                .find_map(|residence| world.book.systems[residence.system].currency);
            world.book.entities[id].currency = residence_currency.unwrap_or(world.book.base);
        }
    }
}

fn builtin_places<'a, 's>(
    world: &mut World<'s>,
    written: &Map<Id<Place>, Written<'a, 's, Decl<'s>>>,
    defaults: &Defaults,
    diags: &mut Vec<Diagnostic>,
) {
    let ids: Vec<_> = world.book.places.ids().collect();
    let mut path = Vec::new();
    for id in ids {
        let kind = world.book.places[id].kind;
        let own = own_builtins(world, written.get(&id).copied(), Target::Place, kind, diags);
        let (kinds, places) = (&world.book.kinds, &mut world.book.places);
        defaults.apply(kinds, kind, &mut path, |assign| places[id].set(assign));
        for assign in &own {
            places[id].set(assign);
        }
        inherit_place_traits(&mut places[id], &kinds[kind]);
    }
}

fn builtin_assets<'a, 's>(
    world: &mut World<'s>,
    written: &Map<Id<Asset>, Written<'a, 's, Decl<'s>>>,
    defaults: &Defaults,
    diags: &mut Vec<Diagnostic>,
) {
    let ids: Vec<_> = world.book.assets.ids().collect();
    let mut path = Vec::new();
    for id in ids {
        let kind = world.book.assets[id].kind;
        let own = own_builtins(world, written.get(&id).copied(), Target::Asset, kind, diags);
        let (kinds, assets, places) = (&world.book.kinds, &mut world.book.assets, &mut world.book.places);
        let asset = &mut assets[id];
        let place = &mut places[asset.place];
        // `part of` is the asset's; every other setting is its place's.
        defaults.apply(kinds, kind, &mut path, |assign| match assign {
            Assign::PartOf(_) => asset.set(assign),
            _ => place.set(assign),
        });
        for assign in &own {
            match assign {
                Assign::PartOf(_) => asset.set(assign),
                _ => place.set(assign),
            }
        }
        inherit_place_traits(place, &kinds[kind]);
    }
    diagnose_asset_cycles(&mut world.book.assets, &world.book.names, diags);
}

fn inherit_place_traits(place: &mut Place, kind: &Kind) {
    place.deferred = kind.deferred;
    place.basis = kind.basis.unwrap_or(if kind.deferred { Basis::Zero } else { Basis::Cost });
    // Claim tabs keep separate parcels even though their synthetic root kinds
    // do not declare the `claim` trait themselves.
    place.claim = kind.claim || matches!(place.role, Role::Tab(_));
}

fn read_builtin_lines<'s>(
    world: &mut World<'s>,
    at: &Lines<'_, 's>,
    targets: &[Target],
    kind: Id<Kind>,
    diags: &mut Vec<Diagnostic>,
) -> Vec<Assign> {
    let mut assigns = Vec::new();
    for line in at.lines {
        if line.name.0 == "owner" {
            if targets.len() != 1 || !matches!(targets[0], Target::Entity | Target::Place | Target::Asset) {
                diags.push(
                    Diagnostic::error("unknown-property", "`owner` is not a property of this kind")
                        .label(line.loc, "owner is set on an entity, account, or asset"),
                );
            }
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
        match read_line(world, at, line, targets, kind) {
            Ok(assign) => assigns.push(assign),
            Err(problem) => diags.push(problem),
        }
    }
    assigns
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
    let mut seen: Map<Id<crate::book::System>, Loc> = Map::default();
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

/// Completes each system's exchange-rate policy after params have been
/// declared, so `rates param NAME` resolves with the system's visibility.
pub(crate) fn system_rates<'s>(world: &mut World<'s>, collected: &Collected<'_, 's>, diags: &mut Vec<Diagnostic>) {
    let mut seen: Map<Id<crate::book::System>, Loc> = Map::default();
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
    fn of(world: &World<'_>, target: PropTarget) -> NativeTarget {
        let kind = match target {
            PropTarget::Place(id) => world.book.places[id].kind,
            PropTarget::Entity(id) => world.book.entities[id].kind,
            PropTarget::Commodity(id) => world.book.commodities[id].kind,
            PropTarget::Asset(id) => world.book.assets[id].kind,
            PropTarget::Kind(id) => id,
        };
        NativeTarget { target, kind, sort: world.book.kinds[kind].sort }
    }
}

fn native_target(world: &World<'_>, written: Written<'_, '_, Decl<'_>>) -> Option<NativeTarget> {
    let word = Word { text: written.node.name.0, loc: written.item.loc };
    let target = match written.node.what {
        DeclKind::Account => PropTarget::Place(world.place(word).ok()?),
        DeclKind::Entity => PropTarget::Entity(world.entity(written.home(), word).ok()?),
        DeclKind::Commodity => PropTarget::Commodity(world.commodity_of(word).ok()?),
        DeclKind::Asset => PropTarget::Asset(world.book.asset(word.text)?),
        DeclKind::Kind => PropTarget::Kind(world.kind(written.home(), word).ok()?),
        DeclKind::Purpose => return None,
    };
    Some(NativeTarget::of(world, target))
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
            targets.extend(world.commodity_of(Word { text: unit.0, loc }).ok().map(PropTarget::Commodity));
        }
        Subject::Name(subject) => {
            let names = &world.book.names;
            targets.extend(
                world.book.lookup.places.find(names, subject.0, |_| true).into_ids().into_iter().map(PropTarget::Place),
            );
            let entities = world.book.lookup.entities.find(names, world.scopes.of(home), subject.0);
            targets.extend(entities.into_ids().into_iter().map(PropTarget::Entity));
            targets.extend(world.book.asset(subject.0).map(PropTarget::Asset));
            targets.extend(world.kind(home, Word { text: subject.0, loc }).ok().map(PropTarget::Kind));
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
    let noun = match target.target {
        PropTarget::Kind(_) => "kind",
        PropTarget::Entity(_) => "entity",
        PropTarget::Commodity(_) => "commodity",
        PropTarget::Place(_) => "account",
        PropTarget::Asset(_) => "asset",
    };
    let mut valid: Vec<&str> = BUILTINS
        .iter()
        .filter(|builtin| match (builtin.1.contains(&Target::Kind), target.sort, target.target) {
            (true, _, PropTarget::Kind(_)) => true,
            (false, Sort::Place(_), PropTarget::Place(_)) => builtin.1.contains(&Target::Place),
            (false, Sort::Entity, PropTarget::Entity(_)) => builtin.1.contains(&Target::Entity),
            (false, Sort::Commodity, PropTarget::Commodity(_)) => builtin.1.contains(&Target::Commodity),
            _ => false,
        })
        .map(|builtin| builtin.0)
        .collect();
    valid.extend(declared_names(world, target.kind));
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
    let error = Diagnostic::error("unknown-property", format!("`{text}` is not a property of {owner}"))
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
    let line = prop.loc.unwrap_or_default();
    if let Some(first) = seen.get(&key).copied() {
        diags.push(problem::filled_twice(world.book.name(prop.name), line, first));
        return;
    }
    seen.insert(key, line);
    world.set_prop(target, prop);
}

fn stage_changes(world: &mut World<'_>, mut updates: Vec<PropertyChange>) {
    let mut groups: Map<((u8, u32), Sym), Vec<PropertyChange>> = Map::default();
    for update in updates.drain(..) {
        groups.entry((target_key(update.target), update.name)).or_default().push(update);
    }
    for ((_target_key, name), mut changes) in groups {
        changes.sort_by_key(|change| (change.since, change.order));
        let target = changes[0].target;
        let base = world
            .prop_writes
            .iter()
            .find(|(candidate, prop)| *candidate == target && prop.name == name && prop.since == Day::MIN)
            .map(|(_, prop)| prop.value)
            .unwrap_or(Value::Empty);
        for (day, value, loc) in property_timeline(base, &changes) {
            let prop = Prop { name, value, since: day, loc: Some(loc) };
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
                Some(old) if change.since > old.since || (change.since == old.since && change.order < old.order) => {
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
        let key = target_key(target.target);
        for slot in book.schema.effective(&book.kinds, target.kind).filter(|slot| fill::is_required(slot.mult)) {
            let number = book.schema.number(slot.name).expect("a declared slot is numbered");
            let by_kind = |above: Id<Kind>| {
                let kind = target_key(PropTarget::Kind(above));
                filled.contains(&(kind.0, kind.1, number))
            };
            if filled.contains(&(key.0, key.1, number)) || book.kinds.lineage(target.kind).any(by_kind) {
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

    fn day(year: i32, month: u32, date: u32) -> Day {
        Day::from_ymd(year, month, date).unwrap()
    }

    fn place(role: Role, name: Sym) -> Place {
        Place {
            path: name,
            class: crate::book::Class::Asset,
            role,
            kind: Id::new(0),
            owner: Id::new(0),
            holds: None,
            select: None,
            deferred: false,
            basis: Basis::Cost,
            claim: false,
            liquidity: None,
            opened: None,
            closed: None,
            shares: Box::default(),
            known_as: Box::default(),
            props: Box::default(),
            doc: None,
            loc: None,
        }
    }

    #[test]
    fn synthetic_claim_tabs_keep_their_claim_trait_without_a_kind_default() {
        let mut names = axiom_core::Interner::default();
        let empty = names.intern("");
        let kind = Kind {
            name: empty,
            sort: Sort::Place(crate::book::Class::Asset),
            system: None,
            restricted: false,
            deferred: false,
            basis: None,
            claim: false,
            select: None,
            liquidity: None,
            purpose: None,
            pays: None,
            takes: Box::default(),
            sales_tax: None,
            shares: Box::default(),
            slots: axiom_core::Run::default(),
            props: Box::default(),
            laws: Box::default(),
            doc: None,
            loc: None,
        };
        let mut tab = place(Role::Tab(Id::new(0)), empty);
        inherit_place_traits(&mut tab, &kind);
        assert!(tab.claim);

        let mut account = place(Role::Account { institution: None }, empty);
        inherit_place_traits(&mut account, &kind);
        assert!(!account.claim);
    }

    fn update(name: Sym, since: Day, until: Option<Day>, value: bool, order: usize) -> PropertyChange {
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
            [(start, Value::Bool(true), Loc::default()), (restore, Value::Empty, Loc::default()),]
        );
    }

    #[test]
    fn deep_kind_defaults_apply_oldest_to_nearest_and_keep_the_nearest_source() {
        let mut names = axiom_core::Interner::default();
        let property = names.intern("rate");
        let kinds = (0..3)
            .map(|at| Kind {
                name: names.intern(["base", "middle", "leaf"][at]),
                sort: Sort::Thing,
                system: None,
                restricted: false,
                deferred: false,
                basis: None,
                claim: false,
                select: None,
                liquidity: None,
                purpose: None,
                pays: None,
                takes: Box::default(),
                sales_tax: None,
                shares: Box::default(),
                slots: axiom_core::Run::default(),
                props: Box::default(),
                laws: Box::default(),
                doc: None,
                loc: None,
            })
            .collect::<Vec<_>>();
        let (tree, ids) = Tree::build(kinds, &[None, Some(0), Some(1)]).unwrap();
        let mut defaults = Defaults::empty(3);
        for (at, value) in [10, 20, 30].into_iter().enumerate() {
            defaults.by_kind[at].push(Assign::Prop(Prop {
                name: property,
                value: Value::Num(Ratio::int(value)),
                since: Day::MIN,
                loc: Some(Loc::new(axiom_core::FileId(0), at as u32, at as u32 + 1)),
            }));
        }

        let mut path = Vec::new();
        let mut props = Box::<[Prop]>::default();
        defaults.apply(&tree, ids[2], &mut path, |assign| {
            if let Assign::Prop(prop) = assign {
                put(&mut props, *prop);
            }
        });

        assert_eq!(path, [ids[0], ids[1], ids[2]]);
        assert_eq!(props.len(), 1);
        assert_eq!(props[0].value, Value::Num(Ratio::int(30)));
        assert_eq!(props[0].loc, Some(Loc::new(axiom_core::FileId(0), 2, 3)));
    }

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
            props: Box::default(),
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
