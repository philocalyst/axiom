//! Governance: which laws watch which place, worked out once.
//!
//! A place is watched by its own laws and its ancestors' (a budget on
//! `expenses/food` covers `expenses/food/groceries`), by the laws of its kind
//! and its kind's ancestors, by the top-level laws of the systems its owner
//! lives under (dated by residence), and by the project's own top-level laws.
//! Every list is then put in dependency order (see [`crate::laws::rank`]): the
//! engine reads these tables and never searches.
//!
//! A household is governed as one. The top-level laws of the systems it lives
//! in govern every place it or its members own, with the household as `self`,
//! so a joint return reads one tally that both paychecks counted into. What is
//! personal stays with the person: the laws of a kind or of a place run with
//! the place's owner, and a member who lives somewhere of their own is governed
//! there as themselves.

use axiom_core::{Days, Groups, Id, Set};

use crate::book::{Book, Contract, Entity, Place, Role, Sort, System};
use crate::law::{Law, Owner, Rule, Rules, Subject, Trigger};

pub(crate) fn govern(book: &mut Book, rank: &[u32]) {
    book.rules = Rules::of(book, rank);
}

/// The laws of every system that governs one entity, dated: each system of
/// every residence's lineage, ancestors first, with residences of one system
/// merged so that its laws never run twice on a day.
type Governing = Vec<(Id<System>, Days)>;

fn governing(book: &Book, entity: Id<Entity>) -> Governing {
    let mut spans: Governing = Vec::new();
    for residence in book.entities[entity].lives.iter() {
        spans.extend(book.systems.lineage(residence.system).map(|system| (system, residence.days)));
    }
    spans.sort();
    let mut merged: Governing = Vec::with_capacity(spans.len());
    for (system, days) in spans {
        if let Some((last, held)) = merged.last_mut()
            && *last == system
            && let Some(both) = held.merge(days)
        {
            *held = both;
        } else {
            merged.push((system, days));
        }
    }
    merged
}

/// What governs each entity as a resident, before ranking.
struct Residents {
    governing: Vec<Governing>,
}

fn always(law: Id<Law>, subject: Subject) -> Rule {
    Rule { law, subject, days: Days::ALWAYS }
}

impl Residents {
    fn of(book: &Book) -> Residents {
        Residents { governing: book.entities.ids().map(|entity| governing(book, entity)).collect() }
    }

    /// The top-level laws of the systems `entity` lives under, as the entity,
    /// except those of systems that `except` lives under as itself.
    fn rules<'b>(
        &'b self,
        book: &'b Book,
        entity: Id<Entity>,
        except: Option<Id<Entity>>,
    ) -> impl Iterator<Item = Rule> + 'b {
        let own = except.map_or(&[][..], |other| &self.governing[other.index()][..]);
        let spans = self.governing[entity.index()].iter();
        spans.filter(move |(system, ..)| !own.iter().any(|(theirs, ..)| theirs == system)).flat_map(
            move |&(system, days)| {
                let laws = book.systems[system].laws.iter();
                laws.map(move |&law| Rule { law, subject: Subject::Entity(entity), days })
            },
        )
    }
}

impl Rules {
    fn of(book: &Book, rank: &[u32]) -> Rules {
        let auxiliary: Set<Id<Law>> = book.also.iter().map(|(_, also)| also.law).collect();
        let written = WrittenIn::of(book, &auxiliary);
        let residents = Residents::of(book);
        let (mut on_in, mut on_out, mut on_gain, mut always_on) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let mut watching = Vec::new();
        for place in book.places.ids() {
            watching.clear();
            watching_place(book, &written, &residents, place, &mut watching);
            watching.sort_by_key(|rule| rank[rule.law.index()]);
            for &rule in &watching {
                let table = match book.laws[rule.law].trigger {
                    Trigger::In => &mut on_in,
                    Trigger::Out => &mut on_out,
                    Trigger::Gain => &mut on_gain,
                    Trigger::Always => &mut always_on,
                    Trigger::Spend | Trigger::Flow | Trigger::Each(..) | Trigger::By(_) => continue,
                };
                table.push((place, rule));
            }
        }
        let places = book.places.len();
        let mut spending = spending(book, &written);
        spending.sort_by_key(|(_, rule)| rank[rule.law.index()]);
        let mut purposes = purpose_flows(book, &written, &auxiliary);
        purposes.sort_by_key(|(_, rule)| rank[rule.law.index()]);
        let mut about = asset_flows(book, &written, &auxiliary);
        about.sort_by_key(|(_, rule)| rank[rule.law.index()]);
        let mut contracts = contract_flows(book, &written, &auxiliary);
        contracts.sort_by_key(|(_, rule)| rank[rule.law.index()]);
        let mut timed = timed(book, &residents);
        timed.sort_by_key(|rule| rank[rule.law.index()]);
        let on_in = Groups::build(places, on_in.iter().copied());
        let on_out = Groups::build(places, on_out.iter().copied());
        let on_gain = Groups::build(places, on_gain.iter().copied());
        let always = Groups::build(places, always_on.iter().copied());
        let on_spend = Groups::build(book.entities.len(), spending.iter().copied());
        let purposes = Groups::build(book.purposes.len(), purposes.iter().copied());
        let about = Groups::build(places, about.iter().copied());
        let contracts = Groups::build(book.contracts.len(), contracts.iter().copied());
        Rules { on_in, on_out, on_gain, always, on_spend, purposes, about, contracts, timed }
    }
}

/// Laws grouped by what they were written in.
struct WrittenIn {
    places: Groups<Place, Id<Law>>,
    entities: Groups<Entity, Id<Law>>,
    purposes: Groups<crate::book::Purpose, Id<Law>>,
    contracts: Groups<Contract, Id<Law>>,
    assets: Groups<Place, Id<crate::book::Asset>>,
    asset_laws: Groups<crate::book::Asset, Id<Law>>,
    project: Vec<Id<Law>>,
}

impl WrittenIn {
    fn of(book: &Book, auxiliary: &Set<Id<Law>>) -> WrittenIn {
        let in_place = book.laws.iter().filter_map(|(id, law)| match law.owner {
            Owner::Place(place) if !auxiliary.contains(&id) => Some((place, id)),
            _ => None,
        });
        let in_entity = book.laws.iter().filter_map(|(id, law)| match law.owner {
            Owner::Entity(entity) if !auxiliary.contains(&id) => Some((entity, id)),
            _ => None,
        });
        let in_contract = book.laws.iter().filter_map(|(id, law)| match law.owner {
            Owner::Contract(contract) if !auxiliary.contains(&id) => Some((contract, id)),
            _ => None,
        });
        let in_purpose = book.laws.iter().filter_map(|(id, law)| match law.owner {
            Owner::Purpose(purpose) if !auxiliary.contains(&id) => Some((purpose, id)),
            _ => None,
        });
        let in_asset = book.laws.iter().filter_map(|(id, law)| match law.owner {
            Owner::Asset(asset) if !auxiliary.contains(&id) => Some((asset, id)),
            _ => None,
        });
        let assets_by_place = book.assets.iter().map(|(id, asset)| (asset.place, id));
        WrittenIn {
            places: Groups::build(book.places.len(), in_place),
            entities: Groups::build(book.entities.len(), in_entity),
            purposes: Groups::build(book.purposes.len(), in_purpose),
            contracts: Groups::build(book.contracts.len(), in_contract),
            assets: Groups::build(book.places.len(), assets_by_place),
            asset_laws: Groups::build(book.assets.len(), in_asset),
            project: book
                .laws
                .iter()
                .filter(|(id, law)| law.owner == Owner::Book && !auxiliary.contains(id))
                .map(|(id, _)| id)
                .collect(),
        }
    }
}

/// Whether a law is only an expression arena for an `also` line. Such a law
/// is evaluated by that line and must not be registered as an event rule.
/// Every rule that watches `place`.
fn watching_place(book: &Book, written: &WrittenIn, residents: &Residents, place: Id<Place>, out: &mut Vec<Rule>) {
    let owner = book.places[place].owner;
    for governing in book.places.lineage(place) {
        out.extend(written.places[governing].iter().map(|&law| always(law, Subject::Place(governing))));
    }
    for kind in book.kinds.lineage(book.places[place].kind) {
        if matches!(book.kinds[kind].sort, Sort::Place(_)) {
            out.extend(book.kinds[kind].laws.iter().map(|&law| always(law, Subject::Place(place))));
        }
    }
    for &asset in written.assets[place].iter() {
        for kind in book.kinds.lineage(book.assets[asset].kind) {
            out.extend(
                book.kinds[kind]
                    .laws
                    .iter()
                    .filter(|&&law| book.laws[law].trigger == Trigger::Always)
                    .map(|&law| always(law, Subject::Asset(asset))),
            );
        }
        out.extend(
            written.asset_laws[asset]
                .iter()
                .filter(|&&law| book.laws[law].trigger == Trigger::Always)
                .map(|&law| always(law, Subject::Asset(asset))),
        );
    }
    // The owner is governed where it lives, and so is the household it belongs
    // to: a household is governed as one, so its members' places answer to it.
    let household = book.entities[owner].member;
    out.extend(residents.rules(book, owner, None));
    if let Some(household) = household {
        out.extend(residents.rules(book, household, Some(owner)));
    }
    let resident = household.unwrap_or(owner);
    out.extend(written.project.iter().map(|&law| always(law, Subject::Entity(resident))));
}

/// A restricted entity's `on spend` laws: its kind chain's, then its own.
fn spending(book: &Book, written: &WrittenIn) -> Vec<(Id<Entity>, Rule)> {
    let mut rules = Vec::new();
    for (id, entity) in book.entities.iter().filter(|(_, entity)| entity.restricted) {
        let kind_laws = book.kinds.lineage(entity.kind).flat_map(|kind| book.kinds[kind].laws.iter().copied());
        let laws = kind_laws.chain(written.entities[id].iter().copied());
        let spends = laws.filter(|&law| book.laws[law].trigger == Trigger::Spend);
        rules.extend(spends.map(|law| (id, always(law, Subject::Entity(id)))));
    }
    rules
}

/// `each` and `by` laws, once for each subject they govern.
fn timed(book: &Book, residents: &Residents) -> Vec<Rule> {
    let mut rules = Vec::new();
    let timed_laws = book.laws.iter().filter(|(_, law)| matches!(law.trigger, Trigger::Each(..) | Trigger::By(_)));
    for (id, law) in timed_laws {
        match law.owner {
            Owner::Place(place) => rules.push(always(id, Subject::Place(place))),
            Owner::Entity(entity) => rules.push(always(id, Subject::Entity(entity))),
            Owner::Purpose(_) => {
                rules.extend(owners(book).into_iter().map(|entity| always(id, Subject::Entity(entity))))
            }
            Owner::Asset(asset) => rules.push(always(id, Subject::Asset(asset))),
            Owner::Contract(contract) => rules.push(always(id, Subject::Contract(contract))),
            Owner::Kind(kind) => match book.kinds[kind].sort {
                Sort::Place(_) => {
                    let governed = book.places.iter().filter(|(_, place)| book.kinds.covers(kind, place.kind));
                    rules.extend(governed.map(|(place, _)| always(id, Subject::Place(place))));
                }
                Sort::Entity => {
                    let governed = book.entities.iter().filter(|(_, entity)| book.kinds.covers(kind, entity.kind));
                    rules.extend(governed.map(|(entity, _)| always(id, Subject::Entity(entity))));
                }
                Sort::Thing => {
                    let governed = book.assets.iter().filter(|(_, asset)| book.kinds.covers(kind, asset.kind));
                    rules.extend(governed.map(|(asset, _)| always(id, Subject::Asset(asset))));
                }
                Sort::Commodity => {}
            },
            Owner::System(_) => {
                for entity in book.entities.ids() {
                    rules.extend(residents.rules(book, entity, None).filter(|rule| rule.law == id));
                }
            }
            Owner::Book => {
                rules.extend(owners(book).into_iter().map(|owner| always(id, Subject::Entity(owner))));
            }
        }
    }
    rules
}

/// Purpose laws are inherited by every descendant purpose. The placeholder
/// subject is replaced with the moving flow's owner when the engine fires it.
fn purpose_flows(book: &Book, written: &WrittenIn, auxiliary: &Set<Id<Law>>) -> Vec<(Id<crate::book::Purpose>, Rule)> {
    let mut rules = Vec::new();
    for actual in book.purposes.ids() {
        for ancestor in book.purposes.lineage(actual) {
            for &law in written.purposes[ancestor].iter() {
                if book.laws[law].trigger == Trigger::Flow && !auxiliary.contains(&law) {
                    rules.push((actual, always(law, Subject::Entity(book.roots.me))));
                }
            }
        }
    }
    rules
}

/// An asset's kind laws and its own laws run when a flow is for that asset.
/// The flow site is the asset's place; the rule subject is the identified asset.
fn asset_flows(book: &Book, written: &WrittenIn, auxiliary: &Set<Id<Law>>) -> Vec<(Id<Place>, Rule)> {
    let mut rules = Vec::new();
    for (asset, data) in book.assets.iter() {
        let place = data.place;
        for kind in book.kinds.lineage(data.kind) {
            rules.extend(
                book.kinds[kind]
                    .laws
                    .iter()
                    .copied()
                    .filter(|&law| book.laws[law].trigger == Trigger::Flow && !auxiliary.contains(&law))
                    .map(|law| (place, always(law, Subject::Asset(asset)))),
            );
        }
        rules.extend(
            written.asset_laws[asset]
                .iter()
                .copied()
                .filter(|&law| book.laws[law].trigger == Trigger::Flow && !auxiliary.contains(&law))
                .map(|law| (place, always(law, Subject::Asset(asset)))),
        );
    }
    rules
}

/// Contract laws are kept keyed by promise identity, never by the party shared
/// by two different contracts. The engine selects these rules from the flow's
/// contract provenance.
fn contract_flows(book: &Book, written: &WrittenIn, auxiliary: &Set<Id<Law>>) -> Vec<(Id<Contract>, Rule)> {
    let mut rules = Vec::new();
    for contract in book.contracts.ids() {
        for &law in written.contracts[contract].iter() {
            if !auxiliary.contains(&law) && !matches!(book.laws[law].trigger, Trigger::Each(..) | Trigger::By(_)) {
                rules.push((contract, always(law, Subject::Contract(contract))));
            }
        }
    }
    rules
}

/// Entities that have a holding or own a modeled thing or contract. Parties
/// with no such relationship do not get a timed law for every purpose.
fn owners(book: &Book) -> Vec<Id<Entity>> {
    let mut owners: Vec<_> = book
        .places
        .values()
        .filter(|place| matches!(place.role, Role::Holding(_)))
        .map(|place| book.entities[place.owner].member.unwrap_or(place.owner))
        .chain(book.assets.values().map(|asset| book.entities[asset.owner].member.unwrap_or(asset.owner)))
        .chain(book.contracts.values().map(|contract| book.entities[contract.owner].member.unwrap_or(contract.owner)))
        .collect();
    owners.sort_unstable();
    owners.dedup();
    owners
}
