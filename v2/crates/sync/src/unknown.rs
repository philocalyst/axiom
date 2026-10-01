//! The memos nothing recognized, grouped, each group with the `known-as` line
//! that would recognize it: what `check` offers.

use axiom_core::Map;

/// Memos that begin alike, which one `known-as` would cover.
pub struct Group<'m> {
    /// What they share, as the first of them wrote it.
    pub stem: &'m str,
    pub count: usize,
    /// One of the memos, whole.
    pub example: &'m str,
}

impl Group<'_> {
    /// The pattern that would recognize the group: `"TRADER JOE'S"`.
    pub fn pattern(&self) -> String {
        format!(
            "\"{}\"",
            self.stem.replace('\\', "\\\\").replace('"', "\\\"")
        )
    }

    /// The line that would recognize the group: `known-as "TRADER JOE'S"`.
    pub fn known_as(&self) -> String {
        format!("known-as {}", self.pattern())
    }
}

/// The memos grouped by their beginning, the biggest group first.
pub fn group<'m>(memos: impl IntoIterator<Item = &'m str>) -> Vec<Group<'m>> {
    let mut groups: Map<String, Group<'m>> = Map::default();
    for memo in memos {
        let stem = stem(memo);
        if stem.is_empty() {
            continue;
        }
        groups
            .entry(stem.to_uppercase())
            .or_insert(Group {
                stem,
                count: 0,
                example: memo,
            })
            .count += 1;
    }
    let mut groups: Vec<Group<'m>> = groups.into_values().collect();
    groups.sort_by(|a, b| b.count.cmp(&a.count).then(a.stem.cmp(b.stem)));
    groups
}

/// The name at the start of a memo: its words up to the first that holds a
/// digit or `#`, since store numbers, dates and cities come after the name.
fn stem(memo: &str) -> &str {
    let memo = memo.trim();
    let numbered = |c: char| c.is_ascii_digit() || c == '#';
    let mut end = 0;
    for word in memo.split_inclusive(char::is_whitespace) {
        if let Some(number) = word.find(numbered) {
            // A number in the very first word is cut at, since there is nothing before it.
            end += if end == 0 { number } else { 0 };
            break;
        }
        end += word.len();
    }
    match memo[..end].trim_end_matches(|c: char| !c.is_alphanumeric()) {
        "" => memo,
        stem => stem,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memos_that_begin_alike_are_one_group_with_the_line_that_would_recognize_them() {
        let memos = [
            "TRADER JOE'S #634 SAN FRANCISCO CA",
            "SQ *BLUE BOTTLE 1234 OAKLAND",
            "Trader Joe's #12 DALY CITY",
            "AMAZON.COM*2K4LM AMZN.COM/BILL",
            "TRADER JOE'S #9",
            "PAYMENT THANK YOU",
            "AMAZON.COM*7H1XP AMZN.COM/BILL",
        ];
        let groups = group(memos);
        let shown: Vec<(usize, String)> = groups
            .iter()
            .map(|group| (group.count, group.known_as()))
            .collect();
        assert_eq!(
            shown,
            [
                (3, "known-as \"TRADER JOE'S\"".to_string()),
                (2, "known-as \"AMAZON.COM\"".to_string()),
                (1, "known-as \"PAYMENT THANK YOU\"".to_string()),
                (1, "known-as \"SQ *BLUE BOTTLE\"".to_string()),
            ]
        );
        assert_eq!(groups[0].example, "TRADER JOE'S #634 SAN FRANCISCO CA");
    }

    #[test]
    fn the_line_offered_recognizes_what_it_was_offered_for() {
        use crate::recognize::{Known, Recognizer, Scratch};
        let memos = [
            "TRADER JOE'S #634 SAN FRANCISCO CA",
            "Trader Joe's #12",
            "SAY \"HI\" 5",
            "BACK\\SLASH 7",
        ];
        for group in group(memos) {
            let pattern = group.pattern();
            let known = vec![Known {
                name: "someone",
                account: false,
                patterns: vec![&pattern],
            }];
            let recognizer = Recognizer::new(known, &[], &crate::Patterns::default())
                .unwrap_or_else(|bad| panic!("{pattern}: {}", bad[0].error.message));
            let reading = recognizer.read(group.example, &mut Scratch::default());
            assert!(
                reading.who.ok().and_then(|found| found.who).is_some(),
                "{pattern} does not recognize {}",
                group.example
            );
        }
    }

    #[test]
    fn odd_memos_still_make_a_line_that_compiles_and_a_blank_one_makes_none() {
        assert!(group(["", "   "]).is_empty());
        for memo in ["7-ELEVEN #123", "#", "\"quoted\" 5", "12345"] {
            let groups = group([memo]);
            assert_eq!(groups.len(), 1, "{memo:?}");
            assert!(
                crate::Pattern::new(&groups[0].pattern(), &crate::Patterns::default()).is_ok(),
                "{memo:?}: {}",
                groups[0].known_as()
            );
        }
    }
}
