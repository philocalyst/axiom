//! A statement's way into the journal (LANGUAGE §13): who each record is, which
//! are already written, which keep a promise, and what is left is written.

use std::fmt::Write;

use axiom_core::num::POW10;
use axiom_core::{Day, Diagnostic, FileId, Map, Qty};

use crate::csv::Csv;
use crate::promise::{Due, keep};
use crate::recognize::{Reading, Recognizer, Tie, Who};
use crate::reconcile::{Existing, reconcile};
use crate::write::Layout;
use crate::{Form, Insert, Record, Unit};

/// What the book says about one account.
#[derive(Default)]
pub struct Account<'a> {
    pub flows: Vec<Existing<'a>>,
    /// The days it has an assertion on.
    pub asserted: Vec<Day>,
}

impl Account<'_> {
    /// The day of the latest flow: what a sync of it has reached.
    pub fn last_day(&self) -> Option<Day> {
        self.flows.iter().map(|flow| flow.day).max()
    }
}

/// Everything the book knows that sync reads. Each feed adds what it writes, so
/// that a transfer both accounts show is written once.
pub struct World<'a> {
    pub recognizer: Recognizer<'a>,
    pub layout: Layout<'a>,
    pub accounts: Map<&'a str, Account<'a>>,
    /// Occurrences that are due and not written.
    pub dues: Vec<Due<'a>>,
    /// Open claims that carry a code: the code, and the party it is with.
    pub claims: Map<&'a str, &'a str>,
}

/// A source of records for one account.
pub struct Feed<'a> {
    pub account: &'a str,
    pub unit: Unit<'a>,
    pub csv: Csv,
}

/// A line to write, and what it moves.
struct Line<'a> {
    day: Day,
    body: String,
    /// The accounts it touches, and by how much, money into them positive.
    moved: Vec<(&'a str, Qty)>,
}

/// The other end of a record: whom it was with, through whom, and which open
/// claims it names.
struct Other<'a> {
    who: Option<Who<'a>>,
    via: Option<&'a str>,
    codes: Vec<&'a str>,
}

impl<'a> World<'a> {
    /// The inserts that bring the book up to a statement, or what is wrong with
    /// it. Nothing is changed unless all of it can be read.
    pub fn feed(&mut self, feed: &Feed<'a>, text: &str) -> Result<Vec<Insert>, Vec<Diagnostic>> {
        let (mut records, mut problems) = feed.csv.records(text, FileId(0), feed.unit);
        // Stable, so that a day's records keep the export's order.
        records.sort_by_key(|record| record.day);
        let readings = self.recognizer.read_all(&records);
        for (record, reading) in records.iter().zip(&readings) {
            problems.extend(reading.who.as_ref().err().map(|tie| tie_error(record, tie)));
        }
        if !problems.is_empty() {
            return Err(problems);
        }
        let (lines, asserted) = self.plan(feed, &records, &readings);
        for line in &lines {
            for &(name, qty) in &line.moved {
                self.accounts.entry(name).or_default().flows.push(Existing { day: line.day, qty, settle: None });
            }
        }
        if let Some(day) = asserted {
            self.accounts.entry(feed.account).or_default().asserted.push(day);
        }
        let insert = |line: Line| Insert { path: self.layout.file_for(line.day), day: line.day, form: Form::Item(line.body) };
        Ok(lines.into_iter().map(insert).collect())
    }

    /// The lines a statement adds, and the day it is asserted on, if it is.
    fn plan(&self, feed: &Feed<'a>, records: &[Record], readings: &[Reading<'a>]) -> (Vec<Line<'a>>, Option<Day>) {
        let account = self.accounts.get(feed.account);
        let flows = account.map_or(&[][..], |account| &account.flows);
        let matched = reconcile(records, flows);
        let others: Vec<Other> = readings.iter().map(|reading| self.other(reading)).collect();
        // Only a record that is neither written nor pending can keep a promise.
        let parties: Vec<Option<&str>> = (0..records.len())
            .map(|at| {
                let open = matched[at].is_none() && !records[at].pending;
                others[at].who.filter(|who| open && !who.account).map(|who| who.name)
            })
            .collect();
        let dues: Vec<Due> = self.dues.iter().filter(|due| due.account == feed.account).cloned().collect();
        let kept = keep(records, &parties, &dues);

        let mut lines = Vec::new();
        let mut pending_today: Map<Day, usize> = Map::default();
        for (at, record) in records.iter().enumerate().filter(|(_, record)| !record.qty.is_zero()) {
            if let Some(flow) = matched[at].map(|flow| &flows[flow]) {
                let settles = if record.pending { None } else { flow.settle };
                lines.extend(settles.map(|code| Line { day: record.day, body: format!("^{code} settled"), moved: vec![] }));
            } else if let Some(due) = kept[at].map(|due| &dues[due]) {
                let amount = if record.qty == due.qty { String::new() } else { format!(" {}", money(record.qty.abs(), feed.unit)) };
                lines.push(Line { day: record.day, body: format!("{}{amount}", due.contract), moved: vec![(feed.account, record.qty)] });
            } else {
                // Pending flows carry a code of their own, for the record that posts them to settle.
                let code = record.pending.then(|| {
                    let so_far = pending_today.entry(record.day).or_default();
                    *so_far += 1;
                    let earlier = flows.iter().filter(|flow| flow.day == record.day).count();
                    format!("pending-{}-{}", record.day.to_string().replace('-', ""), earlier + *so_far)
                });
                let other = &others[at];
                let end = other.who.filter(|who| who.account).map(|who| (who.name, -record.qty));
                let moved = [Some((feed.account, record.qty)), end].into_iter().flatten().collect();
                lines.push(Line { day: record.day, body: flow_text(feed, record, other, code.as_deref()), moved });
            }
        }
        let closing = closing(records).filter(|(day, _)| account.is_none_or(|account| !account.asserted.contains(day)));
        if let Some((day, balance)) = closing {
            let shown = if balance.is_negative() { format!("-{}", money(balance.abs(), feed.unit)) } else { money(balance, feed.unit) };
            lines.push(Line { day, body: format!("{} = {shown}", feed.account), moved: vec![] });
        }
        (lines, closing.map(|(day, _)| day))
    }

    /// Who a record was with: the memo's party, else the party of the claim
    /// whose code it names. Only codes of the claims of that party are carried.
    fn other(&self, reading: &Reading<'a>) -> Other<'a> {
        let found = reading.who.unwrap_or_default();
        let mut other = Other { who: found.who, via: found.via, codes: Vec::new() };
        for code in &reading.codes {
            let Some((&claim, &party)) = self.claims.get_key_value(code.as_str()) else { continue };
            match other.who {
                None => other.who = Some(Who { name: party, account: false }),
                Some(who) if who.name != party => continue,
                Some(_) => {}
            }
            other.codes.push(claim);
        }
        other
    }
}

fn tie_error(record: &Record, tie: &Tie) -> Diagnostic {
    let ((first, first_pattern), (second, second_pattern)) = (tie.first, tie.second);
    let headline = format!("`{}` is known as both {} and {}", record.memo.trim(), first.name, second.name);
    Diagnostic::error("ambiguous-memo", headline)
        .label(record.at, "this memo")
        .note(format!("`known-as \"{first_pattern}\"` of {} and `known-as \"{second_pattern}\"` of {} match it equally well", first.name, second.name))
        .help("make one of the two more specific: the longer match wins")
}

/// `checking -> trader-joes 84.20 USD`: the flow a record is, with the other
/// end it was with, or `?` and its memo as a description if nobody is known.
fn flow_text(feed: &Feed, record: &Record, other: &Other, pending: Option<&str>) -> String {
    let end = other.who.map_or("?", |who| who.name);
    let (from, to) = if record.qty.is_negative() { (feed.account, end) } else { (end, feed.account) };
    let amount = money(record.qty.abs(), feed.unit);
    let mut text = format!("{from} -> {to} {}", if record.pending { format!("({amount})") } else { amount });
    if let Some(via) = other.via {
        let _ = write!(text, " via {via}");
    }
    for code in other.codes.iter().copied().chain(pending) {
        let _ = write!(text, " ^{code}");
    }
    if other.who.is_none() {
        let memo = record.memo.split_whitespace().collect::<Vec<_>>().join(" ");
        let _ = write!(text, " \"{}\"", memo.replace('\\', "\\\\").replace('"', "\\\""));
    }
    text
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
/// everything the day moved leads to.
fn closing(records: &[Record]) -> Option<(Day, Qty)> {
    let day = records.iter().rev().find(|record| !record.pending && record.balance.is_some())?.day;
    let today: Vec<(Qty, Qty)> = records
        .iter()
        .filter(|record| record.day == day && !record.pending)
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
