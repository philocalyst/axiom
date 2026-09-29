//! Commodities: declared, or opened on first use.
//!
//! A commodity's precision is what its declaration says, or else the most
//! decimals any amount written in it uses anywhere in the sources. That makes
//! `84.20 USD` exact without anyone declaring USD.

use axiom_core::{Arena, Diagnostic, Id, Map, Sym};
use axiom_syntax::Decl;

use crate::args::{Args, Builtin};
use crate::book::{Commodity, Sort};
use crate::catalog::Catalog;
use crate::cx::Cx;
use crate::errors::duplicate;
use crate::kinds::Kinds;
use crate::survey::Seen;

/// Quanta are `i64`; eighteen decimals is as fine as one can count.
pub(crate) const MAX_SCALE: u8 = 18;

pub(crate) struct Commodities {
    pub arena: Arena<Commodity>,
    pub by_symbol: Map<Sym, Id<Commodity>>,
    /// The id of each declaration in the catalog, in catalog order.
    pub declared: Vec<Id<Commodity>>,
    pub base: Id<Commodity>,
}

pub(crate) fn declare<'s>(
    catalog: &Catalog<'_, 's>,
    seen: &[Seen<'s>],
    kinds: &Kinds,
    cx: &mut Cx<'_, 's>,
) -> Commodities {
    let precision_seen: Map<&str, u8> = seen.iter().map(|unit| (unit.symbol, unit.places)).collect();
    let mut arena = Arena::new();
    let mut by_symbol: Map<Sym, Id<Commodity>> = Map::default();
    let mut declared = Vec::with_capacity(catalog.commodities.len());

    for written in &catalog.commodities {
        let decl = written.what;
        let symbol = cx.names.intern(decl.name.text);
        if let Some(&first) = by_symbol.get(&symbol) {
            cx.diags.push(duplicate("commodity", decl.name, arena[first].loc));
            declared.push(first);
            continue;
        }
        let thing = format!("the commodity `{}`", decl.name.text);
        let kind = kinds.declared(decl, written.home(), Sort::Commodity, &thing, cx);
        let places = precision_seen.get(decl.name.text).copied().unwrap_or(0);
        let scale = declared_precision(decl, written.exprs(), written.home(), cx).unwrap_or(places.min(MAX_SCALE));
        let id = arena.push(Commodity {
            symbol,
            kind,
            scale,
            title: None,
            liquidity: kinds.tree[kind].liquidity,
            growth: None,
            props: Box::default(),
            doc: written.item.doc.map(|doc| cx.names.intern(doc.0)),
            loc: Some(decl.name.loc),
        });
        by_symbol.insert(symbol, id);
        declared.push(id);
    }

    // Everything else that was written in some amount opens on first use.
    let base_symbol = catalog.base.map_or("USD", |base| base.text);
    let base_seen = Seen { symbol: base_symbol, places: 0 };
    for unit in seen.iter().chain(std::iter::once(&base_seen)) {
        let symbol = cx.names.intern(unit.symbol);
        by_symbol.entry(symbol).or_insert_with(|| {
            arena.push(Commodity {
                symbol,
                kind: kinds.roots.commodity,
                scale: unit.places.min(MAX_SCALE),
                title: None,
                liquidity: None,
                growth: None,
                props: Box::default(),
                doc: None,
                loc: None,
            })
        });
    }
    let base = by_symbol[&cx.names.intern(base_symbol)];
    Commodities { arena, by_symbol, declared, base }
}

/// `precision 3`, if the declaration has it and it is valid.
fn declared_precision(decl: &Decl, exprs: &axiom_syntax::Exprs, home: crate::scope::Home, cx: &mut Cx) -> Option<u8> {
    let prop = decl.props.iter().find(|prop| Builtin::parse(prop.name.text) == Some(Builtin::Precision))?;
    let mut args = Args::new(exprs, home, prop);
    let read = args.count(MAX_SCALE).and_then(|scale| args.done().map(|()| scale));
    read.map_err(|error: Diagnostic| cx.diags.push(error)).ok()
}
