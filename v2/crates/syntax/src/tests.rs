//! Tests for the parts of the parser that are easy to get subtly wrong: how
//! words are classified, the shape of the expression arena, recovery, and the
//! wording of the most important diagnostics. The rest is covered by parsing a
//! realistic file end to end.

use axiom_core::{Day, Dec, Diagnostic, FileId, Span};

use crate::ast::*;
use crate::lex::{Lexer, Malformed, Tok};
use crate::parse;

const EXAMPLE: &str = r#"
base USD
use us/401k
relaxed
layout free

commodity USD : currency
  precision 2
  name "US dollar"

entity acme : employer
entity me : person
  born 1990-05-04
  lives us/ca/san-francisco from 2026-01-01

account assets/bank/checking : bank
  owner me
  holds USD, EUR
  opened 2020-01-01
account assets/retirement : 401k
  employer acme
account expenses/food
  budget 500 USD monthly

kind 401k : asset
  deferred
  has employer entity
  /// Elective deferrals are capped per calendar year.
  law deferral-limit
    on in
    when from is wages
    require total(in, year) <= limit[year] + catch-up

code trip-*
  on expenses/travel/* | bank

param limit
  2025 23_500 USD
  2026 24_500 USD
param ordinary
  2026 single 0 USD 10% | 12_400 USD 12% | 50_400 USD 22% | 105_700 USD 24%

/// A cap that costs money to break.
law early-withdrawal
  on gain
  when not to is 401k | ira
  let over = if owner.age >= 50y then catch-up[year] else empty
  require owner.age >= 59y6m else owe 10% * gain to irs as early-withdrawal-penalty "too early"
  count amount as wages
  warn balance >= empty "overdrawn"

law federal-tax
  each year
  owe progressive(ordinary[year, owner.filing], tally(wages)) to irs by date(year + 1, 4, 15) as federal-income-tax

// Paycheck: legs are targets.
2026-01-15 acme -> 5_200 USD
  /// Pre-tax deferral.
  retirement      800 USD #pretax
  taxes/federal   910 USD  // withheld
  checking        ...

2026-02-01 -> landlord 1_800 USD
  checking       1_000 USD
  savings        ...

2026-01-18 checking -> food 84.20 USD / trader-joes #groceries
2026-01-22 checking 2_000 USD -> brokerage 7 VTI
2026-01-22 checking -> brokerage 7 VTI @ 285.70 USD
2026-09-02 brokerage[fifo] 10 VTI -> checking 3_050 USD
2026-09-02 brokerage[#house, 2024, 2026-01..2026-06, 2026-01-22] all -> checking 52_000 USD
2026-02-01 checking -> plumber (350 USD) #check-1041 ! "waived"
2026-03-02 checking -> ? 40 USD
2026-03-02 checking -> cash ? USD
2026-03-03 -> checking 40 USD
  ? 40 USD
2026-01-01..2026-12-31 checking -> insurance 1_200 USD
2026-01-31 checking = 7_921.30 USD
2026-01-31 checking = 7_921.30 USD !
2026-12-31 visa = empty
2026-02-06 #check-1041 settled
2026-02-20 #check-1044 void
2026-03-04 #deposit-77 returned
2026-01-02 VTI 280.14 USD
2026-04-01 brokerage ->
  savings = 5_000 USD

every month on 1 checking -> landlord 2_400 USD until 2027-06
every 2w checking -> savings 100 USD
every year on 04-15 from 2026-01-01 checking -> irs 3_000 USD
every week on friday until 2027-01-01 checking -> cash 40 USD

sync prices/2026.ax
  run python3 fetch_prices.py --symbol VTI // not a comment
"#;

// ─── Helpers ────────────────────────────────────────────────────────────────

fn parse_clean(src: &str) -> File<'_> {
    let (file, diags) = parse(FileId(0), src);
    assert!(diags.is_empty(), "unexpected diagnostics:\n{}", render(src, &diags));
    file
}

/// The one error a source must produce, checking its code.
fn only_error(src: &str, code: &str) -> Diagnostic {
    let mut errors: Vec<Diagnostic> = parse(FileId(0), src).1.into_iter().filter(Diagnostic::is_error).collect();
    assert_eq!(errors.len(), 1, "expected exactly one error, got:\n{}", render(src, &errors));
    let error = errors.remove(0);
    assert_eq!(error.code, code, "{}", render(src, std::slice::from_ref(&error)));
    error
}

/// A plain rendering of diagnostics, enough to read them in a failing test.
fn render(src: &str, diags: &[Diagnostic]) -> String {
    let mut out = String::new();
    for diag in diags {
        out += &format!("{:?}[{}]: {}\n", diag.severity, diag.code, diag.message);
        for label in &diag.labels {
            out += &format!("  `{}` {}\n", &src[label.loc.range()], label.text);
        }
        for help in &diag.help {
            out += &format!("  help: {}\n", help.text);
        }
    }
    out
}

/// What a diagnostic's first fix replaces, and with what.
fn first_fix<'a>(src: &'a str, diag: &'a Diagnostic) -> (&'a str, &'a str) {
    let (loc, text) = diag.help.iter().find_map(|help| help.edit.as_ref()).expect("a fix");
    (&src[loc.range()], text)
}

fn tokens(src: &str) -> Vec<Tok<'_>> {
    let mut lexer = Lexer::new(src, FileId(0), 0, src.len());
    let mut all = Vec::new();
    loop {
        match lexer.next_token().tok {
            Tok::Eol => return all,
            tok => all.push(tok),
        }
    }
}

fn number(mantissa: i128, scale: u8) -> Tok<'static> {
    Tok::Number(Dec { mantissa, scale })
}

fn day(year: i32, month: u32, day: u32) -> Day {
    Day::from_ymd(year, month, day).expect("a real date")
}

// ─── Whole files ────────────────────────────────────────────────────────────

#[test]
fn a_realistic_file_parses_into_the_expected_shapes() {
    let file = parse_clean(EXAMPLE);
    assert_eq!(file.items.len(), 41);
    let txns: Vec<&Txn> = file
        .items
        .iter()
        .filter_map(|item| if let ItemKind::Txn(txn) = &item.kind { Some(txn) } else { None })
        .collect();
    let plans: Vec<&Plan> = file
        .items
        .iter()
        .filter_map(|item| if let ItemKind::Plan(plan) = &item.kind { Some(plan) } else { None })
        .collect();

    // A paycheck: one named side, legs for the other, the last taking the remainder.
    let paycheck = txns[0];
    assert!(paycheck.flow.from.place.is_some() && paycheck.flow.to.place.is_none());
    assert!(matches!(paycheck.flow.legs[2].amount, Quantity::Rest(_)));
    assert_eq!(paycheck.flow.legs[0].tail.codes[0].text, "pretax");
    assert_eq!(paycheck.flow.legs[0].doc.unwrap().lines().collect::<Vec<_>>(), ["Pre-tax deferral."]);

    // Selectors normalise to inclusive day ranges.
    let sale = txns[6];
    let selectors = &sale.flow.from.place.as_ref().unwrap().select;
    assert!(matches!(&selectors[0], Select::Code(code) if code.text == "house"));
    assert!(matches!(selectors[1], Select::Range(a, b, _) if (a, b) == (day(2024, 1, 1), day(2024, 12, 31))));
    assert!(matches!(selectors[2], Select::Range(a, b, _) if (a, b) == (day(2026, 1, 1), day(2026, 6, 30))));
    assert!(matches!(selectors[3], Select::Range(a, b, _) if (a, b) == (day(2026, 1, 22), day(2026, 1, 22))));
    assert!(matches!(sale.flow.from.amount, Some(Quantity::All(_))));

    // A pending amount, its code and its waiver; a spread.
    assert!(matches!(txns[7].flow.to.amount, Some(Quantity::Pending(_))));
    assert_eq!(txns[7].flow.tail.waive.unwrap().reason, Some("waived"));
    assert_eq!(txns[11].until, Some(day(2026, 12, 31)));

    // Plan bounds may come before the flow or after its tail.
    assert_eq!(
        (plans[0].every, plans[0].on, plans[0].until),
        (Span::months(1), Some(On::MonthDay(1)), Some(day(2027, 6, 30)))
    );
    assert_eq!(plans[1].every, Span::days(14));
    assert_eq!((plans[2].on, plans[2].from), (Some(On::YearDay { month: 4, day: 15 }), Some(day(2026, 1, 1))));
    assert_eq!(plans[3].on, Some(On::Weekday(4)));
}

#[test]
fn comments_docs_and_raw_text() {
    let file = parse_clean("/// One.\n/// Two.\n\nlaw x\n  on in\n");
    assert_eq!(file.items[0].doc.unwrap().lines().collect::<Vec<_>>(), ["One.", "Two."]);

    let file = parse_clean("sync prices/2026.ax // daily\n  run curl -s https://example.com/a // b\r\n");
    let ItemKind::Sync(sync) = &file.items[0].kind else { panic!("a sync") };
    assert_eq!((sync.file.text, sync.run.text), ("prices/2026.ax", "curl -s https://example.com/a // b"));

    let (_, diags) = parse(FileId(0), "2026-01-01 checking -> food 5 USD\n/// Nothing follows.\n");
    assert_eq!(diags[0].code, "unattached-doc");
}

// ─── Tokens ─────────────────────────────────────────────────────────────────

#[test]
fn digit_initial_words_are_classified_by_shape() {
    let cases: [(&str, Tok); 10] = [
        ("2026-01-15", Tok::Date(day(2026, 1, 15))),
        ("2026-01", Tok::Month(day(2026, 1, 1))),
        ("84.20", number(8420, 2)),
        ("24_500", number(24500, 0)),
        ("3.5%", Tok::Percent(Dec { mantissa: 35, scale: 1 })),
        ("59y6m", Tok::Span(Span::months(714))),
        ("2w", Tok::Span(Span::days(14))),
        ("401k", Tok::Name("401k")),
        ("529", number(529, 0)),
        ("04-15", Tok::Name("04-15")),
    ];
    for (src, expected) in cases {
        assert_eq!(tokens(src), [expected], "{src}");
    }
    let malformed = [
        ("2026-02-30", Malformed::Date),
        ("2026-13", Malformed::Date),
        ("2026-1-5", Malformed::LooseDate),
        ("50USD", Malformed::GluedAmount),
        ("84.20USD", Malformed::GluedAmount),
        ("1__000", Malformed::Number),
    ];
    for (src, expected) in malformed {
        assert_eq!(tokens(src), [Tok::Invalid(expected)], "{src}");
    }
}

#[test]
fn hyphens_and_slashes_join_names_but_arrows_and_comments_do_not() {
    assert_eq!(tokens("trader-joes"), [Tok::Name("trader-joes")]);
    assert_eq!(tokens("a - b"), [Tok::Name("a"), Tok::Minus, Tok::Name("b")]);
    assert_eq!(tokens("a->b"), [Tok::Name("a"), Tok::Arrow, Tok::Name("b")]);
    assert_eq!(tokens("assets/bank/*"), [Tok::Name("assets/bank/*")]);
    assert_eq!(tokens("BRK.B"), [Tok::Unit("BRK.B")]);
    assert_eq!(tokens("5 USD // note"), [number(5, 0), Tok::Unit("USD")]);
    assert_eq!(tokens("a//b"), [Tok::Name("a"), Tok::Slash, Tok::Slash, Tok::Name("b")]);
    assert_eq!(tokens("2026..2027"), [number(2026, 0), Tok::DotDot, number(2027, 0)]);
}

// ─── Expressions ────────────────────────────────────────────────────────────

/// The children of an expression node, in evaluation order.
fn children(kind: &ExprKind) -> Vec<ExprId> {
    match kind {
        ExprKind::Num(_)
        | ExprKind::Pct(_)
        | ExprKind::Amount(..)
        | ExprKind::Date(_)
        | ExprKind::Span(_)
        | ExprKind::Str(_)
        | ExprKind::Empty
        | ExprKind::Name(_)
        | ExprKind::Unit(_)
        | ExprKind::Code(_) => vec![],
        ExprKind::Field(base, _) => vec![*base],
        ExprKind::Index(base, keys) => [*base].into_iter().chain(keys.iter().copied()).collect(),
        ExprKind::Call(_, args) => args.to_vec(),
        ExprKind::Unary(_, operand) => vec![*operand],
        ExprKind::Binary(_, lhs, rhs) => vec![*lhs, *rhs],
        ExprKind::Is(lhs, alternatives) => [*lhs].into_iter().chain(alternatives.iter().copied()).collect(),
        ExprKind::If(condition, then, otherwise) => vec![*condition, *then, *otherwise],
        ExprKind::Schedule(brackets) => brackets.iter().flat_map(|&(threshold, rate)| [threshold, rate]).collect(),
    }
}

fn effect_roots(effect: &Effect) -> Vec<ExprId> {
    match effect {
        Effect::Owe { amount, due, .. } => [*amount].into_iter().chain(*due).collect(),
        Effect::Count { amount, .. } => vec![*amount],
    }
}

fn law_roots(law: &Law, out: &mut Vec<ExprId>) {
    if let Trigger::By(when) = law.trigger {
        out.push(when);
    }
    for step in &law.steps {
        match &step.kind {
            StepKind::When(e) | StepKind::Let(_, e) => out.push(*e),
            StepKind::Require { cond, otherwise, .. } => {
                out.push(*cond);
                out.extend(otherwise.iter().flat_map(effect_roots));
            }
            StepKind::Effect(effect) => out.extend(effect_roots(effect)),
        }
    }
}

/// Every expression an item, property, row or step owns.
fn roots(file: &File) -> Vec<ExprId> {
    let mut out = Vec::new();
    for item in &file.items {
        match &item.kind {
            ItemKind::Decl(decl) => {
                out.extend(decl.props.iter().flat_map(|prop| prop.args.iter().copied()));
                decl.laws.iter().for_each(|law| law_roots(law, &mut out));
            }
            ItemKind::Param(param) => out.extend(param.rows.iter().map(|row| row.value)),
            ItemKind::Law(law) => law_roots(law, &mut out),
            _ => {}
        }
    }
    out
}

/// Post-order with `first`: leaves start at themselves, other nodes at their
/// first child, and children are adjacent subtrees that end just before their
/// parent. With `whole`, the roots' subtrees must also be the entire arena, so
/// that no node is dead.
fn assert_post_order(file: &File, whole: bool) {
    let exprs = &file.exprs;
    for index in 0..exprs.len() {
        let id = ExprId(index as u32);
        let node = &exprs[id];
        let mut next = node.first.0;
        for child in children(&node.kind) {
            assert_eq!(exprs[child].first.0, next, "child {} of node {index} does not follow its sibling", child.0);
            next = child.0 + 1;
        }
        assert_eq!(next, id.0, "node {index}: its subtree does not end at it");
    }
    if whole {
        let covered: usize = roots(file).iter().map(|&root| exprs.subtree(root).len()).sum();
        assert_eq!(covered, exprs.len(), "some expression node belongs to no root");
    }
}

#[test]
fn expressions_are_post_order_with_first_nodes() {
    let file = parse_clean(EXAMPLE);
    assert!(file.exprs.len() > 60);
    assert_post_order(&file, true);
}

/// Whatever a damaged file parses to, its arena keeps its shape, and a file
/// that still parses without errors has no dead nodes.
#[test]
fn damaged_files_keep_the_arena_well_formed() {
    let mut state = 0x2545_F491_4F6C_DD1Du64;
    let mut random = |below: usize| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state % below as u64) as usize
    };
    for _ in 0..2_000 {
        let mut bytes = EXAMPLE.as_bytes().to_vec();
        for _ in 0..1 + random(4) {
            let at = random(bytes.len());
            match random(3) {
                0 => bytes[at] = b"-/\"(),:= \n%|"[random(12)],
                1 => drop(bytes.remove(at)),
                _ => bytes.insert(at, b" \n\t#"[random(4)]),
            }
        }
        let src = String::from_utf8_lossy(&bytes);
        let (file, diags) = parse(FileId(0), &src);
        assert_post_order(&file, !diags.iter().any(Diagnostic::is_error));
    }
}

/// A fully parenthesised rendering of an expression.
fn show(exprs: &Exprs, id: ExprId, src: &str) -> String {
    match &exprs[id].kind {
        ExprKind::Name(text) | ExprKind::Unit(text) => text.to_string(),
        ExprKind::Num(_) | ExprKind::Amount(..) => src[exprs[id].loc.range()].to_string(),
        ExprKind::Unary(UnOp::Neg, x) => format!("(-{})", show(exprs, *x, src)),
        ExprKind::Unary(UnOp::Not, x) => format!("(not {})", show(exprs, *x, src)),
        ExprKind::Binary(op, a, b) => format!("({} {} {})", show(exprs, *a, src), op.symbol(), show(exprs, *b, src)),
        ExprKind::Is(x, alts) => {
            let alts: Vec<String> = alts.iter().map(|&a| show(exprs, a, src)).collect();
            format!("({} is {})", show(exprs, *x, src), alts.join(" | "))
        }
        ExprKind::Field(base, name) => format!("{}.{}", show(exprs, *base, src), name.text),
        ExprKind::If(c, t, e) => {
            format!("(if {} then {} else {})", show(exprs, *c, src), show(exprs, *t, src), show(exprs, *e, src))
        }
        other => panic!("`show` does not handle {other:?}"),
    }
}

fn condition(src: &str) -> String {
    let source = format!("law l\n  always\n  when {src}\n");
    let file = parse_clean(&source);
    let ItemKind::Law(law) = &file.items[0].kind else { panic!("a law") };
    let StepKind::When(root) = law.steps[0].kind else { panic!("a when") };
    show(&file.exprs, root, &source)
}

#[test]
fn operators_bind_as_documented() {
    assert_eq!(condition("a or b and c"), "(a or (b and c))");
    assert_eq!(condition("not a == b and c"), "((not (a == b)) and c)");
    assert_eq!(condition("-a * b + c / d"), "(((-a) * b) + (c / d))");
    assert_eq!(condition("x is a | b | c or y"), "((x is a | b | c) or y)");
    assert_eq!(condition("a - b - c"), "((a - b) - c)");
    assert_eq!(condition("if a then b else c + d"), "(if a then b else (c + d))");
    assert_eq!(condition("owner.age >= 5 USD"), "(owner.age >= 5 USD)");
    assert_eq!(condition("x is 529"), "(x is 529)");
    only_error("law l\n  always\n  when a < b < c\n", "chained-comparison");
}

#[test]
fn absurd_nesting_is_an_error_not_a_crash() {
    let nested = |depth: usize| format!("law l\n  always\n  when {}x{}\n", "(".repeat(depth), ")".repeat(depth));
    parse_clean(&nested(50));
    only_error(&nested(50_000), "expression-too-deep");
}

// ─── Diagnostics ────────────────────────────────────────────────────────────

#[test]
fn mistakes_in_amounts_and_dates_come_with_fixes() {
    // (source, error code, what the first fix replaces, its replacement)
    let cases = [
        ("2026-01-18 checking -> food 0\n", "bare-zero", "0", "empty"),
        ("2026-01-18 checking -> food $50\n", "currency-symbol", "$50", "50 USD"),
        ("2026-01-18 checking -> food 50 usd\n", "lowercase-commodity", "usd", "USD"),
        ("2026-01-18 checking -> food 1,000 USD\n", "thousands-comma", ",", "_"),
        ("2026-01-18 checking -> food -20 USD\n", "negative-amount", "-", ""),
        ("2026-02-30 checking -> food 5 USD\n", "bad-date", "2026-02-30", "2026-02-28"),
        ("2026-1-5 checking -> food 5 USD\n", "bad-date", "2026-1-5", "2026-01-05"),
        ("acount assets/bank : bank\n", "unknown-keyword", "acount", "account"),
        ("2026-01-18 checking food 84.20 USD\n", "expected-arrow", "", "-> "),
    ];
    for (src, code, replaced, replacement) in cases {
        let error = only_error(src, code);
        assert_eq!(first_fix(src, &error), (replaced, replacement), "{src}");
    }
    let src = "2026-02-30 checking -> food 5 USD\n";
    let error = only_error(src, "bad-date");
    assert_eq!(error.message, "February 2026 has 28 days");
    assert_eq!(&src[error.anchor().unwrap().range()], "30");
}

#[test]
fn mistakes_in_structure_are_explained() {
    let src = "2026-01-15 acme -> checking 5_200 USD\n  retirement 800 USD\n";
    let error = only_error(src, "many-to-many");
    assert_eq!(&src[error.labels[0].loc.range()], "retirement 800 USD");
    assert!(error.help.iter().any(|help| help.text.contains("two transactions")));

    only_error("2026-01-15 checking -> 5_200 USD\n", "missing-legs");
    only_error("2026-01-15 acme -> 5 USD\n  a ...\n  b ...\n", "two-remainders");

    let src = "2026-01-15 acme -> 5_200 USD\n\tchecking ...\n";
    assert_eq!(first_fix(src, &only_error(src, "tab-indent")), ("\t", "  "));

    let src = "2026-01-15 acme -> 5_200 USD\n  retirement 800 USD\n   checking ...\n";
    assert_eq!(first_fix(src, &only_error(src, "unexpected-indent")), ("   ", "  "));
    only_error("law l\n    always\n  when a\n", "inconsistent-indent");
    only_error("use us\n  stray\n", "unexpected-indent");
    only_error("law l\n  require a\n", "missing-trigger");
}

#[test]
fn a_broken_item_is_skipped_and_every_error_is_reported() {
    let src = "\
2026-01-01 checking -> food 5 USD
2026-01-02 checking -> food 0
2026-01-03 checking -> food 6 USD
account assets/broken :
2026-01-04 checking -> food 7 USD
2026-01-05 checking -> food 8 usd
  stray leg
2026-01-06 checking -> food 9 USD
";
    let (file, diags) = parse(FileId(0), src);
    assert_eq!(
        diags.iter().map(|diag| diag.code).collect::<Vec<_>>(),
        ["bare-zero", "expected-kind", "lowercase-commodity"]
    );
    assert_eq!(file.items.len(), 4);

    // Inside one block every bad line is reported, and the item is dropped.
    let src = "law l\n  always\n  require\n  when\n  let x\n";
    let (file, diags) = parse(FileId(0), src);
    assert_eq!(diags.len(), 3, "{}", render(src, &diags));
    assert!(file.items.is_empty());
}
