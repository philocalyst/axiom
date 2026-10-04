//! `why ASSET`: its parts, basis changes and flows that concern it.

use axiom_core::{Id, Qty};
use axiom_engine::{AdjustmentKind, AssetState, Part, PartKind};
use axiom_model::{Amount, Asset, Object};

use super::flows_table;
use crate::history::all_postings;
use crate::view::View;
use crate::{Cell, Column, Report, Row, Section};

pub fn report<'s>(view: View<'s, '_, '_>, asset_id: Id<Asset>) -> Report<'s> {
    let book = view.book();
    let asset = &book.assets[asset_id];
    let name = book.name(asset.name);
    if !view.owns_entity(asset.owner) {
        return view.refuse(format!("Why {name}"), name, Some(asset.owner));
    }
    let state = view.run.assets.iter().find(|state| state.asset == asset_id);
    // The flows whose purpose is about the asset.
    let about = all_postings(book, view.run).filter(|posting| {
        view.owns_flow(posting.flow)
            && posting.flow.purpose.is_some_and(|purpose| purpose.of == Some(Object::Asset(asset_id)))
    });
    Report::new(format!("Why {name}"))
        .with(overview(view, asset, state))
        .with(parts(view, asset_id, state))
        .with(flows_table(view, about, "Flows"))
}

/// Who owns it, what it is part of, and what it is worth and cost.
fn overview<'s>(view: View<'s, '_, '_>, asset: &Asset, state: Option<&AssetState>) -> Section<'s> {
    let book = view.book();
    let mut overview =
        Section::new([Column::left("Owner"), Column::left("Part of"), Column::right("Value"), Column::right("Basis")])
            .headed("Asset");
    let value = view.value(Amount::new(Qty(1), asset.unit)).map(|value| view.entity_qty(asset.owner, value));
    let basis = state.and_then(|state| state.total_basis().ok()).map(|basis| view.entity_qty(asset.owner, basis));
    overview.push(Row::new([
        Cell::Name(book.name(book.entities[asset.owner].path)),
        asset.part_of.map_or(Cell::Blank, |parent| Cell::Name(book.name(book.assets[parent.value].name))),
        value.map_or(Cell::Blank, |value| Cell::base(book, value)),
        basis.map_or(Cell::Blank, |basis| Cell::base(book, basis)),
    ]));
    if state.is_some_and(|state| state.disposed.is_some()) {
        overview.note("This asset was disposed of.");
    }
    overview
}

/// Each acquisition and improvement: when, at what cost and basis, how much of the basis was consumed, and where it was written.
fn parts<'s>(view: View<'s, '_, '_>, asset_id: Id<Asset>, state: Option<&AssetState>) -> Section<'s> {
    let mut parts = Section::new([
        Column::left("Part"),
        Column::left("Acquired"),
        Column::right("Cost"),
        Column::right("Basis"),
        Column::right("Consumed"),
        Column::left("From"),
    ])
    .headed("Parts");
    for part in state.iter().flat_map(|state| &state.parts) {
        parts.push(part_row(view, asset_id, part));
    }
    if parts.rows.is_empty() {
        parts.note("No asset parts are present in this run.");
    }
    parts
}

fn part_row<'s>(view: View<'s, '_, '_>, asset_id: Id<Asset>, part: &Part) -> Row<'s> {
    let (book, owner) = (view.book(), view.book().assets[asset_id].owner);
    let consumed = view.run
        .adjustments
        .iter()
        .filter(|adjustment| {
            matches!(adjustment.kind, AdjustmentKind::Consumed { asset, part: found } if asset == asset_id && found == part.id)
        })
        .map(|adjustment| adjustment.amount)
        .sum::<Qty>();
    let purpose = part.flow.and_then(|id| book.flows.get(id)).and_then(|flow| flow.purpose).map_or_else(
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
        .or_else(|| part.id.origin.source_txn().and_then(|txn| book.txns.get(txn).map(|txn| txn.loc)))
        .map_or(Cell::Blank, Cell::Source);
    Row::new([
        purpose,
        Cell::Day(part.day),
        Cell::base(book, view.entity_qty(owner, part.cost)),
        Cell::base(book, view.entity_qty(owner, part.basis)),
        Cell::base_or_blank(book, view.entity_qty(owner, consumed)),
        source,
    ])
}
