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

use axiom_core::{Days, Groups, Id};

use crate::book::{Book, Entity, Place, Sort, System};
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
        let written = WrittenIn::of(book);
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
        let mut timed = timed(book, &residents);
        timed.sort_by_key(|rule| rank[rule.law.index()]);
        Rules {
            on_in: Groups::build(places, on_in),
            on_out: Groups::build(places, on_out),
            on_gain: Groups::build(places, on_gain),
            always: Groups::build(places, always_on),
            on_spend: Groups::build(book.entities.len(), spending),
            // v3 bridge: no v3 law is an `on flow` law.
            purposes: Groups::default(),
            about: Groups::default(),
            timed,
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

/// Every rule that watches `place`.
fn watching_place(book: &Book, written: &WrittenIn, residents: &Residents, place: Id<Place>, out: &mut Vec<Rule>) {
    let owner = book.places[place].owner;
    for governing in book.places.lineage(place) {
        out.extend(written.places[governing].iter().map(|&law| always(law, Subject::Place(governing))));
    }
    for kind in book.kinds.lineage(book.places[place].kind) {
        out.extend(book.kinds[kind].laws.iter().map(|&law| always(law, Subject::Place(place))));
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
            // v3 bridge: no v3 law is written in a purpose, an asset or a contract.
            Owner::Purpose(_) | Owner::Asset(_) | Owner::Contract(_) => {}
            Owner::Kind(kind) => match book.kinds[kind].sort {
                Sort::Place(_) => {
                    let governed = book.places.iter().filter(|(_, place)| book.kinds.covers(kind, place.kind));
                    rules.extend(governed.map(|(place, _)| always(id, Subject::Place(place))));
                }
                Sort::Entity => {
                    let governed = book.entities.iter().filter(|(_, entity)| book.kinds.covers(kind, entity.kind));
                    rules.extend(governed.map(|(entity, _)| always(id, Subject::Entity(entity))));
                }
                Sort::Thing | Sort::Commodity => {}
            },
            Owner::System(_) => {
                for entity in book.entities.ids() {
                    rules.extend(residents.rules(book, entity, None).filter(|rule| rule.law == id));
                }
            }
            Owner::Book => {
                let mut owners: Vec<Id<Entity>> = book
                    .places
                    .values()
                    .map(|place| book.entities[place.owner].member.unwrap_or(place.owner))
                    .collect();
                owners.sort();
                owners.dedup();
                rules.extend(owners.into_iter().map(|owner| always(id, Subject::Entity(owner))));
            }
        }
    }
    rules
}
