//! `why ASSET`: its parts, basis changes and flows that concern it.

use axiom_core::{Id, Qty};
use axiom_engine::{AdjustmentKind, PartKind, Run};
use axiom_model::{Amount, Asset, Object};

use crate::lens::Lens;
use crate::places::route;
use crate::{Cell, Column, Report, Row, Section};

pub fn report<'s>(lens: Lens<'s, '_, '_, '_>, run: &Run, asset_id: Id<Asset>) -> Report<'s> {
    let book = lens.book();
    let asset = &book.assets[asset_id];
    let name = book.name(asset.name);
    if !lens.owns_entity(asset.owner) {
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
    let value = lens
        .value(Amount::new(Qty(1), asset.unit))
        .map(|value| lens.entity_qty(asset.owner, value));
    let basis = state
        .and_then(|state| state.total_basis().ok())
        .map(|basis| lens.entity_qty(asset.owner, basis));
    overview.push(Row::new([
        Cell::Name(book.name(book.entities[asset.owner].path)),
        asset.part_of.map_or(Cell::Blank, |parent| {
            Cell::Name(book.name(book.assets[parent.value].name))
        }),
        value.map_or(Cell::Blank, |value| Cell::base(book, value)),
        basis.map_or(Cell::Blank, |basis| Cell::base(book, basis)),
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
        for part in &state.parts {
            let consumed = run
                .adjustments
                .iter()
                .filter(|adjustment| {
                    matches!(adjustment.kind, AdjustmentKind::Consumed { asset, part: found } if asset == asset_id && found == part.id)
                })
                .map(|adjustment| adjustment.amount)
                .sum::<Qty>();
            let purpose = part
                .flow
                .and_then(|id| book.flows.get(id))
                .and_then(|flow| flow.purpose)
                .map_or_else(
                    || {
                        Cell::Word(match part.kind {
                            PartKind::Acquisition => "acquisition",
                            PartKind::Improvement => "improvement",
                        })
                    },
                    |purpose| Cell::Purpose(book.name(book.purposes[purpose.purpose].name)),
                );
            let source = part
                .flow
                .and_then(|id| book.flows.get(id).map(|flow| flow.loc))
                .or_else(|| {
                    part.id
                        .origin
                        .source_txn()
                        .and_then(|txn| book.txns.get(txn).map(|txn| txn.loc))
                })
                .map_or(Cell::Blank, Cell::Source);
            parts.push(Row::new([
                purpose,
                Cell::Day(part.day),
                Cell::base(book, lens.entity_qty(asset.owner, part.cost)),
                Cell::base(book, lens.entity_qty(asset.owner, part.basis)),
                Cell::base_or_blank(book, lens.entity_qty(asset.owner, consumed)),
                source,
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
        lens.owns(crate::flow::movement_place(lens, flow))
            && flow
                .purpose
                .is_some_and(|purpose| purpose.of == Some(Object::Asset(asset_id)))
    }) {
        let posting = crate::history::Posting::at(book, run, id);
        let out = posting.out();
        let amount = crate::flow::scoped_movement_qty(lens, flow, out.qty);
        if amount.is_zero() {
            continue;
        }
        let purpose = flow
            .purpose
            .map(|purpose| book.name(book.purposes[purpose.purpose].name))
            .unwrap_or("");
        about.push(Row::new([
            Cell::Day(flow.day),
            Cell::Purpose(purpose),
            Cell::text(route(book, flow)),
            Cell::amount(book, Amount::new(amount, out.unit)),
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
