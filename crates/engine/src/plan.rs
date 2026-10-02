//! The plan: everything the fold decides before it begins, once.
//!
//! A book has loose ends. `?` amounts are solved from the assertions around
//! them, settlement events change flows' states, and the laws' static shape
//! (which ones only cap a total, which totals must be counted, when the timed
//! ones fall due) and who contains what can be read off before the first flow.
//! None of it depends on the day the fold stops or on what the fold finds, so
//! a [`Plan`] is built once, never changes, and is shared by reference: every
//! [`Ledger`] borrows it, a fork copies only the world, the clock and the
//! records, and any number of threads can fold from it at once.

use axiom_core::{Day, Diagnostic, Groups, Id, Map, Qty, Ratio, Set, Sym};
use axiom_model::{
    Asset, Book, Commodity, Entity, Field, Flow, Func, Kind, Op, Place, Rule, Subject, Txn, Ty, Value, Var,
};

use crate::events::{self, Events};
use crate::facts::{self, LawFacts, PurposeReaders, Readers};
use crate::ledger::{Ledger, fold, fold_to_view, fold_to_view_and_effects_prefix};
use crate::motion::Amounts;
use crate::scope::containing;
use crate::sides::Sides;
use crate::state::World;
use crate::temporal::Query;
use crate::timeline::{self, Schedule};
use crate::totals::Watch;
use crate::{Options, OwnerShare, Run, infer};

/// The names the fold and the views look for by spelling, resolved once: what
/// a law reads (`born`), what marks a currency, a loan's term (`maturity`) and
/// the law a `budget` line compiles to. Each is `None` in a book that never
/// mentions it, and is compared as a `Sym` or an id, never as text.
#[derive(Clone, Copy, Debug)]
pub struct Known {
    pub born: Option<Sym>,
    pub maturity: Option<Sym>,
    pub budget: Option<Sym>,
    pub currency: Option<Id<Kind>>,
}

impl Known {
    pub fn of(book: &Book) -> Known {
        let name = |text| book.names.get(text);
        Known {
            born: name("born"),
            maturity: name("maturity"),
            budget: name("budget"),
            currency: book.kind("currency").ok(),
        }
    }
}

/// What the solve pass decided about a book. Immutable and `Sync`.
pub struct Plan<'b, 's> {
    pub(crate) book: &'b Book<'s>,
    pub(crate) events: Events,
    /// The quantities of the flows written `? USD`, as far as the assertions
    /// around them could solve them. Flows the fold resolves (`=`, `all`) are
    /// the ledger's to remember, for they depend on the balance.
    pub(crate) amounts: Map<Id<Flow>, Amounts>,
    /// The first day of each place and commodity whose balance depends on an
    /// amount that could not be solved, and the flow to blame.
    pub(crate) unsolved: Map<(Id<Place>, Id<Commodity>), (Day, Id<Flow>)>,
    /// What reading the events and solving reported: every ledger starts with them.
    problems: Vec<Diagnostic>,
    pub(crate) known: Known,
    /// The sign each place's balance is shown in.
    pub(crate) sides: Sides,
    /// By law id: what is true of the law whatever runs it.
    pub(crate) laws: Box<[LawFacts]>,
    /// Some list of rules brings one law to one subject twice: `fire` must not run it twice.
    pub(crate) repeats: bool,
    /// The laws to read as a window opens with value already recognized into it.
    pub(crate) readers: Readers,
    /// Purpose values that enter a month or year ahead of time, and the
    /// purpose laws that read those windows. Flow-only laws never run here.
    pub(crate) purpose_readers: PurposeReaders,
    /// The subjects whose flow totals some law reads.
    pub(crate) watch: Watch,
    /// The asset places each entity holds: its own, its subsidiaries' and its
    /// members', in place order.
    members: Groups<Entity, Id<Place>>,
    /// Places inside each identified asset, including every part's place.
    asset_places: Groups<Asset, Id<Place>>,
    /// Effective financial owners for each declared entity and place.
    entity_owners: Groups<Entity, OwnerShare>,
    place_owners: Groups<Place, OwnerShare>,
    /// Places under kinds read by a widened total. Only law-referenced kinds
    /// are indexed, so books without those reads pay no grouping cost.
    pub(crate) kind_places: Map<Id<Kind>, Box<[Id<Place>]>>,
    /// Sparse dated transaction rows that keep a scheduled contract occurrence.
    /// Ordinary transactions are already represented by their flows.
    pub(crate) occurrence_txns: Box<[Id<Txn>]>,
    /// The day of the first fact that starts a period; before it there is nothing to close.
    pub(crate) period_start: Option<Day>,
    last_fact: Option<Day>,
    /// By index in `Rules::timed`: when each falls due.
    pub(crate) timed: Box<[Schedule]>,
    /// Expressions whose value is inspected across time by `peak`, `low` or `days`.
    pub(crate) temporal: Box<[Query]>,
    temporal_dates: Box<[Day]>,
    daily_temporal: bool,
}

impl<'b, 's> Plan<'b, 's> {
    /// Solves what the journal leaves open (`?` amounts, settlement events;
    /// `=` targets and `all` wait for the fold, which knows the balance) and
    /// works out what the laws need before the first flow.
    pub fn new(book: &'b Book<'s>) -> Plan<'b, 's> {
        let (events, mut problems) = events::read(book);
        let sides = Sides::of(book);
        let solution = infer::solve(book, &events, &sides);
        problems.extend(solution.problems);
        let unsolved = solution.unsolved.iter().flat_map(|&id| {
            let flow = &book.flows[id];
            [((flow.from, flow.out.unit), (flow.day, id)), ((flow.to, flow.arrive.unit), (flow.day, id))]
        });
        let mut blocked: Map<_, (Day, Id<Flow>)> = Map::default();
        for (key, first) in unsolved {
            blocked.entry(key).and_modify(|known| *known = (*known).min(first)).or_insert(first);
        }
        let laws: Box<[LawFacts]> = book.laws.values().map(|law| LawFacts::of(book, law)).collect();
        let watch = Watch::of(book, &laws);
        let places = (0..book.places.len() as u32).map(Id::new);
        let held = places.flat_map(|place| {
            containing(book, place).filter_map(move |subject| match subject {
                Subject::Entity(entity) => Some((entity, place)),
                _ => None,
            })
        });
        let kind_places = kind_places(book);
        let entity_owners = entity_owners(book, &mut problems);
        let place_owners = place_owners(book, &entity_owners, &mut problems);
        let (temporal, temporal_dates, daily_temporal) = temporal_queries(book, &entity_owners, &place_owners);
        let mut occurrence_txns: Vec<_> =
            book.txns.iter().filter_map(|(id, txn)| txn.occurrence.is_some().then_some(id)).collect();
        occurrence_txns.sort_unstable_by_key(|&id| (book.txns[id].day, id));
        let mut plan = Plan {
            book,
            amounts: solution.amounts,
            unsolved: blocked,
            problems,
            known: Known::of(book),
            sides,
            repeats: repeats(book),
            readers: facts::readers(book, &laws),
            purpose_readers: facts::purpose_readers(book),
            watch,
            members: Groups::build(book.entities.len(), held),
            asset_places: asset_places(book),
            entity_owners,
            place_owners,
            kind_places,
            occurrence_txns: occurrence_txns.into_boxed_slice(),
            period_start: timeline::start(book, &events),
            last_fact: timeline::last_fact(book, &events),
            events,
            laws,
            timed: Box::default(),
            temporal: temporal.into_boxed_slice(),
            temporal_dates: temporal_dates.into_boxed_slice(),
            daily_temporal,
        };
        let (world, mut values) = (World::new(book, &plan.watch), Vec::new());
        plan.timed = book.rules.timed.iter().map(|rule| Schedule::of(&plan, rule, &world, &mut values)).collect();
        plan
    }

    pub fn book(&self) -> &'b Book<'s> {
        self.book
    }

    /// Dated changes that must be sampled even when the journal has no fact that day.
    pub(crate) fn temporal_dates(&self) -> &[Day] {
        &self.temporal_dates
    }

    /// Whether temporal expressions depend on a value that can change each day.
    pub(crate) fn needs_daily_temporal(&self) -> bool {
        self.daily_temporal
    }

    /// First dated fact that can establish a temporal value. A timeless
    /// residence is part of the initial state but should not create unbounded
    /// history before the first dated event.
    pub(crate) fn temporal_start(&self) -> Option<Day> {
        self.period_start.into_iter().chain(self.temporal_dates.iter().copied().filter(|&day| day != Day::MIN)).min()
    }

    /// Effective financial owners of `place`, including nested business
    /// ownership. Household membership remains governance/scope only.
    pub fn owners_of(&self, place: Id<Place>) -> &[OwnerShare] {
        &self.place_owners[place]
    }

    /// Effective financial owners of an entity, including nested businesses.
    pub fn owners_of_entity(&self, entity: Id<Entity>) -> &[OwnerShare] {
        &self.entity_owners[entity]
    }

    /// Splits a signed quantity among a place's effective owners. Cumulative
    /// boundaries are rounded once and the final owner receives the remainder,
    /// so positive and negative amounts both conserve every quantum.
    pub fn allocate(&self, place: Id<Place>, amount: Qty) -> impl Iterator<Item = (OwnerShare, Qty)> + '_ {
        allocate_owners(self.owners_of(place), amount)
    }

    /// Splits a signed quantity among an entity's effective owners.
    pub fn allocate_entity(&self, entity: Id<Entity>, amount: Qty) -> impl Iterator<Item = (OwnerShare, Qty)> + '_ {
        allocate_owners(self.owners_of_entity(entity), amount)
    }

    /// The names looked up by spelling, resolved once.
    pub fn known(&self) -> Known {
        self.known
    }

    /// The sign each place's balance is shown in.
    pub fn sides(&self) -> &Sides {
        &self.sides
    }

    /// A ledger at the day before the first fact, ready to fold.
    pub fn start(&self, options: Options) -> Ledger<'_, 'b, 's> {
        Ledger::start(self, options)
    }

    /// The journal folded through `options.today` (and every later journal fact).
    pub fn run(&self, options: Options) -> Run {
        fold(self, options)
    }

    /// [`run`](Plan::run), and with it the ledger as it stood on `options.today`
    /// before that day's closings. A view that asks what a withdrawal would
    /// cost, or what the year would owe, forks that ledger (from any number of
    /// threads) instead of folding the journal again.
    pub fn run_with_view(&self, options: Options) -> (Run, Ledger<'_, 'b, 's>) {
        fold_to_view(self, options)
    }

    /// [`run_with_view`](Plan::run_with_view), and the number of effects in
    /// the ledger before that day's closings. The returned run includes later
    /// same-day closings and any later journal facts; this prefix length marks
    /// exactly the effects that already existed in the view checkpoint.
    pub fn run_with_view_and_effects_prefix(&self, options: Options) -> (Run, Ledger<'_, 'b, 's>, usize) {
        fold_to_view_and_effects_prefix(self, options)
    }

    /// Whether `place` lies within `subject`: a place's subtree is a stretch of
    /// the pre-order, and an entity's places are listed once.
    pub(crate) fn inside(&self, subject: Subject, place: Id<Place>) -> bool {
        match subject {
            Subject::Place(root) => self.book.places.covers(root, place),
            Subject::Entity(root) => self.members[root].binary_search(&place).is_ok(),
            Subject::Asset(asset) => self.asset_places[asset].binary_search(&place).is_ok(),
            Subject::Contract(contract) => {
                self.members[self.book.contracts[contract].owner].binary_search(&place).is_ok()
            }
        }
    }

    /// The asset places an entity holds, in place order.
    pub(crate) fn places_of(&self, entity: Id<Entity>) -> &[Id<Place>] {
        &self.members[entity]
    }

    /// The places occupied by an asset and every declared part beneath it.
    pub(crate) fn places_of_asset(&self, asset: Id<Asset>) -> &[Id<Place>] {
        &self.asset_places[asset]
    }

    /// The last day the fold reaches when it is run for `today`: `today`, or
    /// the journal's last fact if that is later.
    pub(crate) fn horizon(&self, today: Day) -> Day {
        self.last_fact.map_or(today, |last| last.max(today))
    }

    /// The problems solving found, for a ledger's record to begin with.
    pub(crate) fn problems(&self) -> Vec<Diagnostic> {
        self.problems.clone()
    }
}

fn allocate_owners(owners: &[OwnerShare], amount: Qty) -> impl Iterator<Item = (OwnerShare, Qty)> + '_ {
    let last = owners.len().saturating_sub(1);
    owners.iter().copied().enumerate().scan(
        (Ratio::ZERO, Qty::ZERO),
        move |(cumulative_share, allocated), (index, owner)| {
            *cumulative_share = cumulative_share
                .checked_add(owner.share)
                .expect("effective owner shares fit the plan's checked ratios");
            let boundary = if index == last {
                amount
            } else {
                amount.scale(*cumulative_share).expect("an owner's quantity fits the source quantity")
            };
            let part = boundary - *allocated;
            *allocated = boundary;
            Some((owner, part))
        },
    )
}

fn temporal_queries(
    book: &Book,
    entity_owners: &Groups<Entity, OwnerShare>,
    place_owners: &Groups<Place, OwnerShare>,
) -> (Vec<Query>, Vec<Day>, bool) {
    let mut queries = Vec::new();
    let mut daily = false;
    for rule in book.rules.all() {
        let law = &book.laws[rule.law];
        let owners: Vec<Id<Entity>> = match rule.subject {
            Subject::Place(place) => place_owners[place].iter().map(|share| share.owner).collect(),
            Subject::Entity(entity) => entity_owners[entity].iter().map(|share| share.owner).collect(),
            Subject::Asset(asset) => place_owners[book.assets[asset].place].iter().map(|share| share.owner).collect(),
            Subject::Contract(contract) => {
                let entity = book.contracts[contract].owner;
                entity_owners[entity].iter().map(|share| share.owner).collect()
            }
        };
        for (id, node) in law.nodes.iter() {
            let Op::Call(func @ (Func::Peak | Func::Low | Func::Days), args) = &node.op else {
                continue;
            };
            let Some(&root) = args.first() else { continue };
            let call = axiom_model::NodeId(id.index() as u32);
            for &owner in &owners {
                queries.push(Query {
                    key: crate::temporal::Key { law: rule.law, subject: rule.subject, owner, call, part: None },
                    func: *func,
                    root,
                });
            }
            daily |= law.nodes.values().take(root.index() + 1).any(|node| match &node.op {
                Op::Var(Var::Date | Var::Year | Var::Month) => true,
                Op::Field(_, Field::Year | Field::Month | Field::Age) => true,
                _ => false,
            });
        }
    }
    queries.sort_by_key(|query| {
        let subject = match query.key.subject {
            Subject::Place(id) => (0, id.index()),
            Subject::Entity(id) => (1, id.index()),
            Subject::Asset(id) => (2, id.index()),
            Subject::Contract(id) => (3, id.index()),
        };
        (query.key.law.index(), subject, query.key.owner.index(), query.key.call.index())
    });
    queries.dedup_by_key(|query| query.key);

    let mut dates = Vec::new();
    for (_, place) in book.places.iter() {
        add_prop_dates(&mut dates, &place.props);
    }
    for (_, entity) in book.entities.iter() {
        add_prop_dates(&mut dates, &entity.props);
        for residence in entity.lives.iter() {
            dates.push(residence.days.first());
            if residence.days.last() != Day::MAX {
                dates.push(residence.days.last().add_days(1));
            }
        }
    }
    for (_, asset) in book.assets.iter() {
        add_prop_dates(&mut dates, &asset.props);
    }
    for (_, commodity) in book.commodities.iter() {
        add_prop_dates(&mut dates, &commodity.props);
    }
    for (_, kind) in book.kinds.iter() {
        add_prop_dates(&mut dates, &kind.props);
    }
    for (_, param) in book.params.iter() {
        dates.extend(param.rows.iter().filter_map(|row| row.since));
    }
    dates.extend(book.prices.quotes().iter().map(|quote| quote.day));
    dates.sort_unstable();
    dates.dedup();
    (queries, dates, daily)
}

fn add_prop_dates(dates: &mut Vec<Day>, props: &[axiom_model::Prop]) {
    dates.extend(props.iter().map(|prop| prop.since).filter(|&day| day != Day::MIN));
}

fn entity_owners(book: &Book, diagnostics: &mut Vec<Diagnostic>) -> Groups<Entity, OwnerShare> {
    let edges: Vec<Vec<(Id<Entity>, Ratio, axiom_core::Loc)>> = book
        .entities
        .ids()
        .map(|entity| {
            let declaration = &book.entities[entity];
            if declaration.owned_by.is_empty() {
                declaration
                    .owner
                    .filter(|&owner| owner != entity)
                    .map(|owner| vec![(owner, Ratio::ONE, declaration.loc.unwrap_or_default())])
                    .unwrap_or_default()
            } else {
                declaration.owned_by.iter().map(|share| (share.entity, share.rate, share.loc)).collect()
            }
        })
        .collect();
    let mut state = vec![0u8; book.entities.len()];
    let mut active_at = vec![None; book.entities.len()];
    let mut invalid = vec![false; book.entities.len()];
    let mut flattened: Vec<Option<Vec<OwnerShare>>> = vec![None; book.entities.len()];

    for root in book.entities.ids() {
        if state[root.index()] != 0 {
            continue;
        }
        let mut stack = vec![(root, 0usize)];
        state[root.index()] = 1;
        active_at[root.index()] = Some(0);
        while let Some(&(current, edge_at)) = stack.last() {
            if edge_at < edges[current.index()].len() {
                let (parent, _, loc) = edges[current.index()][edge_at];
                stack.last_mut().expect("the current ownership node is on the stack").1 += 1;
                match state[parent.index()] {
                    0 => {
                        state[parent.index()] = 1;
                        active_at[parent.index()] = Some(stack.len());
                        stack.push((parent, 0));
                    }
                    1 => {
                        let first = active_at[parent.index()].unwrap_or(0);
                        for (member, _) in &stack[first..] {
                            invalid[member.index()] = true;
                        }
                        diagnostics.push(
                            Diagnostic::error("ownership-cycle", "entity ownership contains a cycle")
                                .label(loc, "this ownership edge closes the cycle"),
                        );
                    }
                    _ => {}
                }
                continue;
            }

            let (entity, _) = stack.pop().expect("the current ownership node is on the stack");
            active_at[entity.index()] = None;
            state[entity.index()] = 2;
            if edges[entity.index()].iter().any(|(parent, _, _)| invalid[parent.index()]) {
                invalid[entity.index()] = true;
            }
            if invalid[entity.index()] {
                flattened[entity.index()] = Some(Vec::new());
                continue;
            }
            if edges[entity.index()].is_empty() {
                flattened[entity.index()] = Some(vec![OwnerShare { owner: entity, share: Ratio::ONE }]);
                continue;
            }

            let mut rates = Map::default();
            let mut overflow = None;
            for &(parent, weight, loc) in &edges[entity.index()] {
                for owner in flattened[parent.index()].as_deref().unwrap_or_default() {
                    let Some(rate) = weight.checked_mul(owner.share) else {
                        overflow = Some(loc);
                        break;
                    };
                    let total = rates.get(&owner.owner).copied().unwrap_or(Ratio::ZERO);
                    let Some(total) = total.checked_add(rate) else {
                        overflow = Some(loc);
                        break;
                    };
                    rates.insert(owner.owner, total);
                }
                if overflow.is_some() {
                    break;
                }
            }
            if let Some(loc) = overflow {
                diagnostics.push(
                    Diagnostic::error("ownership-overflow", "effective ownership share is too large to represent")
                        .label(loc, "this share overflows while ownership is composed"),
                );
                invalid[entity.index()] = true;
                flattened[entity.index()] = Some(Vec::new());
            } else {
                flattened[entity.index()] = Some(sorted_shares(rates));
            }
        }
    }

    let pairs = book.entities.ids().flat_map(|entity| {
        flattened[entity.index()].as_deref().unwrap_or_default().iter().copied().map(move |owner| (entity, owner))
    });
    Groups::build(book.entities.len(), pairs)
}

fn place_owners(
    book: &Book,
    entity_owners: &Groups<Entity, OwnerShare>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Groups<Place, OwnerShare> {
    let mut pairs = Vec::new();
    for place in book.places.ids() {
        let declared = &book.places[place].shares;
        if declared.is_empty() {
            pairs.extend(entity_owners[book.places[place].owner].iter().copied().map(|share| (place, share)));
        } else {
            let mut rates = Map::default();
            let mut overflow = None;
            for share in declared {
                for owner in &entity_owners[share.entity] {
                    let Some(rate) = share.rate.checked_mul(owner.share) else {
                        overflow = Some(share.loc);
                        break;
                    };
                    let total = rates.get(&owner.owner).copied().unwrap_or(Ratio::ZERO);
                    let Some(total) = total.checked_add(rate) else {
                        overflow = Some(share.loc);
                        break;
                    };
                    rates.insert(owner.owner, total);
                }
                if overflow.is_some() {
                    break;
                }
            }
            if let Some(loc) = overflow {
                diagnostics.push(
                    Diagnostic::error("ownership-overflow", "effective ownership share is too large to represent")
                        .label(loc, "this place share overflows while ownership is composed"),
                );
                rates.clear();
            }
            pairs.extend(sorted_shares(rates).into_iter().map(|share| (place, share)));
        }
    }
    let owners = Groups::build(book.places.len(), pairs.iter().copied());
    drop(pairs);
    owners
}

fn sorted_shares(rates: Map<Id<Entity>, Ratio>) -> Vec<OwnerShare> {
    let mut shares: Vec<_> = rates.into_iter().map(|(owner, share)| OwnerShare { owner, share }).collect();
    shares.sort_unstable_by_key(|share| share.owner);
    shares
}

/// Whether some list of rules brings one law to one subject twice, as two
/// residences under one system do.
fn repeats(book: &Book) -> bool {
    let rules = &book.rules;
    let per_place = rules.per_place().into_iter().flat_map(|table| table.iter().map(|(_, list)| list));
    let lists = per_place
        .chain(rules.on_spend.iter().map(|(_, list)| list))
        .chain(rules.purposes.iter().map(|(_, list)| list))
        .chain(rules.about.iter().map(|(_, list)| list))
        .chain([&rules.timed[..]]);
    lists.into_iter().any(|list: &[Rule]| {
        let mut seen = Set::default();
        list.iter().any(|rule| !seen.insert((rule.law, rule.subject)))
    })
}

/// Builds each identified thing's place scope, including the places of its
/// parts. Asset boundaries are fixed by the book, so law filtering need not
/// walk the asset table for every flow.
fn asset_places(book: &Book) -> Groups<Asset, Id<Place>> {
    let mut within = Vec::new();
    for (part, asset) in book.assets.iter() {
        let places: Vec<_> = book.places.subtree(asset.place).collect();
        let mut whole = Some(part);
        while let Some(root) = whole {
            within.extend(places.iter().copied().map(|place| (root, place)));
            whole = book.assets[root].part_of.map(|part| part.value);
        }
    }
    within.sort_unstable();
    within.dedup();
    let places = Groups::build(book.assets.len(), within.iter().copied());
    drop(within);
    places
}

/// Builds the static place set for every kind a `total` reads. If a typed
/// kind argument is computed at run time, any kind can be selected, so all
/// kinds are indexed for that book.
fn kind_places(book: &Book) -> Map<Id<Kind>, Box<[Id<Place>]>> {
    let mut requested = Set::default();
    let mut dynamic = false;
    for law in book.laws.values() {
        for node in law.nodes.values() {
            let Op::Call(Func::Total(..), args) = &node.op else {
                continue;
            };
            for &argument in args.iter().filter(|&&argument| law.nodes[argument].typed_ty() == Some(Ty::Kind)) {
                match &law.nodes[argument].op {
                    Op::Const(Value::Kind(kind)) => {
                        requested.insert(*kind);
                    }
                    _ => dynamic = true,
                }
            }
        }
    }
    if dynamic {
        // A computed kind may select any bucket; walk each place's ancestry
        // once instead of rescanning the place table for every book kind.
        let mut places: Map<Id<Kind>, Vec<Id<Place>>> = book.kinds.ids().map(|kind| (kind, Vec::new())).collect();
        for (place, value) in book.places.iter() {
            for kind in book.kinds.lineage(value.kind) {
                places.get_mut(&kind).expect("every book kind is indexed").push(place);
            }
        }
        return places.into_iter().map(|(kind, matching)| (kind, matching.into_boxed_slice())).collect();
    }
    if requested.is_empty() {
        return Map::default();
    }
    requested
        .into_iter()
        .map(|kind| {
            let places = book
                .places
                .iter()
                .filter(|(_, place)| book.is_a(place.kind, kind))
                .map(|(place, _)| place)
                .collect::<Vec<_>>()
                .into_boxed_slice();
            (kind, places)
        })
        .collect()
}

/// The journal folded through `options.today` (and every later journal fact):
/// a plan built to be used once.
pub fn run(book: &Book, options: Options) -> Run {
    Plan::new(book).run(options)
}
