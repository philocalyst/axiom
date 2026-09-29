//! Governance: which laws watch which place, worked out once.
//!
//! A place is watched by its own laws and its ancestors' (a budget on
//! `expenses/food` covers `expenses/food/groceries`), by the laws of its kind
//! and its kind's ancestors, by the top-level laws of the systems its owner
//! lives under (dated by residence), and by the project's own top-level laws,
//! in that order. The engine reads these tables and never searches.

use axiom_core::{Day, Groups, Id};

use crate::book::{Book, Entity, Place, Sort};
use crate::law::{Law, Owner, Rule, Rules, Subject, Trigger};

/// The span of a rule that has always applied and always will.
const FOREVER: (Day, Day) = (Day(i32::MIN), Day(i32::MAX));

pub(crate) fn govern(book: &mut Book) {
    book.rules = Rules::of(book);
}

impl Rules {
    fn of(book: &Book) -> Rules {
        let written = WrittenIn::of(book);
        let (mut on_in, mut on_out, mut on_gain, mut always) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let mut watching = Vec::new();
        for place in book.places.ids() {
            watching.clear();
            watching_place(book, &written, place, &mut watching);
            for &rule in &watching {
                let table = match book.laws[rule.law].trigger {
                    Trigger::In => &mut on_in,
                    Trigger::Out => &mut on_out,
                    Trigger::Gain => &mut on_gain,
                    Trigger::Always => &mut always,
                    Trigger::Spend | Trigger::Each(_) | Trigger::By(_) => continue,
                };
                table.push((place, rule));
            }
        }
        let places = book.places.len();
        Rules {
            on_in: Groups::build(places, on_in),
            on_out: Groups::build(places, on_out),
            on_gain: Groups::build(places, on_gain),
            always: Groups::build(places, always),
            on_spend: Groups::build(book.entities.len(), spending(book, &written)),
            timed: timed(book),
        }
    }
}

/// Laws grouped by what they were written in.
struct WrittenIn {
    places: Groups<Place, Id<Law>>,
    entities: Groups<Entity, Id<Law>>,
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
        WrittenIn {
            places: Groups::build(book.places.len(), in_place),
            entities: Groups::build(book.entities.len(), in_entity),
            project: book.laws.iter().filter(|(_, law)| law.owner == Owner::Book).map(|(id, _)| id).collect(),
        }
    }
}

fn always(law: Id<Law>, subject: Subject) -> Rule {
    Rule { law, subject, from: FOREVER.0, until: FOREVER.1 }
}

/// Every rule that watches `place`, in the order the engine fires them.
fn watching_place(book: &Book, written: &WrittenIn, place: Id<Place>, out: &mut Vec<Rule>) {
    let owner = book.places[place].owner;
    for governing in book.places.lineage(place) {
        out.extend(written.places[governing].iter().map(|&law| always(law, Subject::Place(governing))));
    }
    for kind in book.kinds.lineage(book.places[place].kind) {
        out.extend(book.kinds[kind].laws.iter().map(|&law| always(law, Subject::Place(place))));
    }
    residence_rules(book, owner, out);
    out.extend(written.project.iter().map(|&law| always(law, Subject::Entity(owner))));
}

/// The top-level laws of `entity`'s jurisdictions, each dated by the residence
/// that brings it. A jurisdiction includes its ancestors, outermost first, so
/// `us` counts a wage before `us/ca` reads the tally.
fn residence_rules(book: &Book, entity: Id<Entity>, out: &mut Vec<Rule>) {
    for (at, residence) in book.entities[entity].lives.iter().enumerate() {
        let until = residence_end(book, entity, at);
        let mut chain: Vec<_> = book.systems.lineage(residence.system).collect();
        chain.reverse();
        for system in chain {
            let dated = book.systems[system].laws.iter().map(|&law| Rule {
                law,
                subject: Subject::Entity(entity),
                from: residence.from,
                until,
            });
            out.extend(dated);
        }
    }
}

/// The last day of the residence at `at`: the day before the next begins.
fn residence_end(book: &Book, entity: Id<Entity>, at: usize) -> Day {
    match book.entities[entity].lives.get(at + 1) {
        Some(next) => Day(next.from.0.saturating_sub(1)),
        None => FOREVER.1,
    }
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
fn timed(book: &Book) -> Vec<Rule> {
    let mut rules = Vec::new();
    let timed_laws = book.laws.iter().filter(|(_, law)| matches!(law.trigger, Trigger::Each(_) | Trigger::By(_)));
    for (id, law) in timed_laws {
        match law.owner {
            Owner::Place(place) => rules.push(always(id, Subject::Place(place))),
            Owner::Entity(entity) => rules.push(always(id, Subject::Entity(entity))),
            Owner::Kind(kind) => match book.kinds[kind].sort {
                Sort::Place(_) => {
                    let governed = book.places.iter().filter(|(_, place)| book.kinds.covers(kind, place.kind));
                    rules.extend(governed.map(|(place, _)| always(id, Subject::Place(place))));
                }
                Sort::Entity => {
                    let governed = book.entities.iter().filter(|(_, entity)| book.kinds.covers(kind, entity.kind));
                    rules.extend(governed.map(|(entity, _)| always(id, Subject::Entity(entity))));
                }
                Sort::Commodity => {}
            },
            Owner::System(system) => {
                for (entity, _) in book.entities.iter() {
                    let residences = book.entities[entity].lives.iter().enumerate();
                    for (at, residence) in
                        residences.filter(|(_, residence)| book.systems.covers(system, residence.system))
                    {
                        let until = residence_end(book, entity, at);
                        rules.push(Rule { law: id, subject: Subject::Entity(entity), from: residence.from, until });
                    }
                }
            }
            Owner::Book => {
                let mut owners: Vec<Id<Entity>> = book.places.values().map(|place| place.owner).collect();
                owners.sort();
                owners.dedup();
                rules.extend(owners.into_iter().map(|owner| always(id, Subject::Entity(owner))));
            }
        }
    }
    rules
}
