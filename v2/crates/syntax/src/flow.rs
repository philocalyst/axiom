//! Flows: the `SOURCE -> TARGET` header, its ends and tail, the indented legs
//! of a one-side split (which an occurrence, a contract and an opening share),
//! and the selectors that say which parcels an end means.

use std::mem::discriminant;

use axiom_core::{Diagnostic, Id, Loc};

use crate::ast::*;
use crate::lex::Tok;
use crate::lines::Line;
use crate::parser::{Parse, Parser, Scope};

const POLICIES: [(&str, Policy); 4] =
    [("fifo", Policy::Fifo), ("lifo", Policy::Lifo), ("hifo", Policy::Hifo), ("prorata", Policy::Prorata)];

impl<'s> Parser<'s> {
    /// The rest of a header once its source side is read: `-> TARGET TAIL`,
    /// whose clauses start at `clauses` in the table, so that ones the caller
    /// read first (a `DATE..DATE` spread) count too. Also gives where the arrow
    /// was. The legs come after the header line has ended: see
    /// [`Self::flow_legs`].
    pub fn flow_head(&mut self, from: Side<'s>, clauses: usize) -> Parse<(Flow<'s>, Loc)> {
        let arrow = self.arrow(&from)?;
        let to = self.side()?;
        let tail = self.tail(clauses)?;
        Ok((Flow { from, to, tail, legs: Many::EMPTY }, arrow))
    }

    /// Reads the legs under `line` into `flow` and checks they fit its sides.
    pub fn flow_legs(&mut self, line: &Line<'s>, flow: &mut Flow<'s>, arrow: Loc) -> Parse<()> {
        flow.legs = self.legs(line, |parser, leg_line| parser.leg(leg_line).map(drop))?;
        self.check_shape(flow, arrow)
    }

    /// The legs the lines under `line` make, each parsed by `each`, which adds
    /// it with [`Self::leg`]. Fails, after every line has been read, if any did.
    pub fn legs(
        &mut self,
        line: &Line<'s>,
        each: impl FnMut(&mut Self, &mut Line<'s>) -> Parse<()>,
    ) -> Parse<Many<Leg<'s>>> {
        let mark = self.mark::<Leg>();
        let children = self.children(line, each);
        children.map(|()| self.since(mark))
    }

    /// One header side: `checking`, `checking 2_000 USD`, `7 VTI`, or nothing.
    // Inlined: what it returns is built where it is wanted, not copied up out of a call.
    #[inline(always)]
    pub fn side(&mut self) -> Parse<Side<'s>> {
        let starts_end = match self.tok() {
            Tok::Name(word) => !matches!(word, "all" | "empty"),
            // An amount starts with its number, so a commodity first is an end.
            Tok::Unit(_) => true,
            // `? USD` is an unknown amount; a lone `?` is the unknown party.
            Tok::Punct("?") => !matches!(self.lexer.peek_second().tok, Tok::Unit(_)),
            _ => false,
        };
        let end = if starts_end { Some(self.end()?) } else { None };
        let starts_amount = match self.tok() {
            Tok::Number(_) | Tok::Punct("(" | "?" | "-") => true,
            Tok::Name(word) => matches!(word, "empty" | "all"),
            Tok::Punct("...") => return self.fail(rest_in_header(self.peek().loc)),
            _ => false,
        };
        let amount = if starts_amount { Some(self.quantity()?) } else { None };
        Ok(Side { end, amount })
    }

    /// The arrow. `=>` and `→` are read as one, with an error that says how to
    /// write it, so the flow around them is still kept.
    fn arrow(&mut self, from: &Side<'s>) -> Parse<Loc> {
        if let Some(loc) = self.eat("->") {
            let written = self.text(loc);
            if written != "->" {
                let diag = Diagnostic::error("unknown-arrow", format!("`{written}` is not the flow arrow; write `->`"))
                    .label(loc, "money moves with `->`")
                    .fix("replace it", loc, "->");
                self.report(diag);
            }
            return Ok(loc);
        }
        let token = self.peek();
        let mut diag = self.unexpected(token, "expected-arrow", "`->`");
        // Another name or amount right where the arrow belongs: a flow written
        // without it.
        if from.end.is_some() && matches!(token.tok, Tok::Name(_) | Tok::Number(_)) {
            diag = diag.help("a flow moves value from one end to another: `checking -> food 84.20 USD`").fix(
                "insert the arrow",
                self.point(token.loc.start),
                "-> ",
            );
        }
        self.fail(diag)
    }

    /// A name, a commodity or `?`, with any lot selectors.
    pub fn end(&mut self) -> Parse<End<'s>> {
        let token = self.peek();
        if !matches!(token.tok, Tok::Name(_) | Tok::Unit(_) | Tok::Punct("?")) {
            return Err(self.expected("expected-end", "a name such as `checking`"));
        }
        self.bump();
        let mark = self.mark::<Select>();
        if self.at("[") {
            self.selector()?;
        }
        // `.basis` must touch what it follows, and so is no other token.
        if self.at(".") && self.peek().loc.start == self.lexer.prev_end() {
            if let Tok::Name("basis") = self.lexer.peek_second().tok {
                let (dot, word) = (self.bump().loc, self.bump().loc);
                return self.fail(basis_is_derived(dot.to(word)));
            }
        }
        Ok(End { name: Name(self.text(token.loc)), select: self.since(mark) })
    }

    /// `84.20 USD`, `empty`, `(350 USD)`, `? USD`, or `all [UNIT]`.
    // Inlined: what it returns is built where it is wanted, not copied up out of a call.
    #[inline(always)]
    fn quantity(&mut self) -> Parse<Quantity<'s>> {
        match self.tok() {
            Tok::Punct("(") => {
                let open = self.bump().loc;
                let amount = self.amount()?;
                self.close(open, ")")?;
                Ok(Quantity::Pending(amount))
            }
            Tok::Punct("?") => {
                self.then(|p| p.unit("expected-commodity", "a commodity such as `USD`")).map(Quantity::Unknown)
            }
            Tok::Name("all") => {
                self.bump();
                let Tok::Unit(unit) = self.tok() else { return Ok(Quantity::All(None)) };
                self.bump();
                Ok(Quantity::All(Some(Name(unit))))
            }
            _ => self.amount().map(Quantity::Fixed),
        }
    }

    /// An indented line of a split: `END LEGAMOUNT TAIL`.
    pub fn leg(&mut self, line: &mut Line<'s>) -> Parse<Id<Leg<'s>>> {
        let doc = line.take_doc();
        let end = self.end()?;
        // What a leg may say that a header side may not: the remainder, a
        // target balance, or a share of the header; and in an opening nothing
        // but a basis, for an asset.
        let amount = match self.tok() {
            Tok::Punct("...") => self.bump_as(Quantity::Rest),
            Tok::Punct("=") => Quantity::Target(self.then(Self::amount)?),
            Tok::Percent(percent) => self.bump_as(Quantity::Percent(percent)),
            Tok::Name("basis") if self.scope == Scope::Opening => Quantity::Whole,
            _ => self.quantity()?,
        };
        let tail = self.tail(self.mark::<Clause>())?;
        self.expect_eol()?;
        let loc = self.loc_from(line.body);
        Ok(self.push(Leg { doc, end, amount, tail, loc }))
    }

    /// `[#PURPOSE [of NAME]] [STRING] CODE* [for WHAT] [due WHEN] [basis AMOUNT] [/ PARTY]
    /// [@ PRICE] [! [STRING]]`, in any order; the waiver ends it. Clauses are
    /// kept in the order written, from `mark`.
    pub fn tail(&mut self, mark: usize) -> Parse<Many<Clause<'s>>> {
        loop {
            let token = self.peek();
            let kind = match token.tok {
                Tok::Purpose(name) => ClauseKind::Purpose(self.purpose(name)?),
                Tok::Str(text) => self.bump_as(ClauseKind::Description(text)),
                Tok::Code(code) => self.bump_as(ClauseKind::Code(code)),
                Tok::Punct("/") => ClauseKind::Party(self.then(|p| p.name("expected-party", "a party, like `paypal`"))?),
                Tok::Punct("@") => ClauseKind::Price(self.then(Self::measured)?),
                Tok::Punct("!") => ClauseKind::Waive(self.waiver()?),
                Tok::Name("for") => ClauseKind::For(self.then(|p| p.for_what(token.loc))?),
                Tok::Name("due") => ClauseKind::Due(self.then(Self::due)?),
                Tok::Name("basis") => ClauseKind::Basis(self.then(Self::amount)?),
                Tok::Name("since") if self.scope == Scope::Opening => {
                    ClauseKind::Since(self.then(|p| p.date("the day the parcels were acquired, like `2023-06-15`"))?)
                }
                _ => break,
            };
            let clause = Clause { at: self.loc_from(token.loc.start as usize), kind };
            let same = |old: &&Clause| discriminant(&old.kind) == discriminant(&kind);
            let earlier = self.slice(self.since(mark)).iter().find(same).map(|old| old.at);
            if let Some(first) = earlier.filter(|_| !matches!(kind, ClauseKind::Code(_))) {
                return Err(self.duplicate(clause_name(&kind), clause.at, first));
            }
            self.t.clauses.push(clause);
            if matches!(kind, ClauseKind::Waive(_)) {
                break;
            }
        }
        Ok(self.since(mark))
    }

    /// `#NAME [of THING]`, the `#NAME` not yet consumed.
    fn purpose(&mut self, name: Name<'s>) -> Parse<Purpose<'s>> {
        self.bump();
        let of = self.eat_word("of").map(|_| self.name("expected-name", "what it is of, such as `condo`"));
        Ok(Purpose { name, of: of.transpose()? })
    }

    /// After `due`: a date, or a span after the date it is measured from.
    fn due(&mut self) -> Parse<Due> {
        match self.tok() {
            Tok::Span(span) => Ok(Due::After(self.bump_as(span))),
            _ => self.date("a date or a span such as `30d`").map(Due::On),
        }
    }

    /// `for car-fund`, or `for` a year, month, date or range; `keyword` is where
    /// the `for` was written.
    fn for_what(&mut self, keyword: Loc) -> Parse<For<'s>> {
        let token = self.peek();
        match token.tok {
            Tok::Name(entity) => Ok(self.bump_as(For::Entity(Name(entity)))),
            // v3 said which claim a payment settled with `for #code`.
            Tok::Code(_) | Tok::Purpose(_) => {
                let (clause, code) = (keyword.to(token.loc), format!("^{}", &self.text(token.loc)[1..]));
                let diag = Diagnostic::error("for-code", "a flow that carries a claim's code settles that claim")
                    .label(clause, "`for` takes a period or a name now, not a code")
                    .fix(format!("write `{code}` on the flow itself"), clause, code);
                self.fail(diag)
            }
            _ => {
                let (first, last, _) = self.days("expected-period", "a period or a name after `for`")?;
                Ok(For::Period(first, last))
            }
        }
    }

    /// `!` with an optional reason string.
    pub fn waiver(&mut self) -> Parse<Waive<'s>> {
        let bang = self.bump().loc;
        let Tok::Str(reason) = self.tok() else { return Ok(Waive { at: bang, reason: None }) };
        Ok(Waive { at: bang.to(self.bump().loc), reason: Some(reason) })
    }

    /// One side split needs exactly one named side and legs for the other.
    fn check_shape(&mut self, flow: &Flow<'s>, arrow: Loc) -> Parse<()> {
        let (from_named, to_named) = (flow.from.end.is_some(), flow.to.end.is_some());
        let legs = self.slice(flow.legs);
        let diag = match (from_named, to_named, legs.first()) {
            (true, true, Some(leg)) => many_to_many(arrow, leg.loc),
            (false, false, _) => no_end(arrow),
            // An exchange with only a source stays at the source: `fidelity 20 VTI -> 5_940 USD`.
            (true, false, None) if flow.from.amount.is_some() && flow.to.amount.is_some() => return Ok(()),
            (true, false, None) => missing_legs(arrow, true),
            (false, true, None) => missing_legs(arrow, false),
            _ => {
                let mut remainders = legs.iter().filter(|leg| matches!(leg.amount, Quantity::Rest));
                let (Some(first), Some(second)) = (remainders.next(), remainders.next()) else { return Ok(()) };
                two_remainders(first.loc, second.loc)
            }
        };
        self.fail(diag)
    }

    // ─── Selectors ──────────────────────────────────────────────────────────

    /// `[fifo, 2024, 2026-01..2026-06, 2026-01-22, #house]` after a place: adds
    /// each selector to the table.
    fn selector(&mut self) -> Parse<()> {
        let open = self.bump().loc;
        loop {
            let select = match self.tok() {
                Tok::Code(code) => self.bump_as(Select::Code(code)),
                Tok::Name(_) => {
                    let (policy, loc) = self.choose(&POLICIES, "unknown-policy", "lot policy")?;
                    Select::Policy(policy, loc)
                }
                _ => {
                    let what = "a lot selector: a policy, year, month, date, range or `#code`";
                    let (first, last, loc) = self.days("expected-selector", what)?;
                    Select::Range(first, last, loc)
                }
            };
            self.t.selects.push(select);
            if self.eat(",").is_none() {
                return self.close(open, "]").map(drop);
            }
        }
    }
}

fn clause_name(kind: &ClauseKind<'_>) -> &'static str {
    match kind {
        ClauseKind::Purpose(_) => "purpose",
        ClauseKind::Description(_) => "description",
        ClauseKind::Code(_) => "code",
        ClauseKind::Party(_) => "party",
        ClauseKind::Price(_) => "price",
        ClauseKind::For(_) => "`for` clause",
        ClauseKind::Due(_) => "`due` clause",
        ClauseKind::Basis(_) => "`basis` clause",
        ClauseKind::Since(_) => "`since` clause",
        ClauseKind::Waive(_) => "waiver",
    }
}

fn many_to_many(arrow: Loc, leg: Loc) -> Diagnostic {
    Diagnostic::error("many-to-many", "a transaction cannot name both sides and also list legs")
        .label(leg, "this leg has no side to belong to")
        .context(arrow, "both sides are already named here")
        .note("legs spell out the side the header leaves unnamed: `acme -> 5_200 USD` with legs for where it goes")
        .help("write two transactions, one for each flow")
}

fn no_end(arrow: Loc) -> Diagnostic {
    Diagnostic::error("flow-without-end", "a flow needs at least one end")
        .label(arrow, "neither side of this arrow names one")
        .help("name where the value comes from or goes to: `checking -> trader-joes 84.20 USD`")
}

/// The header names an end on one side only, and no legs say the other.
fn missing_legs(arrow: Loc, right: bool) -> Diagnostic {
    let (side, at, insert, fix) = match right {
        true => ("right", arrow.end, " ?", "send it to `?`, the party for money whose destination is unknown"),
        false => ("left", arrow.start, "? ", "take it from `?`, the party for money whose origin is unknown"),
    };
    Diagnostic::error("missing-legs", format!("nothing is named on the {side} of this arrow"))
        .label(arrow, "the other side is not written, and no legs list it")
        .help("indent legs below the flow to list where the rest goes, or name it; an exchange states both amounts")
        .fix(fix, Loc::new(arrow.file, at, at), insert)
}

fn two_remainders(first: Loc, second: Loc) -> Diagnostic {
    Diagnostic::error("two-remainders", "only one leg can take the remainder")
        .label(second, "a second `...`")
        .context(first, "this leg already takes whatever remains")
        .help("give one of the legs an amount")
}

/// `house.basis`: v3 moved a basis like a balance.
fn basis_is_derived(loc: Loc) -> Diagnostic {
    Diagnostic::error("basis-end", "a basis is derived, never moved: there is no `.basis`")
        .label(loc, "not an end any more")
        .note("an asset's basis is its cost, plus each improvement, less what laws consume")
        .help("pay an improvement `#improvement of ASSET`, and a law's `consume` lowers a basis")
        .help("value that arrives with a basis other than its cost says so: `basis AMOUNT`")
}

fn rest_in_header(loc: Loc) -> Diagnostic {
    Diagnostic::error("remainder-in-header", "`...` means \"whatever remains\", which only a leg can say")
        .label(loc, "not allowed in a flow's header")
        .help("write the amount, or move this to a leg")
}
