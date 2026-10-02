//! Sources that print Axiom: invoices, prices, the rows of a param. What the
//! command printed is merged into the journal, a file or a param. What is
//! already there (the same day and subject, the same row key) is kept as
//! written, and the rest is added in order.

use std::borrow::Cow;

use axiom_core::{Day, Diagnostic, FileId, Loc, Map};
use axiom_syntax::Folder;

use crate::paths::is_project_path;
use crate::write::{Item, Layout, row_key, row_keys, scan, validate_item_source_at, validate_row_at};
use crate::{Form, Insert};

/// Internal adapter from the model sink declaration to the shared merger.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Sink<'a> {
    /// Into the journal, each item into the file its day belongs to.
    Journal,
    /// Into one file; `{year}` and `{month}` in the path split it by the item's day.
    File(&'a str),
    /// Into the rows of `param NAME`, declared in the file at `path`.
    Param { name: &'a str, path: &'a str },
}

pub(crate) fn merge_at<'a>(
    sink: Sink,
    output: &str,
    layout: &Layout,
    file: FileId,
    read: &mut dyn FnMut(&str) -> Option<Cow<'a, str>>,
) -> Result<Vec<Insert>, Vec<Diagnostic>> {
    match sink {
        Sink::Journal => items(output, |day| layout.file_for(day), file, read),
        Sink::File(pattern) => items(output, |day| file_of(pattern, day), file, read),
        Sink::Param { name, path } => rows(name, path, output, file, read),
    }
}

/// The file an item of `day` goes to, when a sink's path has a `{year}` and a `{month}` to split it by.
fn file_of(pattern: &str, day: Day) -> String {
    let (year, month, _) = day.ymd();
    pattern.replace("{year}", &format!("{year:04}")).replace("{month}", &format!("{month:02}"))
}

/// Existing files that `merge` may inspect for this output. The planner uses
/// this to load targets before it borrows input text from its source catalog.
pub(crate) fn target_paths(sink: Sink<'_>, output: &str, layout: &Layout) -> Vec<String> {
    let paths = match sink {
        Sink::Journal => dated_targets(output, |day| layout.file_for(day)),
        Sink::File(pattern) => dated_targets(output, |day| file_of(pattern, day)),
        Sink::Param { path, .. } => vec![path.to_string()],
    };
    let mut unique = std::collections::BTreeSet::new();
    paths.into_iter().filter(|path| is_project_path(path)).filter(|path| unique.insert(path.clone())).collect()
}

fn dated_targets(output: &str, path_of: impl Fn(Day) -> String) -> Vec<String> {
    let lines: Vec<&str> = output.split_inclusive('\n').collect();
    scan(&lines, Folder::default())
        .0
        .into_iter()
        .filter_map(|item| item.day.filter(|_| !lines[item.head].starts_with("opening")).map(|day| path_of(day)))
        .collect()
}

fn items<'a>(
    output: &str,
    path_of: impl Fn(Day) -> String,
    file: FileId,
    read: &mut dyn FnMut(&str) -> Option<Cow<'a, str>>,
) -> Result<Vec<Insert>, Vec<Diagnostic>> {
    let printed = Printed::new(output, file);
    let (found, _) = scan(&printed.lines, Folder::default());
    let (mut inserts, mut problems) = (Vec::new(), Vec::new());
    let mut present: Map<String, Map<(Day, String), usize>> = Map::default();
    for item in &found {
        let (day, path) = match printed.filed(item, &path_of) {
            Ok(filed) => filed,
            Err(bad) => {
                problems.extend(bad);
                continue;
            }
        };
        let there =
            present.entry(path.clone()).or_insert_with(|| subjects_in(read(&path).as_deref().unwrap_or(""), &path));
        if is_new(there, &(day, subject(&printed.lines, item))) {
            inserts.push(Insert { path, day, form: Form::Item(body(&printed.lines, item)) });
        }
    }
    if problems.is_empty() { Ok(inserts) } else { Err(problems) }
}

/// What a command printed, line by line.
struct Printed<'o> {
    lines: Vec<&'o str>,
    /// Where each line starts, in bytes.
    starts: Vec<usize>,
    file: FileId,
}

impl<'o> Printed<'o> {
    fn new(output: &'o str, file: FileId) -> Printed<'o> {
        let lines: Vec<&str> = output.split_inclusive('\n').collect();
        let starts = lines
            .iter()
            .scan(0, |end, line| {
                let start = *end;
                *end += line.len();
                Some(start)
            })
            .collect();
        Printed { lines, starts, file }
    }

    /// Where line `at` is, without its line ending.
    fn loc(&self, at: usize) -> Loc {
        let start = self.starts[at];
        Loc::new(self.file, start as u32, (start + self.lines[at].trim_end().len()) as u32)
    }

    /// The day an item is filed under, and the file that day goes to; or what is wrong with it.
    fn filed(&self, item: &Item, path_of: &impl Fn(Day) -> String) -> Result<(Day, String), Vec<Diagnostic>> {
        let head = self.lines[item.head];
        let Some(day) = item.day.filter(|_| !head.starts_with("opening")) else {
            let headline = "the output has a line that does not start with a date";
            return Err(vec![
                Diagnostic::error("undated-line", headline)
                    .label(self.loc(item.head), "expected a date such as 2026-03-05")
                    .help("print full dates: sync files each line by its day"),
            ]);
        };
        let path = path_of(day);
        if !is_project_path(&path) {
            return Err(vec![outside_project(&path)]);
        }
        let source = self.lines[item.head..item.end].concat();
        validate_item_source_at(&path, &source, self.file, self.starts[item.head])?;
        Ok((day, path))
    }

    /// The year or day a row is filed under; or what is wrong with it.
    fn keyed(&self, at: usize, name: &str, path: &str) -> Result<(Day, String), Vec<Diagnostic>> {
        let (line, row) = (self.lines[at], self.lines[at].trim());
        let Some(key) = row_key(row) else {
            let headline = "the output has a row that does not start with a year or a date";
            let label = "expected `2026`, `2026-03` or `2026-03-05` here";
            return Err(vec![Diagnostic::error("bad-row", headline).label(self.loc(at), label)]);
        };
        let offset = self.starts[at] + line.find(row).unwrap_or(0);
        validate_row_at(path, name, row, self.file, offset)?;
        Ok(key)
    }
}

fn outside_project(path: &str) -> Diagnostic {
    Diagnostic::error("sync-path-outside-project", format!("`{path}` is not a project-relative file path"))
}

/// Whether `key` is not yet in the file: if the file has one more than the output has used, that one is.
fn is_new(present: &mut Map<(Day, String), usize>, key: &(Day, String)) -> bool {
    match present.get_mut(key) {
        Some(count) if *count > 0 => {
            *count -= 1;
            false
        }
        _ => true,
    }
}

/// How many items each day and subject have in a file.
fn subjects_in(text: &str, path: &str) -> Map<(Day, String), usize> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut counts = Map::default();
    for item in scan(&lines, Folder::of(path)).0 {
        if let Some(day) = item.day {
            *counts.entry((day, subject(&lines, &item))).or_default() += 1;
        }
    }
    counts
}

/// What an item is about: the words after its date up to the first amount,
/// sign or text, and every code on the line. Two invoices to one client on one
/// day differ in their codes.
fn subject(lines: &[&str], item: &Item) -> String {
    let mut words = lines[item.head].split_whitespace();
    if words.next() == Some("opening") {
        words.next();
    }
    let rest: Vec<&str> = words.collect();
    let stops =
        |word: &&str| word.starts_with(|c: char| c.is_ascii_digit() || matches!(c, '(' | '=' | '"' | '#' | '^'));
    let name = rest.iter().take_while(|word| !stops(word));
    let codes = rest.iter().filter(|word| word.starts_with('^'));
    name.chain(codes).copied().collect::<Vec<_>>().join(" ")
}

/// What follows an item's date, and the lines under it, as they were printed.
fn body(lines: &[&str], item: &Item) -> String {
    let head = lines[item.head].trim_end();
    let mut body = head.split_once(char::is_whitespace).map_or("", |(_, rest)| rest.trim_start()).to_string();
    for line in &lines[item.head + 1..item.end] {
        body += "\n";
        body += line.trim_end();
    }
    body
}

fn rows<'a>(
    name: &str,
    path: &str,
    output: &str,
    file: FileId,
    read: &mut dyn FnMut(&str) -> Option<Cow<'a, str>>,
) -> Result<Vec<Insert>, Vec<Diagnostic>> {
    if !is_project_path(path) {
        return Err(vec![outside_project(path)]);
    }
    let existing = read(path).and_then(|text| row_keys(text.as_ref(), name)).ok_or_else(|| {
        vec![
            Diagnostic::error("no-such-param", format!("`param {name}` is not declared in {path}"))
                .help(format!("declare it there: `param {name}`")),
        ]
    })?;
    let mut present: Map<(Day, String), usize> = Map::default();
    for key in existing {
        *present.entry(key).or_default() += 1;
    }
    let printed = Printed::new(output, file);
    let (mut inserts, mut problems) = (Vec::new(), Vec::new());
    for (at, line) in printed.lines.iter().enumerate() {
        let row = line.trim();
        if row.is_empty() || row.starts_with("//") {
            continue;
        }
        match printed.keyed(at, name, path) {
            Ok(key) if is_new(&mut present, &key) => {
                let form = Form::Row { param: name.to_string(), text: row.to_string() };
                inserts.push(Insert { path: path.to_string(), day: key.0, form });
            }
            Ok(_) => {}
            Err(bad) => problems.extend(bad),
        }
    }
    if problems.is_empty() { Ok(inserts) } else { Err(problems) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::write::changes_at;

    const INVOICES: &str = "\
2026-03-27 halcyon owes studio 3_800 USD due 30d ^inv-2026-01
  3_000 USD #design \"brand refresh\"
    800 USD #design \"icon set\"
2026-03-27 halcyon owes studio 500 USD due 30d ^inv-2026-02
2026-04-02 northwind owes studio 900 USD due 30d ^inv-2026-03
";

    fn layout() -> Layout {
        Layout::new(["journal/2026/03.ax", "journal/2026/02.ax"])
    }

    /// The text of every file after merging `output`, given what `files` say.
    fn merged(sink: Sink, output: &str, files: &[(&str, &str)]) -> Result<Vec<(String, String)>, Vec<Diagnostic>> {
        let read = |path: &str| files.iter().find(|(name, _)| *name == path).map(|(_, text)| text.to_string());
        let read = read;
        let mut borrowed = |path: &str| read(path).map(Cow::Owned);
        let inserts = merge_at(sink, output, &layout(), FileId(40), &mut borrowed)?;
        Ok(changes_at(&inserts, FileId(41), &mut |path| read(path).map(Cow::Owned))
            .unwrap()
            .into_iter()
            .map(|change| (change.path, change.after))
            .collect())
    }

    #[test]
    fn axiom_output_joins_the_journal_by_day_and_only_what_is_new() {
        let march = "27 halcyon owes studio 3_800 USD due 30d ^inv-2026-01\n  3_000 USD #design \"brand refresh\"\n    800 USD #design \"icon set\"\n30 me -> pge 9 USD\n";
        let written = merged(Sink::Journal, INVOICES, &[("journal/2026/03.ax", march)]).unwrap();
        assert_eq!(
            written,
            [
                (
                    "journal/2026/03.ax".to_string(),
                    format!("{march}").replace(
                        "30 me -> pge 9 USD\n",
                        "27 halcyon owes studio 500 USD ^inv-2026-02 due 30d\n30 me -> pge 9 USD\n"
                    )
                ),
                (
                    "journal/2026/04.ax".to_string(),
                    "02 northwind owes studio 900 USD ^inv-2026-03 due 30d\n".to_string()
                ),
            ]
        );
        let again: Vec<(String, String)> = written.iter().map(|(path, text)| (path.clone(), text.clone())).collect();
        let files: Vec<(&str, &str)> = again.iter().map(|(path, text)| (path.as_str(), text.as_str())).collect();
        assert!(merged(Sink::Journal, INVOICES, &files).unwrap().is_empty(), "a second sync writes nothing");
    }

    #[test]
    fn malformed_native_output_is_refused_before_a_change_is_planned() {
        let bad_item = "2026-03-27 someone owes 3 USD";
        let mut read = |_: &str| -> Option<Cow<'_, str>> { None };
        let result = merge_at(Sink::Journal, bad_item, &layout(), FileId(40), &mut read);
        assert!(result.is_err(), "incomplete native item must not pass through raw");

        let bad_row = "2026 3_00 USD ???\n";
        let mut read = |path: &str| (path == "settings.ax").then(|| Cow::Owned("param rates\n".to_string()));
        let result =
            merge_at(Sink::Param { name: "rates", path: "settings.ax" }, bad_row, &layout(), FileId(40), &mut read);
        assert!(result.is_err(), "unparseable param output must be refused");
    }

    #[test]
    fn sink_paths_cannot_escape_the_project_or_trigger_external_reads() {
        let read = |_: &str| -> Option<String> { panic!("unsafe sink path must be rejected before reading") };
        let mut borrowed = |path: &str| read(path).map(Cow::Owned);
        let result = merge_at(
            Sink::File("../outside/{year}.ax"),
            "2026-03-27 a -> b 1 USD",
            &layout(),
            FileId(40),
            &mut borrowed,
        );
        assert!(result.is_err());
        let result = merge_at(
            Sink::Param { name: "rates", path: "/tmp/settings.ax" },
            "2026 3 USD\n",
            &layout(),
            FileId(40),
            &mut |path| read(path).map(Cow::Owned),
        );
        assert!(result.is_err());
    }

    #[test]
    fn sink_and_change_planning_accept_a_mutating_read_callback() {
        let mut reads = Vec::new();
        let mut read = |path: &str| {
            reads.push(path.to_string());
            None
        };
        let output = "2026-03-27 a -> b 1 USD";
        let inserts = merge_at(Sink::File("invoices/{year}.ax"), output, &layout(), FileId(40), &mut |path| {
            read(path).map(Cow::Owned)
        })
        .unwrap();
        let changes = changes_at(&inserts, FileId(41), &mut |path| read(path).map(Cow::Owned)).unwrap();
        drop(read);
        assert_eq!(changes.len(), 1);
        assert_eq!(reads, ["invoices/2026.ax", "invoices/2026.ax"]);
    }

    #[test]
    fn an_items_lines_travel_with_it() {
        let written = merged(Sink::Journal, INVOICES, &[]).unwrap();
        let march = &written[0].1;
        assert_eq!(
            march,
            "27 halcyon owes studio 3_800 USD ^inv-2026-01 due 30d\n  3_000 USD #design \"brand refresh\"\n    800 USD #design \"icon set\"\n27 halcyon owes studio 500 USD ^inv-2026-02 due 30d\n"
        );
    }

    #[test]
    fn a_file_with_a_year_in_its_path_splits_the_output_by_year() {
        let prices = "2026-12-30 VTI = 280.14 USD\n2027-01-02 VTI = 301 USD\n2026-12-30 BND = 71.2 USD\n";
        let written =
            merged(Sink::File("prices/{year}.ax"), prices, &[("prices/2026.ax", "12-30 VTI = 280.14 USD\n")]).unwrap();
        assert_eq!(
            written,
            [
                ("prices/2026.ax".to_string(), "12-30 VTI = 280.14 USD\n12-30 BND = 71.2 USD\n".to_string()),
                ("prices/2027.ax".to_string(), "01-02 VTI = 301 USD\n".to_string()),
            ]
        );
    }

    #[test]
    fn a_line_without_a_date_is_reported_where_it_is() {
        let output = "2026-03-27 a 1 USD\nentity oops\n";
        let problems = merged(Sink::Journal, output, &[]).err().expect("refused");
        assert_eq!(problems.len(), 1);
        let loc = problems[0].anchor().unwrap();
        assert_eq!(&output[loc.range()], "entity oops");
        assert_eq!(problems[0].message, "the output has a line that does not start with a date");
    }

    #[test]
    fn param_rows_are_added_unless_their_key_is_there() {
        let file = "param cpi USD\n  2025 single 315.6 USD\n  2026 single 320.9 USD\n";
        let sink = Sink::Param { name: "cpi", path: "params.ax" };
        let written =
            merged(sink, "// the index\n2025 single 999 USD\n2026-07-01 single 322.1 USD\n\n", &[("params.ax", file)])
                .unwrap();
        assert_eq!(
            written,
            [(
                "params.ax".to_string(),
                "param cpi USD\n  2025 single 315.6 USD\n  2026 single 320.9 USD\n  2026-07-01 single 322.1 USD\n"
                    .to_string()
            )]
        );
        let problems = merged(sink, "soon 3\n", &[("params.ax", file)]).err().unwrap();
        assert_eq!(problems[0].message, "the output has a row that does not start with a year or a date");
        let problems =
            merged(Sink::Param { name: "gone", path: "params.ax" }, "2026 1\n", &[("params.ax", file)]).err().unwrap();
        assert_eq!(problems[0].message, "`param gone` is not declared in params.ax");
    }
}
