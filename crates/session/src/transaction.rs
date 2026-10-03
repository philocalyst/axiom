//! A transaction as a typed value, and the line that says it.
//!
//! A client that adds a transaction (an MCP tool, a form in a GUI) has a day, two ends, an amount and a few words, not
//! a line of the language. A line it assembles itself is one the language may read differently from what it meant: a
//! name with a space in it becomes two tokens, a description with a quote ends its string early, a newline starts a
//! second line. So the line is written here, from the typed parts, each checked to be one token or escaped, in the
//! order the language writes a flow's tail (`#purpose`, `"description"`, `^code`), the way `axiom fmt` lays out a
//! line of its own. What the line *means*, whether `checking` is an account and `food` a purpose, is the book's to
//! say once the edit is applied, and it says it as a diagnostic.

use std::fmt::{self, Write as _};

use axiom_core::num::POW10;
use axiom_core::{Day, FileId};
use axiom_report::Money;

use crate::Edit;

/// One flow, to be added to the journal: `DAY FROM -> TO AMOUNT`, and what is said of it.
#[derive(Clone, Copy, Debug)]
pub struct NewTransaction<'a> {
    pub day: Day,
    /// Where the money leaves: an account, an owner, a position.
    pub from: &'a str,
    /// Where it arrives: a party, an account, a promise.
    pub to: &'a str,
    /// How much, in quanta of the commodity, at the commodity's precision. Never negative: a flow's direction is its arrow.
    pub amount: Money<'a>,
    /// What the flow was for, without its `#`.
    pub purpose: Option<&'a str>,
    pub description: Option<&'a str>,
    /// Codes that name the flow, without their `^`.
    pub codes: &'a [&'a str],
}

/// Why a transaction cannot be written as a line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Unwritable {
    /// Not one token: it is empty, or has a space, a quote, a sign or anything else that would not stay one word.
    NotAWord(String),
    /// A flow's amount is never negative.
    Negative,
}

impl fmt::Display for Unwritable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Unwritable::NotAWord(text) => write!(f, "`{text}` is not one word of the language"),
            Unwritable::Negative => f.write_str("a flow's amount cannot be negative: its arrow says which way it goes"),
        }
    }
}

impl std::error::Error for Unwritable {}

impl NewTransaction<'_> {
    /// The line, with no line ending.
    pub fn line(&self) -> Result<String, Unwritable> {
        let mut line = format!("{} {} -> {} {}", self.day, word(self.from)?, word(self.to)?, amount(self.amount)?);
        if let Some(purpose) = self.purpose {
            let _ = write!(line, " #{}", word(purpose)?);
        }
        if let Some(description) = self.description {
            line.push(' ');
            quote(&mut line, description);
        }
        for code in self.codes {
            let _ = write!(line, " ^{}", word(code)?);
        }
        Ok(line)
    }

    /// The edit that adds this transaction to the end of `file`.
    pub fn append_to(&self, file: FileId) -> Result<Edit, Unwritable> {
        Ok(Edit::Append { file, text: self.line()? })
    }
}

/// `text` if it is one token of the language's names, units and codes: it starts with a letter or a digit and goes on
/// with those and `_ - / . :`.
fn word(text: &str) -> Result<&str, Unwritable> {
    let inside = |byte: u8| byte.is_ascii_alphanumeric() || b"_-/.:".contains(&byte);
    let starts = text.bytes().next().is_some_and(|byte| byte.is_ascii_alphanumeric());
    if starts && text.bytes().all(inside) { Ok(text) } else { Err(Unwritable::NotAWord(text.to_string())) }
}

/// `1234.50 USD`: the quanta with the commodity's decimal point, no digit grouping, then the unit.
fn amount(money: Money<'_>) -> Result<String, Unwritable> {
    if money.qty.is_negative() {
        return Err(Unwritable::Negative);
    }
    let (whole, scale) = (i128::from(money.qty.0), usize::from(money.scale));
    let (units, quanta) = (whole / POW10[scale], whole % POW10[scale]);
    let number = if scale == 0 { units.to_string() } else { format!("{units}.{quanta:0scale$}") };
    Ok(format!("{number} {}", word(money.unit)?))
}

/// `"text"`, with the escapes the language reads: `\"`, `\\`, `\n` and `\t`.
fn quote(line: &mut String, text: &str) {
    line.push('"');
    for ch in text.chars() {
        match ch {
            '"' => line.push_str("\\\""),
            '\\' => line.push_str("\\\\"),
            '\n' => line.push_str("\\n"),
            '\t' => line.push_str("\\t"),
            other => line.push(other),
        }
    }
    line.push('"');
}

#[cfg(test)]
mod tests {
    use axiom_core::{Day, FileId, Qty};

    use super::*;

    fn groceries() -> NewTransaction<'static> {
        NewTransaction {
            day: Day::parse(b"2026-03-01").unwrap(),
            from: "checking",
            to: "grocer",
            amount: Money { qty: Qty(1_250), scale: 2, unit: "USD" },
            purpose: Some("food"),
            description: Some("weekly shop"),
            codes: &["receipt-9", "w10"],
        }
    }

    #[test]
    fn the_line_says_the_flow_in_the_order_of_the_language() {
        assert_eq!(
            groceries().line().unwrap(),
            "2026-03-01 checking -> grocer 12.50 USD #food \"weekly shop\" ^receipt-9 ^w10"
        );
        let bare = NewTransaction { purpose: None, description: None, codes: &[], ..groceries() };
        assert_eq!(bare.line().unwrap(), "2026-03-01 checking -> grocer 12.50 USD");
    }

    #[test]
    fn an_amount_keeps_its_precision_and_never_groups_digits() {
        let amount_of = |qty, scale| {
            let money = Money { qty: Qty(qty), scale, unit: "USD" };
            NewTransaction { amount: money, purpose: None, description: None, codes: &[], ..groceries() }
                .line()
                .unwrap()
                .rsplit(" -> grocer ")
                .next()
                .unwrap()
                .to_string()
        };
        assert_eq!(amount_of(123_456_789, 2), "1234567.89 USD");
        assert_eq!(amount_of(5, 2), "0.05 USD", "the fraction keeps its leading zero");
        assert_eq!(amount_of(7, 0), "7 USD", "a commodity with no places has no point");
        assert_eq!(amount_of(100_000_000, 8), "1.00000000 USD");
    }

    #[test]
    fn what_would_not_stay_one_word_or_one_line_is_refused_or_escaped() {
        for bad in ["", "two words", "new\nline", "x\"y", "#tag", "a->b", "-1"] {
            let transaction = NewTransaction { from: bad, ..groceries() };
            assert_eq!(transaction.line(), Err(Unwritable::NotAWord(bad.to_string())), "{bad:?}");
        }
        let negative = NewTransaction { amount: Money { qty: Qty(-1), scale: 2, unit: "USD" }, ..groceries() };
        assert_eq!(negative.line(), Err(Unwritable::Negative));
        let tricky = NewTransaction { description: Some("a \"quote\", a \\ and\ttwo\nlines"), ..groceries() };
        let line = tricky.line().unwrap();
        assert!(line.contains(r#""a \"quote\", a \\ and\ttwo\nlines""#), "{line}");
        assert!(!line.contains('\n'), "one line, whatever the description says");
    }

    #[test]
    fn the_line_is_one_the_formatter_leaves_as_it_is() {
        let line = groceries().line().unwrap() + "\n";
        let (file, diagnostics) = axiom_syntax::parse(FileId(0), &line, axiom_syntax::Folder::default());
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(file.format(), line);
    }

    #[test]
    fn appending_it_is_an_edit_to_that_file() {
        let edit = groceries().append_to(FileId(3)).unwrap();
        assert_eq!(edit.file(), FileId(3));
        assert!(matches!(edit, Edit::Append { ref text, .. } if text.starts_with("2026-03-01 checking")));
    }
}
