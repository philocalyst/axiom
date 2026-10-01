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
  share 50% for studio
  budget 400 USD monthly carries
  also + 5% of amount #tip when amount > 20 USD
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
2026-01-02 VTI = 280.14 USD
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
  also lumen -> retirement 50% of [retirement] up to 3% of amount #match

contract flat with greystar
  2_900 USD monthly on 1 from checking
  share 12% for studio
  until 2026-08-31
  due 5d else + 5% #late-fee

contract mortgage with rocket
  loan 320_000 USD on 2024-02-20 at 5.875% over 30y for condo
    resets 1y from 2029-03-01 to sofr + 2.5% cap 2% life 5%
    prepay recasts
  monthly on 1 from checking
  also -> escrow 410 USD #escrow

contract lease with dana
  2_350 USD monthly on 1 into checking #rent of condo
  deposit 2_350 USD into deposits
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

sync checking
  read "imports/chase-*.csv"
  format csv
    date "Date" "YYYY-MM-DD"
    amount 3 flipped
    memo 2

sync prices
  run python3 fetch_prices.py --symbol VTI // a comment
  into prices/{year}.ax

format ofx
  records STMTTRN
  date DTPOSTED "YYYYMMDD"
  amount TRNAMT
  memo NAME, MEMO

pattern ach = "ACH " ("DEBIT" / "CREDIT") space+

entity paypal : processor
  known-as "PAYPAL *" payee:rest, "PP*" payee:rest
"#;

/// One statement of every verb, and of every form of amount that LANGUAGE §4 writes.
const STATEMENTS: &str = r#"
2026-01-31 checking = 8_828.87 USD
2026-01-31 visa = 2_333.99 USD
2026-01-31 retirement = 58_420.18 USD via market
2026-01-30 me = 45.15 USD !
2026-01-02 VTI = 280.14 USD
2026-01-01 ^bldg-water = 155.00 USD "the building's water bill, Q1"
2026-01-31 ^odometer = 48_210 MI
2026-01-12 me worked 6.5 HR for halcyon ^inv-12
2026-01-21 car used 44 MI #business-travel for studio
2026-07-01 flat now 3_050 USD monthly "renewed at 3,050"
2026-03-01 gym now 120 USD monthly until 05-31 ^promo "spring promotion"
2026-05-28 ^promo now until 08-31 "extended"
2026-07-01 flat now share 20% for studio
2029-03-01 mortgage now at 6.25%
2026-06-15 me now lives us/ny
2026-12-01 #food now budget 1_200 USD monthly until 12-31 "the holidays"
2026-04-20 ^inv-12 now due 05-15
2026-01-12 ^inv-9 now "credit note CN-0002"
  - 900.00 USD #design
  - 93.15 USD #sales-tax-collected
2026-08-16 job now 4_400 USD twice monthly
  ftb   empty
  dtf   190.40 USD
2026-12-01 flat waived "December free: greystar's gift"
2026-04-01 gym waived until 06-30 "frozen while abroad"
2026-09-30 ^inv-9 waived "written off"
2026-09-30 ^inv-9 waived #bad-debt "written off"
  600.00 USD #design
2026-10-10 netflix ends
2026-10-06 ^check-1041 settled
2026-10-20 ^check-1044 void
2026-10-04 ^deposit-77 returned
2026-10-22 FAST split 2 for 1
2026-05-01 car basis 12_000 USD since 2019-03-01
2026-04-01 me filed 2025
  wages 124_200.00 USD
  federal-tax 22_000 USD
2026-04-05 me owes pge 142.50 USD #utilities ^pge-jan
2026-04-05 jo owes me 1/3 of ^pge-jan
2026-04-27 halcyon owes studio due 30d ^inv-12
  ^inv-12[HR] @ 150 USD/HR #design
  + 8.625% of ^inv-12[#design] #sales-tax
2026-03-15 halcyon owes studio 1.5% of ^inv-12 #late-fee
2026-04-01 flat
  + 12% of ^bldg-water #utilities
2026-04-02 flat
  water = 155.00 USD
2026-04-08 phone 47.30 USD
2026-04-20 vti-monthly 1.620 VTI
2026-04-15 estimates 8_800 USD for 2025
2026-04-01 halcyon -> checking 3_800 USD ^inv-12 against ^inv-11
2026-05-05 checking -> pge 90 USD for last month
2026-04-24 visa -> delta 420 USD for lumen
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

/// Every line of the v4 sketch parses without a diagnostic.
#[test]
fn the_v4_sketch_parses_clean() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/v4-sketch");
    let files = ax_files(&root, &root);
    assert_eq!(files.len(), 11, "{files:?}");
    let mut items = 0;
    for path in &files {
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
    assert_eq!(file.items.len(), 81);
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
    assert!(matches!(terms.payment, Some(Payment::Fixed(Amount::Literal(amount))) if amount.0 == "4_600 USD"));
    assert_eq!((terms.cadence, &file[terms.on]), (Cadence::TwiceMonthly, &[On::MonthDay(15), On::Last][..]));
    let holding = terms.holding.unwrap();
    assert_eq!((holding.direction, holding.name.0), (Direction::Into, "checking"));
    assert_eq!((job.purpose.map(|purpose| purpose.name.0), job.description.map(|text| text.0)), (Some("wages"), Some("gross")));
    let legs = &file[job.body.legs];
    assert!(matches!(legs[0].amount, Quantity::Amount(Amount::Computed(root)) if matches!(file.exprs[root].kind, ExprKind::Pct(Dec { mantissa: 6, scale: 0 }))));
    assert_eq!(legs.len(), 2);
    assert_eq!(legs[0].doc.unwrap().lines().collect::<Vec<_>>(), ["Six percent of the gross is deferred."]);
    // The match is an `also` flow, whose amount is an expression over the flow.
    let alsos = &file[job.alsos];
    let (AlsoLine::Flow(flow), None) = (&alsos[0].line, alsos[0].when) else { panic!("a flow that always comes") };
    assert!(flow.from.end.is_some_and(|end| end.name.0 == "lumen") && flow.to.end.is_some_and(|end| end.name.0 == "retirement"));
    let Some(Quantity::Amount(Amount::Computed(root))) = flow.to.amount else { panic!("a computed amount") };
    assert!(matches!(file.exprs[root].kind, ExprKind::Binary(BinOp::UpTo, ..)));
    assert!(props(0).is_empty() && alsos.len() == 1);

    // The flat's properties and its deadline; and a loan whose schedule has no amount.
    assert_eq!(props(1), [line("share", &["12%", "for", "studio"]), line("until", &["2026-08-31"])]);
    let deadline = contracts[1].deadline.as_ref().unwrap();
    assert_eq!(deadline.span, Span::days(5));
    assert!(deadline.otherwise.as_ref().is_some_and(|item| item.sign == Sign::Add && matches!(item.amount, Amount::Computed(root) if matches!(file.exprs[root].kind, ExprKind::Pct(_)))));
    assert!(contracts[2].schedule.is_some_and(|schedule| schedule.terms.payment.is_none()));
    let loan = ["320_000 USD", "on", "2024-02-20", "at", "5.875%", "over", "30y", "for", "condo"];
    assert_eq!(props(2), [line("loan", &loan)]);
    // What a loan says of its resets and prepayments is nested under it.
    let nested = &file[file[contracts[2].props][0].lines];
    let text = |arg: &ExprId| EXAMPLE[file.exprs[*arg].loc.range()].to_string();
    assert_eq!(nested.iter().map(|nested| nested.0.name.0).collect::<Vec<_>>(), ["resets", "prepay"]);
    let resets: Vec<String> = file[nested[0].0.args].iter().map(text).collect();
    assert_eq!(resets, ["1y", "from", "2029-03-01", "to", "sofr + 2.5%", "cap", "2%", "life", "5%"]);
    assert!(matches!(file[contracts[2].alsos][0].line, AlsoLine::Flow(_)));

    // A lease: a deposit into a holding, two properties on one line, a purpose with an object, and a law.
    let lease = contracts[3];
    assert_eq!(props(3), [line("deposit", &["2_350 USD", "into", "deposits"]), line("from", &["2025-07-01", "until", "2026-06-30"])]);
    let rent = lease.purpose.unwrap();
    assert_eq!((rent.name.0, rent.of.map(|of| of.0)), ("rent", Some("condo")));
    assert_eq!(file[lease.laws].len(), 1);

    // A day of every year, what a payment covers, a standing order and a fortnight.
    let insurance = contracts[4];
    assert_eq!(file[insurance.schedule.unwrap().terms.on], [On::YearDay { month: 3, day: 1 }]);
    assert_eq!(props(4), [line("covers", &["the", "year"])]);
    let buy = contracts[5].standing.unwrap().terms;
    assert!(matches!(buy.payment, Some(Payment::Buy { unit: Name("VTI"), spend: Amount::Literal(spend) }) if spend.0 == "500 USD"));
    assert!(contracts[5].schedule.is_none());
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
    let src = "contract c with p\n  5 USD monthly from x\n  retirement\n  share 5% for y\n";
    let (file, diags) = parse(FileId(0), src);
    assert_eq!(diags.iter().map(|diag| &*diag.code).collect::<Vec<_>>(), ["expected-amount"]);
    let contract: &Contract = file.iter().next().unwrap();
    assert!(contract.damaged && contract.schedule.is_some() && contract.props.len() == 1);

    let contract_with = |lines: &str| format!("contract c with p\n{lines}");
    let one = |lines: &str, code: &str| only_error(&contract_with(lines), code);
    one("  deposit 5 USD\n", "missing-schedule");
    one("  5 USD monthly from x\n  due 5d\n  due 6d\n", "duplicate-clause");
    one("  5 USD monthly from x\n  due 5\n", "expected-span");
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
fn a_chart_account_is_rejected_and_not_kept() {
    let src = "account income/salary : wages\n  owner me\naccount checking : bank at chase\n";
    let (file, diags) = parse(FileId(0), src);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, "chart-account");
    assert_eq!(diags[0].severity, Severity::Error);
    assert_eq!(&src[diags[0].anchor().unwrap().range()], "income/salary");
    assert!(diags[0].help.iter().any(|help| help.text.contains("#purpose")));
    assert_eq!(file.iter::<Decl>().count(), 1, "the chart account and its block are skipped");
    assert_eq!(file.iter::<Decl>().next().unwrap().name.0, "checking");

    // A slash remains valid in ordinary names, outside the three chart roots.
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

    // A command is raw text up to a comment, which a blank must precede.
    let file = parse_clean("sync prices // daily\n  run curl -s https://example.com/a // b\r\n  into prices/{year}.ax\n");
    let ItemKind::Sync(id) = file.items[0].kind else { panic!("a sync") };
    let sync = &file[id];
    assert_eq!((sync.name.0, sync.run.map(|run| run.0)), ("prices", Some("curl -s https://example.com/a")));
    assert_eq!(sync.into.map(|text| text.0), Some("prices/{year}.ax"));

    let file = parse_clean("sync paths\n  run echo https://example.com/a//b // trailing\n  into imports/a//b.ax\n");
    let ItemKind::Sync(id) = file.items[0].kind else { panic!("a sync") };
    let sync = &file[id];
    assert_eq!(sync.run.map(|run| run.0), Some("echo https://example.com/a//b"));
    assert_eq!(sync.into.map(|path| path.0), Some("imports/a//b.ax"));

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
    let Some(Quantity::Amount(Amount::Literal(amount))) = txn.flow.to.amount else { panic!("an amount") };
    assert_eq!(&src[file.loc(&amount).range()], "84.20 USD");
    assert_eq!((amount.num(), amount.unit().map(|unit| unit.0)), (Dec { mantissa: 8420, scale: 2 }, Some("USD")));
    let kinds = clauses(&file, txn.flow.tail);
    let (ClauseKind::Via(party), ClauseKind::Code(code)) = (kinds[0], kinds[1]) else { panic!("a party, a code") };
    assert_eq!((&src[file.loc(party).range()], &src[file.loc(code).range()]), ("trader-joes", "^groceries"));
}

#[test]
fn amounts_read_back_from_their_text() {
    let read = |text| (Literal(text).num(), Literal(text).unit().map(|unit| unit.0));
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
    assert!(matches!(kinds[4], ClauseKind::Basis(Amount::Literal(amount)) if amount.0 == "empty"));
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
    only_error("2026-01-01 a -> b 5 USD against ^a against ^b\n", "duplicate-clause");

    // `against` names the flow it is about, and `for last` the period before the day.
    let file = parse_clean("2026-04-15 a -> b 5 USD against ^inv-11 for last quarter\n");
    let kinds = clauses(&file, txns(&file)[0].flow.tail);
    assert!(matches!(kinds[0], ClauseKind::Against(Code("^inv-11"))));
    assert!(matches!(kinds[1], ClauseKind::For(For::Last(Relative::Quarter))));
    only_error("2026-04-15 a -> b 5 USD for last week\n", "unknown-period");
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
    assert!(exchange.to.end.is_none() && matches!(exchange.to.amount, Some(Quantity::Amount(_))) && exchange.body.legs.is_empty());
    // A commodity first is a party for a flow, and the subject of a price when `=` follows it.
    let file = parse_clean("2026-02-03 VTI = 280.14 USD\n");
    assert!(matches!(statements(&file)[0].verb, Verb::Value(Amount::Literal(price)) if price.0 == "280.14 USD"));
    // v3 wrote the price without the `=`.
    let src = "2026-02-03 VTI 280.14 USD\n";
    assert_eq!(first_fix(src, &only_error(src, "price-needs-equals")), ("", "= "));

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
    // A share alone is a node of the arena: the percent, of what the header says.
    assert!(matches!(legs[0].amount, Quantity::Amount(Amount::Computed(root)) if matches!(file.exprs[root].kind, ExprKind::Pct(Dec { mantissa: 6, scale: 0 }))));
    only_error("2026-01-31 lumen -> 6%\n", "expected-end-of-line");

    let file = parse_clean("opening 2026-01-01\n  condo basis 402_000 USD since 2024-02-20\n  checking 5 USD\n");
    let ItemKind::Opening(id) = file.items[0].kind else { panic!("an opening") };
    let lines = &file[file[id].lines];
    assert!(matches!(lines[0].amount, Quantity::Whole) && matches!(lines[1].amount, Quantity::Amount(_)));
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
    assert!(flow.to.end.is_none() && matches!(flow.to.amount, Some(Quantity::Amount(_))));
    assert_eq!(file[flow.body.legs].len(), 2);
}

#[test]
fn basis_is_an_asset_arriving_when_an_amount_follows_and_a_kinds_property_when_not() {
    let file = parse_clean("2026-05-01 car basis 12_000 USD since 2019-03-01\n2026-05-01 401k now basis zero\n");
    let said = statements(&file);
    assert!(matches!(said[0].verb, Verb::Basis { amount: Amount::Literal(amount), since: Some(_) } if amount.0 == "12_000 USD"));
    assert!(matches!(&said[1].verb, Verb::Now(Change::Property(prop)) if prop.name.0 == "basis"));
    // A property is restated with `now`; without it the word after the subject is a verb that takes an amount.
    only_error("2026-05-01 401k basis zero\n", "expected-amount");
}

#[test]
fn a_dated_name_is_an_occurrence_unless_it_says_something_else() {
    let file = parse_clean(
        "2026-01-16 paycheck\n2026-03-13 paycheck 5_900 USD\n  taxes 950 USD\n2026-01-31 paycheck = 5 USD\n",
    );
    let said = statements(&file);
    assert!(matches!(said[0].subject, Subject::Name(Name("paycheck"))));
    assert!(matches!(said[0].verb, Verb::Occurrence(None)) && said[0].body.legs.is_empty());
    assert!(matches!(said[1].verb, Verb::Occurrence(Some(Amount::Literal(amount))) if amount.0 == "5_900 USD"));
    assert_eq!(said[1].body.legs.len(), 1);
    assert!(matches!(said[2].verb, Verb::Value(_)));

    // A commodity amount is what was bought, and a contract may end.
    let file = parse_clean("2026-01-20 vti-monthly 1.620 VTI\n2026-03-10 netflix ends\n");
    let said = statements(&file);
    assert!(matches!(said[0].verb, Verb::Occurrence(Some(Amount::Literal(amount))) if amount.0 == "1.620 VTI"));
    assert!(matches!((&said[1].subject, &said[1].verb), (Subject::Name(Name("netflix")), Verb::Ends)));
    assert_eq!(said[1].date, day(2026, 3, 10));
    only_error("2026-03-10 netflix ends soon\n", "expected-end-of-line");
    only_error("2026-03-10 netflix[fifo] ends\n", "expected-arrow");
    // A name after an occurrence is a flow written without its arrow, whatever the first name means.
    only_error("2026-03-10 checking food 84.20 USD\n", "expected-arrow");
    // The word after the subject decides, and a purpose or a code has no occurrences.
    only_error("2026-03-10 #food\n", "expected-verb");
    only_error("2026-03-10 ^promo\n", "expected-verb");
}

#[test]
fn a_claim_says_who_owes_whom_and_is_flow_shaped() {
    let file = parse_clean(EXAMPLE);
    let claims: Vec<&Statement> = file.iter::<Statement>().filter(|said| matches!(said.verb, Verb::Owes { .. })).collect();
    assert_eq!(claims.len(), 3, "two dated claims, and the one an opening states");
    fn owes<'a>(said: &Statement<'a>) -> (&'a str, &'a str) {
        match &said.verb {
            Verb::Owes { creditor, amount: Some(Amount::Literal(amount)) } => (creditor.0, amount.0),
            other => panic!("a claim with an amount, not {other:?}"),
        }
    }
    let invoice = claims[0];
    assert!(matches!(invoice.subject, Subject::Name(Name("halcyon"))));
    assert_eq!(owes(invoice), ("studio", "3_800 USD"));
    let kinds = clauses(&file, invoice.tail);
    assert!(matches!(kinds[0], ClauseKind::Due(Due::After(_))) && matches!(kinds[1], ClauseKind::Code(code) if code.name() == "inv-2026-01"));
    assert!(matches!(kinds[2], ClauseKind::Purpose(purpose) if purpose.name.0 == "design"));
    let bill = claims[1];
    assert!(matches!(bill.subject, Subject::Name(Name("me"))) && owes(bill).0 == "pge");
    let kinds = clauses(&file, bill.tail);
    assert!(matches!(kinds[0], ClauseKind::Due(Due::On(_))) && matches!(kinds[1], ClauseKind::Description(Text("a bill"))));
    let open = claims[2];
    assert!(matches!(open.subject, Subject::Name(Name("jo"))) && open.date == day(2024, 12, 31));

    only_error("2026-01-27 halcyon owes 3_800 USD\n", "expected-name");
    only_error("2026-01-27 halcyon owes studio\n", "expected-amount");
    only_error("opening 2026-01-01\n  jo owes me 600 USD\n  jo owes me\n", "expected-amount");
    // A claim takes items, not legs.
    only_error("2026-01-27 halcyon owes studio 3_800 USD\n  checking ...\n", "takes-items");
    // A claim with no amount is the sum of its items.
    let file = parse_clean("2026-01-27 halcyon owes studio\n  3_000 USD #design\n    800 USD #design\n");
    let claim = statements(&file)[0];
    assert!(matches!(claim.verb, Verb::Owes { amount: None, .. }) && claim.body.items.len() == 2);
    // The amount may be a share of what a code names: a third of a bill, a late fee on an invoice.
    let file = parse_clean("2026-03-05 jo owes me 1/3 of ^pge-jan\n2026-03-15 halcyon owes studio 1.5% of ^inv-12 #late-fee\n");
    for said in statements(&file) {
        assert!(matches!(said.verb, Verb::Owes { amount: Some(Amount::Computed(_)), .. }), "{said:?}");
    }
}

#[test]
fn values_may_be_negative_and_may_say_where_a_gap_goes() {
    let file = parse_clean(
        "2026-06-30 checking = -42.17 USD\n2026-03-31 retirement = 24_600 USD via market\n2026-01-31 a = 1 USD ! \"x\"\n",
    );
    let said = statements(&file);
    let Verb::Value(Amount::Literal(amount)) = said[0].verb else { panic!("a value") };
    assert_eq!(amount.num(), Dec { mantissa: -4217, scale: 2 });
    assert!(said[0].tail.is_empty());
    assert!(matches!(clauses(&file, said[1].tail)[..], [ClauseKind::Via(Name("market"))]));
    assert!(matches!(clauses(&file, said[2].tail)[..], [ClauseKind::Waive(Waive { reason: Some(Text("x")), .. })]));
    // A named measure and a reading are values too, of a code.
    let file = parse_clean("2026-01-01 ^bldg-water = 155.00 USD \"the water\"\n2026-01-31 ^odometer = 48_210 MI\n");
    assert!(statements(&file).iter().all(|said| matches!(said.subject, Subject::Code(_)) && matches!(said.verb, Verb::Value(_))));
    only_error("2026-01-31 a = 5\n", "expected-commodity");
}

#[test]
fn splits_and_openings() {
    let file = parse_clean(EXAMPLE);
    let split = statements(&file).into_iter().find(|said| matches!(said.verb, Verb::Split { .. })).unwrap();
    let Verb::Split { numerator, denominator } = split.verb else { unreachable!() };
    assert!(matches!(split.subject, Subject::Unit(Name("FAST"))));
    assert_eq!((numerator.mantissa, denominator.mantissa), (2, 1));
    let opening: &Opening = file.iter().next().unwrap();
    let lines = &file[opening.lines];
    assert_eq!((opening.date, lines.len(), opening.claims.len()), (day(2024, 12, 31), 3, 1));
    let kinds = clauses(&file, lines[2].tail);
    assert!(
        matches!(kinds[0], ClauseKind::Basis(_)) && matches!(kinds[1], ClauseKind::Since(d) if *d == day(2023, 6, 15))
    );
    only_error("opening 2024-12-31\n  checking ...\n", "opening-amount");
    only_error("2026-01-01 a -> b 5 USD since 2025-01-01\n", "expected-end-of-line");
    only_error("2026-01-01 FAST split 0 for 1\n", "bad-split");
}

#[test]
fn every_verb_says_one_thing_about_its_subject() {
    let file = parse_clean(STATEMENTS);
    let said = statements(&file);
    let verb = |verb: &Verb| -> &'static str {
        match verb {
            Verb::Occurrence(_) => "occurrence",
            Verb::Value(_) => "value",
            Verb::Owes { .. } => "owes",
            Verb::Now(_) => "now",
            Verb::Worked(_) => "worked",
            Verb::Used(_) => "used",
            Verb::Waived => "waived",
            Verb::Ends => "ends",
            Verb::Event(_) => "event",
            Verb::Split { .. } => "split",
            Verb::Basis { .. } => "basis",
            Verb::Filed(_) => "filed",
        }
    };
    let mut seen: Vec<&str> = said.iter().map(|said| verb(&said.verb)).collect();
    seen.sort();
    seen.dedup();
    assert_eq!(seen, ["basis", "ends", "event", "filed", "now", "occurrence", "owes", "split", "used", "value", "waived", "worked"]);

    // What a value is about: an account, a commodity, a code.
    assert!(matches!(said[4].subject, Subject::Unit(Name("VTI"))));
    assert!(matches!(said[5].subject, Subject::Code(Code("^bldg-water"))));
    // A measure records work or use, and the tail says what for and for whom.
    let Verb::Worked(hours) = said[7].verb else { panic!("work") };
    assert_eq!((hours.num(), hours.unit().map(|unit| unit.0)), (Dec { mantissa: 65, scale: 1 }, Some("HR")));
    let kinds = clauses(&file, said[8].tail);
    assert!(matches!(kinds[..], [ClauseKind::Purpose(Purpose { name: Name("business-travel"), .. }), ClauseKind::For(For::Whom(Name("studio")))]));
    // Settlement states are one verb carrying the state.
    let states: Vec<EventState> = said.iter().filter_map(|said| if let Verb::Event(state) = said.verb { Some(state) } else { None }).collect();
    assert_eq!(states, [EventState::Settled, EventState::Void, EventState::Returned]);
    // A return lists tallies as legs.
    let filed = said.iter().find(|said| matches!(said.verb, Verb::Filed(2025))).unwrap();
    assert_eq!((filed.body.legs.len(), filed.body.items.len()), (2, 0));
    // A waiver may name a purpose and list what is recoverable.
    let waived: Vec<&&Statement> = said.iter().filter(|said| matches!(said.verb, Verb::Waived)).collect();
    assert_eq!(waived.len(), 4);
    assert!(matches!(clauses(&file, waived[1].tail)[..], [ClauseKind::Until(_), ClauseKind::Description(_)]));
    assert_eq!(waived[3].body.items.len(), 1);

    // The word after the subject decides: what a name means never does.
    let file = parse_clean("2026-01-01 flat\n2026-01-02 nobody-at-all\n2026-01-03 #x = 5 USD\n");
    assert!(statements(&file).iter().take(2).all(|said| matches!(said.verb, Verb::Occurrence(None))));
    assert!(matches!(statements(&file)[2].subject, Subject::Purpose(Name("x"))));
}

#[test]
fn a_change_restates_part_of_a_declaration() {
    let file = parse_clean(STATEMENTS);
    let said = statements(&file);
    let terms = |index: usize| match &said[index].verb {
        Verb::Now(Change::Terms(id)) => &file[*id],
        other => panic!("terms, not {other:?}"),
    };
    // New terms need no holding, and carry the rest over.
    let flat = terms(9);
    assert!(matches!(flat.payment, Some(Payment::Fixed(Amount::Literal(amount))) if amount.0 == "3_050 USD"));
    assert_eq!((flat.cadence, flat.holding.is_none()), (Cadence::Every(Span::months(1)), true));
    assert!(matches!(clauses(&file, said[9].tail)[..], [ClauseKind::Description(Text("renewed at 3,050"))]));
    // `until` is the last day it holds, and a code names the change.
    assert!(matches!(
        clauses(&file, said[10].tail)[..],
        [ClauseKind::Until(until), ClauseKind::Code(Code("^promo")), ClauseKind::Description(_)] if *until == day(2026, 5, 31)
    ));
    // Any property, with expression arguments; a change's own span is a property too.
    let property = |index: usize| match &said[index].verb {
        Verb::Now(Change::Property(prop)) => (prop.name.0, file[prop.args].iter().map(|&arg| show(&file, arg, STATEMENTS)).collect::<Vec<_>>()),
        other => panic!("a property, not {other:?}"),
    };
    assert_eq!(property(11), ("until", vec!["08-31".to_string()]));
    assert_eq!(property(12), ("share", vec!["20%".to_string(), "for".to_string(), "studio".to_string()]));
    assert_eq!(property(13), ("at", vec!["6.25%".to_string()]));
    assert_eq!(property(14), ("lives", vec!["us/ny".to_string()]));
    // A budget is restated with its window, and `until` ends it.
    let Verb::Now(Change::Budget(id)) = said[15].verb else { panic!("a budget") };
    let budget = &file[id];
    assert!(matches!(budget.limit, Limit::Amount(Amount::Literal(amount)) if amount.0 == "1_200 USD") && budget.per == Period::Month);
    assert!(matches!(clauses(&file, said[15].tail)[0], ClauseKind::Until(until) if *until == day(2026, 12, 31)));
    // Items under a change with no terms amend a claim, as a credit note does.
    assert!(matches!(said[17].verb, Verb::Now(Change::Amendment)) && said[17].body.items.len() == 2);
    // New terms bring new legs, and a leg dropped is `empty`.
    assert_eq!(terms(18).cadence, Cadence::TwiceMonthly);
    let legs = &file[said[18].body.legs];
    assert!(matches!(legs[0].amount, Quantity::Amount(Amount::Literal(amount)) if amount.0 == "empty"));

    // A short `due` or `until` date counts forward from the statement's own day.
    let due = |src: &str| {
        let file = parse_clean(src);
        let Verb::Now(Change::Property(prop)) = &statements(&file)[0].verb else { panic!("a property") };
        let ExprKind::Date(day) = file.exprs[file[prop.args][0]].kind else { panic!("a date") };
        day
    };
    assert_eq!(due("2026-04-20 ^inv-12 now due 05-15\n"), day(2026, 5, 15));
    assert_eq!(due("2026-12-20 ^inv-12 now due 01-05\n"), day(2027, 1, 5));
    assert_eq!(due("2026-12-20 ^inv-12 now due 2026-12-01\n"), day(2026, 12, 1));
    let src = "2026-01-01 ^a now due 02-30\n";
    assert_eq!(first_fix(src, &only_error(src, "bad-date")), ("02-30", "02-28"));

    only_error("2026-07-01 flat now\n", "expected-amount");
    only_error("2026-07-01 flat now 5 USD\n", "unknown-cadence");
    only_error("2026-07-01 flat now = 5 USD\n", "expected-change");
    // Items amend a claim; legs are for terms.
    only_error("2026-01-12 ^inv-9 now\n  checking 5 USD\n", "takes-items");
}

#[test]
fn a_verb_takes_only_the_clauses_that_mean_something_to_it() {
    let refused = |src: &str, clause: &str| {
        let error = only_error(src, "clause-not-taken");
        assert_eq!(&src[error.anchor().unwrap().range()], clause, "{src}");
        // Every one is offered as a removal.
        assert_eq!(first_fix(src, &error), (clause, ""), "{src}");
    };
    refused("2026-01-01 checking = 5 USD until 02-01\n", "until 02-01");
    refused("2026-01-01 checking = 5 USD #food\n", "#food");
    refused("2026-01-01 checking = 5 USD due 30d\n", "due 30d");
    refused("2026-01-01 flat ends via paypal\n", "via paypal");
    refused("2026-01-01 flat waived for 2025\n", "for 2025");
    refused("2026-01-01 FAST split 2 for 1 #x\n", "#x");
    refused("2026-01-01 me worked 5 HR due 30d\n", "due 30d");
    refused("2026-01-01 flat 5 USD until 02-01\n", "until 02-01");
    // A description and a code go with any of them.
    parse_clean("2026-01-01 checking = 5 USD \"why\" ^c\n2026-01-01 flat ends \"gone\" ^c\n2026-01-02 me worked 5 HR ^c \"why\"\n");
    // A leg is a leg of an occurrence or of terms and of nothing else.
    only_error("2026-01-01 checking = 5 USD\n  a 1 USD\n", "unexpected-indent");
    only_error("2026-01-01 flat waived\n  checking ...\n", "takes-items");
    only_error("2026-01-01 me filed 2025\n  - 5 USD\n", "takes-legs");
    only_error("2026-01-01 me filed\n", "expected-year");
    only_error("2026-01-01 me worked 5\n", "expected-commodity");
    only_error("2026-01-01 checking = \n", "expected-amount");
    only_error("2026-01-01 car basis 5 USD since\n", "expected-date");
    // The word after a code, a purpose or a commodity is a verb, and a near miss is offered.
    let src = "2026-01-01 ^c settle\n";
    assert_eq!(first_fix(src, &only_error(src, "expected-verb")), ("settle", "settled"));
    only_error("2026-01-01 ^c\n", "expected-verb");
}

/// The amount of the first statement's or flow's header, and of the first item under it, written out.
fn shown_amounts(src: &str) -> Vec<String> {
    let file = parse_clean(src);
    let mut out = Vec::new();
    let mut show_amount = |amount: &Amount| {
        out.push(match amount {
            Amount::Literal(literal) => format!("literal {}", literal.0),
            Amount::Computed(root) => show(&file, *root, src),
        })
    };
    for item in &file.items {
        match item.kind {
            ItemKind::Statement(id) => {
                match &file[id].verb {
                    Verb::Occurrence(Some(amount)) | Verb::Value(amount) | Verb::Owes { amount: Some(amount), .. } => show_amount(amount),
                    _ => {}
                }
                file[file[id].body.items].iter().for_each(|item| show_amount(&item.amount));
            }
            ItemKind::Txn(id) => {
                let flow = &file[id].flow;
                for quantity in [&flow.from.amount, &flow.to.amount].into_iter().flatten() {
                    if let Quantity::Amount(amount) = quantity {
                        show_amount(amount);
                    }
                }
                file[flow.body.items].iter().for_each(|item| show_amount(&item.amount));
            }
            _ => {}
        }
    }
    out
}

#[test]
fn amounts_are_written_as_the_book_computes_them() {
    let items = |written: &str| shown_amounts(&format!("2026-04-27 halcyon owes studio\n  {written} #x\n"));
    // A share of what a code names, of a name, and of an amount.
    assert_eq!(items("12% of ^bldg-water"), ["(12% of ^bldg-water)"]);
    assert_eq!(items("1/3 of ^pge-jan"), ["(1/3 of ^pge-jan)"]);
    assert_eq!(items("50% of flat"), ["(50% of flat)"]);
    assert_eq!(items("5% of 100.00 USD"), ["(5% of 100.00 USD)"]);
    // A reference narrowed by selectors, the parts of one purpose, and priced.
    assert_eq!(items("8.625% of ^inv-12[#design]"), ["(8.625% of ^inv-12[#design])"]);
    assert_eq!(items("^inv-12[HR] @ 150 USD/HR"), ["(^inv-12[HR] @ 150 USD/HR)"]);
    assert_eq!(items("^inv-12[2026-01, #design, ^x, retirement, fifo]"), ["^inv-12[2026-01, #design, ^x, retirement, fifo]"]);
    assert_eq!(items("^pge-jan"), ["^pge-jan"]);
    // `up to` is the smaller of two, and looser than a share.
    assert_eq!(items("100 USD up to ^cap"), ["(100 USD up to ^cap)"]);
    assert_eq!(items("5% of ^a up to 3% of ^b"), ["((5% of ^a) up to (3% of ^b))"]);
    assert_eq!(items("1 USD up to 2 USD up to 3 USD"), ["((1 USD up to 2 USD) up to 3 USD)"]);
    // A literal and a lone share need no node of the arena.
    assert_eq!(items("5 USD"), ["literal 5 USD"]);
    assert_eq!(items("10%"), ["10%"]);

    // A quantity of something at a price, on an occurrence or a claim, and a share of a reference in a flow.
    assert_eq!(shown_amounts("2026-04-05 jo owes me 1/3 of ^pge-jan\n"), ["(1/3 of ^pge-jan)"]);
    assert_eq!(shown_amounts("2026-04-05 flat 7 VTI @ 285.70 USD\n"), ["(7 VTI @ 285.70 USD)"]);
    assert_eq!(shown_amounts("2026-04-05 a -> b 50% of ^rent\n"), ["(50% of ^rent)"]);

    // What is not an amount says what it was and how to write it.
    only_error("2026-04-05 a owes b 1/0 of ^x\n", "zero-fraction");
    only_error("2026-04-05 a owes b 1/3\n", "expected-of");
    only_error("2026-04-05 a owes b 5% of\n", "expected-reference");
    only_error("2026-04-05 a owes b 5% of 5\n", "expected-commodity");
    only_error("2026-04-05 a owes b 5 USD up 3 USD\n", "expected-end-of-line");
    only_error("2026-04-05 a owes b 5 USD up to\n", "expected-amount");
    only_error("2026-04-05 a -> b 5%\n", "expected-end-of-line");
    // A price is a quantity of something at a rate.
    only_error("2026-04-05 flat 7 VTI @ 285.70\n", "expected-commodity");
    // A declaration's amount is any expression of the law grammar, and a number alone says what is missing.
    let file = parse_clean("contract c\n  5 USD monthly from x\n  also -> y 2 * amount / 3 #z when amount > 20 USD\n");
    let contract: &Contract = file.iter().next().unwrap();
    let AlsoLine::Flow(flow) = &file[contract.alsos][0].line else { panic!("a flow") };
    let Some(Quantity::Amount(Amount::Computed(root))) = flow.to.amount else { panic!("a computed amount") };
    assert_eq!(show(&file, root, "contract c\n  5 USD monthly from x\n  also -> y 2 * amount / 3 #z when amount > 20 USD\n"), "((2 * amount) / 3)");
    only_error("budget food 900\n", "expected-commodity");
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
    assert_eq!(file[groceries.props][0].name.0, "share");
    // A purpose's budget is its own line, and its `also` is a line of the purpose.
    let allowance = &file[groceries.budget.unwrap()];
    assert!(matches!(allowance.limit, Limit::Amount(Amount::Literal(amount)) if amount.0 == "400 USD"));
    assert!(allowance.carries && allowance.per == Period::Month);
    assert!(matches!(&file[groceries.alsos][0].line, AlsoLine::Item(item) if item.sign == Sign::Add));
    assert!(file[groceries.alsos][0].when.is_some());
    let condo = decls.iter().find(|decl| decl.what == DeclKind::Asset).unwrap();
    assert_eq!((condo.kind.map(|kind| kind.0), file[condo.props].len()), (Some("rental-home"), 2));

    let budgets: Vec<&Budget> = file.iter().collect();
    let (food, groceries) = (&budgets[0], &budgets[1]);
    assert!(matches!(food.allowance.limit, Limit::Amount(Amount::Literal(amount)) if amount.0 == "900 USD"));
    assert_eq!((food.purpose.0, food.allowance.per, food.allowance.carries), ("food", Period::Month, false));
    assert!(matches!(groceries.allowance.limit, Limit::Amount(Amount::Literal(amount)) if amount.0 == "4_000 USD"));
    assert_eq!(groceries.allowance.per, Period::Year);
    only_error("budget food 900 USD weekly\n", "unknown-period");
    only_error("budget food 900\n", "expected-commodity");
    only_error("budget 900 USD monthly\n", "expected-name");
    // A budget may be funded: its limit moves into money held for it.
    let file = parse_clean("budget fun 200 USD monthly carries funded from checking into envelope\n");
    let budget: &Budget = file.iter().next().unwrap();
    let funded = budget.allowance.funded.unwrap();
    assert!(budget.allowance.carries && (funded.from.0, funded.into.0) == ("checking", "envelope"));
    only_error("budget fun 200 USD monthly funded checking into envelope\n", "expected-keyword");
    // A purpose may state its budget once, and only a purpose has the line.
    only_error("purpose fun\n  budget 5 USD monthly\n  budget 6 USD monthly\n", "duplicate-clause");

    // `at` belongs to accounts, and needs an institution.
    only_error("entity x : person at chase\n", "expected-end-of-line");
    let (file, diags) = parse(FileId(0), "account x : deposit at\n");
    assert_eq!((diags.len(), file.iter::<Decl>().count()), (1, 1), "the account stays, without its institution");
}

#[test]
fn a_law_may_override_another_except_and_repair_in_a_chain() {
    let src = "\
law wash-sale overrides flat-rate
  on gain
  when gain < empty
  unless owner.age >= 65y
  require tally(losses) <= 0 USD else owe 10% * gain to irs by date(year + 1, 4, 15) as penalty else owe 20% * gain to irs \"late\"
  require balance >= empty
    else owe 5 USD to irs
    else carry gain to VTI within 30d \"repaired\"
  warn balance >= empty \"overdrawn\"
";
    let file = parse_clean(src);
    let law: &Law = file.iter().next().unwrap();
    assert_eq!(law.overrides.map(|name| name.0), Some("flat-rate"));
    let steps = &file[law.steps];
    let StepKind::Unless(exception) = steps[1].kind else { panic!("an exception") };
    assert_eq!(show(&file, exception, src), "(owner.age >= 59y6m)".replace("59y6m", "65y"));
    // Reparations on the line: each `else` is owed instead of what is before it.
    let StepKind::Require { otherwise, message, warn, .. } = steps[2].kind else { panic!("a require") };
    assert_eq!((file[otherwise].len(), message.map(|text| text.0), warn), (2, Some("late"), false));
    assert!(matches!(file[otherwise][0], Effect::Owe { due: Some(_), name: Some(Name("penalty")), .. }));
    // The same as lines of their own under it, and the message ends the last.
    let StepKind::Require { otherwise, message, .. } = steps[3].kind else { panic!("a require") };
    assert!(matches!(file[otherwise][..], [Effect::Owe { .. }, Effect::Carry { .. }]));
    assert_eq!(message.map(|text| text.0), Some("repaired"));
    // A step that spans lines says where it ends.
    assert!(steps[3].loc.end > steps[3].loc.start + 30);
    let StepKind::Require { otherwise, warn, .. } = steps[4].kind else { panic!("a warn") };
    assert!(warn && otherwise.is_empty());

    only_error("law l\n  on in\n  require a\n    else owe 1 USD to x \"m\"\n    else owe 2 USD to y\n", "else-after-message");
    only_error("law l\n  on in\n  require a\n    owe 1 USD to x\n", "expected-else");
    only_error("law l\n  on in\n  warn a\n    else owe 1 USD to x\n", "unexpected-indent");
    only_error("law l\n  on in\n  require a else\n", "expected-effect");
    only_error("law l overrides\n  on in\n", "expected-name");
    only_error("law l\n  on in\n  unless\n", "expected-expression");
    // A law in a purpose needs no trigger, and a top-level one does.
    let file = parse_clean("purpose wages\n  law count-wages\n    count amount as wages\n  law by-hand\n    on in\n    count amount as x\n");
    let decl: &Decl = file.iter().next().unwrap();
    let laws = &file[decl.laws];
    assert_eq!((laws[0].trigger, laws[1].trigger), (Trigger::Flow, Trigger::In));
    only_error("law l\n  count amount as x\n", "missing-trigger");
    only_error("kind k : asset\n  law l\n    count amount as x\n", "missing-trigger");
}

#[test]
fn a_system_says_what_it_counts_in_and_how_it_converts() {
    let src = "system us\ncurrency USD\nrates param irs-rates\nuse us/ca\nbase USD\n";
    let file = parse_clean(src);
    let settings: Vec<Setting> = file.iter::<Setting>().copied().collect();
    assert!(matches!(settings[1], Setting::Currency(Name("USD"))));
    assert!(matches!(settings[2], Setting::Rates(Rates::Param(Name("irs-rates")))));
    let file = parse_clean("rates spot\n");
    assert!(matches!(file.iter::<Setting>().next(), Some(Setting::Rates(Rates::Spot))));
    let src = "rates sopt\n";
    assert_eq!(first_fix(src, &only_error(src, "unknown-rates")), ("sopt", "spot"));
    only_error("currency\n", "expected-commodity");
    only_error("currency usd\n", "expected-commodity");
    only_error("rates param\n", "expected-name");
    only_error("rates\n", "unknown-rates");

    // A param names its unit, and its rows are looked up as before.
    let file = parse_clean("param cpi USD\n  2025 3.1\n  2026 3.4\nparam single\n  2026 24_500 USD\n");
    let params: Vec<&Param> = file.iter().collect();
    assert_eq!((params[0].unit.map(|unit| unit.0), params[1].unit), (Some("USD"), None));
    assert_eq!(params[0].rows.len(), 2);
}

#[test]
fn a_contract_says_what_holds_of_every_occurrence() {
    let src = "\
contract flat with greystar
  2_900 USD monthly on 1 from checking #rent of unit
  grace 3d
  for last month
  covers the month
  prorated
  rising 3% yearly
  indexed to cpi yearly
  area 1_000 SQFT
  input water USD
  share 120 SQFT for studio, 60% for me
  deposit 5_800 USD into escrow
  due 5d else + 5% #late-fee
  + 12% of water #utilities
  also -> escrow 410 USD #escrow when amount > 0 USD
";
    let file = parse_clean(src);
    let contract: &Contract = file.iter().next().unwrap();
    let props = properties(&file, contract, src);
    let names: Vec<&str> = props.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["grace", "for", "covers", "prorated", "rising", "indexed", "area", "input", "share", "deposit"]);
    let arguments = |name: &str| props.iter().find(|(prop, _)| prop == name).unwrap().1.join(" ");
    assert_eq!(arguments("indexed"), "to cpi yearly");
    assert_eq!(arguments("share"), "120 SQFT for studio 60% for me");
    assert_eq!(arguments("deposit"), "5_800 USD into escrow");
    assert_eq!(arguments("for"), "last month");
    assert_eq!(contract.body.items.len(), 1);
    let deadline = contract.deadline.as_ref().unwrap();
    assert_eq!(deadline.span, Span::days(5));
    assert_eq!(contract.alsos.len(), 1);
    assert!(file[contract.alsos][0].when.is_some());

    // An entity, a kind or a purpose says what every event of it implies, and a flow with no date is a flow.
    let file = parse_clean("entity acme : employer\n  also + 2% of amount #fee\n  also acme -> me 5 USD #bonus when amount > 100 USD\n");
    let decl: &Decl = file.iter().next().unwrap();
    let alsos = &file[decl.alsos];
    assert!(matches!(alsos[0].line, AlsoLine::Item(_)) && alsos[0].when.is_none());
    assert!(matches!(alsos[1].line, AlsoLine::Flow(_)) && alsos[1].when.is_some());

    only_error("contract c\n  5 USD monthly from x\n  also\n", "expected-also");
    only_error("contract c\n  5 USD monthly from x\n  due\n", "expected-span");
    only_error("contract c\n  5 USD monthly from x\n  due 5d else\n", "expected-amount");
    only_error("contract c\n  5 USD monthly from x\n  also 5 USD when\n", "expected-expression");
    // A standing order is its own line, and a contract may have one of each.
    let file = parse_clean("contract vti with fidelity\n  buy VTI for 500 USD monthly on 20 from checking\n");
    let contract: &Contract = file.iter().next().unwrap();
    assert!(contract.schedule.is_none() && contract.standing.is_some() && !contract.damaged);
    only_error("contract c\n  buy VTI for 5 USD monthly from x\n  buy VTI for 6 USD monthly from x\n", "duplicate-clause");
    only_error("contract c\n  buy VTI 5 USD monthly from x\n", "expected-keyword");
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
    let amount = |amount: &Amount| match amount {
        Amount::Literal(amount) => amount.0.to_string(),
        Amount::Computed(root) => src[file.exprs[*root].loc.range()].to_string(),
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
    let kinds: Vec<(Sign, &Amount)> = items.iter().map(|item| (item.sign, &item.amount)).collect();
    assert!(matches!(kinds[0], (Sign::Carve, Amount::Literal(amount)) if amount.0 == "32.10 USD"));
    assert!(matches!(kinds[2], (Sign::Add, Amount::Computed(root)) if matches!(file.exprs[*root].kind, ExprKind::Of(..))));
    let Amount::Computed(root) = items[2].amount else { panic!("a share of an amount") };
    assert_eq!(&src[file.exprs[root].loc.range()], "5% of 100.00 USD");
    assert!(matches!(kinds[3], (Sign::Less, Amount::Literal(_))));
    assert!(matches!(kinds[4], (Sign::Carve, Amount::Computed(root)) if matches!(file.exprs[*root].kind, ExprKind::Pct(percent) if percent.mantissa == 10)));
    let tail = clauses(&file, items[1].tail);
    assert!(matches!(tail[..], [ClauseKind::Purpose(_), ClauseKind::Description(Text("for jo's birthday"))]));
    assert!(flow.body.legs.is_empty());
    // What names an end is a leg, and under a one-sided flow the two may be mixed.
    let file = parse_clean("2026-03-14 lumen -> 4_600 USD\n  retirement 6%\n  - 100 USD #fees\n  checking ...\n");
    let flow = &txns(&file)[0].flow;
    assert_eq!((flow.body.legs.len(), flow.body.items.len()), (2, 1));
    // An item has an amount, and its tail is a flow's.
    only_error("2026-03-14 a -> b 5 USD\n  + #fees\n", "expected-amount");
    // An amount first and then an end is a leg written backwards.
    let src = "2026-03-14 lumen -> 4_600 USD\n  800 USD retirement\n  checking ...\n";
    assert_eq!(first_fix(src, &only_error(src, "item-with-end")), ("800 USD retirement", "retirement 800 USD"));
    let src = "2026-03-14 lumen -> 4_600 USD\n  - 800 USD retirement\n";
    assert!(only_error(src, "item-with-end").help.is_empty());
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
fn a_sync_names_its_source_and_says_where_its_records_are() {
    let src = "/// Chase's export.\nsync checking\n  read \"imports/chase-*.csv\"\n  format csv\n    date \"Posting Date\" \"MM/DD/YYYY\"\n    amount \"Amount\" flipped\n    memo \"Description\"\n";
    let file = parse_clean(src);
    let ItemKind::Sync(id) = file.items[0].kind else { panic!("a sync") };
    let sync = &file[id];
    assert_eq!((sync.name.0, sync.read.map(|text| text.0)), ("checking", Some("imports/chase-*.csv")));
    assert!(sync.run.is_none() && sync.into.is_none());
    let format = &file[sync.format.unwrap()];
    assert_eq!(format.name.0, "csv");
    let lines = &file[format.lines];
    assert_eq!(lines.iter().map(|line| line.key.0).collect::<Vec<_>>(), ["date", "amount", "memo"]);
    assert!(matches!(file[lines[0].args][..], [FormatArg::Quoted(Text("Posting Date")), FormatArg::Quoted(Text("MM/DD/YYYY"))]));
    assert!(matches!(file[lines[1].args][..], [FormatArg::Quoted(Text("Amount")), FormatArg::Word(Text("flipped"))]));
    assert!(file.items[0].doc.is_some());

    // A script is raw text, and so is where it writes; a format may be named and declared elsewhere.
    let src = "sync prices\n  run quotes {units} --since {since}\n  into prices/{year}.ax\nsync visa\n  read \"imports/*.qfx\"\n  format ofx\n";
    let file = parse_clean(src);
    let syncs: Vec<&Sync> = file.iter().collect();
    assert_eq!((syncs[0].run.map(|text| text.0), syncs[0].into.map(|text| text.0)), (Some("quotes {units} --since {since}"), Some("prices/{year}.ax")));
    let ofx = &file[syncs[1].format.unwrap()];
    assert!(ofx.name.0 == "ofx" && ofx.lines.is_empty());

    only_error("sync checking\n  into x\n", "missing-source");
    only_error("sync checking\n  run a\n  run b\n", "duplicate-clause");
    only_error("sync checking\n  run a\n  into x\n  into y\n", "duplicate-clause");
    only_error("sync checking\n  read \"a\"\n  read \"b\"\n", "duplicate-clause");
    only_error("sync checking\n  read \"a\"\n  format csv\n  format ofx\n", "duplicate-clause");
    only_error("sync checking\n  read imports/*.csv\n", "expected-string");
    only_error("sync checking\n  run\n", "expected-command");
    let src = "sync checking\n  reed \"a\"\n";
    assert_eq!(first_fix(src, &only_error(src, "unknown-sync-line")), ("reed", "read"));
    // v3 named the file, which is `into` now.
    let src = "sync prices/2026.ax\n  run python3 fetch.py\n";
    let error = only_error(src, "sync-file");
    assert_eq!(first_fix(src, &error), ("prices/2026.ax", "prices\n  into prices/2026.ax"));
}

#[test]
fn a_format_names_its_fields_as_the_source_does() {
    let src = "format camt053\n  records Ntry\n  date BookgDt/Dt\n  amount Amt, sign CdtDbtInd CRDT\n  code NtryDtls/TxDtls/Ref // the reference\n  category \"Groceries, fresh\" is #groceries\n";
    let file = parse_clean(src);
    let ItemKind::Format(id) = file.items[0].kind else { panic!("a format") };
    let format = &file[id];
    assert_eq!(format.name.0, "camt053");
    let lines = &file[format.lines];
    let words = |line: usize| -> Vec<String> {
        file[lines[line].args].iter().map(|arg| match arg {
            FormatArg::Word(text) => text.0.to_string(),
            FormatArg::Quoted(text) => format!("\"{}\"", text.0),
        }).collect()
    };
    assert_eq!(lines.iter().map(|line| line.key.0).collect::<Vec<_>>(), ["records", "date", "amount", "code", "category"]);
    assert_eq!(words(0), ["Ntry"]);
    assert_eq!(words(1), ["BookgDt/Dt"]);
    assert_eq!(words(2), ["Amt", "sign", "CdtDbtInd", "CRDT"], "commas separate and are dropped");
    assert_eq!(words(3), ["NtryDtls/TxDtls/Ref"], "a comment ends the line");
    assert_eq!(words(4), ["\"Groceries, fresh\"", "is", "#groceries"], "a string keeps its commas");
    assert_eq!(&src[file.loc(&lines[1].key).range()], "date");

    only_error("format csv\n  date \"Posting\n", "unterminated-string");
    only_error("format csv\n  \"date\" x\n", "expected-format-key");
    only_error("format csv\n  Date x\n", "expected-format-key");
    only_error("format\n", "expected-name");
    // A format that is empty is one a sync names and something else declares.
    parse_clean("format ofx\n");
}

/// A pattern written back the way the grammar spells it.
fn spell(file: &File, pattern: &Pattern) -> String {
    let choices: Vec<String> = file[pattern.choices]
        .iter()
        .map(|choice| {
            let terms: Vec<String> = file[choice.terms]
                .iter()
                .map(|term| {
                    let atom = match &term.atom {
                        PatternAtom::Literal(text) => format!("\"{}\"", text.0),
                        PatternAtom::Class(class) => Class::WORDS.iter().find(|(_, known)| known == class).unwrap().0.to_string(),
                        PatternAtom::Named(name) => name.0.to_string(),
                        PatternAtom::Group(inner) => format!("({})", spell(file, inner)),
                    };
                    let repeat = match term.repeat {
                        Repeat::One => "",
                        Repeat::Optional => "?",
                        Repeat::Many => "*",
                        Repeat::Some => "+",
                    };
                    format!("{}{atom}{repeat}", term.capture.map(|name| format!("{}:", name.0)).unwrap_or_default())
                })
                .collect();
            terms.join(" ")
        })
        .collect();
    choices.join(" / ")
}

#[test]
fn patterns_are_ordered_choices_of_sequences() {
    for written in [
        "\"INV-\" digit+ \"-\" digit+",
        "\"PAYPAL *\" payee:rest",
        "\"ACH \" (\"DEBIT\" / \"CREDIT\") space+",
        "ach? digit* letter? alnum+ any",
        "start \"x\" / \"y\" end",
        "(\"a\" (\"b\" / \"c\")+ / d)* named",
    ] {
        let src = format!("pattern p = {written}\n");
        let file = parse_clean(&src);
        let ItemKind::Pattern(id) = file.items[0].kind else { panic!("a pattern") };
        assert_eq!(file[id].name.0, "p");
        assert_eq!(spell(&file, &file[id].pattern), written);
    }
    // Every run is contiguous, however the groups nest: a sequence's terms are read back in order.
    let file = parse_clean("pattern p = \"a\" (\"b\" \"c\" / \"d\") \"e\"\n");
    let ItemKind::Pattern(id) = file.items[0].kind else { unreachable!() };
    let outer = file[file[id].pattern.choices][0];
    assert_eq!(file[outer.terms].len(), 3);

    only_error("pattern p =\n", "expected-pattern");
    only_error("pattern p = \"a\" /\n", "expected-pattern");
    only_error("pattern p = / \"a\"\n", "expected-pattern");
    only_error("pattern p = (\"a\"\n", "unclosed-delimiter");
    only_error("pattern p = ()\n", "expected-pattern");
    only_error("pattern p\n", "expected-equals");
    only_error("pattern p = a*b\n", "bad-pattern-word");
    let error = only_error("pattern p = INV\n", "expected-pattern");
    assert!(error.help[0].text.contains("quotes"));
    let src = "pattern p = digit/letter\n";
    assert_eq!(first_fix(src, &only_error(src, "pattern-slash")), ("digit/letter", "digit / letter"));
    only_error(&format!("pattern p = {}\"a\"{}\n", "(".repeat(200), ")".repeat(200)), "pattern-too-deep");
}

#[test]
fn known_as_lists_the_patterns_that_recognize_a_thing() {
    let src = "entity paypal : processor\n  known-as \"PAYPAL *\" payee:rest, \"PP*\" payee:rest\n  known-as \"PP \" rest\ncode inv-*\n  on client\n  known-as \"INV-\" digit+ \"-\" digit+   // INV-2026-01 is ^inv-2026-01\n";
    let file = parse_clean(src);
    let decl: &Decl = file.iter().next().unwrap();
    let known: Vec<String> = file[decl.known_as].iter().map(|pattern| spell(&file, pattern)).collect();
    assert_eq!(known, ["\"PAYPAL *\" payee:rest", "\"PP*\" payee:rest", "\"PP \" rest"]);
    assert!(decl.props.is_empty());
    let rule: &CodeRule = file.iter().next().unwrap();
    assert_eq!(rule.known_as.len(), 1);
    assert_eq!(spell(&file, &file[rule.known_as][0]), "\"INV-\" digit+ \"-\" digit+");
    assert_eq!(file[rule.on].len(), 1);
    only_error("entity a\n  known-as\n", "expected-pattern");
    only_error("entity a\n  known-as \"x\" \"y\" ]\n", "expected-pattern");
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
fn fractions_and_rates_are_tokens() {
    let fraction = |top, bottom| Tok::Fraction(top, bottom);
    assert_eq!(tokens("1/3"), [fraction(1, 3)]);
    assert_eq!(tokens("12/25 USD"), [fraction(12, 25), Tok::Unit("USD")]);
    // Only two numbers make one: a name, a date and a path stay what they were.
    assert_eq!(tokens("401k/x"), [Tok::Name("401k/x")]);
    assert_eq!(tokens("3d/model"), [Tok::Name("3d/model")]);
    assert_eq!(tokens("2026/1/5"), [Tok::Invalid(Malformed::SlashDate)]);
    // A rate is two units around a slash.
    assert_eq!(tokens("150 USD/HR"), [number(150, 0), Tok::Unit("USD/HR")]);
    assert_eq!(tokens("BRK.B/USD"), [Tok::Unit("BRK.B/USD")]);
    assert_eq!(tokens("USD/mi"), [Tok::Invalid(Malformed::Word)]);
    assert_eq!(tokens("USD/MI/HR"), [Tok::Invalid(Malformed::Word)]);
    assert_eq!(tokens("USD/ x"), [Tok::Unit("USD"), Tok::Punct(Punct::Slash), Tok::Name("x")]);
}

#[test]
fn malformed_unicode_escapes_keep_utf8_boundaries() {
    assert_eq!(tokens(r#""\é""#).first(), Some(&Tok::Invalid(Malformed::Escape(1))));

    let src = "2026-01-15 checking -> groceries 5 USD \"cash\\é\"\n";
    let (file, diagnostics) = parse(FileId(0), src);
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code, "bad-escape");
    for label in &diagnostics[0].labels {
        assert!(src.is_char_boundary(label.loc.start as usize));
        assert!(src.is_char_boundary(label.loc.end as usize));
    }
    assert_eq!(crate::format(src, &file), src);

    let malformed_format = "format csv\n  memo \"a\\界\n";
    let (file, diagnostics) = parse(FileId(0), malformed_format);
    assert!(diagnostics.iter().any(|diag| diag.code == "unterminated-string"));
    assert_eq!(crate::format(malformed_format, &file), malformed_format);
}

#[test]
fn concurrent_files_keep_sources_and_diagnostic_locations_separate() {
    let left = "2026-01-15 checking-left -> food 5 USD\n2026-01-16 checking-left -> food 0\n";
    let right = "2026-02-15 checking-right -> food 6 USD\n2026-02-16 checking-right -> food 0\n";
    std::thread::scope(|scope| {
        let left_parse = scope.spawn(|| parse(FileId(17), left));
        let right_parse = scope.spawn(|| parse(FileId(23), right));
        let (left_file, left_diags) = left_parse.join().unwrap();
        let (right_file, right_diags) = right_parse.join().unwrap();

        assert_eq!(left_file.src, left);
        assert_eq!(right_file.src, right);
        assert_eq!(left_file.id, FileId(17));
        assert_eq!(right_file.id, FileId(23));
        assert_eq!(left_diags[0].code, "bare-zero");
        assert_eq!(right_diags[0].code, "bare-zero");
        assert_eq!(left_diags[0].anchor().unwrap().file, FileId(17));
        assert_eq!(right_diags[0].anchor().unwrap().file, FileId(23));
        assert!(left_diags[0].anchor().unwrap().start < left.len() as u32);
        assert!(right_diags[0].anchor().unwrap().start < right.len() as u32);
    });
}

#[test]
fn a_piece_larger_than_its_reference_space_is_rejected() {
    let prefix = "// comment before the oversized block\n";
    let src = format!("{prefix}2026-01-15 a -> b\n  {}\n", "x".repeat(crate::refs::MAX_LOCAL_NODES + 1));
    let (file, diagnostics) = parse(FileId(0), &src);
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code, "piece-too-large");
    assert!(file.items.is_empty());
    let location = diagnostics[0].labels[0].loc;
    assert_eq!(location.start, prefix.len() as u32);
    assert_eq!(location.start, location.end);
    assert!(src.is_char_boundary(location.start as usize));
    assert_eq!(file.format(), crate::format(&src, &file));
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
    assert!(matches!(statements(&file)[0].verb, Verb::Split { .. }) && statements(&file)[0].date == day(2026, 3, 7));
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
        | ExprKind::Month(_)
        | ExprKind::Year(_)
        | ExprKind::Fraction(..)
        | ExprKind::Span(_)
        | ExprKind::Str(_)
        | ExprKind::Empty
        | ExprKind::Name(_)
        | ExprKind::Unit(_)
        | ExprKind::Purpose(_)
        | ExprKind::Code(_) => vec![],
        ExprKind::Field(base, _) | ExprKind::Unary(_, base) => vec![*base],
        ExprKind::Index(base, keys) => [*base].into_iter().chain(file[*keys].iter().copied()).collect(),
        ExprKind::Call(_, args) | ExprKind::Select(args) => file[*args].to_vec(),
        ExprKind::Binary(_, lhs, rhs) | ExprKind::Of(lhs, rhs) | ExprKind::At(lhs, rhs) => vec![*lhs, *rhs],
        ExprKind::Is(lhs, alternatives) => [*lhs].into_iter().chain(file[*alternatives].iter().copied()).collect(),
        ExprKind::If(condition, then, otherwise) => vec![*condition, *then, *otherwise],
        ExprKind::Schedule(rows) => file[*rows].iter().flat_map(|row| [row.threshold, row.rate]).collect(),
    }
}

fn effect_roots(effect: &Effect) -> Vec<ExprId> {
    match effect {
        Effect::Owe { amount, due, .. } => [*amount].into_iter().chain(*due).collect(),
        Effect::Count { amount, .. } | Effect::Consume(amount) => vec![*amount],
        Effect::Carry { amount, to, .. } => vec![*amount, *to],
    }
}

fn law_roots(file: &File, law: &Law, out: &mut Vec<ExprId>) {
    if let Trigger::By(when) = law.trigger {
        out.push(when);
    }
    for step in &file[law.steps] {
        match &step.kind {
            StepKind::When(e) | StepKind::Unless(e) | StepKind::Let(_, e) => out.push(*e),
            StepKind::Require { cond, otherwise, .. } => {
                out.push(*cond);
                out.extend(file[*otherwise].iter().flat_map(effect_roots));
            }
            StepKind::Effect(effect) => out.extend(effect_roots(effect)),
        }
    }
}

/// The expression an amount is, if it is one.
fn amount_root(amount: &Amount, out: &mut Vec<ExprId>) {
    if let Amount::Computed(root) = amount {
        out.push(*root);
    }
}

fn quantity_root(quantity: &Quantity, out: &mut Vec<ExprId>) {
    if let Quantity::Amount(amount) | Quantity::Pending(amount) | Quantity::Target(amount) = quantity {
        amount_root(amount, out);
    }
}

fn flow_roots(flow: &Flow, out: &mut Vec<ExprId>) {
    for side in [&flow.from, &flow.to] {
        side.amount.iter().for_each(|quantity| quantity_root(quantity, out));
    }
}

fn terms_roots(terms: &Terms, out: &mut Vec<ExprId>) {
    match terms.payment {
        Some(Payment::Fixed(amount)) | Some(Payment::Buy { spend: amount, .. }) => amount_root(&amount, out),
        None => {}
    }
}

fn limit_roots(allowance: &Allowance, out: &mut Vec<ExprId>) {
    if let Limit::Amount(amount) = &allowance.limit {
        amount_root(amount, out);
    }
}

/// Every expression an item, property, row or step owns. Every node lives in a
/// table, so the tables are walked, and the nodes kept inline are visited by
/// what holds them.
fn roots(file: &File) -> Vec<ExprId> {
    let mut out = Vec::new();
    file.iter::<Txn>().for_each(|txn| flow_roots(&txn.flow, &mut out));
    file.iter::<Leg>().for_each(|leg| quantity_root(&leg.amount, &mut out));
    file.iter::<LineItem>().for_each(|item| amount_root(&item.amount, &mut out));
    file.iter::<Clause>().for_each(|clause| {
        if let ClauseKind::Basis(amount) = &clause.kind {
            amount_root(amount, &mut out);
        }
    });
    for said in file.iter::<Statement>() {
        match &said.verb {
            Verb::Occurrence(Some(amount))
            | Verb::Value(amount)
            | Verb::Owes { amount: Some(amount), .. }
            | Verb::Basis { amount, .. } => amount_root(amount, &mut out),
            Verb::Now(Change::Property(prop)) => out.extend(file[prop.args].iter().copied()),
            _ => {}
        }
    }
    file.iter::<Terms>().for_each(|terms| terms_roots(terms, &mut out));
    file.iter::<Allowance>().for_each(|allowance| limit_roots(allowance, &mut out));
    file.iter::<Budget>().for_each(|budget| limit_roots(&budget.allowance, &mut out));
    for contract in file.iter::<Contract>() {
        for schedule in [contract.schedule, contract.standing].into_iter().flatten() {
            terms_roots(&schedule.terms, &mut out);
        }
        if let Some(item) = contract.deadline.as_ref().and_then(|deadline| deadline.otherwise.as_ref()) {
            amount_root(&item.amount, &mut out);
        }
    }
    for also in file.iter::<Also>() {
        match &also.line {
            AlsoLine::Item(item) => amount_root(&item.amount, &mut out),
            AlsoLine::Flow(flow) => flow_roots(flow, &mut out),
        }
        out.extend(also.when);
    }
    file.iter::<Prop>().for_each(|prop| out.extend(file[prop.args].iter().copied()));
    file.iter::<Nested>().for_each(|nested| out.extend(file[nested.0.args].iter().copied()));
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
    // The forms of amounts and statements are where a node is most likely to be left behind.
    assert_post_order(&parse_clean(STATEMENTS), true);
}

/// What a damaged file's changed bytes are taken from: punctuation and signs.
const ALPHABET: &[u8] = b"-/\"(),:= \n%|^#.0+[]@!?*\t";

/// Sources damaged by a few changed bytes: `source`, again and again (`count`
/// times, or that many times `AXIOM_FUZZ`, to fuzz for longer).
fn damaged(source: &str, count: usize, mut with: impl FnMut(&str)) {
    let longer = std::env::var("AXIOM_FUZZ").ok().and_then(|times| times.parse::<usize>().ok()).unwrap_or(1);
    let count = count * longer;
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
    for source in [EXAMPLE, STATEMENTS] {
        damaged(source, 2_000, |src| {
            let (file, diags) = parse(FileId(0), src);
            assert_post_order(&file, !diags.iter().any(Diagnostic::is_error));
        });
    }
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
        ExprKind::At(quantity, price) => format!("({} @ {})", show(file, *quantity, src), show(file, *price, src)),
        ExprKind::Index(base, keys) => {
            let keys: Vec<String> = file[*keys].iter().map(|&key| show(file, key, src)).collect();
            format!("{}[{}]", show(file, *base, src), keys.join(", "))
        }
        ExprKind::Select(keys) => {
            let keys: Vec<String> = file[*keys].iter().map(|&key| show(file, key, src)).collect();
            format!("[{}]", keys.join(", "))
        }
        ExprKind::Pct(_)
        | ExprKind::Fraction(..)
        | ExprKind::Date(_)
        | ExprKind::Month(_)
        | ExprKind::Year(_)
        | ExprKind::Str(_)
        | ExprKind::Empty => src[exprs[id].loc.range()].to_string(),
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
fn expression_selector_years_are_typed_separately_from_numbers() {
    let source = "law l\n  always\n  when total(in, [2026]) > 0 USD\n";
    let file = parse_clean(source);
    let selector = file
        .exprs
        .iter()
        .find_map(|expr| match expr.kind {
            ExprKind::Select(keys) => Some(keys),
            _ => None,
        })
        .expect("a selector expression");
    let key = file[selector][0];
    assert!(matches!(file.exprs[key].kind, ExprKind::Year(2026)));

    let source = "law l\n  always\n  when amount > 2026\n";
    let file = parse_clean(source);
    assert!(file.exprs.iter().any(|expr| matches!(expr.kind, ExprKind::Num(_))));
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
    let StepKind::Require { otherwise, .. } = file[wash.steps][1].kind else { panic!("a require that carries a loss") };
    let [Effect::Carry { to, within, .. }] = file[otherwise] else { panic!("one effect: carry") };
    assert_eq!(show(&file, to, EXAMPLE), "VTI");
    assert_eq!(within, Span::days(30));

    let dynamic = parse_clean("law l\n  on flow\n  carry -gain to amount.unit within 30d\n");
    let law = dynamic.iter::<Law>().next().unwrap();
    let [Step { kind: StepKind::Effect(Effect::Carry { amount, to, within }), .. }] = &dynamic[law.steps][..] else {
        panic!("one dynamic-unit carry effect")
    };
    assert_eq!(show(&dynamic, *amount, dynamic.src), "(-gain)");
    assert_eq!(show(&dynamic, *to, dynamic.src), "amount.unit");
    assert_eq!(&dynamic.src[dynamic.exprs[*to].loc.range()], "amount.unit");
    assert_eq!(*within, Span::days(30));

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

// ─── The house style ────────────────────────────────────────────────────────

/// What a file says, and nothing of how it is laid out: the tree with the places
/// and the blanks taken out.
fn shape(src: &str, folder: Folder) -> String {
    let (file, diags) = crate::parse(FileId(0), src, folder);
    assert!(diags.is_empty(), "unexpected diagnostics:\n{}", render(src, &diags));
    let mut text = dump(&file);
    while let Some(start) = text.find("Loc {") {
        let end = start + text[start..].find('}').unwrap() + 1;
        text.replace_range(start..end, "");
    }
    // A tab between a number and its commodity is a blank the formatter makes one, and Debug writes it `\t`.
    text.retain(|c| !c.is_whitespace());
    text.replace("\\t", "").replace('\\', "")
}

/// Formats `src`, which must parse without error, and checks what formatting promises.
fn formatted(src: &str, folder: Folder) -> String {
    let file = crate::parse(FileId(0), src, folder).0;
    let once = crate::format(src, &file);
    assert_eq!(file.format(), once);
    let (after, before) = (shape(&once, folder), shape(src, folder));
    if after != before {
        let at = after.bytes().zip(before.bytes()).position(|(a, b)| a != b).unwrap_or(after.len().min(before.len()));
        let around = |text: &str| text[at.saturating_sub(100)..(at + 100).min(text.len())].to_string();
        panic!("formatting changed what is said, near\n  before: {}\n  after:  {}", around(&before), around(&after));
    }
    let again = crate::format(&once, &crate::parse(FileId(0), &once, folder).0);
    assert_eq!(again, once, "formatting is not idempotent");
    // What is not a journal line is left as it was, and nothing is added or lost.
    assert_eq!(once.lines().count(), src.lines().count());
    once
}

#[test]
fn the_house_style_puts_subjects_verbs_and_amounts_in_columns() {
    let src = r#"// A month.

2026-01-05 checking -> me 100 USD
2026-01-05   me  ->   taqueria-cancun 18.50 USD  #dining   "lunch"   // cash
2026-01-06 visa->trader-joes 84.20 USD via paypal for 2026-01 ^b ^a !
2026-01-31 checking = 8_828.87 USD
2026-01-31   visa =   2_333.99  USD   // owed
2026-01-31 retirement = 58_420.18 USD via market

2026-01-08 phone  47.30 USD "bill"
2026-01-09 flat
2026-01-10 checking -> savings 400 USD for emergency
2026-01-11 flat now 3_050 USD monthly until 05-31 "renewed" ^promo
2026-01-12 halcyon owes studio due 30d ^inv-1
  3_000 USD #design "brand refresh"
      800 USD #design "icon set"    // small
    + 5% of ^inv-1 #tax
2026-01-15 job
  retirement 276.00 USD ^x #y
  blue-shield    184.20 USD // note
  /// documented
  checking ...
  dana = 5 USD

opening 2026-01-01
  checking 6_062.55 USD
  fidelity 210 VTI basis 48_300 USD since 2021-06-01
  condo basis 402_000 USD since 2024-02-20
  jo owes me 600 USD due 04-01
"#;
    let expected = r#"// A month.

2026-01-05 checking   -> me              100 USD
2026-01-05 me         -> taqueria-cancun 18.50 USD #dining "lunch"  // cash
2026-01-06 visa       -> trader-joes     84.20 USD ^b ^a for 2026-01 via paypal !
2026-01-31 checking   =  8_828.87 USD
2026-01-31 visa       =  2_333.99 USD                               // owed
2026-01-31 retirement =  58_420.18 USD             via market

2026-01-08 phone    47.30 USD "bill"
2026-01-09 flat
2026-01-10 checking ->   savings           400 USD for emergency
2026-01-11 flat     now  3_050 USD monthly         until 05-31 "renewed" ^promo
2026-01-12 halcyon  owes studio                    ^inv-1 due 30d
  3_000 USD #design "brand refresh"
    800 USD #design "icon set"      // small
  + 5% of ^inv-1 #tax
2026-01-15 job
  retirement  276.00 USD #y ^x
  blue-shield 184.20 USD    // note
  /// documented
  checking ...
  dana     = 5 USD

opening 2026-01-01
  checking 6_062.55 USD
  fidelity      210 VTI basis 48_300 USD since 2021-06-01
  condo                 basis 402_000 USD since 2024-02-20
  jo owes me 600 USD due 04-01
"#;
    assert_eq!(formatted(src, Folder::default()), expected);
}

#[test]
fn formatting_keeps_what_is_not_a_journal_line_and_what_has_no_tree() {
    // A file with an error: what parsed is laid out, the rest is left alone.
    let src = "// kept\nbase USD\n\n2026-01-05   a ->   b 5 USD  \"x\"\n2026-01-05 a -> b 5\n\n  stray\n2026-01-06 a -> b 6 USD   #food\n";
    let (file, diags) = parse(FileId(0), src);
    assert!(!diags.is_empty());
    let once = crate::format(src, &file);
    assert_eq!(
        once,
        "// kept\nbase USD\n\n2026-01-05 a -> b 5 USD \"x\"\n2026-01-05 a -> b 5\n\n  stray\n2026-01-06 a -> b 6 USD #food\n"
    );
    // Line endings and a last line with none stay as written.
    let src = "2026-01-05   a -> b 5 USD\r\n2026-01-06 a -> b 6 USD  // c\r\n2026-01-07  a -> b 7 USD";
    let once = formatted(src, Folder::default());
    assert_eq!(once, "2026-01-05 a -> b 5 USD\r\n2026-01-06 a -> b 6 USD  // c\r\n2026-01-07 a -> b 7 USD");
    assert_eq!(formatted("", Folder::default()), "");
    // A blank or a comment between lines makes two blocks, each as wide as it needs.
    let once = formatted("2026-01-05 a -> b 5 USD\n// x\n2026-01-05 long-name -> b 5 USD\n", Folder::default());
    assert_eq!(once, "2026-01-05 a -> b 5 USD\n// x\n2026-01-05 long-name -> b 5 USD\n");
}

#[test]
fn a_tail_says_its_clauses_in_one_order() {
    let src = "2026-01-05 a -> b 5 USD via c basis 1 USD against ^z due 30d for 2025 ^k ^j \"why\" #food of x ! \"ok\"\n";
    let once = formatted(src, Folder::default());
    assert_eq!(once, "2026-01-05 a -> b 5 USD #food of x \"why\" ^k ^j for 2025 due 30d against ^z via c basis 1 USD ! \"ok\"\n");
    // A price stays with the amount, a change's span before the rest, and a spread is part of the date.
    let src = "2026-01-05 a -> b 7 VTI ^k @ 285.70 USD #buy\n\n2026-01-05 a now 5 USD monthly ^k \"x\" until 05-31\n\n2026-01-01..2026-12-31 a -> b 5 USD ^k #food\n";
    let once = formatted(src, Folder::default());
    assert_eq!(
        once,
        "2026-01-05 a -> b 7 VTI @ 285.70 USD #buy ^k\n\n2026-01-05 a now 5 USD monthly until 05-31 \"x\" ^k\n\n2026-01-01..2026-12-31 a -> b 5 USD #food ^k\n"
    );
}

/// Files that must format to what they say, and to themselves.
#[test]
fn the_sketch_the_examples_and_every_verb_format_idempotently() {
    formatted(EXAMPLE, Folder::default());
    formatted(STATEMENTS, Folder::default());
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/v4-sketch");
    for path in ax_files(&root, &root) {
        let src = std::fs::read_to_string(root.join(&path)).unwrap();
        let folder = Folder::of(path.to_str().unwrap());
        let once = formatted(&src, folder);
        // Comments and blank lines are all still there, in order.
        let others = |text: &str| -> Vec<String> {
            text.lines().filter(|line| line.trim().is_empty() || line.trim_start().starts_with("//")).map(str::to_string).collect()
        };
        let comments = |text: &str| text.lines().filter(|line| line.contains("//")).count();
        assert_eq!(comments(&once), comments(&src), "{}", path.display());
        assert_eq!(others(&once).len(), others(&src).len(), "{}", path.display());
    }
}

/// Whatever a damaged file leaves of its tree formats without a panic, and one
/// that still parses formats to what it says.
#[test]
fn damaged_files_format_without_panicking_and_without_changing_what_they_say() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/v4-sketch");
    let mut sources = vec![EXAMPLE.to_string(), STATEMENTS.to_string()];
    for name in ["journal/2026/01.ax", "journal/2026/03.ax"] {
        sources.push(std::fs::read_to_string(root.join(name)).unwrap());
    }
    for source in sources {
        damaged(&source, 300, |text| {
            let (file, diags) = parse(FileId(0), text);
            let once = crate::format(text, &file);
            if !diags.iter().any(Diagnostic::is_error) {
                formatted(text, Folder::default());
            } else {
                assert_eq!(once.lines().count(), text.lines().count());
            }
        });
    }
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
    // An amount is two words: a literal's text, or an expression's id in the space a null text leaves.
    assert!(size_of::<Many<Leg>>() == 8 && size_of::<Amount>() == 16 && size_of::<Literal>() == 16 && size_of::<Name>() == 16);
    assert!(size_of::<Quantity>() <= 24 && size_of::<Side>() <= 48, "Quantity is {}", size_of::<Quantity>());
}

/// An expression written out: its leaves as they are, and the other nodes by
/// where they were written and how far into the subtree their first node is, so
/// that no piece number shows.
fn dump_expr(file: &File, root: ExprId) -> String {
    let nodes = file.exprs.subtree(root);
    let leaves = nodes.iter().map(|node| match children(file, &node.kind).is_empty() {
        true => format!("{:?} {:?}", node.kind, node.loc),
        false => format!("{:?} first={}", node.loc, node.first.offset_from(nodes[0].first)),
    });
    leaves.collect::<Vec<_>>().join(" ")
}

fn dump_amount(file: &File, amount: &Amount) -> String {
    match amount {
        Amount::Computed(root) => format!("Computed({})", dump_expr(file, *root)),
        other => format!("{other:?}"),
    }
}

fn dump_quantity(file: &File, quantity: &Quantity) -> String {
    match quantity {
        Quantity::Amount(amount) => format!("Amount({})", dump_amount(file, amount)),
        Quantity::Pending(amount) => format!("Pending({})", dump_amount(file, amount)),
        Quantity::Target(amount) => format!("Target({})", dump_amount(file, amount)),
        other => format!("{other:?}"),
    }
}

/// A tail's clauses, by what they say and not in the order they were written:
/// that is what a formatter may change.
fn dump_tail(file: &File, tail: Many<Clause>) -> String {
    let clause = |clause: &Clause| {
        let says = match clause.kind {
            ClauseKind::Basis(amount) => format!("Basis({})", dump_amount(file, &amount)),
            other => format!("{other:?}"),
        };
        (says.clone(), format!("{:?} {says}", clause.at))
    };
    let mut clauses: Vec<(String, String)> = file[tail].iter().map(clause).collect();
    clauses.sort();
    clauses.into_iter().map(|(_, written)| written).collect::<Vec<_>>().join(", ")
}

fn dump_item(file: &File, item: &LineItem) -> String {
    format!("{:?} {:?} {} {} {:?}", item.doc, item.sign, dump_amount(file, &item.amount), dump_tail(file, item.tail), item.loc)
}

fn dump_body(file: &File, body: Body) -> String {
    let end = |end: &End| format!("{:?}{:?}", end.name, &file[end.select]);
    let leg = |leg: &Leg| {
        format!("{:?} {} {} {} {:?}", leg.doc, end(&leg.end), dump_quantity(file, &leg.amount), dump_tail(file, leg.tail), leg.loc)
    };
    let legs: Vec<String> = file[body.legs].iter().map(leg).collect();
    let items: Vec<String> = file[body.items].iter().map(|item| dump_item(file, item)).collect();
    format!("{} / {}", legs.join("; "), items.join("; "))
}

fn dump_flow(file: &File, flow: &Flow) -> String {
    let end = |end: &End| format!("{:?}{:?}", end.name, &file[end.select]);
    let side = |side: &Side| {
        format!("{:?} {:?}", side.end.as_ref().map(end), side.amount.as_ref().map(|amount| dump_quantity(file, amount)))
    };
    format!("{} -> {} {} [{}]", side(&flow.from), side(&flow.to), dump_tail(file, flow.tail), dump_body(file, flow.body))
}

fn dump_prop(file: &File, prop: &Prop) -> String {
    let args: Vec<String> = file[prop.args].iter().map(|&arg| dump_expr(file, arg)).collect();
    let lines: Vec<String> = file[prop.lines].iter().map(|nested| dump_prop(file, &nested.0)).collect();
    format!("{:?} {args:?} {lines:?} {:?}", prop.name, prop.loc)
}

fn dump_terms(file: &File, terms: &Terms) -> String {
    let payment = match terms.payment {
        Some(Payment::Fixed(amount)) => format!("Fixed({})", dump_amount(file, &amount)),
        Some(Payment::Buy { unit, spend }) => format!("Buy({unit:?} {})", dump_amount(file, &spend)),
        None => "None".to_string(),
    };
    format!("{:?} {payment} {:?} {:?} {:?}", terms.about, terms.cadence, &file[terms.on], terms.holding)
}

fn dump_allowance(file: &File, allowance: &Allowance) -> String {
    let limit = match &allowance.limit {
        Limit::Amount(amount) => dump_amount(file, amount),
        other => format!("{other:?}"),
    };
    format!("{limit} {:?} {} {:?}", allowance.per, allowance.carries, allowance.funded)
}

fn dump_law(file: &File, law: &Law) -> String {
    let steps: Vec<String> = file[law.steps].iter().map(|step| format!("{:?}", step.loc)).collect();
    let mut roots = Vec::new();
    law_roots(file, law, &mut roots);
    let roots: Vec<String> = roots.into_iter().map(|root| dump_expr(file, root)).collect();
    format!("{:?} {:?} {:?} {:?} {:?} {steps:?} {roots:?}", law.doc, law.name, law.overrides, law.trigger_loc, law.loc)
}

fn dump_also(file: &File, also: &Also) -> String {
    let line = match &also.line {
        AlsoLine::Item(item) => dump_item(file, item),
        AlsoLine::Flow(flow) => dump_flow(file, flow),
    };
    format!("{line} when {:?} {:?}", also.when.map(|root| dump_expr(file, root)), also.loc)
}

fn dump_statement(file: &File, said: &Statement) -> String {
    let verb = match &said.verb {
        Verb::Occurrence(amount) => format!("Occurrence({:?})", amount.as_ref().map(|amount| dump_amount(file, amount))),
        Verb::Value(amount) => format!("Value({})", dump_amount(file, amount)),
        Verb::Owes { creditor, amount } => {
            format!("Owes({creditor:?} {:?})", amount.as_ref().map(|amount| dump_amount(file, amount)))
        }
        Verb::Basis { amount, since } => format!("Basis({} {since:?})", dump_amount(file, amount)),
        Verb::Now(Change::Terms(id)) => format!("Terms({})", dump_terms(file, &file[*id])),
        Verb::Now(Change::Property(prop)) => format!("Property({})", dump_prop(file, prop)),
        Verb::Now(Change::Budget(id)) => format!("Budget({})", dump_allowance(file, &file[*id])),
        other => format!("{other:?}"),
    };
    format!("{:?} {:?} {verb} {} {}", said.date, said.subject, dump_tail(file, said.tail), dump_body(file, said.body))
}

fn dump_format(file: &File, format: &Format) -> String {
    let lines: Vec<String> = file[format.lines]
        .iter()
        .map(|line| format!("{:?} {:?} {:?}", line.key, &file[line.args], line.loc))
        .collect();
    format!("{:?} {lines:?}", format.name)
}

/// Everything an item reaches, written out with every range and id followed,
/// so that two files are equal when they say the same, whichever pieces their
/// tables are kept in.
fn dump(file: &File) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let props = |props: Many<Prop>| -> Vec<String> { file[props].iter().map(|prop| dump_prop(file, prop)).collect() };
    let patterns = |patterns: Many<Pattern>| -> Vec<String> { file[patterns].iter().map(|pattern| spell(file, pattern)).collect() };
    let alsos = |alsos: Many<Also>| -> Vec<String> { file[alsos].iter().map(|also| dump_also(file, also)).collect() };
    let laws = |laws: Many<Law>| -> Vec<String> { file[laws].iter().map(|law| dump_law(file, law)).collect() };
    for item in &file.items {
        write!(out, "{:?} {:?} ", item.loc, item.doc).unwrap();
        let text = match item.kind {
            ItemKind::Txn(id) => format!("{:?} {}", file[id].date, dump_flow(file, &file[id].flow)),
            ItemKind::Statement(id) => dump_statement(file, &file[id]),
            ItemKind::Setting(id) => format!("{:?}", file[id]),
            ItemKind::Sync(id) => {
                let sync = &file[id];
                let format = sync.format.map(|format| dump_format(file, &file[format]));
                format!("{:?} {:?} {:?} {:?} {format:?}", sync.name, sync.read, sync.run, sync.into)
            }
            ItemKind::Format(id) => dump_format(file, &file[id]),
            ItemKind::Pattern(id) => format!("{:?} {}", file[id].name, spell(file, &file[id].pattern)),
            ItemKind::Budget(id) => format!("{:?} {}", file[id].purpose, dump_allowance(file, &file[id].allowance)),
            ItemKind::Opening(id) => {
                let claims: Vec<String> = file[file[id].claims].iter().map(|claim| dump_statement(file, claim)).collect();
                format!("{} {claims:?}", dump_body(file, Body { legs: file[id].lines, items: Many::EMPTY }))
            }
            ItemKind::Contract(id) => {
                let Contract {
                    name,
                    party,
                    schedule: schedule_field,
                    standing,
                    purpose,
                    description,
                    deadline,
                    alsos: also,
                    props: lines,
                    body,
                    laws: nested,
                    damaged,
                } = &file[id];
                let schedule = |schedule: &Option<Schedule>| {
                    schedule.map(|s| format!("{:?} {}", s.at, dump_terms(file, &s.terms)))
                };
                let deadline = deadline.as_ref().map(|d| format!("{:?} {:?}", d.span, d.otherwise.as_ref().map(|item| dump_item(file, item))));
                format!(
                    "{name:?} {party:?} {:?} {:?} {purpose:?} {description:?} {deadline:?} {:?} {:?} {} {:?} {damaged}",
                    schedule(schedule_field),
                    schedule(standing),
                    alsos(*also),
                    props(*lines),
                    dump_body(file, *body),
                    laws(*nested),
                )
            }
            ItemKind::Code(id) => {
                let rule = &file[id];
                format!("{:?} {:?} {:?}", rule.pattern, &file[rule.on], patterns(rule.known_as))
            }
            ItemKind::Law(id) => dump_law(file, &file[id]),
            ItemKind::Param(id) => {
                let rows = file[file[id].rows].iter();
                let rows: Vec<String> =
                    rows.map(|row| format!("{:?} {}", &file[row.keys], dump_expr(file, row.value))).collect();
                format!("{:?} {:?} {rows:?}", file[id].name, file[id].unit)
            }
            ItemKind::Decl(id) => {
                let decl = &file[id];
                let budget = decl.budget.map(|budget| dump_allowance(file, &file[budget]));
                format!(
                    "{:?} {:?} {:?} {:?} {budget:?} {:?} {:?} {:?} {:?}",
                    decl.name,
                    decl.at,
                    decl.purpose,
                    decl.kind,
                    patterns(decl.known_as),
                    alsos(decl.alsos),
                    props(decl.props),
                    laws(decl.laws)
                )
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
    same(STATEMENTS);
    same(&STATEMENTS.repeat(3));
    damaged(STATEMENTS, 300, same);
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
