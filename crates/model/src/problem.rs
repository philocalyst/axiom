//! What the model says is wrong, for the problems that come in families.
//!
//! A name nothing answers to, a name several things answer to, a thing declared twice, something written twice:
//! each is said the same way wherever it is found, so each is one function here, and its words are written once.
//! A caller decides that something is wrong and says what; it never words it. One-off diagnostics stay where
//! they arise, for the catalog is for families.

use axiom_core::{Diagnostic, Id, Interner, Loc, Sym, Tree};

use crate::book::{Miss, System};
use crate::errors::{Candidate, Word, article};
use crate::names::{Names, Scoped};
use crate::scope::Home;

/// What a name names, for the sentence a diagnostic says about it.
#[derive(Clone, Copy)]
pub(crate) enum Noun {
    Account,
    Asset,
    Commodity,
    Contract,
    Entity,
    Format,
    Input,
    Kind,
    Law,
    Owner,
    Param,
    Pattern,
    Place,
    Purpose,
    Sync,
    System,
}

impl Noun {
    /// The noun as a sentence says it, and its plural. A place is called an account in the plural because
    /// that is what a reader has written.
    const fn words(self) -> (&'static str, &'static str) {
        match self {
            Noun::Account => ("account", "accounts"),
            Noun::Asset => ("asset", "assets"),
            Noun::Commodity => ("commodity", "commodities"),
            Noun::Contract => ("contract", "contracts"),
            Noun::Entity => ("entity", "entities"),
            Noun::Format => ("format", "formats"),
            Noun::Input => ("input", "inputs"),
            Noun::Kind => ("kind", "kinds"),
            Noun::Law => ("law", "laws"),
            Noun::Owner => ("owner", "owners"),
            Noun::Param => ("param", "params"),
            Noun::Pattern => ("pattern", "patterns"),
            Noun::Place => ("place", "accounts"),
            Noun::Purpose => ("purpose", "purposes"),
            Noun::Sync => ("sync", "syncs"),
            Noun::System => ("system", "systems"),
        }
    }
}

/// What a code is asked to name, which decides how a failure to name it is worded.
#[derive(Clone, Copy)]
pub(crate) enum CodeUse {
    /// `against ^code`: the flow this one settles.
    Against,
    /// A claim waiver, which has to identify the transaction that made the claim.
    ClaimWaiver,
}

/// What a name is looked up among: the index that answers to names, the interner that spells them, and the
/// systems that declare some of what is indexed. They travel together to every diagnostic about a lookup.
pub(crate) struct Among<'a, 's, T> {
    pub index: &'a Scoped<T>,
    pub names: &'a Interner<'s>,
    pub systems: &'a Tree<System>,
}

impl<'s, T> Among<'_, 's, T> {
    /// The path of the system that declared `id`; the project and the built-ins belong to none.
    pub fn system_of(&self, id: Id<T>) -> Option<&'s str> {
        match self.index.home(id) {
            Home::System(system) => Some(self.names.name(self.systems[system].path)),
            Home::Project | Home::Builtin => None,
        }
    }

    /// `unknown`, and the systems that declare the name without being used.
    pub fn unknown(&self, noun: Noun, word: Word, nearest: Option<&str>) -> Diagnostic {
        let mut diagnostic = unknown(noun, word, nearest);
        for &hidden in self.index.names.candidates(self.names, word.text) {
            if let Some(system) = self.system_of(hidden) {
                diagnostic = diagnostic
                    .note(format!(
                        "the {} `{}` is declared by system `{system}`, which is not used here",
                        noun.words().0,
                        word.text
                    ))
                    .help(format!("add `use {system}` to bring it into scope"));
            }
        }
        diagnostic
    }

    /// Why `word` names no single thing: nothing answers to it, or the things `candidates` describes do.
    pub fn failed(
        &self,
        miss: Miss<T>,
        noun: Noun,
        word: Word,
        candidates: impl FnOnce(&[Id<T>]) -> Vec<Candidate>,
    ) -> Diagnostic {
        match miss {
            Miss::Unknown { suggestion } => self.unknown(noun, word, suggestion.map(|sym| self.names.name(sym))),
            Miss::Ambiguous(ids) => ambiguous(noun, word, &candidates(&ids)),
        }
    }
}

/// The things an ambiguous suffix could mean, each with the shortest written form that means only it.
pub(crate) fn shortest<T>(
    names: &Interner,
    table: &Names<T>,
    ids: &[Id<T>],
    path: impl Fn(Id<T>) -> Sym,
    declared: impl Fn(Id<T>) -> Option<Loc>,
) -> Vec<Candidate> {
    let describe = |&id: &Id<T>| {
        let full = names.name(path(id));
        let write = table.shortest_unique(names, full, id).to_string();
        Candidate { is: format!("`{full}`"), declared: declared(id), write: Some(write) }
    };
    ids.iter().map(describe).collect()
}

/// `there is no place `chekcing``, with the closest known name as the fix.
pub(crate) fn unknown(noun: Noun, word: Word, nearest: Option<&str>) -> Diagnostic {
    let name = noun.words().0;
    let diagnostic = Diagnostic::error(format!("unknown-{name}"), format!("there is no {name} `{}`", word.text))
        .label(word.loc, format!("not a known {name}"));
    match nearest {
        Some(near) => diagnostic.fix(format!("did you mean `{near}`?"), word.loc, near),
        None => diagnostic,
    }
}

/// A name several things answer to: where each is declared and, if there is one, how to write only it.
pub(crate) fn ambiguous(noun: Noun, word: Word, candidates: &[Candidate]) -> Diagnostic {
    let (which, name, plural) =
        (if candidates.len() == 2 { "either of these" } else { "any of these" }, noun.words().0, noun.words().1);
    let mut diagnostic =
        Diagnostic::error(format!("ambiguous-{name}"), format!("`{}` could be {which} {plural}", word.text))
            .label(word.loc, "which one is meant?");
    for candidate in candidates {
        if let Some(loc) = candidate.declared {
            diagnostic = diagnostic.context(loc, format!("{} is declared here", candidate.is));
        }
        diagnostic = match &candidate.write {
            Some(write) => diagnostic.fix(format!("write `{write}` for {}", candidate.is), word.loc, write),
            None => {
                // Candidates that share a spelling would say the same thing once each.
                let note = format!("{} cannot be written any other way: rename it to tell them apart", candidate.is);
                if diagnostic.notes.contains(&note) { diagnostic } else { diagnostic.note(note) }
            }
        };
    }
    diagnostic
}

/// A thing declared again, after `first` or, with no first, because it is built in.
pub(crate) fn duplicate(noun: Noun, word: Word, first: Option<Loc>) -> Diagnostic {
    let (name, text) = (noun.words().0, word.text);
    let code = format!("duplicate-{name}");
    match first {
        Some(first) => Diagnostic::error(code, format!("{name} `{text}` is declared twice"))
            .label(word.loc, "declared again here")
            .context(first, "first declared here")
            .help("keep the declaration you mean and delete the other"),
        None => Diagnostic::error(code, format!("{name} `{text}` is built in"))
            .label(word.loc, "declared again here")
            .help("delete this declaration"),
    }
}

/// Something that may be written once, `what`, written again at `again` after `first`.
pub(crate) fn twice(what: &str, again: Loc, first: Loc) -> Diagnostic {
    Diagnostic::error(format!("duplicate-{}", what.replace(' ', "-")), format!("{} is written twice", article(what)))
        .label(again, "written again here")
        .context(first, "first written here")
}

/// A name in a tree of names that every node must give a parent, declared with none: `kind x` and not `kind x : entity`.
pub(crate) fn parentless(noun: Noun, word: Word, question: &str, roots: &[&str]) -> Diagnostic {
    let name = noun.words().0;
    let roots: Vec<String> = roots.iter().map(|root| format!("`: {root}`")).collect();
    Diagnostic::error(format!("{name}-parent"), format!("{name} `{}` needs a parent", word.text))
        .label(word.loc, question)
        .help(format!("write {}, or another {name}", roots.join(", ")))
}

/// A chain of parents that never reaches a root, as the names on it with where each is declared, in the order each
/// inherits from the next.
pub(crate) fn cycle(noun: Noun, route: &[(&str, Option<Loc>)], root: &str) -> Diagnostic {
    let name = noun.words().0;
    let chain: Vec<&str> = route.iter().chain(&route[..1]).map(|&(member, _)| member).collect();
    let mut diagnostic =
        Diagnostic::error(format!("{name}-cycle"), format!("{name} `{}` inherits from itself", chain[0]))
            .note(format!("the chain is {}", chain.join(" -> ")))
            .help(format!("give one of them a parent outside the loop, such as a root {name} like `{root}`"));
    if let Some(loc) = route[0].1 {
        diagnostic = diagnostic.label(loc, "its parent chain never reaches a root");
    }
    for (&(member, declared), parent) in route.iter().zip(&chain[1..]).skip(1) {
        if let Some(loc) = declared {
            diagnostic = diagnostic.context(loc, format!("`{member}` inherits from `{parent}` here"));
        }
    }
    diagnostic
}

/// A property a kind may not declare because every kind has it.
pub(crate) fn built_in_property(word: Word) -> Diagnostic {
    Diagnostic::error("reserved-property", format!("`{}` is a built-in property", word.text))
        .label(word.loc, "choose another name")
        .note("built-in properties keep their meaning everywhere, so a kind cannot redefine them")
}

pub(crate) fn unknown_code(used: CodeUse, code: &str, at: Loc) -> Diagnostic {
    match used {
        CodeUse::Against => Diagnostic::error("unknown-against", "this code names no earlier transaction")
            .label(at, format!("`{code}` has not named a transaction yet"))
            .help("put this code on an earlier transaction or one of its flows"),
        CodeUse::ClaimWaiver => {
            Diagnostic::error("unknown-claim-reference", "this code identifies no earlier claim transaction")
                .label(at, "no prior transaction has this code")
                .help("put the code on the earlier `owes` transaction")
        }
    }
}

pub(crate) fn ambiguous_code(used: CodeUse, code: &str, at: Loc, first: Loc, second: Loc) -> Diagnostic {
    let (diagnostic, help) = match used {
        CodeUse::Against => (
            Diagnostic::error("ambiguous-against", "this code names more than one earlier transaction")
                .label(at, format!("`{code}` is not a unique transaction reference")),
            "give the original transaction a code used nowhere else",
        ),
        CodeUse::ClaimWaiver => (
            Diagnostic::error("ambiguous-claim-reference", "this code identifies more than one earlier transaction")
                .label(at, "a claim waiver must identify one source transaction"),
            "use a code that appears on only one earlier transaction",
        ),
    };
    diagnostic
        .label(first, "one matching transaction is here")
        .label(second, "another matching transaction is here")
        .help(help)
}

#[cfg(test)]
mod tests {
    use axiom_core::FileId;

    use super::*;

    fn at(start: u32) -> Loc {
        Loc::new(FileId(0), start, start + 3)
    }

    fn word(text: &str) -> Word<'_> {
        Word { text, loc: at(0) }
    }

    #[test]
    fn a_noun_gives_each_family_its_own_code_and_words() {
        let several = ambiguous(Noun::Place, word("x"), &[]);
        assert_eq!((&*several.code, &*several.message), ("ambiguous-place", "`x` could be any of these accounts"));
        assert_eq!(duplicate(Noun::Pattern, word("p"), Some(at(9))).code, "duplicate-pattern");
        assert_eq!(duplicate(Noun::Asset, word("car"), Some(at(9))).message, "asset `car` is declared twice");
    }

    #[test]
    fn a_declaration_of_a_built_in_is_told_to_go() {
        let diagnostic = duplicate(Noun::Kind, word("asset"), None);

        assert_eq!(diagnostic.message, "kind `asset` is built in");
        assert_eq!(diagnostic.help[0].text, "delete this declaration");
    }

    #[test]
    fn something_written_twice_points_at_both_and_tells_each_what_it_is() {
        let diagnostic = twice("prepayment rule", at(20), at(4));

        assert_eq!(
            (&*diagnostic.code, &*diagnostic.message),
            ("duplicate-prepayment-rule", "a prepayment rule is written twice")
        );
        assert_eq!((diagnostic.labels[0].loc, diagnostic.labels[0].primary), (at(20), true));
        assert_eq!(diagnostic.labels[0].text, "written again here");
        assert_eq!((diagnostic.labels[1].loc, diagnostic.labels[1].primary), (at(4), false));
        assert_eq!(diagnostic.labels[1].text, "first written here");
        assert_eq!(twice("area", at(1), at(0)).message, "an area is written twice");
    }

    #[test]
    fn a_code_that_names_two_transactions_is_worded_for_what_it_was_asked_to_name() {
        let ambiguous = |used| ambiguous_code(used, "inv", at(0), at(5), at(9));

        assert_eq!(ambiguous(CodeUse::Against).code, "ambiguous-against");
        let waiver = ambiguous(CodeUse::ClaimWaiver);
        assert_eq!(waiver.code, "ambiguous-claim-reference");
        assert_eq!(waiver.labels.len(), 3);
        assert_eq!(waiver.help[0].text, "use a code that appears on only one earlier transaction");
    }
}
