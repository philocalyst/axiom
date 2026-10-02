//! A flow's purpose when its line does not write one: what the two ends it runs between say it is for.
//!
//! Ordinary flows and the flows of a contract's terms ask the same question, so it is answered once. A party
//! classifies the flows it is the source of by its own purpose, or its kind's `pays`; the flows it is the
//! destination of by its kind's `purpose`; an account kind can take a purpose it receives as another.

use axiom_core::{Diagnostic, Id, Key, Loc};

use crate::book::{Book, Commodity, FlowSide, Place, Purpose, Role};
use crate::builtin;
use crate::declare::World;
use crate::holders::Holder;
use crate::journal::{Provenance, Purposed};
use crate::resolve::End;

/// A purpose, and the line that says it.
#[derive(Clone, Copy)]
struct PurposeEvidence {
    purposed: Purposed,
    loc: Loc,
}

/// The purpose a flow between `from` and `to` has: the one written, or the one its ends say. None when nothing
/// classifies it; an error, said, when the sources disagree, with both places they were declared.
pub(super) fn infer_for_flow(
    world: &World<'_>,
    from: End,
    to: End,
    written: Option<(Purposed, Loc)>,
    loc: Loc,
    diags: &mut Vec<Diagnostic>,
) -> Result<Option<Purposed>, ()> {
    let from_purpose = endpoint_purpose(world, from, FlowSide::Out);
    let to_purpose = endpoint_purpose(world, to, FlowSide::Arrive);
    if let (Some(from), Some(to)) = (from_purpose, to_purpose)
        && !same_purpose(world, from.purposed, to.purposed)
    {
        diags.push(purpose_disagreement(world, loc, from, to));
        return Err(());
    }
    let inferred = from_purpose.or(to_purpose).map(|source| taken_purpose(world, to.place, source).unwrap_or(source));
    if let (Some((written, written_loc)), Some(inferred)) = (written, inferred)
        && !same_purpose(world, written, inferred.purposed)
    {
        diags.push(purpose_disagreement(world, loc, PurposeEvidence { purposed: written, loc: written_loc }, inferred));
        return Err(());
    }
    Ok(written.map(|(purpose, _)| purpose).or(inferred.map(|source| source.purposed)))
}

/// A purpose that a thing or its kinds say, the line that says it, and who says it.
fn said_purpose(book: &Book, key: Key<Id<Purpose>>, thing: Holder) -> Option<(Id<Purpose>, Loc, Holder)> {
    let (purpose, by) = book.saying(key, thing)?;
    Some((purpose, book.site(by, key.slot(), 0).unwrap_or_default(), by))
}

/// What one end of a flow says the flow is for: the party at it classifies flows through its own purpose or the
/// applicable purpose of its kind. A commodity issuer contributes `pays`; an account recipient can transform that
/// source through `takes`.
fn endpoint_purpose(world: &World<'_>, end: End, side: FlowSide) -> Option<PurposeEvidence> {
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
        return Some(PurposeEvidence {
            purposed: Purposed { purpose, of: None, source: Provenance::Entity(entity) },
            loc,
        });
    }
    let (purpose, loc, _) =
        if side == FlowSide::Out { by(builtin::PAYS).or_else(|| by(builtin::PURPOSE)) } else { by(builtin::PURPOSE) }?;
    Some(PurposeEvidence { purposed: Purposed { purpose, of: None, source: Provenance::Party(kind) }, loc })
}

/// What a commodity's issuer says what it pays is: its kind's `pays`, said by the outermost kind that says it.
fn issuer_purpose(book: &Book, unit: Id<Commodity>) -> Option<PurposeEvidence> {
    let (purpose, loc, by) = said_purpose(book, builtin::PAYS, Holder::Kind(book.commodities[unit].kind))?;
    let Holder::Kind(kind) = by else { unreachable!("a kind says it") };
    Some(PurposeEvidence { purposed: Purposed { purpose, of: None, source: Provenance::Commodity(kind) }, loc })
}

/// The purpose an account kind takes `source` as, if it takes it as another.
fn taken_purpose(world: &World<'_>, destination: Id<Place>, source: PurposeEvidence) -> Option<PurposeEvidence> {
    let book = &world.book;
    if !matches!(book.places[destination].role, Role::Account { .. }) {
        return None;
    }
    let kind = book.places[destination].kind;
    let from = source.purposed.purpose;
    let (to, by) = book.take(kind, from)?;
    let loc = book.site(by, builtin::TAKES.slot(), from.index() as u32).unwrap_or_default();
    Some(PurposeEvidence {
        purposed: Purposed { purpose: to, of: source.purposed.of, source: Provenance::Account(kind) },
        loc,
    })
}

fn same_purpose(world: &World<'_>, left: Purposed, right: Purposed) -> bool {
    let related = world.book.purposes.covers(left.purpose, right.purpose)
        || world.book.purposes.covers(right.purpose, left.purpose);
    let object_compatible = match (left.of, right.of) {
        (Some(left), Some(right)) => left == right,
        // An unqualified purpose carries no object fact to contradict an
        // explicit `of` target from another source.
        _ => true,
    };
    related && object_compatible
}

fn purpose_disagreement(world: &World<'_>, loc: Loc, first: PurposeEvidence, second: PurposeEvidence) -> Diagnostic {
    Diagnostic::error("purpose-disagreement", "this flow's purpose sources disagree")
        .label(first.loc, purpose_evidence_label(world, first))
        .label(second.loc, purpose_evidence_label(world, second))
        .label(loc, "these sources classify the same flow differently")
}

fn purpose_evidence_label(world: &World<'_>, evidence: PurposeEvidence) -> String {
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
