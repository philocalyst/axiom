//! Everything the journal records besides transactions: balance assertions,
//! settlement events, prices and splits, and the declarations that go with them:
//! code rules and syncs.

use axiom_core::glob::is_pattern;
use axiom_core::{Day, Diagnostic, Id};
use axiom_syntax::{self as ast, EventState, Item};

use super::shape::{Elab, Priced};
use crate::book::{Amount, CodeRule, CodeScope, PathRoot, Place};
use crate::collect::Entry;
use crate::declare::World;
use crate::journal::{Assert, Gap, Quote, Split, Waive};
use crate::scope::Home;
use crate::sync::{Fetch, Sink, Source};

/// A settlement event, before its code is checked against the flows.
pub(crate) struct RawEvent<'s> {
    pub day: Day,
    pub code: &'s str,
    pub code_loc: axiom_core::Loc,
    pub state: EventState,
    pub loc: axiom_core::Loc,
}

impl<'s> Elab<'_, 's> {
    /// `2026-01-31 checking = 7_921.30 USD [! | via PLACE]`
    pub fn assertion(&mut self, item: &Item<'s>, assert: &ast::Assert<'s>) {
        let place = self.end_of(assert.place.name.0);
        let amount = assert.amount;
        let stated = match amount.unit() {
            Some(_) => self.amount(amount).map(Some),
            None => Some(None),
        };
        let gap = match assert.gap {
            ast::Gap::Refused => Some(Gap::Refused),
            ast::Gap::Waived(waive) => Some(Gap::Unexplained(Waive {
                loc: waive.at,
                reason: waive.reason.map(|reason| self.world.sym(reason)),
            })),
            ast::Gap::Via(name) => self.end_of(name.0).map(|end| Gap::Via { place: end, loc: self.file.loc(name.0) }),
        };
        let (Some(place), Some(stated), Some(gap)) = (place, stated, gap) else {
            return;
        };
        let amount = stated.unwrap_or_else(|| zero_of(self.world, place));
        self.sink.asserts.push(Assert { day: assert.date, place, amount, gap, loc: item.loc });
    }

    /// A place, or an entity's `via`; a name that is neither is noted.
    fn end_of(&mut self, text: &'s str) -> Option<Id<Place>> {
        match self.world.find_end(text) {
            Ok(end) => Some(end.place),
            Err(cause) => {
                self.sink.misses.push((cause, text, self.file.loc(text)));
                None
            }
        }
    }

    /// `2026-02-06 #check-1041 settled`: checked once every flow is known.
    pub fn event(&mut self, item: &Item<'s>, event: &ast::Event<'s>) {
        let code = event.code;
        self.sink.events.push(RawEvent {
            day: event.date,
            code: code.name(),
            code_loc: self.file.loc(code.0),
            state: event.state,
            loc: item.loc,
        });
    }

    /// `2026-01-02 VTI 280.14 USD`
    pub fn price_line(&mut self, item: &Item<'s>, price: &ast::Price<'s>) {
        let (unit, quoted) = (self.commodity(price.unit.0), self.price(price.price));
        let (Some(unit), Some(Priced { rate, quote, .. })) = (unit, quoted) else { return };
        match unit == quote {
            true => self.sink.diags.push(
                Diagnostic::error("price-self", "a commodity's price in itself is always one")
                    .label(self.file.loc(price.unit.0), "priced in itself")
                    .help("quote it in another commodity"),
            ),
            false => self.sink.quotes.push(Quote { unit, quote, day: price.date, rate, implied: false, loc: item.loc }),
        }
    }

    /// `2026-05-22 FAST split 2 for 1`
    pub fn split_line(&mut self, item: &Item<'s>, split: &ast::Split<'s>) {
        let unit = self.commodity(split.unit.0);
        let ratio =
            split.numerator.to_ratio().zip(split.denominator.to_ratio()).and_then(|(new, old)| new.checked_div(old));
        match (unit, ratio) {
            (Some(unit), Some(ratio)) => self.sink.splits.push(Split { day: split.date, unit, ratio, loc: item.loc }),
            (Some(_), None) => self.sink.diags.push(
                Diagnostic::error("bad-split", "this split is too large to count exactly")
                    .label(item.loc, "the ratio does not fit")
                    .help("write it in smaller numbers"),
            ),
            (None, _) => {}
        }
    }
}

/// `= empty` asserts nothing is left: zero of the account's only commodity, or
/// of the base currency when it holds several.
fn zero_of(world: &World, place: Id<Place>) -> Amount {
    match world.book.places[place].holds.as_deref() {
        Some([only]) => Amount::zero(*only),
        _ => Amount::zero(world.book.base),
    }
}

/// `code trip-*` / `on expenses/travel/*`: where codes may appear. A rule from
/// a system the project does not use never applies.
pub(super) fn code_rules<'s>(world: &mut World<'s>, entries: &[Entry<'_, 's>], diags: &mut Vec<Diagnostic>) {
    for entry in entries {
        let Entry::Code(written) = entry else {
            continue;
        };
        let (file, rule) = (written.file(), written.node);
        let mut scopes = Vec::new();
        for name in &file[rule.on] {
            let word = crate::errors::Word { text: name.0, loc: file.loc(name.0) };
            if is_pattern(name.0) || PathRoot::of(name.0).is_some() {
                scopes.push(CodeScope::Places(world.book.names.intern(name.0)));
                continue;
            }
            match world.kind(written.home(), word) {
                Ok(kind) => scopes.push(CodeScope::Kind(kind)),
                Err(diagnostic) => diags.push(diagnostic),
            }
        }
        if world.scopes.of(Home::Project).sees(written.home()) {
            let pattern = world.book.names.intern(rule.pattern.0.strip_prefix('#').unwrap_or(rule.pattern.0));
            // v3 bridge: no v3 code says how it appears in a memo.
            world.book.codes.push(CodeRule {
                pattern,
                on: scopes.into(),
                known_as: Box::default(),
                loc: written.item.loc,
            });
        }
    }
}

pub(super) fn sources<'s>(world: &mut World<'s>, entries: &[Entry<'_, 's>]) -> Vec<Source> {
    let names = &mut world.book.names;
    let sources = entries.iter().filter_map(|entry| match entry {
        Entry::Sync(written) => {
            let file = names.intern(written.node.file.0);
            // v3 bridge: `sync FILE` / `run COMMAND` names the file the command's Axiom is merged into.
            Some(Source {
                name: file,
                fetch: Fetch::Run(names.intern(written.node.run.0)),
                format: None,
                sink: Sink::File(file),
                system: None,
                doc: None,
                loc: written.item.loc,
            })
        }
        _ => None,
    });
    sources.collect()
}
