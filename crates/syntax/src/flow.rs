//! Flows: the `SUBJECT -> OBJECT` and `SUBJECT <- OBJECT` headers, their ends and tail, the indented legs of a split
//! (which an occurrence, a contract and an opening share), and the selectors that say which parcels an end means.

use std::mem::discriminant;

use axiom_core::{Diagnostic, Loc};

use crate::ast::*;
use crate::legacy::Form;
use crate::lex::{Punct, Tok};
use crate::lines::Line;
use crate::parser::{Parse, Parser, Reported, Scope};

impl<'s> Parser<'s> {
    /// The rest of a header once its subject is read: `-> OBJECT TAIL` or `<- OBJECT TAIL`, whose clauses start at
    /// `clauses` in the table, so that ones the caller read first (a `DATE..DATE` spread) count too. Also gives where
    /// the arrow was. The legs come after the header line has ended: see [`Self::flow_legs`].
    pub fn flow_head(&mut self, subject: Side<'s>, scope: Scope, clauses: usize) -> Parse<(Flow<'s>, Loc)> {
        let (junction, arrow) = self.arrow(&subject)?;
        let object = self.side(scope)?;
        if let (Junction::Out, true) = (junction, self.at(Punct::Slash)) {
            return Err(self.slash(Some((&subject, &object))));
        }
        let tail = self.tail(scope, clauses)?;
        let flow = match junction {
            Junction::Out => Flow { from: subject, to: object, tail, body: Body::default(), junction, through: None },
            Junction::In => self.taken(subject, object, tail, arrow)?,
        };
        Ok((flow, arrow))
    }

    /// `S <- O A` is the flow `O -> S A`: the model reads what moved, and the arrow only says whose side it is written from.
    fn taken(&mut self, subject: Side<'s>, object: Side<'s>, tail: Many<Clause<'s>>, arrow: Loc) -> Parse<Flow<'s>> {
        if subject.end.is_none() {
            return self.fail(no_subject(arrow, &object));
        }
        if subject.amount.is_some() {
            return self.fail(amount_before_take(arrow, &subject, &object));
        }
        let (from, to) = (Side { end: object.end, amount: None }, Side { end: subject.end, amount: object.amount });
        Ok(Flow { from, to, tail, body: Body::default(), junction: Junction::In, through: None })
    }

    /// Reads the legs under `line` into `flow` and settles what the header and they say together.
    pub fn flow_legs(&mut self, line: &Line<'s>, flow: &mut Flow<'s>, scope: Scope, arrow: Loc) -> Parse<()> {
        flow.body = self.body(line, scope)?;
        self.settle(flow, arrow)
    }

    /// The lines under a header, each a leg (it names an end) or an item (it
    /// starts with a sign or an amount). Items may be indented further than
    /// their siblings, to line their amounts up.
    pub fn body(&mut self, line: &Line<'s>, scope: Scope) -> Parse<Body<'s>> {
        // Most flows have no lines under them.
        if !self.lines.peek().is_some_and(|next| next.indent > line.indent) {
            return Ok(Body::default());
        }
        let (legs, items) = (self.mark::<Leg>(), self.mark::<LineItem>());
        self.block(line, true, |parser, child| match parser.at_item() {
            true => parser.line_item(child, scope).map(drop),
            false => parser.leg(child, scope).map(drop),
        })?;
        Ok(Body { legs: self.since(legs), items: self.since(items) })
    }

    /// Whether the next token starts an item and not a leg: a sign, or an amount
    /// (a legs starts with the end it names).
    pub fn at_item(&self) -> bool {
        let sign = matches!(self.tok(), Tok::Punct(Punct::Plus | Punct::Minus));
        sign || matches!(self.tok(), Tok::Number(_) | Tok::Percent(_) | Tok::Fraction(..) | Tok::Code(_))
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
    pub fn side(&mut self, scope: Scope) -> Parse<Side<'s>> {
        let end = if starts_end(self.tok(), || self.lexer.peek_second().tok) { Some(self.end()?) } else { None };
        let starts_amount = match self.tok() {
            Tok::Number(_) | Tok::Punct(Punct::LParen | Punct::Question | Punct::Minus) => true,
            // A share is of something: of what the header says, or, in a
            // declaration, of the flow it goes with.
            Tok::Percent(_) | Tok::Fraction(..) => {
                scope == Scope::Undated || matches!(self.lexer.peek_second().tok, Tok::Name("of"))
            }
            Tok::Name(word) => matches!(word, "empty" | "all"),
            Tok::Punct(Punct::Ellipsis) => return self.fail(rest_in_header(self.peek().loc)),
            _ => false,
        };
        let amount = if starts_amount { Some(self.quantity(scope)?) } else { None };
        Ok(Side { end, amount })
    }

    /// The arrow, and which way it points.
    fn arrow(&mut self, subject: &Side<'s>) -> Parse<(Junction, Loc)> {
        match (self.junction(), subject.end) {
            (Some(junction), _) => Ok((junction, self.written_arrow(junction))),
            (None, Some(_)) => Err(self.missing_arrow()),
            (None, None) => Err(self.expected_arrow()),
        }
    }

    /// Consumes the arrow, which is `junction`'s. `=>`, `→` and `←` are read as one, with an error that says how to
    /// write it, so the flow around them is still kept.
    fn written_arrow(&mut self, junction: Junction) -> Loc {
        let loc = self.bump().loc;
        let (written, arrow) = (self.text(loc), junction.spelling());
        if written != arrow {
            let diag =
                Diagnostic::error("unknown-arrow", format!("`{written}` is not the flow arrow; write `{arrow}`"))
                    .label(loc, format!("money moves with `{arrow}`"))
                    .fix("replace it", loc, arrow);
            self.report(diag);
        }
        loc
    }

    fn arrow_expected(&self) -> Diagnostic {
        self.unexpected(self.peek(), "expected-arrow", "`->` or `<-`")
    }

    /// Reports that the arrow is missing.
    pub fn expected_arrow(&mut self) -> Reported {
        let diag = self.arrow_expected();
        self.report(diag)
    }

    /// Reports that the arrow is missing after an end. Another name or amount right where it belongs is a flow
    /// written without it.
    pub fn missing_arrow(&mut self) -> Reported {
        let token = self.peek();
        let mut diag = self.arrow_expected();
        if matches!(token.tok, Tok::Name(_) | Tok::Number(_)) {
            diag = diag.help("a flow moves value from one end to another: `checking -> food 84.20 USD`").fix(
                "insert the arrow",
                self.point(token.loc.start),
                "-> ",
            );
        }
        self.report(diag)
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
    fn quantity(&mut self, scope: Scope) -> Parse<Quantity<'s>> {
        match self.tok() {
            Tok::Punct(Punct::LParen) => {
                let open = self.bump().loc;
                let amount = Amount::Literal(self.literal()?);
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
            _ => self.amount(scope).map(Quantity::Amount),
        }
    }

    /// An indented line of a split: `[ARROW] END LEGAMOUNT TAIL`.
    pub fn leg(&mut self, line: &mut Line<'s>, scope: Scope) -> Parse<Ref<Leg<'s>>> {
        let doc = line.take_doc();
        let arrow = self.leg_arrow(scope)?;
        let end = self.end()?;
        // What a leg may say that a header side may not: the remainder, a
        // target balance, or a share of the header; and in an opening nothing
        // but a basis, for an asset.
        let amount = match self.tok() {
            Tok::Punct(Punct::Ellipsis) => self.bump_as(Quantity::Rest),
            Tok::Punct(Punct::Eq) => Quantity::Target(self.then(|parser| parser.amount(scope))?),
            Tok::Percent(_) | Tok::Fraction(..) => Quantity::Amount(self.amount(scope)?),
            Tok::Name("basis") if scope.is_opening() => Quantity::Whole,
            _ => self.quantity(scope)?,
        };
        let tail = self.tail(scope, self.mark::<Clause>())?;
        self.expect_eol()?;
        let loc = self.loc_from(line.body);
        Ok(self.push(Leg { doc, end, amount, tail, loc, arrow }))
    }

    /// The arrow a leg leads with, if it has one. An opening's lines state balances, which move nothing.
    fn leg_arrow(&mut self, scope: Scope) -> Parse<Option<Junction>> {
        let Some(junction) = self.junction() else { return Ok(None) };
        if scope.is_opening() {
            return Err(self.expected(
                "expected-end",
                "a name such as `checking`: an opening line states a balance and has no arrow",
            ));
        }
        self.written_arrow(junction);
        Ok(Some(junction))
    }

    /// An indented line that names no end: `[+ | -] AMOUNT TAIL`.
    pub fn line_item(&mut self, line: &mut Line<'s>, scope: Scope) -> Parse<Ref<LineItem<'s>>> {
        let doc = line.take_doc();
        let item = self.item_body(doc, line.body, scope)?;
        if let Tok::Name(end) = self.tok() {
            let plain = item.sign == Sign::Carve && matches!(item.amount, Amount::Literal(_));
            return Err(self.item_with_end(line.body, end, plain));
        }
        self.expect_eol()?;
        Ok(self.push(LineItem { loc: self.loc_from(line.body), ..item }))
    }

    /// An item's sign, amount and tail, which start at `start`: the line of an
    /// item, or what an `also` or a `due … else` says in one.
    pub fn item_body(&mut self, doc: Option<Doc<'s>>, start: usize, scope: Scope) -> Parse<LineItem<'s>> {
        let sign = match self.tok() {
            Tok::Punct(Punct::Plus) => self.bump_as(Sign::Add),
            Tok::Punct(Punct::Minus) => self.bump_as(Sign::Less),
            _ => Sign::Carve,
        };
        let amount = self.priced_amount(scope)?;
        let tail = self.tail(scope, self.mark::<Clause>())?;
        Ok(LineItem { doc, sign, amount, tail, loc: self.loc_from(start) })
    }

    /// An item that is followed by an end: `800 USD retirement`, which a leg
    /// writes the other way round. The end is the next token; the item started
    /// at `start`, and the fix, when the item is a plain amount, swaps them.
    fn item_with_end(&mut self, start: usize, end: &str, plain: bool) -> Reported {
        let name = self.bump().loc;
        let amount = self.src[start..name.start as usize].trim_end();
        let diag = Diagnostic::error("item-with-end", format!("`{end}` is an end, and an item names none"))
            .label(name, "a line that starts with an amount is an item of the flow above it")
            .note("a leg names its end first, then says how much: `retirement 800 USD`");
        let whole = Loc::new(self.id, start as u32, name.end);
        self.report(match plain {
            true => diag.fix(format!("write `{end} {amount}`"), whole, format!("{end} {amount}")),
            false => diag,
        })
    }

    /// `[#PURPOSE [of NAME]] [STRING] CODE* [via PARTY] [for WHAT] [due WHEN]
    /// [basis AMOUNT] [@ PRICE] [! [STRING]]`, in any order; the waiver ends
    /// it. Clauses are kept in the order written, from `mark`.
    // Inlined: most lines end here, and need not enter the loop.
    #[inline(always)]
    pub fn tail(&mut self, scope: Scope, mark: usize) -> Parse<Many<Clause<'s>>> {
        match self.tok() {
            Tok::Eol => Ok(self.since(mark)),
            _ => self.clauses(scope, mark),
        }
    }

    fn clauses(&mut self, scope: Scope, mark: usize) -> Parse<Many<Clause<'s>>> {
        loop {
            let token = self.peek();
            let kind = match token.tok {
                Tok::Purpose(name) => ClauseKind::Purpose(self.purpose(name)?),
                Tok::Str(text) => self.bump_as(ClauseKind::Description(Text(text))),
                Tok::Code(code) => self.bump_as(ClauseKind::Code(code)),
                Tok::Name("via") => ClauseKind::Via(
                    self.then(|p| p.name("expected-party", "the party it went through, such as `paypal`"))?,
                ),
                Tok::Punct(Punct::Slash) => return Err(self.slash(None)),
                Tok::Punct(Punct::At) => ClauseKind::Price(self.then(Self::measured)?),
                Tok::Name("against") => ClauseKind::Against(
                    self.then(|p| p.code("expected-code", "the code of the flow it is about, such as `^inv-11`"))?,
                ),
                Tok::Name("until") if matches!(scope, Scope::Statement(_)) => ClauseKind::Until(
                    self.then(|p| p.date_from(scope.day(), "the last day it holds, like `2026-05-31`"))?,
                ),
                Tok::Punct(Punct::Bang) => ClauseKind::Waive(self.waiver()?),
                Tok::Name("for") => ClauseKind::For(self.then(|p| p.for_what(token.loc))?),
                Tok::Name("due") => ClauseKind::Due(self.then(|p| p.due(scope))?),
                Tok::Name("basis") => ClauseKind::Basis(self.then(|parser| parser.amount(scope))?),
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
            Tok::Name("last") => {
                self.bump();
                let periods = [("month", Relative::Month), ("quarter", Relative::Quarter), ("year", Relative::Year)];
                self.choose(&periods, "unknown-period", "period after `last`").map(|(period, _)| For::Last(period))
            }
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
                diag = diag.fix(
                    format!("`{}` is who it went through: `via {}`", &*end.name, &*end.name),
                    end_loc.to(written),
                    via,
                );
                let amount = match (from.amount, to.amount) {
                    (Some(Quantity::Amount(Amount::Literal(amount))), None)
                    | (None, Some(Quantity::Amount(Amount::Literal(amount)))) => Some(amount),
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

    /// What the header and the lines under it say together: a flow between two ends, a split into legs, a split through an
    /// owner, or an exchange inside one end.
    fn settle(&mut self, flow: &mut Flow<'s>, arrow: Loc) -> Parse<()> {
        let legs = !flow.body.legs.is_empty();
        match (flow.from.end, flow.to.end, legs) {
            (None, None, _) => self.fail(no_end(arrow)),
            (Some(_), Some(_), false) => Ok(()),
            (Some(_), Some(_), true) => self.settle_through(flow, arrow),
            (_, _, true) => self.settle_split(flow),
            _ => self.settle_exchange(flow, arrow),
        }?;
        self.note_old(flow, arrow);
        Ok(())
    }

    /// Notes a header that is spelled the way v4 spelled it: an amount on each side of two named ends, an amount before an
    /// arrow that has only legs after it, or no subject. What the upgrade can move or drop is a written amount: a marker,
    /// a share or an `all` has no other spelling yet, and an exchange that names one end is no spelling of anything v4
    /// read.
    fn note_old(&mut self, flow: &Flow<'s>, arrow: Loc) {
        let legs = !flow.body.legs.is_empty();
        let written = |side: &Side<'s>| side.amount.and_then(Quantity::literal);
        let ends = flow.from.end.is_some() && flow.to.end.is_some();
        // Each is pointed at by what makes it old: the amounts, or the arrow that has nothing before or after it.
        let at = |amount: Literal<'s>| self.loc_of(amount.0);
        let form = match (written(&flow.from), written(&flow.to), legs) {
            (Some(left), Some(right), _) if ends => Some((Form::TwoAmounts, at(left).to(at(right)))),
            (Some(left), None, true) => Some((Form::DanglingAmount, at(left).to(arrow))),
            _ => (flow.junction == Junction::Out && flow.from.end.is_none() && flow.through.is_none())
                .then_some((Form::NoSubject, arrow)),
        };
        if let Some((form, loc)) = form {
            self.old.note(form, loc, 1);
        }
        // The legs of a split or a through-split lead with an arrow; the others may leave it off.
        if flow.junction == Junction::Out && flow.through.is_none() {
            self.note_bare_legs(flow.body.legs);
        }
    }

    /// Legs under a header that names two ends are how an owner passes a party's money on: `me <- acme 12_000 USD` with
    /// `->` legs, `me -> shop 100 USD` with `<-` legs. Without arrows on them it is many-to-many.
    fn settle_through(&mut self, flow: &mut Flow<'s>, arrow: Loc) -> Parse<()> {
        let first = &self.slice(flow.body.legs)[0];
        let (arrowed, first) = (first.arrow.is_some() || flow.junction == Junction::In, first.loc);
        if !arrowed || flow.from.amount.is_some() {
            return self.fail(many_to_many(arrow, first));
        }
        flow.through = match flow.junction {
            Junction::Out => flow.from.end.take(),
            Junction::In => flow.to.end.take(),
        };
        self.settle_split(flow)
    }

    /// One named end and legs for the other: the arrows they lead with point the way the split goes.
    fn settle_split(&mut self, flow: &Flow<'s>) -> Parse<()> {
        let toward = if flow.from.end.is_some() { Junction::Out } else { Junction::In };
        let arrows = match (flow.junction, flow.through) {
            (Junction::Out, None) => Arrows::Tolerated(toward),
            _ => Arrows::Required(toward),
        };
        self.legs_point(flow.body.legs, arrows)?;
        let mut remainders = self.slice(flow.body.legs).iter().filter(|leg| matches!(leg.amount, Quantity::Rest));
        let (Some(first), Some(second)) = (remainders.next(), remainders.next()) else { return Ok(()) };
        let diag = two_remainders(first.loc, second.loc);
        self.fail(diag)
    }

    /// One named end and no legs: an exchange inside it, which says its price. v4's source with both amounts stays as it
    /// was written, and the model refuses it.
    fn settle_exchange(&mut self, flow: &mut Flow<'s>, arrow: Loc) -> Parse<()> {
        let (from, to) = (flow.from.amount.is_some(), flow.to.amount.is_some());
        let named = flow.junction == Junction::In || flow.from.end.is_some();
        if from && to && flow.to.end.is_none() {
            return Ok(());
        }
        if !named || !to || from {
            return self.fail(match flow.junction {
                Junction::In => takes_nothing(arrow, flow.to.end),
                Junction::Out => missing_legs(arrow, flow.from.end.is_some()),
            });
        }
        if !self.slice(flow.tail).iter().any(|clause| matches!(clause.kind, ClauseKind::Price(_))) {
            return self.fail(exchange_no_price(arrow, flow));
        }
        flow.within();
        Ok(())
    }

    /// How the legs under a header write their arrows, and whether they do. A leg that leads with an arrow when it is
    /// told it need not still has to point the right way.
    pub fn legs_point(&mut self, legs: Many<Leg<'s>>, arrows: Arrows) -> Parse<()> {
        let problem = self.slice(legs).iter().find_map(|leg| self.arrow_problem(leg, arrows));
        problem.map_or(Ok(()), |diag| self.fail(diag))
    }

    /// Notes the legs of a line that was kept and that name no arrow, which v4 wrote and v5 does not.
    pub fn note_bare_legs(&mut self, legs: Many<Leg<'s>>) {
        let mut bare = self.slice(legs).iter().filter(|leg| leg.arrow.is_none()).map(|leg| leg.loc);
        if let Some(first) = bare.next() {
            let more = bare.count();
            self.old.note(Form::BareLeg, first, 1 + more as u32);
        }
    }

    fn arrow_problem(&self, leg: &Leg<'s>, arrows: Arrows) -> Option<Diagnostic> {
        match (leg.arrow, arrows) {
            (None, Arrows::Required(toward)) => Some(leg_needs_arrow(leg.loc, toward)),
            (Some(written), Arrows::Required(toward) | Arrows::Tolerated(toward)) if written != toward => {
                Some(leg_direction(self.arrow_at(leg), toward))
            }
            (Some(_), Arrows::Forbidden) => Some(tally_arrow(self.arrow_at(leg))),
            _ => None,
        }
    }

    /// Where a leg's arrow is: the first token of its line.
    fn arrow_at(&self, leg: &Leg<'s>) -> Loc {
        let start = leg.loc.start as usize;
        let len = Punct::lex(&self.src.as_bytes()[start..]).map_or(2, |(_, len)| len);
        Loc::new(leg.loc.file, leg.loc.start, (start + len) as u32)
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

pub(crate) fn clause_name(kind: &ClauseKind<'_>) -> &'static str {
    match kind {
        ClauseKind::Purpose(_) => "purpose",
        ClauseKind::Description(_) => "description",
        ClauseKind::Code(_) => "code",
        ClauseKind::Via(_) => "`via` clause",
        ClauseKind::Against(_) => "`against` clause",
        ClauseKind::Until(_) => "`until` clause",
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
        .help("or let the subject pass it on: `me <- acme 5_200 USD` with a leg `-> irs 692 USD`")
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

impl<'s> Flow<'s> {
    /// An exchange inside one end: the end is on both sides, and the amount on the side that gives (a sale) or takes (a
    /// purchase).
    fn within(&mut self) {
        let bare = |end: Option<End<'s>>| end.map(|end| End { select: Many::EMPTY, ..end });
        match self.junction {
            Junction::Out => {
                self.from.amount = self.to.amount.take();
                self.to.end = bare(self.from.end);
            }
            Junction::In => self.from.end = bare(self.to.end),
        }
    }
}

/// How the legs under a header write their arrows: a header written the new way needs them, one written the way v4 did
/// may leave them off, and a return's tallies have none.
#[derive(Clone, Copy)]
pub(crate) enum Arrows {
    Required(Junction),
    Tolerated(Junction),
    Forbidden,
}

/// Whether a token starts an end and not an amount: a name, a commodity (an amount starts with its number, so one
/// first is an end), or a lone `?` (`? USD` is an unknown amount). `after` is the token that follows.
pub(crate) fn starts_end<'s>(token: Tok<'s>, after: impl FnOnce() -> Tok<'s>) -> bool {
    match token {
        Tok::Name(word) => !matches!(word, "all" | "empty"),
        Tok::Unit(_) => true,
        Tok::Punct(Punct::Question) => !matches!(after(), Tok::Unit(_)),
        _ => false,
    }
}

/// What a side says in its own words, as far as they are plain: its end and its amount, `acme 3_200 USD`.
fn said(side: &Side<'_>) -> String {
    let (end, amount) =
        (side.end.map(|end| end.name.0), side.amount.and_then(Quantity::literal).map(|amount| amount.0));
    [end, amount].into_iter().flatten().collect::<Vec<_>>().join(" ")
}

fn no_subject(arrow: Loc, object: &Side<'_>) -> Diagnostic {
    Diagnostic::error("expected-subject", "a `<-` line starts with the end that takes")
        .label(arrow, "nothing is written before this arrow")
        .help(format!("name the account that takes it: `checking <- {}`", said(object)))
}

fn amount_before_take(arrow: Loc, subject: &Side<'_>, object: &Side<'_>) -> Diagnostic {
    let words = Side { end: object.end, amount: subject.amount };
    let end = subject.end.map_or("checking", |end| end.name.0);
    Diagnostic::error("amount-before-take", "a `<-` line states its amount after the end it takes from")
        .label(arrow, "the amount belongs to the right of this arrow")
        .help(format!("write `{end} <- {}`, with the amount last", said(&words)))
}

fn takes_nothing(arrow: Loc, subject: Option<End<'_>>) -> Diagnostic {
    let end = subject.map_or("checking", |end| end.name.0);
    Diagnostic::error("takes-nothing", "nothing follows this `<-`")
        .label(arrow, "what does it take, and from whom?")
        .help(format!("name the end it takes from and an amount: `{end} <- acme 5_750 USD`"))
        .help(format!("or an amount and a price: `{end} <- 7 VTI @ 285.70 USD`"))
}

fn exchange_no_price(arrow: Loc, flow: &Flow<'_>) -> Diagnostic {
    let (end, amount) = (flow.from.end.or(flow.to.end), flow.to.amount.and_then(Quantity::literal));
    let (end, amount) = (end.map_or("fidelity", |end| end.name.0), amount.map_or("7 VTI", |amount| amount.0));
    let example = format!("{end} {} {amount} @ 297.00 USD", flow.junction.spelling());
    Diagnostic::error("exchange-no-price", "this line has one end and an amount, but no price and no legs")
        .label(arrow, "nothing says what the amount was exchanged for, or where it goes")
        .help(format!("an exchange says its price: `{example}`"))
        .help("or indent legs below the line to split the amount among ends")
}

fn leg_needs_arrow(leg: Loc, toward: Junction) -> Diagnostic {
    let arrow = toward.spelling();
    Diagnostic::error("leg-needs-arrow", "a leg of this flow leads with its arrow")
        .label(leg, "this leg names no arrow")
        .note("under a header written with `<-`, or a split through an owner, each leg says which way its value goes")
        .fix(format!("insert `{arrow} `"), Loc::new(leg.file, leg.start, leg.start), format!("{arrow} "))
}

fn leg_direction(arrow: Loc, toward: Junction) -> Diagnostic {
    let (wrong, right) = (toward.turned().spelling(), toward.spelling());
    Diagnostic::error("leg-direction", format!("`{wrong}` points the wrong way for a leg of this flow"))
        .label(arrow, format!("the legs here go `{right}`"))
        .note("value goes from the header's end to each leg with `->`, and from each leg to it with `<-`")
        .fix(format!("write `{right}`"), arrow, right)
}

fn tally_arrow(arrow: Loc) -> Diagnostic {
    Diagnostic::error("tally-arrow", "a tally has no arrow")
        .label(arrow, "this line lists what a return counted, and moves nothing")
        .fix("remove it", Loc::new(arrow.file, arrow.start, arrow.end + 1), "")
}
