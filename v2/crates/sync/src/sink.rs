//! Sources that print Axiom: invoices, prices, the rows of a param. What the
//! command printed is merged into the journal, a file or a param. What is
//! already there (the same day and subject, the same row key) is kept as
//! written, and the rest is added in order.

use axiom_core::{Day, Diagnostic, FileId, Loc, Map};

use crate::write::{
    Context, Item, Layout, row_key, row_keys, scan, validate_item, validate_row,
};
use crate::paths::is_project_path;
use crate::{Form, Insert};

/// Where a source's Axiom goes.
#[derive(Clone, Copy, Debug)]
pub enum Sink<'a> {
    /// Into the journal, each item into the file its day belongs to.
    Journal,
    /// Into one file; `{year}` and `{month}` in the path split it by the item's day.
    File(&'a str),
    /// Into the rows of `param NAME`, declared in the file at `path`.
    Param { name: &'a str, path: &'a str },
}

/// The inserts that add what `output` says and the book does not.
pub fn merge(
    sink: Sink,
    output: &str,
    layout: &Layout,
    read: &dyn Fn(&str) -> Option<String>,
) -> Result<Vec<Insert>, Vec<Diagnostic>> {
    match sink {
        Sink::Journal => items(output, |day| layout.file_for(day), read),
        Sink::File(pattern) => items(
            output,
            |day| {
                let (year, month, _) = day.ymd();
                pattern
                    .replace("{year}", &format!("{year:04}"))
                    .replace("{month}", &format!("{month:02}"))
            },
            read,
        ),
        Sink::Param { name, path } => rows(name, path, output, read),
    }
}

fn items(
    output: &str,
    path_of: impl Fn(Day) -> String,
    read: &dyn Fn(&str) -> Option<String>,
) -> Result<Vec<Insert>, Vec<Diagnostic>> {
    let lines: Vec<&str> = output.split_inclusive('\n').collect();
    let (found, _) = scan(&lines, Context::default());
    let (mut inserts, mut problems) = (Vec::new(), Vec::new());
    let mut present: Map<String, Map<(Day, String), usize>> = Map::default();
    for item in &found {
        let dated = item
            .day
            .filter(|_| !lines[item.head].starts_with("opening"));
        let Some(day) = dated else {
            let headline = "the output has a line that does not start with a date".to_string();
            problems.push(
                Diagnostic::error("undated-line", headline)
                    .label(
                        line_loc(&lines, item.head),
                        "expected a date such as 2026-03-05",
                    )
                    .help("print full dates: sync files each line by its day"),
            );
            continue;
        };
        let path = path_of(day);
        if !is_project_path(&path) {
            problems.push(Diagnostic::error(
                "sync-path-outside-project",
                format!("`{path}` is not a project-relative file path"),
            ));
            continue;
        }
        let item_body = body(&lines, item);
        if let Err(bad) = validate_item(&path, day, &item_body) {
            problems.extend(bad);
            continue;
        }
        let there = present
            .entry(path.clone())
            .or_insert_with(|| subjects_in(read(&path).as_deref().unwrap_or(""), &path));
        match there.get_mut(&(day, subject(&lines, item))) {
            Some(count) if *count > 0 => *count -= 1,
            _ => inserts.push(Insert {
                path,
                day,
                form: Form::Item(item_body),
            }),
        }
    }
    if problems.is_empty() {
        Ok(inserts)
    } else {
        Err(problems)
    }
}

/// Where line `at` of what a command printed is, without its line ending.
fn line_loc(lines: &[&str], at: usize) -> Loc {
    let start: usize = lines[..at].iter().map(|line| line.len()).sum();
    Loc::new(
        FileId(0),
        start as u32,
        (start + lines[at].trim_end().len()) as u32,
    )
}

/// How many items each day and subject have in a file.
fn subjects_in(text: &str, path: &str) -> Map<(Day, String), usize> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut counts = Map::default();
    for item in scan(&lines, Context::of_path(path)).0 {
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
    let stops = |word: &&str| {
        word.starts_with(|c: char| {
            c.is_ascii_digit() || matches!(c, '(' | '=' | '"' | '#' | '^')
        })
    };
    let name = rest.iter().take_while(|word| !stops(word));
    let codes = rest.iter().filter(|word| word.starts_with('^'));
    name.chain(codes).copied().collect::<Vec<_>>().join(" ")
}

/// What follows an item's date, and the lines under it, as they were printed.
fn body(lines: &[&str], item: &Item) -> String {
    let head = lines[item.head].trim_end();
    let mut body = head
        .split_once(char::is_whitespace)
        .map_or("", |(_, rest)| rest.trim_start())
        .to_string();
    for line in &lines[item.head + 1..item.end] {
        body += "\n";
        body += line.trim_end();
    }
    body
}

fn rows(
    name: &str,
    path: &str,
    output: &str,
    read: &dyn Fn(&str) -> Option<String>,
) -> Result<Vec<Insert>, Vec<Diagnostic>> {
    if !is_project_path(path) {
        return Err(vec![Diagnostic::error(
            "sync-path-outside-project",
            format!("`{path}` is not a project-relative file path"),
        )]);
    }
    let existing = read(path)
        .and_then(|text| row_keys(&text, name))
        .ok_or_else(|| {
            vec![
                Diagnostic::error(
                    "no-such-param",
                    format!("`param {name}` is not declared in {path}"),
                )
                .help(format!("declare it there: `param {name}`")),
            ]
        })?;
    let mut present: Map<(Day, String), usize> = Map::default();
    for key in existing {
        *present.entry(key).or_default() += 1;
    }
    let lines: Vec<&str> = output.split_inclusive('\n').collect();
    let (mut inserts, mut problems) = (Vec::new(), Vec::new());
    for (at, line) in lines.iter().enumerate() {
        let row = line.trim();
        if row.is_empty() || row.starts_with("//") {
            continue;
        }
        let Some(key) = row_key(row) else {
            let headline =
                "the output has a row that does not start with a year or a date".to_string();
            let label = "expected `2026`, `2026-03` or `2026-03-05` here";
            problems
                .push(Diagnostic::error("bad-row", headline).label(line_loc(&lines, at), label));
            continue;
        };
        if let Err(bad) = validate_row(path, name, row) {
            problems.extend(bad);
            continue;
        }
        match present.get_mut(&key) {
            Some(count) if *count > 0 => *count -= 1,
            _ => inserts.push(Insert {
                path: path.to_string(),
                day: key.0,
                form: Form::Row {
                    param: name.to_string(),
                    text: row.to_string(),
                },
            }),
        }
    }
    if problems.is_empty() {
        Ok(inserts)
    } else {
        Err(problems)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::write::changes;

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
    fn merged(
        sink: Sink,
        output: &str,
        files: &[(&str, &str)],
    ) -> Result<Vec<(String, String)>, Vec<Diagnostic>> {
        let read = |path: &str| {
            files
                .iter()
                .find(|(name, _)| *name == path)
                .map(|(_, text)| text.to_string())
        };
        let inserts = merge(sink, output, &layout(), &read)?;
        Ok(changes(&inserts, &read)
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
                        "27 halcyon owes studio 500 USD due 30d ^inv-2026-02\n30 me -> pge 9 USD\n"
                    )
                ),
                (
                    "journal/2026/04.ax".to_string(),
                    "02 northwind owes studio 900 USD due 30d ^inv-2026-03\n".to_string()
                ),
            ]
        );
        let again: Vec<(String, String)> = written
            .iter()
            .map(|(path, text)| (path.clone(), text.clone()))
            .collect();
        let files: Vec<(&str, &str)> = again
            .iter()
            .map(|(path, text)| (path.as_str(), text.as_str()))
            .collect();
        assert!(
            merged(Sink::Journal, INVOICES, &files).unwrap().is_empty(),
            "a second sync writes nothing"
        );
    }

    #[test]
    fn malformed_native_output_is_refused_before_a_change_is_planned() {
        let bad_item = "2026-03-27 someone owes 3 USD";
        let result = merge(Sink::Journal, bad_item, &layout(), &|_| None);
        assert!(result.is_err(), "incomplete native item must not pass through raw");

        let bad_row = "2026 3_00 USD ???\n";
        let result = merge(
            Sink::Param {
                name: "rates",
                path: "settings.ax",
            },
            bad_row,
            &layout(),
            &|path| (path == "settings.ax").then(|| "param rates\n".to_string()),
        );
        assert!(result.is_err(), "unparseable param output must be refused");
    }

    #[test]
    fn sink_paths_cannot_escape_the_project_or_trigger_external_reads() {
        let read = |_: &str| panic!("unsafe sink path must be rejected before reading");
        let result = merge(Sink::File("../outside/{year}.ax"), "2026-03-27 a -> b 1 USD", &layout(), &read);
        assert!(result.is_err());
        let result = merge(
            Sink::Param {
                name: "rates",
                path: "/tmp/settings.ax",
            },
            "2026 3 USD\n",
            &layout(),
            &read,
        );
        assert!(result.is_err());
    }

    #[test]
    fn an_items_lines_travel_with_it() {
        let written = merged(Sink::Journal, INVOICES, &[]).unwrap();
        let march = &written[0].1;
        assert_eq!(
            march,
            "27 halcyon owes studio 3_800 USD due 30d ^inv-2026-01\n  3_000 USD #design \"brand refresh\"\n    800 USD #design \"icon set\"\n27 halcyon owes studio 500 USD due 30d ^inv-2026-02\n"
        );
    }

    #[test]
    fn a_file_with_a_year_in_its_path_splits_the_output_by_year() {
        let prices = "2026-12-30 VTI 280.14 USD\n2027-01-02 VTI 301 USD\n2026-12-30 BND 71.2 USD\n";
        let written = merged(
            Sink::File("prices/{year}.ax"),
            prices,
            &[("prices/2026.ax", "12-30 VTI 280.14 USD\n")],
        )
        .unwrap();
        assert_eq!(
            written,
            [
                (
                    "prices/2026.ax".to_string(),
                    "12-30 VTI 280.14 USD\n12-30 BND 71.2 USD\n".to_string()
                ),
                (
                    "prices/2027.ax".to_string(),
                    "01-02 VTI 301 USD\n".to_string()
                ),
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
        assert_eq!(
            problems[0].message,
            "the output has a line that does not start with a date"
        );
    }

    #[test]
    fn param_rows_are_added_unless_their_key_is_there() {
        let file = "param cpi\n  2025 315.6\n  2026 320.9\n";
        let sink = Sink::Param {
            name: "cpi",
            path: "params.ax",
        };
        let written = merged(
            sink,
            "// the index\n2025 999\n2026-07 322.1\n\n",
            &[("params.ax", file)],
        )
        .unwrap();
        assert_eq!(
            written,
            [(
                "params.ax".to_string(),
                "param cpi\n  2025 315.6\n  2026 320.9\n  2026-07 322.1\n".to_string()
            )]
        );
        let problems = merged(sink, "soon 3\n", &[("params.ax", file)])
            .err()
            .unwrap();
        assert_eq!(
            problems[0].message,
            "the output has a row that does not start with a year or a date"
        );
        let problems = merged(
            Sink::Param {
                name: "gone",
                path: "params.ax",
            },
            "2026 1\n",
            &[("params.ax", file)],
        )
        .err()
        .unwrap();
        assert_eq!(
            problems[0].message,
            "`param gone` is not declared in params.ax"
        );
    }
}
