//! What a law's trigger tells it: the variables it may read, and their types.

use axiom_syntax::Trigger as Written;

use crate::law::{Ty, Var};

/// The moment an expression runs. Most are a trigger; the expression after
/// `by` runs before the deadline it computes is known.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum When {
    Deadline,
    In,
    Out,
    Gain,
    Spend,
    Each,
    By,
    Always,
}

impl When {
    pub const TRIGGERS: [When; 7] = [When::In, When::Out, When::Gain, When::Spend, When::Each, When::By, When::Always];

    pub fn of(trigger: &Written) -> When {
        match trigger {
            Written::In => When::In,
            Written::Out => When::Out,
            Written::Gain => When::Gain,
            Written::Spend => When::Spend,
            Written::Each(_) | Written::Closing { .. } => When::Each,
            Written::By(_) => When::By,
            Written::Always => When::Always,
        }
    }

    /// How a law is written when it has this trigger.
    pub fn phrase(self) -> &'static str {
        match self {
            When::Deadline | When::By => "`by`",
            When::In => "`on in`",
            When::Out => "`on out`",
            When::Gain => "`on gain`",
            When::Spend => "`on spend`",
            When::Each => "`each month` or `each year`",
            When::Always => "`always`",
        }
    }
}

const WORDS: [(Var, &str); 16] = [
    (Var::Amount, "amount"),
    (Var::From, "from"),
    (Var::To, "to"),
    (Var::Payee, "payee"),
    (Var::Date, "date"),
    (Var::Year, "year"),
    (Var::Month, "month"),
    (Var::Subject, "self"),
    (Var::Owner, "owner"),
    (Var::Gain, "gain"),
    (Var::Proceeds, "proceeds"),
    (Var::Basis, "basis"),
    (Var::Held, "held"),
    (Var::Balance, "balance"),
    (Var::Remaining, "remaining"),
    (Var::Flow, "flow"),
];

impl Var {
    pub(crate) fn parse(word: &str) -> Option<Var> {
        WORDS.iter().find(|entry| entry.1 == word).map(|entry| entry.0)
    }

    pub(crate) fn words() -> impl Iterator<Item = &'static str> {
        WORDS.iter().map(|entry| entry.1)
    }

    /// Whether an expression running at `when` may read this variable.
    pub(crate) fn provided_by(self, when: When) -> bool {
        let flow = matches!(when, When::In | When::Out | When::Gain | When::Spend);
        match self {
            Var::Subject | Var::Owner => true,
            Var::Date | Var::Year | Var::Month => when != When::Deadline,
            Var::Amount | Var::From | Var::To | Var::Flow => flow,
            // v3 bridge: no v3 trigger knows what a flow is for.
            Var::Purpose | Var::Description => false,
            Var::Payee => flow && when != When::Gain,
            Var::Gain | Var::Proceeds | Var::Basis | Var::Held => when == When::Gain,
            Var::Balance => when == When::Always,
            Var::Remaining => matches!(when, When::Spend | When::Each | When::By),
        }
    }

    /// The triggers that supply it.
    pub(crate) fn suppliers(self) -> impl Iterator<Item = When> {
        When::TRIGGERS.into_iter().filter(move |&when| self.provided_by(when))
    }

    /// Its type, given what `self` is in this law.
    pub(crate) fn ty(self, subject: Ty) -> Ty {
        match self {
            Var::Amount | Var::Gain | Var::Proceeds | Var::Basis | Var::Balance | Var::Remaining => Ty::Amount,
            Var::From | Var::To => Ty::Place,
            Var::Payee | Var::Owner => Ty::Entity,
            Var::Date => Ty::Day,
            Var::Year | Var::Month => Ty::Num,
            Var::Held => Ty::Span,
            Var::Subject => subject,
            Var::Flow => Ty::Flow,
            Var::Purpose => Ty::Purpose,
            Var::Description => Ty::Text,
        }
    }
}
