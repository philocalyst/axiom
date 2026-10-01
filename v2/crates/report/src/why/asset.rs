//! `why ASSET`: its parts, basis changes and flows that concern it.

use axiom_core::{Id, Qty};
use axiom_engine::{AdjustmentKind, Run};
use axiom_model::{Amount, Asset, Book, Object};

use crate::lens::{Lens, Whose};
use crate::places::route;
use crate::{Cell, Column, Report, Row, Section, Style};

pub fn report<'s>(book: &Book<'s>, run: &Run, whose: &Whose, asset_id: Id<Asset>) -> Report<'s> {
    let asset = &book.assets[asset_id];
    let name = book.name(asset.name);
    if !whose.includes(asset.owner) {
        return Report::new(format!("Why {name}")).with(Section::note_only(format!(
            "{name} belongs to {}, whose money this is not.",
            book.name(book.entities[asset.owner].path)
        )));
    }
    let state = run.assets.iter().find(|state| state.asset == asset_id);
    let mut overview = Section::new([
        Column::left("Owner"),
        Column::left("Part of"),
        Column::right("Value"),
        Column::right("Basis"),
    ])
    .headed("Asset");
    let lens = Lens::new(book, whose, run.today);
    let value = lens.value(Amount::new(Qty(1), asset.unit));
    let basis = state.map_or(Qty::ZERO, |state| {
        state.parts.iter().map(|part| part.basis).sum()
    });
    overview.push(Row::new([
        Cell::Name(book.name(book.entities[asset.owner].path)),
        asset.part_of.map_or(Cell::Blank, |parent| {
            Cell::Name(book.name(book.assets[parent.value].name))
        }),
        value.map_or(Cell::Blank, |value| Cell::base(book, value)),
        Cell::base(book, basis),
    ]));

    let mut parts = Section::new([
        Column::left("Part"),
        Column::left("Acquired"),
        Column::right("Cost"),
        Column::right("Basis"),
        Column::right("Consumed"),
        Column::left("From"),
    ])
    .headed("Parts");
    if let Some(state) = state {
        for (index, part) in state.parts.iter().enumerate() {
            let consumed = run
                .adjustments
                .iter()
                .filter(|adjustment| matches!(adjustment.kind, AdjustmentKind::Consumed { asset, part: found } if asset == asset_id && found as usize == index))
                .map(|adjustment| adjustment.amount)
                .sum();
            let flow = &book.flows[part.flow];
            let purpose = flow.purpose.map_or(Cell::Blank, |purpose| {
                Cell::Purpose(book.name(book.purposes[purpose.purpose].name))
            });
            parts.push(Row::new([
                purpose,
                Cell::Day(part.day),
                Cell::base(book, part.cost),
                Cell::base(book, part.basis),
                Cell::base_or_blank(book, consumed),
                Cell::Source(flow.loc),
            ]));
        }
    }
    if parts.rows.is_empty() {
        parts.note("No asset parts are present in this run.");
    }

    let mut about = Section::new([
        Column::left("Date"),
        Column::left("Purpose"),
        Column::left("Flow"),
        Column::right("Amount"),
        Column::left("From"),
    ])
    .headed("Flows about it");
    for (id, flow) in book.flows.iter().filter(|(_, flow)| {
        whose.includes(flow.owner)
            && flow
                .purpose
                .is_some_and(|purpose| purpose.of == Some(Object::Asset(asset_id)))
    }) {
        let posting = crate::history::Posting::at(book, run, id);
        let purpose = flow
            .purpose
            .map(|purpose| book.name(book.purposes[purpose.purpose].name))
            .unwrap_or("");
        about.push(Row::new([
            Cell::Day(flow.day),
            Cell::Purpose(purpose),
            Cell::text(route(book, flow)),
            Cell::amount(book, posting.out()),
            Cell::Source(flow.loc),
        ]));
    }
    if about.rows.is_empty() {
        about.note("No flows name this asset as their purpose's object.");
    }

    if state.is_some_and(|state| state.disposed.is_some()) {
        overview.note("This asset was disposed of.");
    }
    Report::new(format!("Why {name}"))
        .with(overview)
        .with(parts)
        .with(about)
}
