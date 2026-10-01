use super::*;
use axiom_core::{FileId, Loc, Qty, calendar::DateLayout};
use axiom_syntax::Folder;
use axiom_model::sync::{Column, Fetch, Field, Format, Rule, Shape, Sink, Source, Spec};

const USD: Unit<'static> = Unit {
    name: "USD",
    scale: 2,
};

fn spec(field: Field, places: impl Into<Box<[Column]>>) -> Spec {
    Spec {
        field,
        places: places.into(),
        layout: None,
        rule: Rule::None,
        loc: Loc::default(),
    }
}

fn book() -> axiom_model::Book<'static> {
    let std = include_str!("../../systems/src/std.ax");
    let sources = [("std.ax", std, true), ("axiom.ax", "base USD\n", false)].map(
        |(path, text, embedded)| {
            let (file, diagnostics) =
                axiom_syntax::parse(FileId(0), text, Folder::default());
            assert!(diagnostics.is_empty(), "{path}: {diagnostics:?}");
            axiom_model::Source {
                path,
                file,
                embedded,
            }
        },
    );
    let (book, diagnostics) = axiom_model::build(&sources);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    book
}

#[test]
fn typed_rows_read_amount_sign_memo_and_balance() {
    let mut book = book();
    let header = |book: &mut axiom_model::Book<'static>, text| {
        Column::Header(book.intern_text(text))
    };
    let date = header(&mut book, "Posting Date");
    let amount = header(&mut book, "Amount");
    let sign_column = header(&mut book, "Direction");
    let memo = header(&mut book, "Description");
    let balance = header(&mut book, "Balance");
    let into = book.intern_text("CRDT");
    let mut date_spec = spec(Field::Date, [date]);
    date_spec.layout = Some(DateLayout::parse("MM/DD/YYYY").unwrap());
    let mut amount_spec = spec(Field::Amount, [amount]);
    amount_spec.rule = Rule::Sign {
        place: sign_column,
        into,
    };
    let format = Format {
        name: book.names.intern("bank"),
        shape: Shape::Rows,
        specs: vec![
            date_spec,
            amount_spec,
            spec(Field::Memo, [memo]),
            spec(Field::Balance, [balance]),
        ]
        .into_boxed_slice(),
        categories: Box::default(),
        loc: Loc::default(),
    };
    let source =
        "Posting Date,Amount,Direction,Description,Balance\n03/04/2026,12.50,CRDT,Shop,100.00\n";
    let (records, problems) = read(&book, &format, source, FileId(0), USD, &[USD]);
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(records.len(), 1);
    assert_eq!(
        records[0].day,
        axiom_core::Day::from_ymd(2026, 3, 4).unwrap()
    );
    assert_eq!(records[0].qty, Qty(1250));
    assert_eq!(records[0].memo, "Shop");
    assert_eq!(records[0].balance, Some(Qty(10_000)));
}

#[test]
fn typed_tagged_paths_read_camt_style_records() {
    let mut book = book();
    let records = book.names.intern("Ntry");
    let date = Column::Path(book.intern_text("BookgDt/Dt"));
    let amount = Column::Path(book.intern_text("Amt"));
    let sign = Column::Path(book.intern_text("CdtDbtInd"));
    let memo = Column::Path(book.intern_text("AddtlNtryInf"));
    let remittance = Column::Path(book.intern_text("RmtInf/Ustrd"));
    let crdt = book.intern_text("CRDT");
    let mut amount_spec = spec(Field::Amount, [amount]);
    amount_spec.rule = Rule::Sign {
        place: sign,
        into: crdt,
    };
    let format = Format {
        name: book.names.intern("camt053"),
        shape: Shape::Tagged { records },
        specs: vec![
            spec(Field::Date, [date]),
            amount_spec,
            spec(Field::Memo, [memo, remittance]),
        ]
        .into_boxed_slice(),
        categories: Box::default(),
        loc: Loc::default(),
    };
    let source = "<Document><Ntry><BookgDt><Dt>2026-03-04</Dt></BookgDt><Amt>12.50</Amt><CdtDbtInd>CRDT</CdtDbtInd><AddtlNtryInf>Refund</AddtlNtryInf><RmtInf><Ustrd>Card credit</Ustrd></RmtInf></Ntry></Document>";
    let (records, problems) = read(&book, &format, source, FileId(0), USD, &[USD]);
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].qty, Qty(1250));
    assert_eq!(records[0].memo, "Refund Card credit");
    let format_id = book.formats.push(format.clone());
    let local_source = Source {
        name: book.names.intern("statement"),
        fetch: Fetch::Read(book.intern_text("imports/camt.xml")),
        format: Some(format_id),
        sink: Sink::Journal,
        system: None,
        doc: None,
        loc: Loc::default(),
    };
    let memos = read_memos(&book, &local_source, source, FileId(0)).unwrap();
    assert_eq!(memos, ["Refund Card credit"]);
}

#[test]
fn check_can_read_memos_from_local_sources_without_reconciling_rows() {
    let mut book = book();
    let memo = Column::Header(book.intern_text("Description"));
    let extra = Column::Header(book.intern_text("Extra"));
    let format = Format {
        name: book.names.intern("bank"),
        shape: Shape::Rows,
        specs: vec![spec(Field::Memo, [memo, extra])].into_boxed_slice(),
        categories: Box::default(),
        loc: Loc::default(),
    };
    let format_id = book.formats.push(format);
    let source = Source {
        name: book.names.intern("checking"),
        fetch: Fetch::Read(book.intern_text("imports/checking.csv")),
        format: Some(format_id),
        sink: Sink::Journal,
        system: None,
        doc: None,
        loc: Loc::default(),
    };
    let memos = read_memos(
        &book,
        &source,
        "Description,Extra\nTRADER JOE'S #10,Card\nAmazon 2K4LM,Monthly\n",
        FileId(7),
    )
    .unwrap();
    assert_eq!(memos, ["TRADER JOE'S #10 Card", "Amazon 2K4LM Monthly"]);
}

#[test]
fn checking_never_executes_a_run_source() {
    let mut book = book();
    let memo = Column::Index(1);
    let format = Format {
        name: book.names.intern("bank"),
        shape: Shape::Rows,
        specs: vec![spec(Field::Memo, [memo])].into_boxed_slice(),
        categories: Box::default(),
        loc: Loc::default(),
    };
    let format_id = book.formats.push(format);
    let source = Source {
        name: book.names.intern("online"),
        fetch: Fetch::Run(book.intern_text("fetch statement")),
        format: Some(format_id),
        sink: Sink::Journal,
        system: None,
        doc: None,
        loc: Loc::default(),
    };
    let problems = read_memos(&book, &source, "TRADER JOE'S #10\n", FileId(0)).unwrap_err();
    assert_eq!(problems[0].code, "sync-run-memos");
}

#[test]
fn malformed_amounts_are_diagnosed_at_the_source_cell() {
    let mut book = book();
    let date = Column::Header(book.intern_text("Date"));
    let amount = Column::Header(book.intern_text("Amount"));
    let memo = Column::Header(book.intern_text("Memo"));
    let format = Format {
        name: book.names.intern("bank"),
        shape: Shape::Rows,
        specs: vec![
            spec(Field::Date, [date]),
            spec(Field::Amount, [amount]),
            spec(Field::Memo, [memo]),
        ]
        .into_boxed_slice(),
        categories: Box::default(),
        loc: Loc::default(),
    };
    let source = "Date,Amount,Memo\n2026-03-04,12,50,Shop\n";
    let (_, problems) = read(&book, &format, source, FileId(3), USD, &[USD]);
    assert!(!problems.is_empty());
    assert_eq!(problems[0].anchor().unwrap().file, FileId(3));
}
