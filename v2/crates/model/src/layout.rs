//! Folder layout as a constraint (LANGUAGE §10).
//!
//! Where a file lives says what it may hold. A `YYYY` folder or `YYYY.ax` file
//! holds one year, a `MM` folder or file after it (or `YYYY-MM.ax`) holds one
//! month, `prices/` holds only prices, and `systems/` holds only systems.
//! `layout free` turns all of it off.

use axiom_core::{Day, Diagnostic, Loc};
use axiom_syntax::{Item, ItemKind};

use crate::errors::{count, iso};
use crate::scope::Home;
use crate::sources::Site;

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

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Only {
    Anything,
    Prices,
    Systems,
}

pub(crate) struct Layout<'a> {
    /// The path's segments, the file's name without its `.ax`.
    segments: Vec<&'a str>,
    /// Which segment names the year and which the month (the same one for
    /// `2026-03`), and what they say.
    year: Option<(usize, i32)>,
    month: Option<(usize, u32)>,
    pub only: Only,
}

/// An item dated outside the file that holds it.
#[derive(Clone, Copy)]
pub(crate) struct Misfiled {
    pub loc: Loc,
    pub day: Day,
    pub noun: &'static str,
}

fn digits(text: &str, count: usize) -> Option<u32> {
    (text.len() == count && text.bytes().all(|byte| byte.is_ascii_digit())).then(|| text.parse().ok()).flatten()
}

impl<'a> Layout<'a> {
    pub fn of(path: &'a str) -> Layout<'a> {
        let segments: Vec<&str> = path.strip_suffix(".ax").unwrap_or(path).split('/').collect();
        let (mut year, mut month) = (None, None);
        for (at, &segment) in segments.iter().enumerate() {
            match (segment.split_once('-'), year, month) {
                (Some((y, m)), None, None) if digits(y, 4).is_some() && digits(m, 2).is_some() => {
                    (year, month) = (digits(y, 4).map(|y| (at, y as i32)), digits(m, 2).map(|m| (at, m)));
                }
                (_, None, _) if digits(segment, 4).is_some() => year = digits(segment, 4).map(|y| (at, y as i32)),
                (_, Some((year_at, _)), None) if at == year_at + 1 => {
                    month = digits(segment, 2).filter(|m| (1..=12).contains(m)).map(|m| (at, m));
                }
                _ => {}
            }
        }
        let folders = &segments[..segments.len() - 1];
        let only = match folders.iter().rev().find(|&&segment| segment == "prices" || segment == "systems") {
            Some(&"prices") => Only::Prices,
            Some(_) => Only::Systems,
            None => Only::Anything,
        };
        Layout { segments, year, month, only }
    }

    /// Whether an item dated `day` belongs in this file.
    pub fn holds(&self, day: Day) -> bool {
        let (year, month, _) = day.ymd();
        self.year.is_none_or(|(_, held)| held == year) && self.month.is_none_or(|(_, held)| held == month)
    }

    /// `March 2026`, `2026`.
    fn describe(&self) -> String {
        match (self.year, self.month) {
            (Some((_, year)), Some((_, month))) => format!("{} {year}", MONTHS[month as usize - 1]),
            (Some((_, year)), None) => year.to_string(),
            _ => "anything".to_string(),
        }
    }

    /// Where an item on `day` belongs: this path with its year and month
    /// swapped for the day's.
    fn path_for(&self, day: Day) -> String {
        let (year, month, _) = day.ymd();
        let segment = |(at, text): (usize, &str)| match (self.year, self.month) {
            (Some((y, _)), Some((m, _))) if y == at && m == at => format!("{year:04}-{month:02}"),
            (Some((y, _)), _) if y == at => format!("{year:04}"),
            (_, Some((m, _))) if m == at => format!("{month:02}"),
            _ => text.to_string(),
        };
        format!("{}.ax", self.segments.iter().copied().enumerate().map(segment).collect::<Vec<_>>().join("/"))
    }

    /// One report for a file's misdated items: the root cause is where the
    /// file is, not any one line of it.
    pub fn misfiled(&self, path: &str, items: &[Misfiled]) -> Diagnostic {
        let first = items[0];
        let date = |item: &Misfiled| Loc::new(item.loc.file, item.loc.start, item.loc.start + 10);
        let held = self.describe();
        let headline = match items.len() {
            1 => format!("this {} is dated {}, which `{path}` does not hold", first.noun, iso(first.day)),
            more => format!("`{path}` holds {held}, but {} are dated elsewhere", count(more, "item")),
        };
        let mut diagnostic = Diagnostic::error("layout", headline).label(date(&first), format!("outside {held}"));
        for item in items[1..].iter().take(3) {
            diagnostic = diagnostic.context(date(item), format!("{} dated {}", item.noun, iso(item.day)));
        }
        if items.len() > 4 {
            diagnostic = diagnostic.note(format!("{} more are dated elsewhere too", items.len() - 4));
        }
        diagnostic
            .note(format!("`{path}` holds {held}, because of where it is"))
            .help(format!("move it to `{}`, or set `layout free` to ignore folder names", self.path_for(first.day)))
    }
}

/// The date an item is filed under, and what to call the item.
pub(crate) fn dated(item: &Item, file: &axiom_syntax::File) -> Option<(Day, &'static str)> {
    Some(match item.kind {
        ItemKind::Txn(id) => (file[id].date, "transaction"),
        ItemKind::Assert(id) => (file[id].date, "balance assertion"),
        ItemKind::Event(id) => (file[id].date, "event"),
        ItemKind::Price(id) => (file[id].date, "price"),
        ItemKind::Split(id) => (file[id].date, "split"),
        ItemKind::Occurrence(id) => (file[id].date, "plan occurrence"),
        ItemKind::Opening(id) => (file[id].date, "opening"),
        _ => return None,
    })
}

/// The rules about what a file may hold, apart from dates.
pub(crate) fn check(sites: &[Site], diags: &mut Vec<Diagnostic>) {
    for site in sites.iter().filter(|site| !site.source.embedded) {
        let items = &site.source.file.items;
        match site.layout.only {
            Only::Prices => {
                diags.extend(items.iter().filter(|item| !matches!(item.kind, ItemKind::Price(_))).map(only_prices))
            }
            Only::Systems if site.home == Home::Project => diags.extend(items.first().map(not_a_system)),
            Only::Systems | Only::Anything => {}
        }
    }
}

fn only_prices(item: &Item) -> Diagnostic {
    Diagnostic::error("layout", "files under `prices/` may contain only prices")
        .label(item.loc, "this is not a price")
        .help("move it out of `prices/`, or set `layout free`")
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
