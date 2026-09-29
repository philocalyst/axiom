use axiom_core::{Diagnostic, Disposition, FileId, Loc};

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

/// A system whose law's `require` is on line 12.
static LAW: &str = "\
// The 401k system.
// The 401k system.
// The 401k system.
// The 401k system.
// The 401k system.
// The 401k system.
// The 401k system.
/// Elective deferrals are capped per calendar year (IRC §402(g)).
law deferral-limit
  on in
  when from is wages
  require total(in, year) <= limit[year] + catch-up
";

#[test]
fn one_label_with_a_note() {
    let sources = Sources::in_memory(&[("journal/2026/01.ax", "2026-01-18 chekcing -> food 84.20 USD\n")], &[]);
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
"
    );
}

#[test]
fn several_labels_on_one_line_hang_right_to_left() {
    let sources = Sources::in_memory(&[("journal/2026/11.ax", JOURNAL)], &[]);
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
"
    );
}

#[test]
fn the_readers_file_comes_first_and_a_built_in_source_says_so() {
    static SYSTEMS: [(&str, &str); 1] = [("us/401k.ax", LAW)];
    let sources = Sources::in_memory(&[("journal/2026/11.ax", JOURNAL)], &SYSTEMS);
    // The law is what failed, so it anchors the diagnostic; the reader edits the journal.
    let diagnostic = Diagnostic::error("deferral-limit", "401k contributions would exceed the 2026 limit")
        .label(find(&sources, 1, "total(in, year)", 0), "25,300.00 USD")
        .context(find(&sources, 0, "retirement", 0), "a 401k (us/401k)")
        .context(find(&sources, 0, "2_600 USD", 0), "this contribution")
        .context(find(&sources, 1, "limit[year]", 0), "24,500.00 USD")
        .note("Elective deferrals are capped per calendar year (IRC §402(g)).")
        .help("at most 1,800.00 USD more can go in this year");
    assert_eq!(
        draw(&sources, &diagnostic),
        "\
error[deferral-limit]: 401k contributions would exceed the 2026 limit
   ╭─[journal/2026/11.ax:4:3]
   │
 3 │ 2026-11-14 acme -> 5_200 USD
 4 │   retirement   2_600 USD
   │   ─────┬────   ────┬────
   │        │           ╰── this contribution
   │        ╰── a 401k (us/401k)
   │
   ├─[us/401k.ax:12:11] (built in)
   │
12 │   require total(in, year) <= limit[year] + catch-up
   │           ───────┬───────    ─────┬─────
   │                  │                ╰── 24,500.00 USD
   │                  ╰── 25,300.00 USD
   │
   = note: Elective deferrals are capped per calendar year (IRC §402(g)).
   = help: at most 1,800.00 USD more can go in this year
"
    );
}

#[test]
fn a_fix_is_a_diff() {
    let sources = Sources::in_memory(&[("journal.ax", "// prices\n2026-01-18 chekcing -> food 84.20 USD\n")], &[]);
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
2 - 2026-01-18 chekcing -> food 84.20 USD
2 + 2026-01-18 checking -> food 84.20 USD
"
    );
}

#[test]
fn an_insertion_shows_only_what_it_adds_and_a_deletion_only_what_it_takes() {
    let sources = Sources::in_memory(
        &[("a.ax", "account assets/checking\n  opened 2026-03-01\n  closed 2026-01-31\n\nbase USD\n")],
        &[],
    );
    let after_first = Loc::new(FileId(0), 23, 23);
    let closed_line = find(&sources, 0, "  closed 2026-01-31", 0);
    let diagnostic = Diagnostic::error("edit", "edits")
        .label(find(&sources, 0, "opened 2026-03-01", 0), "first")
        .fix("say where its money lives", after_first, "\n  via assets/bank")
        .fix("drop it", closed_line, "");
    assert_eq!(
        draw(&sources, &diagnostic),
        "\
error[edit]: edits
  ╭─[a.ax:2:3]
  │
1 │ account assets/checking
2 │   opened 2026-03-01
  │   ────────┬────────
  │           ╰── first
  │
  = help: say where its money lives
2 +   via assets/bank
  = help: drop it
3 -   closed 2026-01-31
"
    );
}

#[test]
fn a_label_spanning_lines_is_marked_on_each_with_its_text_under_the_last() {
    let sources = Sources::in_memory(&[("journal.ax", JOURNAL)], &[]);
    let whole = find(&sources, 0, "2026-11-14 acme -> 5_200 USD\n  retirement   2_600 USD\n  checking     ...", 0);
    let diagnostic =
        Diagnostic::error("split", "the legs do not add up").label(whole, "the legs come to 2_600 USD, not 5_200 USD");
    assert_eq!(
        draw(&sources, &diagnostic),
        "\
error[split]: the legs do not add up
  ╭─[journal.ax:3:1]
  │
3 │ 2026-11-14 acme -> 5_200 USD
  │ ^^^^^^^^^^^^^^^^^^^^^^^^^^^^
4 │   retirement   2_600 USD
  │   ^^^^^^^^^^^^^^^^^^^^^^
5 │   checking     ...
  │   ────────┬───────
  │           ╰── the legs come to 2_600 USD, not 5_200 USD
"
    );
}

#[test]
fn a_span_is_trimmed_of_the_whitespace_around_it() {
    let sources = Sources::in_memory(&[("journal.ax", JOURNAL)], &[]);
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
fn distant_lines_are_separated_by_a_gap_and_a_tab_shows() {
    let sources = Sources::in_memory(&[("journal/2026/01.ax", TABBED)], &[]);
    let diagnostic = Diagnostic::error("assert", "assertion failed")
        .label(find(&sources, 0, "1_000 USD", 0), "expected here")
        .context(find(&sources, 0, "2_600 USD", 0), "past a tab");
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
9 │ ⇥   retirement   2_600 USD
  │                  ────┬────
  │                      ╰── past a tab
"
    );
}

#[test]
fn a_wide_line_is_cut_around_the_label() {
    let line = format!("{}(needle){}", "a".repeat(2_000_000), "b".repeat(2_000_000));
    let sources = Sources::in_memory(&[("wide.ax", &format!("{line}\n"))], &[]);
    let diagnostic = Diagnostic::error("wide", "wide").label(find(&sources, 0, "(needle)", 0), "here");
    let drawn = Renderer::new(&sources, Terminal::plain(60)).diagnostic(&diagnostic);
    assert!(drawn.len() < 500, "{} bytes", drawn.len());
    let shown = drawn.lines().find(|shown| shown.contains("(needle)")).expect("the label's line");
    assert!(shown.starts_with("1 │ …a") && shown.ends_with("b…"), "{shown}");
}

#[test]
fn diagnostics_come_in_reading_order_one_report_per_cause_and_are_counted() {
    let sources = Sources::in_memory(&[("a.ax", "one\ntwo\nthree\nfour\nfive\nsix\n"), ("b.ax", "seven\n")], &[]);
    let at = |diagnostic: Diagnostic, file: u16, word: &str| diagnostic.label(find(&sources, file, word, 0), "");
    let all = [
        at(Diagnostic::error("f", "in b"), 1, "seven"),
        Diagnostic::error("g", "nowhere"),
        at(Diagnostic::info("n", "a plain note"), 0, "six"),
        at(Diagnostic::warning("w", "late"), 0, "two"),
        at(Diagnostic::info("p", "costs 100.00 USD").disposed(Disposition::Priced), 0, "five"),
        at(Diagnostic::error("e", "early"), 0, "one"),
        at(Diagnostic::warning("w2", "accepted").disposed(Disposition::Waived), 0, "four"),
        at(Diagnostic::error("d", "again"), 0, "three"),
        at(Diagnostic::error("d", "again"), 0, "four"),
        at(Diagnostic::error("d", "again"), 0, "five"),
    ];
    let refs: Vec<&Diagnostic> = all.iter().collect();
    let (drawn, tally) = Renderer::new(&sources, Terminal::plain(100)).present(&refs, false);
    let headers: Vec<&str> = drawn.lines().filter(|line| line.contains("]: ")).collect();
    assert_eq!(
        headers,
        [
            "error[e]: early",
            "error[d]: again",
            "error[f]: in b",
            "error[g]: nowhere",
            "warning[w]: late",
            "warning[w2]: accepted",
            "note[p]: costs 100.00 USD",
            "note[n]: a plain note",
        ]
    );
    assert!(drawn.contains("= note: also at a.ax:4, a.ax:5\n"), "{drawn}");
    assert!(drawn.ends_with("\n\n"), "each diagnostic is followed by a blank line");

    assert_eq!(tally, Tally { errors: 6, warnings: 1, priced: 1, waived: 1 });
    let plain = Terminal::plain(100).painter;
    assert_eq!(
        tally.line().map(|line| line.render(plain)),
        Some("✗ 6 errors · 1 warning · 1 priced · 1 waived".into())
    );
    let priced_only = Tally { priced: 2, ..Tally::default() };
    assert_eq!(priced_only.line().map(|line| line.render(plain)), Some("2 priced".into()));
    assert!(Tally::default().line().is_none());
}

#[test]
fn a_flood_is_counted_not_drawn_unless_asked_for() {
    let text = "x\n".repeat(60);
    let sources = Sources::in_memory(&[("a.ax", &text)], &[]);
    let all: Vec<Diagnostic> = (0..60)
        .map(|line| {
            let start = 2 * line;
            Diagnostic::error(format!("e{line}"), "flood").label(Loc::new(FileId(0), start, start + 1), "")
        })
        .collect();
    let refs: Vec<&Diagnostic> = all.iter().collect();
    let renderer = Renderer::new(&sources, Terminal::plain(100));
    let (drawn, tally) = renderer.present(&refs, false);
    assert_eq!(drawn.matches("error[").count(), 50);
    assert!(drawn.contains("… 10 more diagnostics not shown (10 errors); `--all` shows every one"), "{drawn}");
    assert_eq!(tally.errors, 60);
    assert_eq!(renderer.present(&refs, true).0.matches("error[").count(), 60);
}

#[test]
fn colour_follows_severity() {
    let sources = Sources::in_memory(&[("a.ax", "abc\n")], &[]);
    let diagnostic = Diagnostic::warning("w", "careful").label(Loc::new(FileId(0), 0, 3), "here");
    let drawn = Renderer::new(&sources, Terminal::colored(100)).diagnostic(&diagnostic);
    assert!(drawn.starts_with("\x1b[1;33mwarning[w]\x1b[0m\x1b[1m: careful\x1b[0m\n"), "{drawn:?}");
    assert!(drawn.contains("\x1b[1;33m─┬─\x1b[0m"), "{drawn:?}");
    assert!(drawn.contains("\x1b[2m╭─[\x1b[0ma.ax:1:1\x1b[2m]\x1b[0m"), "{drawn:?}");
}

#[test]
fn odd_labels_never_panic() {
    let sources = Sources::in_memory(&[("a.ax", "one\ntwo\tthree\nlast"), ("empty.ax", "")], &[]);
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
