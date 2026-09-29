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
    /// The line that would recognize the group: `known-as "TRADER JOE'S*"`.
    pub fn known_as(&self) -> String {
        format!("known-as \"{}*\"", self.stem.replace(['\\', '"'], ""))
    }
}

/// The memos grouped by their beginning, the biggest group first.
pub fn group<'m>(memos: impl IntoIterator<Item = &'m str>) -> Vec<Group<'m>> {
    let mut groups: Map<String, Group<'m>> = Map::default();
    for memo in memos {
        let stem = stem(memo);
        groups.entry(stem.to_uppercase()).or_insert(Group { stem, count: 0, example: memo }).count += 1;
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
    let start_of = |word: &str| word.as_ptr() as usize - memo.as_ptr() as usize;
    let Some(number) = memo.split_whitespace().find(|word| word.contains(numbered)) else { return memo };
    let end = match start_of(number) {
        0 => number.find(numbered).unwrap_or(number.len()),
        start => start,
    };
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
        let shown: Vec<(usize, String)> = groups.iter().map(|group| (group.count, group.known_as())).collect();
        assert_eq!(
            shown,
            [
                (3, "known-as \"TRADER JOE'S*\"".to_string()),
                (2, "known-as \"AMAZON.COM*\"".to_string()),
                (1, "known-as \"PAYMENT THANK YOU*\"".to_string()),
                (1, "known-as \"SQ *BLUE BOTTLE*\"".to_string()),
            ]
        );
        assert_eq!(groups[0].example, "TRADER JOE'S #634 SAN FRANCISCO CA");
    }

    #[test]
    fn odd_memos_still_make_a_line() {
        for memo in ["", "   ", "7-ELEVEN #123", "#", "\"quoted\" 5", "12345"] {
            let groups = group([memo]);
            assert_eq!(groups.len(), 1, "{memo:?}");
            assert!(!groups[0].known_as().contains("\"\""), "{memo:?}: {}", groups[0].known_as());
        }
    }
}
