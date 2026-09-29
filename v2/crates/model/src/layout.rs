//! Folder layout as a constraint (LANGUAGE §8).
//!
//! Where a file lives says what it may hold. A `YYYY` folder or `YYYY.ax` file
//! holds one year, a `MM` folder or file after it (or `YYYY-MM.ax`) holds one
//! month, `prices/` holds only prices, and `systems/` holds only systems.
//! `layout free` turns all of it off.

use axiom_core::{Day, Diagnostic, Loc};
use axiom_syntax::{Item, ItemKind};

use crate::catalog::{Catalog, Site};
use crate::errors::iso;
use crate::scope::Home;

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// A path segment, read for what it means.
#[derive(Clone, Copy)]
enum Piece<'a> {
    Plain(&'a str),
    Year,
    Month,
    YearMonth,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Only {
    Anything,
    Prices,
    Systems,
}

pub(crate) struct Layout<'a> {
    pieces: Vec<Piece<'a>>,
    year: Option<i32>,
    month: Option<u32>,
    pub only: Only,
}

fn digits(text: &str, count: usize) -> Option<u32> {
    (text.len() == count && text.bytes().all(|byte| byte.is_ascii_digit())).then(|| text.parse().ok()).flatten()
}

impl<'a> Layout<'a> {
    pub fn of(path: &'a str) -> Layout<'a> {
        let mut layout = Layout { pieces: Vec::new(), year: None, month: None, only: Only::Anything };
        let mut segments: Vec<&str> = path.split('/').collect();
        let file = segments.pop().unwrap_or_default();
        let stem = file.strip_suffix(".ax").unwrap_or(file);
        let mut after_year = false;
        for segment in segments {
            layout.only = match segment {
                "prices" => Only::Prices,
                "systems" => Only::Systems,
                _ => layout.only,
            };
            after_year = layout.piece(segment, after_year);
        }
        match stem.split_once('-') {
            Some((year, month)) if layout.year.is_none() && digits(year, 4).is_some() && digits(month, 2).is_some() => {
                layout.year = digits(year, 4).map(|year| year as i32);
                layout.month = digits(month, 2);
                layout.pieces.push(Piece::YearMonth);
            }
            _ => {
                layout.piece(stem, after_year);
            }
        }
        layout
    }

    /// Reads one segment; whether it was the year that a month may follow.
    fn piece(&mut self, segment: &'a str, after_year: bool) -> bool {
        if let (Some(year), None) = (digits(segment, 4), self.year) {
            self.year = Some(year as i32);
            self.pieces.push(Piece::Year);
            return true;
        }
        match digits(segment, 2) {
            Some(month) if after_year && self.month.is_none() && (1..=12).contains(&month) => {
                self.month = Some(month);
                self.pieces.push(Piece::Month);
            }
            _ => self.pieces.push(Piece::Plain(segment)),
        }
        false
    }

    fn holds(&self, day: Day) -> bool {
        let (year, month, _) = day.ymd();
        self.year.is_none_or(|held| held == year) && self.month.is_none_or(|held| held == month)
    }

    /// `March 2026`, `2026`.
    fn describe(&self) -> String {
        match (self.year, self.month) {
            (Some(year), Some(month)) => format!("{} {year}", MONTHS[month as usize - 1]),
            (Some(year), None) => year.to_string(),
            _ => "anything".to_string(),
        }
    }

    /// Where an item on `day` belongs: this path with its year and month
    /// swapped for the day's.
    fn path_for(&self, day: Day) -> String {
        let (year, month, _) = day.ymd();
        let pieces: Vec<String> = self
            .pieces
            .iter()
            .map(|piece| match piece {
                Piece::Plain(text) => text.to_string(),
                Piece::Year => format!("{year:04}"),
                Piece::Month => format!("{month:02}"),
                Piece::YearMonth => format!("{year:04}-{month:02}"),
            })
            .collect();
        format!("{}.ax", pieces.join("/"))
    }
}

/// The date an item is filed under, and what to call the item.
fn dated(item: &Item) -> Option<(Day, &'static str)> {
    match &item.kind {
        ItemKind::Txn(txn) => Some((txn.date, "transaction")),
        ItemKind::Assert(assert) => Some((assert.date, "balance assertion")),
        ItemKind::Event(event) => Some((event.date, "event")),
        ItemKind::Price(price) => Some((price.date, "price")),
        _ => None,
    }
}

impl Layout<'_> {
    /// Whether an item dated `day` is filed where its date belongs. Journal
    /// transactions are checked as they are elaborated.
    pub fn check_date(&self, path: &str, item: &Item) -> Option<Diagnostic> {
        let (day, noun) = dated(item)?;
        if self.holds(day) {
            return None;
        }
        let date = Loc::new(item.loc.file, item.loc.start, item.loc.start + 10);
        Some(
            Diagnostic::error("layout", format!("this {noun} is dated {}, which `{path}` does not hold", iso(day)))
                .label(date, format!("outside {}", self.describe()))
                .note(format!("`{path}` holds {}, because of where it is", self.describe()))
                .help(format!("move it to `{}`, or set `layout free` to ignore folder names", self.path_for(day))),
        )
    }
}

/// The rules about what a file may hold, and the dates of everything but
/// journal transactions.
pub(crate) fn check(catalog: &Catalog, sites: &[Site], diags: &mut Vec<Diagnostic>) {
    if catalog.layout_free {
        return;
    }
    let dated_items = catalog
        .asserts
        .iter()
        .map(|written| (written.site, written.item))
        .chain(catalog.events.iter().map(|written| (written.site, written.item)))
        .chain(catalog.prices.iter().map(|written| (written.site, written.item)));
    for (site, item) in dated_items {
        diags.extend(site.layout.check_date(site.source.path, item));
    }
    for site in sites.iter().filter(|site| !site.source.embedded) {
        match site.layout.only {
            Only::Prices => diags.extend(site.source.file.items.iter().filter_map(only_prices)),
            Only::Systems if site.home == Home::Project => {
                diags.extend(site.source.file.items.first().map(not_a_system))
            }
            Only::Systems | Only::Anything => {}
        }
    }
}

fn only_prices(item: &Item) -> Option<Diagnostic> {
    if matches!(item.kind, ItemKind::Price(_)) {
        return None;
    }
    Some(
        Diagnostic::error("layout", "files under `prices/` may contain only prices")
            .label(item.loc, "this is not a price")
            .help("move it out of `prices/`, or set `layout free`"),
    )
}

fn not_a_system(first: &Item) -> Diagnostic {
    Diagnostic::error("layout", "files under `systems/` must be systems")
        .label(first.loc, "this file does not begin with `system`")
        .help("start it with `system PATH`, or move it out of `systems/`, or set `layout free`")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_and_files_constrain_dates() {
        let march = Layout::of("journal/2026/03.ax");
        let (in_march, in_april) = (Day::from_ymd(2026, 3, 9).unwrap(), Day::from_ymd(2026, 4, 2).unwrap());
        assert!(march.holds(in_march) && !march.holds(in_april));
        assert_eq!(march.describe(), "March 2026");
        assert_eq!(march.path_for(in_april), "journal/2026/04.ax");

        let dashed = Layout::of("journal/2026-03.ax");
        assert!(dashed.holds(in_march) && !dashed.holds(in_april));
        assert_eq!(dashed.path_for(in_april), "journal/2026-04.ax");

        let year = Layout::of("2026.ax");
        assert!(year.holds(in_april) && !year.holds(Day::from_ymd(2027, 1, 1).unwrap()));

        let plain = Layout::of("accounts.ax");
        assert!(plain.holds(in_april) && plain.only == Only::Anything);
        assert!(Layout::of("prices/2026.ax").only == Only::Prices);
    }
}
