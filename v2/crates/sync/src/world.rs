//! A statement's way into the journal (LANGUAGE §13): which records are already
//! written, who the rest are, which keep a promise, and what is left is written.

use std::fmt::Write;

use axiom_core::num::POW10;
use axiom_core::{Day, Diagnostic, FileId, Map, Qty};

use crate::promise::{Due, keep};
use crate::recognize::{Reading, Recognizer, Tie, Who};
use crate::reconcile::{Existing, reconcile};
use crate::statement::{Format, Statement};
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
    pub format: Format,
}

/// A line to write, and what it moves.
struct Line<'a> {
    day: Day,
    body: String,
    /// The accounts it touches, and by how much, money into them positive.
    moved: Vec<(&'a str, Qty)>,
}

impl<'a> Line<'a> {
    fn statement(day: Day, body: String) -> Line<'a> {
        Line { day, body, moved: Vec::new() }
    }
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
        let (Statement { mut records, closing }, problems) = feed.format.read(text, FileId(0), feed.unit);
        if !problems.is_empty() {
            return Err(problems);
        }
        // Stable, so that a day's records keep the export's order.
        records.sort_by_key(|record| record.day);
        let (lines, asserted) = self.plan(feed, &records, closing)?;
        for line in &lines {
            for &(name, qty) in &line.moved {
                self.accounts.entry(name).or_default().flows.push(Existing { day: line.day, qty, settle: None });
            }
        }
        if let Some(day) = asserted {
            self.accounts.entry(feed.account).or_default().asserted.push(day);
        }
        let insert =
            |line: Line| Insert { path: self.layout.file_for(line.day), day: line.day, form: Form::Item(line.body) };
        Ok(lines.into_iter().map(insert).collect())
    }

    /// The lines a statement adds, and the day it is asserted on, if it is.
    fn plan(
        &self,
        feed: &Feed<'a>,
        records: &[Record],
        closing: Option<(Day, Qty)>,
    ) -> Result<(Vec<Line<'a>>, Option<Day>), Vec<Diagnostic>> {
        let account = self.accounts.get(feed.account);
        let flows = account.map_or(&[][..], |account| &account.flows);
        let matched = reconcile(records, flows);
        let others = self.others(records, &matched)?;
        // Only a record that is neither written nor pending can keep a promise.
        let parties: Vec<Option<&str>> = (0..records.len())
            .map(|at| {
                let who = others[at].as_ref().filter(|_| !records[at].pending).and_then(|other| other.who);
                who.filter(|who| !who.account).map(|who| who.name)
            })
            .collect();
        let dues: Vec<Due> = self.dues.iter().filter(|due| due.account == feed.account).cloned().collect();
        let kept = keep(records, &parties, &dues);

        // Pending flows carry a code of their own, for the record that posts
        // them to settle. It counts on from the flows the account has that day.
        let mut per_day: Map<Day, usize> = Map::default();
        if records.iter().any(|record| record.pending) {
            flows.iter().for_each(|flow| *per_day.entry(flow.day).or_default() += 1);
        }
        let mut pending_code = |day: Day| {
            let number = per_day.entry(day).or_default();
            *number += 1;
            format!("pending-{}-{number}", day.to_string().replace('-', ""))
        };
        let mut lines = Vec::new();
        for (at, record) in records.iter().enumerate().filter(|(_, record)| !record.qty.is_zero()) {
            let line = match (matched[at], kept[at], &others[at]) {
                (Some(flow), _, _) => {
                    let settles = flows[flow].settle.filter(|_| !record.pending);
                    settles.map(|code| Line::statement(record.day, format!("^{code} settled")))
                }
                (None, Some(due), _) => Some(occurrence(&dues[due], record, feed)),
                (None, None, Some(other)) => {
                    let code = record.pending.then(|| pending_code(record.day));
                    Some(new_flow(feed, record, other, code.as_deref()))
                }
                (None, None, None) => None,
            };
            lines.extend(line);
        }
        let closing = closing.or_else(|| closing_of(records));
        let closing = closing.filter(|(day, _)| account.is_none_or(|account| !account.asserted.contains(day)));
        if let Some((day, balance)) = closing {
            let shown = match balance.is_negative() {
                true => format!("-{}", money(balance.abs(), feed.unit)),
                false => money(balance, feed.unit),
            };
            lines.push(Line::statement(day, format!("{} = {shown}", feed.account)));
        }
        Ok((lines, closing.map(|(day, _)| day)))
    }

    /// Who each record that is not yet written was with; ties are errors. A
    /// written record, or one that moves nothing, is not read at all.
    fn others(&self, records: &[Record], matched: &[Option<usize>]) -> Result<Vec<Option<Other<'a>>>, Vec<Diagnostic>> {
        let open: Vec<usize> =
            (0..records.len()).filter(|&at| matched[at].is_none() && !records[at].qty.is_zero()).collect();
        let readings = self.recognizer.read_all(&open.iter().map(|&at| &records[at]).collect::<Vec<_>>());
        let ties: Vec<Diagnostic> = open
            .iter()
            .zip(&readings)
            .filter_map(|(&at, reading)| reading.who.as_ref().err().map(|tie| tie_error(&records[at], tie)))
            .collect();
        if !ties.is_empty() {
            return Err(ties);
        }
        let mut others: Vec<Option<Other<'a>>> = records.iter().map(|_| None).collect();
        for (&at, reading) in open.iter().zip(&readings) {
            others[at] = Some(self.other(reading));
        }
        Ok(others)
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
    let note = format!(
        "`known-as {first_pattern}` of {} and `known-as {second_pattern}` of {} match it equally well",
        first.name, second.name
    );
    Diagnostic::error("ambiguous-memo", headline)
        .label(record.at, "this memo")
        .note(note)
        .help("make one of the two more specific: the one that matches more of the memo wins")
}

/// `01 flat`, or `08 phone 47.30 USD` when the record was for another amount.
fn occurrence<'a>(due: &Due, record: &Record, feed: &Feed<'a>) -> Line<'a> {
    let amount = match record.qty == due.qty {
        true => String::new(),
        false => format!(" {}", money(record.qty.abs(), feed.unit)),
    };
    let moved = vec![(feed.account, record.qty)];
    Line { day: record.day, body: format!("{}{amount}", due.contract), moved }
}

/// `checking -> trader-joes 84.20 USD`: the flow a record is, with the other
/// end it was with, or `?` and its memo as a description if nobody is known.
fn new_flow<'a>(feed: &Feed<'a>, record: &Record, other: &Other<'a>, pending: Option<&str>) -> Line<'a> {
    let end = other.who.map_or("?", |who| who.name);
    let (from, to) = if record.qty.is_negative() { (feed.account, end) } else { (end, feed.account) };
    let amount = money(record.qty.abs(), feed.unit);
    let mut body = format!("{from} -> {to} {}", if record.pending { format!("({amount})") } else { amount });
    if let Some(via) = other.via {
        let _ = write!(body, " via {via}");
    }
    for code in other.codes.iter().copied().chain(pending) {
        let _ = write!(body, " ^{code}");
    }
    if other.who.is_none() {
        let memo = record.memo.split_whitespace().collect::<Vec<_>>().join(" ");
        let _ = write!(body, " \"{}\"", memo.replace('\\', "\\\\").replace('"', "\\\""));
    }
    // What arrives at the other end, if that is an account of the book too.
    let transfer = other.who.filter(|who| who.account).map(|who| (who.name, -record.qty));
    let moved = [Some((feed.account, record.qty)), transfer].into_iter().flatten().collect();
    Line { day: record.day, body, moved }
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
fn closing_of(records: &[Record]) -> Option<(Day, Qty)> {
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

#[cfg(test)]
mod tests {
    use crate::csv::{Amounts, Column, Csv, DateFormat};
    use crate::recognize::Known;

    use super::*;

    const USD: Unit = Unit { name: "USD", scale: 2 };

    fn day(text: &str) -> Day {
        Day::parse(text.as_bytes()).unwrap()
    }

    /// Rows are `date,amount,memo,balance,pending`, with no header.
    fn feed() -> Feed<'static> {
        let at = Column::Index;
        let csv = Csv {
            date: at(1),
            format: DateFormat::new("YYYY-MM-DD").unwrap(),
            amount: Amounts::Signed { column: at(2), flipped: false },
            memo: at(3),
            balance: Some(at(4)),
            pending: Some(at(5)),
        };
        Feed { account: "checking", unit: USD, format: Format::Csv(csv) }
    }

    fn world(known: Vec<Known<'static>>) -> World<'static> {
        let codes = ["code:(\"inv-\" digit+ \"-\" digit+)"];
        World {
            recognizer: Recognizer::new(known, &codes).unwrap(),
            layout: Layout::new(["journal/2026/01.ax"]),
            accounts: Map::default(),
            dues: Vec::new(),
            claims: Map::default(),
        }
    }

    fn party(name: &'static str, pattern: &'static str) -> Known<'static> {
        Known { name, account: false, patterns: vec![pattern] }
    }

    /// The lines a statement adds, dated as they are written.
    fn written(world: &mut World<'static>, text: &str) -> Vec<String> {
        let inserts = world.feed(&feed(), text).unwrap_or_else(|problems| panic!("{}", problems[0].message));
        inserts
            .iter()
            .map(|insert| match &insert.form {
                Form::Item(body) => format!("{} {body}", insert.day.to_string().split_at(8).1),
                Form::Row { .. } => unreachable!("a statement adds journal lines"),
            })
            .collect()
    }

    #[test]
    fn a_pending_record_is_written_in_parentheses_and_settled_when_it_posts() {
        let mut world = world(vec![]);
        let pending = "2026-01-05,-12.50,CORNER STORE,,pending\n2026-01-05,-3.00,COFFEE,,pending\n";
        assert_eq!(
            written(&mut world, pending),
            [
                "05 checking -> ? (12.50 USD) ^pending-20260105-1 \"CORNER STORE\"",
                "05 checking -> ? (3 USD) ^pending-20260105-2 \"COFFEE\"",
            ]
        );
        // The book now has the flow, still pending, with its code; the bank posts it a day later.
        let mut world = self::world(vec![]);
        let account = world.accounts.entry("checking").or_default();
        account.flows.push(Existing { day: day("2026-01-05"), qty: Qty(-1250), settle: Some("pending-20260105-1") });
        account.flows.push(Existing { day: day("2026-01-05"), qty: Qty(-300), settle: None });
        let posted = "2026-01-06,-12.50,CORNER STORE 1234,,\n2026-01-06,-3.00,COFFEE,,\n";
        assert_eq!(
            written(&mut world, posted),
            ["06 ^pending-20260105-1 settled"],
            "the flow with no code has nothing to say"
        );
    }

    #[test]
    fn an_invoice_code_finds_its_party_and_only_its_partys_codes_are_carried() {
        let mut world = world(vec![party("halcyon", "\"HALCYON\"")]);
        world.claims.insert("inv-2026-01", "halcyon");
        world.claims.insert("inv-2026-09", "northwind");
        let text = "2026-01-08,3800.00,WIRE FROM SOMEONE PAYING INV-2026-01,,\n\
                    2026-01-09,100.00,HALCYON RE INV-2026-09,,\n\
                    2026-01-10,5.00,INV-2026-77 UNKNOWN,,\n";
        assert_eq!(
            written(&mut world, text),
            [
                "08 halcyon -> checking 3_800 USD ^inv-2026-01",
                "09 halcyon -> checking 100 USD",
                "10 ? -> checking 5 USD \"INV-2026-77 UNKNOWN\"",
            ]
        );
    }

    #[test]
    fn an_occurrence_that_differs_says_so_and_a_pending_one_waits() {
        let mut world = world(vec![party("mint", "\"MINT MOBILE\"")]);
        let due = |on: &str| Due {
            contract: "phone",
            party: "mint",
            account: "checking",
            day: day(on),
            qty: Qty(-4500),
            window: 15,
        };
        world.dues = vec![due("2026-01-08"), due("2026-02-08")];
        let text = "2026-01-09,-47.30,MINT MOBILE,,\n2026-02-08,-45.00,MINT MOBILE,,pending\n";
        assert_eq!(
            written(&mut world, text),
            ["09 phone 47.30 USD", "08 checking -> mint (45 USD) ^pending-20260208-1"]
        );
        let mut world = self::world(vec![party("mint", "\"MINT MOBILE\"")]);
        world.dues = vec![due("2026-01-08")];
        assert_eq!(written(&mut world, "2026-01-08,-45.00,MINT MOBILE,,\n"), ["08 phone"]);
    }

    #[test]
    fn a_statement_ends_in_an_assertion_however_its_days_are_ordered() {
        let asserted = |text: &str| {
            written(&mut world(vec![]), text).into_iter().filter(|line| line.contains(" = ")).collect::<Vec<_>>()
        };
        let oldest_first = "2026-01-05,-10.00,A,90.00,\n2026-01-06,-5.00,B,85.00,\n2026-01-06,-1.00,C,84.00,\n";
        let newest_first = "2026-01-06,-1.00,C,84.00,\n2026-01-06,-5.00,B,85.00,\n2026-01-05,-10.00,A,90.00,\n";
        assert_eq!(asserted(oldest_first), ["06 checking = 84 USD"]);
        assert_eq!(asserted(newest_first), ["06 checking = 84 USD"]);
        assert_eq!(asserted("2026-01-06,-1.00,C,-84.00,\n"), ["06 checking = -84 USD"]);
        assert!(
            asserted("2026-01-06,-5.00,B,85.00,\n2026-01-06,-1.00,C,50.00,\n").is_empty(),
            "balances that do not add up are no assertion"
        );
        assert!(asserted("2026-01-06,-5.00,B,,\n").is_empty());
        let mut world = world(vec![]);
        world.accounts.entry("checking").or_default().asserted.push(day("2026-01-06"));
        assert!(written(&mut world, oldest_first).iter().all(|line| !line.contains(" = ")), "one assertion to a day");
    }

    #[test]
    fn a_memo_nobody_is_known_as_is_a_description_that_reads_back() {
        let mut world = world(vec![]);
        let lines = written(
            &mut world,
            "2026-01-05,-9.99,\"  SQ   *CAFE \"\"LUNA\"\" \\ ETC \",,\n2026-01-05,0.00,NOTHING MOVED,,\n",
        );
        assert_eq!(lines, ["05 checking -> ? 9.99 USD \"SQ *CAFE \\\"LUNA\\\" \\\\ ETC\""]);
    }

    #[test]
    fn two_memos_that_tie_are_an_error_naming_both_and_nothing_is_written() {
        let mut world = world(vec![party("shell-oil", "\"SHELL\""), party("shell-station", "\"SHELL\" any*")]);
        let problems = world.feed(&feed(), "2026-01-05,-9.99,SHELL 1234,,\n").err().expect("refused");
        assert_eq!(problems[0].message, "`SHELL 1234` is known as both shell-oil and shell-station");
        assert!(problems[0].help[0].text.contains("matches more of the memo"));
        assert!(world.accounts.is_empty(), "nothing is remembered from a source that failed");
    }

    #[test]
    fn a_record_already_written_is_not_read_again() {
        let mut world = world(vec![party("shell-oil", "\"SHELL\""), party("shell-station", "\"SHELL\" any*")]);
        let account = world.accounts.entry("checking").or_default();
        account.flows.push(Existing { day: day("2026-01-05"), qty: Qty(-999), settle: None });
        assert!(
            written(&mut world, "2026-01-05,-9.99,SHELL 1234,,\n").is_empty(),
            "a tie in what is written is not in the way"
        );
    }

    #[test]
    #[cfg_attr(debug_assertions, ignore = "timings are for release builds")]
    fn a_statement_of_a_hundred_thousand_records_against_an_account_of_a_million_flows() {
        let mut seed = 11u64;
        let mut next = |bound: u64| {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            (seed >> 33) % bound
        };
        let names: Vec<String> = (0..100).map(|n| format!("merchant-{n}")).collect();
        let patterns: Vec<String> = (0..100).map(|n| format!("\"SHOP {n:03}\"")).collect();
        let known = names.iter().zip(&patterns).map(|(name, pattern)| Known {
            name: Box::leak(name.clone().into_boxed_str()),
            account: false,
            patterns: vec![Box::leak(pattern.clone().into_boxed_str())],
        });
        let mut world = world(known.collect());
        let first = day("2016-01-01");
        let account = world.accounts.entry("checking").or_default();
        account.flows = (0..1_000_000)
            .map(|_| Existing {
                day: first.add_days(next(3650) as i32),
                qty: Qty(-(next(50_000) as i64) - 1),
                settle: None,
            })
            .collect();
        let mut text = String::new();
        for at in 0..100_000 {
            // A fifth are on the book already, a few days off; the rest are new.
            let (day, cents) = match at % 5 {
                0 => {
                    let known = account.flows[next(1_000_000) as usize];
                    (known.day.add_days(next(3) as i32), known.qty.0)
                }
                _ => (first.add_days(next(3650) as i32), -(next(50_000) as i64) - 1),
            };
            let (whole, fraction) = (cents.abs() / 100, cents.abs() % 100);
            text += &format!("{day},-{whole}.{fraction:02},POS PURCHASE SHOP {:03} SAN FRANCISCO,,\n", next(120));
        }
        let started = std::time::Instant::now();
        let lines = world.feed(&feed(), &text).unwrap_or_else(|problems| panic!("{}", problems[0].message));
        eprintln!(
            "a statement of 100,000 records against 1,000,000 flows: {} lines in {:?}",
            lines.len(),
            started.elapsed()
        );
        assert!(started.elapsed().as_millis() < 1000, "{:?}", started.elapsed());
        assert!(lines.len() > 70_000 && lines.len() < 85_000, "{}", lines.len());
    }
}
