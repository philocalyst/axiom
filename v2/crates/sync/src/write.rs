//! Where a line goes, and how it is written (LANGUAGE §10): into the file its
//! day belongs to, in day order, dated as briefly as that spot allows, and
//! without touching anything already there.

use std::collections::BTreeMap;
use std::path::Path;

use axiom_core::{Day, Diagnostic, FileId};
use axiom_syntax::{Folder, format};

use crate::{Form, Insert};

/// What a short date can lean on: the year, and the month, where they are known.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Context {
    year: Option<i32>,
    month: Option<u32>,
}

impl Context {
    /// What the path says: a folder or file named `YYYY`, and beneath it `MM`
    /// (a folder or `MM.ax`), or a file `YYYY-MM.ax`.
    pub fn of_path(path: &str) -> Context {
        stamp(path).map_or(Context::default(), |stamp| Context {
            year: Some(stamp.year),
            month: stamp.month,
        })
    }

    /// The context a heading line gives: a lone year (`2026`) or month (`2026-02`).
    fn heading(line: &str) -> Option<Context> {
        match named(line.split("//").next()?.trim())? {
            Named::Year(year) => Some(Context {
                year: Some(year),
                month: None,
            }),
            Named::YearMonth(year, month) => Some(Context {
                year: Some(year),
                month: Some(month),
            }),
            Named::Month(_) => None,
        }
    }

    /// The day a written date means here, if it is one: `2026-01-15`, `01-15`
    /// where the year is known, `15` where the month is too.
    fn complete(self, token: &str) -> Option<Day> {
        let number = |text: &str| {
            text.parse::<u32>()
                .ok()
                .filter(|_| text.bytes().all(|byte| byte.is_ascii_digit()))
        };
        match token.len() {
            10 => Day::parse(token.as_bytes()),
            5 => {
                let (month, day) = token.split_once('-')?;
                Day::from_ymd(self.year?, number(month)?, number(day)?)
            }
            1 | 2 => Day::from_ymd(self.year?, self.month?, number(token)?),
            _ => None,
        }
    }

    /// `day` as briefly as this context allows.
    pub fn shorten(self, day: Day) -> String {
        let (year, month, of_month) = day.ymd();
        match (self.year == Some(year), self.month == Some(month)) {
            (true, true) => format!("{of_month:02}"),
            (true, false) => format!("{month:02}-{of_month:02}"),
            _ => day.to_string(),
        }
    }
}

/// A part of a path that names a period.
enum Named {
    Year(i32),
    Month(u32),
    YearMonth(i32, u32),
}

fn named(part: &str) -> Option<Named> {
    let digits = |text: &str, length: usize| {
        text.len() == length && text.bytes().all(|byte| byte.is_ascii_digit())
    };
    match part.split_once('-') {
        Some((year, month)) if digits(year, 4) && digits(month, 2) => {
            Some(Named::YearMonth(year.parse().ok()?, month.parse().ok()?))
        }
        None if digits(part, 4) => Some(Named::Year(part.parse().ok()?)),
        None if digits(part, 2) => part
            .parse()
            .ok()
            .filter(|month| (1..=12).contains(month))
            .map(Named::Month),
        _ => None,
    }
}

/// The period a path names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Stamp {
    year: i32,
    month: Option<u32>,
    /// The file itself is named for the year (`2026.ax`), not a folder above it.
    own: bool,
}

fn stamp(path: &str) -> Option<Stamp> {
    let parts: Vec<&str> = path
        .strip_suffix(".ax")
        .unwrap_or(path)
        .split('/')
        .collect();
    let mut found = None;
    for (at, part) in parts.iter().enumerate() {
        found = match (named(part), found) {
            (Some(Named::Year(year)), _) => Some(Stamp {
                year,
                month: None,
                own: at + 1 == parts.len(),
            }),
            (Some(Named::YearMonth(year, month)), _) => Some(Stamp {
                year,
                month: Some(month),
                own: true,
            }),
            (
                Some(Named::Month(month)),
                Some(Stamp {
                    month: None,
                    year,
                    own,
                }),
            ) => Some(Stamp {
                year,
                month: Some(month),
                own,
            }),
            (_, found) => found,
        };
    }
    found
}

/// `path` with the parts that name a period naming `day`'s.
fn restamp(path: &str, day: Day) -> String {
    let (year, month, _) = day.ymd();
    let (stem, extension) = path
        .strip_suffix(".ax")
        .map_or((path, ""), |stem| (stem, ".ax"));
    let parts = stem.split('/').map(|part| match named(part) {
        Some(Named::Year(_)) => format!("{year:04}"),
        Some(Named::Month(_)) => format!("{month:02}"),
        Some(Named::YearMonth(..)) => format!("{year:04}-{month:02}"),
        None => part.to_string(),
    });
    parts.collect::<Vec<_>>().join("/") + extension
}

/// Which file a day belongs to, by the project's own habit.
pub struct Layout {
    /// The project's files that name a month, or are named for a year.
    dated: Vec<(Stamp, String)>,
}

impl Layout {
    pub fn new<'a>(paths: impl IntoIterator<Item = &'a str>) -> Layout {
        let stamped = paths
            .into_iter()
            .filter_map(|path| Some((stamp(path)?, path.to_string())));
        let mut dated: Vec<_> = stamped
            .filter(|(stamp, _)| stamp.month.is_some() || stamp.own)
            .collect();
        dated.sort();
        Layout { dated }
    }

    /// The month's file if there is one, else the year's; else the latest
    /// file's habit carried to this day; else `journal/YYYY/MM.ax`.
    pub fn file_for(&self, day: Day) -> String {
        let (year, month, _) = day.ymd();
        let of = |month: Option<u32>| {
            self.dated
                .iter()
                .find(|(stamp, _)| stamp.year == year && stamp.month == month)
        };
        match (of(Some(month)).or(of(None)), self.dated.last()) {
            (Some((_, path)), _) => path.to_string(),
            (None, Some((_, latest))) => restamp(latest, day),
            (None, None) => format!("journal/{year:04}/{month:02}.ax"),
        }
    }
}

/// One item of a file: a line at the margin, the indented lines under it, and
/// the comments just above.
pub struct Item {
    pub day: Option<Day>,
    /// The line with the date on it.
    pub head: usize,
    /// The first line: the comments above, or the head.
    pub start: usize,
    /// One past the last line.
    pub end: usize,
    /// What a short date meant at the head.
    pub ctx: Context,
}

/// The items of a file's lines, and the context it ends in.
pub fn scan(lines: &[&str], mut ctx: Context) -> (Vec<Item>, Context) {
    let indented = |line: &str| line.starts_with([' ', '\t']);
    let (mut items, mut comments, mut at) = (Vec::new(), None, 0);
    while at < lines.len() {
        let line = lines[at].trim_end();
        at += 1;
        if line.is_empty() {
            comments = None;
        } else if line.starts_with("//") {
            comments.get_or_insert(at - 1);
        } else if let Some(heading) = Context::heading(line) {
            (ctx, comments) = (heading, None);
        } else if !indented(line) {
            let head = at - 1;
            let mut end = at;
            while let Some(next) = lines
                .get(at)
                .filter(|next| next.trim().is_empty() || indented(next))
            {
                at += 1;
                if !next.trim().is_empty() {
                    end = at;
                }
            }
            items.push(Item {
                day: date_of(line, ctx),
                head,
                start: comments.take().unwrap_or(head),
                end,
                ctx,
            });
            at = end;
        }
    }
    (items, ctx)
}

/// The day an item starts with (after `opening`, if that is what it is).
fn date_of(line: &str, ctx: Context) -> Option<Day> {
    let mut words = line.split_whitespace();
    let first = words.next()?;
    let token = if first == "opening" {
        words.next()?
    } else {
        first
    };
    ctx.complete(token.split("..").next()?)
}

/// The items that have a day, in day order and, within a day, file order: what
/// [`place`] looks a day up in.
fn by_day(items: &[Item]) -> Vec<&Item> {
    let mut dated: Vec<&Item> = items.iter().filter(|item| item.day.is_some()).collect();
    dated.sort_by_key(|item| item.day);
    dated
}

/// The line to insert `day` before, and the context there: after the last item
/// that is not later, or before the first that is, whichever lets the date be
/// shorter. A file with nothing to go by takes `fallback`.
fn place(dated: &[&Item], day: Day, fallback: (usize, Context)) -> (usize, Context) {
    let cut = dated.partition_point(|item| item.day <= Some(day));
    let after = cut.checked_sub(1).map(|last| dated[last]);
    let before = dated.get(cut).copied();
    match (after, before) {
        (Some(after), Some(before))
            if before.ctx.shorten(day).len() < after.ctx.shorten(day).len() =>
        {
            (before.start, before.ctx)
        }
        (Some(after), _) => (after.end, after.ctx),
        (None, Some(before)) => (before.start, before.ctx),
        (None, None) => fallback,
    }
}

/// New lines, each group to go in before the line it is keyed by.
#[derive(Default)]
struct Additions {
    /// A group is what goes in one place: each addition's day, its order among
    /// the additions, and its text.
    groups: BTreeMap<usize, Vec<(Day, usize, String)>>,
}

impl Additions {
    fn add(&mut self, before: usize, day: Day, text: String) {
        let group = self.groups.entry(before).or_default();
        group.push((day, group.len(), text));
    }

    /// `lines` with the additions in, the lines of a group in day order and,
    /// within a day, in the order they were added. A file's own line endings
    /// are kept.
    fn splice(mut self, lines: &[&str]) -> String {
        let eol = if lines.first().is_some_and(|line| line.ends_with("\r\n")) {
            "\r\n"
        } else {
            "\n"
        };
        let mut text = String::new();
        for at in 0..=lines.len() {
            if let Some(mut group) = self.groups.remove(&at) {
                group.sort();
                if !text.is_empty() && !text.ends_with('\n') {
                    text += eol;
                }
                for line in group.iter().flat_map(|(_, _, added)| added.lines()) {
                    text += line;
                    text += eol;
                }
            }
            text += lines.get(at).copied().unwrap_or("");
        }
        text
    }
}

fn insert_items(text: &str, path: &str, adds: &[(Day, &str)]) -> Result<String, Vec<Diagnostic>> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let (items, last) = scan(&lines, Context::of_path(path));
    let dated = by_day(&items);
    let mut additions = Additions::default();
    for &(day, body) in adds {
        let (at, ctx) = place(&dated, day, (lines.len(), last));
        additions.add(at, day, format_item(path, ctx.shorten(day), body)?);
    }
    Ok(additions.splice(&lines))
}

/// Format just the new transaction, leaving every existing source byte alone.
/// Running `axiom fmt` over the combined journal here would make sync rewrite
/// unrelated transactions in that file.
fn format_item(path: &str, date: String, body: &str) -> Result<String, Vec<Diagnostic>> {
    let source = format!("{date} {body}\n");
    let (file, problems) = axiom_syntax::parse(FileId(0), &source, Folder::of(path));
    if !problems.is_empty() {
        return Err(problems
            .into_iter()
            .map(|problem| problem.note("sync refused to write invalid generated Axiom syntax"))
            .collect());
    }
    Ok(format(&source, &file)
        .trim_end_matches(['\r', '\n'])
        .to_string())
}

/// Whether a path names a file beneath the project root. A sync declaration
/// cannot read or propose a change outside the project.
pub(crate) fn is_project_path(path: &str) -> bool {
    use std::path::Component;

    let drive_prefix = path.as_bytes().get(..2).is_some_and(|head| {
        head[0].is_ascii_alphabetic() && head[1] == b':'
    });
    if path.is_empty() || path.starts_with('/') || path.contains('\\') || drive_prefix {
        return false;
    }
    Path::new(path)
        .components()
        .all(|part| matches!(part, Component::Normal(_)))
}

/// Parse one generated item with a complete date before planning a file change.
pub(crate) fn validate_item(path: &str, day: Day, body: &str) -> Result<(), Vec<Diagnostic>> {
    let source = format!("{day} {body}\n");
    let (_, problems) = axiom_syntax::parse(FileId(0), &source, Folder::of(path));
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems
            .into_iter()
            .map(|problem| problem.note("sync refused to write invalid generated Axiom syntax"))
            .collect())
    }
}

/// Parse a generated row in the native declaration shape that will contain it.
pub(crate) fn validate_row(path: &str, name: &str, row: &str) -> Result<(), Vec<Diagnostic>> {
    let source = format!("param {name}\n  {row}\n");
    let (_, problems) = axiom_syntax::parse(FileId(0), &source, Folder::of(path));
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems
            .into_iter()
            .map(|problem| problem.note("sync refused to write an invalid generated param row"))
            .collect())
    }
}

/// What a row of a param starts with: the day it holds from (`2026` is the
/// first of that year, `2026-03` of that month), and the names that follow.
pub fn row_key(row: &str) -> Option<(Day, String)> {
    let mut words = row.split_whitespace();
    let since = match words.next()? {
        year if year.len() == 4 => Day::from_ymd(year.parse().ok()?, 1, 1)?,
        month if month.len() == 7 => {
            let (year, month) = month.split_once('-')?;
            Day::from_ymd(year.parse().ok()?, month.parse().ok()?, 1)?
        }
        date => Day::parse(date.as_bytes())?,
    };
    let is_name = |word: &&str| word.starts_with(|c: char| c.is_ascii_lowercase());
    Some((
        since,
        words.take_while(is_name).collect::<Vec<_>>().join(" "),
    ))
}

/// The rows under a `param NAME` line: one item each, and where the block ends.
struct Block<'t> {
    rows: Vec<Item>,
    end: usize,
    indent: &'t str,
}

fn block<'t>(lines: &[&'t str], param: &str) -> Option<Block<'t>> {
    let declared = |line: &str| {
        let mut words = line.split_whitespace();
        !line.starts_with([' ', '\t'])
            && words.next() == Some("param")
            && words.next() == Some(param)
    };
    let header = lines.iter().position(|line| declared(line))?;
    let mut block = Block {
        rows: Vec::new(),
        end: header + 1,
        indent: "  ",
    };
    for (at, line) in lines
        .iter()
        .enumerate()
        .skip(header + 1)
        .filter(|(_, line)| !line.trim().is_empty())
    {
        if !line.starts_with([' ', '\t']) {
            break;
        }
        block.end = at + 1;
        if !line.trim_start().starts_with("//") {
            if block.rows.is_empty() {
                block.indent = &line[..line.len() - line.trim_start().len()];
            }
            let day = row_key(line).map(|(since, _)| since);
            block.rows.push(Item {
                day,
                head: at,
                start: at,
                end: at + 1,
                ctx: Context::default(),
            });
        }
    }
    Some(block)
}

/// The keys of the rows already under `param NAME`; `None` if there is no such
/// block.
pub fn row_keys(text: &str, param: &str) -> Option<Vec<(Day, String)>> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let block = block(&lines, param)?;
    Some(
        block
            .rows
            .iter()
            .filter_map(|row| row_key(lines[row.head]))
            .collect(),
    )
}

/// Adds rows to the block of `param NAME`, each after the last row that is not
/// later.
fn insert_rows(text: &str, param: &str, rows: &[(Day, &str)]) -> Option<String> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let block = block(&lines, param)?;
    let dated = by_day(&block.rows);
    let mut additions = Additions::default();
    for &(since, row) in rows {
        let (at, _) = place(&dated, since, (block.end, Context::default()));
        additions.add(at, since, format!("{}{row}", block.indent));
    }
    Some(additions.splice(&lines))
}

/// The text of `path` with the inserts in.
fn apply(text: &str, path: &str, inserts: &[&Insert]) -> Result<String, Vec<Diagnostic>> {
    let mut items = Vec::new();
    let mut rows: BTreeMap<&str, Vec<(Day, &str)>> = BTreeMap::new();
    for insert in inserts {
        match &insert.form {
            Form::Item(body) => items.push((insert.day, body.as_str())),
            Form::Row { param, text } => rows
                .entry(param.as_str())
                .or_default()
                .push((insert.day, text.as_str())),
        }
    }
    let mut text = if items.is_empty() {
        text.to_string()
    } else {
        insert_items(text, path, &items)?
    };
    for (param, rows) in rows {
        text = insert_rows(&text, param, &rows).ok_or_else(|| {
            vec![Diagnostic::error(
                "no-such-param",
                format!("`param {param}` is not declared in {path}"),
            )]
        })?;
    }
    Ok(text)
}

/// A file as it is, and as it would be.
pub struct Change {
    pub path: String,
    /// `None` for a file that does not exist yet.
    pub before: Option<String>,
    pub after: String,
}

/// The change each file's inserts make, files in path order.
pub fn changes(
    inserts: &[Insert],
    read: &dyn Fn(&str) -> Option<String>,
) -> Result<Vec<Change>, Vec<Diagnostic>> {
    let mut by_path: BTreeMap<&str, Vec<&Insert>> = BTreeMap::new();
    for insert in inserts {
        if !is_project_path(&insert.path) {
            return Err(vec![Diagnostic::error(
                "sync-path-outside-project",
                format!("`{}` is not a project-relative file path", insert.path),
            )]);
        }
        by_path.entry(&insert.path).or_default().push(insert);
    }
    let change = |(path, inserts): (&str, Vec<&Insert>)| -> Result<Change, Vec<Diagnostic>> {
        let before = read(path);
        let after = apply(before.as_deref().unwrap_or(""), path, &inserts)?;
        let (_, problems) = axiom_syntax::parse(FileId(0), &after, Folder::of(path));
        if !problems.is_empty() {
            return Err(problems
                .into_iter()
                .map(|problem| problem.note("sync refused to plan a file with invalid Axiom syntax"))
                .collect());
        }
        Ok(Change {
            path: path.to_string(),
            before,
            after,
        })
    };
    by_path.into_iter().map(change).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(text: &str) -> Day {
        Day::parse(text.as_bytes()).unwrap()
    }

    #[test]
    fn path_validation_rejects_parent_absolute_and_windows_drive_paths_on_every_host() {
        for path in ["", "../outside.ax", "/outside.ax", "C:/outside.ax", "d:outside.ax", "\\\\server\\share.ax"] {
            assert!(!is_project_path(path), "{path:?}");
        }
        for path in ["journal.ax", "imports/checking.csv", "journal/2026/03.ax"] {
            assert!(is_project_path(path), "{path:?}");
        }
    }

    /// `text` with each `(day, body)` put in, as sync would write it into `path`.
    fn write(path: &str, text: &str, adds: &[(&str, &str)]) -> String {
        let inserts: Vec<Insert> = adds
            .iter()
            .map(|&(date, body)| Insert {
                path: path.into(),
                day: day(date),
                form: Form::Item(body.into()),
            })
            .collect();
        apply(text, path, &inserts.iter().collect::<Vec<_>>()).unwrap()
    }

    #[test]
    fn a_path_says_the_year_and_the_month() {
        let context = |path: &str| Context::of_path(path);
        let write = |path: &str, date: &str| context(path).shorten(day(date));
        assert_eq!(write("journal/2026/03.ax", "2026-03-05"), "05");
        assert_eq!(write("journal/2026/03.ax", "2026-04-01"), "04-01");
        assert_eq!(write("journal/2026/03.ax", "2027-01-01"), "2027-01-01");
        assert_eq!(write("journal/2026-03.ax", "2026-03-05"), "05");
        assert_eq!(write("2026.ax", "2026-03-05"), "03-05");
        assert_eq!(write("journal/2026/notes.ax", "2026-03-05"), "03-05");
        assert_eq!(write("journal.ax", "2026-03-05"), "2026-03-05");
        assert_eq!(
            write("03/2026.ax", "2026-03-05"),
            "03-05",
            "a month above the year says nothing"
        );
    }

    #[test]
    fn a_new_item_uses_the_house_formatter_and_parses_back() {
        let path = "journal/2026/03.ax";
        let item = format_item(path, "05".into(), "checking -> store 12 USD \"memo\"").unwrap();
        let source = format!("{item}\n");
        let (file, problems) = axiom_syntax::parse(FileId(0), &source, Folder::of(path));
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(format(&source, &file), source);
    }

    #[test]
    fn a_day_belongs_to_the_file_the_project_would_have_put_it_in() {
        let files = [
            "axiom.ax",
            "journal/2026/01.ax",
            "journal/2026/02.ax",
            "prices/2025.ax",
            "journal/2026/notes.ax",
        ];
        let layout = Layout::new(files);
        assert_eq!(layout.file_for(day("2026-02-14")), "journal/2026/02.ax");
        assert_eq!(
            layout.file_for(day("2026-03-01")),
            "journal/2026/03.ax",
            "the latest habit, carried on"
        );
        assert_eq!(layout.file_for(day("2027-01-01")), "journal/2027/01.ax");
        assert_eq!(
            Layout::new(["2025.ax", "notes.ax"]).file_for(day("2026-03-01")),
            "2026.ax"
        );
        assert_eq!(
            Layout::new(["journal/2025-12.ax"]).file_for(day("2026-03-01")),
            "journal/2026-03.ax"
        );
        assert_eq!(
            Layout::new(["axiom.ax", "journal.ax"]).file_for(day("2026-03-01")),
            "journal/2026/03.ax"
        );
        assert_eq!(
            Layout::new(["2026.ax", "2026/01.ax"]).file_for(day("2026-01-09")),
            "2026/01.ax"
        );
    }

    const MARCH: &str = "\
// March.

01 flat
05 me -> corner-store 9.80 USD
  \"food\"
  // kept together
05 phone

10 job
  retirement 276.00 USD
  checking ...

31 checking = 8_828.87 USD
";

    #[test]
    fn new_lines_go_in_day_order_after_the_last_of_their_day() {
        let path = "journal/2026/03.ax";
        let out = write(
            path,
            MARCH,
            &[
                ("2026-03-05", "visa -> a 1 USD"),
                ("2026-03-07", "visa -> b 2 USD"),
                ("2026-03-31", "visa -> c 3 USD"),
            ],
        );
        let expected = MARCH
            .replace(
                "05 phone\n",
                "05 phone\n05 visa -> a 1 USD\n07 visa -> b 2 USD\n",
            )
            .replace(
                "31 checking = 8_828.87 USD\n",
                "31 checking = 8_828.87 USD\n31 visa -> c 3 USD\n",
            );
        assert_eq!(out, expected);
    }

    #[test]
    fn an_item_stays_whole_and_what_is_written_is_never_reformatted() {
        let out = write(
            "journal/2026/03.ax",
            MARCH,
            &[
                ("2026-03-10", "visa -> a 1 USD"),
                ("2026-03-02", "visa -> b 2 USD"),
            ],
        );
        let without_new = out
            .replace("10 visa -> a 1 USD\n", "")
            .replace("02 visa -> b 2 USD\n", "");
        assert_eq!(
            without_new, MARCH,
            "every line that was there is there, as it was"
        );
        assert!(
            out.contains("  checking ...\n10 visa -> a 1 USD\n"),
            "{out}"
        );
        assert!(out.contains("01 flat\n02 visa -> b 2 USD\n05 me"), "{out}");
    }

    #[test]
    fn before_everything_after_everything_and_in_a_file_that_is_not_there() {
        let item = "a -> b 1 USD";
        let early = write("journal/2026/03.ax", "10 a\n", &[("2026-03-01", item)]);
        assert_eq!(early, "01 a -> b 1 USD\n10 a\n");
        let late = write("journal/2026/03.ax", "10 a", &[("2026-03-20", item)]);
        assert_eq!(
            late, "10 a\n20 a -> b 1 USD\n",
            "a file without a last newline gets one"
        );
        assert_eq!(
            write(
                "journal/2026/03.ax",
                "",
                &[("2026-03-20", item), ("2026-03-02", item)]
            ),
            "02 a -> b 1 USD\n20 a -> b 1 USD\n"
        );
        assert_eq!(
            write("journal.ax", "entity a\n", &[("2026-03-20", item)]),
            "entity a\n2026-03-20 a -> b 1 USD\n"
        );
        assert_eq!(
            write(
                "journal/2026/03.ax",
                "10 a\r\n20 b\r\n",
                &[("2026-03-15", item)]
            ),
            "10 a\r\n15 a -> b 1 USD\r\n20 b\r\n"
        );
    }

    #[test]
    fn headings_give_short_dates_their_year_and_month() {
        let text = "2025\n12-30 a\n\n2026-01\n05 b\n\n2026-02\n03 c\n";
        let out = write(
            "journal.ax",
            text,
            &[
                ("2026-01-20", "a -> b 1 USD"),
                ("2026-02-10", "a -> b 1 USD"),
                ("2026-03-01", "a -> b 1 USD"),
                ("2025-12-31", "a -> b 1 USD"),
            ],
        );
        assert_eq!(
            out,
            "2025\n12-30 a\n12-31 a -> b 1 USD\n\n2026-01\n05 b\n20 a -> b 1 USD\n\n2026-02\n03 c\n10 a -> b 1 USD\n03-01 a -> b 1 USD\n"
        );
    }

    #[test]
    fn a_line_goes_where_its_date_is_shorter() {
        // After January's last line the date is `02-01`; below the February heading it is `01`.
        let text = "2026-01\n05 b\n\n2026-02\n03 c\n";
        assert_eq!(
            write("journal.ax", text, &[("2026-02-01", "a -> b 1 USD")]),
            "2026-01\n05 b\n\n2026-02\n01 a -> b 1 USD\n03 c\n"
        );
    }

    #[test]
    fn opening_and_ranges_start_with_their_day() {
        let text = "opening 01\n  checking 5 USD\n\n03-01..05-31 gym 10 USD\n";
        let out = write("journal/2026/03.ax", text, &[("2026-03-02", "a -> b 1 USD")]);
        assert_eq!(
            out,
            "opening 01\n  checking 5 USD\n\n03-01..05-31 gym 10 USD\n02 a -> b 1 USD\n"
        );
    }

    #[test]
    fn param_rows_join_their_block_in_order() {
        let text = "param cpi\n  2024 310.3\n  2026 320.9\n\nparam other\n  2025 1\n";
        let rows = [
            (day("2025-01-01"), "2025 315.6"),
            (day("2027-01-01"), "2027 325.0"),
        ];
        let out = insert_rows(text, "cpi", &rows).unwrap();
        assert_eq!(
            out,
            "param cpi\n  2024 310.3\n  2025 315.6\n  2026 320.9\n  2027 325.0\n\nparam other\n  2025 1\n"
        );
        assert_eq!(insert_rows(text, "missing", &rows), None);
        assert_eq!(
            insert_rows("param empty\n", "empty", &rows).unwrap(),
            "param empty\n  2025 315.6\n  2027 325.0\n"
        );
        let keys = row_keys(
            "param limit\n  2026 single 0 USD 10%\n  2026-07 joint 5\n",
            "limit",
        )
        .unwrap();
        assert_eq!(
            keys,
            [
                (day("2026-01-01"), "single".to_string()),
                (day("2026-07-01"), "joint".to_string())
            ]
        );
    }

    #[test]
    #[ignore = "a timing, alone: cargo test -p axiom-sync --release -- --ignored --test-threads=1"]
    fn a_hundred_thousand_lines_into_a_file_of_two_hundred_thousand() {
        let first = day("2016-01-01");
        let mut text = String::new();
        for at in 0..200_000 {
            text += &format!("{} visa -> shop 1 USD\n", first.add_days(at / 55));
        }
        let adds: Vec<(Day, String)> = (0..100_000)
            .map(|at| {
                (
                    first.add_days((at * 7 % 3650) as i32),
                    format!("visa -> new-{at} 2 USD"),
                )
            })
            .collect();
        let inserts: Vec<Insert> = adds
            .iter()
            .map(|(day, body)| Insert {
                path: "journal.ax".into(),
                day: *day,
                form: Form::Item(body.clone()),
            })
            .collect();
        let started = std::time::Instant::now();
        let out = apply(&text, "journal.ax", &inserts.iter().collect::<Vec<_>>());
        eprintln!("100,000 lines into 200,000 in {:?}", started.elapsed());
        assert_eq!(out.lines().count(), 300_000);
        assert!(
            started.elapsed().as_millis() < 1000,
            "{:?}",
            started.elapsed()
        );
        let dates: Vec<&str> = out
            .lines()
            .map(|line| line.split(' ').next().unwrap())
            .collect();
        assert!(
            dates.windows(2).all(|pair| pair[0] <= pair[1]),
            "the file is still in day order"
        );
    }

    #[test]
    fn changes_are_per_file_and_leave_untouched_files_out() {
        let inserts = [
            Insert {
                path: "b.ax".into(),
                day: day("2026-01-02"),
                form: Form::Item("a -> b 1 USD".into()),
            },
            Insert {
                path: "a.ax".into(),
                day: day("2026-01-02"),
                form: Form::Item("a -> b 2 USD".into()),
            },
        ];
        let read = |path: &str| (path == "a.ax").then(|| "2026-01-01 a -> b 1 USD\n".to_string());
        let made = changes(&inserts, &read).unwrap();
        let shown: Vec<_> = made
            .iter()
            .map(|c| (c.path.as_str(), c.before.is_some(), c.after.as_str()))
            .collect();
        assert_eq!(
            shown,
            [
                (
                    "a.ax",
                    true,
                    "2026-01-01 a -> b 1 USD\n2026-01-02 a -> b 2 USD\n"
                ),
                ("b.ax", false, "2026-01-02 a -> b 1 USD\n")
            ]
        );
    }
}
