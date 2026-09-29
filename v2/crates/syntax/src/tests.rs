//! Tests for the parts of the parser that are easy to get subtly wrong: how
//! words are classified, the shape of the tree and its arenas, recovery, the
//! parallel parse, and the wording of the most important diagnostics. The rest
//! is covered by parsing a realistic file end to end.

use std::path::{Path, PathBuf};

use axiom_core::{Day, Dec, Diagnostic, FileId, Severity, Span};

use crate::ast::*;
use crate::lex::{Lexer, Malformed, Punct, Tok};
use crate::parse_in as parse_pieces;

const EXAMPLE: &str = r#"
base USD
use us/401k
relaxed

commodity USD : currency
  precision 2
  name "US dollar"

entity acme : employer
entity me : person
  born 1990-05-04
  lives us/ca/san-francisco from 2026-01-01
entity aldi, kroger : grocer
  purpose groceries

account checking : bank at chase
  owner me
  holds USD, EUR
  opened 2020-01-01
account joint/savings : deposit
account retirement : 401k at fidelity
  employer acme

asset condo : rental-home
  in-service 2024-03-01
  land 120_000 USD

purpose food
purpose groceries : food
  business 50% for studio
purpose improvement : capital
  of asset
budget food 900 USD monthly
budget groceries 4_000 USD yearly

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

law rental-expenses
  on flow
  when purpose is repair of self | #insurance and not ^lump
  let last = tally(rental-income, year - 1)
  consume amount
  count amount as rental-expenses

law wash-sale
  on gain
  when gain < empty
  require tally(losses) <= 0 USD else carry gain to VTI within 30d

// Paycheck: legs are targets.
2026-01-15 acme -> 5_200 USD
  /// Pre-tax deferral.
  retirement      800 USD ^pretax
  taxes/federal   910 USD  // withheld
  checking        ...

2026-02-01 -> landlord 1_800 USD
  checking       1_000 USD
  savings        ...

2026-01-18 checking -> food 84.20 USD via trader-joes ^groceries
2026-01-22 checking 2_000 USD -> brokerage 7 VTI
2026-01-22 checking -> brokerage 7 VTI @ 285.70 USD
2026-09-02 brokerage[fifo] 10 VTI -> checking 3_050 USD
2026-09-02 brokerage[^house, 2024, 2026-01..2026-06, 2026-01-22] all -> checking 52_000 USD
2026-02-01 checking -> plumber (350 USD) ^check-1041 ! "waived"
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
2026-02-06 ^check-1041 settled
2026-02-20 ^check-1044 void
2026-03-04 ^deposit-77 returned
2026-01-02 VTI 280.14 USD
2026-05-22 FAST split 2 for 1
2026-04-01 brokerage ->
  savings = 5_000 USD

2026-01-15 checking -> taxes/federal 3_000 USD for 2025
2026-03-01 design -> acme 4_800 USD ^inv-12 due 30d
2026-04-02 acme -> checking 4_800 USD ^inv-12
2026-09-06 checking -> savings 100 USD for car-fund
2026-05-16 grandma -> college 3_000 USD basis 3_000 USD
2026-05-01 old-broker all VXUS -> new-broker
2026-12-29 house 1 HOME -> 431_500 USD
  closing-costs  25_000 USD
  mortgage       276_282.05 USD
  checking       ...

2026-02-02 checking -> bay-plumbing 1_480 USD #improvement of condo "new water heater" via plumbers-inc
2026-02-03 VTI -> fidelity 198.12 USD
2026-02-05 fidelity[2026-01-20] 1.62 VTI -> 481.14 USD
2026-03-26 VXUS -> 25.09 USD
  foreign-tax    2.49 USD
  fidelity       ...
2026-01-31 lumen -> 4_600 USD
  retirement     6%
  checking       ...
2026-01-27 halcyon owes studio 3_800 USD due 30d ^inv-2026-01 #design
2026-02-04 me owes pge 142.50 USD due 2026-02-20 "a bill"
2026-03-10 netflix ends

opening 2024-12-31
  jo owes me    600 USD due 2026-04-01
  checking      10_000 USD
  college       24_600 USD   basis 19_850 USD
  house         1 HOME       basis 540_000 USD   since 2023-06-15

2026-01-16 paycheck
2026-01-30 paycheck
  taxes/federal  950 USD
2026-03-13 paycheck 5_900 USD

/// The job, paid twice a month.
contract job with lumen
  4_600 USD twice monthly on 15, last into checking #wages "gross"
  /// Six percent of the gross is deferred.
  retirement   6%
  blue-shield  184.20 USD #premium
  match 50% of retirement up to 6%

contract flat with greystar
  2_900 USD monthly on 1 from checking
  business 12% for studio
  until 2026-08-31

contract mortgage with rocket
  loan 320_000 USD on 2024-02-20 at 5.875% over 30y for condo
  monthly on 1 from checking
  escrow 410 USD into escrow

contract lease with dana
  2_350 USD monthly on 1 into checking #rent of condo
  deposit 2_350 USD
  from 2025-07-01 until 2026-06-30
  law no-late-rent
    on in
    warn total(in, month) <= 2_350 USD

contract condo-insurance with state-farm
  1_140 USD yearly on 03-01 from checking #insurance of condo
  covers the year

contract vti-monthly with fidelity
  buy VTI for 500 USD monthly on 20 from checking

contract gym with equinox
  every 2w on friday from checking

sync prices
  run python3 fetch_prices.py --symbol VTI // not a comment
  into prices/{year}.ax
  csv date "Date" "YYYY-MM-DD", amount 3 flipped, memo 2
"#;

// ─── Helpers ────────────────────────────────────────────────────────────────

/// A file that gives no place: every date is written in full.
fn parse(file: FileId, src: &str) -> (File<'_>, Vec<Diagnostic>) {
    crate::parse(file, src, Folder::default())
}

fn parse_in(file: FileId, src: &str, pieces: usize) -> (File<'_>, Vec<Diagnostic>) {
    parse_pieces(file, src, Folder::default(), pieces)
}

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

/// The statements of a file, in order.
fn statements<'f, 's>(file: &'f File<'s>) -> Vec<&'f Statement<'s>> {
    let kinds = file.items.iter().filter_map(|item| match item.kind {
        ItemKind::Statement(id) => Some(&file[id]),
        _ => None,
    });
    kinds.collect()
}

/// The clauses of a tail, as their kinds.
fn clauses<'f, 's>(file: &'f File<'s>, tail: Many<Clause<'s>>) -> Vec<&'f ClauseKind<'s>> {
    file[tail].iter().map(|clause| &clause.kind).collect()
}

// ─── Whole files ────────────────────────────────────────────────────────────

/// The `.ax` files under `dir`, with their paths relative to `root`.
fn ax_files(root: &Path, dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        match entry.path() {
            path if path.is_dir() => found.extend(ax_files(root, &path)),
            path if path.extension().is_some_and(|extension| extension == "ax") => {
                found.push(path.strip_prefix(root).unwrap().to_path_buf())
            }
            _ => {}
        }
    }
    found.sort();
    found
}

/// Every line of the v4 sketch, but the sketch of std, parses without a diagnostic.
#[test]
fn the_v4_sketch_parses_clean() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/v4-sketch");
    let files = ax_files(&root, &root);
    assert_eq!(files.len(), 11, "{files:?}");
    let mut items = 0;
    for path in files.iter().filter(|path| path.file_name().is_some_and(|name| name != "std-sketch.ax")) {
        let src = std::fs::read_to_string(root.join(path)).unwrap();
        let (file, diags) = crate::parse(FileId(0), &src, Folder::of(path.to_str().unwrap()));
        assert!(diags.is_empty(), "{}:\n{}", path.display(), render(&src, &diags));
        items += file.items.len();
    }
    assert!(items > 100, "{items} items");
}

#[test]
fn a_realistic_file_parses_into_the_expected_shapes() {
    let file = parse_clean(EXAMPLE);
    assert_eq!(file.items.len(), 77);
    let txns = txns(&file);

    // A paycheck: one named side, legs for the other, the last taking the remainder.
    let paycheck = txns[0];
    let legs = &file[paycheck.flow.body.legs];
    assert!(paycheck.flow.from.end.is_some() && paycheck.flow.to.end.is_none());
    assert!(matches!(legs[2].amount, Quantity::Rest));
    assert!(matches!(clauses(&file, legs[0].tail)[..], [ClauseKind::Code(Code("^pretax"))]));
    assert_eq!(legs[0].doc.unwrap().lines().collect::<Vec<_>>(), ["Pre-tax deferral."]);

    // Selectors normalise to inclusive day ranges.
    let sale = txns[6];
    let end = sale.flow.from.end.as_ref().unwrap();
    let selectors = &file[end.select];
    assert!(matches!(&selectors[0], Select::Code(code) if code.name() == "house"));
    assert!(matches!(selectors[1], Select::Range(a, b, _) if (a, b) == (day(2024, 1, 1), day(2024, 12, 31))));
    assert!(matches!(selectors[2], Select::Range(a, b, _) if (a, b) == (day(2026, 1, 1), day(2026, 6, 30))));
    assert!(matches!(selectors[3], Select::Range(a, b, _) if (a, b) == (day(2026, 1, 22), day(2026, 1, 22))));
    assert!(matches!(sale.flow.from.amount, Some(Quantity::All(None))));

    // A pending amount, its code and its waiver.
    assert!(matches!(txns[7].flow.to.amount, Some(Quantity::Pending(_))));
    let kinds = clauses(&file, txns[7].flow.tail);
    assert!(matches!(kinds[0], ClauseKind::Code(code) if code.name() == "check-1041"));
    assert!(matches!(kinds[1], ClauseKind::Waive(Waive { reason: Some(Text("waived")), .. })));

    // A spread is the clause `for DATE..DATE`.
    let spread = clauses(&file, txns[11].flow.tail);
    assert!(
        matches!(spread[..], [ClauseKind::For(For::Period(a, b))] if (*a, *b) == (day(2026, 1, 1), day(2026, 12, 31)))
    );
}

/// The properties of a contract, each as its name and the text of its arguments.
fn properties(file: &File, contract: &Contract, src: &str) -> Vec<(String, Vec<String>)> {
    let text = |arg: &ExprId| src[file.exprs[*arg].loc.range()].to_string();
    file[contract.props].iter().map(|prop| (prop.name.0.to_string(), file[prop.args].iter().map(text).collect())).collect()
}

#[test]
fn a_contract_has_a_schedule_properties_and_a_template() {
    let file = parse_clean(EXAMPLE);
    let contracts: Vec<&Contract> = file.iter().collect();
    assert_eq!(contracts.len(), 7);
    let props = |contract: usize| properties(&file, contracts[contract], EXAMPLE);
    let line = |name: &str, args: &[&str]| (name.to_string(), args.iter().map(|arg| arg.to_string()).collect::<Vec<_>>());

    // The job: a payment, two days a month, a purpose and a description, a template and a match.
    let job = contracts[0];
    assert_eq!((job.name.0, job.party.map(|party| party.0)), ("job", Some("lumen")));
    let schedule = job.schedule.unwrap();
    let terms = schedule.terms;
    assert!(matches!(terms.payment, Some(Payment::Fixed(amount)) if amount.0 == "4_600 USD"));
    assert_eq!((terms.cadence, &file[terms.on]), (Cadence::TwiceMonthly, &[On::MonthDay(15), On::Last][..]));
    let holding = terms.holding.unwrap();
    assert_eq!((holding.direction, holding.name.0), (Direction::Into, "checking"));
    assert_eq!((terms.purpose.map(|purpose| purpose.name.0), schedule.description.map(|text| text.0)), (Some("wages"), Some("gross")));
    let legs = &file[job.body.legs];
    assert!(matches!(legs[0].amount, Quantity::Percent(_)) && legs.len() == 2);
    assert_eq!(legs[0].doc.unwrap().lines().collect::<Vec<_>>(), ["Six percent of the gross is deferred."]);
    assert_eq!(props(0), [line("match", &["50%", "of", "retirement", "up", "to", "6%"])]);

    // The flat's properties, and a loan whose schedule has no amount.
    assert_eq!(props(1), [line("business", &["12%", "for", "studio"]), line("until", &["2026-08-31"])]);
    assert!(contracts[2].schedule.is_some_and(|schedule| schedule.terms.payment.is_none()));
    let loan = ["320_000 USD", "on", "2024-02-20", "at", "5.875%", "over", "30y", "for", "condo"];
    assert_eq!(props(2), [line("loan", &loan), line("escrow", &["410 USD", "into", "escrow"])]);

    // A lease: a deposit, two properties on one line, a purpose with an object, and a law.
    let lease = contracts[3];
    assert_eq!(props(3), [line("deposit", &["2_350 USD"]), line("from", &["2025-07-01", "until", "2026-06-30"])]);
    let rent = lease.schedule.unwrap().terms.purpose.unwrap();
    assert_eq!((rent.name.0, rent.of.map(|of| of.0)), ("rent", Some("condo")));
    assert_eq!(file[lease.laws].len(), 1);

    // A day of every year, what a payment covers, a standing order and a fortnight.
    let insurance = contracts[4];
    assert_eq!(file[insurance.schedule.unwrap().terms.on], [On::YearDay { month: 3, day: 1 }]);
    assert_eq!(props(4), [line("covers", &["the", "year"])]);
    let buy = contracts[5].schedule.unwrap().terms;
    assert!(matches!(buy.payment, Some(Payment::Buy { unit: Name("VTI"), spend }) if spend.0 == "500 USD"));
    let gym = contracts[6].schedule.unwrap().terms;
    assert_eq!((gym.cadence, &file[gym.on]), (Cadence::Every(Span::days(14)), &[On::Weekday(4)][..]));
}

#[test]
fn a_contracts_lines_come_in_any_order() {
    let src = "contract a with p\n  covers 6m\n  retirement 6%\n  5 USD monthly from x\n  from 07-01\n";
    let (file, diags) = crate::parse(FileId(0), src, YEAR);
    assert!(diags.is_empty(), "{}", render(src, &diags));
    let contract: &Contract = file.iter().next().unwrap();
    assert!(contract.schedule.is_some() && contract.body.legs.len() == 1 && contract.props.len() == 2);
    // A short date in a property is completed as anywhere else.
    assert!(matches!(file.exprs[file[file[contract.props][1].args][0]].kind, ExprKind::Date(d) if d == day(2026, 7, 1)));
}

#[test]
fn every_cadence_is_a_span_between_occurrences() {
    let cadences = ["daily", "weekly", "monthly", "quarterly", "yearly", "every 2w", "every 1y6m"];
    let months = [0, 0, 1, 3, 12, 0, 18];
    let days = [1, 7, 0, 0, 0, 14, 0];
    for (written, (months, days)) in cadences.into_iter().zip(months.into_iter().zip(days)) {
        let src = format!("contract c with p\n  1 USD {written} from x\n");
        let file = parse_clean(&src);
        let contract: &Contract = file.iter().next().unwrap();
        assert_eq!(contract.schedule.unwrap().terms.cadence, Cadence::Every(Span { months, days }), "{written}");
    }
}

#[test]
fn a_contract_keeps_its_good_lines_and_says_what_is_wrong_with_the_bad() {
    let src = "contract c with p\n  5 USD monthly from x\n  retirement\n  business 5% for y\n";
    let (file, diags) = parse(FileId(0), src);
    assert_eq!(diags.iter().map(|diag| &*diag.code).collect::<Vec<_>>(), ["expected-amount"]);
    let contract: &Contract = file.iter().next().unwrap();
    assert!(contract.damaged && contract.schedule.is_some() && contract.props.len() == 1);

    let contract_with = |lines: &str| format!("contract c with p\n{lines}");
    let one = |lines: &str, code: &str| only_error(&contract_with(lines), code);
    one("  deposit 5 USD\n", "missing-schedule");
    one("  5 USD monthly from x\n  weekly from y\n", "duplicate-clause");
    one("  5 USD monthly on 32 from x\n", "bad-day");
    one("  5 USD monthly on 02-30 from x\n", "bad-day");
    one("  5 USD monthly on 15 x\n", "unknown-direction");
    one("  5 USD monthly from x ^code\n", "expected-end-of-line");
    one("  5 USD monthly from x for 2025\n", "expected-end-of-line");
    one("  5 USD every from x\n", "expected-span");
    one("  5 USD twice from x\n", "expected-keyword");
    for (written, code, replaced, fix) in [
        ("montly", "unknown-cadence", "montly", "monthly"),
        ("monthly on mondey", "unknown-day", "mondey", "monday"),
    ] {
        let src = contract_with(&format!("  5 USD {written} from x\n"));
        assert_eq!(first_fix(&src, &only_error(&src, code)), (replaced, fix));
    }
}

#[test]
fn a_v3_code_is_written_with_a_caret_now() {
    for (src, replaced) in [
        ("2026-02-06 #check-1041 settled\n", "#check-1041"),
        ("2026-09-02 fidelity[#house] all -> checking 5 USD\n", "#house"),
        ("2026-09-02 fidelity[fifo, #house.a] all -> checking 5 USD\n", "#house.a"),
    ] {
        let error = only_error(src, "hash-code");
        assert_eq!(first_fix(src, &error), (replaced, &*format!("^{}", &replaced[1..])), "{src}");
    }
    // In a tail it is a purpose, which only the model can say is not one.
    parse_clean("2026-09-02 a -> b 5 USD #house\n");
}

#[test]
fn a_chart_account_is_marked_with_a_note_and_kept() {
    let src = "account income/salary : wages\naccount expenses/food\naccount equity/opening\naccount assets/bank : bank\n";
    let (file, diags) = parse(FileId(0), src);
    let marked: Vec<&str> = diags.iter().map(|diag| &src[diag.anchor().unwrap().range()]).collect();
    assert_eq!(marked, ["income/salary", "expenses/food", "equity/opening"]);
    assert!(diags.iter().all(|diag| diag.severity == Severity::Note && diag.code == "chart-account"));
    assert_eq!(file.iter::<Decl>().count(), 4, "the accounts are kept, and the model judges them");
    // Only accounts have a chart, and only under those roots.
    parse_clean("entity income/x\naccount incomes/x : y\n");
}

#[test]
fn a_plan_is_a_contract_now() {
    for src in ["every month on 1 checking -> landlord 2_400 USD\n", "plan paycheck every 2w acme -> 5_200 USD\n  a ...\n"] {
        let error = only_error(src, "plan-is-a-contract");
        assert!(error.help[0].text.contains("contract NAME with PARTY"));
    }
}

#[test]
fn comments_docs_and_raw_text() {
    let file = parse_clean("/// One.\n/// Two.\n\nlaw x\n  on in\n");
    assert_eq!(file.items[0].doc.unwrap().lines().collect::<Vec<_>>(), ["One.", "Two."]);

    let file = parse_clean("sync prices // daily\n  run curl -s https://example.com/a // b\r\n  into prices/{year}.ax\n");
    let ItemKind::Sync(id) = file.items[0].kind else { panic!("a sync") };
    let sync = &file[id];
    assert_eq!((sync.name.0, sync.run.0), ("prices", "curl -s https://example.com/a // b"));
    assert_eq!(sync.into.map(|text| text.0), Some("prices/{year}.ax"));

    let (_, diags) = parse(FileId(0), "2026-01-01 checking -> food 5 USD\n/// Nothing follows.\n");
    assert_eq!(diags[0].code, "unattached-doc");
}

#[test]
fn locations_come_from_where_a_slice_was_written() {
    let src = "2026-01-18 checking -> food 84.20 USD via trader-joes ^groceries\n";
    let file = parse_clean(src);
    let txn = txns(&file)[0];
    let end = txn.flow.to.end.as_ref().unwrap();
    assert_eq!(&src[file.loc(&end.name).range()], "food");
    let Some(Quantity::Fixed(amount)) = txn.flow.to.amount else { panic!("an amount") };
    assert_eq!(&src[file.loc(&amount).range()], "84.20 USD");
    assert_eq!((amount.num(), amount.unit().map(|unit| unit.0)), (Dec { mantissa: 8420, scale: 2 }, Some("USD")));
    let kinds = clauses(&file, txn.flow.tail);
    let (ClauseKind::Via(party), ClauseKind::Code(code)) = (kinds[0], kinds[1]) else { panic!("a party, a code") };
    assert_eq!((&src[file.loc(party).range()], &src[file.loc(code).range()]), ("trader-joes", "^groceries"));
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
    let src = "2026-03-01 design -> acme 4_800 USD due 30d for 2025 via me ^inv-12 basis empty @ 2 USD ! \"ok\"\n";
    let file = parse_clean(src);
    let kinds = clauses(&file, txns(&file)[0].flow.tail);
    assert!(matches!(kinds[0], ClauseKind::Due(Due::After(span)) if *span == Span::days(30)));
    assert!(matches!(kinds[1], ClauseKind::For(For::Period(a, b)) if (*a, *b) == (day(2025, 1, 1), day(2025, 12, 31))));
    assert!(matches!(kinds[2], ClauseKind::Via(Name("me"))));
    assert!(matches!(kinds[3], ClauseKind::Code(_)));
    assert!(matches!(kinds[4], ClauseKind::Basis(amount) if amount.0 == "empty"));
    assert!(matches!(kinds[5], ClauseKind::Price(_)));
    assert!(matches!(kinds[6], ClauseKind::Waive(_)));

    let file = parse_clean("2026-01-01 a -> b 5 USD for car-fund ^a ^b\n");
    let kinds = clauses(&file, txns(&file)[0].flow.tail);
    assert!(matches!(kinds[0], ClauseKind::For(For::Whom(Name("car-fund")))));
    assert!(matches!(kinds[1..], [ClauseKind::Code(Code("^a")), ClauseKind::Code(Code("^b"))]), "codes repeat");

    only_error("2026-01-01 a -> b 5 USD for 2025 for 2026\n", "duplicate-clause");
    only_error("2026-01-01..2026-12-31 a -> b 5 USD for 2025\n", "duplicate-clause");
    only_error("2026-01-01 a -> b 5 USD due tomorrow\n", "expected-date");
    only_error("2026-01-01 a -> b 5 USD via c via d\n", "duplicate-clause");
}

#[test]
fn ends_take_selectors_and_all_of_a_commodity() {
    let file = parse_clean("2026-09-16 checking[^roof, fifo] -> house 1_000 USD\n2026-05-01 old all VXUS -> new\n");
    let txns = txns(&file);
    let source = txns[0].flow.from.end.as_ref().unwrap();
    assert_eq!(file[source.select].len(), 2);
    assert!(matches!(txns[1].flow.from.amount, Some(Quantity::All(Some(Name("VXUS"))))));
}

#[test]
fn a_flow_says_why_with_a_purpose_a_description_and_codes() {
    let src = "2026-02-02 a -> b 1_480 USD #improvement of condo \"new water heater\" ^job-7 via plumbers-inc\n";
    let file = parse_clean(src);
    let kinds = clauses(&file, txns(&file)[0].flow.tail);
    let ClauseKind::Purpose(purpose) = kinds[0] else { panic!("a purpose") };
    assert_eq!((purpose.name.0, purpose.of.map(|of| of.0)), ("improvement", Some("condo")));
    assert!(matches!(kinds[1], ClauseKind::Description(Text("new water heater"))));
    assert!(matches!(kinds[2..], [ClauseKind::Code(Code("^job-7")), ClauseKind::Via(Name("plumbers-inc"))]));

    // The purpose's object is its own name, not the next clause.
    let file = parse_clean("2026-02-02 a -> b 5 USD #groceries \"the weekly shop\"\n");
    let kinds = clauses(&file, txns(&file)[0].flow.tail);
    assert!(matches!(kinds[0], ClauseKind::Purpose(Purpose { name: Name("groceries"), of: None })));
    // A string after `!` is the waiver's reason, not a description.
    let file = parse_clean("2026-02-02 a -> b 5 USD ! \"ok\"\n");
    assert!(matches!(clauses(&file, txns(&file)[0].flow.tail)[..], [ClauseKind::Waive(Waive { reason: Some(Text("ok")), .. })]));

    only_error("2026-02-02 a -> b 5 USD #a #b\n", "duplicate-clause");
    only_error("2026-02-02 a -> b 5 USD \"x\" \"y\"\n", "duplicate-clause");
    only_error("2026-02-02 a -> b 5 USD #repair of\n", "expected-name");
}

#[test]
fn ends_may_be_commodities_and_an_exchange_may_name_only_its_source() {
    let file = parse_clean("2026-02-03 VTI -> fidelity 198.12 USD\n2026-02-05 fidelity[2026-01-20] 1.62 VTI -> 481.14 USD\n");
    let flows = txns(&file);
    assert_eq!(flows[0].flow.from.end.map(|end| end.name.0), Some("VTI"));
    let exchange = &flows[1].flow;
    assert!(exchange.to.end.is_none() && matches!(exchange.to.amount, Some(Quantity::Fixed(_))) && exchange.body.legs.is_empty());
    // A commodity first is a party for a flow, and a price when nothing follows it.
    let file = parse_clean("2026-02-03 VTI 280.14 USD\n");
    assert!(matches!(statements(&file)[0].predicate, Predicate::Price(price) if price.0 == "280.14 USD"));

    let file = parse_clean("2026-03-26 VXUS -> 25.09 USD\n  foreign-tax 2.49 USD\n  fidelity ...\n");
    assert_eq!(file[txns(&file)[0].flow.body.legs].len(), 2);
    // Both amounts are what says it is an exchange; a lone source is not one.
    only_error("2026-02-05 fidelity -> 481.14 USD\n", "missing-legs");
    only_error("2026-02-05 fidelity 1.62 VTI ->\n", "missing-legs");
}

#[test]
fn a_leg_may_be_a_share_of_the_header_and_an_opening_line_an_asset() {
    let file = parse_clean("2026-01-31 lumen -> 4_600 USD\n  retirement 6%\n  checking ...\n");
    let legs = &file[txns(&file)[0].flow.body.legs];
    assert!(matches!(legs[0].amount, Quantity::Percent(Dec { mantissa: 6, scale: 0 })));
    only_error("2026-01-31 lumen -> 6%\n", "expected-end-of-line");

    let file = parse_clean("opening 2026-01-01\n  condo basis 402_000 USD since 2024-02-20\n  checking 5 USD\n");
    let ItemKind::Opening(id) = file.items[0].kind else { panic!("an opening") };
    let lines = &file[file[id].lines];
    assert!(matches!(lines[0].amount, Quantity::Whole) && matches!(lines[1].amount, Quantity::Fixed(_)));
    assert!(matches!(clauses(&file, lines[0].tail)[..], [ClauseKind::Basis(_), ClauseKind::Since(_)]));
    // Only an opening has things without amounts.
    only_error("opening 2026-01-01\n  condo since 2024-02-20\n", "expected-amount");
    only_error("2026-01-31 lumen -> 4_600 USD\n  condo basis 5 USD\n", "expected-amount");
}

#[test]
fn a_code_settles_by_being_on_the_flow_not_after_for() {
    let src = "2026-04-02 acme -> checking 4_800 USD for #inv-12\n";
    assert_eq!(first_fix(src, &only_error(src, "hash-code")), ("for #inv-12", "^inv-12"));
    only_error("2026-04-02 acme -> checking 4_800 USD for ^inv-12\n", "expected-period");
}

#[test]
fn a_basis_is_no_end_any_more() {
    for src in ["2026-09-15 checking -> house.basis 14_200 USD\n", "2026-09-16 checking -> house[^roof].basis 1_000 USD\n"] {
        let error = only_error(src, "basis-end");
        assert_eq!(&src[error.anchor().unwrap().range()], ".basis");
        assert!(error.help.iter().any(|help| help.text.contains("#improvement of ASSET")));
    }
    // A dot that does not touch what it follows is no `.basis`.
    only_error("2026-09-15 checking -> house .basis 14_200 USD\n", "expected-end-of-line");
}

#[test]
fn a_header_may_state_both_amounts_while_naming_one_end() {
    let file = parse_clean("2026-12-29 house 1 HOME -> 431_500 USD\n  closing 25_000 USD\n  checking ...\n");
    let flow = &txns(&file)[0].flow;
    assert!(flow.from.end.is_some() && flow.from.amount.is_some());
    assert!(flow.to.end.is_none() && matches!(flow.to.amount, Some(Quantity::Fixed(_))));
    assert_eq!(file[flow.body.legs].len(), 2);
}

#[test]
fn a_dated_name_is_an_occurrence_unless_it_says_something_else() {
    let file = parse_clean(
        "2026-01-16 paycheck\n2026-03-13 paycheck 5_900 USD\n  taxes 950 USD\n2026-01-31 paycheck = 5 USD\n",
    );
    let said = statements(&file);
    assert!(matches!(said[0].subject, Subject::Name(Name("paycheck"))));
    assert!(matches!(said[0].predicate, Predicate::Occurrence { amount: None }) && said[0].body.legs.is_empty());
    assert!(matches!(said[1].predicate, Predicate::Occurrence { amount: Some(amount) } if amount.0 == "5_900 USD"));
    assert_eq!(said[1].body.legs.len(), 1);
    assert!(matches!(said[2].predicate, Predicate::Assert(_)));

    // A commodity amount is what was bought, and a contract may end.
    let file = parse_clean("2026-01-20 vti-monthly 1.620 VTI\n2026-03-10 netflix ends\n");
    let said = statements(&file);
    assert!(matches!(said[0].predicate, Predicate::Occurrence { amount: Some(amount) } if amount.0 == "1.620 VTI"));
    assert!(matches!((&said[1].subject, &said[1].predicate), (Subject::Name(Name("netflix")), Predicate::Ends)));
    assert_eq!(said[1].date, day(2026, 3, 10));
    only_error("2026-03-10 netflix ends soon\n", "expected-end-of-line");
    only_error("2026-03-10 netflix[fifo] ends\n", "expected-arrow");
}

#[test]
fn a_claim_says_who_owes_whom_and_is_flow_shaped() {
    let file = parse_clean(EXAMPLE);
    let claims: Vec<(&Statement, &Owes)> = file
        .iter::<Statement>()
        .filter_map(|said| if let Predicate::Owes(owes) = &said.predicate { Some((said, owes)) } else { None })
        .collect();
    assert_eq!(claims.len(), 3, "two dated claims, and the one an opening states");
    let (invoice, owes) = claims[0];
    assert!(matches!(invoice.subject, Subject::Name(Name("halcyon"))));
    assert_eq!((owes.creditor.0, owes.amount.map(|amount| amount.0)), ("studio", Some("3_800 USD")));
    assert!(matches!(owes.due, Some(Due::After(_))) && owes.purpose.is_some_and(|purpose| purpose.name.0 == "design"));
    assert_eq!(file[invoice.codes].iter().map(|code| code.0).collect::<Vec<_>>(), ["^inv-2026-01"]);
    let (bill, owes) = claims[1];
    assert!(matches!(bill.subject, Subject::Name(Name("me"))) && owes.creditor.0 == "pge");
    assert!(matches!(owes.due, Some(Due::On(_))) && bill.description.is_some_and(|text| text.0 == "a bill"));
    let (open, _) = claims[2];
    assert!(matches!(open.subject, Subject::Name(Name("jo"))) && open.date == day(2024, 12, 31));

    only_error("2026-01-27 halcyon owes 3_800 USD\n", "expected-name");
    only_error("2026-01-27 halcyon owes studio\n", "expected-amount");
    only_error("opening 2026-01-01\n  jo owes me 600 USD\n  jo owes me\n", "expected-amount");
    // A claim takes items, not legs.
    only_error("2026-01-27 halcyon owes studio 3_800 USD\n  checking ...\n", "claim-takes-items");
    // A claim with no amount is the sum of its items.
    let file = parse_clean("2026-01-27 halcyon owes studio\n  3_000 USD #design\n    800 USD #design\n");
    let claim = statements(&file)[0];
    assert!(matches!(claim.predicate, Predicate::Owes(Owes { amount: None, .. })) && claim.body.items.len() == 2);
}

#[test]
fn assertions_may_be_negative_and_may_say_where_a_gap_goes() {
    let file = parse_clean(
        "2026-06-30 checking = -42.17 USD\n2026-03-31 retirement = 24_600 USD via market\n2026-01-31 a = 1 USD ! \"x\"\n",
    );
    let asserts: Vec<&Assertion> = statements(&file)
        .into_iter()
        .filter_map(|said| if let Predicate::Assert(assertion) = &said.predicate { Some(assertion) } else { None })
        .collect();
    assert_eq!(asserts[0].amount.num(), Dec { mantissa: -4217, scale: 2 });
    assert!(matches!(asserts[0].gap, Gap::Refused));
    assert!(matches!(asserts[1].gap, Gap::Via(Name("market"))));
    assert!(matches!(asserts[2].gap, Gap::Waived(Waive { reason: Some(Text("x")), .. })));
}

#[test]
fn splits_and_openings() {
    let file = parse_clean(EXAMPLE);
    let split = statements(&file).into_iter().find(|said| matches!(said.predicate, Predicate::Split { .. })).unwrap();
    let Predicate::Split { numerator, denominator } = split.predicate else { unreachable!() };
    assert!(matches!(split.subject, Subject::Unit(Name("FAST"))));
    assert_eq!((numerator.mantissa, denominator.mantissa), (2, 1));
    let opening: &Opening = file.iter().next().unwrap();
    let lines = &file[opening.lines];
    assert_eq!((opening.date, lines.len()), (day(2024, 12, 31), 3));
    let kinds = clauses(&file, lines[2].tail);
    assert!(
        matches!(kinds[0], ClauseKind::Basis(_)) && matches!(kinds[1], ClauseKind::Since(d) if *d == day(2023, 6, 15))
    );
    only_error("opening 2024-12-31\n  checking ...\n", "opening-amount");
    only_error("2026-01-01 a -> b 5 USD since 2025-01-01\n", "expected-end-of-line");
    only_error("2026-01-01 FAST split 0 for 1\n", "bad-split");
}

#[test]
fn declarations_take_lists_institutions_and_several_globs() {
    let file = parse_clean(EXAMPLE);
    let decls: Vec<&Decl> = file.iter::<Decl>().collect();
    let names: Vec<&str> = decls.iter().map(|decl| decl.name.0).collect();
    assert_eq!(&names[..8], ["USD", "acme", "me", "aldi", "kroger", "checking", "joint/savings", "retirement"]);
    let institutions: Vec<Option<&str>> = decls[5..8].iter().map(|decl| decl.at.map(|at| at.0)).collect();
    assert_eq!(institutions, [Some("chase"), None, Some("fidelity")]);
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
fn assets_purposes_and_budgets_are_declared() {
    let file = parse_clean(EXAMPLE);
    let decls: Vec<&Decl> = file.iter::<Decl>().collect();
    let of = |what| decls.iter().filter(|decl| decl.what == what).map(|decl| decl.name.0).collect::<Vec<_>>();
    assert_eq!(of(DeclKind::Asset), ["condo"]);
    assert_eq!(of(DeclKind::Purpose), ["food", "groceries", "improvement"]);
    let groceries = decls.iter().find(|decl| decl.name.0 == "groceries" && decl.what == DeclKind::Purpose).unwrap();
    assert_eq!(groceries.kind.map(|parent| parent.0), Some("food"));
    assert_eq!(file[groceries.props][0].name.0, "business");
    let condo = decls.iter().find(|decl| decl.what == DeclKind::Asset).unwrap();
    assert_eq!((condo.kind.map(|kind| kind.0), file[condo.props].len()), (Some("rental-home"), 2));

    let budgets: Vec<&Budget> = file.iter().collect();
    let (food, groceries) = (&budgets[0], &budgets[1]);
    assert!(matches!(food.allowance.limit, Limit::Fixed(amount) if amount.0 == "900 USD"));
    assert_eq!((food.purpose.0, food.allowance.per, food.allowance.carries), ("food", Period::Month, false));
    assert!(matches!(groceries.allowance.limit, Limit::Fixed(amount) if amount.0 == "4_000 USD"));
    assert_eq!(groceries.allowance.per, Period::Year);
    only_error("budget food 900 USD weekly\n", "unknown-period");
    only_error("budget food 900\n", "expected-commodity");
    only_error("budget 900 USD monthly\n", "expected-name");

    // `at` belongs to accounts, and needs an institution.
    only_error("entity x : person at chase\n", "expected-end-of-line");
    let (file, diags) = parse(FileId(0), "account x : deposit at\n");
    assert_eq!((diags.len(), file.iter::<Decl>().count()), (1, 1), "the account stays, without its institution");
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

// ─── Line items, `via`, and what v3 wrote ───────────────────────────────────

/// The items of the first flow or statement of `src`, as (sign, amount text, tail kinds).
fn items_of(src: &str) -> Vec<(Sign, String, usize)> {
    let file = parse_clean(src);
    let body = match file.items[0].kind {
        ItemKind::Txn(id) => file[id].flow.body,
        ItemKind::Statement(id) => file[id].body,
        _ => panic!("a flow or a statement"),
    };
    let amount = |amount: &ItemAmount| match amount {
        ItemAmount::Fixed(amount) => amount.0.to_string(),
        ItemAmount::Share(percent) => format!("{}%", percent.mantissa),
        ItemAmount::ShareOf(percent, of) => format!("{}% of {}", percent.mantissa, of.0),
    };
    file[body.items].iter().map(|item| (item.sign, amount(&item.amount), file[item.tail].len())).collect()
}

#[test]
fn a_line_that_names_no_end_is_an_item_of_the_flow_above_it() {
    let src = "\
2026-03-14 visa -> target 120.00 USD #household
  32.10 USD #groceries
  12.00 USD #gifts \"for jo's birthday\"
  + 5% of 100.00 USD #fees
  - 2.50 USD #discount
  10%
";
    let file = parse_clean(src);
    let flow = &txns(&file)[0].flow;
    let items = &file[flow.body.items];
    let kinds: Vec<(Sign, &ItemAmount)> = items.iter().map(|item| (item.sign, &item.amount)).collect();
    assert!(matches!(kinds[0], (Sign::Carve, ItemAmount::Fixed(amount)) if amount.0 == "32.10 USD"));
    assert!(matches!(kinds[2], (Sign::Add, ItemAmount::ShareOf(percent, of)) if percent.mantissa == 5 && of.0 == "100.00 USD"));
    assert!(matches!(kinds[3], (Sign::Less, ItemAmount::Fixed(_))));
    assert!(matches!(kinds[4], (Sign::Carve, ItemAmount::Share(percent)) if percent.mantissa == 10));
    let tail = clauses(&file, items[1].tail);
    assert!(matches!(tail[..], [ClauseKind::Purpose(_), ClauseKind::Description(Text("for jo's birthday"))]));
    assert!(flow.body.legs.is_empty());
    // What names an end is a leg, and under a one-sided flow the two may be mixed.
    let file = parse_clean("2026-03-14 lumen -> 4_600 USD\n  retirement 6%\n  - 100 USD #fees\n  checking ...\n");
    let flow = &txns(&file)[0].flow;
    assert_eq!((flow.body.legs.len(), flow.body.items.len()), (2, 1));
    // An item has an amount, and its tail is a flow's.
    only_error("2026-03-14 a -> b 5 USD\n  + #fees\n", "expected-amount");
    only_error("2026-03-14 a -> b 5 USD\n  - - 5 USD\n", "negative-amount");
}

#[test]
fn items_follow_occurrences_and_claims_and_may_be_lined_up_by_indenting() {
    assert_eq!(
        items_of("2026-03-01 flat\n  + 12% of 155.00 USD #utilities \"the building's water\"\n"),
        [(Sign::Add, "12% of 155.00 USD".to_string(), 2)]
    );
    // Amounts right-aligned with spaces are still one block of items.
    let invoice = "2026-01-27 halcyon owes studio due 30d ^inv-2026-01\n  3_000 USD #design \"brand refresh\"\n    800 USD #design \"icon set\"\n";
    assert_eq!(items_of(invoice).len(), 2);
    // Only what starts with an amount may be indented further.
    let src = "2026-01-27 a -> b 5 USD\n  c 1 USD\n    d 2 USD\n";
    only_error(src, "unexpected-indent");
    // A contract's template may have them too, and its schedule is not one.
    let src = "contract c with p\n  45 USD monthly from x\n  - 2 USD #fee\n  5%\n  y 3 USD\n";
    let file = parse_clean(src);
    let contract: &Contract = file.iter().next().unwrap();
    assert_eq!((contract.body.legs.len(), contract.body.items.len()), (1, 2));
    assert!(contract.schedule.is_some());
}

#[test]
fn via_names_the_party_it_went_through() {
    let file = parse_clean("2026-03-20 checking -> etsy-seller 20 USD via paypal ^x\n");
    let kinds = clauses(&file, txns(&file)[0].flow.tail);
    assert!(matches!(kinds[..], [ClauseKind::Via(Name("paypal")), ClauseKind::Code(_)]));
    only_error("2026-03-20 a -> b 5 USD via\n", "expected-party");
}

#[test]
fn a_v3_party_after_a_slash_says_how_it_is_written_now() {
    // The end went through the party: swap them.
    let src = "2026-03-20 checking -> paypal 20 USD / etsy-seller ^x\n";
    let error = only_error(src, "v3-party");
    assert_eq!(&src[error.anchor().unwrap().range()], "/ etsy-seller");
    assert_eq!(first_fix(src, &error), ("paypal 20 USD / etsy-seller", "etsy-seller 20 USD via paypal"));

    // An asset is bought, not paid through: the second fix says so.
    let src = "2026-03-24 visa 1_739.13 USD -> laptop / best-buy\n";
    let error = only_error(src, "v3-party");
    let fixes: Vec<(&str, &str)> = error.help.iter().filter_map(|help| help.edit.as_ref()).map(|(loc, text)| (&src[loc.range()], &**text)).collect();
    assert_eq!(fixes[0], ("laptop / best-buy", "best-buy via laptop"));
    assert_eq!(fixes[1], ("visa 1_739.13 USD -> laptop / best-buy", "visa -> best-buy 1_739.13 USD #purchase of laptop"));
    assert!(error.help[1].text.contains("if `laptop` is an asset"));
    // The same when the amount is on the target.
    let src = "2026-03-24 visa -> laptop 1_739.13 USD / best-buy\n";
    let error = only_error(src, "v3-party");
    assert_eq!(first_fix(src, &error).1, "best-buy 1_739.13 USD via laptop");

    // Where there is nothing to swap, or the slash is late, it is explained without an edit.
    for src in ["2026-03-24 visa 5 USD -> ? / x\n", "2026-03-24 a -> b 5 USD ^c / x\n", "2026-03-24 a -> 5 USD\n  b 1 USD / x\n"] {
        let error = only_error(src, "v3-party");
        assert!(error.help.iter().all(|help| help.edit.is_none()) || src.contains("? /"), "{src}");
    }
    only_error("2026-03-24 a -> b 5 USD / 5\n", "expected-party");
}

#[test]
fn contracts_may_leave_out_their_party_and_declarations_take_the_new_lines() {
    let file = parse_clean("contract netflix\n  15 USD monthly on 22 from visa\ncontract flat with greystar\n  about 2_900 USD monthly on 1 from checking #rent\n");
    let contracts: Vec<&Contract> = file.iter().collect();
    assert_eq!((contracts[0].name.0, contracts[0].party), ("netflix", None));
    assert_eq!(contracts[1].party.map(|party| party.0), Some("greystar"));
    assert!(contracts[1].schedule.unwrap().terms.about && !contracts[0].schedule.unwrap().terms.about);
    // A party written without `with` is asked for.
    let src = "contract flat greystar\n  5 USD monthly from x\n";
    assert_eq!(first_fix(src, &only_error(src, "expected-with")), ("", "with "));
    only_error("contract flat with\n", "expected-name");

    // An entity may carry its own purpose, with or without a kind.
    let file = parse_clean("entity farmers-market #groceries\nentity a, b : grocer #groceries\nentity c : person\n");
    let decls: Vec<&Decl> = file.iter().collect();
    let purposes: Vec<Option<&str>> = decls.iter().map(|decl| decl.purpose.map(|purpose| purpose.0)).collect();
    assert_eq!(purposes, [Some("groceries"), Some("groceries"), Some("groceries"), None]);
    only_error("account a : bank #groceries\n", "expected-end-of-line");

    // A budget's limit is an amount or a share of another purpose's total, and it may carry.
    let file = parse_clean("budget food 900 USD monthly\nbudget fun 10% of #income monthly carries\nbudget travel 3_000 USD yearly carries\n");
    let budgets: Vec<&Budget> = file.iter().collect();
    assert!(matches!(budgets[1].allowance.limit, Limit::Share { percent, of: Name("income") } if percent.mantissa == 10));
    assert_eq!(budgets.iter().map(|budget| budget.allowance.carries).collect::<Vec<_>>(), [false, true, true]);
    assert_eq!(budgets[2].allowance.per, Period::Year);
    only_error("budget fun 10% monthly\n", "expected-of");
    only_error("budget fun 10% of income monthly\n", "expected-purpose");
}

#[test]
fn a_sync_names_its_source_and_takes_generic_lines() {
    let src = "/// Chase's export.\nsync checking\n  run chase-export checking --since {since}\n  csv date \"Posting Date\" \"MM/DD/YYYY\", amount 4 flipped, memo 3\n  into prices/{year}.ax\n";
    let file = parse_clean(src);
    let ItemKind::Sync(id) = file.items[0].kind else { panic!("a sync") };
    let sync = &file[id];
    assert_eq!((sync.name.0, sync.run.0), ("checking", "chase-export checking --since {since}"));
    assert_eq!(sync.into.map(|text| text.0), Some("prices/{year}.ax"));
    let csv = &file[sync.props][0];
    let words: Vec<&str> = file[csv.args].iter().map(|&arg| &src[file.exprs[arg].loc.range()]).collect();
    assert_eq!(words, ["date", "\"Posting Date\"", "\"MM/DD/YYYY\"", "amount", "4", "flipped", "memo", "3"]);
    assert!(file.items[0].doc.is_some());

    only_error("sync checking\n  csv date 1\n", "missing-run");
    only_error("sync checking\n  run a\n  run b\n", "duplicate-clause");
    only_error("sync checking\n  run a\n  into x\n  into y\n", "duplicate-clause");
    // v3 named the file, which is `into` now.
    let src = "sync prices/2026.ax\n  run python3 fetch.py\n";
    let error = only_error(src, "sync-file");
    assert_eq!(first_fix(src, &error), ("prices/2026.ax", "prices\n  into prices/2026.ax"));
}

// ─── Tokens ─────────────────────────────────────────────────────────────────

#[test]
fn digit_initial_words_are_classified_by_shape() {
    let cases: [(&str, Tok); 12] = [
        ("2026-01-15", Tok::Date(day(2026, 1, 15))),
        ("2026-01", Tok::Month(day(2026, 1, 1))),
        ("84.20", number(8420, 2)),
        ("24_500", number(24500, 0)),
        ("3.5%", Tok::Percent(Dec { mantissa: 35, scale: 1 })),
        ("59y6m", Tok::Span(Span::months(714))),
        ("2w", Tok::Span(Span::days(14))),
        ("401k", Tok::Name("401k")),
        ("529", number(529, 0)),
        ("04-15", Tok::MonthDay(4, 15)),
        ("27.5y", Tok::Span(Span::months(330))),
        ("1.25y", Tok::Span(Span::months(15))),
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
        ("27.33y", Malformed::Span),
        ("#", Malformed::Mark),
        ("^-x", Malformed::Mark),
    ];
    for (src, expected) in malformed {
        assert_eq!(tokens(src), [Tok::Invalid(expected)], "{src}");
    }
}

#[test]
fn hyphens_and_slashes_join_names_but_arrows_and_comments_do_not() {
    assert_eq!(tokens("trader-joes"), [Tok::Name("trader-joes")]);
    assert_eq!(tokens("a - b"), [Tok::Name("a"), Tok::Punct(Punct::Minus), Tok::Name("b")]);
    assert_eq!(tokens("a->b"), [Tok::Name("a"), Tok::Punct(Punct::Arrow), Tok::Name("b")]);
    assert_eq!(tokens("assets/bank/*"), [Tok::Name("assets/bank/*")]);
    assert_eq!(tokens("BRK.B"), [Tok::Unit("BRK.B")]);
    assert_eq!(tokens("5 USD // note"), [number(5, 0), Tok::Unit("USD")]);
    assert_eq!(tokens("a//b"), [Tok::Name("a"), Tok::Punct(Punct::Slash), Tok::Punct(Punct::Slash), Tok::Name("b")]);
    assert_eq!(tokens("a/ b"), [Tok::Name("a"), Tok::Punct(Punct::Slash), Tok::Name("b")]);
    assert_eq!(tokens("a/B"), [Tok::Invalid(Malformed::Word)]);
    assert_eq!(tokens("2026..2027"), [number(2026, 0), Tok::Punct(Punct::DotDot), number(2027, 0)]);
    assert_eq!(tokens("=> →"), [Tok::Punct(Punct::Arrow), Tok::Punct(Punct::Arrow)]);
    assert_eq!(tokens("^check-1041"), [Tok::Code(Code("^check-1041"))]);
}

#[test]
fn purposes_and_codes_have_their_own_marks() {
    assert_eq!(tokens("#groceries"), [Tok::Purpose(Name("groceries"))]);
    assert_eq!(tokens("#a/b-c ^inv:2026.01"), [Tok::Purpose(Name("a/b-c")), Tok::Code(Code("^inv:2026.01"))]);
    assert_eq!(tokens("#repair of"), [Tok::Purpose(Name("repair")), Tok::Name("of")]);
    // A purpose takes what a code does, and only the model can say it names none.
    assert_eq!(tokens("#a:b"), [Tok::Purpose(Name("a:b"))]);
    assert_eq!(tokens("#Groceries"), [Tok::Invalid(Malformed::Word)]);
    assert_eq!(tokens("^Inv"), [Tok::Invalid(Malformed::Word)]);
    let src = "2026-01-01 a -> b 5 USD #Food\n";
    assert_eq!(first_fix(src, &only_error(src, "mixed-case")), ("#Food", "#food"));
    let src = "2026-01-01 a -> b 5 USD ^Inv-1\n";
    assert_eq!(first_fix(src, &only_error(src, "mixed-case")), ("^Inv-1", "^inv-1"));
}

// ─── Dates and the file's place ─────────────────────────────────────────────

const MARCH: Folder = Folder { year: Some(2026), month: Some(3) };
const YEAR: Folder = Folder { year: Some(2026), month: None };

/// The dates of a file's transactions, parsed in `place`.
fn dates_in(place: Folder, src: &str) -> Vec<Day> {
    let (file, diags) = crate::parse(FileId(0), src, place);
    assert!(diags.is_empty(), "unexpected diagnostics:\n{}", render(src, &diags));
    txns(&file).iter().map(|txn| txn.date).collect()
}

/// The one error a source parsed in `place` must produce.
fn place_error(place: Folder, src: &str, code: &str) -> Diagnostic {
    let mut errors: Vec<Diagnostic> = crate::parse(FileId(0), src, place).1.into_iter().collect();
    assert_eq!(errors.len(), 1, "expected exactly one diagnostic, got:\n{}", render(src, &errors));
    let error = errors.remove(0);
    assert_eq!(error.code, code, "{}", render(src, std::slice::from_ref(&error)));
    error
}

#[test]
fn a_folder_gives_a_year_and_a_month_by_its_path() {
    let of = |path| Folder::of(path);
    assert_eq!(of("journal/2026/03.ax"), MARCH);
    assert_eq!(of("journal/2026-03.ax"), MARCH);
    assert_eq!(of("2026/03/notes.ax"), Folder { year: Some(2026), month: Some(3) });
    assert_eq!(of("journal/2026.ax"), YEAR);
    assert_eq!(of("journal/2026/notes.ax"), YEAR);
    // A month is only ever right beneath its year, and a year is four digits.
    assert_eq!(of("journal/2026/x/03.ax"), YEAR);
    assert_eq!(of("journal/03.ax"), Folder::default());
    assert_eq!(of("journal/26/03.ax"), Folder::default());
    assert_eq!(of("journal/2026/13.ax"), YEAR);
    assert_eq!(of("journal/2026-13.ax"), Folder::default());
    assert_eq!(of("accounts.ax"), Folder::default());
}

#[test]
fn layout_free_is_gone_and_says_why() {
    let src = "layout free // folders stop giving dates\n";
    let error = only_error(src, "layout-is-gone");
    assert_eq!(first_fix(src, &error), ("layout free // folders stop giving dates", ""));
    assert!(error.notes[0].contains("heading"));
}

#[test]
fn an_items_date_may_leave_out_what_its_place_gives() {
    let lines = "15 a -> b 5 USD\n03-16 a -> b 5 USD\n2026-03-17 a -> b 5 USD\n";
    assert_eq!(dates_in(MARCH, lines), [day(2026, 3, 15), day(2026, 3, 16), day(2026, 3, 17)]);
    assert_eq!(dates_in(YEAR, "02-01 a -> b 5 USD\n2027-01-01 a -> b 5 USD\n"), [day(2026, 2, 1), day(2027, 1, 1)]);
    // A whole date is always allowed: whether it agrees with the file is for the model.
    assert_eq!(dates_in(MARCH, "2027-05-05 a -> b 5 USD\n"), [day(2027, 5, 5)]);
    assert_eq!(dates_in(Folder::default(), "2026-01-15 a -> b 5 USD\n"), [day(2026, 1, 15)]);

    // So is every other item that starts with one.
    let src = "opening 01\n  checking 5 USD\n07 FAST split 2 for 1\n";
    let (file, diags) = crate::parse(FileId(0), src, MARCH);
    assert!(diags.is_empty(), "{}", render(src, &diags));
    assert!(matches!(file.items[0].kind, ItemKind::Opening(id) if file[id].date == day(2026, 3, 1)));
    assert!(matches!(statements(&file)[0].predicate, Predicate::Split { .. }) && statements(&file)[0].date == day(2026, 3, 7));
}

#[test]
fn any_other_date_may_leave_out_the_year_when_the_place_gives_it() {
    let src = "2026-03-05 a -> b 5 USD due 04-01 for 01-01..03-31\n";
    let (file, diags) = crate::parse(FileId(0), src, YEAR);
    assert!(diags.is_empty(), "{}", render(src, &diags));
    let kinds = clauses(&file, txns(&file)[0].flow.tail);
    assert!(matches!(kinds[0], ClauseKind::Due(Due::On(d)) if *d == day(2026, 4, 1)));
    assert!(matches!(kinds[1], ClauseKind::For(For::Period(a, b)) if (*a, *b) == (day(2026, 1, 1), day(2026, 3, 31))));
    let src = "03-05..03-31 a -> b 5 USD\n";
    let (file, diags) = crate::parse(FileId(0), src, YEAR);
    assert!(diags.is_empty() && matches!(clauses(&file, txns(&file)[0].flow.tail)[..], [ClauseKind::For(_)]));
}

#[test]
fn a_short_due_date_counts_forward_from_its_line() {
    // No context is needed: `due 04-01` is the first April the 1st on or after the line's day.
    let due = |src: &str| {
        let (file, diags) = crate::parse(FileId(0), src, Folder::default());
        assert!(diags.is_empty(), "{}", render(src, &diags));
        let kinds = clauses(&file, txns(&file)[0].flow.tail);
        let ClauseKind::Due(Due::On(day)) = kinds[0] else { panic!("a due date") };
        *day
    };
    assert_eq!(due("2026-01-12 a -> b 5 USD due 04-01\n"), day(2026, 4, 1));
    assert_eq!(due("2026-04-01 a -> b 5 USD due 04-01\n"), day(2026, 4, 1), "on the day itself");
    assert_eq!(due("2026-12-20 a -> b 5 USD due 01-05\n"), day(2027, 1, 5), "next year");
    assert_eq!(due("2026-12-20 a -> b 5 USD due 2026-12-01\n"), day(2026, 12, 1), "a whole date is as written");
    assert_eq!(due("2027-01-01 a -> b 5 USD due 02-29\n"), day(2028, 2, 29), "waits for a leap year");
    let src = "2026-01-01 a -> b 5 USD due 02-30\n";
    assert_eq!(first_fix(src, &only_error(src, "bad-date")), ("02-30", "02-28"));
}

#[test]
fn a_heading_gives_the_lines_below_it_their_year_or_month() {
    let src = "\
2026-02
15 a -> b 5 USD
2026-03-01 a -> b 5 USD
03 a -> b 5 USD
2027
02-01 a -> b 5 USD
2026-04 // April again
30 a -> b 5 USD
";
    assert_eq!(
        dates_in(Folder::default(), src),
        [day(2026, 2, 15), day(2026, 3, 1), day(2026, 2, 3), day(2027, 2, 1), day(2026, 4, 30)]
    );
    // A heading replaces what the folder said, month included.
    let src = "2026-04\n03 a -> b 5 USD\n2027\n03 a -> b 5 USD\n";
    let errors = crate::parse(FileId(0), src, MARCH).1;
    assert_eq!(errors.iter().map(|error| &*error.code).collect::<Vec<_>>(), ["short-date"]);
    assert_eq!(&src[errors[0].anchor().unwrap().range()], "03");
    assert_eq!(errors[0].anchor().unwrap().start as usize, src.rfind("03 a").unwrap(), "the one under `2027`");
    // What is not a heading is not one: a date, a heading with words, an impossible month.
    for (src, code) in [("2026-13\n", "bad-date"), ("2026-03 x\n", "expected-item"), ("2026 x\n", "expected-date")] {
        assert_eq!(crate::parse(FileId(0), src, Folder::default()).1[0].code, code, "{src}");
    }
    // It documents nothing, and says so.
    let (_, diags) = crate::parse(FileId(0), "/// Not a doc.\n2026\n", Folder::default());
    assert_eq!(diags[0].code, "misplaced-doc");
}

#[test]
fn a_short_date_where_the_place_does_not_give_the_rest_is_an_error() {
    // (place, source, what the date leaves out)
    let cases = [
        (Folder::default(), "15 a -> b 5 USD\n", "year"),
        (YEAR, "15 a -> b 5 USD\n", "month"),
        (Folder::default(), "03-15 a -> b 5 USD\n", "year"),
        (Folder::default(), "2026-03-15 a -> b 5 USD for 04-01\n", "year"),
    ];
    for (place, src, missing) in cases {
        let error = place_error(place, src, "short-date");
        assert!(error.message.ends_with(&format!("leaves out the {missing}, which no heading above it and no folder gives")));
        let written = &src[error.anchor().unwrap().range()];
        assert!(error.message.starts_with(&format!("`{written}`")), "{}", error.message);
    }
    // The label is on the date itself, in the middle of the line too.
    let src = "2026-03-15 a -> b 5 USD for 04-01\n";
    assert_eq!(&src[place_error(Folder::default(), src, "short-date").anchor().unwrap().range()], "04-01");
}

#[test]
fn a_short_date_that_is_not_on_the_calendar_says_so_in_its_own_form() {
    let february = Folder { year: Some(2026), month: Some(2) };
    let cases = [
        (february, "30 a -> b 5 USD\n", "30", "28"),
        (YEAR, "02-29 a -> b 5 USD\n", "02-29", "02-28"),
        (YEAR, "04-31 a -> b 5 USD\n", "04-31", "04-30"),
        (february, "00 a -> b 5 USD\n", "00", ""),
        (YEAR, "13-45 a -> b 5 USD\n", "13-45", ""),
    ];
    for (place, src, written, last) in cases {
        let error = place_error(place, src, "bad-date");
        assert_eq!(&src[error.anchor().unwrap().range()].len(), &2, "{}", render(src, &[error.clone()]));
        match last {
            "" => assert!(error.help.is_empty()),
            last => assert_eq!(first_fix(src, &error), (written, last)),
        }
    }
    assert_eq!(place_error(february, "30 a -> b 5 USD\n", "bad-date").message, "February 2026 has 28 days");
    assert_eq!(place_error(YEAR, "13-45 a -> b 5 USD\n", "bad-date").message, "`13-45` is not a date: there is no month 13");
}

/// The fast paths for plain words decline whatever would make them longer or
/// different, and leave it to the general path.
#[test]
fn a_plain_word_ends_where_the_general_reading_says() {
    assert_eq!(tokens("5. x"), [number(5, 0), Tok::Punct(Punct::Dot), Tok::Name("x")]);
    assert_eq!(tokens("84.20 USD"), [number(8420, 2), Tok::Unit("USD")]);
    assert_eq!(tokens("10%"), [Tok::Percent(Dec { mantissa: 10, scale: 0 })]);
    assert_eq!(tokens("1_000 USD"), [number(1000, 0), Tok::Unit("USD")]);
    assert_eq!(tokens("123456789012345678901"), [Tok::Invalid(Malformed::Number)]);
    assert_eq!(tokens("USD ->"), [Tok::Unit("USD"), Tok::Punct(Punct::Arrow)]);
    assert_eq!(tokens("USD2"), [Tok::Unit("USD2")]);
    assert_eq!(tokens("USD/x"), [Tok::Invalid(Malformed::Word)]);
    assert_eq!(tokens("a-/b->"), [Tok::Name("a-/b"), Tok::Punct(Punct::Arrow)]);
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
        | ExprKind::Purpose(_)
        | ExprKind::Code(_) => vec![],
        ExprKind::Field(base, _) | ExprKind::Unary(_, base) => vec![*base],
        ExprKind::Index(base, keys) => [*base].into_iter().chain(file[*keys].iter().copied()).collect(),
        ExprKind::Call(_, args) => file[*args].to_vec(),
        ExprKind::Binary(_, lhs, rhs) | ExprKind::Of(lhs, rhs) => vec![*lhs, *rhs],
        ExprKind::Is(lhs, alternatives) => [*lhs].into_iter().chain(file[*alternatives].iter().copied()).collect(),
        ExprKind::If(condition, then, otherwise) => vec![*condition, *then, *otherwise],
        ExprKind::Schedule(rows) => file[*rows].iter().flat_map(|row| [row.threshold, row.rate]).collect(),
    }
}

fn effect_roots(effect: &Effect) -> Vec<ExprId> {
    match effect {
        Effect::Owe { amount, due, .. } => [*amount].into_iter().chain(*due).collect(),
        Effect::Count { amount, .. } | Effect::Consume(amount) | Effect::Carry { amount, .. } => vec![*amount],
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
    for contract in file.iter::<Contract>() {
        out.extend(file[contract.props].iter().flat_map(|prop| file[prop.args].iter().copied()));
    }
    for sync in file.iter::<Sync>() {
        out.extend(file[sync.props].iter().flat_map(|prop| file[prop.args].iter().copied()));
    }
    for said in file.iter::<Statement>() {
        if let Predicate::Property(prop) = &said.predicate {
            out.extend(file[prop.args].iter().copied());
        }
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
        let id = ExprId::new(0, index);
        let node = &exprs[id];
        let mut next = node.first.local();
        for child in children(file, &node.kind) {
            let first = exprs[child].first.local();
            assert_eq!(first, next, "child {child:?} of node {index} does not follow its sibling");
            next = child.local() + 1;
        }
        assert_eq!(next, id.local(), "node {index}: its subtree does not end at it");
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

/// What a damaged file's changed bytes are taken from: punctuation and signs.
const ALPHABET: &[u8] = b"-/\"(),:= \n%|^#.0+[]@!?*\t";

/// Sources damaged by a few changed bytes: `source`, again and again.
fn damaged(source: &str, count: usize, mut with: impl FnMut(&str)) {
    let mut state = 0x2545_F491_4F6C_DD1Du64;
    let mut random = |below: usize| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state % below as u64) as usize
    };
    for _ in 0..count {
        let mut bytes = source.as_bytes().to_vec();
        for _ in 0..1 + random(4) {
            let at = random(bytes.len());
            match random(3) {
                0 => bytes[at] = ALPHABET[random(ALPHABET.len())],
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
    damaged(EXAMPLE, 2_000, |src| {
        let (file, diags) = parse(FileId(0), src);
        assert_post_order(&file, !diags.iter().any(Diagnostic::is_error));
    });
}

/// The same of every file of the sketch, in every folder: short dates and the
/// forms of the second wave (statements, items, headings, syncs) are where a
/// damaged file has the most to go wrong, and none may panic.
#[test]
fn damaged_sketch_files_keep_the_arena_well_formed_in_any_folder() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/v4-sketch");
    for path in ax_files(&root, &root) {
        let src = std::fs::read_to_string(root.join(&path)).unwrap();
        for folder in [Folder::default(), YEAR, MARCH, Folder::of(path.to_str().unwrap())] {
            damaged(&src, 100, |text| {
                let (file, diags) = crate::parse(FileId(0), text, folder);
                assert_post_order(&file, !diags.iter().any(Diagnostic::is_error));
            });
        }
    }
}

/// A fully parenthesised rendering of an expression.
fn show(file: &File, id: ExprId, src: &str) -> String {
    let exprs = &file.exprs;
    match &exprs[id].kind {
        ExprKind::Name(text) | ExprKind::Unit(text) => text.0.to_string(),
        ExprKind::Num(_) | ExprKind::Amount(_) | ExprKind::Span(_) => src[exprs[id].loc.range()].to_string(),
        ExprKind::Unary(UnOp::Neg, x) => format!("(-{})", show(file, *x, src)),
        ExprKind::Unary(UnOp::Not, x) => format!("(not {})", show(file, *x, src)),
        ExprKind::Binary(op, a, b) => format!("({} {} {})", show(file, *a, src), op.symbol(), show(file, *b, src)),
        ExprKind::Is(x, alts) => {
            let alts: Vec<String> = file[*alts].iter().map(|&a| show(file, a, src)).collect();
            format!("({} is {})", show(file, *x, src), alts.join(" | "))
        }
        ExprKind::Field(base, name) => format!("{}.{}", show(file, *base, src), name.0),
        ExprKind::Purpose(name) => format!("#{}", name.0),
        ExprKind::Code(code) => code.0.to_string(),
        ExprKind::Of(purpose, object) => format!("({} of {})", show(file, *purpose, src), show(file, *object, src)),
        ExprKind::Call(name, args) => {
            let args: Vec<String> = file[*args].iter().map(|&arg| show(file, arg, src)).collect();
            format!("{}({})", name.0, args.join(", "))
        }
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
fn expressions_name_purposes_codes_and_last_years_tallies() {
    assert_eq!(condition("purpose is #groceries | ^inv-12"), "(purpose is #groceries | ^inv-12)");
    // A purpose takes its object with `of`, in a bare name or a marked one.
    assert_eq!(condition("purpose is repair of self | #insurance"), "(purpose is (repair of self) | #insurance)");
    assert_eq!(condition("purpose is #improvement of self.owner"), "(purpose is (#improvement of self.owner))");
    assert_eq!(condition("tally(x, year - 1) > 0 USD"), "(tally(x, (year - 1)) > 0 USD)");
    assert_eq!(condition("straight-line(cost, 27.5y, from)"), "straight-line(cost, 27.5y, from)");
}

#[test]
fn laws_may_fire_on_flows_and_consume_or_carry() {
    let file = parse_clean(EXAMPLE);
    let laws: Vec<&Law> = file.iter().collect();
    let rental = laws.iter().find(|law| law.name.0 == "rental-expenses").unwrap();
    assert_eq!(rental.trigger, Trigger::Flow);
    let steps = &file[rental.steps];
    assert!(matches!(steps[2].kind, StepKind::Effect(Effect::Consume(_))));
    let wash = laws.iter().find(|law| law.name.0 == "wash-sale").unwrap();
    let StepKind::Require { otherwise: Some(Effect::Carry { to, within, .. }), .. } = file[wash.steps][1].kind else {
        panic!("a require that carries a loss")
    };
    assert_eq!((to.0, within), ("VTI", Span::days(30)));

    only_error("law l\n  on flow\n  consume\n", "expected-expression");
    only_error("law l\n  on flow\n  carry a to VTI\n", "expected-keyword");
    only_error("law l\n  on flow\n  carry a to VTI within soon\n", "expected-span");
    only_error("law l\n  on flow\n  carry a within 30d\n", "expected-to");
    only_error("law l\n  on flws\n", "unknown-trigger");
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
account food : expense
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
    assert!(size_of::<Item>() <= 48, "Item is {}", size_of::<Item>());
    assert!(size_of::<Txn>() <= 160, "Txn is {}", size_of::<Txn>());
    assert!(size_of::<Leg>() <= 128, "Leg is {}", size_of::<Leg>());
    assert!(size_of::<Clause>() <= 56, "Clause is {}", size_of::<Clause>());
    assert!(size_of::<Expr>() <= 48, "Expr is {}", size_of::<Expr>());
    assert!(size_of::<Statement>() <= 168, "Statement is {}", size_of::<Statement>());
    assert!(size_of::<LineItem>() <= 96, "LineItem is {}", size_of::<LineItem>());
    assert!(size_of::<Many<Leg>>() == 8 && size_of::<Amount>() == 16 && size_of::<Name>() == 16);
}

/// Everything an item reaches, written out with every range and id followed,
/// so that two files are equal when they say the same, whichever pieces their
/// tables are kept in.
fn dump(file: &File) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let end = |end: &End| format!("{:?}{:?}", end.name, &file[end.select]);
    let tail = |tail: Many<Clause>| format!("{:?}", &file[tail]);
    let body = |body: Body| {
        let leg = |leg: &Leg| format!("{:?} {} {:?} {} {:?}", leg.doc, end(&leg.end), leg.amount, tail(leg.tail), leg.loc);
        let item = |item: &LineItem| {
            format!("{:?} {:?} {:?} {} {:?}", item.doc, item.sign, item.amount, tail(item.tail), item.loc)
        };
        let legs: Vec<String> = file[body.legs].iter().map(leg).collect();
        let items: Vec<String> = file[body.items].iter().map(item).collect();
        format!("{} / {}", legs.join("; "), items.join("; "))
    };
    let flow = |flow: &Flow| {
        let side = |side: &Side| format!("{:?} {:?}", side.end.as_ref().map(end), side.amount);
        format!("{} -> {} {} [{}]", side(&flow.from), side(&flow.to), tail(flow.tail), body(flow.body))
    };
    let expr = |root: ExprId| {
        let nodes = file.exprs.subtree(root);
        let leaves = nodes.iter().map(|node| match children(file, &node.kind).is_empty() {
            true => format!("{:?} {:?}", node.kind, node.loc),
            false => format!("{:?} first={}", node.loc, node.first.offset_from(nodes[0].first)),
        });
        leaves.collect::<Vec<_>>().join(" ")
    };
    let prop = |prop: &Prop| format!("{:?} {:?}", prop.name, file[prop.args].iter().map(|&arg| expr(arg)).collect::<Vec<_>>());
    let props = |props: Many<Prop>| -> Vec<String> { file[props].iter().map(prop).collect() };
    let terms = |t: &Terms| {
        format!("{:?} {:?} {:?} {:?} {:?} {:?}", t.about, t.payment, t.cadence, &file[t.on], t.holding, t.purpose)
    };
    let statement = |said: &Statement| {
        let predicate = match &said.predicate {
            Predicate::Terms(id) => terms(&file[*id]),
            Predicate::Property(found) => prop(found),
            other => format!("{other:?}"),
        };
        let codes = format!("{:?}", &file[said.codes]);
        format!("{:?} {:?} {predicate} {:?} {:?} {codes} {}", said.date, said.subject, said.until, said.description, body(said.body))
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
            ItemKind::Statement(id) => statement(&file[id]),
            ItemKind::Setting(id) => format!("{:?}", file[id]),
            ItemKind::Sync(id) => {
                let sync = &file[id];
                format!("{:?} {:?} {:?} {:?}", sync.name, sync.run, sync.into, props(sync.props))
            }
            ItemKind::Budget(id) => format!("{:?}", file[id]),
            ItemKind::Opening(id) => {
                let claims: Vec<String> = file[file[id].claims].iter().map(statement).collect();
                format!("{} {claims:?}", body(Body { legs: file[id].lines, items: Many::EMPTY }))
            }
            ItemKind::Contract(id) => {
                let Contract { name, party, schedule, props: lines, body: template, laws, damaged } = &file[id];
                let schedule = schedule.map(|s| format!("{:?} {} {:?}", s.at, terms(&s.terms), s.description));
                let laws: Vec<String> = file[*laws].iter().map(law).collect();
                format!("{name:?} {party:?} {schedule:?} {:?} {} {laws:?} {damaged}", props(*lines), body(*template))
            }
            ItemKind::Code(id) => format!("{:?} {:?}", file[id].pattern, &file[file[id].on]),
            ItemKind::Law(id) => law(&file[id]),
            ItemKind::Param(id) => {
                let rows = file[file[id].rows].iter();
                let rows: Vec<String> = rows.map(|row| format!("{:?} {}", &file[row.keys], expr(row.value))).collect();
                format!("{:?} {rows:?}", file[id].name)
            }
            ItemKind::Decl(id) => {
                let decl = &file[id];
                let laws: Vec<String> = file[decl.laws].iter().map(law).collect();
                format!("{:?} {:?} {:?} {:?} {:?} {laws:?}", decl.name, decl.at, decl.purpose, decl.kind, props(decl.props))
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
            let (dumped, found) = (dump(&file), format!("{diags:?}"));
            let differs = dumped.lines().zip(expected.0.lines()).find(|(a, b)| a != b);
            assert_eq!(differs, None, "{pieces} pieces of\n{src}");
            assert_eq!((dumped, found), expected, "{pieces} pieces of\n{src}");
        }
    };
    same(EXAMPLE);
    same(&EXAMPLE.repeat(3));
    same(
        "/// Kept with the item below, across a cut.\n\n// and a comment\n2026-01-01 a -> b 5 USD\n\n/// Two.\n/// Lines.\nlaw l\n  on in\n",
    );
    damaged(EXAMPLE, 300, same);
    same(&"2026-01-01 a -> b 5 USD\n2026-01-02 a -> b 6 USD\n\tstray\n2026-01-03 a -> b 7 USD\n".repeat(50));
    // A heading reaches the lines below it, in whichever piece they are.
    let months = (1..=12).map(|month| format!("2026-{month:02}\n01 a -> b 5 USD\n// a note\n15 a -> b 6 USD due 02-01\n"));
    let headed = months.collect::<String>() + "2027\n03-04 a -> b 7 USD\n";
    same(&headed);
    same(&headed.repeat(3));
    damaged(&headed, 300, same);
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
