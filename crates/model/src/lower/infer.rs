//! What a flow is for when its line does not say, and what wins when its sources disagree.
//!
//! A flow has up to six sources of a purpose, and LANGUAGE §2 lists them in the order that decides: **first match
//! wins**. Its own line, its promise, its party (the party's own purpose, then its kind's), its commodity's kind in
//! party position, and last its accounts' kinds. [`classify`] is that list as one function over [`Provenance::rank`].
//!
//! The line and the promise are *written*: they outrank everything and are never compared with anything, so a flow
//! that has one is classified before its ends are read. Below them a flow's two ends each say what the flow is for,
//! the source's before the target's, and the better ranked wins. **Two sources of one rank that name unrelated
//! purposes are the one disagreement left**: rank cannot choose between two parties that each say what the money is
//! for. An account kind's `takes` is not a source but a rewrite of what the ends said, so it applies to an inferred
//! purpose and never to a written one.

use axiom_core::{Diagnostic, Id, Key, Loc};

use crate::book::{Book, Commodity, Place, Purpose, Role};
use crate::builtin;
use crate::declare::World;
use crate::holders::Holder;
use crate::journal::{Provenance, Purposed};
use crate::resolve::End;
use crate::split::FlowSide;

/// A purpose some source says, and the line that says it.
#[derive(Clone, Copy)]
struct Evidence {
    purposed: Purposed,
    loc: Loc,
}

impl Evidence {
    fn rank(self) -> u8 {
        self.purposed.source.rank()
    }
}

/// The purpose a flow between `from` and `to` has: the one its line or its promise wrote, else the best its ends say
/// (rewritten by the account kind that takes it as another). None when nothing classifies it; an error, said, when
/// the two ends say unrelated things at one rank.
pub(super) fn classify(
    world: &mut World<'_>,
    from: End,
    to: End,
    written: Option<Purposed>,
    loc: Loc,
) -> Result<Option<Purposed>, ()> {
    if written.is_some() {
        return Ok(written);
    }
    let said = match (said_at(world, from, FlowSide::Out), said_at(world, to, FlowSide::Arrive)) {
        (Some(source), Some(target)) => Some(outrank(world, source, target, loc)?),
        (source, target) => source.or(target),
    };
    Ok(said.map(|said| taken(world, to.place, said).unwrap_or(said).purposed))
}

/// The better ranked of what the two ends say; the source's when they are of one rank and related, where the
/// target's purpose is the same or a refinement of it.
fn outrank(world: &mut World<'_>, source: Evidence, target: Evidence, loc: Loc) -> Result<Evidence, ()> {
    if source.rank() != target.rank() {
        return Ok(if source.rank() < target.rank() { source } else { target });
    }
    if related(world, source.purposed.purpose, target.purposed.purpose) {
        return Ok(source);
    }
    world.diags.push(purpose_disagreement(world, loc, source, target));
    Err(())
}

/// Whether one purpose is the other or lies beneath it: compatible classifications at different levels of detail.
fn related(world: &World<'_>, left: Id<Purpose>, right: Id<Purpose>) -> bool {
    world.book.purposes.covers(left, right) || world.book.purposes.covers(right, left)
}

/// A purpose that a thing or its kinds say, the line that says it, and who says it.
fn said_purpose(book: &Book, key: Key<Id<Purpose>>, thing: Holder) -> Option<(Id<Purpose>, Loc, Holder)> {
    let (purpose, by) = book.saying(key, thing)?;
    Some((purpose, book.site(by, key.slot(), 0).unwrap_or_default(), by))
}

/// What one end of a flow says the flow is for: the party at it classifies flows through its own purpose or the
/// applicable purpose of its kind. A commodity issuer contributes `pays`.
fn said_at(world: &World<'_>, end: End, side: FlowSide) -> Option<Evidence> {
    let book = &world.book;
    if let Role::Issuer(unit) = book.places[end.place].role {
        return if side == FlowSide::Out { issuer_purpose(book, unit) } else { None };
    }
    let role_entity = match book.places[end.place].role {
        Role::Outside(Some(entity)) | Role::Tab(entity) => Some(entity),
        _ => None,
    };
    let entity = end.entity.or(role_entity)?;
    let kind = book.entities[entity].kind;
    let by = |key| said_purpose(book, key, Holder::Kind(kind));
    // A purpose the entity says itself comes before its kind's.
    if let Some((purpose, loc, Holder::Entity(_))) = said_purpose(book, builtin::PURPOSE, Holder::Entity(entity)) {
        return Some(Evidence { purposed: Purposed { purpose, of: None, source: Provenance::Entity(entity) }, loc });
    }
    let (purpose, loc, _) =
        if side == FlowSide::Out { by(builtin::PAYS).or_else(|| by(builtin::PURPOSE)) } else { by(builtin::PURPOSE) }?;
    Some(Evidence { purposed: Purposed { purpose, of: None, source: Provenance::Party(kind) }, loc })
}

/// What a commodity's issuer says what it pays is: its kind's `pays`, said by the outermost kind that says it.
fn issuer_purpose(book: &Book, unit: Id<Commodity>) -> Option<Evidence> {
    let (purpose, loc, by) = said_purpose(book, builtin::PAYS, Holder::Kind(book.commodities[unit].kind))?;
    let Holder::Kind(kind) = by else { unreachable!("a kind says it") };
    Some(Evidence { purposed: Purposed { purpose, of: None, source: Provenance::Commodity(kind) }, loc })
}

/// The purpose an account kind takes `source` as, if it takes it as another.
fn taken(world: &World<'_>, destination: Id<Place>, source: Evidence) -> Option<Evidence> {
    let book = &world.book;
    if !matches!(book.places[destination].role, Role::Account { .. }) {
        return None;
    }
    let kind = book.places[destination].kind;
    let from = source.purposed.purpose;
    let (to, by) = book.take(kind, from)?;
    let loc = book.site(by, builtin::TAKES.slot(), from.index() as u32).unwrap_or_default();
    Some(Evidence {
        purposed: Purposed { purpose: to, of: source.purposed.of, source: Provenance::Account(kind) },
        loc,
    })
}

fn purpose_disagreement(world: &World<'_>, loc: Loc, first: Evidence, second: Evidence) -> Diagnostic {
    Diagnostic::error("purpose-disagreement", "this flow's purpose sources disagree")
        .label(first.loc, purpose_evidence_label(world, first))
        .label(second.loc, purpose_evidence_label(world, second))
        .label(loc, "these sources classify the same flow differently")
        .help("write the purpose this flow is for on its line, as `#groceries`: a written purpose outranks both")
}

fn purpose_evidence_label(world: &World<'_>, evidence: Evidence) -> String {
    let book = &world.book;
    let purpose = book.name(book.purposes[evidence.purposed.purpose].name);
    match evidence.purposed.source {
        Provenance::Written => format!("the written purpose is `#{purpose}`"),
        Provenance::Contract(contract) => {
            format!("contract `{}` gives purpose `#{purpose}`", book.name(book.contracts[contract].name))
        }
        Provenance::Entity(entity) => {
            format!("party `{}` gives purpose `#{purpose}`", book.name(book.entities[entity].path))
        }
        Provenance::Party(kind) => {
            format!("party kind `{}` gives purpose `#{purpose}`", book.name(book.kinds[kind].name))
        }
        Provenance::Commodity(kind) => {
            format!("commodity kind `{}` gives purpose `#{purpose}`", book.name(book.kinds[kind].name))
        }
        Provenance::Account(kind) => {
            format!("account kind `{}` takes the flow as `#{purpose}`", book.name(book.kinds[kind].name))
        }
        Provenance::Derived => format!("the derived flow has purpose `#{purpose}`"),
    }
}
