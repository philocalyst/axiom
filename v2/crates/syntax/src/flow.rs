//! Flows: the `SOURCE -> TARGET` header shared by transactions and plans, its
//! tail, and the indented legs of a one-side split.

use axiom_core::{Diagnostic, FileId, Loc};

use crate::ast::{Amount, Flow, Leg, Name, PlaceRef, Quantity, Side, Tail, Waive};
use crate::errors::Delim;
use crate::lex::Tok;
use crate::lines::Line;
use crate::parser::{Parse, Parser};

impl<'s> Parser<'s> {
    /// The rest of a header once its source side is read: `-> TARGET [@ PRICE]
    /// TAIL`. The flow is built once, without legs, and [`Self::flow_legs`]
    /// fills them in place: flows are large, and moving them is not free.
    pub fn flow_head(&mut self, from: Side<'s>) -> Parse<Flow<'s>> {
        let arrow = self.arrow(&from)?;
        let to = self.side()?;
        let (price, tail) = self.trailer()?;
        Ok(Flow { from, to, arrow, price, tail, legs: Vec::new() })
    }

    /// Reads the legs under `line` into `flow` and checks they fit its sides.
    pub fn flow_legs(&mut self, line: &Line<'s>, flow: &mut Flow<'s>) -> Parse<()> {
        self.children(line, |parser, leg_line| {
            flow.legs.push(parser.leg(leg_line)?);
            Ok(())
        })?;
        self.check_shape(flow)
    }

    /// One header side: `checking`, `checking 2_000 USD`, `7 VTI`, or nothing.
    pub fn side(&mut self) -> Parse<Side<'s>> {
        let place = self.optional_place()?;
        let amount = self.optional_quantity()?;
        Ok(Side { place, amount })
    }

    fn arrow(&mut self, from: &Side<'s>) -> Parse<Loc> {
        if let Some(token) = self.cursor.eat(Tok::Arrow) {
            return Ok(token.loc);
        }
        let token = self.cursor.peek();
        let mut diag = self.unexpected(token, "expected-arrow", "`->`");
        // Another place or amount right where the arrow belongs: a flow written
        // without it.
        if from.place.is_some() && matches!(token.tok, Tok::Name(_) | Tok::Number(_)) {
            let before = Loc::new(self.file, token.loc.start, token.loc.start);
            diag = diag.help("a flow moves value from one place to another: `checking -> food 84.20 USD`");
            diag = diag.fix("insert the arrow", before, "-> ");
        }
        self.fail(diag)
    }

    fn optional_place(&mut self) -> Parse<Option<PlaceRef<'s>>> {
        let token = self.cursor.peek();
        let is_place = match token.tok {
            Tok::Name(word) => !matches!(word, "all" | "empty"),
            // `? USD` is an unknown amount; a lone `?` is the unknown place.
            Tok::Question => !matches!(self.cursor.peek_second().tok, Tok::Unit(_)),
            _ => false,
        };
        if is_place { self.place().map(Some) } else { Ok(None) }
    }

    /// A place, or `?`, with any lot selector.
    pub fn place(&mut self) -> Parse<PlaceRef<'s>> {
        let token = self.cursor.peek();
        if !matches!(token.tok, Tok::Name(_) | Tok::Question) {
            return Err(self.expected("expected-place", "a place such as `checking`"));
        }
        self.cursor.bump();
        let name = Name { text: self.text(token.loc), loc: token.loc };
        let select = if matches!(self.cursor.peek().tok, Tok::LBracket) { self.selector()? } else { Vec::new() };
        Ok(PlaceRef { name, select })
    }

    fn optional_quantity(&mut self) -> Parse<Option<Quantity<'s>>> {
        let token = self.cursor.peek();
        let starts_amount = match token.tok {
            Tok::Number(_) | Tok::LParen | Tok::Question | Tok::Minus => true,
            Tok::Name(word) => matches!(word, "empty" | "all"),
            Tok::Ellipsis => return self.fail(rest_in_header(token.loc)),
            _ => false,
        };
        if starts_amount { self.quantity().map(Some) } else { Ok(None) }
    }

    /// `84.20 USD`, `empty`, `(350 USD)`, `? USD`, or `all`.
    fn quantity(&mut self) -> Parse<Quantity<'s>> {
        let token = self.cursor.peek();
        match token.tok {
            Tok::LParen => self.pending(),
            Tok::Question => self.unknown_amount(),
            Tok::Name("all") => {
                self.cursor.bump();
                Ok(Quantity::All(token.loc))
            }
            _ => self.amount().map(Quantity::Fixed),
        }
    }

    fn pending(&mut self) -> Parse<Quantity<'s>> {
        let open = self.cursor.bump().loc;
        let inner = self.amount()?;
        let close = self.close(open, Delim::Paren)?;
        Ok(Quantity::Pending(Amount { loc: open.to(close), ..inner }))
    }

    fn unknown_amount(&mut self) -> Parse<Quantity<'s>> {
        let question = self.cursor.bump().loc;
        let unit = self.unit("expected-commodity", "a commodity such as `USD`")?;
        Ok(Quantity::Unknown { unit, loc: question.to(unit.loc) })
    }

    /// An indented line of a split: `PLACE LEGAMOUNT [@ PRICE] TAIL`.
    fn leg(&mut self, line: &mut Line<'s>) -> Parse<Leg<'s>> {
        let doc = line.take_doc();
        let place = self.place()?;
        let amount = self.leg_amount()?;
        let (price, tail) = self.trailer()?;
        self.expect_eol()?;
        Ok(Leg { doc, place, amount, price, tail, loc: self.loc_from(line.body) })
    }

    /// What a leg may say that a header side may not: the remainder, or a
    /// target balance.
    fn leg_amount(&mut self) -> Parse<Quantity<'s>> {
        let token = self.cursor.peek();
        match token.tok {
            Tok::Ellipsis => {
                self.cursor.bump();
                Ok(Quantity::Rest(token.loc))
            }
            Tok::Eq => {
                self.cursor.bump();
                self.amount().map(Quantity::Target)
            }
            _ => self.quantity(),
        }
    }

    /// `[@ PRICE] [/ PAYEE] CODE* [! [STRING]]`, in any order; the waiver ends it.
    pub fn trailer(&mut self) -> Parse<(Option<Amount<'s>>, Tail<'s>)> {
        let mut price: Option<Amount<'s>> = None;
        let mut tail = Tail::default();
        loop {
            let token = self.cursor.peek();
            match token.tok {
                Tok::At => {
                    self.cursor.bump();
                    if let Some(first) = &price {
                        return Err(self.duplicate("price", token.loc, first.loc));
                    }
                    price = Some(self.measured()?);
                }
                Tok::Slash => {
                    self.cursor.bump();
                    if let Some(first) = &tail.payee {
                        return Err(self.duplicate("payee", token.loc, first.loc));
                    }
                    tail.payee = Some(self.name("expected-payee", "a payee such as `trader-joes`")?);
                }
                Tok::Code(text) => {
                    self.cursor.bump();
                    tail.codes.push(Name { text, loc: token.loc });
                }
                Tok::Bang => {
                    tail.waive = self.waiver()?;
                    return Ok((price, tail));
                }
                _ => return Ok((price, tail)),
            }
        }
    }

    /// `!` with an optional reason string.
    pub fn waiver(&mut self) -> Parse<Option<Waive<'s>>> {
        let Some(bang) = self.cursor.eat(Tok::Bang) else { return Ok(None) };
        let Tok::Str(reason) = self.cursor.peek().tok else {
            return Ok(Some(Waive { loc: bang.loc, reason: None }));
        };
        let string = self.cursor.bump();
        Ok(Some(Waive { loc: bang.loc.to(string.loc), reason: Some(reason) }))
    }

    /// One side split needs exactly one named side and legs for the other.
    fn check_shape(&mut self, flow: &Flow<'s>) -> Parse<()> {
        let (from_named, to_named) = (flow.from.place.is_some(), flow.to.place.is_some());
        let diag = match (from_named, to_named, flow.legs.first()) {
            (true, true, Some(leg)) => many_to_many(flow.arrow, leg.loc),
            (false, false, _) => no_place(flow.arrow),
            (true, false, None) => missing_legs(self.file, flow.arrow, Unnamed::Target),
            (false, true, None) => missing_legs(self.file, flow.arrow, Unnamed::Source),
            _ => return self.check_remainders(&flow.legs),
        };
        self.fail(diag)
    }

    /// At most one leg may take whatever remains.
    fn check_remainders(&mut self, legs: &[Leg<'s>]) -> Parse<()> {
        let mut remainders = legs.iter().filter(|leg| matches!(leg.amount, Quantity::Rest(_)));
        let (Some(first), Some(second)) = (remainders.next(), remainders.next()) else {
            return Ok(());
        };
        self.fail(two_remainders(first.loc, second.loc))
    }
}

/// Which side of the arrow a header leaves unnamed.
#[derive(Clone, Copy)]
enum Unnamed {
    Source,
    Target,
}

fn many_to_many(arrow: Loc, leg: Loc) -> Diagnostic {
    Diagnostic::error("many-to-many", "a transaction cannot name both sides and also list legs")
        .label(leg, "this leg has no side to belong to")
        .context(arrow, "both sides are already named here")
        .note("legs spell out the side the header leaves unnamed: `acme -> 5_200 USD` with legs for where it goes")
        .help("write two transactions, one for each flow")
}

fn no_place(arrow: Loc) -> Diagnostic {
    Diagnostic::error("flow-without-place", "a flow needs at least one place")
        .label(arrow, "neither side of this arrow names a place")
        .help("name where the value comes from or goes to: `checking -> food 84.20 USD`")
}

fn missing_legs(file: FileId, arrow: Loc, unnamed: Unnamed) -> Diagnostic {
    let (side, at, insert, fix) = match unnamed {
        Unnamed::Target => {
            ("right", arrow.end, " ?", "send it to `?`, the place for money whose destination is unknown")
        }
        Unnamed::Source => ("left", arrow.start, "? ", "take it from `?`, the place for money whose origin is unknown"),
    };
    Diagnostic::error("missing-legs", format!("nothing is named on the {side} of this arrow"))
        .label(arrow, "the other side is not written, and no legs list it")
        .help("indent legs below the flow to list where the rest goes, or name a place")
        .fix(fix, Loc::new(file, at, at), insert)
}

fn two_remainders(first: Loc, second: Loc) -> Diagnostic {
    Diagnostic::error("two-remainders", "only one leg can take the remainder")
        .label(second, "a second `...`")
        .context(first, "this leg already takes whatever remains")
        .help("give one of the legs an amount")
}

fn rest_in_header(loc: Loc) -> Diagnostic {
    Diagnostic::error("remainder-in-header", "`...` means \"whatever remains\", which only a leg can say")
        .label(loc, "not allowed in a flow's header")
        .help("write the amount, or move this to a leg")
}
