use super::*;
use axiom_core::{Interner, Loc, Qty, calendar::DateLayout};
use axiom_model::sync::{Column, Field, Format, Rule, Shape, Spec};

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

fn named<'s>(names: &mut Interner<'s>, text: &'s str) -> Column {
    Column::Header(names.intern(text))
}

#[test]
fn typed_rows_read_amount_sign_memo_and_balance() {
    let mut names = Interner::default();
    let date = named(&mut names, "Posting Date");
    let amount = named(&mut names, "Amount");
    let sign_column = named(&mut names, "Direction");
    let memo = named(&mut names, "Description");
    let balance = named(&mut names, "Balance");
    let into = names.intern("CRDT");
    let mut date_spec = spec(Field::Date, [date]);
    date_spec.layout = Some(DateLayout::parse("MM/DD/YYYY").unwrap());
    let mut amount_spec = spec(Field::Amount, [amount]);
    amount_spec.rule = Rule::Sign {
        place: sign_column,
        into,
    };
    let format = Format {
        name: names.intern("bank"),
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
    let (records, problems) = read(&format, &names, source, FileId(0), USD, &[USD]);
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
    let mut names = Interner::default();
    let records = names.intern("Ntry");
    let date = Column::Path(names.intern("BookgDt/Dt"));
    let amount = Column::Path(names.intern("Amt"));
    let sign = Column::Path(names.intern("CdtDbtInd"));
    let memo = Column::Path(names.intern("AddtlNtryInf"));
    let crdt = names.intern("CRDT");
    let mut amount_spec = spec(Field::Amount, [amount]);
    amount_spec.rule = Rule::Sign {
        place: sign,
        into: crdt,
    };
    let format = Format {
        name: names.intern("camt053"),
        shape: Shape::Tagged { records },
        specs: vec![
            spec(Field::Date, [date]),
            amount_spec,
            spec(Field::Memo, [memo]),
        ]
        .into_boxed_slice(),
        categories: Box::default(),
        loc: Loc::default(),
    };
    let source = "<Document><Ntry><BookgDt><Dt>2026-03-04</Dt></BookgDt><Amt>12.50</Amt><CdtDbtInd>CRDT</CdtDbtInd><AddtlNtryInf>Refund</AddtlNtryInf></Ntry></Document>";
    let (records, problems) = read(&format, &names, source, FileId(0), USD, &[USD]);
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].qty, Qty(1250));
    assert_eq!(records[0].memo, "Refund");
}

#[test]
fn malformed_amounts_are_diagnosed_at_the_source_cell() {
    let mut names = Interner::default();
    let date = named(&mut names, "Date");
    let amount = named(&mut names, "Amount");
    let memo = named(&mut names, "Memo");
    let format = Format {
        name: names.intern("bank"),
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
    let (_, problems) = read(&format, &names, source, FileId(3), USD, &[USD]);
    assert!(!problems.is_empty());
    assert_eq!(problems[0].anchor().unwrap().file, FileId(3));
}
