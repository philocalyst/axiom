//! Statements (LANGUAGE §5): `DATE SUBJECT VERB …`.
//!
//! One line says one thing about one thing on a day, and the word after the
//! subject says what: `=` a value, `owes` a claim, `now` a change, `worked` and
//! `used` a measure, `waived`, `ends`, `settled`, `void`, `returned`, `split`,
//! `basis` and `filed` events, and no verb at all an occurrence. The rest of the
//! line is read by that word and never by what the subject's name means: `01
//! flat` is an occurrence whether or not `flat` is a contract.

use axiom_core::diag::closest;
use axiom_core::{Day, Dec, Diagnostic, Loc};

use crate::ast::*;
use crate::lex::{Punct, Tok};
use crate::lines::Line;
use crate::parser::{Parse, Parser, Scope};

const EVENT_STATES: [(&str, EventState); 3] =
    [("settled", EventState::Settled), ("void", EventState::Void), ("returned", EventState::Returned)];

/// The words that follow a subject and say what a line is.
const VERBS: [&str; 12] =
    ["owes", "now", "worked", "used", "waived", "ends", "settled", "void", "returned", "split", "basis", "filed"];

pub(crate) const BUDGET_PERIODS: [(&str, Period); 2] = [("monthly", Period::Month), ("yearly", Period::Year)];

impl<'s> Parser<'s> {
    /// A dated line about `subject`, whose header and lines are read here. An
    /// `amount` already read after the subject is the occurrence's own.
    pub fn statement(
        &mut self,
        line: &mut Line<'s>,
        date: Day,
        subject: Subject<'s>,
        amount: Option<Amount<'s>>,
    ) -> Parse<()> {
        let mut statement = self.said(date, subject, amount)?;
        if let (Verb::Occurrence(_), Tok::Name(_)) = (&statement.verb, self.tok()) {
            // A name where the tail starts: a flow written without its arrow.
            return Err(self.expected_arrow(true));
        }
        let header = self.end_header(line)?;
        if takes_lines(&statement.verb) {
            statement.body = self.body(line, Scope::Statement(date))?;
        }
        self.check_lines(&statement, header.loc)?;
        self.emit(&header, statement, ItemKind::Statement);
        Ok(())
    }

    /// The statement a header line says, without the lines under it.
    // Inlined: what it returns is built where it is wanted, not copied up out of a call.
    #[inline(always)]
    pub fn said(&mut self, date: Day, subject: Subject<'s>, amount: Option<Amount<'s>>) -> Parse<Statement<'s>> {
        let scope = Scope::Statement(date);
        let verb = self.verb(scope, subject, amount)?;
        let tail = self.tail(scope, self.mark::<Clause>())?;
        if !tail.is_empty() {
            for clause in self.slice(tail) {
                if !takes(&verb, &clause.kind) {
                    let diag = clause_not_taken(clause, what_it_says(&verb));
                    return self.fail(diag);
                }
            }
        }
        Ok(Statement { date, subject, verb, tail, body: Body::default() })
    }

    /// The word after the subject, and what it takes.
    fn verb(&mut self, scope: Scope, subject: Subject<'s>, amount: Option<Amount<'s>>) -> Parse<Verb<'s>> {
        // An amount read before anything else can only be an occurrence's own.
        if amount.is_some() {
            return Ok(Verb::Occurrence(amount));
        }
        match self.tok() {
            Tok::Punct(Punct::Eq) => {
                self.bump();
                if self.at(Punct::Minus) && matches!(self.lexer.peek_second().tok, Tok::Number(_)) {
                    self.signed_literal().map(|literal| Verb::Value(Amount::Literal(literal)))
                } else if self.at(Punct::Minus) {
                    let start = self.peek().loc.start as usize;
                    self.bump();
                    let value = self.expression()?;
                    let first = self.expr(value).first;
                    let root = self.node(ExprKind::Unary(UnOp::Neg, value), self.loc_from(start), first);
                    Ok(Verb::Value(Amount::Computed(root)))
                } else {
                    self.amount(scope).map(Verb::Value)
                }
            }
            Tok::Name(word) => self.word_verb(scope, subject, word),
            _ => self.occurrence(subject),
        }
    }

    /// A verb that is a word, or, when the word is none, an occurrence whose
    /// tail starts here.
    fn word_verb(&mut self, scope: Scope, subject: Subject<'s>, word: &str) -> Parse<Verb<'s>> {
        match word {
            "owes" => self.owes(scope),
            "now" => self.now(scope).map(Verb::Now),
            "worked" => self.then(Self::measured).map(Verb::Worked),
            "used" => self.then(Self::measured).map(Verb::Used),
            "waived" => Ok(self.bump_as(Verb::Waived)),
            "ends" => Ok(self.bump_as(Verb::Ends)),
            "settled" | "void" | "returned" => self
                .choose(&EVENT_STATES, "unknown-event-state", "settlement state")
                .map(|(state, _)| Verb::Event(state)),
            "split" => self.split(),
            "basis" => self.basis(scope),
            "filed" => self.filed(),
            _ => self.occurrence(subject),
        }
    }

    /// No verb: a promise kept, which only a name can be.
    fn occurrence(&mut self, subject: Subject<'s>) -> Parse<Verb<'s>> {
        if let Subject::Name(_) = subject {
            return Ok(Verb::Occurrence(None));
        }
        let token = self.peek();
        let mut diag = self.unexpected(
            token,
            "expected-verb",
            "what the line says of it: `=`, `now`, `owes`, `ends` or another verb",
        );
        if let Tok::Name(word) = token.tok {
            if let Some(near) = closest(word, VERBS) {
                diag = diag.fix(format!("did you mean `{near}`?"), token.loc, near);
            }
        }
        self.fail(diag)
    }

    /// `owes CREDITOR [AMOUNT]`, the rest of a claim being the statement's tail.
    fn owes(&mut self, scope: Scope) -> Parse<Verb<'s>> {
        self.bump();
        let creditor = self.name("expected-name", "the party or owner it is owed to")?;
        let amount = match self.tok() {
            Tok::Number(_) | Tok::Percent(_) | Tok::Fraction(..) | Tok::Name("empty") => {
                Some(self.priced_amount(scope)?)
            }
            _ => None,
        };
        Ok(Verb::Owes { creditor, amount })
    }

    /// `now` and what follows: new terms, a property, a budget, or nothing.
    fn now(&mut self, scope: Scope) -> Parse<Change<'s>> {
        self.bump();
        match self.tok() {
            Tok::Number(_) | Tok::Name("about" | "buy") => {
                self.terms(scope, false).map(|terms| Change::Terms(self.push(terms)))
            }
            Tok::Percent(_) | Tok::Fraction(..) | Tok::Punct(Punct::LParen) => {
                self.terms(scope, false).map(|terms| Change::Terms(self.push(terms)))
            }
            Tok::Code(_) if self.cadence_follows_name() => {
                self.terms(scope, false).map(|terms| Change::Terms(self.push(terms)))
            }
            Tok::Name("budget") => {
                self.bump();
                self.allowance(scope).map(|allowance| Change::Budget(self.push(allowance)))
            }
            Tok::Name(_) if self.at_cadence() || self.cadence_follows_name() => {
                self.terms(scope, false).map(|terms| Change::Terms(self.push(terms)))
            }
            Tok::Name(_) => self.prop(scope).map(Change::Property),
            Tok::Eol | Tok::Str(_) | Tok::Code(_) => Ok(Change::Amendment),
            _ => Err(self.expected("expected-change", "what changes: terms, a property, or the items of an amendment")),
        }
    }

    /// `split N for M`
    fn split(&mut self) -> Parse<Verb<'s>> {
        self.bump();
        let numerator = self.split_count("the new number of units, like `2` in `split 2 for 1`")?;
        self.expect_word("for", "expected-for", "`for` and the old number of units, like `split 2 for 1`")?;
        let denominator = self.split_count("the old number of units, like `1` in `split 2 for 1`")?;
        Ok(Verb::Split { numerator, denominator })
    }

    /// A number of units in a split: a positive number.
    fn split_count(&mut self, what: &str) -> Parse<Dec> {
        let token = self.peek();
        let Tok::Number(count) = token.tok else { return Err(self.expected("expected-number", what)) };
        if count.is_zero() {
            let diag = Diagnostic::error("bad-split", "a split cannot have zero units")
                .label(token.loc, "zero units would destroy every holding");
            return self.fail(diag);
        }
        self.bump();
        Ok(count)
    }

    /// `basis AMOUNT [since DATE]`
    fn basis(&mut self, scope: Scope) -> Parse<Verb<'s>> {
        self.bump();
        let amount = self.amount(scope)?;
        let since = match self.eat_word("since") {
            Some(_) => Some(self.date("the day the asset was acquired, like `2019-03-01`")?),
            None => None,
        };
        Ok(Verb::Basis { amount, since })
    }

    /// `filed YEAR`
    fn filed(&mut self) -> Parse<Verb<'s>> {
        self.bump();
        let token = self.peek();
        let Some(year) = self.year(token) else {
            return Err(self.expected("expected-year", "the year the return is for, like `2025`"));
        };
        self.bump();
        Ok(Verb::Filed(year))
    }

    /// `LIMIT monthly|yearly [carries] [funded from HOLDING into HOLDING]`, where
    /// a limit is an amount or `N% of #PURPOSE`.
    pub fn allowance(&mut self, scope: Scope) -> Parse<Allowance<'s>> {
        let limit = match self.tok() {
            Tok::Percent(percent) => {
                self.bump();
                self.expect_word("of", "expected-of", "`of` and the purpose it is a share of, like `10% of #income`")?;
                let Tok::Purpose(of) = self.tok() else {
                    return Err(self.expected("expected-purpose", "the purpose it is a share of, like `#income`"));
                };
                self.bump();
                Limit::Share { percent, of }
            }
            _ => Limit::Amount(self.amount(scope)?),
        };
        let (per, _) = self.choose(&BUDGET_PERIODS, "unknown-period", "budget period")?;
        let carries = self.eat_word("carries").map(|_| true);
        let funded = match self.eat_word("funded") {
            Some(_) => {
                self.keyword("from")?;
                let from = self.name("expected-name", "the account the limit is moved from, such as `checking`")?;
                self.keyword("into")?;
                let into = self.name("expected-name", "the account it is moved into, such as `envelope`")?;
                Some(Funding { from, into })
            }
            None => None,
        };
        Ok(Allowance { limit, per, carries, funded })
    }

    /// What the lines under a statement may say, and that it says something.
    fn check_lines(&mut self, statement: &Statement<'s>, header: Loc) -> Parse<()> {
        // Most statements have no lines, and say all they need to.
        let needs_lines = matches!(statement.verb, Verb::Owes { amount: None, .. } | Verb::Now(Change::Amendment));
        if !needs_lines && statement.body.legs.is_empty() && statement.body.items.is_empty() {
            return Ok(());
        }
        let (legs, items) = (self.slice(statement.body.legs), statement.body.items);
        let legs_only = matches!(statement.verb, Verb::Filed(_));
        let items_only = matches!(statement.verb, Verb::Owes { .. } | Verb::Waived | Verb::Now(Change::Amendment));
        if let (true, Some(leg)) = (items_only, legs.first()) {
            return self.fail(takes_items_only(leg.loc, what_it_says(&statement.verb)));
        }
        if let (true, false) = (legs_only, items.is_empty()) {
            let item = self.slice(items)[0].loc;
            return self.fail(takes_legs_only(item, what_it_says(&statement.verb)));
        }
        let says_nothing = match &statement.verb {
            Verb::Owes { amount, .. } => amount.is_none() && items.is_empty(),
            Verb::Now(Change::Amendment) => items.is_empty(),
            _ => false,
        };
        match says_nothing {
            true => self.fail(no_amount_or_items(header, what_it_says(&statement.verb))),
            false => Ok(()),
        }
    }

    /// The claim an opening's line states: there are no lines under it.
    pub fn check_claim(&mut self, statement: &Statement<'s>, header: Loc) -> Parse<()> {
        match statement.verb {
            Verb::Owes { amount: None, .. } => self.fail(no_amount_or_items(header, "a claim")),
            _ => Ok(()),
        }
    }
}

/// Whether the lines under a statement of this kind mean anything.
fn takes_lines(verb: &Verb<'_>) -> bool {
    match verb {
        Verb::Occurrence(_) | Verb::Owes { .. } | Verb::Waived | Verb::Filed(_) => true,
        Verb::Now(change) => matches!(change, Change::Terms(_) | Change::Amendment),
        _ => false,
    }
}

/// Whether a statement of this kind may carry a clause of that kind in its tail.
fn takes(verb: &Verb<'_>, clause: &ClauseKind<'_>) -> bool {
    use ClauseKind::*;
    match (verb, clause) {
        (_, Description(_) | Code(_)) => true,
        (
            Verb::Occurrence(_) | Verb::Owes { .. },
            Purpose(_) | For(_) | Due(_) | Against(_) | Via(_) | Basis(_) | Waive(_),
        ) => true,
        (Verb::Value(_), Via(_) | Waive(_)) => true,
        (Verb::Now(_) | Verb::Waived, Until(_)) => true,
        (Verb::Waived, Purpose(_)) => true,
        (Verb::Worked(_) | Verb::Used(_), Purpose(_) | For(_) | Against(_)) => true,
        _ => false,
    }
}

/// What a verb is, in a sentence: "a value" for `= 5 USD`.
fn what_it_says(verb: &Verb<'_>) -> &'static str {
    match verb {
        Verb::Occurrence(_) => "an occurrence",
        Verb::Value(_) => "a value",
        Verb::Owes { .. } => "a claim",
        Verb::Now(_) => "a change",
        Verb::Worked(_) => "a measure of work",
        Verb::Used(_) => "a measure of use",
        Verb::Waived => "a waiver",
        Verb::Ends => "an ending",
        Verb::Event(_) => "a settlement",
        Verb::Split { .. } => "a split",
        Verb::Basis { .. } => "a basis",
        Verb::Filed(_) => "a return",
    }
}

fn clause_not_taken(clause: &Clause<'_>, says: &str) -> Diagnostic {
    let name = crate::flow::clause_name(&clause.kind);
    Diagnostic::error("clause-not-taken", format!("{says} takes no {name}"))
        .label(clause.at, format!("not part of {says}"))
        .fix("remove it", clause.at, "")
}

fn takes_items_only(leg: Loc, says: &str) -> Diagnostic {
    Diagnostic::error("takes-items", format!("{says} is broken down in items, not legs"))
        .label(leg, "this line names an end")
        .note("items say what it is for: `3_000 USD #design \"brand refresh\"`")
}

fn takes_legs_only(item: Loc, says: &str) -> Diagnostic {
    Diagnostic::error("takes-legs", format!("{says} lists tallies, not items"))
        .label(item, "this line names no tally")
        .note("each line is a tally and its amount: `wages 124_200.00 USD`")
}

fn no_amount_or_items(header: Loc, says: &str) -> Diagnostic {
    Diagnostic::error("expected-amount", format!("{says} needs an amount, or items that make one"))
        .label(header, "no amount here, and no items under it")
        .help("write the amount on the line, or indent items below it")
}
