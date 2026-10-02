//! Commodities, and the base currency every amount is counted against.

use axiom_core::{Arena, Diagnostic, Id, Interner, Map};
use axiom_syntax::DeclKind;

use super::{Resolving, Settings};
use crate::book::{Commodity, Sort};
use crate::collect::Collected;
use crate::errors::Word;
use crate::problem::{self, Noun};

/// Every commodity of the book, by name, and the one the books are kept in.
pub(super) struct Commodities<'s> {
    pub arena: Arena<Commodity>,
    pub by_name: Map<&'s str, Id<Commodity>>,
    pub base: Id<Commodity>,
}

/// A commodity of no kind of its own: scale, title and the rest are for the property pass to fill.
pub(super) fn commodity(
    symbol: axiom_core::Sym,
    kind: Id<crate::book::Kind>,
    scale: u8,
    doc: Option<axiom_core::Sym>,
    loc: Option<axiom_core::Loc>,
) -> Commodity {
    Commodity { symbol, kind, scale, title: None, growth: None, doc, loc }
}

/// The commodities written, a USD of two decimals when none is, and the base currency chosen among them.
pub(super) fn declare<'a, 's>(
    collected: &Collected<'a, 's>,
    settings: &Settings<'s>,
    resolving: &Resolving<'_>,
    names: &mut Interner<'s>,
    diags: &mut Vec<Diagnostic>,
) -> Commodities<'s> {
    let root = resolving.kind_roots.commodity;
    let mut arena = Arena::new();
    let mut by_name: Map<&'s str, Id<Commodity>> = Map::default();
    for written in collected.decls_of(DeclKind::Commodity) {
        let (file, decl) = (written.file(), written.node);
        let symbol = decl.name.0;
        if let Some(&first) = by_name.get(symbol) {
            diags.push(problem::duplicate(Noun::Commodity, Word::of(file, symbol), arena[first].loc));
            continue;
        }
        let kind = resolving.kind(names, written, Sort::Commodity, root, diags);
        let doc = written.item.doc.map(|doc| names.intern(doc.0));
        let id = arena.push(commodity(names.intern(symbol), kind, 0, doc, Some(file.loc(symbol))));
        by_name.insert(symbol, id);
    }
    let mut synthetic = None;
    if arena.is_empty() {
        let id = arena.push(commodity(names.intern("USD"), root, 2, None, None));
        by_name.insert("USD", id);
        synthetic = Some(id);
    }
    if settings.base.is_none() && synthetic.is_none() && !by_name.contains_key("USD") && by_name.len() > 1 {
        let first = by_name.values().next().and_then(|&id| arena[id].loc).unwrap_or_default();
        diags.push(
            Diagnostic::error("base-currency-required", "a book with several currencies needs a base currency")
                .label(first, "choose the currency amounts are converted into")
                .help("write `base UNIT` once, such as `base USD`"),
        );
    }
    let base = settings
        .base
        .and_then(|word| {
            by_name.get(word.text).copied().or_else(|| {
                let suggestion = axiom_core::diag::closest(word.text, by_name.keys().copied()).map(|near| near as &str);
                diags.push(problem::unknown(Noun::Commodity, word, suggestion));
                None
            })
        })
        .or_else(|| by_name.get("USD").copied())
        .or(synthetic)
        .or_else(|| arena.ids().next())
        .expect("a book always has a base commodity");
    Commodities { arena, by_name, base }
}
