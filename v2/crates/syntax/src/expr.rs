//! Expressions: precedence climbing into the file's post-order arena.
//!
//! Every node is added after its children, so a subtree is the contiguous run
//! `first..=root` of the arena. `first` is read off the arena *before* a
//! subtree is parsed (or off its leftmost child afterwards), which is why a
//! node never needs to be revisited.

use axiom_core::{Dec, Diagnostic, Loc};

use crate::ast::*;
use crate::lex::{Punct, Tok, Token};
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

/// How tightly an infix operator binds. (`is` is not one of them: its right
/// side is a list of alternatives.)
fn power(op: BinOp) -> u8 {
    match op {
        BinOp::Or => OR,
        BinOp::And => AND,
        BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => COMPARE,
        BinOp::Add | BinOp::Sub => ADD,
        BinOp::Mul | BinOp::Div => MUL,
    }
}

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
        let start = self.peek().loc.start;
        let mut expr = self.atom()?;
        let first = self.expr(expr).first;
        // A postfix operator must touch its operand, so `limit[year]` is an
        // index but `holds VTI [x]` is two arguments.
        while self.peek().loc.start == self.lexer.prev_end() {
            let token = self.peek();
            let kind = match token.tok {
                Tok::Punct(Punct::Dot) => {
                    self.bump();
                    ExprKind::Field(expr, self.name("expected-name", "a field name after `.`")?)
                }
                Tok::Punct(Punct::LBracket) => {
                    self.bump();
                    ExprKind::Index(expr, self.list(token.loc, Punct::RBracket)?)
                }
                _ => break,
            };
            expr = self.node(kind, self.loc_from(start as usize), first);
        }
        Ok(expr)
    }

    /// Every way an expression nests (parentheses, arguments, `if`, prefix
    /// operators) passes through here, so this is where depth is limited.
    fn binary(&mut self, min_power: u8) -> Parse<ExprId> {
        if self.depth == MAX_DEPTH {
            return self.fail(too_deep(self.peek().loc));
        }
        self.depth += 1;
        let parsed = self.binary_within_limit(min_power);
        self.depth -= 1;
        parsed
    }

    fn binary_within_limit(&mut self, min_power: u8) -> Parse<ExprId> {
        let mut lhs = self.prefix()?;
        while let Some((op, power)) = self.infix().filter(|&(_, power)| power >= min_power) {
            self.bump();
            lhs = self.infix_rest(lhs, op, power)?;
        }
        Ok(lhs)
    }

    fn infix(&self) -> Option<(Infix, u8)> {
        let op = match self.tok() {
            Tok::Name("is") => return Some((Infix::Is, COMPARE)),
            Tok::Name("or") => BinOp::Or,
            Tok::Name("and") => BinOp::And,
            Tok::Punct(punct) => punct.infix()?,
            _ => return None,
        };
        Some((Infix::Binary(op), power(op)))
    }

    /// The right-hand side of an operator whose token was just consumed.
    fn infix_rest(&mut self, lhs: ExprId, op: Infix, power: u8) -> Parse<ExprId> {
        let (first, start) = (self.expr(lhs).first, self.expr(lhs).loc.start as usize);
        let kind = match op {
            Infix::Is => ExprKind::Is(lhs, self.alternatives()?),
            Infix::Binary(op) => ExprKind::Binary(op, lhs, self.binary(power + 1)?),
        };
        let node = self.node(kind, self.loc_from(start), first);
        if power == COMPARE && matches!(self.infix(), Some((_, COMPARE))) {
            let diag = Diagnostic::error("chained-comparison", "comparisons do not chain")
                .label(self.peek().loc, "a second comparison")
                .help("write each comparison and join them with `and`: `a < b and b < c`");
            return self.fail(diag);
        }
        Ok(node)
    }

    fn prefix(&mut self) -> Parse<ExprId> {
        let token = self.peek();
        let (op, power) = match token.tok {
            Tok::Name("not") => (UnOp::Not, COMPARE),
            Tok::Punct(Punct::Minus) => (UnOp::Neg, NEGATE),
            _ => return self.primary(),
        };
        self.bump();
        let first = self.next_expr();
        let operand = self.binary(power)?;
        Ok(self.node(ExprKind::Unary(op, operand), self.loc_from(token.loc.start as usize), first))
    }

    fn atom(&mut self) -> Parse<ExprId> {
        let token = self.peek();
        match token.tok {
            Tok::Number(num) => self.number(token, num),
            Tok::Percent(num) => self.leaf(token, ExprKind::Pct(num)),
            Tok::Date(day) => self.leaf(token, ExprKind::Date(day)),
            Tok::MonthDay(..) => {
                let (first, day) = (self.next_expr(), self.date("a date")?);
                Ok(self.node(ExprKind::Date(day), token.loc, first))
            }
            Tok::Span(span) => self.leaf(token, ExprKind::Span(span)),
            Tok::Str(text) => self.leaf(token, ExprKind::Str(text)),
            Tok::Unit(text) => self.leaf(token, ExprKind::Unit(Name(text))),
            Tok::Purpose(name) => self.leaf(token, ExprKind::Purpose(name)),
            Tok::Code(code) => self.leaf(token, ExprKind::Code(code)),
            Tok::Name("empty") => self.leaf(token, ExprKind::Empty),
            Tok::Name("if") => self.conditional(),
            Tok::Name(text) => self.name_or_call(token, text),
            Tok::Punct(Punct::LParen) => {
                // Grouping leaves no node: `(a + b)` is the node of `a + b`.
                self.bump();
                let inner = self.expression()?;
                self.close(token.loc, Punct::RParen)?;
                Ok(inner)
            }
            _ => Err(self.expected("expected-expression", "an expression")),
        }
    }

    fn leaf(&mut self, token: Token<'s>, kind: ExprKind<'s>) -> Parse<ExprId> {
        let first = self.next_expr();
        self.bump();
        Ok(self.node(kind, token.loc, first))
    }

    /// A number, or an amount when a commodity follows it.
    fn number(&mut self, token: Token<'s>, num: Dec) -> Parse<ExprId> {
        let first = self.next_expr();
        self.bump();
        let next = self.peek();
        let Tok::Unit(_) = next.tok else { return Ok(self.node(ExprKind::Num(num), token.loc, first)) };
        self.bump();
        let loc = token.loc.to(next.loc);
        Ok(self.node(ExprKind::Amount(Amount(self.text(loc))), loc, first))
    }

    /// A name, or a call when `(` touches it: `total(in, year)`. The callee is
    /// not a node of its own, so it never appears in the arena's evaluation
    /// order.
    fn name_or_call(&mut self, token: Token<'s>, text: &'s str) -> Parse<ExprId> {
        let first = self.next_expr();
        self.bump();
        let paren = self.peek();
        if !self.at(Punct::LParen) || paren.loc.start != token.loc.end {
            return Ok(self.node(ExprKind::Name(Name(text)), token.loc, first));
        }
        self.bump();
        let args = self.list(paren.loc, Punct::RParen)?;
        Ok(self.node(ExprKind::Call(Name(text), args), self.loc_from(token.loc.start as usize), first))
    }

    /// Comma-separated expressions up to the `closer` of the bracket opened at
    /// `open`. A call may have none.
    fn list(&mut self, open: Loc, closer: Punct) -> Parse<Many<ExprId>> {
        let start = self.roots.len();
        if closer == Punct::RBracket || !self.at(Punct::RParen) {
            loop {
                let item = self.expression()?;
                self.roots.push(item);
                if self.eat(Punct::Comma).is_none() {
                    break;
                }
            }
        }
        self.close(open, closer)?;
        Ok(self.roots_since(start))
    }

    /// Moves the expression roots collected since `start` to the table, as
    /// one run.
    pub fn roots_since(&mut self, start: usize) -> Many<ExprId> {
        let mark = self.mark::<ExprId>();
        for at in start..self.roots.len() {
            self.push(self.roots[at]);
        }
        self.roots.truncate(start);
        self.since(mark)
    }

    fn conditional(&mut self) -> Parse<ExprId> {
        let first = self.next_expr();
        let keyword = self.bump();
        let condition = self.expression()?;
        self.conditional_word("then")?;
        let then = self.expression()?;
        self.conditional_word("else")?;
        let otherwise = self.expression()?;
        Ok(self.node(ExprKind::If(condition, then, otherwise), self.loc_from(keyword.loc.start as usize), first))
    }

    fn conditional_word(&mut self, word: &str) -> Parse<()> {
        if self.eat_word(word).is_some() {
            return Ok(());
        }
        let diag = self
            .unexpected(self.peek(), "incomplete-conditional", &format!("`{word}`"))
            .note("a conditional is written `if CONDITION then A else B`");
        self.fail(diag)
    }

    /// The right side of `is`: `wages`, `401k | ira`, `#groceries`, `^inv-12`,
    /// `self.purpose`, and a purpose with its object: `repair of self`.
    pub fn alternatives(&mut self) -> Parse<Many<ExprId>> {
        let start = self.roots.len();
        loop {
            // An alternative is a name even when it is spelled like a number:
            // kinds such as `529` are written the same way as the number 529.
            let token = self.peek();
            let mut alternative = match self.integer_name(token) {
                Some(name) => self.leaf(token, ExprKind::Name(name)),
                None => self.primary(),
            }?;
            if self.eat_word("of").is_some() {
                let (first, object) = (self.expr(alternative).first, self.primary()?);
                let loc = self.loc_from(token.loc.start as usize);
                alternative = self.node(ExprKind::Of(alternative, object), loc, first);
            }
            self.roots.push(alternative);
            if self.eat(Punct::Pipe).is_none() {
                return Ok(self.roots_since(start));
            }
        }
    }
}

fn too_deep(loc: Loc) -> Diagnostic {
    Diagnostic::error("expression-too-deep", format!("expressions nest at most {MAX_DEPTH} levels"))
        .label(loc, "nested too deeply")
        .help("bind an inner part with `let` and use its name")
}
