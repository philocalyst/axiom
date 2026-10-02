//! Borrowed cells shared by the row and tagged readers.

use std::borrow::Cow;

use crate::Span;

pub(crate) const ABSENT: Span = Span { start: usize::MAX, end: usize::MAX };

pub(crate) struct Cell<'t> {
    pub text: Cow<'t, str>,
    pub span: Span,
}

#[derive(Default)]
pub(crate) struct MemoJoin<'t> {
    first: Option<Cow<'t, str>>,
    joined: Option<String>,
    span: Option<Span>,
}

impl<'t> MemoJoin<'t> {
    pub(crate) fn push(&mut self, cell: &Cell<'t>) {
        if cell.text.is_empty() {
            return;
        }
        if let Some(joined) = &mut self.joined {
            joined.push(' ');
            joined.push_str(&cell.text);
        } else if let Some(first) = self.first.take() {
            let mut joined = match first {
                Cow::Borrowed(text) => text.to_owned(),
                Cow::Owned(text) => text,
            };
            joined.push(' ');
            joined.push_str(&cell.text);
            self.joined = Some(joined);
        } else {
            self.first = Some(cell.text.clone());
            self.span = Some(cell.span);
        }
    }

    pub(crate) fn finish(self) -> Option<(Cow<'t, str>, Span)> {
        let span = self.span.unwrap_or(ABSENT);
        match self.joined {
            Some(joined) => Some((Cow::Owned(joined), span)),
            None => self.first.map(|first| (first, span)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memo_join_keeps_all_cells_after_promoting_an_owned_first_cell() {
        let mut joined = MemoJoin::default();
        joined.push(&Cell { text: Cow::Owned("Remit".to_string()), span: Span { start: 4, end: 9 } });
        joined.push(&Cell { text: Cow::Borrowed("Card"), span: Span { start: 10, end: 14 } });
        joined.push(&Cell { text: Cow::Borrowed("credit"), span: Span { start: 15, end: 21 } });
        let (memo, span) = joined.finish().unwrap();
        assert_eq!(memo, "Remit Card credit");
        assert_eq!(span, Span { start: 4, end: 9 });
    }

    #[test]
    fn a_single_borrowed_memo_stays_borrowed() {
        let mut joined = MemoJoin::default();
        joined.push(&Cell { text: Cow::Borrowed("Refund"), span: Span { start: 8, end: 14 } });
        let (memo, span) = joined.finish().unwrap();
        assert!(matches!(memo, Cow::Borrowed("Refund")));
        assert_eq!(span, Span { start: 8, end: 14 });
    }
}
