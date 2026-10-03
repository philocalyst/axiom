//! What the model says is wrong, for the problems that come in families.
//!
//! A name nothing answers to, a name several things answer to, a thing declared twice, something written twice:
//! each is said the same way wherever it is found, so each is one function here, and its words are written once.
//! A caller decides that something is wrong and says what; it never words it. One-off diagnostics stay where
//! they arise, for the catalog is for families.

use axiom_core::diag::closest;
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

/// A slot declared twice in one kind.
pub(crate) fn slot_twice(name: &str, again: Loc, first: Loc) -> Diagnostic {
    Diagnostic::error("duplicate-property-declaration", format!("property `{name}` is declared twice on this kind"))
        .label(again, "declared again here")
        .context(first, "first declared here")
}

/// A slot that takes one sort of value in one kind and another in another: laws read `.name` without knowing the kind.
pub(crate) fn slot_type(name: &str, now: &str, then: &str, loc: Loc, first: Loc) -> Diagnostic {
    Diagnostic::error("property-type", format!("`{name}` takes {now} here, but {then} elsewhere"))
        .label(loc, format!("takes {now} here"))
        .context(first, format!("takes {then} here"))
        .note(format!("a slot takes one sort of value wherever it is declared, so that `.{name}` means the same thing wherever it is written"))
}

/// What a slot repeated beneath its first declaration takes more of.
#[derive(Clone, Copy)]
pub(crate) enum Widening {
    Range,
    Count,
    Weight,
}

/// A kind repeats a slot of one above it and takes more than it does; `narrowed` is the line that would not.
pub(crate) fn slot_widening(name: &str, how: Widening, narrowed: &str, loc: Loc, above: Loc) -> Diagnostic {
    let wider = match how {
        Widening::Range => "takes things the slot above does not",
        Widening::Count => "takes more values than the slot above",
        Widening::Weight => "weighs its values differently from the slot above",
    };
    Diagnostic::error("slot-widening", format!("`{name}` is wider here than the slot it repeats"))
        .label(loc, wider)
        .context(above, "the slot above is declared here")
        .note("a kind may repeat a slot of the kinds above it only to narrow it: fewer kinds or words, or a tighter count")
        .fix("narrow it to what the slot above takes", loc, narrowed)
}

/// `entity` or `name` as a slot's whole range, which takes anything of its sort; `proposal` is what the book's own
/// things say the slot takes, if they say.
pub(crate) fn untyped_slot(slot: Word, wide: Word, proposal: Option<&str>) -> Diagnostic {
    let what = if wide.text == "name" { "word" } else { "entity" };
    let diagnostic = Diagnostic::error("untyped-slot", format!("`{}` takes any {what}", slot.text))
        .label(wide.loc, format!("every {what} is one"))
        .note("a slot takes the kinds of thing it is for, or the words it knows, so that a wrong one is an error where it is written");
    match proposal {
        Some(text) => diagnostic.fix(format!("write `{text}`, which is what the book fills it with"), wide.loc, text),
        None => diagnostic.help(format!(
            "name the kinds it takes, as in `has {0} person | household`, or the words, as in `has {0} one of a | b`",
            slot.text
        )),
    }
}

/// A range of kinds of different sorts of thing: `person | 401k`.
pub(crate) fn mixed_sorts(word: Word, kind: &str) -> Diagnostic {
    Diagnostic::error(
        "slot-range-sorts",
        format!("`{kind}` is not the sort of thing the other kinds of this range are"),
    )
    .label(word.loc, "a different sort of thing")
    .note("a slot takes things of one sort: people and households, or accounts, but not both")
}

/// A weight on a slot that takes one value, which there is nothing to weigh against.
pub(crate) fn weighted_one(slot: Word, line: Loc) -> Diagnostic {
    Diagnostic::error("weight-on-one", format!("`{}` takes one value, so there is nothing to weigh", slot.text))
        .label(line, "only a slot that is `some` or `many` is weighed")
        .help("write `some` or `many` before `by`, or remove the weight")
}

/// A value that is not of a kind the slot takes: `found` says what it is, `takes` what the slot takes, and `fitting`
/// are the things of a kind that fits, the closest of which is the fix.
pub(crate) fn wrong_kind(slot: &str, value: Word, found: &str, takes: &str, fitting: &[&str]) -> Diagnostic {
    let diagnostic =
        Diagnostic::error("wrong-kind", format!("`{}` is {found}, and `{slot}` takes {takes}", value.text))
            .label(value.loc, format!("{found}, not {takes}"));
    match (closest(value.text, fitting.iter().copied()), fitting) {
        (Some(near), _) => diagnostic.fix(format!("did you mean `{near}`?"), value.loc, near),
        (None, [only]) => diagnostic.fix(format!("`{only}` is the only one that fits"), value.loc, *only),
        (None, []) => diagnostic.help(format!("declare {takes}, and write its name: `{slot} NAME`")),
        (None, some) => diagnostic
            .note(format!("it could be {}", list_names(&some[..some.len().min(5)])))
            .help(format!("write one of them, as in `{slot} {}`", some[0])),
    }
}

/// A word that is not one of the words a slot takes.
pub(crate) fn wrong_word(slot: &str, value: Word, words: &[&str]) -> Diagnostic {
    let diagnostic =
        Diagnostic::error("wrong-word", format!("`{}` is not one of the words `{slot}` takes", value.text))
            .label(value.loc, format!("not {}", list_names(words)))
            .note(format!("`{slot}` takes one of {}", list_names(words)));
    match (closest(value.text, words.iter().copied()), words.first()) {
        (Some(near), _) => diagnostic.fix(format!("did you mean `{near}`?"), value.loc, near),
        (None, Some(first)) => diagnostic.help(format!("write one of them, as in `{slot} {first}`")),
        (None, None) => diagnostic,
    }
}

/// `a`, `b` or `c`, each in backticks.
fn list_names(names: &[&str]) -> String {
    crate::errors::list(names)
}

/// A line that gives a slot of one value more than one; `remove` is the extra values and what separates them.
pub(crate) fn too_many(slot: &str, given: usize, extra: Loc, remove: Loc) -> Diagnostic {
    Diagnostic::error("too-many", format!("`{slot}` takes one value, and this line gives {given}"))
        .label(extra, "more than it takes")
        .note(format!("to take several, declare the slot `some` or `many`, as in `has {slot} person many`"))
        .fix("keep the first", remove, "")
}

/// A slot of one value filled by two lines.
pub(crate) fn filled_twice(slot: &str, again: Loc, first: Loc) -> Diagnostic {
    Diagnostic::error("too-many", format!("`{slot}` takes one value, and it is filled twice"))
        .label(again, "filled again here")
        .context(first, "first filled here")
        .help("keep the line you mean and delete the other")
}

/// A value of a slot that weighs its values, with no weight after it.
pub(crate) fn missing_weight(slot: &str, weight: &str, last: Loc) -> Diagnostic {
    Diagnostic::error("missing-weight", format!("`{slot}` is weighed by {weight}, and this value has none"))
        .label(last, format!("write its {weight} after it"))
        .help(format!("each value takes its weight: `{slot} dana 60%, theo 40%`"))
}

/// A weight that is not a rate, or not an amount of the commodity the slot weighs in.
pub(crate) fn weight_type(slot: &str, unit: Option<&str>, loc: Loc) -> Diagnostic {
    let (message, want) = match unit {
        Some(unit) => (format!("`{slot}` is weighed in {unit}"), format!("an amount in {unit}, such as `100 {unit}`")),
        None => (
            format!("`{slot}` is weighed by a rate"),
            "a percentage, a fraction or a number, such as `60%`".to_string(),
        ),
    };
    Diagnostic::error("weight-type", message).label(loc, "not a weight").help(format!("write {want}"))
}

/// A required slot that no line of the thing or of a kind above it fills. `candidates` are the things that fit; with
/// exactly one, the line that fills the slot with it is the fix, written at `insert`.
pub(crate) fn missing_role(
    thing: Word,
    kind: &str,
    slot: &str,
    takes: &str,
    candidates: &[&str],
    insert: Loc,
) -> Diagnostic {
    let diagnostic = Diagnostic::error("missing-role", format!("`{}` has no `{slot}`", thing.text))
        .label(thing.loc, format!("a {kind} takes {takes} as its `{slot}`"))
        .note("a slot that is not `optional` is filled by the thing or by its kind");
    match candidates {
        [only] => diagnostic.fix(format!("fill `{slot}` with `{only}`"), insert, format!("\n  {slot} {only}")),
        [] => diagnostic.help(format!("add a line giving {takes}: `{slot} VALUE`")),
        some => diagnostic
            .note(format!("it could be {}", list_names(&some[..some.len().min(5)])))
            .help(format!("add a line giving {takes}, as in `{slot} {}`", some[0])),
    }
}

/// A slot declared under a thing and not under its kind.
pub(crate) fn slot_on_a_thing(line: Loc, noun: &str) -> Diagnostic {
    Diagnostic::error("unknown-property", format!("`has` is not a property of {}", article(noun)))
        .label(line, "only a kind declares slots")
        .help("write the slot under the kind this one is of")
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

/// What a split cannot do with its total.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Unbalanced {
    /// The parts add to less than the total, and none of them is the remainder.
    Short,
    /// The parts take more than there is.
    Over,
    /// The parts take all of it, and a leg in another commodity has nothing left to be exchanged for.
    Unfunded,
}

/// A split whose parts cannot add up to what it says it moves: `parts` are what takes from it, each where it is
/// written and what it takes. A split that cannot conserve is an error in the book, not money that appears.
pub(crate) fn split_imbalance(
    how: Unbalanced,
    what: &str,
    header: (Loc, &str),
    taken: &str,
    parts: &[(Loc, String)],
) -> Diagnostic {
    let (loc, total) = header;
    let (message, help) = match how {
        Unbalanced::Short => (
            format!("the {what} of this split come to {taken}, and its total is {total}"),
            "write `...` on the leg that takes what remains, or add the missing leg",
        ),
        Unbalanced::Over => (
            format!("the {what} of this split take {taken}, and its total is only {total}"),
            "take less, or raise the total",
        ),
        Unbalanced::Unfunded => (
            format!("the {what} of this split take {taken} of its total of {total}, and nothing is left to exchange"),
            "take less of the total for the others, or write the leg in the commodity of the total",
        ),
    };
    let mut diagnostic =
        Diagnostic::error("split-imbalance", message).label(loc, format!("the total is {total}")).help(help);
    for (at, amount) in parts {
        diagnostic = diagnostic.context(*at, format!("takes {amount}"));
    }
    diagnostic
}

/// A leg in another commodity than the total it takes from: nothing in the split balances it.
pub(crate) fn split_unit(leg: Loc, found: &str, header: (Loc, &str)) -> Diagnostic {
    let (loc, total) = header;
    Diagnostic::error("split-imbalance", format!("this leg is in {found}, and the total it takes from is {total}"))
        .label(leg, "a leg takes from the total in the total's commodity")
        .context(loc, format!("the total is {total}"))
        .help("write the leg in the commodity of the total, or leave the total out")
}

/// Two legs that both take what the others leave: `...`, or a leg in another commodity than the total, which is the
/// exchange of it.
pub(crate) fn split_remainders(leg: Loc, header: (Loc, &str)) -> Diagnostic {
    let (loc, total) = header;
    Diagnostic::error("split-imbalance", "two legs of this split take what the others leave")
        .label(leg, "this leg wants the remainder as well")
        .context(loc, format!("the total is {total}"))
        .help("a split has one remainder: `...`, or the one leg in another commodity")
}

/// Amounts of a split too large to be added: no total can be said of them.
pub(crate) fn split_overflow(loc: Loc) -> Diagnostic {
    Diagnostic::error("split-imbalance", "the amounts of this split are too large to add up")
        .label(loc, "this split's legs and items cannot be summed")
}
