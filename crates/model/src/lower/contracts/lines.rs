//! What a contract's property lines say: `loan` with its `prepay` and `resets`, `from` and `until`, `grace`,
//! `for last`, `covers`, `rising`, `indexed`, `area`, `deposit` and `share`. Each is read by the one reader of a
//! property line ([`Args`]); its form, below, is what a mistake in it shows as the help.

use axiom_core::{Day, Days, Diagnostic, Id, Loc, Ratio, Span};
use axiom_syntax as ast;
use axiom_syntax::{BinOp, ExprKind};

use super::{Keeping, TermsCx, WrittenContract, asset_area};
use crate::args::Args;
use crate::book::{Amount, Asset, Coverage, Entity, Escalation, Param, Place, Prepay, Relative, Reset, Role, Share};
use crate::declare::World;
use crate::errors::{Reported, Word};
use crate::problem;
use crate::scope::Home;

const LOAN: &str = "loan AMOUNT on DATE at RATE over SPAN [for ASSET]";
const PREPAY: &str = "prepay shortens|recasts";
const RESETS: &str = "resets SPAN from DATE to PARAM + PERCENT [cap PERCENT] [life PERCENT]";
const FROM: &str = "from DATE";
const UNTIL: &str = "until DATE";
const GRACE: &str = "grace SPAN";
const FOR_LAST: &str = "for last month|quarter|year";
const COVERS: &str = "covers SPAN|the month|the quarter|the year";
const RISING: &str = "rising PERCENT yearly";
const INDEXED: &str = "indexed to PARAM yearly";
const AREA: &str = "area AMOUNT";
const DEPOSIT: &str = "deposit AMOUNT [into HOLDING]";
const SHARE: &str = "share RATE for ENTITY [RATE for ENTITY ...]";

/// The one line named `name` among `props`, if one is written: a second is a mistake, said as `what` written twice.
fn only<'a, 's>(
    file: &'a ast::File<'s>,
    props: ast::Many<ast::Prop<'s>>,
    name: &str,
    what: &str,
) -> Result<Option<&'a ast::Prop<'s>>, Diagnostic> {
    let mut written = file[props].iter().filter(|prop| prop.name.0 == name);
    let first = written.next();
    match (first, written.next()) {
        (Some(first), Some(again)) => Err(problem::twice(what, again.loc, first.loc)),
        (first, _) => Ok(first),
    }
}

/// A param that names a rate, from where `word` is written.
fn param(world: &World<'_>, home: Home, word: Word) -> Result<Id<Param>, Diagnostic> {
    match world.seek_param(home, word) {
        Ok(Some(param)) => Ok(param),
        Ok(None) => Err(world.missing_param(home, word)),
        Err(problem) => Err(problem),
    }
}

/// What a `loan` line says, and the yearly rate it was made at.
pub(super) struct LoanLine {
    pub principal: Amount,
    pub on: Day,
    pub rate: Ratio,
    pub term: Span,
    pub asset: Option<Id<Asset>>,
    pub resets: Option<Reset>,
    pub prepay: Prepay,
    pub loc: Loc,
}

/// `loan AMOUNT on DATE at RATE over SPAN [for ASSET]`, with the lines under it.
pub(super) fn loan<'s>(world: &World<'s>, contract: WrittenContract<'_, 's>) -> Result<Option<LoanLine>, Diagnostic> {
    let file = contract.file();
    let Some(prop) = only(file, contract.node.props, "loan", "loan")? else {
        return Ok(None);
    };
    let mut a = Args::shaped(file, prop, "contract-loan", LOAN);
    let principal = a.with("contract-loan-principal", |a| a.positive_amount(world, world.book.base))?;
    a.word(&["on"])?;
    let on = a.with("contract-loan-date", Args::day)?;
    a.word(&["at"])?;
    let rate = a.with("contract-loan-rate", Args::rate)?;
    a.word(&["over"])?;
    let term = a.with("contract-loan-term", Args::positive_span)?;
    let asset = match a.peek() {
        None => None,
        Some(_) => {
            let asset = a.with("contract-loan-asset", |a| a.word(&["for"]).and_then(|_| a.asset(world)));
            let undeclared = |problem: Diagnostic| match problem.code == "unknown-asset" {
                true => Diagnostic { code: "contract-loan-asset".into(), ..problem },
                false => problem,
            };
            Some(asset.map_err(undeclared)?.0)
        }
    };
    a.done()?;
    let (resets, prepay) = loan_lines(world, contract, prop, on)?;
    Ok(Some(LoanLine { principal, on, rate, term, asset, resets, prepay, loc: prop.loc }))
}

/// The lines under a loan: at most one `resets` and one `prepay`, and nothing else.
fn loan_lines<'s>(
    world: &World<'s>,
    contract: WrittenContract<'_, 's>,
    loan: &ast::Prop<'s>,
    on: Day,
) -> Result<(Option<Reset>, Prepay), Diagnostic> {
    let file = contract.file();
    let nested: Vec<&ast::Prop<'s>> = file[loan.lines].iter().map(|nested| &nested.0).collect();
    if let Some(other) = nested.iter().find(|line| !matches!(line.name.0, "resets" | "prepay")) {
        return Err(Diagnostic::error("contract-loan-property", "this nested loan property is not supported")
            .label(other.loc, "remove or correct this property"));
    }
    let once = |name: &str, what: &str| -> Result<Option<&ast::Prop<'s>>, Diagnostic> {
        let mut written = nested.iter().copied().filter(|line| line.name.0 == name);
        match (written.next(), written.next()) {
            (Some(first), Some(again)) => Err(problem::twice(what, again.loc, first.loc)),
            (first, _) => Ok(first),
        }
    };
    let resets = match once("resets", "reset rule")? {
        Some(line) => Some(resets(world, contract.home(), file, line, on)?),
        None => None,
    };
    let prepay = match once("prepay", "prepayment rule")? {
        Some(line) => {
            let mut a = Args::shaped(file, line, "contract-loan-prepay", PREPAY);
            let word = a.word(&["shortens", "recasts"])?;
            a.done()?;
            if word == "recasts" { Prepay::Recasts } else { Prepay::Shortens }
        }
        None => Prepay::Shortens,
    };
    Ok((resets, prepay))
}

/// `resets 1y from DATE to PARAM + PERCENT`, then `cap PERCENT` and `life PERCENT`, each at most once.
fn resets<'s>(
    world: &World<'s>,
    home: Home,
    file: &ast::File<'s>,
    line: &ast::Prop<'s>,
    on: Day,
) -> Result<Reset, Diagnostic> {
    let mut a = Args::shaped(file, line, "contract-loan-resets", RESETS);
    let every = a.positive_span()?;
    a.word(&["from"])?;
    let at = a.peek().map_or(line.loc, |expr| expr.loc);
    let from = a.day()?;
    if from < on {
        return Err(a.refuse(at, "a first reset on or after the loan's day"));
    }
    a.word(&["to"])?;
    let index_and_margin = |expr: &ast::Expr<'s>| match expr.kind {
        ExprKind::Binary(BinOp::Add, index, margin) => match (&file.exprs[index].kind, &file.exprs[margin].kind) {
            (ExprKind::Name(name), ExprKind::Pct(percent)) => {
                Some((Word { text: name.0, loc: file.exprs[index].loc }, *percent, file.exprs[margin].loc))
            }
            _ => None,
        },
        _ => None,
    };
    let (word, margin, at) = a.arg("`PARAM + PERCENT`", index_and_margin)?;
    let margin = Ratio::percent(margin.mantissa.into(), margin.scale).filter(|margin| !margin.is_negative());
    let margin = margin.ok_or_else(|| a.refuse(at, "a margin of zero or more"))?;
    let index = param(world, home, word)?;
    if world.book.params[index].unit.is_some_and(|unit| unit != axiom_core::Dim::Number) {
        return Err(Diagnostic::error("contract-loan-index-unit", "a loan reset index is a rate")
            .label(word.loc, "use a parameter with no unit or a percentage value"));
    }
    let (mut cap, mut life) = (None, None);
    while a.peek().is_some() {
        let at = a.peek().map_or(line.loc, |expr| expr.loc);
        let word = a.word(&["cap", "life"])?;
        let limit = if word == "cap" { &mut cap } else { &mut life };
        if limit.replace(a.rate()?).is_some() {
            return Err(a.twice(at, word));
        }
    }
    Ok(Reset { every, from, index, margin, cap, life })
}

/// The days a contract holds: from its `from` to its `until`, each written at most once.
pub(super) fn days(file: &ast::File<'_>, props: ast::Many<ast::Prop<'_>>) -> Result<Days, Diagnostic> {
    let day = |name, what, form| -> Result<Option<Day>, Diagnostic> {
        let Some(line) = only(file, props, name, what)? else { return Ok(None) };
        let mut a = Args::shaped(file, line, "contract-date", form);
        let day = a.day()?;
        a.done().map(|()| Some(day))
    };
    let first = day("from", "start date", FROM)?.unwrap_or(Day::MIN);
    let last = day("until", "end date", UNTIL)?.unwrap_or(Day::MAX);
    Days::new(first, last).ok_or_else(|| {
        Diagnostic::error("contract-range", "a contract ends before it begins")
            .label(file[props].first().map_or(Loc::default(), |prop| prop.loc), "these dates do not overlap")
    })
}

/// `grace 5d`: how long after a due day an occurrence is still on time.
pub(super) fn grace(file: &ast::File<'_>, props: ast::Many<ast::Prop<'_>>) -> Result<Option<Span>, Diagnostic> {
    let Some(line) = only(file, props, "grace", "grace interval")? else { return Ok(None) };
    let mut a = Args::shaped(file, line, "contract-span", GRACE);
    let at = a.peek().map_or(line.loc, |expr| expr.loc);
    let span = a.span()?;
    a.done()?;
    match span.months < 0 || span.days < 0 {
        false => Ok(Some(span)),
        true => Err(a.refuse_as("contract-grace", at, "a span of zero or more")),
    }
}

/// `for last month`: the period before a due day that an occurrence is for.
pub(super) fn period(file: &ast::File<'_>, props: ast::Many<ast::Prop<'_>>) -> Result<Option<Relative>, Diagnostic> {
    let Some(line) = file[props].iter().find(|prop| prop.name.0 == "for") else { return Ok(None) };
    let mut a = Args::shaped(file, line, "contract-period", FOR_LAST);
    a.word(&["last"])?;
    let period = match a.word(&["month", "quarter", "year"])? {
        "month" => Relative::Last(axiom_core::Period::Month),
        "quarter" => Relative::LastQuarter,
        _ => Relative::Last(axiom_core::Period::Year),
    };
    a.done().map(|()| Some(period))
}

/// `covers 1y` or `covers the year`: the days an occurrence pays for.
pub(super) fn covers(file: &ast::File<'_>, props: ast::Many<ast::Prop<'_>>) -> Result<Option<Coverage>, Diagnostic> {
    let Some(line) = file[props].iter().find(|prop| prop.name.0 == "covers") else { return Ok(None) };
    let mut a = Args::shaped(file, line, "contract-covers", COVERS);
    let coverage = match a.takes("the") {
        true => match a.word(&["month", "quarter", "year"])? {
            "month" => Coverage::Calendar(axiom_core::Period::Month),
            "quarter" => Coverage::Quarter,
            _ => Coverage::Calendar(axiom_core::Period::Year),
        },
        false => Coverage::Span(a.span()?),
    };
    a.done().map(|()| Some(coverage))
}

/// `rising 3% yearly` or `indexed to cpi yearly`: how what a contract asks grows. The first that is right counts;
/// each that is wrong is said.
pub(super) fn escalation<'s>(
    world: &World<'s>,
    home: Home,
    file: &ast::File<'s>,
    props: ast::Many<ast::Prop<'s>>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Escalation> {
    let lines = file[props].iter().filter(|prop| matches!(prop.name.0, "rising" | "indexed"));
    lines.filter_map(|line| escalation_line(world, home, file, line).or_report(diags)).next()
}

fn escalation_line<'s>(
    world: &World<'s>,
    home: Home,
    file: &ast::File<'s>,
    line: &ast::Prop<'s>,
) -> Result<Escalation, Diagnostic> {
    let rising = line.name.0 == "rising";
    if file[line.args].is_empty() {
        let form = if rising { RISING } else { INDEXED };
        return Err(Args::shaped(file, line, "contract-escalation", form).next_id("a rate or an index").unwrap_err());
    }
    if rising {
        let mut a = Args::shaped(file, line, "contract-rate", RISING);
        let rate = a.rate()?;
        a.word(&["yearly"])?;
        a.done()?;
        return Ok(Escalation::Rising(rate));
    }
    let mut a = Args::shaped(file, line, "contract-index", INDEXED);
    a.word(&["to"])?;
    let index = a.name("a param")?;
    a.word(&["yearly"])?;
    a.done()?;
    Ok(Escalation::Indexed(param(world, home, index)?))
}

/// `area 1_000 SQFT`: how much space a contract is about, in a measure.
pub(super) fn area<'s>(
    world: &World<'s>,
    file: &ast::File<'s>,
    props: ast::Many<ast::Prop<'s>>,
) -> Result<Option<Amount>, Diagnostic> {
    let Some(line) = only(file, props, "area", "area")? else { return Ok(None) };
    let mut a = Args::shaped(file, line, "contract-area", AREA);
    a.flat()?;
    let (area, at) = a.amount(world, world.book.base)?;
    a.done()?;
    if !world.book.is_a(world.book.commodities[area.unit].kind, world.book.roots.kinds.measure) {
        return Err(a.refuse_as("contract-area-unit", at, "an area in a measure such as `SQFT`"));
    }
    match area.qty.0 > 0 {
        true => Ok(Some(area)),
        false => Err(a.refuse_as("contract-area-positive", at, "an area above zero")),
    }
}

/// `deposit 1_000 USD [into HOLDING]`: what the owner keeps of the party's, and where.
pub(super) fn deposit<'s>(
    world: &World<'s>,
    contract: WrittenContract<'_, 's>,
    keeping: Keeping<'s>,
) -> Result<Option<(Amount, Id<Place>)>, Diagnostic> {
    let file = contract.file();
    let Some(line) = only(file, contract.node.props, "deposit", "deposit")? else { return Ok(None) };
    let mut a = Args::shaped(file, line, "contract-deposit", DEPOSIT);
    a.flat()?;
    let currency = world.book.currency(keeping.owner);
    let (amount, at) = a.with("contract-deposit-amount", |a| a.amount(world, currency))?;
    if amount.qty.0 <= 0 {
        return Err(a.refuse_as("contract-deposit-positive", at, "a deposit above zero"));
    }
    let holding = match a.peek() {
        Some(_) => a.with("contract-deposit-holding", |a| a.word(&["into"]).and_then(|_| a.name("an account")))?,
        None => match keeping.default_holding {
            Some((name, loc)) => Word { text: name.0, loc },
            None => {
                return Err(Diagnostic::error(
                    "contract-deposit-holding-required",
                    "a deposit needs a holding account",
                )
                .label(line.loc, "name `into HOLDING` or give this contract an active schedule with a holding"));
            }
        },
    };
    a.done()?;
    let place = kept_in(world, contract, keeping, holding)?;
    if world.book.holds(place).is_some_and(|mut units| !units.any(|unit| unit == amount.unit)) {
        return Err(Diagnostic::error("contract-deposit-unit", "the deposit holding does not accept this unit")
            .label(at, "choose a unit the holding can keep"));
    }
    Ok(Some((amount, place)))
}

/// The account a deposit is kept in, which must be an account of the owner's.
fn kept_in<'s>(
    world: &World<'s>,
    contract: WrittenContract<'_, 's>,
    keeping: Keeping<'s>,
    holding: Word<'s>,
) -> Result<Id<Place>, Diagnostic> {
    let place = world.end(contract.home(), Word::of(contract.file(), holding.text))?.place;
    let kept = &world.book.places[place];
    if !matches!(kept.role, Role::Account { .. } | Role::Holding(_)) {
        return Err(Diagnostic::error("contract-deposit-holding", "a deposit is held in an account")
            .label(holding.loc, "choose an account or holding, not an asset or party"));
    }
    if kept.owner != keeping.owner {
        return Err(Diagnostic::error("contract-deposit-owner", "the deposit holding belongs to another owner")
            .label(holding.loc, "choose a holding owned by the contract owner")
            .context(kept.loc.unwrap_or(holding.loc), "this place is declared here"));
    }
    Ok(place)
}

/// The shares a contract divides what it brings in by: each `share RATE for ENTITY`, as a percentage, a fraction or
/// a measure of the area the contract or the asset it is about has. A line stops at the first pair that is wrong.
pub(super) fn shares<'s>(world: &World<'s>, cx: &TermsCx<'_, 's>, diags: &mut Vec<Diagnostic>) -> Vec<Share> {
    let file = cx.file;
    let mut shares = Vec::new();
    let mut total = Ratio::ZERO;
    for line in file[cx.written.node.props].iter().filter(|prop| prop.name.0 == "share") {
        let mut a = Args::shaped(file, line, "contract-share", SHARE);
        let read = share_line(world, cx, &mut a, &mut total, &mut shares);
        read.or_report(diags);
    }
    shares
}

fn share_line<'s>(
    world: &World<'s>,
    cx: &TermsCx<'_, 's>,
    a: &mut Args<'_, 's>,
    total: &mut Ratio,
    shares: &mut Vec<Share>,
) -> Result<(), Diagnostic> {
    loop {
        let at = a.peek().map_or(a.line.loc, |expr| expr.loc);
        let (rate, measure) = share_rate(world, cx, a)?;
        let rate = rate.filter(|rate| !rate.is_negative()).ok_or_else(|| a.refuse(at, "a share of zero or more"))?;
        a.word(&["for"])?;
        let owner = a.name("an entity")?;
        *total = add_share(*total, rate, a.line.loc)?;
        let entity: Id<Entity> = world.entity(cx.written.site.home, owner)?;
        shares.push(Share { rate, entity, measure, loc: a.line.loc });
        if a.peek().is_none() {
            return Ok(());
        }
    }
}

/// The total of the shares with this one in it, which is at most the whole.
fn add_share(total: Ratio, rate: Ratio, loc: Loc) -> Result<Ratio, Diagnostic> {
    let Some(next) = total.checked_add(rate) else {
        return Err(Diagnostic::error("contract-share-total", "contract shares exceed exact arithmetic")
            .label(loc, "reduce the declared shares"));
    };
    match next.checked_sub(Ratio::ONE).is_some_and(|excess| !excess.is_negative() && !excess.is_zero()) {
        false => Ok(next),
        true => Err(Diagnostic::error("contract-share-total", "contract shares add up to more than 100%")
            .label(loc, "the total shares cannot exceed 100%")),
    }
}

/// What a share is of the whole, and for a measure the two amounts it is the ratio of.
fn share_rate<'s>(
    world: &World<'s>,
    cx: &TermsCx<'_, 's>,
    a: &mut Args<'_, 's>,
) -> Result<(Option<Ratio>, Option<(Amount, Amount)>), Diagnostic> {
    let written = |expr: &ast::Expr<'s>| match expr.kind {
        ExprKind::Pct(percent) => Some(Err(percent.to_ratio().and_then(|rate| rate.checked_div(Ratio::new(100, 1)?)))),
        ExprKind::Fraction(top, bottom) => Some(Err(Ratio::new(i128::from(top), i128::from(bottom)))),
        ExprKind::Amount(literal) => Some(Ok((literal, expr.loc))),
        _ => None,
    };
    let (literal, loc) = match a.arg("a percentage, a fraction or a measure", written)? {
        Err(rate) => return Ok((rate, None)),
        Ok(measure) => measure,
    };
    let numerator = measured(world, a, literal, loc)?;
    let denominator = cx.area.or_else(|| {
        cx.purpose.and_then(|at| at.value.of).and_then(|object| match object {
            crate::journal::Object::Asset(asset) => asset_area(world, asset, cx.anchor),
            _ => None,
        })
    });
    match denominator.filter(|area| area.unit == numerator.unit && area.qty.0 > 0) {
        Some(area) => Ok((Ratio::new(i128::from(numerator.qty.0), i128::from(area.qty.0)), Some((numerator, area)))),
        None => Err(Diagnostic::error(
            "contract-share-measure",
            "a measured share needs a positive contract or asset area in the same unit",
        )
        .label(loc, "cannot resolve this measure")),
    }
}

/// `120 SQFT`: the measured amount a share is of an area, which must be in a measure unit.
fn measured<'s>(
    world: &World<'s>,
    a: &Args<'_, 's>,
    literal: ast::Literal<'s>,
    loc: Loc,
) -> Result<Amount, Diagnostic> {
    let amount = world.literal_amount(a.file, literal, None)?;
    match world.book.is_a(world.book.commodities[amount.unit].kind, world.book.roots.kinds.measure) {
        true => Ok(amount),
        false => Err(Diagnostic::error("contract-share-unit", "a measured share must use a measure unit")
            .label(loc, "this commodity is not a measure")),
    }
}
