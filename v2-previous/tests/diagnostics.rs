use axiom_v2::{Diagnostic, IssueKind, Label, Source, Span, render_diagnostics};

fn span(source: &str, needle: &str, line: usize) -> Span {
    let start = source.find(needle).expect("span text exists");
    Span {
        start,
        end: start + needle.len(),
        line,
    }
}

#[test]
fn renders_compact_source_excerpt_with_precise_marker_and_help() {
    let text = "ledger sample\n  name \"x\"\n";
    let diagnostic = Diagnostic::new(2, "invalid field value")
        .at(0, span(text, "name", 2))
        .help("use a declared field name");

    assert_eq!(
        render_diagnostics(&[Source::new("ledger.axm", text)], &[diagnostic]),
        "error: invalid field value\n  --> ledger.axm:2:3\n   │\n 2 │   name \"x\"\n   │   ^~~~\n  = help: use a declared field name\n"
    );
}

#[test]
fn renders_related_locations_in_their_own_source_files() {
    let first = "package alpha\n  rule calculate\n";
    let second = "package beta\n  rule calculate\n";
    let diagnostic = Diagnostic::new(2, "conflicting rule definitions")
        .at(0, span(first, "rule calculate", 2))
        .related(Label::new(
            Some(1),
            span(second, "rule calculate", 2),
            "the other definition is here",
        ));

    let rendered = render_diagnostics(
        &[
            Source::new("alpha.axm", first),
            Source::new("beta.axm", second),
        ],
        &[diagnostic],
    );
    assert!(rendered.contains("  --> alpha.axm:2:3\n"), "{rendered}");
    let marker = format!("^{}", "~".repeat("rule calculate".len() - 1));
    assert!(rendered.contains(&marker), "{rendered}");
    assert!(
        rendered.contains("  = note: the other definition is here\n"),
        "{rendered}"
    );
    assert!(rendered.contains("  --> beta.axm:2:3\n"), "{rendered}");
}

#[test]
fn handles_crlf_tabs_utf8_and_terminal_controls_without_emitting_controls() {
    let text = "ledger café\r\n\tname value\u{1b}[31m\r\n";
    let start = text.find("name").expect("field exists");
    let diagnostic = Diagnostic::new(2, "bad\u{1b}[31m input")
        .at(
            0,
            Span {
                start,
                end: start + "name".len(),
                line: 2,
            },
        )
        .help("check the value\u{202e} carefully");
    let rendered = render_diagnostics(&[Source::new("file\u{1b}.axm", text)], &[diagnostic]);

    assert!(
        rendered.contains("error: bad\\u{1b}[31m input"),
        "{rendered}"
    );
    assert!(
        rendered.contains("  --> file\\u{1b}.axm:2:5\n"),
        "{rendered}"
    );
    assert!(
        rendered.contains(" 2 │     name value\\u{1b}[31m"),
        "{rendered}"
    );
    assert!(rendered.contains("\\u{202e}"), "{rendered}");
    assert!(!rendered.contains('\u{1b}'), "raw ESC leaked: {rendered:?}");
}

#[test]
fn invalid_or_out_of_bounds_spans_fall_back_to_a_safe_line_excerpt() {
    let text = "ledger\n  name value\n";
    let diagnostic = Diagnostic::new(2, "invalid source span").at(
        0,
        Span {
            start: text.len() + 100,
            end: text.len() + 120,
            line: 2,
        },
    );
    let rendered = render_diagnostics(&[Source::new("ledger.axm", text)], &[diagnostic]);

    assert!(rendered.contains("  --> ledger.axm:2\n"), "{rendered}");
    assert!(rendered.contains(" 2 |   name value\n"), "{rendered}");
    assert!(
        !rendered.contains('^'),
        "invalid span was underlined: {rendered}"
    );

    let unicode_text = "ledger\n  café value\n";
    let inside_accent = unicode_text.find('é').unwrap() + 1;
    let bad_boundary = Diagnostic::new(2, "split character").at(
        0,
        Span {
            start: inside_accent,
            end: inside_accent + 1,
            line: 2,
        },
    );
    let rendered = render_diagnostics(&[Source::new("ledger.axm", unicode_text)], &[bad_boundary]);
    assert!(
        !rendered.contains('^'),
        "invalid UTF-8 boundary was underlined: {rendered}"
    );
}

#[test]
fn long_lines_are_bounded_around_the_diagnostic_span() {
    let text = format!("{}needle{}\n", "x".repeat(700), "y".repeat(700));
    let diagnostic = Diagnostic::new(1, "bad token").at(0, span(&text, "needle", 1));
    let rendered = render_diagnostics(&[Source::new("large.axm", &text)], &[diagnostic]);
    let excerpt = rendered
        .lines()
        .find(|line| line.contains("needle"))
        .unwrap();

    assert!(excerpt.contains("…"), "{excerpt}");
    assert!(excerpt.contains("needle"), "{excerpt}");
    assert!(
        excerpt.chars().count() < 140,
        "excerpt was not bounded: {excerpt}"
    );
}

#[test]
fn caps_diagnostics_related_notes_and_untrusted_message_lengths() {
    let diagnostics: Vec<_> = (0..54)
        .map(|_| Diagnostic::new(1, "x".repeat(900)))
        .collect();
    let rendered = render_diagnostics(&[], &diagnostics);
    assert_eq!(rendered.matches("error: ").count(), 50);
    assert!(rendered.contains("= 4 additional diagnostics omitted\n"));
    assert!(!rendered.contains(&"x".repeat(513)));

    let mut with_related = Diagnostic::new(1, "conflict");
    for _ in 0..11 {
        with_related = with_related.related(Label::new(
            None,
            Span {
                start: 0,
                end: 0,
                line: 1,
            },
            "y".repeat(900),
        ));
    }
    let rendered = render_diagnostics(&[], &[with_related]);
    assert_eq!(rendered.matches("  = note: y").count(), 8);
    assert!(rendered.contains("  = note: 3 additional related locations omitted\n"));
    assert!(!rendered.contains(&"y".repeat(513)));
}

#[test]
fn unassigned_diagnostics_keep_their_line_without_guessing_a_source() {
    let diagnostic = Diagnostic::new(7, "source assignment is missing");
    let rendered = render_diagnostics(&[Source::new("one.axm", "line one\n")], &[diagnostic]);
    assert_eq!(
        rendered,
        "error: source assignment is missing\n  --> line 7\n"
    );
}

#[test]
fn semantic_issue_kinds_use_distinct_headings_and_legacy_payloads_default_to_error() {
    let kinds = [
        (IssueKind::Error, "error:"),
        (IssueKind::NeedsDecision, "needs a decision:"),
        (IssueKind::MissingInformation, "missing information:"),
        (IssueKind::Conflict, "conflict:"),
        (IssueKind::Incomplete, "cannot determine:"),
    ];
    for (kind, heading) in kinds {
        let diagnostic = Diagnostic::new(1, "subject has no proven result").kind(kind);
        let rendered = render_diagnostics(&[], &[diagnostic]);
        assert!(rendered.starts_with(heading), "{rendered}");
    }

    let legacy: Diagnostic = serde_json::from_str(r#"{"line":3,"message":"old"}"#).unwrap();
    assert_eq!(legacy.kind, IssueKind::Error);
    assert_eq!(legacy.message, "old");
}
