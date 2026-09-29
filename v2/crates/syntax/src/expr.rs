//! Expressions: precedence climbing into the file's post-order arena.
//!
//! Every node is pushed after its children, so a subtree is the contiguous run
//! `first..=root` of the arena. `first` is read off the arena *before* a
//! subtree is parsed (or off its leftmost child afterwards), which is why a
//! node never needs to be revisited.

use axiom_core::{Dec, Diagnostic, Loc};

use crate::ast::{BinOp, ExprId, ExprKind, Name, UnOp};
use crate::errors::Delim;
use crate::lex::{Tok, Token};
use crate::parser::{Parse, Parser};

// Binding powers, loosest first. Binary operators associate to the left. `not`
// binds tighter than `and` but looser than a comparison, so it parses its
// operand at `COMPARE`; unary minus is tighter than `*` and `/`.
const OR: u8 = 1;
const AND: u8 = 2;
const COMPARE: u8 = 4;
const ADD: u8 = 5;
const MUL: u8 = 6;
const NEGATE: u8 = 7;

/// How deeply expressions may nest. Far beyond what a law needs; the limit
/// keeps a hostile file from overflowing the stack.
const MAX_DEPTH: u32 = 100;

#[derive(Clone, Copy)]
enum Infix {
    Binary(BinOp),
    /// `x is a | b | c`
    Is,
}

impl<'s> Parser<'s> {
    pub fn expression(&mut self) -> Parse<ExprId> {
        self.binary(0)
    }

    /// An atom with its postfix operators: what a property argument is.
    pub fn primary(&mut self) -> Parse<ExprId> {
        let start = self.cursor.peek().loc.start;
        let mut expr = self.atom()?;
        let first = self.exprs[expr].first;
        // A postfix operator must touch its operand, so `limit[year]` is an
        // index but `holds VTI [x]` is two arguments.
        while self.cursor.peek().loc.start == self.cursor.prev_end() {
            let token = self.cursor.peek();
            let kind = match token.tok {
                Tok::Dot => {
                    self.cursor.bump();
                    ExprKind::Field(expr, self.name("expected-name", "a field name after `.`")?)
                }
                Tok::LBracket => {
                    self.cursor.bump();
                    ExprKind::Index(expr, self.list(token, Delim::Bracket)?)
                }
                _ => break,
            };
            expr = self.exprs.push(kind, self.loc_from(start as usize), first);
        }
        Ok(expr)
    }

    /// Every way an expression nests (parentheses, arguments, `if`, prefix
    /// operators) passes through here, so this is where depth is limited.
    fn binary(&mut self, min_power: u8) -> Parse<ExprId> {
        if self.depth == MAX_DEPTH {
            let token = self.cursor.peek();
            return self.fail(too_deep(token.loc));
        }
        self.depth += 1;
        let parsed = self.binary_within_limit(min_power);
        self.depth -= 1;
        parsed
    }

    fn binary_within_limit(&mut self, min_power: u8) -> Parse<ExprId> {
        let mut lhs = self.prefix()?;
        while let Some((op, power)) = self.infix() {
            if power < min_power {
                break;
            }
            self.cursor.bump();
            lhs = self.infix_rest(lhs, op, power)?;
        }
        Ok(lhs)
    }

    fn infix(&self) -> Option<(Infix, u8)> {
        let (op, power) = match self.cursor.peek().tok {
            Tok::Name("or") => (Infix::Binary(BinOp::Or), OR),
            Tok::Name("and") => (Infix::Binary(BinOp::And), AND),
            Tok::Name("is") => (Infix::Is, COMPARE),
            Tok::EqEq => (Infix::Binary(BinOp::Eq), COMPARE),
            Tok::NotEq => (Infix::Binary(BinOp::Ne), COMPARE),
            Tok::Lt => (Infix::Binary(BinOp::Lt), COMPARE),
            Tok::Le => (Infix::Binary(BinOp::Le), COMPARE),
            Tok::Gt => (Infix::Binary(BinOp::Gt), COMPARE),
            Tok::Ge => (Infix::Binary(BinOp::Ge), COMPARE),
            Tok::Plus => (Infix::Binary(BinOp::Add), ADD),
            Tok::Minus => (Infix::Binary(BinOp::Sub), ADD),
            Tok::Star => (Infix::Binary(BinOp::Mul), MUL),
            Tok::Slash => (Infix::Binary(BinOp::Div), MUL),
            _ => return None,
        };
        Some((op, power))
    }

    /// The right-hand side of an operator whose token was just consumed.
    fn infix_rest(&mut self, lhs: ExprId, op: Infix, power: u8) -> Parse<ExprId> {
        let (first, start) = (self.exprs[lhs].first, self.exprs[lhs].loc.start as usize);
        let kind = match op {
            Infix::Is => ExprKind::Is(lhs, self.alternatives()?),
            Infix::Binary(op) => ExprKind::Binary(op, lhs, self.binary(power + 1)?),
        };
        let node = self.exprs.push(kind, self.loc_from(start), first);
        if power == COMPARE {
            self.reject_chained_comparison()?;
        }
        Ok(node)
    }

    fn reject_chained_comparison(&mut self) -> Parse<()> {
        if !matches!(self.infix(), Some((_, COMPARE))) {
            return Ok(());
        }
        let token = self.cursor.peek();
        let diag = Diagnostic::error("chained-comparison", "comparisons do not chain")
            .label(token.loc, "a second comparison")
            .help("write each comparison and join them with `and`: `a < b and b < c`");
        self.fail(diag)
    }

    fn prefix(&mut self) -> Parse<ExprId> {
        let token = self.cursor.peek();
        let (op, power) = match token.tok {
            Tok::Name("not") => (UnOp::Not, COMPARE),
            Tok::Minus => (UnOp::Neg, NEGATE),
            _ => return self.primary(),
        };
        self.cursor.bump();
        let first = self.exprs.next();
        let operand = self.binary(power)?;
        Ok(self.exprs.push(ExprKind::Unary(op, operand), self.loc_from(token.loc.start as usize), first))
    }

    fn atom(&mut self) -> Parse<ExprId> {
        let token = self.cursor.peek();
        match token.tok {
            Tok::Number(num) => self.number(token, num),
            Tok::Percent(num) => self.leaf(token, ExprKind::Pct(num)),
            Tok::Date(day) => self.leaf(token, ExprKind::Date(day)),
            Tok::Span(span) => self.leaf(token, ExprKind::Span(span)),
            Tok::Str(text) => self.leaf(token, ExprKind::Str(text)),
            Tok::Unit(text) => self.leaf(token, ExprKind::Unit(text)),
            Tok::Code(text) => self.leaf(token, ExprKind::Code(text)),
            Tok::Name("empty") => self.leaf(token, ExprKind::Empty),
            Tok::Name("if") => self.conditional(),
            Tok::Name(text) => self.name_or_call(token, text),
            Tok::LParen => self.parenthesized(),
            _ => Err(self.expected("expected-expression", "an expression")),
        }
    }

    fn leaf(&mut self, token: Token<'s>, kind: ExprKind<'s>) -> Parse<ExprId> {
        let first = self.exprs.next();
        self.cursor.bump();
        Ok(self.exprs.push(kind, token.loc, first))
    }

    /// A number, or an amount when a commodity follows it.
    fn number(&mut self, token: Token<'s>, num: Dec) -> Parse<ExprId> {
        let first = self.exprs.next();
        self.cursor.bump();
        let next = self.cursor.peek();
        let Tok::Unit(text) = next.tok else {
            return Ok(self.exprs.push(ExprKind::Num(num), token.loc, first));
        };
        self.cursor.bump();
        let unit = Name { text, loc: next.loc };
        Ok(self.exprs.push(ExprKind::Amount(num, unit), token.loc.to(next.loc), first))
    }

    /// A name, or a call when `(` touches it: `total(in, year)`. The callee is
    /// not a node of its own, so it never appears in the arena's evaluation
    /// order.
    fn name_or_call(&mut self, token: Token<'s>, text: &'s str) -> Parse<ExprId> {
        let first = self.exprs.next();
        self.cursor.bump();
        let next = self.cursor.peek();
        if !matches!(next.tok, Tok::LParen) || next.loc.start != token.loc.end {
            return Ok(self.exprs.push(ExprKind::Name(text), token.loc, first));
        }
        self.cursor.bump();
        let args = self.list(next, Delim::Paren)?;
        let callee = Name { text, loc: token.loc };
        Ok(self.exprs.push(ExprKind::Call(callee, args), self.loc_from(token.loc.start as usize), first))
    }

    /// Comma-separated expressions up to the closer of the bracket `open`.
    fn list(&mut self, open: Token<'s>, delim: Delim) -> Parse<Box<[ExprId]>> {
        let mut items = Vec::new();
        let empty_call = matches!(delim, Delim::Paren) && matches!(self.cursor.peek().tok, Tok::RParen);
        if !empty_call {
            loop {
                items.push(self.expression()?);
                if self.cursor.eat(Tok::Comma).is_none() {
                    break;
                }
            }
        }
        self.close(open.loc, delim)?;
        Ok(items.into_boxed_slice())
    }

    /// Grouping leaves no node: `(a + b)` is the node of `a + b`.
    fn parenthesized(&mut self) -> Parse<ExprId> {
        let open = self.cursor.bump();
        let inner = self.expression()?;
        self.close(open.loc, Delim::Paren)?;
        Ok(inner)
    }

    fn conditional(&mut self) -> Parse<ExprId> {
        let first = self.exprs.next();
        let keyword = self.cursor.bump();
        let condition = self.expression()?;
        self.conditional_word("then")?;
        let then = self.expression()?;
        self.conditional_word("else")?;
        let otherwise = self.expression()?;
        Ok(self.exprs.push(ExprKind::If(condition, then, otherwise), self.loc_from(keyword.loc.start as usize), first))
    }

    fn conditional_word(&mut self, word: &str) -> Parse<()> {
        if self.eat_word(word).is_some() {
            return Ok(());
        }
        let token = self.cursor.peek();
        let diag = self
            .unexpected(token, "incomplete-conditional", &format!("`{word}`"))
            .note("a conditional is written `if CONDITION then A else B`");
        self.fail(diag)
    }

    /// The right side of `is`: `wages`, `401k | ira`, `#house`, `self.purpose`.
    fn alternatives(&mut self) -> Parse<Box<[ExprId]>> {
        let mut alternatives = vec![self.alternative()?];
        while self.cursor.eat(Tok::Bar).is_some() {
            alternatives.push(self.alternative()?);
        }
        Ok(alternatives.into_boxed_slice())
    }

    /// An alternative is a name even when it is spelled like a number: kinds
    /// such as `529` are written the same way as the number 529.
    fn alternative(&mut self) -> Parse<ExprId> {
        let token = self.cursor.peek();
        match self.integer_name(token) {
            Some(name) => self.leaf(token, ExprKind::Name(name.text)),
            None => self.primary(),
        }
    }
}

fn too_deep(loc: Loc) -> Diagnostic {
    Diagnostic::error("expression-too-deep", format!("expressions nest at most {MAX_DEPTH} levels"))
        .label(loc, "nested too deeply")
        .help("bind an inner part with `let` and use its name")
}
