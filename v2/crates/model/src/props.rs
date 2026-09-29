//! Properties: the lines written under accounts, entities, commodities and
//! kinds.
//!
//! Some are the language's own (`owner`, `opened`, `via`, `precision`); the
//! rest are declared by a kind with `has NAME TYPE` and typed by it. A line
//! naming neither is an error with a suggestion. A kind's own lines are
//! defaults for its instances, so each instance ends up with its own
//! properties first and then the defaults it did not override.

use axiom_core::diag::closest;
use axiom_core::{Day, Diagnostic, Id, Loc};
use axiom_syntax::{Decl, ExprKind, Prop as PropLine};

use crate::args::{Args, Builtin, Target, list};
use crate::book::{Amount, Commodity, Entity, Has, Kind, Place, Prop, Props, Residence};
use crate::catalog::{Catalog, Written};
use crate::commodities::MAX_SCALE;
use crate::errors::article;
use crate::kinds::policy;
use crate::law::Window;
use crate::world::World;

/// `budget 500 USD monthly`, waiting to become a law once laws can be built.
pub(crate) struct Budget {
    pub place: Id<Place>,
    pub amount: Amount,
    pub window: Window,
    pub loc: Loc,
}

pub(crate) fn apply<'s>(world: &mut World<'s>, catalog: &Catalog<'_, 's>, diags: &mut Vec<Diagnostic>) {
    kind_defaults(world, catalog, diags);
    for (written, id) in catalog.commodities.iter().zip(world.declared.commodities.clone()) {
        commodity(world, written, id, diags);
    }
    for (written, id) in catalog.entities.iter().zip(world.declared.entities.clone()) {
        entity(world, written, id, diags);
    }
    for (written, id) in catalog.accounts.iter().zip(world.declared.places.clone()) {
        if let Some(id) = id {
            place(world, written, id, diags);
        }
    }
}

enum Line {
    Builtin(Builtin),
    Declared(Has),
}

/// Reads every property line of `written`. The language's own go to `builtin`;
/// declared ones are typed and collected.
fn read_lines<'s>(
    world: &mut World<'s>,
    written: &Written<'_, 's, Decl<'s>>,
    target: Target,
    kind: Id<Kind>,
    diags: &mut Vec<Diagnostic>,
    mut builtin: impl FnMut(&mut World<'s>, Builtin, &mut Args<'_, 's>) -> Result<(), Diagnostic>,
) -> Vec<Prop> {
    let has = world.book.kinds[kind].has.clone();
    let mut declared = Vec::new();
    for line in &written.what.props {
        let mut args = Args::new(written.exprs(), written.home(), line);
        let read = match classify(world, target, kind, &has, line) {
            Ok(Line::Builtin(which)) => builtin(world, which, &mut args).and_then(|()| args.done()),
            Ok(Line::Declared(has)) => declared_value(world, &mut args, has).map(|value| declared.push(value)),
            Err(unknown) => Err(unknown),
        };
        diags.extend(read.err());
    }
    declared
}

fn classify(world: &World, target: Target, kind: Id<Kind>, has: &[Has], line: &PropLine) -> Result<Line, Diagnostic> {
    let word = line.name.text;
    if let Some(which) = Builtin::parse(word).filter(|which| which.applies_to(target)) {
        return Ok(Line::Builtin(which));
    }
    let declared = world.book.names.get(word).and_then(|sym| has.iter().find(|has| has.name == sym));
    match declared {
        Some(&has) => Ok(Line::Declared(has)),
        None => Err(unknown_property(world, target, kind, has, line)),
    }
}

/// `benificiary` is not a property of an account of kind `529`.
fn unknown_property(world: &World, target: Target, kind: Id<Kind>, has: &[Has], line: &PropLine) -> Diagnostic {
    let word = line.name.text;
    let kind_name = world.book.name(world.book.kinds[kind].name);
    let mut valid: Vec<&str> = Builtin::words(target).collect();
    valid.extend(has.iter().map(|has| world.book.name(has.name)));
    let mut diagnostic = Diagnostic::error(
        "unknown-property",
        format!("`{word}` is not a property of {} of kind `{kind_name}`", article(target.noun())),
    )
    .label(line.name.loc, "no such property");
    if let Some(near) = closest(word, valid.iter().copied()) {
        diagnostic = diagnostic.fix(format!("did you mean `{near}`?"), line.name.loc, near);
    }
    if let Some(elsewhere) = Builtin::parse(word) {
        let owners =
            elsewhere.targets().iter().map(|owner| format!("{}s", owner.noun())).collect::<Vec<_>>().join(" and ");
        diagnostic = diagnostic.note(format!("`{word}` describes {owners}, not {}", article(target.noun())));
    }
    diagnostic.note(format!("its properties are {}", list(&valid)))
}

/// A property declared by `has`, given a value of the type it declares.
fn declared_value<'s>(world: &mut World<'s>, args: &mut Args<'_, 's>, has: Has) -> Result<Prop, Diagnostic> {
    let id = args.next_id("a value")?;
    let (value, _) = world.constant(args.home, args.exprs, id, Some(has.ty))?;
    args.done()?;
    Ok(Prop { name: has.name, value, loc: Some(args.prop.loc) })
}

/// A thing's own properties, then the defaults its kind gives that it did not
/// set itself.
fn with_defaults(own: Vec<Prop>, defaults: &[Prop]) -> Props {
    let inherited = defaults.iter().filter(|default| !own.iter().any(|set| set.name == default.name));
    own.iter().chain(inherited).copied().collect()
}

/// Kinds pass their defaults down: each kind's own lines, then its parent's.
fn kind_defaults<'s>(world: &mut World<'s>, catalog: &Catalog<'_, 's>, diags: &mut Vec<Diagnostic>) {
    let mut own: Vec<Vec<Prop>> = vec![Vec::new(); world.book.kinds.len()];
    for (written, id) in catalog.kinds.iter().zip(world.declared.kinds.clone()) {
        // The kind's built-ins were read when it was declared.
        let read = read_lines(world, written, Target::Kind, id, diags, |_, _, args| {
            args.skip();
            Ok(())
        });
        own[id.index()].extend(read);
    }
    for id in world.book.kinds.ids() {
        let inherited =
            world.book.kinds.parent(id).map_or_else(Box::default, |parent| world.book.kinds[parent].props.clone());
        world.book.kinds[id].props = with_defaults(std::mem::take(&mut own[id.index()]), &inherited);
    }
}

fn commodity<'s>(
    world: &mut World<'s>,
    written: &Written<'_, 's, Decl<'s>>,
    id: Id<Commodity>,
    diags: &mut Vec<Diagnostic>,
) {
    let kind = world.book.commodities[id].kind;
    let declared = read_lines(world, written, Target::Commodity, kind, diags, |world, which, args| {
        match which {
            // Read when the commodity was declared, since counting needs it.
            Builtin::Precision => {
                args.count(MAX_SCALE)?;
            }
            Builtin::Title => {
                let title = args.text()?;
                world.book.commodities[id].title = Some(world.book.names.intern(title));
            }
            Builtin::Liquidity => world.book.commodities[id].liquidity = Some(args.span()?),
            Builtin::Grows => {
                let rate = args.percent()?;
                args.word(&["yearly"])?;
                world.book.commodities[id].growth = Some(rate);
            }
            _ => unreachable!("read_lines passes only the built-ins that describe a commodity"),
        }
        Ok(())
    });
    world.book.commodities[id].props = with_defaults(declared, &world.book.kinds[kind].props);
}

fn entity<'s>(world: &mut World<'s>, written: &Written<'_, 's, Decl<'s>>, id: Id<Entity>, diags: &mut Vec<Diagnostic>) {
    let kind = world.book.entities[id].kind;
    let mut lives: Vec<Residence> = Vec::new();
    let declared = read_lines(world, written, Target::Entity, kind, diags, |world, which, args| {
        match which {
            Builtin::Via => {
                let via = args.place(world)?;
                world.book.entities[id].via = Some(via);
            }
            Builtin::Lives => lives.push(args.residence(world)?),
            _ => unreachable!("read_lines passes only the built-ins that describe an entity"),
        }
        Ok(())
    });
    lives.sort_by_key(|residence| residence.from);
    let entity = &mut world.book.entities[id];
    entity.lives = lives.into();
    entity.props = with_defaults(declared, &world.book.kinds[kind].props);
}

fn place<'s>(world: &mut World<'s>, written: &Written<'_, 's, Decl<'s>>, id: Id<Place>, diags: &mut Vec<Diagnostic>) {
    let kind = world.book.places[id].kind;
    let declared = read_lines(world, written, Target::Place, kind, diags, |world, which, args| {
        match which {
            Builtin::Owner => {
                let owner = args.entity(world)?;
                world.book.places[id].owner = owner;
            }
            Builtin::Holds => {
                let holds = args.holds(world)?;
                world.book.places[id].holds = holds;
            }
            Builtin::Select => world.book.places[id].select = Some(policy(args)?),
            Builtin::Opened => world.book.places[id].opened = Some(args.day()?),
            Builtin::Closed => world.book.places[id].closed = Some(args.day()?),
            Builtin::Liquidity => world.book.places[id].liquidity = Some(args.span()?),
            Builtin::Budget => {
                let amount = args.amount(world)?;
                let window = match args.word(&["monthly", "yearly"])? {
                    "monthly" => Window::Month,
                    _ => Window::Year,
                };
                world.budgets.push(Budget { place: id, amount, window, loc: args.prop.loc });
            }
            _ => unreachable!("read_lines passes only the built-ins that describe an account"),
        }
        Ok(())
    });
    let place = &mut world.book.places[id];
    place.props = with_defaults(declared, &world.book.kinds[kind].props);
    if let (Some(opened), Some(closed)) = (place.opened, place.closed)
        && closed < opened
    {
        let loc = written.what.name.loc;
        diags.push(
            Diagnostic::error("closed-before-opened", "this account closes before it opens")
                .label(loc, "opened and closed the wrong way round"),
        );
    }
}

// Reading arguments that name things needs the world to name them in.
impl<'s> Args<'_, 's> {
    fn entity(&mut self, world: &World<'s>) -> Result<Id<Entity>, Diagnostic> {
        let name = self.name("an entity")?;
        world.entity(self.home, name)
    }

    fn place(&mut self, world: &World<'s>) -> Result<Id<Place>, Diagnostic> {
        let name = self.name("a place")?;
        world.place(name)
    }

    fn amount(&mut self, world: &World<'s>) -> Result<Amount, Diagnostic> {
        let expr = self.take("an amount")?;
        match expr.kind {
            ExprKind::Amount(number, unit) => world.amount(number, unit, expr.loc),
            _ => Err(self.wrong(expr, "an amount such as `500 USD`")),
        }
    }

    /// `holds USD, VTI`, or `holds any`.
    fn holds(&mut self, world: &World<'s>) -> Result<Option<Box<[Id<Commodity>]>>, Diagnostic> {
        if matches!(self.peek().map(|expr| &expr.kind), Some(ExprKind::Name("any"))) {
            self.word(&["any"])?;
            return Ok(None);
        }
        let mut units = Vec::new();
        while self.peek().is_some() {
            let expr = self.take("a commodity")?;
            let ExprKind::Unit(symbol) = expr.kind else {
                return Err(self.wrong(expr, "commodities such as `USD`, or `any`"));
            };
            units.push(world.commodity(axiom_syntax::Name { text: symbol, loc: expr.loc })?);
        }
        if units.is_empty() {
            return Err(self.wrong_missing());
        }
        Ok(Some(units.into()))
    }

    /// `lives us/ca`, or `lives us/ca from 2026-01-01`.
    fn residence(&mut self, world: &World<'s>) -> Result<Residence, Diagnostic> {
        let name = self.name("a system")?;
        let system = world.system(name)?;
        let from = match self.peek() {
            Some(_) => {
                self.word(&["from"])?;
                self.day()?
            }
            None => Day(i32::MIN),
        };
        Ok(Residence { from, system })
    }

    fn wrong_missing(&self) -> Diagnostic {
        Diagnostic::error("property-argument", format!("`{}` needs commodities or `any`", self.prop.name.text))
            .label(self.prop.loc, "name what it holds")
    }
}
