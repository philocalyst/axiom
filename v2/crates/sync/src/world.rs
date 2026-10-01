//! A statement's way into the journal (LANGUAGE §14): which records are already
//! written, who the rest are, which keep a promise or name a claim, and what is
//! left is written in the house style.

use std::fmt::Write;

use axiom_core::num::POW10;
use axiom_core::{Day, Diagnostic, FileId, Interner, Map, Qty};
use axiom_engine::{Run as EngineRun, State};
use axiom_model::{Book, Place, Role};

use crate::amount::amount;
use crate::date::iso_day;
use crate::format::Format;
use crate::promise::{Due, keep};
use crate::recognize::{Reading, Recognizer, Scratch, Tie, Who};
use crate::reconcile::{Existing, reconcile};
use crate::write::Layout;
use crate::{Form, Insert, Record, Unit};

/// What the book says about one account.
#[derive(Clone, Default)]
pub struct Account<'a> {
    /// Every flow, leg, derived flow and batch total the account has.
    pub flows: Vec<Existing<'a>>,
    /// The days it has an assertion on.
    pub asserted: Vec<Day>,
}

impl Account<'_> {
    /// The day a source of this account should start from: the day after its
    /// latest flow, or `first` if it has none.
    pub fn since(&self, first: Day) -> Day {
        self.flows
            .iter()
            .map(|flow| flow.day)
            .max()
            .map_or(first, |last| last.add_days(1))
    }
}

/// Everything the book knows that sync reads. Each source adds what it writes,
/// so that a transfer both accounts show is written once.
pub struct World<'b, 's> {
    pub book: &'b Book<'s>,
    pub run: &'b EngineRun,
    pub recognizer: Recognizer<'b, 's>,
    pub layout: Layout,
    pub accounts: Map<&'s str, Account<'s>>,
    /// Every unit the book has, for a record that names its own currency.
    pub units: Vec<Unit<'s>>,
    /// Occurrences that are due and not written.
    pub dues: Vec<Due<'s>>,
    /// Open claims that carry a code: the code, and the party it is with.
    pub claims: Map<&'s str, &'s str>,
}

/// A source of records for one account.
pub struct Feed<'b, 's> {
    /// The account its records are of, unless a record says otherwise (`route`).
    pub account: &'s str,
    pub unit: Unit<'s>,
    pub format: &'b Format,
}

/// A line to write, and what it moves.
struct Line<'s> {
    day: Day,
    body: String,
    /// The accounts it touches, in what unit, and by how much, money into them positive.
    moved: Vec<(&'s str, Option<&'s str>, Qty)>,
}

impl<'s> Line<'s> {
    fn statement(day: Day, body: String) -> Line<'s> {
        Line {
            day,
            body,
            moved: Vec::new(),
        }
    }
}

/// The other end of a record: whom it was with, through whom, what it is for,
/// and which codes it carries.
#[derive(Default)]
struct Other<'s> {
    who: Option<Who<'s>>,
    via: Option<&'s str>,
    codes: Vec<&'s str>,
    /// A purpose the export's own category maps to, and the thing it is of.
    purpose: Option<(axiom_core::Sym, Option<String>)>,
}

/// What the memo and the export's own `party` and `via` say of a record.
struct Told<'t, 's> {
    memo: Reading<'t, 's>,
    party: Option<Reading<'t, 's>>,
    via: Option<Reading<'t, 's>>,
}

impl<'b, 's> World<'b, 's> {
    /// The inserts that bring the book up to a statement, or what is wrong with
    /// it. Nothing is changed unless all of it can be read.
    pub fn feed(
        &mut self,
        feed: &Feed<'b, 's>,
        text: &str,
    ) -> Result<Vec<Insert>, Vec<Diagnostic>> {
        self.feed_at(feed, text, FileId(0))
    }

    /// As [`feed`](Self::feed), with the file identity that owns the imported
    /// text. Local imports and captured command output use auxiliary files so
    /// diagnostics point into the actual input, not an arbitrary source file.
    pub fn feed_at(
        &mut self,
        feed: &Feed<'b, 's>,
        text: &str,
        file: FileId,
    ) -> Result<Vec<Insert>, Vec<Diagnostic>> {
        let (records, problems) = crate::format::read(
            self.book,
            feed.format,
            text,
            file,
            feed.unit,
            &self.units,
        );
        if !problems.is_empty() {
            return Err(problems);
        }
        let mut planned = Vec::new();
        for (account, records) in self.routed(feed, records)? {
            planned.push((account, self.plan(account, feed, records)?));
        }
        let mut inserts = Vec::new();
        for (account, (lines, asserted)) in planned {
            for line in &lines {
                for &(name, unit, qty) in &line.moved {
                    let flow = Existing {
                        unit,
                        ..Existing::new(line.day, qty)
                    };
                    self.accounts.entry(name).or_default().flows.push(flow);
                }
            }
            if let Some(day) = asserted {
                self.accounts.entry(account).or_default().asserted.push(day);
            }
            let insert = |line: Line| Insert {
                path: self.layout.file_for(line.day),
                day: line.day,
                form: Form::Item(line.body),
            };
            inserts.extend(lines.into_iter().map(insert));
        }
        Ok(inserts)
    }

    /// What lines the book's own flows say of the accounts it has: a document a
    /// source printed (an invoice paid, a payout with its fee) is a flow the
    /// bank's line for the same money is then matched with.
    pub fn learn(&mut self, inserts: &[Insert]) {
        for insert in inserts {
            let Form::Item(body) = &insert.form else {
                continue;
            };
            for (name, unit, qty) in self.moved_by(body) {
                let flow = Existing {
                    unit: Some(unit),
                    ..Existing::new(insert.day, qty)
                };
                self.accounts.entry(name).or_default().flows.push(flow);
            }
        }
    }

    /// `FROM -> TO 970 USD` and the `+` and `-` items under it: what it moves on
    /// each end that is an account of the book. Pending, unpriced and unknown
    /// units move nothing.
    fn moved_by(&self, body: &str) -> Vec<(&'s str, &'s str, Qty)> {
        let mut lines = body.lines();
        let words: Vec<&str> = lines.next().unwrap_or("").split_whitespace().collect();
        let [from, "->", to, number, unit, ..] = words.as_slice() else {
            return Vec::new();
        };
        let Some(unit) = self.units.iter().find(|known| known.name == *unit) else {
            return Vec::new();
        };
        let parse = |number: &str| amount(&number.replace('_', ""), unit.scale).ok().flatten();
        let Some(mut net) = parse(number) else {
            return Vec::new();
        };
        for item in lines {
            let words: Vec<&str> = item.split_whitespace().collect();
            match words.as_slice() {
                ["+", number, name, ..] if *name == unit.name => {
                    net += parse(number).unwrap_or_default()
                }
                ["-", number, name, ..] if *name == unit.name => {
                    net -= parse(number).unwrap_or_default()
                }
                _ => {}
            }
        }
        let ends = [(*from, -net), (*to, net)];
        ends.iter()
            .filter_map(|&(name, qty)| Some((self.recognizer.account(name)?, unit.name, qty)))
            .collect()
    }

    /// The records of each account they belong to: the feed's, unless a record's
    /// `route` names another.
    fn routed<'t>(
        &self,
        feed: &Feed<'b, 's>,
        records: Vec<Record<'t>>,
    ) -> Result<Vec<(&'s str, Vec<Record<'t>>)>, Vec<Diagnostic>> {
        let (mut groups, mut problems): (Vec<(&'s str, Vec<Record<'t>>)>, Vec<Diagnostic>) =
            (Vec::new(), Vec::new());
        let mut scratch = Scratch::default();
        for record in records {
            let account = match record.facts().route.as_deref() {
                None => feed.account,
                Some(route) => {
                    let named = self
                        .recognizer
                        .read(route, &mut scratch)
                        .who
                        .ok()
                        .and_then(|found| found.who);
                    match named.filter(|who| who.account) {
                        Some(who) => who.name,
                        None => {
                            let headline = format!("`{route}` is not an account the book knows");
                            problems.push(Diagnostic::error("unknown-route", headline).label(record.at, "this row").help(
                                "give the account a `known-as` that matches how the export names it, or name it as the export does",
                            ));
                            continue;
                        }
                    }
                }
            };
            match groups.iter_mut().find(|(name, _)| *name == account) {
                Some((_, group)) => group.push(record),
                None => groups.push((account, vec![record])),
            }
        }
        if problems.is_empty() {
            Ok(groups)
        } else {
            Err(problems)
        }
    }

    /// The unit a record is counted in.
    fn unit_of(&self, feed: &Feed<'b, 's>, record: &Record) -> Unit<'s> {
        let named = record.facts().currency.as_deref();
        let found = named.and_then(|name| {
            self.units
                .iter()
                .find(|unit| unit.name.eq_ignore_ascii_case(name))
        });
        found.copied().unwrap_or(feed.unit)
    }

    /// The lines an account's records add, and the day it is asserted on, if it is.
    fn plan<'t>(
        &self,
        account: &'s str,
        feed: &Feed<'b, 's>,
        mut records: Vec<Record<'t>>,
    ) -> Result<(Vec<Line<'s>>, Option<Day>), Vec<Diagnostic>> {
        // A memo may say its own amount or day: what the record has to be matched by.
        let mut readings = self
            .recognizer
            .read_all(&records.iter().collect::<Vec<_>>());
        self.adopt(feed, &mut records, &readings)?;
        // Stable, so that a day's records keep the export's order.
        let mut order: Vec<usize> = (0..records.len()).collect();
        order.sort_by_key(|&at| records[at].day);
        let mut slots: Vec<Option<(Record<'t>, Reading<'t, 's>)>> = records
            .into_iter()
            .zip(readings.drain(..))
            .map(Some)
            .collect();
        let (records, readings): (Vec<Record<'t>>, Vec<Reading<'t, 's>>) =
            order.iter().filter_map(|&at| slots[at].take()).unzip();

        let existing = self.accounts.get(account);
        let flows = existing.map_or(&[][..], |account| &account.flows);
        let matched = reconcile(&records, flows, feed.unit.name);
        let told = self.told(&records, readings, &matched)?;
        let others: Vec<Option<Other<'s>>> = (0..records.len())
            .map(|at| {
                told[at]
                    .as_ref()
                    .map(|told| self.other(feed, account, &records[at], told))
            })
            .collect();
        // Only a record that is neither written nor pending can keep a promise.
        let parties: Vec<Option<&str>> = (0..records.len())
            .map(|at| {
                let who = others[at]
                    .as_ref()
                    .filter(|_| !records[at].pending)
                    .and_then(|other| other.who);
                who.filter(|who| !who.account).map(|who| who.name)
            })
            .collect();
        let dues: Vec<Due> = self
            .dues
            .iter()
            .filter(|due| due.account == account)
            .cloned()
            .collect();
        let kept = keep(&records, &parties, &dues);

        // Pending flows carry a code of their own, for the record that posts
        // them to settle. It counts on from the flows the account has that day.
        let mut per_day: Map<Day, usize> = Map::default();
        if records.iter().any(|record| record.pending) {
            flows
                .iter()
                .for_each(|flow| *per_day.entry(flow.day).or_default() += 1);
        }
        let mut pending_code = |day: Day| {
            let number = per_day.entry(day).or_default();
            *number += 1;
            format!("pending-{}-{number}", day.to_string().replace('-', ""))
        };
        let exchanges = self.exchanges(&records, &others, &kept);
        let mut lines = Vec::new();
        for (at, record) in records
            .iter()
            .enumerate()
            .filter(|(_, record)| !record.qty.is_zero())
        {
            let line = match (matched[at], kept[at], &others[at]) {
                (Some(flow), _, _) => {
                    let settles = flows[flow].settle.filter(|_| !record.pending);
                    settles.map(|code| Line::statement(record.day, format!("^{code} settled")))
                }
                (None, Some(due), _) => Some(occurrence(
                    &dues[due],
                    record,
                    account,
                    self.unit_of(feed, record),
                )),
                (None, None, Some(other)) => match exchanges[at] {
                    Exchange::Second => None,
                    Exchange::First(with) => {
                        Some(self.exchange(account, feed, record, &records[with], other))
                    }
                    Exchange::No => {
                        let code = record.pending.then(|| pending_code(record.day));
                        Some(self.new_flow(account, feed, record, other, code.as_deref()))
                    }
                },
                (None, None, None) => None,
            };
            lines.extend(line);
        }
        let closing = closing_of(&records)
            .filter(|(day, _)| existing.is_none_or(|acct| !acct.asserted.contains(day)));
        if let Some((day, balance)) = closing {
            let shown = match balance.is_negative() {
                true => format!("-{}", money(balance.abs(), feed.unit)),
                false => money(balance, feed.unit),
            };
            lines.push(Line::statement(day, format!("{account} = {shown}")));
        }
        Ok((lines, closing.map(|(day, _)| day)))
    }

    /// Gives a record the amount and the day its memo says of itself, when a
    /// pattern captured them: the record is matched by them, and written with them.
    fn adopt<'t>(
        &self,
        feed: &Feed<'b, 's>,
        records: &mut [Record<'t>],
        readings: &[Reading<'t, 's>],
    ) -> Result<(), Vec<Diagnostic>> {
        let mut problems = Vec::new();
        for (record, reading) in records.iter_mut().zip(readings) {
            let unit = self.unit_of(feed, record);
            let bad = |what: &str, text: &str, record: &Record| {
                let headline = format!("`{text}`, which a pattern took for the {what}, is not one");
                Diagnostic::error("bad-capture", headline).label(record.at, "in this memo")
            };
            if let Some(text) = &reading.amount {
                match amount(&text.replace('_', ""), unit.scale) {
                    Ok(Some(qty)) if !qty.is_zero() => {
                        record.qty = if record.qty.is_negative() {
                            -qty.abs()
                        } else {
                            qty.abs()
                        }
                    }
                    _ => problems.push(bad("amount", text, record)),
                }
            }
            if let Some(text) = &reading.date {
                match crate::format::date_layout(feed.format)
                    .map_or_else(|| iso_day(text), |layout| layout.read(text))
                {
                    Some(day) => record.day = day,
                    None => problems.push(bad("day", text, record)),
                }
            }
            if let Some(text) = &reading.original {
                match original(text, &self.units) {
                    Some(mut original) => {
                        // The memo may omit a sign because the statement's
                        // amount supplies the direction of the converted leg.
                        original.qty = if record.qty.is_negative() {
                            -original.qty.abs()
                        } else {
                            original.qty.abs()
                        };
                        record.facts.get_or_insert_with(Default::default).original = Some(original);
                    }
                    None => problems.push(bad("original amount and unit", text, record)),
                }
            }
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(problems)
        }
    }

    /// What the memo, and the export's own `party` and `via`, say of each record
    /// that is not yet written; ties are errors. A written record, or one that
    /// moves nothing, is not in the way of anything.
    fn told<'t>(
        &self,
        records: &[Record<'t>],
        readings: Vec<Reading<'t, 's>>,
        matched: &[Option<usize>],
    ) -> Result<Vec<Option<Told<'t, 's>>>, Vec<Diagnostic>> {
        let (mut scratch, mut problems) = (Scratch::default(), Vec::new());
        let mut told = Vec::with_capacity(records.len());
        for ((record, memo), matched) in records.iter().zip(readings).zip(matched) {
            if matched.is_some() || record.qty.is_zero() {
                told.push(None);
                continue;
            }
            let mut read = |text: &Option<std::borrow::Cow<str>>| {
                text.as_deref()
                    .map(|text| self.recognizer.read(text, &mut scratch))
            };
            let (party, via) = (read(&record.facts().party), read(&record.facts().via));
            for read in [party.as_ref(), via.as_ref()].into_iter().flatten() {
                if let Err(tie) = &read.who {
                    problems.push(tie_error(self.book, record, tie));
                }
            }
            let has_structured_party = party
                .as_ref()
                .and_then(|read| read.who.as_ref().ok())
                .is_some_and(|who| who.who.is_some())
                || via
                    .as_ref()
                    .and_then(|read| read.who.as_ref().ok())
                    .is_some_and(|who| who.who.is_some());
            let has_claim_code = record
                .facts()
                .code
                .as_deref()
                .is_some_and(|code| self.claims.contains_key(code));
            if !has_structured_party && !has_claim_code {
                if let Err(tie) = &memo.who {
                    problems.push(tie_error(self.book, record, tie));
                }
            }
            told.push(Some(Told { memo, party, via }));
        }
        if problems.is_empty() {
            Ok(told)
        } else {
            Err(problems)
        }
    }

    /// Who a record was with. What the export says of it beats what patterns
    /// find in its memo: first the party of the open claim its own `code` names,
    /// then its `party`, then its `via`, and last the memo. Whoever the export
    /// names, the memo's party is the go-between. Only the codes of the claims
    /// of that party are carried, and the record's own code.
    fn other<'t>(
        &self,
        feed: &Feed<'b, 's>,
        account: &str,
        record: &Record<'t>,
        told: &Told<'t, 's>,
    ) -> Other<'s> {
        let recognized =
            |reading: &Reading<'t, 's>| reading.who.as_ref().ok().copied().unwrap_or_default();
        let memo = recognized(&told.memo);
        let named = |reading: &Option<Reading<'t, 's>>| {
            reading.as_ref().and_then(|reading| recognized(reading).who)
        };
        let facts = record.facts();
        let own_claim = facts
            .code
            .as_deref()
            .and_then(|code| self.claims.get(code))
            .map(|&party| Who {
                name: party,
                account: false,
            });
        let structured = own_claim.or(named(&told.party)).or(named(&told.via));
        let mut other = Other::default();
        (other.who, other.via) = match structured {
            Some(who) => (
                Some(who),
                memo.who
                    .filter(|go_between| !go_between.account && go_between.name != who.name)
                    .map(|w| w.name),
            ),
            None => (memo.who, memo.via),
        };
        // Money with the account itself is no other end.
        if other.who.is_some_and(|who| who.name == account) {
            (other.who, other.via) = (None, None);
        }
        for code in told.memo.codes.iter().copied().chain(facts.code.as_deref()) {
            let Some((&claim, &party)) = self.claims.get_key_value(code) else {
                continue;
            };
            match other.who {
                None => {
                    other.who = Some(Who {
                        name: party,
                        account: false,
                    })
                }
                Some(who) if who.name != party => continue,
                Some(_) => {}
            }
            other.codes.push(claim);
        }
        let purpose = facts
            .category
            .as_deref()
            .and_then(|category| crate::format::category(feed.format, self.book, category));
        other.purpose = purpose.map(|purpose| {
            (
                self.book.purposes[purpose].name,
                facts.object.as_deref().map(str::to_string),
            )
        });
        other
    }

    /// Which records are the two sides of one exchange: the same `id`, one
    /// unit out and another in, both new and neither pending.
    fn exchanges(
        &self,
        records: &[Record],
        others: &[Option<Other>],
        kept: &[Option<usize>],
    ) -> Vec<Exchange> {
        let mut by_id: Map<&str, Vec<usize>> = Map::default();
        for (at, record) in records.iter().enumerate() {
            let free = others[at].is_some()
                && kept[at].is_none()
                && !record.pending
                && !record.qty.is_zero();
            if let Some(id) = record.facts().id.as_deref().filter(|_| free) {
                by_id.entry(id).or_default().push(at);
            }
        }
        let mut exchanges = vec![Exchange::No; records.len()];
        for ends in by_id.values() {
            let [a, b] = ends[..] else { continue };
            let (units, signs) = (
                (&records[a].facts().currency, &records[b].facts().currency),
                (records[a].qty, records[b].qty),
            );
            if units.0 != units.1 && signs.0.is_negative() != signs.1.is_negative() {
                // The side that leaves is the first of the two, so the line says out then in.
                let (out, into) = if signs.0.is_negative() {
                    (a, b)
                } else {
                    (b, a)
                };
                (exchanges[out], exchanges[into]) = (Exchange::First(into), Exchange::Second);
            }
        }
        exchanges
    }

    /// `checking 100 USD -> 92 EUR "memo"`: the two sides of an exchange on one account.
    fn exchange<'t>(
        &self,
        account: &'s str,
        feed: &Feed<'b, 's>,
        out: &Record<'t>,
        into: &Record<'t>,
        other: &Other<'s>,
    ) -> Line<'s> {
        let (unit_out, unit_in) = (self.unit_of(feed, out), self.unit_of(feed, into));
        let mut body = format!(
            "{account} {} -> {}",
            money(out.qty.abs(), unit_out),
            money(into.qty.abs(), unit_in)
        );
        tail(self.book, &mut body, other, Some(out), out.facts());
        let moved = vec![
            (account, Some(unit_out.name), out.qty),
            (account, Some(unit_in.name), into.qty),
        ];
        Line {
            day: out.day.min(into.day),
            body,
            moved,
        }
    }

    /// `checking -> trader-joes 84.20 USD`: the flow a record is, with the other
    /// end it was with, or `?` and its memo as a description if nobody is known.
    /// A payout with its fee is the gross, and the fee an item under it.
    fn new_flow(
        &self,
        account: &'s str,
        feed: &Feed<'b, 's>,
        record: &Record,
        other: &Other<'s>,
        pending: Option<&str>,
    ) -> Line<'s> {
        let unit = self.unit_of(feed, record);
        let end = other.who.map_or("?", |who| who.name);
        let (from, to) = if record.qty.is_negative() {
            (account, end)
        } else {
            (end, account)
        };
        let facts = record.facts();
        let fee = facts.fee.filter(|fee| !fee.is_zero());
        let header = fee.map_or(record.qty.abs(), |fee| {
            facts.gross.unwrap_or(record.qty.abs() + fee)
        });
        let amount = money(header, unit);
        let mut body = format!(
            "{from} -> {to} {}",
            if record.pending {
                format!("({amount})")
            } else {
                amount
            }
        );
        tail(
            self.book,
            &mut body,
            other,
            Some(record).filter(|_| other.who.is_none()),
            record.facts(),
        );
        if let Some(code) = pending {
            let _ = write!(body, " ^{code}");
        }
        if let Some(via) = other.via {
            let _ = write!(body, " via {via}");
        }
        if let Some(fee) = fee {
            let sign = if record.qty.is_negative() { '+' } else { '-' };
            let _ = write!(body, "\n  {sign} {} #fees via {account}", money(fee, unit));
        }
        // What arrives at the other end, if that is an account of the book too.
        let transfer = other
            .who
            .filter(|who| who.account)
            .map(|who| (who.name, Some(unit.name), -record.qty));
        let moved = [Some((account, Some(unit.name), record.qty)), transfer]
            .into_iter()
            .flatten()
            .collect();
        Line {
            day: record.day,
            body,
            moved,
        }
    }
}

/// Parse a typed `CUR amount` captured by an `original` pattern. Keep the
/// currency spelling borrowed from the memo and use that unit's precision.
fn original<'t>(text: &'t str, units: &[Unit<'_>]) -> Option<crate::Original<'t>> {
    let text = text.trim();
    let split = text.find(char::is_whitespace)?;
    let (unit, amount_text) = text.split_at(split);
    let unit_spec = units
        .iter()
        .find(|known| known.name.eq_ignore_ascii_case(unit))?;
    let qty = amount(amount_text.trim(), unit_spec.scale).ok().flatten()?;
    Some(crate::Original {
        qty,
        unit: unit.into(),
    })
}

#[cfg(test)]
mod original_tests {
    use super::*;

    #[test]
    fn original_capture_is_a_typed_currency_amount_and_keeps_its_text_borrowed() {
        let units = [Unit {
            name: "CHF",
            scale: 2,
        }];
        let text = "CHF 3,290.00";
        let parsed = original(text, &units).expect("known unit and valid amount");
        assert_eq!(parsed.qty, Qty(329_000));
        assert_eq!(parsed.unit.as_ref(), "CHF");
        assert!(matches!(parsed.unit, std::borrow::Cow::Borrowed(_)));
        assert!(original("CHF 3,290.001", &units).is_none());
        assert!(original("EUR 3,290.00", &units).is_none());
    }
}

/// Whether a record is one side of an exchange, and which.
#[derive(Clone, Copy)]
enum Exchange {
    No,
    /// The side that leaves, with the record that arrives.
    First(usize),
    /// The side that arrives: written with the first.
    Second,
}

/// The tail of a written line, in the order of the language: `#purpose of
/// THING`, `"description"`, `^codes`. A description is what the memo says, when
/// nobody is known to say it for.
fn tail(
    book: &Book<'_>,
    body: &mut String,
    other: &Other,
    describe: Option<&Record>,
    facts: &Facts<'_>,
) {
    if let Some((purpose, object)) = &other.purpose {
        let _ = write!(body, " #{}", book.name(*purpose));
        if let Some(object) = object {
            let _ = write!(body, " of {object}");
        }
    }
    if let Some(record) = describe {
        let memo = record.memo.split_whitespace().collect::<Vec<_>>().join(" ");
        let _ = write!(
            body,
            " \"{}\"",
            memo.replace('\\', "\\\\").replace('"', "\\\"")
        );
    }
    for &code in &other.codes {
        let _ = write!(body, " ^{code}");
    }
    if let Some(code) = facts
        .code
        .as_deref()
        .filter(|code| !other.codes.contains(code))
    {
        let _ = write!(body, " ^{code}");
    }
}

fn tie_error(book: &Book<'_>, record: &Record, tie: &Tie) -> Diagnostic {
    let (first, second) = (&tie.first, &tie.second);
    let headline = format!(
        "`{}` is known as both {} and {}",
        record.memo.trim(),
        first.name,
        second.name
    );
    let mut diagnostic = Diagnostic::error("ambiguous-memo", headline)
        .label(record.at, "this memo")
        .help("make one of the two more specific: the one that matches more of the memo wins");
    for (id, name) in [
        (tie.first_pattern, first.name),
        (tie.second_pattern, second.name),
    ] {
        if let Some(id) = id {
            diagnostic = diagnostic.context(
                book.patterns[id].loc,
                format!("this known-as pattern for `{name}` also matches"),
            );
        }
    }
    diagnostic
}

/// `01 flat`, or `08 phone 47.30 USD` when the record was for another amount.
fn occurrence<'a>(due: &Due, record: &Record, account: &'a str, unit: Unit<'a>) -> Line<'a> {
    let amount = match record.qty == due.qty {
        true => String::new(),
        false => format!(" {}", money(record.qty.abs(), unit)),
    };
    let moved = vec![(account, Some(unit.name), record.qty)];
    Line {
        day: record.day,
        body: format!("{}{amount}", due.contract),
        moved,
    }
}

/// `2_900 USD`, `84.20 USD`: a whole amount without decimals, any other to the
/// commodity's precision, in the language's own digits.
pub fn money(qty: Qty, unit: Unit) -> String {
    let quantum = POW10[unit.scale as usize];
    let digits = match i128::from(qty.0) % quantum {
        0 => Qty((i128::from(qty.0) / quantum) as i64).show(0),
        _ => qty.show(unit.scale),
    };
    format!("{} {}", digits.to_string().replace(',', "_"), unit.name)
}

/// The balance a statement ends on: that of the last day that has one, when
/// the day's posted records agree on it. Records can come in either order, so
/// the closing balance is the one that the day's opening balance plus
/// everything the day moved leads to. Records in another currency are not on
/// the account's own balance.
fn closing_of(records: &[Record]) -> Option<(Day, Qty)> {
    let own = |record: &&Record| !record.pending && record.facts().currency.is_none();
    let day = records
        .iter()
        .rev()
        .filter(own)
        .find(|record| record.balance.is_some())?
        .day;
    let today: Vec<(Qty, Qty)> = records
        .iter()
        .filter(own)
        .filter(|record| record.day == day)
        .filter_map(|record| Some((record.qty, record.balance?)))
        .collect();
    let moved: Qty = today.iter().map(|(qty, _)| *qty).sum();
    let mut closings: Vec<Qty> = today
        .iter()
        .map(|(qty, balance)| *balance - *qty + moved)
        .filter(|closing| today.iter().any(|(_, balance)| balance == closing))
        .collect();
    closings.sort();
    closings.dedup();
    (closings.len() == 1).then(|| (day, closings[0]))
}

#[cfg(test)]
mod output_tests {
    use super::*;
    use axiom_core::{FileId, Folder};
    use axiom_model::Source;
    use std::borrow::Cow;

    #[test]
    fn a_structured_code_is_written_even_when_a_party_suppresses_the_memo() {
        let std = include_str!("../../../systems/src/std.ax");
        let sources = [("std.ax", std, true), ("axiom.ax", "base USD\n", false)].map(
            |(path, text, embedded)| {
                let (file, problems) = axiom_syntax::parse(FileId(0), text, Folder::default());
                assert!(problems.is_empty(), "{path}: {problems:?}");
                Source { path, file, embedded }
            },
        );
        let (book, problems) = axiom_model::build(&sources);
        assert!(problems.is_empty(), "{problems:?}");
        let other = Other {
            who: Some(Who {
                id: crate::recognize::KnownId::Entity(axiom_core::Id::new(0)),
                name: "merchant",
                account: false,
            }),
            ..Other::default()
        };
        let facts = Facts {
            code: Some(Cow::Borrowed("statement-37")),
            ..Facts::default()
        };
        let mut body = String::new();
        tail(&book, &mut body, &other, None, &facts);
        assert_eq!(body, " ^statement-37");
    }
}

#[cfg(test)]
mod tests;
