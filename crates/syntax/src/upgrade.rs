//! `axiom fmt --upgrade`: a book written the way v4 wrote it, written the way v5 does.
//!
//! What changes is how a line is spelled, and never what the book says. A leg gets its arrow, a payment that arrives from a
//! party is written `<-` from the book's own side, a paystub names the owner it passes through, and an exchange says its
//! price once. A name, a comment, or an amount that a price does not already say is never touched.
//!
//! Which end of a line is the book's own, who owns a position and how many decimals a commodity has are things no text
//! says, so they are asked of a [`Registry`], which a book answers; this crate knows no model. Every change is an edit to
//! the text the file was written in, and the edited text is laid out by the formatter, because a line that is rewritten
//! changes width and so do its neighbours' columns.
//!
//! Two kinds of line stay as written. One the book rejects today, such as an exchange that names one end, has a meaning
//! in v5, and reading it would change the book. One the upgrade cannot rewrite without guessing is refused, with the
//! words that would settle it.

use std::mem::take;
use std::ops::Range;

use axiom_core::num::{POW10, div_round};
use axiom_core::{Dec, Diagnostic, Loc};

use crate::ast::*;
use crate::style::{Header, Reader, Says};

/// Whose side of the book a name is on, as the end of a flow.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Standing {
    /// An account, an owner's own holding, or a loan's debt: the book's own side.
    Own,
    /// A party: money to it has left the owners, and money from it is new to them.
    Outside,
    /// A name nothing in the book answers to.
    Unknown,
}

/// What an upgrade asks of the book it upgrades.
pub trait Registry {
    /// Whose side `name` is on.
    fn standing(&self, name: &str) -> Standing;
    /// The owner of the own position `name`.
    fn owner(&self, name: &str) -> Option<&str>;
    /// The owner of what nothing says otherwise: the person keeping the book.
    fn keeper(&self) -> &str;
    /// The commodity the book counts in, which a price is quoted in when one of two units is it.
    fn base(&self) -> &str;
    /// How many decimals the commodity `unit` is kept to.
    fn scale(&self, unit: &str) -> Option<u8>;
}

/// A file upgraded: its new text, and the lines that were refused.
pub struct Upgraded {
    pub text: String,
    pub refused: Vec<Diagnostic>,
}

/// `file`, which is `src` and was parsed in `folder`, written the v5 way.
pub fn upgrade(src: &str, file: &File<'_>, folder: Folder, registry: &dyn Registry) -> Upgraded {
    let mut upgrade = Upgrade { reader: Reader::new(src, file), registry, edits: Vec::new(), refused: Vec::new() };
    file.items.iter().for_each(|item| upgrade.item(item));
    let Upgrade { mut edits, refused, .. } = upgrade;
    edits.sort_by_key(|(at, _)| at.start);
    let mut text = src.to_string();
    for (at, with) in edits.into_iter().rev() {
        text.replace_range(at, &with);
    }
    Upgraded { text: crate::parse(file.id, &text, folder).0.format(), refused }
}

struct Upgrade<'a, 's> {
    reader: Reader<'a, 's>,
    registry: &'a dyn Registry,
    /// What to write in place of a range of the source.
    edits: Vec<(Range<usize>, String)>,
    refused: Vec<Diagnostic>,
}

/// Where an amount is written on a header: before the verb, or after it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Place {
    Before,
    After,
}

impl<'a, 's> Upgrade<'a, 's> {
    fn item(&mut self, item: &Item<'s>) {
        let file = self.reader.file();
        match item.kind {
            ItemKind::Txn(id) => self.flow(item.loc, &file[id].flow),
            // A return's tallies are not flows, and move nothing.
            ItemKind::Statement(id) if !matches!(file[id].verb, Verb::Filed(_)) => {
                self.arrows(file[id].body.legs, Junction::Out)
            }
            ItemKind::Contract(id) => self.arrows(file[id].body.legs, Junction::Out),
            _ => {}
        }
    }

    /// Writes `toward` before each leg of `legs` that has none.
    fn arrows(&mut self, legs: Many<Leg<'s>>, toward: Junction) {
        for leg in self.reader.file()[legs].iter().filter(|leg| leg.arrow.is_none()) {
            let at = leg.loc.start as usize;
            self.edits.push((at..at, format!("{} ", toward.spelling())));
        }
    }

    /// A flow written the v4 way, rewritten: its header, and the arrows of its legs.
    fn flow(&mut self, loc: Loc, flow: &Flow<'s>) {
        if flow.junction == Junction::In || flow.through.is_some() {
            return;
        }
        let Some(header) = self.reader.cut_header(loc, flow.tail, Says::Flow) else { return };
        match self.rewritten(flow, &header) {
            Ok((new, toward)) => {
                if new != header {
                    self.edits.push((loc.range(), new.line()));
                }
                self.arrows(flow.body.legs, toward);
            }
            Err(refusal) => self.refused.push(refusal.at(loc)),
        }
    }

    /// The header a flow is written with now, and the way the arrows of its legs point. A line whose amounts no price
    /// relates is refused with the line it would be if the nearest price had been written.
    fn rewritten(&self, flow: &Flow<'s>, header: &Header) -> Result<(Header, Junction), Refusal> {
        self.rewrite(flow, header, None).map_err(|refusal| match refusal {
            Refusal::Price { exchange, quote, rate: Some(rate), .. } => {
                let fix = self.rewrite(flow, header, Some(rate)).ok().map(|(fixed, _)| fixed.line());
                Refusal::Price { exchange, quote, rate: Some(rate), fix }
            }
            other => other,
        })
    }

    /// [`Self::rewritten`], taking `assumed` as the price of an exchange that states none.
    fn rewrite(&self, flow: &Flow<'s>, header: &Header, assumed: Option<Dec>) -> Result<(Header, Junction), Refusal> {
        let mut h = header.clone();
        let name = |side: &Side<'s>| side.end.map(|end| end.name.0);
        let legs = !flow.body.legs.is_empty();
        let toward = match (name(&flow.from), name(&flow.to), legs) {
            (Some(source), Some(target), false) => {
                self.priced(flow, &mut h, assumed)?;
                self.written_from(flow, &mut h, source, target)?;
                Junction::Out
            }
            (Some(source), None, true) => {
                self.split(flow, &mut h, source)?;
                Junction::Out
            }
            (None, Some(target), true) => {
                self.split_into(flow, &mut h, target)?;
                Junction::In
            }
            _ => Junction::Out,
        };
        Ok((h, toward))
    }

    /// Whose line it is: the book's own end is written first, so a payment that arrives from a party is `<-`.
    fn written_from(&self, flow: &Flow<'s>, h: &mut Header, source: &str, target: &str) -> Result<(), Refusal> {
        match (self.registry.standing(source), self.registry.standing(target)) {
            (Standing::Own, _) => Ok(()),
            (_, Standing::Own) => Ok(self.take(flow, h)),
            _ => Err(Refusal::NeitherOwn),
        }
    }

    /// `S -> O A` written `O <- S A`, when its amount can go where a take puts it: after the end it takes from.
    fn take(&self, flow: &Flow<'s>, h: &mut Header) {
        let moves = h.held.is_empty() || (h.amount.is_empty() && is_literal(flow.from.amount) && !self.has_price(flow));
        if moves {
            std::mem::swap(&mut h.subject, &mut h.object);
            h.verb = Junction::In.spelling().into();
            self.amount_after(h);
        }
    }

    /// An amount written before the arrow, written after it.
    fn amount_after(&self, h: &mut Header) {
        if h.amount.is_empty() {
            h.amount = take(&mut h.held);
        }
    }

    /// One end and legs for the other: the amount goes after the arrow, and a party's money passes through its owner.
    fn split(&self, flow: &Flow<'s>, h: &mut Header, source: &str) -> Result<(), Refusal> {
        if is_literal(flow.from.amount) {
            self.amount_after(h);
        }
        if self.registry.standing(source) != Standing::Own {
            let owner = self.owner_of(flow)?;
            h.object = std::mem::replace(&mut h.subject, owner);
            h.verb = Junction::In.spelling().into();
        }
        Ok(())
    }

    /// What v4 wrote as `-> TARGET AMOUNT` with legs that are its sources: an own target is the subject, and a party's
    /// is paid by an owner.
    fn split_into(&self, flow: &Flow<'s>, h: &mut Header, target: &str) -> Result<(), Refusal> {
        match self.registry.standing(target) {
            Standing::Own => {
                h.subject = take(&mut h.object);
                h.verb = Junction::In.spelling().into();
            }
            _ => h.subject = self.owner_of(flow)?,
        }
        Ok(())
    }

    /// The one owner of the own accounts a split's legs end in, or the keeper of the book when it has none.
    fn owner_of(&self, flow: &Flow<'s>) -> Result<String, Refusal> {
        let mut owners: Vec<&str> = Vec::new();
        for leg in &self.reader.file()[flow.body.legs] {
            let name = leg.end.name.0;
            if self.registry.standing(name) == Standing::Own {
                let owner = self.registry.owner(name).unwrap_or(self.registry.keeper());
                if !owners.contains(&owner) {
                    owners.push(owner);
                }
            }
        }
        match owners[..] {
            [] => Ok(self.registry.keeper().into()),
            [owner] => Ok(owner.into()),
            [first, second, ..] => Err(Refusal::Owners(first.into(), second.into())),
        }
    }

    /// An amount written twice says it twice: the one in the price's unit is the other times the price, and the line says
    /// it once.
    fn priced(&self, flow: &Flow<'s>, h: &mut Header, assumed: Option<Dec>) -> Result<(), Refusal> {
        let (Some(left), Some(right)) = (money(flow.from.amount), money(flow.to.amount)) else { return Ok(()) };
        if left.unit == right.unit {
            if same_number(left.number, right.number) {
                h.held.clear();
            }
            return Ok(());
        }
        let written = self.price_written(flow);
        let quote = written.as_ref().map_or_else(|| self.quoted_in(&left, &right), |price| price.unit);
        let (kept, dropped, gone) = match (quote == left.unit, quote == right.unit) {
            (true, _) => (&right, &left, Place::Before),
            (_, true) => (&left, &right, Place::After),
            _ => return Ok(()),
        };
        let scale = self.registry.scale(quote).unwrap_or(2);
        let rate = match written {
            Some(price) if agrees(kept.number, price.number, dropped.number, scale) => None,
            Some(_) => return Ok(()),
            None => Some(
                assumed
                    .or_else(|| exact(dropped.number, kept.number))
                    .ok_or_else(|| inexact(kept, dropped, quote, scale))?,
            ),
        };
        state_once(h, gone, rate.map(|rate| format!("{} {quote}", shown(rate))).as_deref());
        Ok(())
    }

    /// The units a price is quoted in when a line names none: the book's own when it is one of the two, else the unit that
    /// arrives.
    fn quoted_in<'m>(&self, left: &Money<'m>, right: &Money<'m>) -> &'m str {
        match self.registry.base() {
            base if base == left.unit => left.unit,
            _ => right.unit,
        }
    }

    /// The price a flow's tail states, `@ 285.70 USD`.
    fn price_written(&self, flow: &Flow<'s>) -> Option<Money<'s>> {
        let file = self.reader.file();
        file[flow.tail].iter().find_map(|clause| match clause.kind {
            ClauseKind::Price(literal) => {
                Some(Money { number: literal.num(), unit: literal.unit()?.0, text: literal.0 })
            }
            _ => None,
        })
    }

    fn has_price(&self, flow: &Flow<'s>) -> bool {
        self.price_written(flow).is_some()
    }
}

/// An amount as a line wrote it.
struct Money<'s> {
    number: Dec,
    unit: &'s str,
    text: &'s str,
}

fn money<'s>(quantity: Option<Quantity<'s>>) -> Option<Money<'s>> {
    match quantity {
        Some(Quantity::Amount(Amount::Literal(literal))) => {
            Some(Money { number: literal.num(), unit: literal.unit()?.0, text: literal.0 })
        }
        _ => None,
    }
}

fn is_literal(quantity: Option<Quantity<'_>>) -> bool {
    money(quantity).is_some()
}

/// A line that said an amount twice says it once: the amount at `gone` is not written, and `price` is, when one was needed.
fn state_once(h: &mut Header, gone: Place, price: Option<&str>) {
    match gone {
        Place::Before => h.held.clear(),
        Place::After => h.amount.clear(),
    }
    if let Some(price) = price {
        h.tail = format!("@ {price} {}", h.tail).trim_end().to_string();
    }
}

fn same_number(a: Dec, b: Dec) -> bool {
    let (a, b) = ((i128::from(a.mantissa), a.scale), (i128::from(b.mantissa), b.scale));
    a.0 * POW10[usize::from(b.1)] == b.0 * POW10[usize::from(a.1)]
}

/// Whether `kept × rate`, rounded half to even to `scale` places, is `dropped`: what the model checks of a price and the
/// amounts a line writes with it.
fn agrees(kept: Dec, rate: Dec, dropped: Dec, scale: u8) -> bool {
    let product = i128::from(kept.mantissa) * i128::from(rate.mantissa) * POW10[usize::from(scale)];
    let rounded = div_round(product, POW10[usize::from(kept.scale) + usize::from(rate.scale)]);
    let written =
        (dropped.scale <= scale).then(|| i128::from(dropped.mantissa) * POW10[usize::from(scale - dropped.scale)]);
    rounded.is_some() && rounded == written
}

/// `dropped ÷ kept` to `places` decimals, rounded, and whether nothing was lost.
fn rate_at(dropped: Dec, kept: Dec, places: u8) -> Option<(Dec, bool)> {
    let numerator = i128::from(dropped.mantissa) * POW10[usize::from(kept.scale) + usize::from(places)];
    let denominator = i128::from(kept.mantissa) * POW10[usize::from(dropped.scale)];
    let mantissa = i64::try_from(div_round(numerator, denominator)?).ok()?;
    Some((Dec { mantissa, scale: places }, numerator % denominator == 0))
}

/// The most places a price a person writes has.
const PLACES: u8 = 10;

/// `dropped ÷ kept`, if it is a decimal that ends.
fn exact(dropped: Dec, kept: Dec) -> Option<Dec> {
    (0..=PLACES).find_map(|places| rate_at(dropped, kept, places).filter(|(_, whole)| *whole).map(|(rate, _)| rate))
}

/// The shortest price that, times `kept` and rounded to `scale` places, is `dropped`.
fn nearest(dropped: Dec, kept: Dec, scale: u8) -> Option<Dec> {
    (0..=PLACES)
        .find_map(|places| rate_at(dropped, kept, places).filter(|(rate, _)| agrees(kept, *rate, dropped, scale)))
        .map(|(rate, _)| rate)
}

/// A decimal as a person writes a price: at least two places, and more only where it has them.
fn shown(dec: Dec) -> String {
    let places = dec.scale.max(2);
    let mantissa = i128::from(dec.mantissa) * POW10[usize::from(places - dec.scale)];
    let (whole, part) = (mantissa / POW10[usize::from(places)], mantissa % POW10[usize::from(places)]);
    format!("{whole}.{part:0width$}", width = usize::from(places))
}

/// The refusal of two amounts that no price anyone wrote relates: what they are, and the nearest price that does.
fn inexact(kept: &Money<'_>, dropped: &Money<'_>, quote: &str, scale: u8) -> Refusal {
    let exchange = format!("{} for {}", kept.text, dropped.text);
    let rate = nearest(dropped.number, kept.number, scale);
    Refusal::Price { exchange, quote: quote.to_string(), rate, fix: None }
}

/// Why a line was not rewritten.
enum Refusal {
    /// Neither end is the book's own.
    NeitherOwn,
    /// The own accounts a split ends in have two owners.
    Owners(String, String),
    /// Two amounts and no price that a person would write: what they are, the nearest price that makes them agree (in
    /// the units `quote` names), and the line it makes.
    Price { exchange: String, quote: String, rate: Option<Dec>, fix: Option<String> },
}

impl Refusal {
    fn at(self, line: Loc) -> Diagnostic {
        match self {
            Refusal::NeitherOwn => Diagnostic::error("upgrade-sides", "neither end of this line is the book's own")
                .label(line, "which side is the owners' is not written, and is not guessed")
                .help("write the account or owner whose book it is first: `checking -> acme 100 USD`"),
            Refusal::Owners(first, second) => {
                Diagnostic::error("upgrade-owner", "this split ends in accounts of two owners")
                    .label(line, format!("`{first}` and `{second}` both hold part of it"))
                    .help("write the owner the money passes through first: `me <- acme 5_200 USD`, with the legs below")
            }
            Refusal::Price { exchange, quote, rate, fix } => {
                let diag = Diagnostic::error("upgrade-price", "no price anyone would write relates these two amounts")
                    .label(line, format!("{exchange} is not a whole price"))
                    .note("v5 states one amount and its price; the model checks the other against it");
                let Some(rate) = rate else { return diag.help("write the price the trade was made at") };
                let price = format!("{} {quote}", shown(rate));
                let diag =
                    diag.help(format!("if the trade was made at `@ {price}`, write it: that gives the other amount"));
                match fix {
                    Some(fixed) => diag.fix(format!("write `@ {price}`"), line, fixed),
                    None => diag,
                }
            }
        }
    }
}
