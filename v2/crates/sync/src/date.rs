//! ISO dates when a format has no explicit compiled `core::DateLayout`.

use axiom_core::Day;

pub fn iso_day(text: &str) -> Option<Day> {
    let text = text.trim();
    if let Some(day) = Day::parse(text.as_bytes()) {
        return Some(day);
    }
    if text.len() != 8 || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let year = text[..4].parse().ok()?;
    let month = text[4..6].parse().ok()?;
    let day = text[6..8].parse().ok()?;
    Day::from_ymd(year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_only_exact_iso_date_forms_without_a_compiled_layout() {
        let date = Day::from_ymd(2026, 1, 5);
        assert_eq!(iso_day("2026-01-05"), date);
        assert_eq!(iso_day("20260105"), date);
        assert_eq!(iso_day("2026-01-05T12:00"), None);
        assert_eq!(iso_day("soon"), None);
    }
}
