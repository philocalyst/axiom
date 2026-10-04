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

use axiom_core::{Days, Diagnostic, Groups, Id, Set};

use crate::book::{Book, Contract, Entity, Place, Role, Sort, System};
use crate::law::{Keys, Law, Owner, Rule, Rules, Subject, Trigger, Watch};

pub(crate) fn govern(book: &mut Book, rank: &[u32]) {
    book.rules = Rules::of(book, rank);
}

/// Says of each `on flow` law of the project that no flow can reach, because nothing it governs exists: the `also` of
/// a kind that nothing is of is never read, and nothing in the fold would say so. A system's laws are written for every
/// book that lives under it, and most books have no thing of the kinds they govern.
pub(crate) fn unreached(book: &Book, diags: &mut Vec<Diagnostic>) {
    let reached: Set<Id<Law>> = book.rules.all().iter().map(|rule| rule.law).collect();
    let flows = book
        .laws
        .iter()
        .filter(|(id, law)| law.trigger == Trigger::Flow && law.system.is_none() && !reached.contains(id));
    for (_, law) in flows {
        let Owner::Kind(kind) = law.owner else { continue };
        let of = match book.kinds[kind].sort {
            Sort::Place(_) => "account",
            Sort::Entity => "entity",
            Sort::Thing => "asset",
            Sort::Commodity | Sort::Contract => continue,
        };
        let name = book.name(book.kinds[kind].name);
        diags.push(
            Diagnostic::warning("law-never-fires", format!("no flow can reach this law: no {of} is of kind `{name}`"))
                .label(law.loc, format!("this watches the flows of every {of} of kind `{name}`, and there is none"))
                .help(format!("declare an {of} of kind `{name}`, or remove the law")),
        );
    }
}

/// The laws of every system that governs one entity, dated: each system of
/// every residence's lineage, ancestors first, with residences of one system
/// merged so that its laws never run twice on a day.
type Governing = Vec<(Id<System>, Days)>;

fn governing(book: &Book, entity: Id<Entity>) -> Governing {
    let mut spans: Governing = Vec::new();
    for (days, system) in book.residences(entity) {
        spans.extend(book.systems.lineage(system).map(|system| (system, days)));
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
        let written = WrittenIn::of(book);
        let residents = Residents::of(book);
        let mut entries = Vec::new();
        let mut watching = Vec::new();
        for place in book.places.ids() {
            watching.clear();
            watching_place(book, &written, &residents, place, &mut watching);
            entries.extend(watching.iter().filter_map(|&rule| Some((place_watch(book, rule, place)?, rule))));
        }
        entries.extend(spending(book, &written));
        entries.extend(purpose_flows(book, &written));
        entries.extend(asset_flows(book, &written));
        entries.extend(contract_flows(book, &written));
        entries.extend(timed(book, &residents).into_iter().map(|rule| (Watch::Timed, rule)));
        // One stable sort puts every row in dependency order, and keeps the order a row was filled in among equals.
        entries.sort_by_key(|(_, rule)| rank[rule.law.index()]);
        Rules::build(Keys::of(book), entries.iter().copied())
    }
}

/// How a rule that watches `place` is looked up, if it is looked up by a place at all.
fn place_watch(book: &Book, rule: Rule, place: Id<Place>) -> Option<Watch> {
    match book.laws[rule.law].trigger {
        Trigger::In => Some(Watch::In(place)),
        Trigger::Out => Some(Watch::Out(place)),
        Trigger::Gain => Some(Watch::Gain(place)),
        Trigger::Always => Some(Watch::Always(place)),
        Trigger::Flow => Some(Watch::Touching(place)),
        Trigger::Spend | Trigger::Each(..) | Trigger::By(_) => None,
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
    fn of(book: &Book) -> WrittenIn {
        let in_place = book.laws.iter().filter_map(|(id, law)| match law.owner {
            Owner::Place(place) => Some((place, id)),
            _ => None,
        });
        let in_entity = book.laws.iter().filter_map(|(id, law)| match law.owner {
            Owner::Entity(entity) => Some((entity, id)),
            _ => None,
        });
        let in_contract = book.laws.iter().filter_map(|(id, law)| match law.owner {
            Owner::Contract(contract) => Some((contract, id)),
            _ => None,
        });
        let in_purpose = book.laws.iter().filter_map(|(id, law)| match law.owner {
            Owner::Purpose(purpose) => Some((purpose, id)),
            _ => None,
        });
        let in_asset = book.laws.iter().filter_map(|(id, law)| match law.owner {
            Owner::Asset(asset) => Some((asset, id)),
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
            project: book.laws.iter().filter(|(_, law)| law.owner == Owner::Book).map(|(id, _)| id).collect(),
        }
    }
}

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
    out.extend(standing_flows(book, written, book.standing_at(place)));
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
    let household = book.member(owner);
    out.extend(residents.rules(book, owner, None));
    if let Some(household) = household {
        out.extend(residents.rules(book, household, Some(owner)));
    }
    let resident = household.unwrap_or(owner);
    out.extend(written.project.iter().map(|&law| always(law, Subject::Entity(resident))));
}

/// What the entity that stands at a place says about a flow there: the `on flow` laws of its kind chain, then its own. The
/// rest of what an entity writes is about its money leaving or about time, and is not looked up by a place.
fn standing_flows<'b>(book: &'b Book, written: &'b WrittenIn, entity: Id<Entity>) -> impl Iterator<Item = Rule> + 'b {
    let kind_laws = book.kinds.lineage(book.entities[entity].kind).flat_map(|kind| book.kinds[kind].laws.iter());
    let laws = kind_laws.chain(written.entities[entity].iter());
    let flows = laws.filter(|&&law| book.laws[law].trigger == Trigger::Flow);
    flows.map(move |&law| always(law, Subject::Entity(entity)))
}

/// A restricted entity's `on spend` laws: its kind chain's, then its own.
fn spending(book: &Book, written: &WrittenIn) -> Vec<(Watch, Rule)> {
    let mut rules = Vec::new();
    for (id, entity) in book.entities.iter().filter(|&(id, _)| book.is_restricted(id)) {
        let kind_laws = book.kinds.lineage(entity.kind).flat_map(|kind| book.kinds[kind].laws.iter().copied());
        let laws = kind_laws.chain(written.entities[id].iter().copied());
        let spends = laws.filter(|&law| book.laws[law].trigger == Trigger::Spend);
        rules.extend(spends.map(|law| (Watch::Spend(id), always(law, Subject::Entity(id)))));
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
                Sort::Commodity | Sort::Contract => {}
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
fn purpose_flows(book: &Book, written: &WrittenIn) -> Vec<(Watch, Rule)> {
    let mut rules = Vec::new();
    for actual in book.purposes.ids() {
        for ancestor in book.purposes.lineage(actual) {
            for &law in written.purposes[ancestor].iter() {
                if book.laws[law].trigger == Trigger::Flow {
                    rules.push((Watch::Purpose(actual), always(law, Subject::Entity(book.roots.me))));
                }
            }
        }
    }
    rules
}

/// An asset's kind laws and its own laws run when a flow is for that asset.
/// The flow site is the asset's place; the rule subject is the identified asset.
fn asset_flows(book: &Book, written: &WrittenIn) -> Vec<(Watch, Rule)> {
    let mut rules = Vec::new();
    for (asset, data) in book.assets.iter() {
        let place = data.place;
        for kind in book.kinds.lineage(data.kind) {
            rules.extend(
                book.kinds[kind]
                    .laws
                    .iter()
                    .copied()
                    .filter(|&law| book.laws[law].trigger == Trigger::Flow)
                    .map(|law| (Watch::About(place), always(law, Subject::Asset(asset)))),
            );
        }
        rules.extend(
            written.asset_laws[asset]
                .iter()
                .copied()
                .filter(|&law| book.laws[law].trigger == Trigger::Flow)
                .map(|law| (Watch::About(place), always(law, Subject::Asset(asset)))),
        );
    }
    rules
}

/// Contract laws are kept keyed by promise identity, never by the party shared by two different contracts. The laws
/// that judge a flow are read as it posts, and the laws that derive one when an occurrence is made.
fn contract_flows(book: &Book, written: &WrittenIn) -> Vec<(Watch, Rule)> {
    let mut rules = Vec::new();
    for contract in book.contracts.ids() {
        let flow_laws = written.contracts[contract]
            .iter()
            .filter(|&&law| !matches!(book.laws[law].trigger, Trigger::Each(..) | Trigger::By(_)));
        for &law in flow_laws {
            let watch = if book.laws[law].derives() { Watch::Occurrence(contract) } else { Watch::Contract(contract) };
            rules.push((watch, always(law, Subject::Contract(contract))));
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
        .map(|place| book.member(place.owner).unwrap_or(place.owner))
        .chain(book.assets.values().map(|asset| book.member(asset.owner).unwrap_or(asset.owner)))
        .chain(book.contracts.values().map(|contract| book.member(contract.owner).unwrap_or(contract.owner)))
        .collect();
    owners.sort_unstable();
    owners.dedup();
    owners
}
