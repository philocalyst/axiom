//! The marks under one source line: an underline for every label, and the
//! connectors that carry each label's text down to a row of its own.
//!
//! ```text
//!   retirement   2_600 USD
//!   ─────┬────   ────┬────
//!        │           ╰── this contribution
//!        ╰── a 401k (us/401k)
//! ```
//!
//! Labels hang from their underline right to left, so a connector never has to
//! cross another label's text.

use axiom_core::Set;

use crate::style::{Ink, Line};

/// A label on a single line, in display columns.
#[derive(Clone, Copy)]
pub struct LineLabel<'a> {
    /// The first column underlined.
    pub start: usize,
    /// One past the last column; always after `start`.
    pub end: usize,
    /// What the label says; empty for a mark with nothing to add.
    pub text: &'a str,
    /// The cause, rather than something relevant to it.
    pub primary: bool,
    pub ink: Ink,
}

impl LineLabel<'_> {
    fn width(&self) -> usize {
        self.end - self.start
    }
}

/// The rows to print under the source line: the underline, then one row for
/// each label that has text.
pub fn annotate(labels: &[LineLabel]) -> Vec<Line> {
    let anchors = anchors(labels);
    let mut rows = vec![underline(labels, &anchors)];
    let mut hanging: Vec<(usize, &LineLabel)> =
        anchors.iter().zip(labels).filter_map(|(&anchor, label)| Some((anchor?, label))).collect();
    hanging.sort_by_key(|&(anchor, _)| anchor);
    while let Some((anchor, label)) = hanging.pop() {
        let mut row = Line::new();
        for &(left, other) in &hanging {
            row.put(left, "│", other.ink);
        }
        row.put(anchor, "╰── ", label.ink);
        row.push(label.text, Ink::PLAIN);
        rows.push(row);
    }
    rows
}

/// The column each label's connector hangs from, or `None` for a label without
/// text. Narrow labels choose first, so a wide label that encloses them moves
/// aside rather than colliding with them.
fn anchors(labels: &[LineLabel]) -> Vec<Option<usize>> {
    let mut order: Vec<usize> = (0..labels.len()).filter(|&at| !labels[at].text.is_empty()).collect();
    order.sort_by_key(|&at| labels[at].width());
    let mut taken = Set::default();
    let mut anchors = vec![None; labels.len()];
    for at in order {
        let column = free_column(&labels[at], &taken);
        taken.insert(column);
        anchors[at] = Some(column);
    }
    anchors
}

/// The column nearest the middle of `label` that no other connector uses, and
/// preferably none is next to, so that neighbours stay distinct; just past its
/// end if the whole range is taken.
fn free_column(label: &LineLabel, taken: &Set<usize>) -> usize {
    let middle = label.start + label.width() / 2;
    // 0, -1, +1, -2, +2, … around the middle.
    let outward: Vec<usize> = (0..2 * label.width())
        .filter_map(|step| if step % 2 == 1 { middle.checked_sub(step.div_ceil(2)) } else { Some(middle + step / 2) })
        .filter(|column| (label.start..label.end).contains(column))
        .collect();
    let is_free = |column: &usize| !taken.contains(column);
    let has_room = |column: &usize| {
        is_free(column) && is_free(&(column + 1)) && column.checked_sub(1).is_none_or(|left| is_free(&left))
    };
    let beyond = || (label.end..).find(is_free).unwrap_or(label.end);
    outward.iter().copied().find(has_room).or_else(|| outward.iter().copied().find(is_free)).unwrap_or_else(beyond)
}

/// The row of `─`, `^` and `┬` right under the source. Wide labels are drawn
/// first so that narrower ones inside them stay visible.
fn underline(labels: &[LineLabel], anchors: &[Option<usize>]) -> Line {
    let mut row = Line::new();
    let mut widest_first: Vec<&LineLabel> = labels.iter().collect();
    widest_first.sort_by_key(|label| std::cmp::Reverse(label.width()));
    for label in widest_first {
        // Only a primary label with nothing to say is worth a caret; text
        // gets a connector, which needs a line to hang from.
        let mark = if label.primary && label.text.is_empty() { "^" } else { "─" };
        row.put(label.start, &mark.repeat(label.width()), label.ink);
    }
    for (anchor, label) in anchors.iter().zip(labels) {
        if let Some(column) = anchor {
            row.put(*column, "┬", label.ink);
        }
    }
    row
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::Terminal;

    fn label(start: usize, end: usize, text: &str, primary: bool) -> LineLabel<'_> {
        LineLabel { start, end, text, primary, ink: Ink::PLAIN }
    }

    fn draw(labels: &[LineLabel]) -> Vec<String> {
        annotate(labels).iter().map(|row| row.render(Terminal::plain(80).painter)).collect()
    }

    #[test]
    fn a_caret_for_a_primary_label_without_text_and_a_rule_for_a_secondary_one() {
        assert_eq!(draw(&[label(2, 5, "", true), label(8, 10, "", false)]), ["  ^^^   ──"]);
    }

    #[test]
    fn labels_hang_right_to_left() {
        let rows = draw(&[label(0, 5, "left", true), label(8, 13, "right", false)]);
        assert_eq!(rows, ["──┬──   ──┬──", "  │       ╰── right", "  ╰── left"]);
    }

    #[test]
    fn a_label_enclosing_another_moves_its_connector_aside() {
        let rows = draw(&[label(0, 9, "outer", false), label(2, 5, "inner", true)]);
        assert_eq!(rows, ["───┬─┬───", "   │ ╰── outer", "   ╰── inner"]);
    }

    #[test]
    fn two_labels_on_one_column_do_not_share_a_connector() {
        let rows = draw(&[label(3, 4, "a", true), label(3, 4, "b", false)]);
        assert_eq!(rows[0].matches('┬').count(), 2);
    }
}
