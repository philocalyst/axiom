use axiom_core::Day;

use super::*;

const USD: Unit = Unit { name: "USD", scale: 2 };
const EUR: Unit = Unit { name: "EUR", scale: 2 };

fn day(text: &str) -> Day {
    Day::parse(text.as_bytes()).unwrap()
}

fn name(text: &str) -> Place {
    Place::Name(text.into())
}

fn path(text: &str) -> Place {
    Place::Path(text.into())
}

fn layout(text: &str) -> DateFormat {
    DateFormat::new(text).unwrap()
}

fn rows(specs: Vec<Spec>) -> Format {
    Format { shape: Shape::Rows, specs, categories: Vec::new() }
}

/// `date "Posting Date" "MM/DD/YYYY"`, `amount "Amount"`, `memo "Description"`, `balance "Balance"`.
fn chase(date: &str, pattern: &str) -> Format {
    rows(vec![
        Spec::new(Field::Date, [name(date)]).layout(layout(pattern)),
        Spec::new(Field::Amount, [name("Amount")]),
        Spec::new(Field::Memo, [name("Description")]),
        Spec::new(Field::Balance, [name("Balance")]),
    ])
}

/// `format ofx` as std declares it (crates/sync/std/formats.ax).
fn ofx() -> Format {
    Format {
        shape: Shape::Tagged { records: "STMTTRN".into() },
        specs: vec![
            Spec::new(Field::Date, [path("DTPOSTED")]).layout(layout("YYYYMMDD")),
            Spec::new(Field::Amount, [path("TRNAMT")]),
            Spec::new(Field::Memo, [path("NAME"), path("MEMO")]),
            Spec::new(Field::Code, [path("CHECKNUM")]),
        ],
        categories: Vec::new(),
    }
}

/// `format camt053` as std declares it.
fn camt053() -> Format {
    Format {
        shape: Shape::Tagged { records: "Ntry".into() },
        specs: vec![
            Spec::new(Field::Date, [path("BookgDt/Dt")]),
            Spec::new(Field::Amount, [path("Amt")]).rule(Rule::Sign { place: path("CdtDbtInd"), into: "CRDT".into() }),
            Spec::new(Field::Memo, [path("AddtlNtryInf"), path("RmtInf/Ustrd")]),
            Spec::new(Field::Pending, [path("Sts")]).rule(Rule::Is("PDNG".into())),
            Spec::new(Field::Code, [path("NtryDtls/TxDtls/RmtInf/Strd/CdtrRefInf/Ref")]),
            Spec::new(Field::Via, [path("NtryDtls/TxDtls/RltdPties/UltmtCdtr/Nm")]),
        ],
        categories: Vec::new(),
    }
}

fn read<'t>(format: &Format, text: &'t str) -> Vec<Record<'t>> {
    let (records, problems) = format.read(text, FileId(0), USD, &[USD, EUR]);
    assert!(problems.is_empty(), "{problems:?}");
    records
}

fn problems(format: &Format, text: &str) -> Vec<(String, String)> {
    let (_, problems) = format.read(text, FileId(3), USD, &[USD, EUR]);
    problems.iter().map(|problem| (problem.code.to_string(), problem.message.clone())).collect()
}

#[test]
fn a_bank_export_reads_quotes_crlf_and_the_byte_order_mark() {
    let text = "\u{feff}Posting Date,Description,Amount,Balance\r\n\
                01/05/2026,\"TRADER JOE'S, #634 \"\"SF\"\"\",-84.20,\"8,915.80\"\r\n\
                \r\n\
                01/06/2026 , plain memo ,\"1,000.00\",9915.80\r\n";
    let records = read(&chase("Posting Date", "MM/DD/YYYY"), text);
    let shown: Vec<_> = records.iter().map(|r| (r.day, r.qty.0, r.memo.as_ref(), r.balance.map(|b| b.0))).collect();
    assert_eq!(
        shown,
        [
            (day("2026-01-05"), -8420, "TRADER JOE'S, #634 \"SF\"", Some(891_580)),
            (day("2026-01-06"), 100_000, "plain memo", Some(991_580)),
        ]
    );
}

#[test]
fn columns_by_position_need_no_header_but_may_have_one() {
    let format = rows(vec![
        Spec::new(Field::Date, [Place::Index(1)]),
        Spec::new(Field::Amount, [Place::Index(4)]).rule(Rule::Flipped),
        Spec::new(Field::Memo, [Place::Index(3)]),
        Spec::new(Field::Pending, [Place::Index(2)]),
    ]);
    for text in ["date,status,memo,amount\n2026-01-05,pending,Coffee,4.50\n", "2026-01-05,Pending,Coffee,4.50"] {
        let records = read(&format, text);
        assert_eq!((records[0].qty.0, records[0].pending, records[0].memo.as_ref()), (-450, true, "Coffee"), "{text}");
    }
}

#[test]
fn debit_and_credit_columns() {
    let format = rows(vec![
        Spec::new(Field::Date, [name("Date")]),
        Spec::new(Field::Debit, [name("Debit")]),
        Spec::new(Field::Credit, [name("Credit")]),
        Spec::new(Field::Memo, [name("Memo")]),
    ]);
    let text = "Date,Memo,Debit,Credit\n2026-01-05,rent,2900.00,\n2026-01-06,pay,,3054.70\n2026-01-07,both,1.00,2.00\n";
    let (records, problems) = format.read(text, FileId(0), USD, &[]);
    assert_eq!(records.iter().map(|r| r.qty.0).collect::<Vec<_>>(), [-290_000, 305_470]);
    assert_eq!(problems.len(), 1);
    assert!(problems[0].message.starts_with("row 4: both the debit and the credit"), "{}", problems[0].message);
}

#[test]
fn what_cannot_be_read_is_reported_at_its_cell() {
    let format = chase("Posting Date", "MM/DD/YYYY");
    let text = "Posting Date,Description,Amount,Balance\n\
                25/12/2026,a,1.00,1\n\
                01/05/2026,b,twelve,1\n\
                01/06/2026,\"c\" x,1.00,1\n\
                01/07/2026,short\n\
                01/08/2026,\"never closed,1.00,1\n";
    let (records, problems) = format.read(text, FileId(3), USD, &[]);
    assert!(records.is_empty());
    let shown: Vec<_> = problems.iter().map(|p| (p.code.as_ref(), p.message.as_str())).collect();
    assert_eq!(
        shown,
        [
            ("bad-date", "row 2: `25/12/2026` is not a date written MM/DD/YYYY"),
            ("bad-amount", "row 3: `twelve` is not an amount"),
            ("bad-csv", "row 4: text follows the closing quote"),
            ("short-row", "row 5: has 2 columns, but the \"Amount\" column is number 3"),
            ("bad-csv", "row 6: the quote is never closed"),
        ]
    );
    assert_eq!(problems[0].help[0].text, "if the day comes first, write the pattern as \"DD/MM/YYYY\"");
    let cell = problems[1].anchor().unwrap();
    assert_eq!((cell.file, &text[cell.range()]), (FileId(3), "twelve"));
}

#[test]
fn a_missing_column_says_what_the_export_has() {
    let text = "Posting Date,Description,Amount,Balance\n01/05/2026,a,1.00,1\n";
    let format = chase("Posting Dat", "MM/DD/YYYY");
    assert_eq!(
        problems(&format, text),
        [("no-such-column".to_string(), "row 1: the export has no column \"Posting Dat\"".to_string())]
    );
    let (_, all) = format.read(text, FileId(0), USD, &[]);
    assert_eq!(all[0].notes[0], "its columns are \"Posting Date\", \"Description\", \"Amount\", \"Balance\"");
    assert_eq!(all[0].help[0].text, "did you mean \"Posting Date\"?");
}

#[test]
fn a_column_that_fails_every_row_stops_the_reading() {
    let text = format!("Date,Description,Amount,Balance\n{}", "01/05/2026,a,1.00,1\n".repeat(50));
    let (_, all) = chase("Date", "YYYY-MM-DD").read(&text, FileId(0), USD, &[]);
    assert_eq!(all.len(), MAX_PROBLEMS);
    assert_eq!(all.last().unwrap().notes, ["the rest of the export was not read"]);
}

#[test]
fn garbage_never_panics() {
    let format = rows(vec![
        Spec::new(Field::Date, [name("A")]),
        Spec::new(Field::Amount, [name("B")]),
        Spec::new(Field::Memo, [name("C")]),
    ]);
    for text in ["", "\n\n", "\"", "A,B,C\n\"", ",,,\n,,", "A,B,C\n\u{0}\u{1},\u{ff}", "\u{feff}", "A\n,\"\"\"\"\"\n"] {
        let _ = format.read(text, FileId(0), USD, &[]);
        let _ = ofx().read(text, FileId(0), USD, &[]);
    }
}

#[test]
fn a_declaration_that_cannot_work_is_refused_with_why() {
    let bad = |format: Format| format.check().unwrap_err();
    let (date, amount) = (Spec::new(Field::Date, [name("D")]), Spec::new(Field::Amount, [name("A")]));
    assert_eq!(bad(rows(vec![amount.clone()])), "the format names no date");
    assert_eq!(bad(rows(vec![date.clone()])), "the format names no amount: `amount`, `debit` and `credit`, or `gross`");
    assert_eq!(bad(rows(vec![date.clone(), amount.clone(), amount.clone()])), "`amount` is declared twice");
    assert_eq!(bad(rows(vec![date.clone(), Spec::new(Field::Debit, [name("D")])])), "`debit` and `credit` go together");
    assert_eq!(
        bad(rows(vec![date.clone(), Spec::new(Field::Amount, [Place::Index(0)])])),
        "columns are counted from 1"
    );
    assert_eq!(
        bad(rows(vec![date.clone(), Spec::new(Field::Amount, [path("A")])])),
        "`amount` names a tag; a csv format names columns"
    );
    assert_eq!(
        bad(rows(vec![date.clone(), Spec::new(Field::Amount, [name("A"), name("B")])])),
        "`amount` is one column or tag; only `memo` may name several"
    );
    assert_eq!(bad(rows(vec![date.clone(), amount.clone().rule(Rule::Is("x".into()))])), "`amount` cannot take that");
    let mut tagged = ofx();
    tagged.specs.push(Spec::new(Field::Memo, [name("X")]));
    assert_eq!(bad(tagged), "`memo` is declared twice");
    let mut tagged = ofx();
    tagged.specs[1] = Spec::new(Field::Amount, [name("X")]);
    assert_eq!(bad(tagged), "`amount` names a column; a tagged format names tags");
    assert!(rows(vec![date, amount]).check().is_ok() && ofx().check().is_ok() && camt053().check().is_ok());
    let (records, problems) = rows(vec![]).read("a,b", FileId(0), USD, &[]);
    assert!(records.is_empty() && problems[0].message == "the format names no date");
}

/// A version 1 file: SGML, tags unclosed.
const CHECKING: &str = "OFXHEADER:100\nDATA:OFXSGML\nVERSION:102\n\n\
<OFX>\n<BANKMSGSRSV1><STMTTRNRS><STMTRS><CURDEF>USD\n<BANKTRANLIST>\n\
<STMTTRN>\n<TRNTYPE>DEBIT\n<DTPOSTED>20260105120000[-5:EST]\n<TRNAMT>-84.20\n<FITID>2026010501\n<NAME>TRADER JOE'S #634\n<MEMO>SAN FRANCISCO CA\n</STMTTRN>\n\
<STMTTRN>\n<TRNTYPE>CHECK\n<DTPOSTED>20260106\n<TRNAMT>-350.00\n<FITID>2026010601\n<CHECKNUM>1041\n<NAME>BAY PLUMBING &amp; HEATING\n</STMTTRN>\n\
<STMTTRN>\n<TRNTYPE>CREDIT\n<DTPOSTED>20260108\n<TRNAMT>3800.00\n<FITID>2026010801\n<NAME>HALCYON PAYMENT\n<MEMO>\n</STMTTRN>\n\
</BANKTRANLIST>\n<LEDGERBAL><BALAMT>3162.55\n<DTASOF>20260112120000\n</LEDGERBAL>\n</STMTRS></STMTTRNRS></BANKMSGSRSV1>\n</OFX>\n";

#[test]
fn a_version_1_ofx_statement_is_read_by_the_declared_format() {
    let records = read(&ofx(), CHECKING);
    let shown: Vec<_> =
        records.iter().map(|r| (r.day.to_string(), r.qty.0, r.memo.to_string(), r.facts().code.clone())).collect();
    assert_eq!(
        shown,
        [
            ("2026-01-05".to_string(), -8420, "TRADER JOE'S #634 SAN FRANCISCO CA".to_string(), None),
            ("2026-01-06".to_string(), -35_000, "BAY PLUMBING & HEATING".to_string(), Some("1041".into())),
            ("2026-01-08".to_string(), 380_000, "HALCYON PAYMENT".to_string(), None),
        ]
    );
    assert_eq!(&CHECKING[records[0].at.range()], "TRADER JOE'S #634", "the memo is located at its first tag");
}

#[test]
fn a_version_2_ofx_statement_is_read_by_the_same_format() {
    let text = "<?xml version=\"1.0\"?><OFX><CREDITCARDMSGSRSV1><CCSTMTTRNRS><CCSTMTRS><BANKTRANLIST>\
<STMTTRN><TRNTYPE>DEBIT</TRNTYPE><DTPOSTED>20260107</DTPOSTED><TRNAMT>-84.20</TRNAMT><NAME>TRADER JOE'S</NAME></STMTTRN>\
</BANKTRANLIST></CCSTMTRS></CCSTMTTRNRS></CREDITCARDMSGSRSV1></OFX>";
    let records = read(&ofx(), text);
    assert_eq!(
        (records[0].day, records[0].qty.0, records[0].memo.as_ref()),
        (day("2026-01-07"), -8420, "TRADER JOE'S")
    );
}

/// An ISO 20022 statement: an entry a customer was credited, one still pending.
const CAMT: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<Document xmlns=\"urn:iso:std:iso:20022:tech:xsd:camt.053.001.02\"><BkToCstmrStmt><Stmt><Id>S1</Id>\n\
<Ntry><Amt Ccy=\"USD\">1250.00</Amt><CdtDbtInd>CRDT</CdtDbtInd><Sts>BOOK</Sts><BookgDt><Dt>2026-01-05</Dt></BookgDt>\n\
<AddtlNtryInf>PAYPAL PAYOUT</AddtlNtryInf><NtryDtls><TxDtls><Amt Ccy=\"USD\">1250.00</Amt><RltdPties><UltmtCdtr><Nm>Etsy Seller</Nm></UltmtCdtr></RltdPties>\
<RmtInf><Ustrd>ORDER 77</Ustrd><Strd><CdtrRefInf><Ref>INV-2026-01</Ref></CdtrRefInf></Strd></RmtInf></TxDtls></NtryDtls></Ntry>\n\
<Ntry><Amt Ccy=\"USD\">47.30</Amt><CdtDbtInd>DBIT</CdtDbtInd><Sts>PDNG</Sts><BookgDt><Dt>2026-01-06</Dt></BookgDt><AddtlNtryInf>MINT MOBILE</AddtlNtryInf></Ntry>\n\
</Stmt></BkToCstmrStmt></Document>";

#[test]
fn a_camt053_statement_says_its_sign_its_status_its_reference_and_its_ultimate_party() {
    let records = read(&camt053(), CAMT);
    assert_eq!(records.len(), 2);
    let (first, second) = (&records[0], &records[1]);
    let shown = |record: &Record| (record.day, record.qty.0, record.memo.to_string(), record.pending);
    assert_eq!(shown(first), (day("2026-01-05"), 125_000, "PAYPAL PAYOUT ORDER 77".to_string(), false));
    assert_eq!(
        (first.facts().code.as_deref(), first.facts().via.as_deref()),
        (Some("inv-2026-01"), Some("Etsy Seller"))
    );
    assert_eq!(shown(second), (day("2026-01-06"), -4730, "MINT MOBILE".to_string(), true));
    assert_eq!(second.facts, None, "a record that says nothing more has no facts");
}

#[test]
fn tagged_problems_point_at_the_tag_and_say_which_record() {
    let text = "<OFX><STMTTRN><DTPOSTED>soon<TRNAMT>1.00<NAME>a</STMTTRN>\
<STMTTRN><DTPOSTED>20260105<TRNAMT>12,5<NAME>b</STMTTRN>\
<STMTTRN><DTPOSTED>20260105<NAME>c</STMTTRN>\
<STMTTRN><TRNAMT>1.00</STMTTRN></OFX>";
    let (records, all) = ofx().read(text, FileId(4), USD, &[]);
    assert!(records.is_empty());
    let shown: Vec<_> = all.iter().map(|problem| problem.message.as_str()).collect();
    assert_eq!(
        shown,
        [
            "record 1: `soon` is not a date written YYYYMMDD",
            "record 2: `12,5` is not an amount",
            "record 3: there is no amount",
            "record 4: it has no DTPOSTED",
        ]
    );
    let at = all[1].anchor().unwrap();
    assert_eq!((at.file, &text[at.range()]), (FileId(4), "12,5"));
    assert_eq!(problems(&ofx(), "Posting Date,Amount\n")[0].1, "record 0: there are no tags in it");
    let no_sign = "<Ntry><Amt>5</Amt><BookgDt><Dt>2026-01-05</Dt></BookgDt></Ntry>";
    assert_eq!(problems(&camt053(), no_sign)[0].1, "record 1: it has no sign for its amount");
}

#[test]
fn what_a_record_says_besides_its_amount_becomes_its_facts() {
    let mut format = rows(vec![
        Spec::new(Field::Date, [name("Date")]),
        Spec::new(Field::Gross, [name("Gross")]),
        Spec::new(Field::Fee, [name("Fee")]),
        Spec::new(Field::Memo, [name("Memo")]),
        Spec::new(Field::Code, [name("Ref")]),
        Spec::new(Field::Id, [name("Id")]),
        Spec::new(Field::Party, [name("Who")]),
        Spec::new(Field::Currency, [name("Currency")]),
        Spec::new(Field::Category, [name("Category")]),
        Spec::new(Field::Object, [name("Object")]),
        Spec::new(Field::Route, [name("Card")]),
    ]);
    format.categories.push(("Groceries".into(), "groceries".into()));
    let header = "Date,Gross,Fee,Memo,Ref,Id,Who,Currency,Category,Object,Card\n";
    let text = format!(
        "{header}2026-01-05,100.00,3.20,STRIPE PAYOUT,Order #77,tx1,Etsy Seller,usd,Groceries,laptop,visa\n\
         2026-01-06,50.00,0,plain,,,,eur,,,\n"
    );
    let records = read(&format, &text);
    assert_eq!(records[0].qty.0, 9680, "what arrives is the gross less the fee");
    let facts = records[0].facts();
    assert_eq!((facts.gross.map(|q| q.0), facts.fee.map(|q| q.0)), (Some(10_000), Some(320)));
    assert_eq!(
        (facts.code.as_deref(), facts.id.as_deref(), facts.party.as_deref()),
        (Some("order--77"), Some("tx1"), Some("Etsy Seller"))
    );
    assert_eq!(
        (facts.category.as_deref(), facts.object.as_deref(), facts.route.as_deref()),
        (Some("Groceries"), Some("laptop"), Some("visa"))
    );
    assert_eq!(facts.currency, None, "the account's own unit is not a fact");
    assert_eq!(format.purpose("groceries"), Some("groceries"));
    assert_eq!(format.purpose("Rent"), None);
    assert_eq!(records[1].facts().currency.as_deref(), Some("EUR"));
    let (_, unknown) = format.read(&format!("{header}2026-01-05,1,0,m,,,,XYZ,,,\n"), FileId(0), USD, &[USD]);
    assert_eq!(unknown[0].message, "row 2: the book has no unit `XYZ`");
}

#[test]
fn codes_are_written_as_the_language_writes_them() {
    assert_eq!(code_of("INV 2026/01").as_deref(), Some("inv-2026/01"));
    assert_eq!(code_of("  #77 ").as_deref(), Some("77"));
    assert_eq!(code_of("--"), None);
    assert_eq!(code_of(""), None);
}

#[test]
#[ignore = "a timing, alone: cargo test -p axiom-sync --release -- --ignored --test-threads=1"]
fn a_million_rows() {
    let format = chase("Posting Date", "MM/DD/YYYY");
    let mut text = String::from("Posting Date,Description,Amount,Balance\n");
    for row in 0..1_000_000u32 {
        let (month, day) = (row % 12 + 1, row % 28 + 1);
        let quoted = if row % 5 == 0 { "\"TRADER JOE'S, #634 \"\"SF\"\"\"" } else { "SHELL OIL 5741" };
        text += &format!("{month:02}/{day:02}/2026,{quoted},-{}.{:02},\"1,234.56\"\n", row % 900, row % 100);
    }
    let started = std::time::Instant::now();
    let (records, problems) = format.read(&text, FileId(0), USD, &[]);
    let took = started.elapsed();
    eprintln!("read {} rows ({} MB) in {took:?}", records.len(), text.len() >> 20);
    assert!(problems.is_empty() && records.len() == 1_000_000);
}

#[test]
#[ignore = "a timing, alone: cargo test -p axiom-sync --release -- --ignored --test-threads=1"]
fn a_hundred_thousand_ofx_transactions() {
    let mut text = String::from("<OFX><BANKTRANLIST>\n");
    for at in 0..100_000u32 {
        let (month, day, whole, cents) = (at % 12 + 1, at % 28 + 1, at % 900, at % 100);
        text += &format!(
            "<STMTTRN><TRNTYPE>DEBIT<DTPOSTED>2026{month:02}{day:02}120000<TRNAMT>-{whole}.{cents:02}<FITID>{at}\
             <NAME>SHELL OIL {at}<MEMO>SAN FRANCISCO CA</STMTTRN>\n"
        );
    }
    text += "</BANKTRANLIST></OFX>";
    let started = std::time::Instant::now();
    let (records, problems) = ofx().read(&text, FileId(0), USD, &[]);
    eprintln!("read {} OFX transactions ({} MB) in {:?}", records.len(), text.len() >> 20, started.elapsed());
    assert!(problems.is_empty() && records.len() == 100_000);
}
