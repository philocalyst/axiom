use axiom_core::{Day, FileId, Loc};
use axiom_engine::{Options, Plan};
use axiom_model::sync::{Column, Field, Format, Rule, Shape, Spec};
use axiom_model::{Book, Source};
use axiom_syntax::Folder;

use crate::binding;
use super::{Feed, Form};

const PROJECT: &str = "base USD\n";

fn book() -> Book<'static> {
    let (file, problems) = axiom_syntax::parse(FileId(0), PROJECT, Folder::default());
    assert!(problems.is_empty(), "book.ax: {problems:?}");
    let sources = [Source {
        path: "book.ax",
        file,
        embedded: false,
    }];
    let (book, problems) = axiom_model::build(&sources);
    assert!(problems.is_empty(), "{problems:?}");
    book
}

fn feed_format(book: &mut Book<'static>) -> axiom_core::Id<Format> {
    let spec = |field, index| Spec {
        field,
        places: Box::new([Column::Index(index)]),
        layout: None,
        rule: Rule::None,
        loc: Loc::default(),
    };
    book.formats.push(Format {
        name: book.names.intern("test-feed"),
        shape: Shape::Rows,
        specs: vec![
            spec(Field::Date, 1),
            spec(Field::Amount, 2),
            spec(Field::Memo, 3),
        ]
        .into_boxed_slice(),
        categories: Box::default(),
        loc: Loc::default(),
    })
}

fn run(book: &Book<'_>) -> axiom_engine::Run {
    Plan::new(book).run(Options {
        today: Day::parse(b"2026-01-10").unwrap(),
        relaxed: false,
    })
}

#[test]
fn a_new_statement_row_is_committed_once_and_then_reconciles() {
    let mut book = book();
    let format = feed_format(&mut book);
    let run = run(&book);
    let mut world = binding::world(&book, &run, &["journal/2026/01.ax"]).unwrap();
    let feed = Feed {
        account: "checking",
        unit: crate::Unit {
            name: "USD",
            scale: 2,
        },
        format: &book.formats[format],
    };
    let statement = "2026-01-06,-3.25,UNKNOWN MARKET\n";

    let first = world.feed(&feed, statement).unwrap();
    assert_eq!(first.len(), 1);
    assert!(matches!(&first[0].form, Form::Item(line) if line.contains("3.25 USD")));
    let second = world.feed(&feed, statement).unwrap();
    assert!(second.is_empty(), "the committed delta makes the feed idempotent");
}
