//! The v4 spellings the parser still reads, and the one warning that counts them.
//!
//! They are not a second grammar. A leg that names no arrow, an amount on each side of an arrow, an amount before an
//! arrow with nothing after it, and an arrow with no subject are what the v5 productions parse too; the parser only
//! notes, as it settles a line, that this is one of them. So taking them out is a decision about one warning, and
//! `axiom fmt --upgrade` finds the lines to rewrite in the tree the parser built, with no recogniser of its own.
//!
//! A book written in v4 has hundreds of such lines, so they are counted and reported once for the file, as tabs are,
//! with a label at the first of each form: the counts are small fixed arrays, merged in order from piece to piece.

use axiom_core::{Diagnostic, Loc};

/// A line spelled the way v4 did. What none of them can be told by is a name: a party-subject `->` is the same text in v4
/// and v5, and is the upgrade's to find, with the book.
#[derive(Clone, Copy)]
pub(crate) enum Form {
    BareLeg,
    TwoAmounts,
    DanglingAmount,
    NoSubject,
}

impl Form {
    const ALL: [Form; 4] = [Form::BareLeg, Form::TwoAmounts, Form::DanglingAmount, Form::NoSubject];

    /// What a line of this form is (one, and many), and how v5 writes it.
    const fn says(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Form::BareLeg => {
                ("a leg with no arrow", "legs with no arrow", "a leg leads with its arrow: `-> irs 692 USD`")
            }
            Form::TwoAmounts => (
                "an amount on both sides of the arrow",
                "lines with an amount on both sides of the arrow",
                "an exchange states one amount and its price: `checking -> broker 5.58 VTI @ 268.57 USD`",
            ),
            Form::DanglingAmount => (
                "an amount before an arrow with nothing after it",
                "lines with an amount before an arrow with nothing after it",
                "the amount follows the arrow: `checking -> 900.00 EUR`",
            ),
            Form::NoSubject => (
                "an arrow with no subject",
                "arrows with no subject",
                "the end that takes comes first: `checking <- 100 USD`",
            ),
        }
    }
}

/// How many lines of one form a piece has, and where the first is.
#[derive(Clone, Copy, Default)]
struct Seen {
    count: u32,
    first: Option<Loc>,
}

/// The lines of a piece, or of a file, that are written the v4 way, by form.
#[derive(Clone, Copy, Default)]
pub(crate) struct Old([Seen; Form::ALL.len()]);

impl Old {
    /// Notes `count` lines of `form`, the first of them at `first`.
    pub fn note(&mut self, form: Form, first: Loc, count: u32) {
        let seen = &mut self.0[form as usize];
        seen.count += count;
        seen.first.get_or_insert(first);
    }

    /// Adds the lines of the piece that follows.
    pub fn merge(&mut self, later: Old) {
        for (seen, later) in self.0.iter_mut().zip(later.0) {
            seen.count += later.count;
            seen.first = seen.first.or(later.first);
        }
    }

    /// One warning for the whole file, whatever number of lines are written the v4 way.
    pub fn diagnostic(&self) -> Option<Diagnostic> {
        let seen = Form::ALL.into_iter().zip(self.0);
        let mut found: Vec<_> = seen.filter_map(|(form, seen)| Some((seen.first?, form, seen.count))).collect();
        found.sort_by_key(|(first, ..)| first.start);
        let (&(at, form, count), rest) = found.split_first()?;
        let message = match found.iter().map(|(.., count)| count).sum() {
            1 => "a line is written the v4 way".to_string(),
            n => format!("{n} lines are written the v4 way"),
        };
        let mut diag = Diagnostic::warning("v4-syntax", message).label(at, describe(form, count));
        for &(at, form, count) in rest {
            diag = diag.context(at, describe(form, count));
        }
        for (_, form, _) in &found {
            diag = diag.help(form.says().2);
        }
        Some(diag.help("`axiom fmt --upgrade` writes them the v5 way, and checks that the book says the same"))
    }
}

/// What a label says of the first line of a form, of which there are `count`.
fn describe(form: Form, count: u32) -> String {
    let (one, many, _) = form.says();
    match count {
        1 => one.to_string(),
        n => format!("the first of {n} {many}"),
    }
}
