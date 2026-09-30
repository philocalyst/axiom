//! Dates as an export writes them, from a layout the ledger declares:
//! `YYYY-MM-DD`, `MM/DD/YYYY`, `DD.MM.YYYY`, `M/D/YY`.

use std::fmt;

use axiom_core::Day;

/// A date layout. Reading takes the day from the start of the text and lets a
/// time or a zone follow it, as banks add them (`2026-01-05 12:00`,
/// `20260105120000[-5:EST]`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DateFormat(Vec<Part>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Part {
    Year {
        digits: usize,
    },
    /// `MM` is exactly two digits, `M` one or two.
    Month {
        min: usize,
    },
    Day {
        min: usize,
    },
    Literal(char),
}

const WORDS: [(&str, Part); 6] = [
    ("YYYY", Part::Year { digits: 4 }),
    ("YY", Part::Year { digits: 2 }),
    ("MM", Part::Month { min: 2 }),
    ("M", Part::Month { min: 1 }),
    ("DD", Part::Day { min: 2 }),
    ("D", Part::Day { min: 1 }),
];

impl DateFormat {
    pub fn new(pattern: &str) -> Result<DateFormat, String> {
        let mut parts = Vec::new();
        let mut rest = pattern;
        while let Some(next) = rest.chars().next() {
            let word = WORDS.iter().find(|(word, _)| rest.starts_with(word));
            let (length, part) = match word {
                Some(&(word, part)) => (word.len(), part),
                None if next.is_ascii_alphanumeric() => {
                    return Err(format!("`{next}` means nothing in a date pattern: use YYYY, YY, MM, M, DD or D"));
                }
                None => (next.len_utf8(), Part::Literal(next)),
            };
            parts.push(part);
            rest = &rest[length..];
        }
        let named = |is: fn(&Part) -> bool| parts.iter().filter(|part| is(part)).count();
        let once = named(|p| matches!(p, Part::Year { .. })) == 1
            && named(|p| matches!(p, Part::Month { .. })) == 1
            && named(|p| matches!(p, Part::Day { .. })) == 1;
        if !once {
            return Err("a date pattern names the year, the month and the day once each".into());
        }
        Ok(DateFormat(parts))
    }

    /// The day `text` says, if it begins with this pattern and a real date. What
    /// follows may be a time, a zone, anything but a word.
    pub fn read(&self, text: &str) -> Option<Day> {
        let bytes = text.trim().as_bytes();
        let (mut at, mut year, mut month, mut day) = (0, 0, 0, 0);
        for part in &self.0 {
            match *part {
                Part::Year { digits } => {
                    year = number(bytes, &mut at, digits, digits)?;
                    if digits == 2 {
                        year += if year < 70 { 2000 } else { 1900 };
                    }
                }
                Part::Month { min } => month = number(bytes, &mut at, min, 2)?,
                Part::Day { min } => day = number(bytes, &mut at, min, 2)?,
                Part::Literal(c) => {
                    let mut buffer = [0; 4];
                    let wanted = c.encode_utf8(&mut buffer).as_bytes();
                    (bytes.get(at..at + wanted.len())? == wanted).then_some(())?;
                    at += wanted.len();
                }
            }
        }
        // A `T` is the ISO way to say a time follows; another letter is not a time.
        let over = bytes.get(at).is_none_or(|next| !next.is_ascii_alphabetic() || matches!(next, b'T' | b't'));
        over.then_some(())?;
        Day::from_ymd(year as i32, month, day)
    }

    /// The same pattern with the month and the day trading places: what a
    /// date that fails to read may have meant.
    pub fn swapped(&self) -> DateFormat {
        let swap = |part: &Part| match *part {
            Part::Month { min } => Part::Day { min },
            Part::Day { min } => Part::Month { min },
            other => other,
        };
        DateFormat(self.0.iter().map(swap).collect())
    }
}

/// The day a date says when nothing says how it is written: `2026-01-05`, or
/// `20260105` as OFX has it.
pub fn iso_day(text: &str) -> Option<Day> {
    let (dashed, compact) = (DateFormat::new("YYYY-MM-DD"), DateFormat::new("YYYYMMDD"));
    [dashed, compact].into_iter().flatten().find_map(|layout| layout.read(text))
}

impl fmt::Display for DateFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for part in &self.0 {
            match part {
                Part::Literal(c) => write!(f, "{c}")?,
                part => f.write_str(WORDS.iter().find(|(_, word)| word == part).map_or("", |(text, _)| text))?,
            }
        }
        Ok(())
    }
}

/// Between `min` and `max` digits at `at`, as many as there are.
fn number(bytes: &[u8], at: &mut usize, min: usize, max: usize) -> Option<u32> {
    let digits = bytes[*at..].iter().take(max).take_while(|b| b.is_ascii_digit()).count();
    (digits >= min).then_some(())?;
    let value = bytes[*at..*at + digits].iter().fold(0u32, |sum, b| sum * 10 + u32::from(b - b'0'));
    *at += digits;
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(text: &str) -> Day {
        Day::parse(text.as_bytes()).unwrap()
    }

    #[test]
    fn date_patterns() {
        let read = |pattern: &str, text: &str| DateFormat::new(pattern).unwrap().read(text);
        assert_eq!(read("YYYY-MM-DD", "2026-01-05"), Some(day("2026-01-05")));
        assert_eq!(read("MM/DD/YYYY", "01/05/2026"), Some(day("2026-01-05")));
        assert_eq!(read("DD.MM.YYYY", "05.01.2026"), Some(day("2026-01-05")));
        assert_eq!(read("M/D/YY", "1/5/26"), Some(day("2026-01-05")));
        assert_eq!(read("M/D/YY", "12/25/99"), Some(day("1999-12-25")));
        assert_eq!(read("MM/DD/YYYY", "1/5/2026"), None, "MM is two digits");
        assert_eq!(read("MM/DD/YYYY", "13/05/2026"), None);
        assert_eq!(read("YYYY-MM-DD", "2026-02-30"), None);
        assert!(DateFormat::new("yyyy-MM-DD").unwrap_err().contains("`y`"));
        assert!(DateFormat::new("YYYY-MM").unwrap_err().contains("once each"));
        assert_eq!(DateFormat::new("D.M.YYYY").unwrap().to_string(), "D.M.YYYY");
    }

    #[test]
    fn a_time_or_a_zone_may_follow_the_day() {
        let read = |pattern: &str, text: &str| DateFormat::new(pattern).unwrap().read(text);
        assert_eq!(read("YYYY-MM-DD", "2026-01-05 12:00"), Some(day("2026-01-05")));
        assert_eq!(read("YYYY-MM-DD", "2026-01-05T10:00:00Z"), Some(day("2026-01-05")));
        assert_eq!(read("YYYYMMDD", "20260105120000[-5:EST]"), Some(day("2026-01-05")));
        assert_eq!(read("YYYY-MM-DD", "2026-01-05x"), None, "a word is not a time");
        assert_eq!(read("YYYY-MM-DD", "2026-01"), None);
        assert_eq!(read("MM/DD/YYYY", "01/05/26"), None, "a short year is not a long one");
    }

    #[test]
    fn nothing_declared_reads_iso_dates() {
        assert_eq!(iso_day("2026-01-05"), Some(day("2026-01-05")));
        assert_eq!(iso_day("20260105"), Some(day("2026-01-05")));
        assert_eq!(iso_day("soon"), None);
    }
}
