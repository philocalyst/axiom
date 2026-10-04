//! A cell said as text, once.
//!
//! A terminal table and a JSON document write the same words for the same cell: `^code`, `#purpose`, `12.00 USD`, `each year
//! closing 04-15`, a sentence of parts with the blank ones left out and no space before a comma. What differs is what the
//! one that reads them does around the words (a terminal inks them, pads an amount's unit to its column's and groups a
//! count; a document does none of it), so the words are written here, straight to a [`CellSink`] that says what differs.
//! A part of a sentence is written to the sink as it is met: no part is made a `String` first, and the sink decides what
//! a mark, a count and an amount are to it.

use std::fmt;

use axiom_core::calendar::Window;
use axiom_core::{Days, Qty};
use axiom_model::{Closing, Period, Trigger};

use crate::{Cell, SourceProvider, percent};

/// What the words a cell is writing are, for a sink that draws some of them differently.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mark {
    /// Wording, drawn as the row it is in is drawn.
    Plain,
    /// A negative amount.
    Negative,
    /// Where something is written in the sources.
    Source,
}

/// Where a cell is written. The defaults are a document's: nothing is drawn, a count has no grouping, and an amount is
/// its digits and its unit.
pub trait CellSink: fmt::Write {
    /// What the text that follows is.
    fn mark(&mut self, _: Mark) {}

    /// The number of things in `3 flows`.
    fn count(&mut self, count: usize) -> fmt::Result {
        write!(self, "{count}")
    }

    /// An amount: its digits at the commodity's precision, a space, and the unit.
    fn amount(&mut self, qty: Qty, scale: u8, unit: &str) -> fmt::Result {
        write!(self, "{} {unit}", qty.show(scale))
    }
}

impl Cell<'_> {
    /// The words of the cell, written to `out`: a place of the sources as `path:line`, and nothing for one that has no
    /// position.
    pub fn write_plain(&self, out: &mut impl CellSink, sources: &dyn SourceProvider) -> fmt::Result {
        out.mark(Mark::Plain);
        match self {
            Cell::Blank => Ok(()),
            Cell::Word(text) | Cell::Name(text) => out.write_str(text),
            Cell::Text(text) | Cell::Said(text) => out.write_str(text),
            Cell::Code(code) => write!(out, "^{code}"),
            Cell::Purpose(purpose) => write!(out, "#{purpose}"),
            Cell::Amount { qty, scale, unit } => out.amount(*qty, *scale, unit),
            Cell::Day(day) => write!(out, "{day}"),
            Cell::Span(span) => write!(out, "{span}"),
            Cell::Period(days) => write_period(out, *days),
            Cell::Percent(ratio) => out.write_str(&percent(*ratio)),
            Cell::Number(ratio) => write!(out, "{ratio}"),
            Cell::Count(count, "") => write!(out, "{}", Qty(*count as i64).show(0)),
            Cell::Count(count, noun) => {
                out.count(*count)?;
                write!(out, " {noun}{}", if *count == 1 { "" } else { "s" })
            }
            Cell::Trigger(trigger) => write_trigger(out, *trigger),
            Cell::Source(loc) => {
                let Some(position) = sources.describe(*loc) else { return Ok(()) };
                out.mark(Mark::Source);
                write!(out, "{}:{}", position.path, position.line)
            }
            Cell::Join(separator, parts) => {
                let mut any = false;
                for part in parts.iter().filter(|part| part.is_visible(sources)) {
                    if any && !(*separator == " " && part.starts_with_punctuation(sources)) {
                        out.write_str(separator)?;
                    }
                    part.write_plain(out, sources)?;
                    any = true;
                }
                Ok(())
            }
        }
    }

    /// Whether the cell writes anything: a blank, an empty text and a place with no path do not.
    pub fn is_visible(&self, sources: &dyn SourceProvider) -> bool {
        match self {
            Cell::Blank => false,
            Cell::Text(text) | Cell::Said(text) => !text.is_empty(),
            Cell::Word(text) | Cell::Name(text) | Cell::Purpose(text) => !text.is_empty(),
            Cell::Source(loc) => sources.describe(*loc).is_some_and(|position| !position.path.is_empty()),
            Cell::Join(_, parts) => parts.iter().any(|part| part.is_visible(sources)),
            _ => true,
        }
    }

    /// Whether the first thing the cell writes is punctuation that closes what is before it: no space goes between.
    fn starts_with_punctuation(&self, sources: &dyn SourceProvider) -> bool {
        let first = match self {
            Cell::Text(text) | Cell::Said(text) => text.chars().next(),
            Cell::Word(text) | Cell::Name(text) => text.chars().next(),
            Cell::Purpose(text) => text.chars().next().or(Some('#')),
            Cell::Source(loc) => sources.describe(*loc).and_then(|position| position.path.chars().next()),
            Cell::Join(_, parts) => {
                let first = parts.iter().find(|part| part.is_visible(sources));
                return first.is_some_and(|part| part.starts_with_punctuation(sources));
            }
            _ => None,
        };
        first.is_some_and(|ch| matches!(ch, ',' | ';' | ':' | '.' | ')'))
    }
}

/// `2026-03`, `2026`, `on 2026-03-31`, `ever`, or the range itself.
fn write_period(out: &mut impl fmt::Write, days: Days) -> fmt::Result {
    match (Window::exactly(days), days.single()) {
        (Some(window), _) => write!(out, "{window}"),
        (None, Some(day)) => write!(out, "on {day}"),
        (None, None) if days == Days::ALWAYS => out.write_str("ever"),
        (None, None) => write!(out, "{}..{}", days.first(), days.last()),
    }
}

/// When a law fires, in the words it is written with.
pub(crate) fn write_trigger(out: &mut impl fmt::Write, trigger: Trigger) -> fmt::Result {
    match trigger {
        Trigger::In => out.write_str("on in"),
        Trigger::Out => out.write_str("on out"),
        Trigger::Gain => out.write_str("on gain"),
        Trigger::Spend => out.write_str("on spend"),
        Trigger::Flow => out.write_str("on flow"),
        Trigger::Each(Period::Month, _) => out.write_str("each month"),
        Trigger::Each(Period::Year, None) => out.write_str("each year"),
        Trigger::Each(Period::Year, Some(Closing { month, day })) => {
            write!(out, "each year closing {month:02}-{day:02}")
        }
        Trigger::By(_) => out.write_str("by a date"),
        Trigger::Always => out.write_str("always"),
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;
    use std::fmt::Write as _;

    use axiom_core::{FileId, Loc, Ratio};

    use super::*;
    use crate::SourcePosition;

    /// A sink that is only a document's: every default.
    struct Document(String);

    impl fmt::Write for Document {
        fn write_str(&mut self, text: &str) -> fmt::Result {
            self.0.push_str(text);
            Ok(())
        }
    }

    impl CellSink for Document {}

    /// A sink that records what it is marked as, and pads and groups as a terminal does.
    #[derive(Default)]
    struct Marked {
        text: String,
        marks: Vec<Mark>,
    }

    impl fmt::Write for Marked {
        fn write_str(&mut self, text: &str) -> fmt::Result {
            self.text.push_str(text);
            Ok(())
        }
    }

    impl CellSink for Marked {
        fn mark(&mut self, mark: Mark) {
            self.marks.push(mark);
        }

        fn count(&mut self, count: usize) -> fmt::Result {
            write!(self, "<{count}>")
        }
    }

    struct Sources;

    impl SourceProvider for Sources {
        fn locate(&self, _: &str, _: usize) -> Option<Loc> {
            None
        }

        fn describe(&self, loc: Loc) -> Option<SourcePosition<'_>> {
            (loc.file == FileId(0)).then_some(SourcePosition { path: "journal.ax", line: 3, column: 1 })
        }
    }

    fn said(cell: &Cell<'_>) -> String {
        let mut out = Document(String::new());
        cell.write_plain(&mut out, &Sources).unwrap();
        out.0
    }

    #[test]
    fn a_sentence_leaves_out_what_is_blank_and_does_not_space_before_punctuation() {
        let sentence = Cell::Join(
            " ",
            vec![
                Cell::Word("Income"),
                Cell::Join(
                    " ",
                    vec![Cell::Amount { qty: Qty(1_200), scale: 2, unit: "USD" }, Cell::Said(Cow::Borrowed(","))],
                ),
                Cell::Blank,
                Cell::Text(Cow::Borrowed("")),
                Cell::Source(Loc::new(FileId(1), 0, 1)),
                Cell::Word("today"),
            ],
        );
        assert_eq!(said(&sentence), "Income 12.00 USD, today");
        let listed = Cell::Join(
            " · ",
            vec![Cell::Code("a"), Cell::Blank, Cell::Purpose("food"), Cell::Source(Loc::new(FileId(0), 0, 1))],
        );
        assert_eq!(said(&listed), "^a · #food · journal.ax:3");
    }

    #[test]
    fn what_is_said_is_the_same_whatever_the_sink_does_around_it() {
        let cells = [
            Cell::Day(axiom_core::Day::from_ymd(2026, 3, 4).unwrap()),
            Cell::Percent(Ratio::percent(35, 1).unwrap()),
            Cell::Period(Days::ALWAYS),
            Cell::Trigger(Trigger::Each(Period::Year, Some(Closing { month: 4, day: 15 }))),
            Cell::Count(0, ""),
        ];
        let words: Vec<_> = cells.iter().map(said).collect();
        assert_eq!(words, ["2026-03-04", "3.5%", "ever", "each year closing 04-15", "0"]);
    }

    #[test]
    fn a_sink_is_marked_for_what_it_is_to_draw_differently_and_counts_its_own_way() {
        let mut out = Marked::default();
        Cell::Count(1_000, "flow").write_plain(&mut out, &Sources).unwrap();
        assert_eq!(out.text, "<1000> flows", "the count is the sink's, the plural is the words'");
        let mut out = Marked::default();
        Cell::Count(1, "flow").write_plain(&mut out, &Sources).unwrap();
        assert_eq!(out.text, "<1> flow");
        let mut out = Marked::default();
        Cell::Source(Loc::new(FileId(0), 0, 1)).write_plain(&mut out, &Sources).unwrap();
        assert_eq!(out.marks, [Mark::Plain, Mark::Source]);
        let mut out = Marked::default();
        Cell::Source(Loc::new(FileId(1), 0, 1)).write_plain(&mut out, &Sources).unwrap();
        assert_eq!(
            (out.text.as_str(), out.marks.as_slice()),
            ("", &[Mark::Plain][..]),
            "a place that is nowhere says nothing"
        );
    }

    #[test]
    fn a_count_with_no_noun_is_grouped_in_every_sink() {
        let mut out = Marked::default();
        Cell::Count(12_345, "").write_plain(&mut out, &Sources).unwrap();
        assert_eq!(out.text, "12,345");
    }
}
