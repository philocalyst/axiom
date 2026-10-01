//! What would be written, as a unified diff. Sync only adds lines, so the two
//! texts are walked together: a line of the new text that is not the next line
//! of the old one is an addition.

use crate::write::Change;

/// Lines of the file shown around each change.
const CONTEXT: usize = 3;

enum Line<'t> {
    Same(&'t str),
    Added(&'t str),
    Removed(&'t str),
}

impl Change {
    /// The change as `diff -u` would show it; nothing for a file that stays as
    /// it is.
    pub fn diff(&self) -> String {
        let before = self.before.as_deref().unwrap_or("");
        let (old, new): (Vec<&str>, Vec<&str>) =
            (before.lines().collect(), self.after.lines().collect());
        let mut lines = Vec::new();
        let mut kept = old.iter().peekable();
        for &line in &new {
            match kept.peek() {
                Some(&&next) if next == line => {
                    lines.push(Line::Same(line));
                    kept.next();
                }
                _ => lines.push(Line::Added(line)),
            }
        }
        lines.extend(kept.map(|&line| Line::Removed(line)));
        if lines.iter().all(|line| matches!(line, Line::Same(_))) {
            return String::new();
        }
        let from = if self.before.is_some() {
            format!("a/{}", self.path)
        } else {
            "/dev/null".to_string()
        };
        let mut diff = format!("--- {from}\n+++ b/{}\n", self.path);
        for (start, end) in hunks(&lines) {
            diff += &hunk(&lines, start, end);
        }
        diff
    }
}

/// The ranges of lines to show: each change with its context, and changes
/// whose context meets shown as one.
fn hunks(lines: &[Line]) -> Vec<(usize, usize)> {
    let mut hunks: Vec<(usize, usize)> = Vec::new();
    for at in lines
        .iter()
        .enumerate()
        .filter(|(_, line)| !matches!(line, Line::Same(_)))
        .map(|(at, _)| at)
    {
        let (start, end) = (
            at.saturating_sub(CONTEXT),
            (at + CONTEXT + 1).min(lines.len()),
        );
        match hunks.last_mut() {
            Some(last) if start <= last.1 => last.1 = end,
            _ => hunks.push((start, end)),
        }
    }
    hunks
}

fn hunk(lines: &[Line], start: usize, end: usize) -> String {
    let count = |keep: fn(&Line) -> bool, range: std::ops::Range<usize>| {
        lines[range].iter().filter(|line| keep(line)).count()
    };
    let in_old = |line: &Line| !matches!(line, Line::Added(_));
    let in_new = |line: &Line| !matches!(line, Line::Removed(_));
    let (old_before, new_before) = (count(in_old, 0..start), count(in_new, 0..start));
    let (old_len, new_len) = (count(in_old, start..end), count(in_new, start..end));
    let first = |before: usize, len: usize| if len == 0 { before } else { before + 1 };
    let mut text = format!(
        "@@ -{},{old_len} +{},{new_len} @@\n",
        first(old_before, old_len),
        first(new_before, new_len)
    );
    for line in &lines[start..end] {
        let (mark, shown) = match line {
            Line::Same(shown) => (' ', shown),
            Line::Added(shown) => ('+', shown),
            Line::Removed(shown) => ('-', shown),
        };
        text.push(mark);
        text += shown;
        text.push('\n');
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(before: Option<&str>, after: &str) -> Change {
        Change {
            path: "journal/2026/03.ax".into(),
            before: before.map(String::from),
            after: after.into(),
        }
    }

    #[test]
    fn added_lines_are_shown_with_their_context() {
        let before = (1..=20).map(|n| format!("line {n}\n")).collect::<String>();
        let after = before
            .replace("line 4\n", "line 4\nnew a\n")
            .replace("line 20\n", "line 20\nnew b\n");
        let expected = "\
--- a/journal/2026/03.ax
+++ b/journal/2026/03.ax
@@ -2,6 +2,7 @@
 line 2
 line 3
 line 4
+new a
 line 5
 line 6
 line 7
@@ -18,3 +19,4 @@
 line 18
 line 19
 line 20
+new b
";
        assert_eq!(change(Some(&before), &after).diff(), expected);
    }

    #[test]
    fn nearby_changes_share_a_hunk_and_a_new_file_comes_from_nothing() {
        let before = "a\nb\nc\nd\ne\nf\ng\nh\n";
        let after = "a\nX\nb\nc\nd\ne\nf\ng\nY\nh\n";
        assert_eq!(
            change(Some(before), after).diff().matches("@@").count(),
            2,
            "one hunk, two markers"
        );
        assert_eq!(
            change(None, "01 flat\n").diff(),
            "--- /dev/null\n+++ b/journal/2026/03.ax\n@@ -0,0 +1,1 @@\n+01 flat\n"
        );
        assert_eq!(change(Some("a\n"), "a\n").diff(), "");
    }
}
