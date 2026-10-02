//! Turning what someone typed into ids, or into a diagnostic that helps.

use axiom_core::diag::closest;
use axiom_core::{Diagnostic, Id};
use axiom_model::{Asset, Book, Contract, Entity, Miss, Place};

/// How to speak about one kind of name in errors.
struct Noun {
    word: &'static str,
    unknown: &'static str,
    ambiguous: &'static str,
}

const PLACE: Noun = Noun { word: "place", unknown: "unknown-place", ambiguous: "ambiguous-place" };
const ENTITY: Noun = Noun { word: "entity", unknown: "unknown-entity", ambiguous: "ambiguous-entity" };

pub fn place(book: &Book, text: &str) -> Result<Id<Place>, Diagnostic> {
    book.place(text).map_err(|miss| place_miss(book, text, miss))
}

pub fn entity(book: &Book, text: &str) -> Result<Id<Entity>, Diagnostic> {
    book.entity(text).map_err(|miss| explain(book, &ENTITY, text, miss, |id| book.entities[id].path))
}

/// Why `text` is not one entity.
pub fn entity_miss(book: &Book, text: &str, miss: Miss<Entity>) -> Diagnostic {
    explain(book, &ENTITY, text, miss, |id| book.entities[id].path)
}

pub fn contract(book: &Book, text: &str) -> Result<Id<Contract>, Diagnostic> {
    book.contract(text).ok_or_else(|| {
        nothing_named("contract", text, book.contracts.iter().map(|(_, contract)| book.name(contract.name)))
    })
}

pub fn asset(book: &Book, text: &str) -> Result<Id<Asset>, Diagnostic> {
    book.asset(text)
        .ok_or_else(|| nothing_named("asset", text, book.assets.iter().map(|(_, asset)| book.name(asset.name))))
}

/// Why `text` is not one place.
pub fn place_miss(book: &Book, text: &str, miss: Miss<Place>) -> Diagnostic {
    explain(book, &PLACE, text, miss, |id| book.places[id].path)
}

/// The error for a name that matched nothing among `candidates`, with the
/// closest candidate as a suggestion.
pub fn nothing_named<'a>(word: &str, text: &str, candidates: impl IntoIterator<Item = &'a str>) -> Diagnostic {
    let error = Diagnostic::error("unknown-target", format!("no {word} named `{text}`"));
    match closest(text, candidates) {
        Some(near) => error.help(format!("did you mean `{near}`?")),
        None => error,
    }
}

fn explain<T>(
    book: &Book,
    noun: &Noun,
    text: &str,
    miss: Miss<T>,
    name_of: impl Fn(Id<T>) -> axiom_core::Sym,
) -> Diagnostic {
    match miss {
        Miss::Unknown { suggestion } => {
            let error = Diagnostic::error(noun.unknown, format!("unknown {} `{text}`", noun.word));
            match suggestion {
                Some(near) => error.help(format!("did you mean `{}`?", book.name(near))),
                None => error,
            }
        }
        Miss::Ambiguous(candidates) => {
            let names: Vec<String> = candidates.iter().map(|&id| format!("`{}`", book.name(name_of(id)))).collect();
            Diagnostic::error(noun.ambiguous, format!("`{text}` could be {}", names.join(", ")))
                .help(format!("write more of the {}'s path", noun.word))
        }
    }
}
