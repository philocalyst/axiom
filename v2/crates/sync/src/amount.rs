//! Amounts as banks write them, in a CSV cell or an OFX tag.

use axiom_core::num::{Dec, DecError};
use axiom_core::{Diagnostic, Loc, Qty};

use crate::Unit;

/// Why some text is not an amount.
pub enum Why {
    Malformed { comma_decimal: bool },
    Precision,
    Range,
}

impl Why {
    /// The problem, in `place` (`row 3`): `text` is at `at`, and `label` says what it is.
    pub fn diagnostic(
        self,
        place: &str,
        at: Loc,
        label: String,
        text: &str,
        unit: Unit,
    ) -> Diagnostic {
        let error = |headline: String| {
            Diagnostic::error("bad-amount", format!("{place}: {headline}")).label(at, label.clone())
        };
        match self {
            Why::Malformed { comma_decimal } => {
                let error = error(format!("`{text}` is not an amount"));
                match comma_decimal {
                    true => error
                        .note("a comma reads as a thousands separator, so `12,50` is not twelve and a half")
                        .help("ask the bank for an export that writes the decimal mark as a point"),
                    false => error,
                }
            }
            Why::Precision => error(format!(
                "`{text}` has more decimal places than {} keeps",
                unit.name
            ))
            .note(format!(
                "{} is counted to {} decimal places",
                unit.name, unit.scale
            )),
            Why::Range => error(format!("`{text}` is too large to be an amount")),
        }
    }
}

/// `-1,234.56`, `(12.00)`, `$12`, `-$12`: a leading currency sign, thousands
/// separators, and parentheses or a minus for negatives. `None` for an empty
/// cell.
pub fn amount(cell: &str, scale: u8) -> Result<Option<Qty>, Why> {
    if cell.trim().is_empty() {
        return Ok(None);
    }
    let (negative, number) = sign(cell)?;
    let digits = Digits::of(number)?;
    let dec = Dec::parse(digits.as_bytes()).ok_or(Why::Range)?;
    let qty = dec.to_qty(scale).map_err(|why| match why {
        DecError::Inexact => Why::Precision,
        DecError::Range => Why::Range,
    })?;
    Ok(Some(if negative { -qty } else { qty }))
}

/// Whether the cell says negative, and the number without its sign: a minus or
/// parentheses make it so, and a currency sign is only decoration.
fn sign(cell: &str) -> Result<(bool, &str), Why> {
    let cell = cell.trim();
    let (parenthesized, mut rest) = match cell
        .strip_prefix('(')
        .and_then(|inner| inner.strip_suffix(')'))
    {
        Some(inner) => (true, inner),
        None => (false, cell),
    };
    if parenthesized {
        rest = rest.trim();
        if let Some(ch @ ('$' | '€' | '£' | '¥')) = rest.chars().next() {
            rest = rest[ch.len_utf8()..].trim_start();
        }
        if rest.starts_with(['+', '-', '$', '€', '£', '¥']) {
            return Err(Why::Malformed {
                comma_decimal: false,
            });
        }
        return Ok((true, rest));
    }

    rest = rest.trim_start();
    let mut sign = None;
    if let Some(ch @ ('+' | '-')) = rest.chars().next() {
        sign = Some(ch);
        rest = rest[ch.len_utf8()..].trim_start();
    }
    if let Some(ch @ ('$' | '€' | '£' | '¥')) = rest.chars().next() {
        rest = rest[ch.len_utf8()..].trim_start();
        if sign.is_none() {
            if let Some(next @ ('+' | '-')) = rest.chars().next() {
                sign = Some(next);
                rest = rest[next.len_utf8()..].trim_start();
            }
        }
    }
    if rest.starts_with(['+', '-', '$', '€', '£', '¥']) {
        return Err(Why::Malformed {
            comma_decimal: false,
        });
    }
    Ok((sign == Some('-'), rest))
}

/// A number's digits and its point, with the thousands separators taken out.
struct Digits {
    bytes: [u8; 40],
    len: usize,
}

impl Digits {
    /// `1,234.5` as `1234.5`; a comma that is not between thousands is refused.
    fn of(number: &str) -> Result<Digits, Why> {
        let (whole, fraction) = number.split_once('.').unwrap_or((number, ""));
        let grouped = whole.contains(',');
        let group_ok = |(at, group): (usize, &str)| {
            let size = match (grouped, at) {
                (false, _) => true,
                (true, 0) => (1..=3).contains(&group.len()),
                (true, _) => group.len() == 3,
            };
            size && group.bytes().all(|b| b.is_ascii_digit())
        };
        let sound = whole.split(',').enumerate().all(group_ok)
            && fraction.bytes().all(|b| b.is_ascii_digit())
            && !(whole.is_empty() && fraction.is_empty());
        if !sound {
            let tail = whole.rsplit_once(',').map_or(0, |(_, tail)| tail.len());
            return Err(Why::Malformed {
                comma_decimal: fraction.is_empty() && matches!(tail, 1 | 2),
            });
        }
        let mut digits = Digits {
            bytes: [0; 40],
            len: 0,
        };
        if whole.is_empty() {
            digits.push(b'0')?;
        }
        for byte in whole.bytes().filter(|&byte| byte != b',') {
            digits.push(byte)?;
        }
        if !fraction.is_empty() {
            digits.push(b'.')?;
            for byte in fraction.bytes() {
                digits.push(byte)?;
            }
        }
        Ok(digits)
    }

    fn push(&mut self, byte: u8) -> Result<(), Why> {
        *self.bytes.get_mut(self.len).ok_or(Why::Range)? = byte;
        self.len += 1;
        Ok(())
    }

    fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amounts_as_banks_write_them() {
        let qty = |text: &str| amount(text, 2).ok().flatten().map(|q| q.0);
        assert_eq!(qty("1,234.56"), Some(123_456));
        assert_eq!(qty("(12.00)"), Some(-1200));
        assert_eq!(qty("($12.00)"), Some(-1200));
        assert_eq!(qty("(£ 12.00)"), Some(-1200));
        assert_eq!(qty("(€12.00)"), Some(-1200));
        assert_eq!(qty("$12"), Some(1200));
        assert_eq!(qty("-$12.50"), Some(-1250));
        assert_eq!(qty("$-12.50"), Some(-1250));
        assert_eq!(qty("+ 7.5"), Some(750));
        assert_eq!(qty(".5"), Some(50));
        assert_eq!(qty("1,234,567"), Some(123_456_700));
        assert_eq!(amount("", 2).ok(), Some(None));
        for bad in ["12,5", "1,23.00", "12.3.4", "abc", "$", "1 000", "1.234,56"] {
            assert!(
                matches!(amount(bad, 2), Err(Why::Malformed { .. })),
                "{bad}"
            );
        }
        for bad in ["+-12", "-+12", "--12", "++12", "-$-12", "(-12)", "($-12)"] {
            assert!(matches!(amount(bad, 2), Err(Why::Malformed { .. })), "{bad}");
        }
        assert!(matches!(amount("0.005", 2), Err(Why::Precision)));
        assert!(matches!(
            amount("9".repeat(30).as_str(), 2),
            Err(Why::Range)
        ));
        assert!(matches!(
            amount("12,50", 2),
            Err(Why::Malformed {
                comma_decimal: true
            })
        ));
    }
}
