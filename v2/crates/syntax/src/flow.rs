//! Flows: the `SOURCE -> TARGET` header, its ends and tail, the indented legs
//! of a one-side split (which an occurrence, a contract and an opening share),
//! and the selectors that say which parcels an end means.

use std::mem::discriminant;

use axiom_core::{Day, Diagnostic, Loc};

use crate::ast::*;
use crate::lex::{Punct, Tok};
use crate::lines::Line;
use crate::parser::{Parse, Parser, Reported, Scope};

impl<'s> Parser<'s> {
    /// The rest of a header once its source side is read: `-> TARGET TAIL`,
    /// whose clauses start at `clauses` in the table, so that ones the caller
    /// read first (a `DATE..DATE` spread) count too. Also gives where the arrow
    /// was. The legs come after the header line has ended: see
    /// [`Self::flow_legs`].
    pub fn flow_head(&mut self, from: Side<'s>, date: Day, clauses: usize) -> Parse<(Flow<'s>, Loc)> {
        let arrow = self.arrow(&from)?;
        let to = self.side()?;
        if self.at(Punct::Slash) {
            return Err(self.slash(Some((&from, &to))));
        }
        let tail = self.tail(Scope::Dated(date), clauses)?;
        Ok((Flow { from, to, tail, body: Body::default() }, arrow))
    }

    /// Reads the legs under `line` into `flow` and checks they fit its sides.
    pub fn flow_legs(&mut self, line: &Line<'s>, flow: &mut Flow<'s>, date: Day, arrow: Loc) -> Parse<()> {
        flow.body = self.body(line, Scope::Dated(date))?;
        self.check_shape(flow, arrow)
    }

    /// The lines under a header, each a leg (it names an end) or an item (it
    /// starts with a sign or an amount). Items may be indented further than
    /// their siblings, to line their amounts up.
    pub fn body(&mut self, line: &Line<'s>, scope: Scope) -> Parse<Body<'s>> {
        let (legs, items) = (self.mark::<Leg>(), self.mark::<LineItem>());
        self.block(line, true, |parser, child| match parser.at_item() {
            true => parser.line_item(child, scope).map(drop),
            false => parser.leg(child, scope).map(drop),
        })?;
        Ok(Body { legs: self.since(legs), items: self.since(items) })
    }

    /// Whether the next token starts an item and not a leg: a sign, or an amount.
    pub fn at_item(&self) -> bool {
        matches!(self.tok(), Tok::Number(_) | Tok::Percent(_) | Tok::Punct(Punct::Plus | Punct::Minus))
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
            Tok::Punct(Punct::Question) => !matches!(self.lexer.peek_second().tok, Tok::Unit(_)),
            _ => false,
        };
        let end = if starts_end { Some(self.end()?) } else { None };
        let starts_amount = match self.tok() {
            Tok::Number(_) | Tok::Punct(Punct::LParen | Punct::Question | Punct::Minus) => true,
            Tok::Name(word) => matches!(word, "empty" | "all"),
            Tok::Punct(Punct::Ellipsis) => return self.fail(rest_in_header(self.peek().loc)),
            _ => false,
        };
        let amount = if starts_amount { Some(self.quantity()?) } else { None };
        Ok(Side { end, amount })
    }

    /// The arrow. `=>` and `→` are read as one, with an error that says how to
    /// write it, so the flow around them is still kept.
    fn arrow(&mut self, from: &Side<'s>) -> Parse<Loc> {
        if let Some(loc) = self.eat(Punct::Arrow) {
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
        if !matches!(token.tok, Tok::Name(_) | Tok::Unit(_) | Tok::Punct(Punct::Question)) {
            return Err(self.expected("expected-end", "a name such as `checking`"));
        }
        self.bump();
        let mark = self.mark::<Select>();
        if self.at(Punct::LBracket) {
            self.selector()?;
        }
        // `.basis` must touch what it follows, and so is no other token.
        if self.at(Punct::Dot) && self.peek().loc.start == self.lexer.prev_end() {
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
            Tok::Punct(Punct::LParen) => {
                let open = self.bump().loc;
                let amount = self.amount()?;
                self.close(open, Punct::RParen)?;
                Ok(Quantity::Pending(amount))
            }
            Tok::Punct(Punct::Question) => {
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
    pub fn leg(&mut self, line: &mut Line<'s>, scope: Scope) -> Parse<Ref<Leg<'s>>> {
        let doc = line.take_doc();
        let end = self.end()?;
        // What a leg may say that a header side may not: the remainder, a
        // target balance, or a share of the header; and in an opening nothing
        // but a basis, for an asset.
        let amount = match self.tok() {
            Tok::Punct(Punct::Ellipsis) => self.bump_as(Quantity::Rest),
            Tok::Punct(Punct::Eq) => Quantity::Target(self.then(Self::amount)?),
            Tok::Percent(percent) => self.bump_as(Quantity::Percent(percent)),
            Tok::Name("basis") if scope.is_opening() => Quantity::Whole,
            _ => self.quantity()?,
        };
        let tail = self.tail(scope, self.mark::<Clause>())?;
        self.expect_eol()?;
        let loc = self.loc_from(line.body);
        Ok(self.push(Leg { doc, end, amount, tail, loc }))
    }

    /// An indented line that names no end: `[+ | -] ITEMAMOUNT TAIL`.
    pub fn line_item(&mut self, line: &mut Line<'s>, scope: Scope) -> Parse<Ref<LineItem<'s>>> {
        let doc = line.take_doc();
        let sign = match self.tok() {
            Tok::Punct(Punct::Plus) => self.bump_as(Sign::Add),
            Tok::Punct(Punct::Minus) => self.bump_as(Sign::Less),
            _ => Sign::Carve,
        };
        let amount = match self.tok() {
            Tok::Percent(percent) => {
                self.bump();
                match self.eat_word("of") {
                    Some(_) => ItemAmount::ShareOf(percent, self.amount()?),
                    None => ItemAmount::Share(percent),
                }
            }
            _ => ItemAmount::Fixed(self.amount()?),
        };
        let tail = self.tail(scope, self.mark::<Clause>())?;
        self.expect_eol()?;
        let loc = self.loc_from(line.body);
        Ok(self.push(LineItem { doc, sign, amount, tail, loc }))
    }

    /// `[#PURPOSE [of NAME]] [STRING] CODE* [via PARTY] [for WHAT] [due WHEN]
    /// [basis AMOUNT] [@ PRICE] [! [STRING]]`, in any order; the waiver ends
    /// it. Clauses are kept in the order written, from `mark`.
    pub fn tail(&mut self, scope: Scope, mark: usize) -> Parse<Many<Clause<'s>>> {
        loop {
            let token = self.peek();
            let kind = match token.tok {
                Tok::Purpose(name) => ClauseKind::Purpose(self.purpose(name)?),
                Tok::Str(text) => self.bump_as(ClauseKind::Description(Text(text))),
                Tok::Code(code) => self.bump_as(ClauseKind::Code(code)),
                Tok::Name("via") => {
                    ClauseKind::Via(self.then(|p| p.name("expected-party", "the party it went through, such as `paypal`"))?)
                }
                Tok::Punct(Punct::Slash) => return Err(self.slash(None)),
                Tok::Punct(Punct::At) => ClauseKind::Price(self.then(Self::measured)?),
                Tok::Punct(Punct::Bang) => ClauseKind::Waive(self.waiver()?),
                Tok::Name("for") => ClauseKind::For(self.then(|p| p.for_what(token.loc))?),
                Tok::Name("due") => ClauseKind::Due(self.then(|p| p.due(scope))?),
                Tok::Name("basis") => ClauseKind::Basis(self.then(Self::amount)?),
                Tok::Name("since") if scope.is_opening() => {
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
            self.push(clause);
            if matches!(kind, ClauseKind::Waive(_)) {
                break;
            }
        }
        Ok(self.since(mark))
    }

    /// `#NAME [of THING]`, the `#NAME` not yet consumed.
    pub fn purpose(&mut self, name: Name<'s>) -> Parse<Purpose<'s>> {
        self.bump();
        let of = self.eat_word("of").map(|_| self.name("expected-name", "what the purpose is of, such as `condo`"));
        Ok(Purpose { name, of: of.transpose()? })
    }

    /// After `due`: a date, which if short is the first such day on or after
    /// the day the line is dated, or a span after that day.
    pub fn due(&mut self, scope: Scope) -> Parse<Due> {
        match self.tok() {
            Tok::Span(span) => Ok(Due::After(self.bump_as(span))),
            _ => self.date_from(scope.day(), "a date or a span such as `30d`").map(Due::On),
        }
    }

    /// `for car-fund`, or `for` a year, month, date or range; `keyword` is where
    /// the `for` was written.
    fn for_what(&mut self, keyword: Loc) -> Parse<For<'s>> {
        match self.tok() {
            Tok::Name(whom) => Ok(self.bump_as(For::Whom(Name(whom)))),
            // v3 said which claim a payment settled with `for #code`: now the payment carries the code.
            Tok::Purpose(_) => Err(self.hash_code(keyword.to(self.peek().loc))),
            _ => {
                let (first, last, _) = self.days("expected-period", "a period or a name after `for`")?;
                Ok(For::Period(first, last))
            }
        }
    }

    /// A `#name` (the next token) where v3 wrote a code: v4 writes `^name`, and
    /// `#name` is a purpose. A fix rewrites `replaced`, which holds it.
    pub fn hash_code(&mut self, replaced: Loc) -> Reported {
        let (at, code) = (self.peek().loc, format!("^{}", &self.text(self.peek().loc)[1..]));
        let diag = Diagnostic::error("hash-code", "a code is written `^code`, and `#name` is a purpose")
            .label(at, "a purpose goes in a flow's tail, and this is where a code is meant")
            .fix(format!("write `{code}`"), replaced, code);
        self.report(diag)
    }

    /// v3's `/ PARTY`, the `/` being the next token. It named who a payment was
    /// really for when it went through another end (`checking -> paypal 20 USD
    /// / etsy-seller`); v4 writes that party as the end and the one it went
    /// through as `via` (`checking -> etsy-seller 20 USD via paypal`). Where
    /// the header's two sides are known the fix swaps them, and, since an end
    /// may as well be an asset that is bought (`visa 1_739.13 USD -> laptop /
    /// best-buy`), a second fix writes the purchase.
    pub fn slash(&mut self, header: Option<(&Side<'s>, &Side<'s>)>) -> Reported {
        let slash = self.bump().loc;
        let Tok::Name(party) = self.tok() else { return self.expected("expected-party", "the party it was for") };
        let written = slash.to(self.bump().loc);
        let mut diag = Diagnostic::error("v3-party", format!("`/ {party}` is written the other way round now"))
            .label(written, format!("`{party}` is where the flow ends, and the party it went through is `via`"))
            .note("v3 wrote `checking -> paypal 20 USD / etsy-seller`; v4 writes `checking -> etsy-seller 20 USD via paypal`");
        let plain = |end: &End<'_>| end.select.is_empty() && &*end.name != "?";
        if let Some((from, to)) = header {
            if let Some(end) = to.end.filter(plain) {
                let end_loc = self.loc_of(&end.name);
                let between = &self.src[end_loc.end as usize..slash.start as usize];
                let via = format!("{party}{between}via {}", &*end.name);
                diag = diag.fix(format!("`{}` is who it went through: `via {}`", &*end.name, &*end.name), end_loc.to(written), via);
                let amount = match (from.amount, to.amount) {
                    (Some(Quantity::Fixed(amount)), None) | (None, Some(Quantity::Fixed(amount))) => Some(amount),
                    _ => None,
                };
                if let (Some(source), Some(amount)) = (from.end.filter(plain), amount) {
                    let bought = format!("{} -> {party} {} #purchase of {}", &*source.name, &*amount, &*end.name);
                    let how = format!("if `{}` is an asset, it is bought: `#purchase of {}`", &*end.name, &*end.name);
                    diag = diag.fix(how, self.loc_of(&source.name).to(written), bought);
                }
            }
        }
        self.report(diag)
    }

    /// `!` with an optional reason string.
    pub fn waiver(&mut self) -> Parse<Waive<'s>> {
        let bang = self.bump().loc;
        let Tok::Str(reason) = self.tok() else { return Ok(Waive { at: bang, reason: None }) };
        Ok(Waive { at: bang.to(self.bump().loc), reason: Some(Text(reason)) })
    }

    /// One side split needs exactly one named side and legs for the other.
    fn check_shape(&mut self, flow: &Flow<'s>, arrow: Loc) -> Parse<()> {
        let (from_named, to_named) = (flow.from.end.is_some(), flow.to.end.is_some());
        let legs = self.slice(flow.body.legs);
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

    /// `[fifo, 2024, 2026-01..2026-06, 2026-01-22, ^house]` after an end: adds
    /// each selector to the table.
    fn selector(&mut self) -> Parse<()> {
        let open = self.bump().loc;
        loop {
            let select = match self.tok() {
                Tok::Code(code) => self.bump_as(Select::Code(code)),
                Tok::Purpose(_) => return Err(self.hash_code(self.peek().loc)),
                Tok::Name(_) => {
                    let (policy, loc) = self.choose(&Policy::WORDS, "unknown-policy", "lot policy")?;
                    Select::Policy(policy, loc)
                }
                _ => {
                    let what = "a lot selector: a policy, year, month, date, range or `^code`";
                    let (first, last, loc) = self.days("expected-selector", what)?;
                    Select::Range(first, last, loc)
                }
            };
            self.push(select);
            if self.eat(Punct::Comma).is_none() {
                return self.close(open, Punct::RBracket).map(drop);
            }
        }
    }
}

fn clause_name(kind: &ClauseKind<'_>) -> &'static str {
    match kind {
        ClauseKind::Purpose(_) => "purpose",
        ClauseKind::Description(_) => "description",
        ClauseKind::Code(_) => "code",
        ClauseKind::Via(_) => "`via` clause",
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
        .note("an asset's basis is its cost, plus each improvement, less what a law's `consume` takes")
        .help("pay an improvement `#improvement of ASSET`; value arriving with a basis of its own says `basis AMOUNT`")
}

fn rest_in_header(loc: Loc) -> Diagnostic {
    Diagnostic::error("remainder-in-header", "`...` means \"whatever remains\", which only a leg can say")
        .label(loc, "not allowed in a flow's header")
        .help("write the amount, or move this to a leg")
}
