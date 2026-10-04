//! Dated lines (LANGUAGE §2): `DATE SUBJECT VERB …`, and the `opening` block that says what a book begins with.
//!
//! One line says one thing about one thing on a day, and the word after the subject says what. That word is looked up
//! in one table ([`PUNCTUATION`] and [`WORDS`], keyed by the token that is it): `->` a flow, `=` a value, `owes` a
//! claim, `now` a change, `worked` and `used` a measure, `waived`, `ends`, `settled`, `void`, `returned`, `split`,
//! `basis` and `filed` events, and no word at all an occurrence. The rest of the line is read by that word and never
//! by what the subject's name means: `01 flat` is an occurrence whether or not `flat` is a contract.
//!
//! The subject is read before the word is looked for, which is what lets one pass tell a flow from a statement: they
//! share their first end, and a name followed by an arrow is the source of a flow while the same name followed by
//! anything else is what a statement says something of.

use axiom_core::diag::closest;
use axiom_core::{Day, Dec, Diagnostic, Loc};

use crate::ast::*;
use crate::dates::empty_range;
use crate::flow::Arrows;
use crate::lex::{Punct, Tok};
use crate::lines::Line;
use crate::parser::{Parse, Parser, Reported, Scope};

/// What follows a dated line's subject, and so what the line is.
#[derive(Clone, Copy)]
enum Word {
    /// An arrow: the line is a flow.
    Flow(Junction),
    Value,
    Owes,
    Now,
    Worked,
    Used,
    Waived,
    Ends,
    Event(EventState),
    Split,
    Basis,
    Filed,
}

/// What a dated line's subject is followed by when that is a punctuation mark.
const PUNCTUATION: [(Punct, Word); 3] =
    [(Punct::Arrow, Word::Flow(Junction::Out)), (Punct::Back, Word::Flow(Junction::In)), (Punct::Eq, Word::Value)];

/// What a dated line's subject is followed by when that is a word. A line with none is an occurrence.
const WORDS: [(&str, Word); 12] = [
    ("owes", Word::Owes),
    ("now", Word::Now),
    ("worked", Word::Worked),
    ("used", Word::Used),
    ("waived", Word::Waived),
    ("ends", Word::Ends),
    ("settled", Word::Event(EventState::Settled)),
    ("void", Word::Event(EventState::Void)),
    ("returned", Word::Event(EventState::Returned)),
    ("split", Word::Split),
    ("basis", Word::Basis),
    ("filed", Word::Filed),
];

pub(crate) const BUDGET_PERIODS: [(&str, Period); 2] = [("monthly", Period::Month), ("yearly", Period::Year)];

/// What a dated line starts with, read before anything says what the line is.
enum Head<'s> {
    /// A `^code`, a `#purpose` or a commodity that no arrow follows: only a statement is about these.
    Said(Subject<'s>),
    /// A name, a commodity or `?`, and the amount written after it: a flow's source, or what a statement is about.
    Side(Side<'s>),
}

impl<'s> Parser<'s> {
    /// A line that began with a date, which is read as one whole: `[..DATE] SUBJECT [AMOUNT] VERB …`.
    pub fn journal_entry(&mut self, line: &mut Line<'s>, date: Day) -> Parse<()> {
        let clauses = self.mark::<Clause>();
        let spread = self.spread(line, date)?;
        let head = self.head(Scope::Dated(date))?;
        let word = self.word();
        let from = match (head, word) {
            (Head::Side(from), Some(Word::Flow(_))) => return self.flow(line, date, from, clauses),
            (Head::Side(from), _) => from,
            (Head::Said(subject), _) => return self.statement(line, date, subject, None, word),
        };
        match about(&from, spread) {
            Some((subject, amount)) => self.statement(line, date, subject, amount, word),
            // A lot selector, or any amount but a plain one, belongs to a flow: the arrow is what is missing.
            None => self.flow(line, date, from, clauses),
        }
    }

    /// The subject of a dated line, and whatever amount is written after it.
    fn head(&mut self, scope: Scope) -> Parse<Head<'s>> {
        let token = self.peek();
        Ok(match token.tok {
            Tok::Code(code) => Head::Said(self.bump_as(Subject::Code(code))),
            // v3 wrote `DATE #code settled`; a purpose is the subject of a change.
            Tok::Purpose(_) if matches!(self.lexer.peek_second().tok, Tok::Name("settled" | "void" | "returned")) => {
                return Err(self.hash_code(token.loc));
            }
            Tok::Purpose(purpose) => Head::Said(self.bump_as(Subject::Purpose(purpose))),
            // A commodity that starts a flow is a party: `VTI -> fidelity 198.12 USD`.
            Tok::Unit(unit) if !matches!(self.lexer.peek_second().tok, Tok::Punct(Punct::Arrow | Punct::Back)) => {
                self.bump();
                if let Tok::Number(_) = self.tok() {
                    return Err(self.price_needs_equals());
                }
                Head::Said(Subject::Unit(Name(unit)))
            }
            _ => {
                let mut from = self.side(scope)?;
                // A quantity at a price is one amount, however the line goes on.
                if let (true, Some(Quantity::Amount(Amount::Literal(quantity)))) = (self.at(Punct::At), from.amount) {
                    from.amount = Some(Quantity::Amount(self.at_price(quantity)?));
                }
                Head::Side(from)
            }
        })
    }

    /// The way the next token points, if it is an arrow. Only punctuation can be one, so no word is looked up.
    pub fn junction(&self) -> Option<Junction> {
        let Tok::Punct(punct) = self.tok() else { return None };
        match Self::punctuation(punct) {
            Some(Word::Flow(junction)) => Some(junction),
            _ => None,
        }
    }

    /// The word after the subject, if the table has it.
    fn word(&self) -> Option<Word> {
        match self.tok() {
            Tok::Punct(punct) => Self::punctuation(punct),
            Tok::Name(name) => WORDS.iter().find(|(known, _)| *known == name).map(|&(_, word)| word),
            _ => None,
        }
    }

    fn punctuation(punct: Punct) -> Option<Word> {
        PUNCTUATION.iter().find(|(known, _)| *known == punct).map(|&(_, word)| word)
    }

    /// The rest of a flow whose source was read: the arrow, its target, the tail and the lines under it.
    fn flow(&mut self, line: &mut Line<'s>, date: Day, from: Side<'s>, clauses: usize) -> Parse<()> {
        let scope = Scope::Dated(date);
        let (mut flow, arrow) = self.flow_head(from, scope, clauses)?;
        let header = self.end_header(line)?;
        self.flow_legs(line, &mut flow, scope, arrow)?;
        self.emit(&header, Txn { date, flow }, ItemKind::Txn);
        Ok(())
    }

    /// v4's first form of a price, `DATE VTI 280.14 USD`: a price is a value now.
    fn price_needs_equals(&mut self) -> Reported {
        let number = self.peek().loc;
        let diag = Diagnostic::error("price-needs-equals", "a price is a value: `VTI = 280.14 USD`")
            .label(number, "a commodity's price is written after `=`")
            .fix("insert `=`", self.point(number.start), "= ");
        self.report(diag)
    }

    /// `..DATE` after a transaction's date: it is paid that day and recognized
    /// over the range, which is what the clause `for DATE..DATE` says. Adds
    /// that clause.
    fn spread(&mut self, line: &Line<'s>, date: Day) -> Parse<bool> {
        let Some(dots) = self.eat(Punct::DotDot) else { return Ok(false) };
        let last = self.date("the last day of the range, like `2026-12-31`")?;
        let range = self.loc_from(line.body);
        if last < date {
            return self.fail(empty_range(range, self.text(range), date, last));
        }
        self.push(Clause { at: dots.to(range), kind: ClauseKind::For(For::Period(date, last)) });
        Ok(true)
    }

    /// A dated line about `subject`, whose header and lines are read here. An
    /// `amount` already read after the subject is the occurrence's own, and
    /// `word` is what the table says follows it.
    fn statement(
        &mut self,
        line: &mut Line<'s>,
        date: Day,
        subject: Subject<'s>,
        amount: Option<Amount<'s>>,
        word: Option<Word>,
    ) -> Parse<()> {
        let mut statement = self.said(date, subject, amount, word)?;
        if let (Verb::Occurrence(_), Tok::Name(_)) = (&statement.verb, self.tok()) {
            // A name where the tail starts: a flow written without its arrow.
            return Err(self.missing_arrow());
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
    fn said(
        &mut self,
        date: Day,
        subject: Subject<'s>,
        amount: Option<Amount<'s>>,
        word: Option<Word>,
    ) -> Parse<Statement<'s>> {
        let scope = Scope::Statement(date);
        let verb = self.verb(scope, subject, amount, word)?;
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

    /// What the line says of its subject: what its word says, or, with no word, that the promise was kept.
    fn verb(
        &mut self,
        scope: Scope,
        subject: Subject<'s>,
        amount: Option<Amount<'s>>,
        word: Option<Word>,
    ) -> Parse<Verb<'s>> {
        // An amount read before anything else can only be an occurrence's own.
        if amount.is_some() {
            return Ok(Verb::Occurrence(amount));
        }
        match word {
            // An arrow after a subject that no flow starts with is no verb.
            None | Some(Word::Flow(_)) => self.occurrence(subject),
            Some(Word::Value) => self.value(scope),
            Some(Word::Owes) => self.owes(scope),
            Some(Word::Now) => self.now(scope).map(Verb::Now),
            Some(Word::Worked) => self.then(Self::measured).map(Verb::Worked),
            Some(Word::Used) => self.then(Self::measured).map(Verb::Used),
            Some(Word::Waived) => Ok(self.bump_as(Verb::Waived)),
            Some(Word::Ends) => Ok(self.bump_as(Verb::Ends)),
            Some(Word::Event(state)) => Ok(self.bump_as(Verb::Event(state))),
            Some(Word::Split) => self.split(),
            Some(Word::Basis) => self.basis(scope),
            Some(Word::Filed) => self.filed(),
        }
    }

    /// `= AMOUNT`: after `=` an overdrawn account is `-50 USD`, and a negated expression is one too.
    fn value(&mut self, scope: Scope) -> Parse<Verb<'s>> {
        self.bump();
        if !self.at(Punct::Minus) {
            return self.amount(scope).map(Verb::Value);
        }
        if let Tok::Number(_) = self.lexer.peek_second().tok {
            return self.signed_literal().map(|literal| Verb::Value(Amount::Literal(literal)));
        }
        let start = self.peek().loc.start as usize;
        self.bump();
        let value = self.expression()?;
        let first = self.expr(value).first;
        let root = self.node(ExprKind::Unary(UnOp::Neg, value), self.loc_from(start), first);
        Ok(Verb::Value(Amount::Computed(root)))
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
            if let Some(near) = closest(word, WORDS.iter().map(|(known, _)| *known)) {
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
        let legs_only = matches!(statement.verb, Verb::Filed(_));
        let arrows = if legs_only { Arrows::Forbidden } else { Arrows::Tolerated(Junction::Out) };
        self.legs_point(statement.body.legs, arrows)?;
        let (legs, items) = (self.slice(statement.body.legs), statement.body.items);
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
        if says_nothing {
            return self.fail(no_amount_or_items(header, what_it_says(&statement.verb)));
        }
        if !legs_only {
            self.note_bare_legs(statement.body.legs);
        }
        Ok(())
    }

    /// The claim an opening's line states: there are no lines under it.
    fn check_claim(&mut self, statement: &Statement<'s>, header: Loc) -> Parse<()> {
        match statement.verb {
            Verb::Owes { amount: None, .. } => self.fail(no_amount_or_items(header, "a claim")),
            _ => Ok(()),
        }
    }

    /// `opening DATE` and its lines `END [SELECTOR] AMOUNT [basis AMOUNT] [since DATE]`,
    /// `ASSET basis AMOUNT [since DATE]` and `DEBTOR owes CREDITOR AMOUNT TAIL`.
    pub fn opening(&mut self, line: &mut Line<'s>) -> Parse<()> {
        let date = self.item_date("the day the balances are stated, like `2024-12-31`")?;
        let header = self.end_header(line)?;
        let claims = self.mark::<Statement>();
        let lines = self.legs(line, |parser, opening_line| {
            if let (Tok::Name(debtor), Tok::Name("owes")) = (parser.tok(), parser.lexer.peek_second().tok) {
                parser.bump();
                let claim = parser.said(date, Subject::Name(Name(debtor)), None, Some(Word::Owes))?;
                parser.check_claim(&claim, parser.line_loc(opening_line))?;
                parser.expect_eol()?;
                parser.push(claim);
                return Ok(());
            }
            let leg = parser.leg(opening_line, Scope::Opening(date))?;
            match parser.get(leg).amount {
                Quantity::Amount(_) | Quantity::Whole => Ok(()),
                _ => parser.fail(opening_needs_amount(parser.get(leg).loc)),
            }
        });
        let claims = self.since(claims);
        self.emit(&header, Opening { date, lines: lines?, claims }, ItemKind::Opening);
        Ok(())
    }
}

/// What a statement is about when the flow's source that was read is all it says: a name that no selector narrows, and
/// perhaps the occurrence's own amount. A line that spreads a payment over days is a flow whatever follows.
fn about<'s>(from: &Side<'s>, spread: bool) -> Option<(Subject<'s>, Option<Amount<'s>>)> {
    let end = from.end.filter(|end| !spread && end.select.is_empty())?;
    let amount = match from.amount {
        None => None,
        Some(Quantity::Amount(amount)) => Some(amount),
        Some(_) => return None,
    };
    Some((Subject::Name(end.name), amount))
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

fn opening_needs_amount(leg: Loc) -> Diagnostic {
    Diagnostic::error("opening-amount", "an opening line says how much a place holds")
        .label(leg, "no amount here")
        .help("write the balance the statement shows: `checking 10_000 USD`")
}
