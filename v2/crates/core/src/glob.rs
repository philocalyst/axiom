//! Matching names against patterns.
//!
//! One engine serves places, entities, and codes: `*` spans any run of
//! characters (across `/`), `?` matches exactly one.

/// Whether `text` matches `pattern`. Linear in the common case; backtracks only
/// to the most recent `*`.
pub fn glob(pattern: &str, text: &str) -> bool {
    let (p, t) = (pattern.as_bytes(), text.as_bytes());
    let (mut pi, mut ti) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while ti < t.len() {
        match p.get(pi) {
            Some(b'*') => {
                star = Some((pi, ti));
                pi += 1;
            }
            Some(&c) if c == b'?' || c == t[ti] => {
                pi += 1;
                ti += 1;
            }
            _ => match star {
                Some((sp, st)) => {
                    (pi, ti) = (sp + 1, st + 1);
                    star = Some((sp, st + 1));
                }
                None => return false,
            },
        }
    }
    p[pi..].iter().all(|&c| c == b'*')
}

/// Whether `path` is `root` or lies beneath it: `expenses/food` covers
/// `expenses/food/snacks` but not `expenses/foodstuffs`.
pub fn covers(root: &str, path: &str) -> bool {
    path.strip_prefix(root).is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

/// Whether a written name is a pattern rather than a literal.
pub fn is_pattern(name: &str) -> bool {
    name.bytes().any(|b| b == b'*' || b == b'?')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs() {
        assert!(glob("trip-*", "trip-japan"));
        assert!(glob("expenses/*/rent", "expenses/housing/rent"));
        assert!(glob("*", ""));
        assert!(glob("a*b*c", "a-b-b-c"));
        assert!(!glob("a*b*c", "a-b-b-d"));
        assert!(glob("check-????", "check-1041"));
        assert!(covers("expenses/food", "expenses/food/snacks"));
        assert!(covers("expenses/food", "expenses/food"));
        assert!(!covers("expenses/food", "expenses/foodstuffs"));
    }
}
