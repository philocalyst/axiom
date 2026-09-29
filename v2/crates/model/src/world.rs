//! The book under construction, with what only the build needs.
//!
//! Declaration creates every thing that has a name: kinds, commodities,
//! entities, places. Later stages fill in what those things hold and resolve
//! the names written in laws and in the journal against them.

use axiom_core::{Arena, Diagnostic, Groups, Id, Interner, Set, Sym, Tree};

use crate::book::{Book, Commodity, Entity, Kind, Lookup, Place, Roots, System};
use crate::catalog::Catalog;
use crate::cx::Cx;
use crate::kinds::PropTable;
use crate::law::Rules;
use crate::names::{Names, Scoped};
use crate::props::Budget;
use crate::scope::Scopes;
use crate::survey::Facts;
use crate::systems::Systems;
use crate::{commodities, entities, kinds, places};

pub(crate) struct World<'s> {
    pub book: Book<'s>,
    pub scopes: Scopes,
    pub systems: Systems<'s>,
    pub properties: PropTable,
    pub declared: Declared,
    pub budgets: Vec<Budget>,
    /// Names some law counts.
    pub tallies: Set<&'s str>,
    /// Codes that mark a transaction or a leg.
    pub codes: Set<Sym>,
}

/// The id of everything the catalog declares, in catalog order.
pub(crate) struct Declared {
    pub kinds: Vec<Id<Kind>>,
    pub commodities: Vec<Id<Commodity>>,
    pub entities: Vec<Id<Entity>>,
    pub places: Vec<Option<Id<Place>>>,
}

impl<'s> World<'s> {
    pub fn declare(
        mut names: Interner<'s>,
        facts: Facts<'s>,
        catalog: &Catalog<'_, 's>,
        tree: Tree<System>,
        systems: Systems<'s>,
        scopes: Scopes,
        diags: &mut Vec<Diagnostic>,
    ) -> World<'s> {
        let Facts { units, place_paths, tallies, codes } = facts;
        let mut cx = Cx { names: &mut names, systems: &tree, scopes: &scopes, diags };
        let kinds = kinds::declare(catalog, &mut cx);
        let commodities = commodities::declare(catalog, &units, &kinds, &mut cx);
        let entities = entities::declare(catalog, &kinds, &mut cx);
        let places = places::declare(catalog, &place_paths, &kinds, &entities, &mut cx);

        let roots = Roots {
            me: entities.me,
            unknown: places.unknown,
            asset: kinds.roots.asset,
            liability: kinds.roots.liability,
            income: kinds.roots.income,
            expense: kinds.roots.expense,
            equity: kinds.roots.equity,
            commodity: kinds.roots.commodity,
            entity: kinds.roots.entity,
        };
        let declared = Declared {
            kinds: kinds.declared,
            commodities: commodities.declared,
            entities: entities.declared,
            places: places.declared,
        };
        let book = Book {
            names,
            base: commodities.base,
            relaxed: catalog.relaxed,
            roots,
            places: places.tree,
            entities: entities.tree,
            kinds: kinds.tree,
            systems: tree,
            commodities: commodities.arena,
            laws: Arena::new(),
            rules: Rules::default(),
            params: Arena::new(),
            schedules: Arena::new(),
            codes: Vec::new(),
            txns: Arena::new(),
            flows: Arena::new(),
            touching: Groups::default(),
            asserts: Vec::new(),
            events: Vec::new(),
            prices: Default::default(),
            plans: Vec::new(),
            syncs: Vec::new(),
            lookup: Lookup {
                places: places.names,
                entities: entities.index,
                kinds: kinds.index,
                params: Scoped::default(),
                laws: Names::default(),
                commodities: commodities.by_symbol,
            },
        };
        World { book, scopes, systems, properties: kinds.properties, declared, budgets: Vec::new(), tallies, codes }
    }
}
