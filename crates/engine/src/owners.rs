//! Effective ownership: who owns each entity and place in the end, and in what shares.
//!
//! Ownership is declared one step at a time and nests: a household owns half of a business that owns a
//! rental. The fold wants the end of the chain, so the declared shares are multiplied along every path and
//! summed by owner, once, before the first flow.
//!
//! An entity's declarations become edges to its owners, counting-sorted into one flat table. A depth-first
//! walk closes an owner before the entities it owns, so each is composed from rows that are already final,
//! and rows are runs of one arena, never a vector apiece. The answer is two `Groups`: the fold reads a slice.
//!
//! What cannot be composed has no owners. A cycle (reported at each edge that closes one) and a share that
//! does not fit (reported at the edge where it overflowed) leave their entity with an empty row, and an
//! entity with an ownerless owner is ownerless too.

use axiom_core::{Arena, Diagnostic, Groups, Id, Loc, Ratio, Run};
use axiom_model::{Book, Entity, Place, Share};

use crate::OwnerShare;

/// The effective owners of every entity and place, in owner order.
pub(crate) struct Owners {
    entities: Groups<Entity, OwnerShare>,
    places: Groups<Place, OwnerShare>,
}

impl Owners {
    /// Composes the book's declared ownership, reporting what cannot be composed into `problems`.
    pub(crate) fn of(book: &Book, problems: &mut Vec<Diagnostic>) -> Owners {
        let entities = entity_owners(book, problems);
        let places = place_owners(book, &entities, problems);
        Owners { entities, places }
    }

    pub(crate) fn entity(&self, entity: Id<Entity>) -> &[OwnerShare] {
        &self.entities[entity]
    }

    pub(crate) fn place(&self, place: Id<Place>) -> &[OwnerShare] {
        &self.places[place]
    }
}

/// One declared step of ownership: `weight` of what is owned belongs to `parent`, as written at `loc`.
#[derive(Clone, Copy)]
struct Edge {
    parent: Id<Entity>,
    weight: Ratio,
    loc: Loc,
}

impl Edge {
    /// A sole owner owns everything, so nothing overflows and `loc` is never what a problem points at.
    fn sole(owner: Id<Entity>, loc: Option<Loc>) -> Edge {
        Edge { parent: owner, weight: Ratio::ONE, loc: loc.unwrap_or_default() }
    }
}

impl From<&Share> for Edge {
    fn from(share: &Share) -> Edge {
        Edge { parent: share.entity, weight: share.rate, loc: share.loc }
    }
}

/// What `entity` declares about its owners: its shares, else its sole owner (never itself).
fn entity_edges(entity: &Entity, id: Id<Entity>) -> impl Iterator<Item = Edge> + Clone + '_ {
    let sole = entity.owner.filter(|&owner| owner != id && entity.owned_by.is_empty());
    entity.owned_by.iter().map(Edge::from).chain(sole.map(|owner| Edge::sole(owner, entity.loc)))
}

/// What `place` declares about its owners: its shares, else the entity that holds it.
fn place_edges(place: &Place) -> impl Iterator<Item = Edge> + '_ {
    let holder = place.shares.is_empty().then(|| Edge::sole(place.owner, place.loc));
    place.shares.iter().map(Edge::from).chain(holder)
}

/// The owners reached through `edges`: each edge's weight times its parent's owners, summed by owner. Fails
/// with the edge at which a share no longer fits.
fn compose<'o>(
    edges: impl IntoIterator<Item = Edge>,
    owners_of: impl Fn(Id<Entity>) -> &'o [OwnerShare],
) -> Result<Vec<OwnerShare>, Loc> {
    let mut owned = Vec::<OwnerShare>::new();
    for edge in edges {
        for owner in owners_of(edge.parent) {
            let rate = edge.weight.checked_mul(owner.share).ok_or(edge.loc)?;
            match owned.binary_search_by_key(&owner.owner, |held| held.owner) {
                Ok(at) => owned[at].share = owned[at].share.checked_add(rate).ok_or(edge.loc)?,
                Err(at) => owned.insert(at, OwnerShare { owner: owner.owner, share: rate }),
            }
        }
    }
    Ok(owned)
}

fn overflow(at: Loc, label: &str) -> Diagnostic {
    Diagnostic::error("ownership-overflow", "effective ownership share is too large to represent").label(at, label)
}

fn entity_owners(book: &Book, problems: &mut Vec<Diagnostic>) -> Groups<Entity, OwnerShare> {
    let declared = book.entities.ids().flat_map(|id| entity_edges(&book.entities[id], id).map(move |edge| (id, edge)));
    let edges = Groups::build(book.entities.len(), declared);
    let mut walk = Walk::new(&edges, problems);
    book.entities.ids().for_each(|root| walk.visit(root));
    walk.into_groups(book.entities.ids())
}

fn place_owners(
    book: &Book,
    entities: &Groups<Entity, OwnerShare>,
    problems: &mut Vec<Diagnostic>,
) -> Groups<Place, OwnerShare> {
    let mut rows = Vec::new();
    for (id, place) in book.places.iter() {
        let owned = compose(place_edges(place), |parent| &entities[parent]).unwrap_or_else(|at| {
            problems.push(overflow(at, "this place share overflows while ownership is composed"));
            Vec::new()
        });
        rows.extend(owned.into_iter().map(|share| (id, share)));
    }
    Groups::build(book.places.len(), rows.iter().copied())
}

/// How far the walk has got with an entity.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Visit {
    Fresh,
    /// On the path from the root: reaching it again is a cycle.
    Open,
    Closed,
}

/// The depth-first walk over entities' owners, closing each after everything it is owned through.
struct Walk<'a> {
    edges: &'a Groups<Entity, Edge>,
    visits: Vec<Visit>,
    /// Each entity's owners, a run of `shares`. Until it closes, an entity has none.
    owners: Vec<Run<OwnerShare>>,
    shares: Arena<OwnerShare>,
    /// The entities from the root to the one being walked, each with the edges it has yet to follow.
    path: Vec<(Id<Entity>, std::slice::Iter<'a, Edge>)>,
    problems: &'a mut Vec<Diagnostic>,
}

const NOBODY: Run<OwnerShare> = Run::new(Id::new(0), 0);

impl<'a> Walk<'a> {
    fn new(edges: &'a Groups<Entity, Edge>, problems: &'a mut Vec<Diagnostic>) -> Walk<'a> {
        let entities = edges.keys();
        let (visits, owners) = (vec![Visit::Fresh; entities], vec![NOBODY; entities]);
        Walk { edges, visits, owners, shares: Arena::new(), path: Vec::new(), problems }
    }

    /// Closes `root` and every entity not yet closed that it is owned through.
    fn visit(&mut self, root: Id<Entity>) {
        if self.visits[root.index()] != Visit::Fresh {
            return;
        }
        self.open(root);
        while let Some((entity, edges)) = self.path.last_mut() {
            let entity = *entity;
            match edges.next().copied() {
                Some(edge) => self.follow(edge),
                None => {
                    self.path.pop();
                    self.close(entity);
                }
            }
        }
    }

    fn open(&mut self, entity: Id<Entity>) {
        self.visits[entity.index()] = Visit::Open;
        self.path.push((entity, self.edges[entity].iter()));
    }

    fn follow(&mut self, edge: Edge) {
        match self.visits[edge.parent.index()] {
            Visit::Fresh => self.open(edge.parent),
            Visit::Open => self.problems.push(
                Diagnostic::error("ownership-cycle", "entity ownership contains a cycle")
                    .label(edge.loc, "this ownership edge closes the cycle"),
            ),
            Visit::Closed => {}
        }
    }

    fn close(&mut self, entity: Id<Entity>) {
        self.visits[entity.index()] = Visit::Closed;
        let owned = self.owners_of(entity);
        self.owners[entity.index()] = self.shares.extend(owned);
    }

    /// `entity`'s owners, composed from the owners of its owners, which are closed.
    fn owners_of(&mut self, entity: Id<Entity>) -> Vec<OwnerShare> {
        let edges = &self.edges[entity];
        if edges.is_empty() {
            return vec![OwnerShare { owner: entity, share: Ratio::ONE }];
        }
        if edges.iter().any(|edge| self.closed(edge.parent).is_empty()) {
            return Vec::new();
        }
        compose(edges.iter().copied(), |parent| self.closed(parent)).unwrap_or_else(|at| {
            self.problems.push(overflow(at, "this share overflows while ownership is composed"));
            Vec::new()
        })
    }

    /// The owners of an entity that has closed; of one that has not, none.
    fn closed(&self, entity: Id<Entity>) -> &[OwnerShare] {
        &self.shares[self.owners[entity.index()]]
    }

    fn into_groups(self, entities: impl Iterator<Item = Id<Entity>> + Clone) -> Groups<Entity, OwnerShare> {
        let rows = entities
            .zip(&self.owners)
            .flat_map(|(entity, &run)| self.shares[run].iter().map(move |&share| (entity, share)));
        Groups::build(self.owners.len(), rows)
    }
}

#[cfg(test)]
mod tests {
    use axiom_core::FileId;

    use super::*;
    use crate::fixture::Fixture;

    fn share(entity: Id<Entity>, num: i64, den: i64, at: u32) -> Share {
        let rate = Ratio::new(num.into(), den.into()).expect("a ratio");
        Share { entity, rate, measure: None, loc: Loc::new(FileId(0), at, at + 1) }
    }

    fn of(fixture: Fixture) -> (Owners, Vec<Diagnostic>, Book<'static>) {
        let book = fixture.book();
        let mut problems = Vec::new();
        (Owners::of(&book, &mut problems), problems, book)
    }

    #[test]
    fn an_entity_without_an_owner_owns_itself_and_a_sole_owner_owns_everything() {
        let mut f = Fixture::new();
        let (me, grant, savings) = (f.me, f.grant, f.savings);
        f.entities[grant].owner = Some(me);
        let (owners, problems, _) = of(f);
        assert!(problems.is_empty());
        assert_eq!(owners.entity(me), [OwnerShare { owner: me, share: Ratio::ONE }]);
        assert_eq!(owners.entity(grant), [OwnerShare { owner: me, share: Ratio::ONE }]);
        assert_eq!(owners.place(savings), [OwnerShare { owner: me, share: Ratio::ONE }]);
    }

    #[test]
    fn shares_through_two_paths_to_one_owner_are_summed() {
        let mut f = Fixture::new();
        let (me, grant, household, savings) = (f.me, f.grant, f.household, f.savings);
        f.entities[grant].owned_by = [share(me, 1, 2, 0), share(household, 1, 2, 1)].into();
        f.places[savings].shares = [share(me, 1, 4, 2), share(grant, 3, 4, 3)].into();
        let (owners, problems, _) = of(f);
        assert!(problems.is_empty());
        let sum = |owner, num, den| OwnerShare { owner, share: Ratio::new(num, den).unwrap() };
        assert_eq!(owners.place(savings), [sum(me, 5, 8), sum(household, 3, 8)]);
    }

    #[test]
    fn a_cycle_is_reported_once_and_leaves_it_and_what_it_owns_without_owners() {
        let mut f = Fixture::new();
        let (me, grant, household, savings) = (f.me, f.grant, f.household, f.savings);
        f.entities[me].owned_by = [share(grant, 1, 1, 0)].into();
        f.entities[grant].owned_by = [share(me, 1, 1, 1)].into();
        f.entities[household].owned_by = [share(me, 1, 1, 2)].into();
        let (owners, problems, _) = of(f);
        let codes: Vec<_> = problems.iter().map(|problem| problem.code.as_ref()).collect();
        assert_eq!(codes, ["ownership-cycle"]);
        assert!([me, grant, household].iter().all(|&entity| owners.entity(entity).is_empty()));
        assert!(owners.place(savings).is_empty(), "the place is held by `me`, who has no owners");
    }

    #[test]
    fn an_overflowing_share_is_reported_where_it_is_declared_and_poisons_what_it_owns() {
        let mut f = Fixture::new();
        let (me, grant, household) = (f.me, f.grant, f.household);
        f.entities[household].owned_by = [share(me, 2, 1, 0)].into();
        f.entities[grant].owned_by = [share(household, i64::MAX, 1, 7)].into();
        let (owners, problems, _) = of(f);
        assert_eq!(problems.len(), 1);
        assert_eq!((problems[0].code.as_ref(), problems[0].labels[0].loc.start), ("ownership-overflow", 7));
        assert!(owners.entity(grant).is_empty() && !owners.entity(household).is_empty());
    }

    #[test]
    fn an_overflowing_place_share_leaves_only_that_place_without_owners() {
        let mut f = Fixture::new();
        let (me, household, savings, checking) = (f.me, f.household, f.savings, f.checking);
        f.entities[household].owned_by = [share(me, 2, 1, 0)].into();
        f.places[savings].shares = [share(household, i64::MAX, 1, 9)].into();
        let (owners, problems, _) = of(f);
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].labels[0].text, "this place share overflows while ownership is composed");
        assert!(owners.place(savings).is_empty() && !owners.place(checking).is_empty());
    }
}
