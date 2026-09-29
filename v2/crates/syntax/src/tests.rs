//! Tests for the parts of the parser that are easy to get subtly wrong: how
//! words are classified, the shape of the tree and its arenas, recovery, the
//! parallel parse, and the wording of the most important diagnostics. The rest
//! is covered by parsing a realistic file end to end.

use axiom_core::{Day, Dec, Diagnostic, FileId, Span};

use crate::ast::*;
use crate::lex::{Lexer, Malformed, Tok};
use crate::{parse, parse_in};

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
entity aldi, kroger : grocer
  via expenses/food

account assets/bank/checking as chk : bank
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
    on in from wages | bonus
    require total(in, year) <= limit[year] + catch-up

code trip-* meal-*
  on expenses/travel/* | bank
  on assets/cash

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

law fourth-payment
  each year closing 04-15
  count amount as estimated

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
2026-06-30 checking = -42.17 USD
2026-03-31 retirement = 24_600 USD via market
2026-12-31 visa = empty
2026-02-06 #check-1041 settled
2026-02-20 #check-1044 void
2026-03-04 #deposit-77 returned
2026-01-02 VTI 280.14 USD
2026-05-22 FAST split 2 for 1
2026-04-01 brokerage ->
  savings = 5_000 USD

2026-01-15 checking -> taxes/federal 3_000 USD for 2025
2026-03-01 design -> acme 4_800 USD #inv-12 due 30d
2026-04-02 acme -> checking 4_800 USD for #inv-12
2026-09-06 checking -> savings 100 USD for car-fund
2026-05-16 grandma -> college 3_000 USD basis 3_000 USD
2026-09-15 checking -> house.basis 14_200 USD / roofer
2026-09-16 checking -> house[#roof].basis 1_000 USD
2026-05-01 old-broker all VXUS -> new-broker
2026-12-29 house 1 HOME -> 431_500 USD
  closing-costs  25_000 USD
  mortgage       276_282.05 USD
  checking       ...

opening 2024-12-31
  checking      10_000 USD
  college       24_600 USD   basis 19_850 USD
  house         1 HOME       basis 540_000 USD   since 2023-06-15

2026-01-16 paycheck
2026-01-30 paycheck
  taxes/federal  950 USD
2026-03-13 paycheck 5_900 USD

every month on 1 checking -> landlord 2_400 USD until 2027-06
every 2w checking -> savings 100 USD
every year on 04-15 from 2026-01-01 checking -> irs 3_000 USD
every week on friday until 2027-01-01 checking -> cash 40 USD
plan paycheck every 2w from 2026-01-02 acme -> 5_200 USD
  retirement     800 USD
  checking       ...

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
        for note in &diag.notes {
            out += &format!("  note: {note}\n");
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
    let mut lexer = Lexer::new(src, FileId(0));
    lexer.load(0, src.len());
    let mut all = Vec::new();
    while !matches!(lexer.peek().tok, Tok::Eol) {
        all.push(lexer.bump().tok);
    }
    all
}

fn number(mantissa: i64, scale: u8) -> Tok<'static> {
    Tok::Number(Dec { mantissa, scale })
}

fn day(year: i32, month: u32, day: u32) -> Day {
    Day::from_ymd(year, month, day).expect("a real date")
}

/// The transactions of a file, in order.
fn txns<'f, 's>(file: &'f File<'s>) -> Vec<&'f Txn<'s>> {
    let kinds =
        file.items.iter().filter_map(|item| if let ItemKind::Txn(id) = item.kind { Some(&file[id]) } else { None });
    kinds.collect()
}

/// The clauses of a tail, as their kinds.
fn clauses<'f, 's>(file: &'f File<'s>, tail: &Tail<'s>) -> Vec<&'f ClauseKind<'s>> {
    file[tail.clauses].iter().map(|clause| &clause.kind).collect()
}

// ─── Whole files ────────────────────────────────────────────────────────────

#[test]
fn a_realistic_file_parses_into_the_expected_shapes() {
    let file = parse_clean(EXAMPLE);
    assert_eq!(file.items.len(), 62);
    let txns = txns(&file);

    // A paycheck: one named side, legs for the other, the last taking the remainder.
    let paycheck = txns[0];
    let legs = &file[paycheck.flow.legs];
    assert!(paycheck.flow.from.place.is_some() && paycheck.flow.to.place.is_none());
    assert!(matches!(legs[2].amount, Quantity::Rest));
    assert!(matches!(clauses(&file, &legs[0].tail)[..], [ClauseKind::Code(Code("#pretax"))]));
    assert_eq!(legs[0].doc.unwrap().lines().collect::<Vec<_>>(), ["Pre-tax deferral."]);

    // Selectors normalise to inclusive day ranges.
    let sale = txns[6];
    let place = sale.flow.from.place.as_ref().unwrap();
    let selectors = &file[place.select];
    assert!(matches!(&selectors[0], Select::Code(code) if code.name() == "house"));
    assert!(matches!(selectors[1], Select::Range(a, b, _) if (a, b) == (day(2024, 1, 1), day(2024, 12, 31))));
    assert!(matches!(selectors[2], Select::Range(a, b, _) if (a, b) == (day(2026, 1, 1), day(2026, 6, 30))));
    assert!(matches!(selectors[3], Select::Range(a, b, _) if (a, b) == (day(2026, 1, 22), day(2026, 1, 22))));
    assert!(matches!(sale.flow.from.amount, Some(Quantity::All(None))));

    // A pending amount, its code and its waiver.
    assert!(matches!(txns[7].flow.to.amount, Some(Quantity::Pending(_))));
    let kinds = clauses(&file, &txns[7].flow.tail);
    assert!(matches!(kinds[0], ClauseKind::Code(code) if code.name() == "check-1041"));
    assert!(matches!(kinds[1], ClauseKind::Waive(Waive { reason: Some("waived"), .. })));

    // A spread is the clause `for DATE..DATE`.
    let spread = clauses(&file, &txns[11].flow.tail);
    assert!(
        matches!(spread[..], [ClauseKind::For(For::Period(a, b))] if (*a, *b) == (day(2026, 1, 1), day(2026, 12, 31)))
    );
}

#[test]
fn plans_take_their_bounds_before_the_flow_or_after_its_tail() {
    let file = parse_clean(EXAMPLE);
    let plans: Vec<&Plan> = file.iter::<Plan>().collect();
    assert_eq!(
        (plans[0].every, plans[0].on, plans[0].until),
        (Span::months(1), Some(On::MonthDay(1)), Some(day(2027, 6, 30)))
    );
    assert_eq!(plans[1].every, Span::days(14));
    assert_eq!((plans[2].on, plans[2].from), (Some(On::YearDay { month: 4, day: 15 }), Some(day(2026, 1, 1))));
    assert_eq!(plans[3].on, Some(On::Weekday(4)));
    assert_eq!(plans[4].name.map(|name| name.0), Some("paycheck"));
    assert_eq!(file[plans[4].flow.legs].len(), 2);
}

#[test]
fn comments_docs_and_raw_text() {
    let file = parse_clean("/// One.\n/// Two.\n\nlaw x\n  on in\n");
    assert_eq!(file.items[0].doc.unwrap().lines().collect::<Vec<_>>(), ["One.", "Two."]);

    let file = parse_clean("sync prices/2026.ax // daily\n  run curl -s https://example.com/a // b\r\n");
    let ItemKind::Sync(id) = file.items[0].kind else { panic!("a sync") };
    assert_eq!((file[id].file.0, file[id].run.0), ("prices/2026.ax", "curl -s https://example.com/a // b"));

    let (_, diags) = parse(FileId(0), "2026-01-01 checking -> food 5 USD\n/// Nothing follows.\n");
    assert_eq!(diags[0].code, "unattached-doc");
}

#[test]
fn locations_come_from_where_a_slice_was_written() {
    let src = "2026-01-18 checking -> food 84.20 USD / trader-joes #groceries\n";
    let file = parse_clean(src);
    let txn = txns(&file)[0];
    let place = txn.flow.to.place.as_ref().unwrap();
    assert_eq!(&src[file.loc(&place.name).range()], "food");
    let Some(Quantity::Fixed(amount)) = txn.flow.to.amount else { panic!("an amount") };
    assert_eq!(&src[file.loc(&amount).range()], "84.20 USD");
    assert_eq!((amount.num(), amount.unit().map(|unit| unit.0)), (Dec { mantissa: 8420, scale: 2 }, Some("USD")));
    let Some(ClauseKind::Code(code)) = clauses(&file, &txn.flow.tail).first().copied() else { panic!("a code") };
    assert_eq!(&src[file.loc(&code).range()], "#groceries");
}

#[test]
fn amounts_read_back_from_their_text() {
    let read = |text| (Amount(text).num(), Amount(text).unit().map(|unit| unit.0));
    assert_eq!(read("24_500 USD"), (Dec { mantissa: 24500, scale: 0 }, Some("USD")));
    assert_eq!(read("-42.170 EUR"), (Dec { mantissa: -42170, scale: 3 }, Some("EUR")));
    assert_eq!(read("empty"), (Dec::ZERO, None));
}

// ─── The v3 surface ─────────────────────────────────────────────────────────

#[test]
fn tails_take_their_clauses_in_any_order() {
    let src = "2026-03-01 design -> acme 4_800 USD due 30d for 2025 / me #inv-12 basis empty @ 2 USD ! \"ok\"\n";
    let file = parse_clean(src);
    let tail = &txns(&file)[0].flow.tail;
    assert_eq!(tail.payee.map(|payee| payee.0), Some("me"));
    let kinds = clauses(&file, tail);
    assert!(matches!(kinds[0], ClauseKind::Due(Due::After(span)) if *span == Span::days(30)));
    assert!(matches!(kinds[1], ClauseKind::For(For::Period(a, b)) if (*a, *b) == (day(2025, 1, 1), day(2025, 12, 31))));
    assert!(matches!(kinds[2], ClauseKind::Code(_)));
    assert!(matches!(kinds[3], ClauseKind::Basis(amount) if amount.0 == "empty"));
    assert!(matches!(kinds[4], ClauseKind::Price(_)));
    assert!(matches!(kinds[5], ClauseKind::Waive(_)));

    let file = parse_clean("2026-01-01 a -> b 5 USD for #inv-1\n2026-01-01 a -> b 5 USD for car-fund\n");
    let whats: Vec<&ClauseKind> = txns(&file).iter().flat_map(|txn| clauses(&file, &txn.flow.tail)).collect();
    assert!(matches!(whats[0], ClauseKind::For(For::Code(_))) && matches!(whats[1], ClauseKind::For(For::Entity(_))));

    only_error("2026-01-01 a -> b 5 USD for 2025 for 2026\n", "duplicate-clause");
    only_error("2026-01-01..2026-12-31 a -> b 5 USD for 2025\n", "duplicate-clause");
    only_error("2026-01-01 a -> b 5 USD due tomorrow\n", "expected-due");
}

#[test]
fn ends_can_be_basis_ends_and_take_all_of_a_commodity() {
    let file =
        parse_clean("2026-09-16 checking -> house[#roof, fifo].basis 1_000 USD\n2026-05-01 old all VXUS -> new\n");
    let txns = txns(&file);
    let target = txns[0].flow.to.place.as_ref().unwrap();
    assert!(target.is_basis(&file) && file[target.select].len() == 3);
    assert!(!txns[0].flow.from.place.as_ref().unwrap().is_basis(&file));
    assert!(matches!(txns[1].flow.from.amount, Some(Quantity::All(Some(Name("VXUS"))))));
}

#[test]
fn a_header_may_state_both_amounts_while_naming_one_place() {
    let file = parse_clean("2026-12-29 house 1 HOME -> 431_500 USD\n  closing 25_000 USD\n  checking ...\n");
    let flow = &txns(&file)[0].flow;
    assert!(flow.from.place.is_some() && flow.from.amount.is_some());
    assert!(flow.to.place.is_none() && matches!(flow.to.amount, Some(Quantity::Fixed(_))));
    assert_eq!(file[flow.legs].len(), 2);
}

#[test]
fn a_dated_name_is_a_plan_occurrence_unless_it_asserts_or_flows() {
    let file = parse_clean(
        "2026-01-16 paycheck\n2026-03-13 paycheck 5_900 USD\n  taxes 950 USD\n2026-01-31 paycheck = 5 USD\n",
    );
    let ItemKind::Occurrence(first) = file.items[0].kind else { panic!("an occurrence") };
    let ItemKind::Occurrence(second) = file.items[1].kind else { panic!("an occurrence") };
    assert_eq!((file[first].plan.0, file[first].amount, file[first].legs.len()), ("paycheck", None, 0));
    assert_eq!((file[second].amount.map(|amount| amount.0), file[second].legs.len()), (Some("5_900 USD"), 1));
    assert!(matches!(file.items[2].kind, ItemKind::Assert(_)));
}

#[test]
fn assertions_may_be_negative_and_may_say_where_a_gap_goes() {
    let file = parse_clean(
        "2026-06-30 checking = -42.17 USD\n2026-03-31 retirement = 24_600 USD via market\n2026-01-31 a = 1 USD ! \"x\"\n",
    );
    let asserts: Vec<&Assert> = file.iter().collect();
    assert_eq!(asserts[0].amount.num(), Dec { mantissa: -4217, scale: 2 });
    assert!(matches!(asserts[0].gap, Gap::Refused));
    assert!(matches!(asserts[1].gap, Gap::Via(Name("market"))));
    assert!(matches!(asserts[2].gap, Gap::Waived(Waive { reason: Some("x"), .. })));
}

#[test]
fn splits_and_openings() {
    let file = parse_clean(EXAMPLE);
    let split: &Split = file.iter().next().unwrap();
    assert_eq!((split.unit.0, split.numerator.mantissa, split.denominator.mantissa), ("FAST", 2, 1));
    let opening: &Opening = file.iter().next().unwrap();
    let lines = &file[opening.lines];
    assert_eq!((opening.date, lines.len()), (day(2024, 12, 31), 3));
    let kinds = clauses(&file, &lines[2].tail);
    assert!(
        matches!(kinds[0], ClauseKind::Basis(_)) && matches!(kinds[1], ClauseKind::Since(d) if *d == day(2023, 6, 15))
    );
    only_error("opening 2024-12-31\n  checking ...\n", "opening-amount");
    only_error("2026-01-01 a -> b 5 USD since 2025-01-01\n", "expected-end-of-line");
    only_error("2026-01-01 FAST split 0 for 1\n", "bad-split");
}

#[test]
fn declarations_take_aliases_lists_and_several_globs() {
    let file = parse_clean(EXAMPLE);
    let decls: Vec<&Decl> = file.iter::<Decl>().collect();
    let names: Vec<&str> = decls.iter().map(|decl| decl.name.0).collect();
    assert_eq!(&names[..6], ["USD", "acme", "me", "aldi", "kroger", "assets/bank/checking"]);
    assert_eq!(decls[5].alias.map(|alias| alias.0), Some("chk"));
    // Entities on one line share what is written under them.
    assert_eq!(format!("{:?}", decls[3].props), format!("{:?}", decls[4].props));
    assert_eq!(decls[3].kind.map(|kind| kind.0), Some("grocer"));
    assert_eq!(file[decls[3].props].len(), 1);

    let rules: Vec<&CodeRule> = file.iter::<CodeRule>().collect();
    assert_eq!((rules[0].pattern.0, rules[1].pattern.0), ("trip-*", "meal-*"));
    let on: Vec<&str> = file[rules[0].on].iter().map(|glob| glob.0).collect();
    assert_eq!(on, ["expenses/travel/*", "bank", "assets/cash"]);
    assert_eq!(format!("{:?}", rules[0].on), format!("{:?}", rules[1].on));
}

#[test]
fn laws_take_closing_days_and_desugar_their_sources() {
    let file = parse_clean(EXAMPLE);
    let laws: Vec<&Law> = file.iter::<Law>().collect();
    assert_eq!(laws[0].name.0, "deferral-limit");
    assert_eq!(laws[0].trigger, Trigger::In);
    let steps = &file[laws[0].steps];
    let StepKind::When(filter) = steps[0].kind else { panic!("a first step that filters") };
    let ExprKind::Is(subject, alternatives) = file.exprs[filter].kind else { panic!("`from is …`") };
    assert!(matches!(file.exprs[subject].kind, ExprKind::Name(Name("from"))));
    let names: Vec<&str> = file[alternatives].iter().map(|&alt| &EXAMPLE[file.exprs[alt].loc.range()]).collect();
    assert_eq!(names, ["wages", "bonus"]);
    assert!(matches!(steps[1].kind, StepKind::Require { .. }));
    assert_eq!(file.exprs.subtree(filter).len(), 4);
    let closing = laws.iter().find(|law| law.name.0 == "fourth-payment").unwrap();
    assert_eq!(closing.trigger, Trigger::Closing { month: 4, day: 15 });
    only_error("law l\n  each year closing 13-45\n", "bad-day");
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
        ("01/15/2026", Malformed::SlashDate),
        ("2026/1/5", Malformed::SlashDate),
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
    assert_eq!(tokens("a - b"), [Tok::Name("a"), Tok::Punct("-"), Tok::Name("b")]);
    assert_eq!(tokens("a->b"), [Tok::Name("a"), Tok::Punct("->"), Tok::Name("b")]);
    assert_eq!(tokens("assets/bank/*"), [Tok::Name("assets/bank/*")]);
    assert_eq!(tokens("BRK.B"), [Tok::Unit("BRK.B")]);
    assert_eq!(tokens("5 USD // note"), [number(5, 0), Tok::Unit("USD")]);
    assert_eq!(tokens("a//b"), [Tok::Name("a"), Tok::Punct("/"), Tok::Punct("/"), Tok::Name("b")]);
    assert_eq!(tokens("a/ b"), [Tok::Name("a"), Tok::Punct("/"), Tok::Name("b")]);
    assert_eq!(tokens("a/B"), [Tok::Invalid(Malformed::Word)]);
    assert_eq!(tokens("2026..2027"), [number(2026, 0), Tok::Punct(".."), number(2027, 0)]);
    assert_eq!(tokens("=> →"), [Tok::Punct("->"), Tok::Punct("->")]);
    assert_eq!(tokens("#check-1041"), [Tok::Code(Code("#check-1041"))]);
}

/// The eight-bytes-at-a-time scan finds the same word ends as a byte at a time.
#[test]
fn names_scan_like_a_byte_at_a_time() {
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    let alphabet = "ab0-_*/ AZ?.>#é";
    let alphabet: Vec<char> = alphabet.chars().collect();
    for _ in 0..20_000 {
        let len = 1 + (state % 40) as usize;
        let text: String = (0..len)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                alphabet[(state % alphabet.len() as u64) as usize]
            })
            .collect();
        let expected = text.bytes().take_while(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'*' | b'-')).count();
        assert_eq!(crate::lex::name_run(text.as_bytes()), expected, "{text:?}");
    }
}

// ─── Expressions ────────────────────────────────────────────────────────────

/// The children of an expression node, in evaluation order.
fn children(file: &File, kind: &ExprKind) -> Vec<ExprId> {
    match kind {
        ExprKind::Num(_)
        | ExprKind::Pct(_)
        | ExprKind::Amount(_)
        | ExprKind::Date(_)
        | ExprKind::Span(_)
        | ExprKind::Str(_)
        | ExprKind::Empty
        | ExprKind::Name(_)
        | ExprKind::Unit(_)
        | ExprKind::Code(_) => vec![],
        ExprKind::Field(base, _) | ExprKind::Unary(_, base) => vec![*base],
        ExprKind::Index(base, keys) => [*base].into_iter().chain(file[*keys].iter().copied()).collect(),
        ExprKind::Call(_, args) => file[*args].to_vec(),
        ExprKind::Binary(_, lhs, rhs) => vec![*lhs, *rhs],
        ExprKind::Is(lhs, alternatives) => [*lhs].into_iter().chain(file[*alternatives].iter().copied()).collect(),
        ExprKind::If(condition, then, otherwise) => vec![*condition, *then, *otherwise],
        ExprKind::Schedule(rows) => file[*rows].iter().flat_map(|row| [row.threshold, row.rate]).collect(),
    }
}

fn effect_roots(effect: &Effect) -> Vec<ExprId> {
    match effect {
        Effect::Owe { amount, due, .. } => [*amount].into_iter().chain(*due).collect(),
        Effect::Count { amount, .. } => vec![*amount],
    }
}

fn law_roots(file: &File, law: &Law, out: &mut Vec<ExprId>) {
    if let Trigger::By(when) = law.trigger {
        out.push(when);
    }
    for step in &file[law.steps] {
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
    for decl in file.iter::<Decl>() {
        out.extend(file[decl.props].iter().flat_map(|prop| file[prop.args].iter().copied()));
    }
    for param in file.iter::<Param>() {
        out.extend(file[param.rows].iter().map(|row| row.value));
    }
    file.iter::<Law>().for_each(|law| law_roots(file, law, &mut out));
    // Entities declared on one line share their properties.
    out.sort();
    out.dedup();
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
        for child in children(file, &node.kind) {
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

/// Sources damaged by a few changed bytes: `EXAMPLE`, again and again.
fn damaged(count: usize, mut with: impl FnMut(&str)) {
    let mut state = 0x2545_F491_4F6C_DD1Du64;
    let mut random = |below: usize| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state % below as u64) as usize
    };
    for _ in 0..count {
        let mut bytes = EXAMPLE.as_bytes().to_vec();
        for _ in 0..1 + random(4) {
            let at = random(bytes.len());
            match random(3) {
                0 => bytes[at] = b"-/\"(),:= \n%|"[random(12)],
                1 => drop(bytes.remove(at)),
                _ => bytes.insert(at, b" \n\t#"[random(4)]),
            }
        }
        with(&String::from_utf8_lossy(&bytes));
    }
}

/// Whatever a damaged file parses to, its arena keeps its shape, and a file
/// that still parses without errors has no dead nodes.
#[test]
fn damaged_files_keep_the_arena_well_formed() {
    damaged(2_000, |src| {
        let (file, diags) = parse(FileId(0), src);
        assert_post_order(&file, !diags.iter().any(Diagnostic::is_error));
    });
}

/// A fully parenthesised rendering of an expression.
fn show(file: &File, id: ExprId, src: &str) -> String {
    let exprs = &file.exprs;
    match &exprs[id].kind {
        ExprKind::Name(text) | ExprKind::Unit(text) => text.0.to_string(),
        ExprKind::Num(_) | ExprKind::Amount(_) => src[exprs[id].loc.range()].to_string(),
        ExprKind::Unary(UnOp::Neg, x) => format!("(-{})", show(file, *x, src)),
        ExprKind::Unary(UnOp::Not, x) => format!("(not {})", show(file, *x, src)),
        ExprKind::Binary(op, a, b) => format!("({} {} {})", show(file, *a, src), op.symbol(), show(file, *b, src)),
        ExprKind::Is(x, alts) => {
            let alts: Vec<String> = file[*alts].iter().map(|&a| show(file, a, src)).collect();
            format!("({} is {})", show(file, *x, src), alts.join(" | "))
        }
        ExprKind::Field(base, name) => format!("{}.{}", show(file, *base, src), name.0),
        ExprKind::If(c, t, e) => {
            format!("(if {} then {} else {})", show(file, *c, src), show(file, *t, src), show(file, *e, src))
        }
        other => panic!("`show` does not handle {other:?}"),
    }
}

fn condition(src: &str) -> String {
    let source = format!("law l\n  always\n  when {src}\n");
    let file = parse_clean(&source);
    let StepKind::When(root) = file[file.iter::<Law>().next().unwrap().steps][0].kind else { panic!("a when") };
    show(&file, root, &source)
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
        ("2026-01-18 checking -> food 1,000 USD\n", "thousands-comma", "1,000", "1_000"),
        ("2026-01-18 checking -> food 1,234.56 USD\n", "thousands-comma", "1,234.56", "1_234.56"),
        ("2026-01-18 checking -> food 1.234,56 USD\n", "european-number", "1.234,56", "1_234.56"),
        ("2026-01-18 checking -> food 12,5 USD\n", "european-number", "12,5", "12.5"),
        ("2026-01-18 checking -> food 1.234.567,89 USD\n", "european-number", "1.234.567,89", "1_234_567.89"),
        ("2026-02-30 checking -> food 5 USD\n", "bad-date", "2026-02-30", "2026-02-28"),
        ("2026-1-5 checking -> food 5 USD\n", "bad-date", "2026-1-5", "2026-01-05"),
        ("01/15/2026 checking -> food 5 USD\n", "bad-date", "01/15/2026", "2026-01-15"),
        ("15/01/2026 checking -> food 5 USD\n", "bad-date", "15/01/2026", "2026-01-15"),
        ("2026/1/15 checking -> food 5 USD\n", "bad-date", "2026/1/15", "2026-01-15"),
        ("acount assets/bank : bank\n", "unknown-keyword", "acount", "account"),
        ("2026-01-18 checking food 84.20 USD\n", "expected-arrow", "", "-> "),
        ("2026-01-18 checking => food 84.20 USD\n", "unknown-arrow", "=>", "->"),
        ("2026-01-18 checking → food 84.20 USD\n", "unknown-arrow", "→", "->"),
        ("2026-01-18 checking -> food 5 USD ! \"cash\n", "unterminated-string", "", "\""),
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
fn a_regrouped_number_is_never_another_amount() {
    for (written, spelled) in [("1,234.56", "1_234.56"), ("1.234,56", "1_234.56"), ("12,5", "12.5"), ("1,234", "1_234")]
    {
        let src = format!("2026-01-18 a -> b {written} USD\n");
        let error = parse(FileId(0), &src).1.remove(0);
        let (_, fix) = first_fix(&src, &error);
        assert_eq!(fix, spelled);
    }
    // `1,234` is a thousand and more; `1.234` is what the other reading would be.
    let src = "2026-01-18 a -> b 1,234 USD\n";
    assert!(only_error(src, "thousands-comma").notes[0].contains("1.234"));
}

#[test]
fn a_slash_date_is_never_guessed_between_two_readings() {
    let src = "01/02/2026 checking -> food 5 USD\n";
    let error = only_error(src, "bad-date");
    assert!(error.help.iter().all(|help| help.edit.is_none()));
    assert!(error.notes[0].contains("never guesses"));
    only_error("03/03/2026 a -> b 5 USD\n", "bad-date");
    let src = "31/02/2026 a -> b 5 USD\n";
    assert!(only_error(src, "bad-date").help.is_empty());
}

#[test]
fn a_number_without_a_commodity_asks_which_and_lists_the_files() {
    let src = "2026-01-01 a -> b 5 EUR\n2026-01-02 a -> b 6 USD\n2026-01-03 a -> b 7 USD\n2026-01-18 checking -> food 84.20\n";
    let error = only_error(src, "expected-commodity");
    assert_eq!(error.message, "`84.20` has no commodity");
    assert!(error.notes[0].ends_with("EUR, USD"), "{:?}", error.notes);
    assert_eq!(first_fix(src, &error), ("", " EUR"));
    // With nothing to go by, it still says how an amount is written.
    let error = only_error("2026-01-18 checking -> food 84.20\n", "expected-commodity");
    assert!(error.help[0].text.contains("84.20 USD"));
}

#[test]
fn mistakes_in_structure_are_explained() {
    let src = "2026-01-15 acme -> checking 5_200 USD\n  retirement 800 USD\n";
    let error = only_error(src, "many-to-many");
    assert_eq!(&src[error.labels[0].loc.range()], "retirement 800 USD");
    assert!(error.help.iter().any(|help| help.text.contains("two transactions")));

    only_error("2026-01-15 checking -> 5_200 USD\n", "missing-legs");
    only_error("2026-01-15 acme -> 5 USD\n  a ...\n  b ...\n", "two-remainders");

    let src = "2026-01-15 acme -> 5_200 USD\n  retirement 800 USD\n   checking ...\n";
    assert_eq!(first_fix(src, &only_error(src, "unexpected-indent")), ("   ", "  "));
    only_error("law l\n    always\n  when a\n", "inconsistent-indent");
    only_error("use us\n  stray\n", "unexpected-indent");
    only_error("law l\n  require a\n", "missing-trigger");
    only_error("law l\n  always\n  when (a + b))\n", "unbalanced-delimiter");
    let src = "2026-12-31..2026-01-01 checking -> insurance 600 USD\n";
    assert_eq!(first_fix(src, &only_error(src, "empty-range")), ("2026-12-31..2026-01-01", "2026-01-01..2026-12-31"));
}

#[test]
fn tabs_are_reported_once_for_the_file_with_their_count() {
    let src = "2026-01-15 acme -> 5_200 USD\n\tchecking 100 USD\n\tsavings ...\n2026-01-16 acme -> 5_200 USD\n\t\tchecking ...\n";
    let (file, diags) = parse(FileId(0), src);
    assert_eq!(diags.len(), 1, "{}", render(src, &diags));
    assert_eq!(diags[0].message, "3 lines are indented with a tab");
    assert_eq!(first_fix(src, &diags[0]), ("\t", "  "));
    assert_eq!(file.items.len(), 2, "the tabbed lines still parse, at two columns each");
    let (_, diags) = parse(FileId(0), "2026-01-15 acme -> 5_200 USD\n\tchecking ...\n");
    assert_eq!(diags[0].message, "a line is indented with a tab");
}

#[test]
fn a_very_long_word_is_not_copied_into_its_diagnostic() {
    let word = "a".repeat(100_000) + "B";
    let src = format!("2026-01-18 {word} -> food 5 USD\n");
    let error = only_error(&src, "mixed-case");
    assert!(error.message.len() < 200 && error.help.iter().all(|help| help.edit.is_none()));
    let src = format!("2026-01-18 checking -> food {}USD\n", "9".repeat(100_000));
    assert!(only_error(&src, "glued-amount").message.len() < 200);
    let src = format!("2026-01-18 checking -> food ${}\n", "9".repeat(100_000));
    let error = only_error(&src, "currency-symbol");
    assert!(error.message.len() < 200 && error.help.is_empty());
}

// ─── Recovery ───────────────────────────────────────────────────────────────

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
        diags.iter().map(|diag| &*diag.code).collect::<Vec<_>>(),
        ["bare-zero", "expected-kind", "lowercase-commodity"]
    );
    // The account is kept without its kind, so its uses are not errors too.
    assert_eq!((file.items.len(), file.iter::<Decl>().count()), (5, 1));

    // A law keeps its good steps.
    let src = "law l\n  always\n  require\n  when x\n  let x\n";
    let (file, diags) = parse(FileId(0), src);
    assert_eq!(diags.len(), 2, "{}", render(src, &diags));
    assert_eq!(file[file.iter::<Law>().next().unwrap().steps].len(), 1);
}

#[test]
fn a_declaration_keeps_its_good_lines_when_one_is_bad() {
    let src = "\
account expenses/food : expense
  owner me
  budget (
  2026 single
  opened 2020-01-01
  law monthly-cap
    on gian
    warn total(in, month) <= 650 USD
  law fine
    on in
    warn total(in, month) <= 650 USD
2026-01-01 checking -> food 5 USD
";
    let (file, diags) = parse(FileId(0), src);
    let codes: Vec<&str> = diags.iter().map(|diag| &*diag.code).collect();
    assert_eq!(codes, ["expected-expression", "expected-property", "unknown-trigger"], "{}", render(src, &diags));
    let ItemKind::Decl(id) = file.items[0].kind else { panic!("the declaration survives") };
    let props: Vec<&str> = file[file[id].props].iter().map(|prop| prop.name.0).collect();
    assert_eq!(props, ["owner", "opened"]);
    assert_eq!(file[file[id].laws].iter().map(|law| law.name.0).collect::<Vec<_>>(), ["fine"]);
    assert_eq!(file.items.len(), 2);

    // Junk after a header, and a typo in the keyword, keep the declaration too.
    let (file, diags) = parse(FileId(0), "account a : bank junk\nacount b\n");
    assert_eq!((file.iter::<Decl>().count(), diags.len()), (2, 2));
}

// ─── Size and the parallel parse ────────────────────────────────────────────

#[test]
fn the_tree_is_compact() {
    use std::mem::size_of;
    eprintln!(
        "Item {} Txn {} Flow {} End {} Place {} Quantity {} Tail {} Leg {} Clause {} Select {} Assert {} Event {} Price {} Decl {} Law {} Step {} Expr {} Plan {} Occurrence {} Prop {}",
        size_of::<Item>(),
        size_of::<Txn>(),
        size_of::<Flow>(),
        size_of::<End>(),
        size_of::<Place>(),
        size_of::<Quantity>(),
        size_of::<Tail>(),
        size_of::<Leg>(),
        size_of::<Clause>(),
        size_of::<Select>(),
        size_of::<Assert>(),
        size_of::<Event>(),
        size_of::<Price>(),
        size_of::<Decl>(),
        size_of::<Law>(),
        size_of::<Step>(),
        size_of::<Expr>(),
        size_of::<Plan>(),
        size_of::<Occurrence>(),
        size_of::<Prop>()
    );
    assert!(size_of::<Item>() <= 48, "Item is {}", size_of::<Item>());
    assert!(size_of::<Txn>() <= 160, "Txn is {}", size_of::<Txn>());
    assert!(size_of::<Leg>() <= 128, "Leg is {}", size_of::<Leg>());
    assert!(size_of::<Expr>() <= 48, "Expr is {}", size_of::<Expr>());
    assert!(size_of::<Many<Leg>>() == 8 && size_of::<Amount>() == 16 && size_of::<Name>() == 16);
}

/// Everything an item reaches, written out with every range and id followed,
/// so that two files are equal when they say the same, whichever pieces their
/// tables are kept in.
fn dump(file: &File) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let place = |place: &Place| format!("{:?}{:?}", place.name, &file[place.select]);
    let tail = |tail: &Tail| format!("{:?}{:?}", tail.payee, &file[tail.clauses]);
    let quantity = |end: &End| format!("{:?}", end.amount);
    let legs = |legs: Many<Leg>| {
        let each = |leg: &Leg| {
            format!("{:?} {} {:?} {} {:?}", leg.doc, place(&leg.place), leg.amount, tail(&leg.tail), leg.loc)
        };
        file[legs].iter().map(each).collect::<Vec<_>>().join("; ")
    };
    let flow = |flow: &Flow| {
        let end = |end: &End| format!("{:?} {}", end.place.as_ref().map(place), quantity(end));
        format!("{} -> {} {} [{}]", end(&flow.from), end(&flow.to), tail(&flow.tail), legs(flow.legs))
    };
    let expr = |root: ExprId| {
        let nodes = file.exprs.subtree(root);
        let leaves = nodes.iter().map(|node| match children(file, &node.kind).is_empty() {
            true => format!("{:?} {:?}", node.kind, node.loc),
            false => format!("{:?} first={}", node.loc, node.first.index() - nodes[0].first.index()),
        });
        leaves.collect::<Vec<_>>().join(" ")
    };
    let law = |law: &Law| {
        let steps: Vec<String> = file[law.steps].iter().map(|step| format!("{:?}", step.loc)).collect();
        let mut roots = Vec::new();
        law_roots(file, law, &mut roots);
        let roots: Vec<String> = roots.into_iter().map(expr).collect();
        format!("{:?} {:?} {:?} {:?} {steps:?} {roots:?}", law.doc, law.name, law.trigger_loc, law.loc)
    };
    for item in &file.items {
        write!(out, "{:?} {:?} ", item.loc, item.doc).unwrap();
        let text = match item.kind {
            ItemKind::Txn(id) => format!("{:?} {}", file[id].date, flow(&file[id].flow)),
            ItemKind::Assert(id) => format!("{:?} {}", file[id].amount, place(&file[id].place)),
            ItemKind::Event(id) => format!("{:?}", file[id]),
            ItemKind::Price(id) => format!("{:?}", file[id]),
            ItemKind::Split(id) => format!("{:?}", file[id]),
            ItemKind::Setting(id) => format!("{:?}", file[id]),
            ItemKind::Sync(id) => format!("{:?}", file[id]),
            ItemKind::Occurrence(id) => format!("{:?} {}", file[id].plan, legs(file[id].legs)),
            ItemKind::Opening(id) => legs(file[id].lines),
            ItemKind::Plan(id) => format!("{:?} {:?} {}", file[id].name, file[id].every, flow(&file[id].flow)),
            ItemKind::Code(id) => format!("{:?} {:?}", file[id].pattern, &file[file[id].on]),
            ItemKind::Law(id) => law(&file[id]),
            ItemKind::Param(id) => {
                let rows = file[file[id].rows].iter();
                let rows: Vec<String> = rows.map(|row| format!("{:?} {}", &file[row.keys], expr(row.value))).collect();
                format!("{:?} {rows:?}", file[id].name)
            }
            ItemKind::Decl(id) => {
                let decl = &file[id];
                let props = file[decl.props].iter();
                let props: Vec<String> = props
                    .map(|prop| {
                        format!(
                            "{:?} {:?}",
                            prop.name,
                            file[prop.args].iter().map(|&arg| expr(arg)).collect::<Vec<_>>()
                        )
                    })
                    .collect();
                let laws: Vec<String> = file[decl.laws].iter().map(law).collect();
                format!("{:?} {:?} {:?} {props:?} {laws:?}", decl.name, decl.alias, decl.kind)
            }
        };
        writeln!(out, "{text}").unwrap();
    }
    out
}

/// Cutting a file into pieces and joining them gives, node for node, what one
/// thread builds; so do the diagnostics, however small the pieces.
#[test]
fn a_file_parsed_in_pieces_is_the_file_parsed_whole() {
    let same = |src: &str| {
        let (whole, whole_diags) = parse_in(FileId(0), src, 1);
        let expected = (dump(&whole), format!("{whole_diags:?}"));
        for pieces in [2, 3, 7, 16, 200] {
            let (file, diags) = parse_in(FileId(0), src, pieces);
            assert_eq!((dump(&file), format!("{diags:?}")), expected, "{pieces} pieces of\n{src}");
        }
    };
    same(EXAMPLE);
    same(&EXAMPLE.repeat(3));
    same(
        "/// Kept with the item below, across a cut.\n\n// and a comment\n2026-01-01 a -> b 5 USD\n\n/// Two.\n/// Lines.\nlaw l\n  on in\n",
    );
    damaged(300, same);
    same(&"2026-01-01 a -> b 5 USD\n2026-01-02 a -> b 6 USD\n\tstray\n2026-01-03 a -> b 7 USD\n".repeat(50));
}

#[test]
fn cuts_fall_between_items_and_keep_docs_with_theirs() {
    let src = "a -> b\n  leg\n// note\n/// Doc.\n\n2026-01-01 x\n  y\nlaw z\n";
    for at in 0..src.len() {
        if let Some(boundary) = crate::item_boundary(src.as_bytes(), at) {
            assert!(src[boundary..].starts_with("// note") || src[boundary..].starts_with("law"), "{boundary}");
        }
    }
}
