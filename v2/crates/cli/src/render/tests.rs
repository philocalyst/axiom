use axiom_core::{Diagnostic, FileId, Loc};

use super::*;
use crate::project::Sources;
use crate::style::Terminal;

/// The range of the `nth` (from 0) occurrence of `needle` in file `file`.
fn find(sources: &Sources, file: u16, needle: &str, nth: usize) -> Loc {
    let text = &sources.get(FileId(file)).expect("file exists").text;
    let start = text.match_indices(needle).nth(nth).expect("needle occurs").0;
    Loc::new(FileId(file), start as u32, (start + needle.len()) as u32)
}

fn draw(sources: &Sources, diagnostic: &Diagnostic) -> String {
    Renderer::new(sources, Terminal::plain(100)).diagnostic(diagnostic)
}

const JOURNAL: &str = "\
// November

2026-11-14 acme -> 5_200 USD
  retirement   2_600 USD
  checking     ...
";

/// A law whose `require` is on line 12.
fn law_source() -> String {
    let mut text = "// The 401k system.\n".repeat(7);
    text.push_str("/// Elective deferrals are capped per calendar year (IRC §402(g)).\n");
    text.push_str("law deferral-limit\n  on in\n  when from is wages\n");
    text.push_str("  require total(in, year) <= limit[year] + catch-up\n");
    text
}

#[test]
fn one_label_with_a_note() {
    let sources = Sources::in_memory(&[("journal/2026/01.ax", "2026-01-18 chekcing -> food 84.20 USD\n")]);
    let diagnostic = Diagnostic::error("unknown-place", "no place called `chekcing`")
        .label(find(&sources, 0, "chekcing", 0), "not declared")
        .note("A place is declared with `account`, or opened by writing its full path under a root such as `assets`.");
    assert_eq!(
        draw(&sources, &diagnostic),
        "\
error[unknown-place]: no place called `chekcing`
  ╭─[journal/2026/01.ax:1:12]
  │
1 │ 2026-01-18 chekcing -> food 84.20 USD
  │            ────┬───
  │                ╰── not declared
  │
  = note: A place is declared with `account`, or opened by writing its full path under a root such
          as `assets`.
──╯
"
    );
}

#[test]
fn several_labels_on_one_line_hang_right_to_left() {
    let sources = Sources::in_memory(&[("journal/2026/11.ax", JOURNAL)]);
    let diagnostic = Diagnostic::warning("budget", "over budget")
        .label(find(&sources, 0, "2_600 USD", 0), "this contribution")
        .context(find(&sources, 0, "retirement", 0), "a 401k")
        .context(find(&sources, 0, "2026-11-14", 0), "on this day");
    assert_eq!(
        draw(&sources, &diagnostic),
        "\
warning[budget]: over budget
  ╭─[journal/2026/11.ax:4:16]
  │
3 │ 2026-11-14 acme -> 5_200 USD
  │ ─────┬────
  │      ╰── on this day
4 │   retirement   2_600 USD
  │   ─────┬────   ────┬────
  │        │           ╰── this contribution
  │        ╰── a 401k
  │
──╯
"
    );
}

#[test]
fn labels_in_two_files() {
    let sources = Sources::in_memory(&[("journal/2026/11.ax", JOURNAL), ("us/401k.ax", &law_source())]);
    let diagnostic = Diagnostic::error("law", "401k contributions would exceed the 2026 limit")
        .label(find(&sources, 0, "retirement", 0), "a 401k (us/401k)")
        .label(find(&sources, 0, "2_600 USD", 0), "this contribution")
        .context(find(&sources, 1, "total(in, year)", 0), "25,300.00 USD")
        .context(find(&sources, 1, "limit[year]", 0), "24,500.00 USD")
        .context(find(&sources, 1, "catch-up", 0), "0.00 USD")
        .note("Elective deferrals are capped per calendar year (IRC §402(g)).")
        .help("at most 1,800.00 USD more can go in this year");
    assert_eq!(
        draw(&sources, &diagnostic),
        "\
error[law]: 401k contributions would exceed the 2026 limit
   ╭─[journal/2026/11.ax:4:3]
   │
 3 │ 2026-11-14 acme -> 5_200 USD
 4 │   retirement   2_600 USD
   │   ─────┬────   ────┬────
   │        │           ╰── this contribution
   │        ╰── a 401k (us/401k)
   │
   ├─[us/401k.ax:12:11]
   │
12 │   require total(in, year) <= limit[year] + catch-up
   │           ───────┬───────    ─────┬─────   ────┬───
   │                  │                │            ╰── 0.00 USD
   │                  │                ╰── 24,500.00 USD
   │                  ╰── 25,300.00 USD
   │
   = note: Elective deferrals are capped per calendar year (IRC §402(g)).
   = help: at most 1,800.00 USD more can go in this year
───╯
"
    );
}

#[test]
fn a_fix_shows_the_edited_line() {
    let sources = Sources::in_memory(&[("journal.ax", "// prices\n2026-01-18 chekcing -> food 84.20 USD\n")]);
    let place = find(&sources, 0, "chekcing", 0);
    let diagnostic = Diagnostic::error("unknown-place", "no place called `chekcing`").label(place, "not declared").fix(
        "did you mean `checking`?",
        place,
        "checking",
    );
    assert_eq!(
        draw(&sources, &diagnostic),
        "\
error[unknown-place]: no place called `chekcing`
  ╭─[journal.ax:2:12]
  │
2 │ 2026-01-18 chekcing -> food 84.20 USD
  │            ────┬───
  │                ╰── not declared
  │
  = help: did you mean `checking`?
2 + 2026-01-18 checking -> food 84.20 USD
──╯
"
    );
}

#[test]
fn a_label_spanning_lines_marks_both_ends() {
    let sources = Sources::in_memory(&[("journal.ax", JOURNAL)]);
    let whole = find(&sources, 0, "2026-11-14 acme -> 5_200 USD\n  retirement   2_600 USD\n  checking     ...", 0);
    let diagnostic =
        Diagnostic::error("split", "the legs do not add up").label(whole, "the legs come to 2_600 USD, not 5_200 USD");
    assert_eq!(
        draw(&sources, &diagnostic),
        "\
error[split]: the legs do not add up
  ╭─[journal.ax:3:1]
  │
3 │ ╭─▶ 2026-11-14 acme -> 5_200 USD
4 │ │     retirement   2_600 USD
5 │ ├─▶   checking     ...
  │ │
  │ ╰── the legs come to 2_600 USD, not 5_200 USD
  │
──╯
"
    );
}

#[test]
fn a_span_is_trimmed_of_the_whitespace_around_it() {
    let sources = Sources::in_memory(&[("journal.ax", JOURNAL)]);
    let leg = find(&sources, 0, "  retirement   2_600 USD\n", 0);
    let diagnostic =
        Diagnostic::error("leg", "the whole line, with its indentation and line ending").label(leg, "a leg");
    assert_eq!(
        draw(&sources, &diagnostic),
        "\
error[leg]: the whole line, with its indentation and line ending
  ╭─[journal.ax:4:3]
  │
3 │ 2026-11-14 acme -> 5_200 USD
4 │   retirement   2_600 USD
  │   ───────────┬──────────
  │              ╰── a leg
  │
──╯
"
    );
}

/// Lines 8 to 11 are one transaction, with a tab where its first leg is indented.
const TABBED: &str = "\
2026-01-01 checking = 1_000 USD
2026-01-02 checking -> food 4 USD
2026-01-03 checking -> food 5 USD
2026-01-04 checking -> food 6 USD
2026-01-05 checking -> food 7 USD
2026-01-06 checking -> food 8 USD
2026-01-07 checking -> food 9 USD
2026-01-15 acme -> 5_200 USD
\tretirement   2_600 USD
  taxes        910 USD
  checking     ...
";

#[test]
fn distant_lines_are_separated_by_a_gap() {
    let sources = Sources::in_memory(&[("journal/2026/01.ax", TABBED)]);
    let diagnostic = Diagnostic::error("assert", "assertion failed")
        .label(find(&sources, 0, "1_000 USD", 0), "expected here")
        .context(find(&sources, 0, "acme -> 5_200 USD", 0), "but this happened");
    assert_eq!(
        draw(&sources, &diagnostic),
        "\
error[assert]: assertion failed
  ╭─[journal/2026/01.ax:1:23]
  │
1 │ 2026-01-01 checking = 1_000 USD
  │                       ────┬────
  │                           ╰── expected here
  ⋮
8 │ 2026-01-15 acme -> 5_200 USD
  │            ────────┬────────
  │                    ╰── but this happened
  │
──╯
"
    );
}

#[test]
fn multi_line_labels_nest_and_tabs_line_up() {
    let sources = Sources::in_memory(&[("journal/2026/01.ax", TABBED)]);
    let whole = find(
        &sources,
        0,
        "2026-01-15 acme -> 5_200 USD\n\tretirement   2_600 USD\n  taxes        910 USD\n  checking     ...",
        0,
    );
    let legs = find(&sources, 0, "\tretirement   2_600 USD\n  taxes        910 USD", 0);
    let diagnostic = Diagnostic::warning("split", "nested")
        .label(whole, "the whole transaction")
        .context(legs, "the legs")
        .context(find(&sources, 0, "2_600 USD", 0), "past a tab");
    assert_eq!(
        draw(&sources, &diagnostic),
        "\
warning[split]: nested
   ╭─[journal/2026/01.ax:8:1]
   │
 8 │ ╭──▶ 2026-01-15 acme -> 5_200 USD
 9 │ │╭─▶     retirement   2_600 USD
   │ ││                    ────┬────
   │ ││                        ╰── past a tab
10 │ │├─▶   taxes        910 USD
   │ ││
   │ │╰── the legs
11 │ ├──▶   checking     ...
   │ │
   │ ╰─── the whole transaction
   │
───╯
"
    );
}

#[test]
fn diagnostics_come_in_file_and_source_order_and_are_counted() {
    let sources = Sources::in_memory(&[("a.ax", "one\ntwo\n"), ("b.ax", "three\n")]);
    let early = Diagnostic::error("e", "early").label(find(&sources, 0, "one", 0), "");
    let late = Diagnostic::warning("w", "late").label(find(&sources, 0, "two", 0), "");
    let other_file = Diagnostic::error("f", "in b").label(find(&sources, 1, "three", 0), "");
    let nowhere = Diagnostic::error("g", "nowhere");
    let all = [&other_file, &nowhere, &late, &early];

    let drawn = Renderer::new(&sources, Terminal::plain(100)).diagnostics(&all);
    let headers: Vec<&str> = drawn.lines().filter(|line| line.contains("]: ")).collect();
    assert_eq!(headers, ["error[e]: early", "warning[w]: late", "error[f]: in b", "error[g]: nowhere"]);
    assert!(drawn.ends_with("\n\n"), "each diagnostic is followed by a blank line");

    let plain = Terminal::plain(100).painter;
    let tally = Tally::of(&all);
    assert_eq!(tally, Tally { errors: 3, warnings: 1 });
    assert_eq!(tally.line().map(|line| line.render(plain)), Some("✗ 3 errors, 1 warning".to_string()));
    let warnings_only = Tally { errors: 0, warnings: 1 };
    assert_eq!(warnings_only.line().map(|line| line.render(plain)), Some("1 warning".to_string()));
    assert!(Tally::default().line().is_none());
}

#[test]
fn colour_follows_severity() {
    let sources = Sources::in_memory(&[("a.ax", "abc\n")]);
    let diagnostic = Diagnostic::warning("w", "careful").label(Loc::new(FileId(0), 0, 3), "here");
    let drawn = Renderer::new(&sources, Terminal::colored(100)).diagnostic(&diagnostic);
    assert!(drawn.starts_with("\x1b[1;33mwarning[w]\x1b[0m\x1b[1m: careful\x1b[0m\n"), "{drawn:?}");
    assert!(drawn.contains("\x1b[1;33m─┬─\x1b[0m"), "{drawn:?}");
    assert!(drawn.contains("\x1b[2m╭─[\x1b[0ma.ax:1:1\x1b[2m]\x1b[0m"), "{drawn:?}");
}

#[test]
fn odd_labels_never_panic() {
    let sources = Sources::in_memory(&[("a.ax", "one\ntwo\tthree\nlast"), ("empty.ax", "")]);
    let text_len = sources.get(FileId(0)).expect("file").text.len() as u32;
    let diagnostic = Diagnostic::error("odd", "odd labels")
        .label(Loc::new(FileId(0), text_len, text_len), "at the very end")
        .context(Loc::new(FileId(0), 6, 6), "empty, after a tab")
        .context(Loc::new(FileId(0), 5, 9999), "past the end")
        .context(Loc::new(FileId(0), 9, 2), "backwards")
        .context(Loc::new(FileId(1), 0, 0), "in an empty file")
        .context(Loc::new(FileId(7), 0, 3), "in a file nobody loaded")
        .fix("cut it", Loc::new(FileId(0), 3, 500), "")
        .fix("elsewhere", Loc::new(FileId(9), 0, 1), "x");
    let drawn = draw(&sources, &diagnostic);
    assert!(drawn.starts_with("error[odd]: odd labels\n"));
    assert!(drawn.contains("at the very end") && drawn.contains("in an empty file"));
    assert!(!drawn.contains("nobody loaded"));
}
