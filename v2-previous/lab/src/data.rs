//! Four semantic-equivalent representations of the same generated ledger.
//! The baseline enum mirrors v2 Value layout but is isolated from production.
use crate::number::{self, Exact64, Exceptional, NumberRef};
use num_rational::BigRational;
use std::collections::BTreeMap;
use std::io::Write;
use std::num::NonZeroU32;

#[allow(dead_code)]
#[derive(Clone)]
pub enum Value {
    Number(BigRational),
    Quantity(BigRational, String),
    Date(String),
    Text(String),
    Bool(bool),
    Ref(String),
    Hole(String),
    List(Vec<Value>),
    Record(BTreeMap<String, Value>),
}
#[derive(Clone)]
pub struct Row {
    pub id: String,
    pub schema: String,
    pub fields: BTreeMap<String, Value>,
}
pub struct Baseline {
    pub rows: Vec<Row>,
    pub ids: BTreeMap<String, usize>,
}

pub const FIELD_COUNT: usize = 7;
const AMOUNT: usize = 0;
const DATE: usize = 1;
const MEMO: usize = 2;
const ACCOUNT: usize = 3;
const COUNTERPARTY: usize = 4;
const ACTIVE: usize = 5;
const REF: usize = 6;

pub fn sale(i: usize) -> bool {
    !i.is_multiple_of(4)
}
pub fn target(i: usize) -> usize {
    i / 4 * 4
}
pub fn day(i: usize) -> u32 {
    let day = i % 336;
    (day / 28 + 1) as u32 * 32 + (day % 28 + 1) as u32
}
pub fn active(i: usize) -> bool {
    !i.is_multiple_of(13)
}
pub fn unit(i: usize) -> usize {
    i % 3
}
fn account(i: usize) -> usize {
    i % 32
}
fn counterparty(i: usize) -> usize {
    i % 16
}
fn memo(i: usize) -> usize {
    i % 64
}
fn id(i: usize) -> String {
    format!("r{i:07}")
}

pub struct Dictionary {
    pub values: Vec<String>,
}
impl Dictionary {
    fn new() -> Self {
        let mut values = ["USD", "EUR", "GBP"]
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        values.extend((0..64).map(|i| format!("memo{i:02}: a repeated ledger description")));
        values.extend((0..32).map(|i| format!("account{i:02}")));
        values.extend((0..16).map(|i| format!("party{i:02}")));
        Self { values }
    }
    fn memo(&self, i: usize) -> &str {
        &self.values[3 + memo(i)]
    }
    fn account(&self, i: usize) -> &str {
        &self.values[67 + account(i)]
    }
    fn party(&self, i: usize) -> &str {
        &self.values[99 + counterparty(i)]
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Symbol(pub NonZeroU32);
impl Symbol {
    fn new(i: usize) -> Self {
        Self(NonZeroU32::new(u32::try_from(i + 1).unwrap()).unwrap())
    }
    fn index(self) -> usize {
        self.0.get() as usize - 1
    }
}
#[derive(Clone, Copy, Debug)]
pub struct NumberId(pub NonZeroU32);
impl NumberId {
    fn new(i: usize) -> Self {
        Self(NonZeroU32::new(u32::try_from(i + 1).unwrap()).unwrap())
    }
    fn index(self) -> usize {
        self.0.get() as usize - 1
    }
}
#[derive(Clone, Copy, Debug)]
pub struct RowId(pub NonZeroU32);
impl RowId {
    fn new(i: usize) -> Self {
        Self(NonZeroU32::new(u32::try_from(i + 1).unwrap()).unwrap())
    }
    fn index(self) -> usize {
        self.0.get() as usize - 1
    }
}
#[derive(Clone, Copy)]
pub enum Cell {
    Absent,
    Quantity(NumberId, Symbol),
    Date(u32),
    Text(Symbol),
    Bool(bool),
    Ref(RowId),
}
pub struct DenseRow {
    pub id: RowId,
    pub schema: u32,
    pub cells: [Cell; FIELD_COUNT],
}

/// External authored identity bytes are retained, never equated by value. The
/// generator supplies resolved reference handles; a real loader needs an ID
/// resolution index at ingestion, which can be dropped after lowering.
pub struct Names {
    bytes: Vec<u8>,
    ranges: Vec<(u32, u32)>,
}
impl Names {
    fn new(n: usize) -> Self {
        Self {
            bytes: Vec::with_capacity(n * 8),
            ranges: Vec::with_capacity(n),
        }
    }
    fn push(&mut self, i: usize) {
        let start = u32::try_from(self.bytes.len()).unwrap();
        // Format directly into the identity arena: no temporary String and
        // no per-occurrence heap allocation for compact layouts.
        write!(&mut self.bytes, "r{i:07}").unwrap();
        self.ranges.push((start, self.bytes.len() as u32 - start));
    }
    pub fn get(&self, i: usize) -> &str {
        let (start, len) = self.ranges[i];
        std::str::from_utf8(&self.bytes[start as usize..(start + len) as usize]).unwrap()
    }
}

pub trait NumberPool {
    fn with_capacity(n: usize) -> Self;
    fn push(&mut self, i: usize) -> NumberId;
    fn get(&self, id: NumberId) -> NumberRef<'_>;
}
pub struct BigPool(pub Vec<BigRational>);
impl NumberPool for BigPool {
    fn with_capacity(n: usize) -> Self {
        Self(Vec::with_capacity(n))
    }
    fn push(&mut self, i: usize) -> NumberId {
        let id = NumberId::new(self.0.len());
        self.0.push(number::rational(i));
        id
    }
    fn get(&self, id: NumberId) -> NumberRef<'_> {
        NumberRef::Big(&self.0[id.index()])
    }
}
pub struct CompactPool {
    pub values: Vec<Exact64>,
    pub exceptions: Vec<Exceptional>,
}
impl NumberPool for CompactPool {
    fn with_capacity(n: usize) -> Self {
        Self {
            values: Vec::with_capacity(n),
            exceptions: Vec::with_capacity(n / 200 + 1),
        }
    }
    fn push(&mut self, i: usize) -> NumberId {
        let id = NumberId::new(self.values.len());
        self.values.push(number::compact(i, &mut self.exceptions));
        id
    }
    fn get(&self, id: NumberId) -> NumberRef<'_> {
        NumberRef::Compact(self.values[id.index()], &self.exceptions)
    }
}
pub struct Dense<P> {
    pub rows: Vec<DenseRow>,
    pub names: Names,
    pub dictionary: Dictionary,
    pub numbers: P,
}

/// Schema-specialized storage is physical specialization only: package schema
/// compilation would generate these column kinds, never domain Rust enums.
pub struct Columns {
    ids: Vec<RowId>,
    amount: Vec<Exact64>,
    unit: Vec<Symbol>,
    dates: Vec<u32>,
    memo: Vec<Symbol>,
    accounts: Vec<Symbol>,
    parties: Vec<Symbol>,
    active: Vec<u64>,
    refs: Vec<RowId>,
}
#[derive(Clone, Copy)]
pub struct Location {
    pub sale: bool,
    pub local: u32,
}
pub struct Soa {
    buys: Columns,
    sales: Columns,
    locations: Vec<Location>,
    pub names: Names,
    dictionary: Dictionary,
    exceptions: Vec<Exceptional>,
}
impl Columns {
    fn new(n: usize, sales: bool) -> Self {
        Self {
            ids: Vec::with_capacity(n),
            amount: Vec::with_capacity(n),
            unit: Vec::with_capacity(n),
            dates: Vec::with_capacity(n),
            memo: Vec::with_capacity(n),
            accounts: Vec::with_capacity(n),
            parties: Vec::with_capacity(n),
            active: vec![0; n.div_ceil(64)],
            refs: Vec::with_capacity(if sales { n } else { 0 }),
        }
    }
    fn push(&mut self, i: usize, exceptions: &mut Vec<Exceptional>) {
        let local = self.ids.len();
        self.ids.push(RowId::new(i));
        self.amount.push(number::compact(i, exceptions));
        self.unit.push(Symbol::new(unit(i)));
        self.dates.push(day(i));
        self.memo.push(Symbol::new(3 + memo(i)));
        self.accounts.push(Symbol::new(67 + account(i)));
        self.parties.push(Symbol::new(99 + counterparty(i)));
        if active(i) {
            self.active[local / 64] |= 1 << (local % 64);
        }
        if sale(i) {
            self.refs.push(RowId::new(target(i)));
        }
    }
}

pub struct Fact<'a> {
    pub sale: bool,
    pub amount: NumberRef<'a>,
    pub unit: usize,
    pub date: u32,
    pub memo: &'a str,
    pub account: &'a str,
    pub party: &'a str,
    pub active: bool,
    pub reference: Option<usize>,
}
pub trait Ledger {
    fn len(&self) -> usize;
    fn fact(&self, i: usize) -> Fact<'_>;
    fn identity(&self, i: usize) -> &str;
}

impl Baseline {
    pub fn new(n: usize) -> Self {
        let dictionary = Dictionary::new();
        let mut rows = Vec::with_capacity(n);
        let mut ids = BTreeMap::new();
        for i in 0..n {
            let date = day(i);
            let mut fields = BTreeMap::from([
                (
                    "amount".to_owned(),
                    Value::Quantity(number::rational(i), dictionary.values[unit(i)].clone()),
                ),
                (
                    "date".to_owned(),
                    Value::Date(format!("2026-{:02}-{:02}", date / 32, date % 32)),
                ),
                (
                    "memo".to_owned(),
                    Value::Text(dictionary.memo(i).to_owned()),
                ),
                (
                    "account".to_owned(),
                    Value::Text(dictionary.account(i).to_owned()),
                ),
                (
                    "counterparty".to_owned(),
                    Value::Text(dictionary.party(i).to_owned()),
                ),
                ("active".to_owned(), Value::Bool(active(i))),
            ]);
            if sale(i) {
                fields.insert("purchase".to_owned(), Value::Ref(id(target(i))));
            }
            let occurrence = id(i);
            ids.insert(occurrence.clone(), i);
            rows.push(Row {
                id: occurrence,
                schema: if sale(i) { "sale" } else { "purchase" }.to_owned(),
                fields,
            });
        }
        Self { rows, ids }
    }
    pub fn relation(&self) -> Vec<Value> {
        self.rows
            .iter()
            .map(|row| {
                let mut fields = row.fields.clone();
                fields.insert("id".into(), Value::Ref(row.id.clone()));
                Value::Record(fields)
            })
            .collect()
    }
}
fn text(value: &Value) -> &str {
    match value {
        Value::Text(s) => s,
        _ => panic!("fixture text invariant"),
    }
}
impl Ledger for Baseline {
    fn len(&self) -> usize {
        self.rows.len()
    }
    fn identity(&self, i: usize) -> &str {
        &self.rows[i].id
    }
    fn fact(&self, i: usize) -> Fact<'_> {
        let row = &self.rows[i];
        let f = &row.fields;
        let (amount, unit) = match &f["amount"] {
            Value::Quantity(amount, unit) => (
                amount,
                match unit.as_str() {
                    "USD" => 0,
                    "EUR" => 1,
                    "GBP" => 2,
                    _ => unreachable!(),
                },
            ),
            _ => unreachable!(),
        };
        let date = match &f["date"] {
            Value::Date(s) => s.as_bytes(),
            _ => unreachable!(),
        };
        Fact {
            sale: row.schema == "sale",
            amount: NumberRef::Big(amount),
            unit,
            date: ((date[5] - b'0') as u32 * 10 + (date[6] - b'0') as u32) * 32
                + (date[8] - b'0') as u32 * 10
                + (date[9] - b'0') as u32,
            memo: text(&f["memo"]),
            account: text(&f["account"]),
            party: text(&f["counterparty"]),
            active: matches!(f["active"], Value::Bool(true)),
            reference: match f.get("purchase") {
                Some(Value::Ref(s)) => Some(self.ids[s]),
                _ => None,
            },
        }
    }
}

impl<P: NumberPool> Dense<P> {
    pub fn new(n: usize) -> Self {
        let mut rows = Vec::with_capacity(n);
        let mut names = Names::new(n);
        let dictionary = Dictionary::new();
        let mut numbers = P::with_capacity(n);
        for i in 0..n {
            names.push(i);
            rows.push(DenseRow {
                id: RowId::new(i),
                schema: sale(i) as u32,
                cells: [
                    Cell::Quantity(numbers.push(i), Symbol::new(unit(i))),
                    Cell::Date(day(i)),
                    Cell::Text(Symbol::new(3 + memo(i))),
                    Cell::Text(Symbol::new(67 + account(i))),
                    Cell::Text(Symbol::new(99 + counterparty(i))),
                    Cell::Bool(active(i)),
                    if sale(i) {
                        Cell::Ref(RowId::new(target(i)))
                    } else {
                        Cell::Absent
                    },
                ],
            });
        }
        Self {
            rows,
            names,
            dictionary,
            numbers,
        }
    }
}
impl<P: NumberPool> Ledger for Dense<P> {
    fn len(&self) -> usize {
        self.rows.len()
    }
    fn identity(&self, i: usize) -> &str {
        self.names.get(self.rows[i].id.index())
    }
    fn fact(&self, i: usize) -> Fact<'_> {
        let row = &self.rows[i];
        let (amount, unit) = match row.cells[AMOUNT] {
            Cell::Quantity(n, u) => (self.numbers.get(n), u.index()),
            _ => unreachable!(),
        };
        let symbol = |slot| match row.cells[slot] {
            Cell::Text(s) => self.dictionary.values[s.index()].as_str(),
            _ => unreachable!(),
        };
        Fact {
            sale: row.schema == 1,
            amount,
            unit,
            date: match row.cells[DATE] {
                Cell::Date(d) => d,
                _ => unreachable!(),
            },
            memo: symbol(MEMO),
            account: symbol(ACCOUNT),
            party: symbol(COUNTERPARTY),
            active: matches!(row.cells[ACTIVE], Cell::Bool(true)),
            reference: match row.cells[REF] {
                Cell::Ref(r) => Some(r.index()),
                _ => None,
            },
        }
    }
}
impl Soa {
    pub fn new(n: usize) -> Self {
        let nb = n.div_ceil(4);
        let mut buys = Columns::new(nb, false);
        let mut sales = Columns::new(n - nb, true);
        let mut locations = Vec::with_capacity(n);
        let mut names = Names::new(n);
        let mut exceptions = Vec::with_capacity(n / 200 + 1);
        for i in 0..n {
            names.push(i);
            let table = if sale(i) { &mut sales } else { &mut buys };
            locations.push(Location {
                sale: sale(i),
                local: table.ids.len() as u32,
            });
            table.push(i, &mut exceptions);
        }
        Self {
            buys,
            sales,
            locations,
            names,
            dictionary: Dictionary::new(),
            exceptions,
        }
    }
}
impl Ledger for Soa {
    fn len(&self) -> usize {
        self.locations.len()
    }
    fn identity(&self, i: usize) -> &str {
        self.names.get(i)
    }
    fn fact(&self, i: usize) -> Fact<'_> {
        let Location { sale, local } = self.locations[i];
        let j = local as usize;
        let table = if sale { &self.sales } else { &self.buys };
        Fact {
            sale,
            amount: NumberRef::Compact(table.amount[j], &self.exceptions),
            unit: table.unit[j].index(),
            date: table.dates[j],
            memo: &self.dictionary.values[table.memo[j].index()],
            account: &self.dictionary.values[table.accounts[j].index()],
            party: &self.dictionary.values[table.parties[j].index()],
            active: table.active[j / 64] & (1 << (j % 64)) != 0,
            reference: if sale {
                Some(table.refs[j].index())
            } else {
                None
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_field_and_identity_round_trips() {
        let n = 8192;
        let baseline = Baseline::new(n);
        let dense = Dense::<CompactPool>::new(n);
        let soa = Soa::new(n);
        for i in 0..n {
            let b = baseline.fact(i);
            for ledger in [&dense as &dyn Ledger, &soa as &dyn Ledger] {
                let c = ledger.fact(i);
                assert_eq!(baseline.identity(i), ledger.identity(i));
                assert_eq!(
                    (
                        b.sale,
                        b.unit,
                        b.date,
                        b.memo,
                        b.account,
                        b.party,
                        b.active,
                        b.reference
                    ),
                    (
                        c.sale,
                        c.unit,
                        c.date,
                        c.memo,
                        c.account,
                        c.party,
                        c.active,
                        c.reference
                    )
                );
                assert_eq!(b.amount.to_big(), c.amount.to_big());
            }
        }
    }
}
