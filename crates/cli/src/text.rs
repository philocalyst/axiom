//! Small helpers for prose.

use axiom_core::Qty;

/// `1 error`, `2 errors`, `1,284 flows`. Every noun here pluralizes with an `s`.
pub fn plural(count: usize, noun: &str) -> String {
    let suffix = if count == 1 { "" } else { "s" };
    format!("{} {noun}{suffix}", Qty(count as i64).show(0))
}

/// Breaks `text` into lines of at most `width` characters at word boundaries.
/// A word longer than `width` gets a line to itself. Line breaks in `text`
/// start new paragraphs, and a paragraph that starts with a space is
/// preformatted (a table, a list of candidates) and is kept as it is. Always
/// returns at least one line.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.lines() {
        if paragraph.starts_with(' ') {
            lines.push(paragraph.trim_end().to_string());
            continue;
        }
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > width {
                lines.push(std::mem::take(&mut line));
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
        lines.push(line);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plurals() {
        assert_eq!(plural(1, "error"), "1 error");
        assert_eq!(plural(0, "warning"), "0 warnings");
        assert_eq!(plural(1284, "flow"), "1,284 flows");
    }

    #[test]
    fn wraps_at_word_boundaries() {
        assert_eq!(wrap("one two three four", 9), ["one two", "three", "four"]);
        assert_eq!(wrap("unbreakable words", 4), ["unbreakable", "words"]);
        assert_eq!(wrap("first\n\nsecond", 40), ["first", "", "second"]);
        assert_eq!(wrap("", 40), [""]);
    }

    #[test]
    fn indented_lines_are_left_alone() {
        let candidates = "Each would realize:\n  2026-01-22   7 VTI   120.00 USD\n  2026-03-02   3 VTI   -4.10 USD";
        assert_eq!(
            wrap(candidates, 20),
            ["Each would realize:", "  2026-01-22   7 VTI   120.00 USD", "  2026-03-02   3 VTI   -4.10 USD"]
        );
    }
}
