//! What the model says is wrong, for the problems that come in families.
//!
//! A name nothing answers to, a name several things answer to, a thing declared twice, a code that names no
//! transaction: each is said the same way wherever it is found, so each is one function here, taking the facts
//! borrowed from the book, and its words are written once. A caller decides that something is wrong and says
//! what; it never words it. One-off diagnostics stay where they arise, for the catalog is for families.

use axiom_core::{Diagnostic, Id, Interner, Loc};

use crate::book::Miss;
use crate::errors::{Candidate, Word};

/// What a name names, for the sentence a diagnostic says about it.
#[derive(Clone, Copy)]
pub(crate) enum Noun {
    Account,
    Asset,
    BaseCommodity,
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
    VisibleFormat,
}

impl Noun {
    /// The noun as a sentence says it, and its plural. A place is called an account in the plural because
    /// that is what a reader has written.
    const fn words(self) -> (&'static str, &'static str) {
        match self {
            Noun::Account => ("account", "accounts"),
            Noun::Asset => ("asset", "assets"),
            Noun::BaseCommodity => ("base commodity", "base commodities"),
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
            Noun::VisibleFormat => ("visible format", "visible formats"),
        }
    }

    /// The noun diagnostic codes are made of: `unknown-commodity`, for a base commodity as well.
    const fn slug(self) -> &'static str {
        match self {
            Noun::BaseCommodity => "commodity",
            Noun::VisibleFormat => "format",
            other => other.words().0,
        }
    }

    /// Things that are declared share one code; those that sync and the contract machinery declare have their own.
    fn duplicate_code(self) -> String {
        match self {
            Noun::Format | Noun::Input | Noun::Pattern | Noun::Sync => format!("duplicate-{}", self.slug()),
            _ => "duplicate-declaration".to_string(),
        }
    }
}

/// A thing a system the reader has not used declares under the name that was written.
#[derive(Clone, Copy)]
pub(crate) struct Unused<'a> {
    pub name: &'a str,
    pub system: &'a str,
}

/// What a code is asked to name, which decides how a failure to name it is worded.
#[derive(Clone, Copy)]
pub(crate) enum CodeUse {
    /// `against ^code`: the flow this one settles.
    Against,
    /// A claim waiver, which has to identify the transaction that made the claim.
    ClaimWaiver,
}

/// How a diagnostic that lists what an ambiguous name could be reads: the declarations of a book say "could
/// mean", the declaration of a purpose's parent says "could name".
#[derive(Clone, Copy)]
pub(crate) enum Reads {
    Mean,
    Name,
}

/// Something written once that was written again: the code and message, and what each of the two says.
#[derive(Clone, Copy)]
pub(crate) enum Twice {
    AdditionalEnd,
    ContractArea,
    ContractDate,
    ContractDeposit,
    ContractGrace,
    ContractInput,
    ContractLoan,
    LoanPrepay,
    LoanResets,
    PropertyChange,
    SystemCurrency,
    SystemRates,
    TemplateLeg,
}

impl Twice {
    /// The code, the message, what the second one is told, and what the first one is told.
    const fn words(self) -> [&'static str; 4] {
        match self {
            Twice::AdditionalEnd => [
                "contract-occurrence-leg-duplicate",
                "this additional end is written twice",
                "keep one replacement for this end",
                "the first replacement is here",
            ],
            Twice::ContractArea => [
                "contract-area-duplicate",
                "a contract's area is declared twice",
                "remove this repeated area",
                "the first area is here",
            ],
            Twice::ContractDate => [
                "duplicate-contract-date",
                "a contract date is written twice",
                "written again here",
                "first written here",
            ],
            Twice::ContractDeposit => [
                "contract-deposit-duplicate",
                "a contract has one deposit",
                "remove this repeated deposit",
                "the first deposit is here",
            ],
            Twice::ContractGrace => [
                "duplicate-contract-grace",
                "a contract has one grace interval",
                "a second interval cannot replace the first",
                "the first interval is here",
            ],
            Twice::ContractInput => [
                "contract-input-duplicate",
                "this contract input is supplied twice",
                "remove the repeated binding",
                "the input is declared here",
            ],
            Twice::ContractLoan => [
                "duplicate-contract-loan",
                "a contract has one loan definition",
                "a second loan cannot replace the first",
                "the first loan is here",
            ],
            Twice::LoanPrepay => [
                "duplicate-loan-prepay",
                "a loan has one prepayment rule",
                "a second rule cannot replace the first",
                "the first rule is here",
            ],
            Twice::LoanResets => [
                "duplicate-loan-resets",
                "a loan has one reset rule",
                "a second reset cannot replace the first",
                "the first reset is here",
            ],
            Twice::PropertyChange => [
                "duplicate-property-change",
                "this property changes twice on the same day",
                "change written again here",
                "first change written here",
            ],
            Twice::SystemCurrency => [
                "duplicate-system-currency",
                "this system sets its currency twice",
                "currency set again here",
                "first set here",
            ],
            Twice::SystemRates => [
                "duplicate-system-rates",
                "this system sets its rate policy twice",
                "rate policy set again here",
                "first set here",
            ],
            Twice::TemplateLeg => [
                "contract-occurrence-leg-duplicate",
                "this template leg is overridden twice",
                "keep one replacement for this end",
                "the template leg is declared here",
            ],
        }
    }
}

/// Something that may be written once, written again at `again` after `first`.
pub(crate) fn twice(what: Twice, again: Loc, first: Loc) -> Diagnostic {
    let [code, message, second, earlier] = what.words();
    Diagnostic::error(code, message).label(again, second).context(first, earlier)
}

/// A property a kind may not declare because every kind has it.
pub(crate) fn built_in_property(word: Word) -> Diagnostic {
    Diagnostic::error("reserved-property", format!("`{}` is a built-in property", word.text))
        .label(word.loc, "choose another name")
        .note("built-in properties keep their meaning everywhere, so a kind cannot redefine them")
}

/// `there is no place `chekcing``, with the closest known name as the fix.
pub(crate) fn unknown(noun: Noun, word: Word, nearest: Option<&str>, unused: &[Unused]) -> Diagnostic {
    let name = noun.words().0;
    let mut diagnostic =
        Diagnostic::error(format!("unknown-{}", noun.slug()), format!("there is no {name} `{}`", word.text))
            .label(word.loc, format!("not a known {name}"));
    if let Some(near) = nearest {
        diagnostic = diagnostic.fix(format!("did you mean `{near}`?"), word.loc, near);
    }
    for Unused { name: written, system } in unused {
        diagnostic = diagnostic
            .note(format!("the {name} `{written}` is declared by system `{system}`, which is not used here"))
            .help(format!("add `use {system}` to bring it into scope"));
    }
    diagnostic
}

pub(crate) fn ambiguous(noun: Noun, word: Word, candidates: &[Candidate]) -> Diagnostic {
    let (which, plural) = (if candidates.len() == 2 { "either of these" } else { "any of these" }, noun.words().1);
    let mut diagnostic =
        Diagnostic::error(format!("ambiguous-{}", noun.slug()), format!("`{}` could be {which} {plural}", word.text))
            .label(word.loc, "which one is meant?");
    for candidate in candidates {
        if let Some(loc) = candidate.declared {
            diagnostic = diagnostic.context(loc, format!("{} is declared here", candidate.is));
        }
        diagnostic = match &candidate.write {
            Some(write) => diagnostic.fix(format!("write `{write}` for {}", candidate.is), word.loc, write),
            None => diagnostic
                .note(format!("{} cannot be written any other way: rename it to tell them apart", candidate.is)),
        };
    }
    diagnostic
}

pub(crate) fn ambiguous_name(noun: Noun, word: Word, among: &[String], reads: Reads) -> Diagnostic {
    let could = match reads {
        Reads::Mean => "mean",
        Reads::Name => "name",
    };
    Diagnostic::error(format!("ambiguous-{}", noun.slug()), format!("{} `{}` is ambiguous", noun.words().0, word.text))
        .label(word.loc, format!("could {could} {}", among.join(" or ")))
}

pub(crate) fn duplicate(noun: Noun, word: Word, first: Option<Loc>) -> Diagnostic {
    let (text, loc, noun) = (word.text, word.loc, noun.words().0);
    match first {
        Some(first) => Diagnostic::error("duplicate-declaration", format!("{noun} `{text}` is declared twice"))
            .label(loc, "declared again here")
            .context(first, "first declared here")
            .help("keep the declaration you mean and delete the other"),
        None => Diagnostic::error("duplicate-declaration", format!("{noun} `{text}` is built in"))
            .label(loc, "declared again here")
            .help("delete this declaration"),
    }
}

pub(crate) fn declared_twice(noun: Noun, word: Word, first: Option<Loc>, advice: Option<&str>) -> Diagnostic {
    let mut diagnostic =
        Diagnostic::error(noun.duplicate_code(), format!("{} `{}` is declared twice", noun.words().0, word.text))
            .label(word.loc, "declared again here");
    if let Some(first) = first {
        diagnostic = diagnostic.context(first, "first declared here");
    }
    match advice {
        Some(advice) => diagnostic.help(advice),
        None => diagnostic,
    }
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

/// Why a name a declaration wrote names no single thing: nothing answers to it, or the things `describe` says do.
pub(crate) fn unresolved<T>(
    miss: Miss<T>,
    noun: Noun,
    word: Word,
    names: &Interner,
    reads: Reads,
    describe: impl Fn(Id<T>) -> String,
) -> Diagnostic {
    match miss {
        Miss::Unknown { suggestion } => {
            let nearest = suggestion.map(|sym| names.name(sym));
            unknown(noun, word, nearest, &[])
        }
        Miss::Ambiguous(ids) => {
            let among: Vec<_> = ids.iter().map(|&id| describe(id)).collect();
            ambiguous_name(noun, word, &among, reads)
        }
    }
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
    fn an_unknown_name_offers_its_nearest_as_an_edit_and_names_the_systems_that_declare_it() {
        let unused = [Unused { name: "chekcing", system: "us" }];
        let diagnostic = unknown(Noun::Place, word("chekcing"), Some("checking"), &unused);

        assert_eq!(diagnostic.code, "unknown-place");
        assert_eq!(diagnostic.message, "there is no place `chekcing`");
        assert_eq!(diagnostic.labels[0].text, "not a known place");
        assert_eq!(diagnostic.help[0].edit, Some((at(0), "checking".to_string())));
        assert_eq!(diagnostic.notes, ["the place `chekcing` is declared by system `us`, which is not used here"]);
        assert_eq!(diagnostic.help[1].text, "add `use us` to bring it into scope");
    }

    #[test]
    fn a_noun_gives_each_family_its_own_code_and_words() {
        let several = ambiguous(Noun::Place, word("x"), &[]);
        assert_eq!((&*several.code, &*several.message), ("ambiguous-place", "`x` could be any of these accounts"));

        let base = unknown(Noun::BaseCommodity, word("ZZZ"), None, &[]);
        assert_eq!(base.code, "unknown-commodity");
        assert_eq!(declared_twice(Noun::Pattern, word("p"), Some(at(9)), None).code, "duplicate-pattern");
        assert_eq!(declared_twice(Noun::Asset, word("car"), None, None).code, "duplicate-declaration");
    }

    #[test]
    fn a_declaration_of_a_built_in_is_told_to_go() {
        let diagnostic = duplicate(Noun::Kind, word("asset"), None);

        assert_eq!(diagnostic.message, "kind `asset` is built in");
        assert_eq!(diagnostic.help[0].text, "delete this declaration");
    }

    #[test]
    fn something_written_twice_points_at_both_and_tells_each_what_it_is() {
        let diagnostic = twice(Twice::LoanPrepay, at(20), at(4));

        assert_eq!(diagnostic.message, "a loan has one prepayment rule");
        assert_eq!((diagnostic.labels[0].loc, diagnostic.labels[0].primary), (at(20), true));
        assert_eq!(diagnostic.labels[0].text, "a second rule cannot replace the first");
        assert_eq!((diagnostic.labels[1].loc, diagnostic.labels[1].primary), (at(4), false));
        assert_eq!(diagnostic.labels[1].text, "the first rule is here");
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
