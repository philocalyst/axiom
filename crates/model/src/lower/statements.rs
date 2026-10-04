//! Statements that say something on a day without being a flow: a value, a measure, a return filed, a split, an
//! event, an ending, a waiver and an asset's basis. Each reads one dated line into its own record of the Book.

use axiom_core::{Day, Days, Dec, Diagnostic, Dim, Id, Loc, Map, Qty, Ratio, Run, Sym};
use axiom_syntax as ast;
use axiom_syntax::{ClauseKind, Quantity, Subject};

use super::flow::{
    Codes, Ends, FlowCx, ResolvedEnd, Shape, empty_codes, keep_program, make_resolved_flow, push_flow_expressions,
    push_tail_roots,
};
use super::record::CodeIndex;
use super::staged::Staged;
use super::tail::{Line, Tail, read_tail};
use crate::args::Args;
use crate::book::{
    Amount, Asset, Change as BookChange, Commodity, Contract, Entity, EventState, Place, RateChange, Role,
};
use crate::builtin;
use crate::declare::World;
use crate::errors::{Reported, Word};
use crate::holders::Holder;
use crate::journal::{
    Action, Assert, ClaimChange, ClaimChangeAction, EndEvent, EndTarget, Event, Filed, Flow, Gap, Infer, Measure, Mode,
    Program, Quote, Reading, Split, Txn, Waive,
};
use crate::law::{NodeId, Subject as ModelSubject, Ty};
use crate::problem::{self, CodeUse};
use crate::scope::Home;
use crate::sources::Site;

/// Where a statement is written.
#[derive(Clone, Copy)]
pub(super) enum Within {
    /// On a line of its own in the journal.
    Journal,
    /// Under an `opening`, as a claim it opens with.
    Opening,
}

/// A dated statement being lowered, and all its lowering needs: the book it goes into (which says what is wrong),
/// where it is written, what it says and the codes the journal has so far. It is the context of that one pass, made
/// for the statement and dropped with it, so every function of the pass takes it first.
pub(super) struct Stated<'c, 'a, 's> {
    pub world: &'c mut World<'s>,
    pub site: &'c Site<'a, 's>,
    pub statement: &'c ast::Statement<'s>,
    pub loc: Loc,
    pub within: Within,
    pub code_index: &'c CodeIndex,
}

impl<'a, 's> Stated<'_, 'a, 's> {
    pub fn file(&self) -> &'a ast::File<'s> {
        &self.site.source.file
    }

    pub fn home(&self) -> Home {
        self.site.home
    }

    /// A statement the Book has no lowering for, said.
    pub fn unsupported(&mut self, message: &str) {
        let said = Diagnostic::error("statement-lowering", message);
        self.world.diags.push(said.label(self.loc, "this record is not included in the Book yet"));
    }

    /// Whether lines are indented under the statement.
    fn has_lines(&self) -> bool {
        let body = &self.statement.body;
        !body.legs.is_empty() || !body.items.is_empty()
    }
}

/// `CODE opened` and its like: a promise's life, in the order it happened.
pub(super) fn lower_event(at: &mut Stated<'_, '_, '_>, state: EventState) {
    let Subject::Code(code) = at.statement.subject else {
        at.unsupported("events need a code subject");
        return;
    };
    let code = at.world.book.names.intern(code.name());
    at.world.book.events.push(Event { day: at.statement.date, code, state, loc: at.loc });
}

/// `VTI 2 for 1`: a commodity splits.
pub(super) fn lower_split(at: &mut Stated<'_, '_, '_>, numerator: Dec, denominator: Dec) {
    let Subject::Unit(unit) = at.statement.subject else {
        at.unsupported("a split needs a commodity subject");
        return;
    };
    let Some(unit) = at.world.commodity_of(Word::of(at.file(), unit.0)).or_report(at.world) else {
        return;
    };
    let Some(ratio) = numerator
        .to_ratio()
        .zip(denominator.to_ratio())
        .and_then(|(numerator, denominator)| numerator.checked_div(denominator))
        .filter(|ratio| *ratio > Ratio::ZERO)
    else {
        at.world.diags.push(
            Diagnostic::error("split-ratio", "a split ratio must be greater than zero")
                .label(at.loc, "the written ratio cannot be represented"),
        );
        return;
    };
    at.world.book.splits.push(Split { day: at.statement.date, unit, ratio, loc: at.loc });
}

/// `filed us 2025` and the tally under it: what a return said, line by line.
pub(super) fn lower_filed(at: &mut Stated<'_, '_, '_>, year: i32) {
    let (file, statement, loc) = (at.file(), at.statement, at.loc);
    let Subject::Name(system_name) = statement.subject else {
        at.unsupported("a return needs a system subject");
        return;
    };
    let Some(system) = at.world.system(Word::of(file, system_name.0)).or_report(at.world) else {
        return;
    };
    let fallback = at.world.book.systems[system].currency.unwrap_or(at.world.book.base);
    let start = at.world.diags.len();
    let mut lines = Vec::with_capacity(statement.body.legs.len());
    for line in &file[statement.body.legs] {
        let Quantity::Amount(ast::Amount::Literal(literal)) = line.amount else {
            at.world.diags.push(
                Diagnostic::error("filed-amount", "a filed tally needs a literal amount")
                    .label(line.loc, "write the amount as reported"),
            );
            continue;
        };
        let Some(amount) = at.world.literal_amount(file, literal, Some(fallback)).or_report(at.world) else {
            continue;
        };
        lines.push((at.world.book.names.intern(line.end.name.0), amount, line.loc));
    }
    if at.world.diags.len() != start || lines.len() != statement.body.legs.len() {
        return;
    }
    let filed = Filed {
        day: statement.date,
        system,
        year,
        owner: at.world.book.roots.me,
        lines: lines.into_boxed_slice(),
        loc,
    };
    at.world.book.filed.push(filed);
}

// ─── Values, readings and quotes ────────────────────────────────────────────

/// What a statement's subject is, once its name is looked up.
#[derive(Clone, Copy)]
enum StatementTarget {
    Place(Id<Place>),
    Entity(Id<Entity>),
    Asset(Id<Asset>),
    Unit(Id<Commodity>),
    Code(Sym),
    Purpose,
}

fn statement_target(at: &mut Stated<'_, '_, '_>) -> Option<StatementTarget> {
    let (home, file) = (at.home(), at.file());
    match at.statement.subject {
        Subject::Name(name) => named_target(at.world, home, Word::of(file, name.0), at.statement.date),
        Subject::Code(code) => Some(StatementTarget::Code(at.world.book.names.intern(code.name()))),
        Subject::Purpose(name) => {
            at.world.purpose(home, Word::of(file, name.0)).map(|_| StatementTarget::Purpose).or_report(at.world)
        }
        Subject::Unit(name) => {
            at.world.commodity_of(Word::of(file, name.0)).map(StatementTarget::Unit).or_report(at.world)
        }
    }
}

/// What a plain name is: an asset, a loan's debt, a party, or a place.
fn named_target(world: &mut World<'_>, home: Home, word: Word<'_>, day: Day) -> Option<StatementTarget> {
    if let Some(asset) = world.book.asset(word.text) {
        return Some(StatementTarget::Asset(asset));
    }
    if let Some(contract) = world.book.contract(word.text)
        && let Some(loan) = world.book.contracts[contract].loan
    {
        // A loan contract's name denotes its debt position in a balance assertion, not the lender entity that
        // resolves as its ordinary flow endpoint.
        return Some(StatementTarget::Place(loan.debt));
    }
    let end = world.end_on(home, word, Some(day)).or_report(world)?;
    Some(match (end.entity, world.book.places[end.place].role) {
        (Some(entity), _) => StatementTarget::Entity(entity),
        (None, Role::Asset(asset)) => StatementTarget::Asset(asset),
        (None, _) => StatementTarget::Place(end.place),
    })
}

/// `checking = 4_000 USD`, `balance of ASSET = …`, `^code = …`, `VTI = 285.70 USD`: what is worth what.
pub(super) fn lower_value<'s>(at: &mut Stated<'_, '_, 's>, value: ast::Amount<'s>) {
    let Some(target) = statement_target(at) else {
        return;
    };
    match target {
        StatementTarget::Place(place) => {
            let fallback = at.world.book.holds_only(place).unwrap_or(at.world.book.base);
            assert_value(at, value, Asserted { place, subject: ModelSubject::Place(place), fallback });
        }
        StatementTarget::Asset(asset) => {
            let (place, fallback) = (at.world.book.assets[asset].place, at.world.book.base);
            assert_value(at, value, Asserted { place, subject: ModelSubject::Asset(asset), fallback });
        }
        StatementTarget::Code(code) => lower_reading(at, value, code),
        StatementTarget::Unit(unit) => lower_quote(at, value, unit),
        StatementTarget::Entity(_) | StatementTarget::Purpose => {
            at.unsupported("a value needs an account, asset, code or commodity subject")
        }
    }
}

/// What a balance assertion is about.
struct Asserted {
    place: Id<Place>,
    subject: ModelSubject,
    /// The unit of a literal that names none.
    fallback: Id<Commodity>,
}

/// A place or asset is worth this much on this day, and what the gap is if it is not.
fn assert_value<'s>(at: &mut Stated<'_, '_, 's>, value: ast::Amount<'s>, asserted: Asserted) {
    let Some(gap) = assertion_gap(at) else {
        return;
    };
    let ty = match asserted.subject {
        ModelSubject::Asset(_) => Ty::Asset,
        _ => Ty::Place,
    };
    let Some((amount, computed)) = assertion_amount(at, value, asserted.fallback, ty) else {
        return;
    };
    let (day, loc) = (at.statement.date, at.loc);
    let assert = Assert { day, place: asserted.place, subject: asserted.subject, amount, computed, gap, loc };
    at.world.book.asserts.push(assert);
}

/// `^code = 12 USD`: a reading of a named measure.
fn lower_reading<'s>(at: &mut Stated<'_, '_, 's>, value: ast::Amount<'s>, code: Sym) {
    let ast::Amount::Literal(literal) = value else {
        unsupported_computed_value(at.loc, "a named measure reading", &mut at.world.diags);
        return;
    };
    let Some(amount) = at.world.literal_amount(at.file(), literal, Some(at.world.book.base)).or_report(at.world) else {
        return;
    };
    at.world.book.readings.push(Reading { day: at.statement.date, code, amount, loc: at.loc });
}

/// `VTI = 285.70 USD`: what one of a commodity was worth in another.
fn lower_quote<'s>(at: &mut Stated<'_, '_, 's>, value: ast::Amount<'s>, unit: Id<Commodity>) {
    let ast::Amount::Literal(literal) = value else {
        unsupported_computed_value(at.loc, "a price quote", &mut at.world.diags);
        return;
    };
    let Some(quote_name) = literal.unit() else {
        at.world.diags.push(
            Diagnostic::error("price-unit", "a price needs a quoted commodity")
                .label(at.loc, "write `VTI = 285.70 USD`"),
        );
        return;
    };
    let Some(quote) = at.world.commodity_of(Word::of(at.file(), quote_name.0)).or_report(at.world) else {
        return;
    };
    let Some(rate) = literal.num().to_ratio().filter(|rate| *rate > Ratio::ZERO) else {
        at.world.diags.push(
            Diagnostic::error("price-zero", "a price must be greater than zero")
                .label(at.file().loc(literal.0), "this price is not positive"),
        );
        return;
    };
    let quote = Quote { unit, quote, day: at.statement.date, rate, implied: false, loc: at.loc };
    at.world.book.prices.quotes.push(quote);
}

/// What an assertion's amount is: written, or computed by a node of a program of its own.
type Computed = Option<(Id<Program>, NodeId)>;

fn assertion_amount<'s>(
    at: &mut Stated<'_, '_, 's>,
    value: ast::Amount<'s>,
    fallback: Id<Commodity>,
    subject: Ty,
) -> Option<(Amount, Computed)> {
    let (home, file) = (at.home(), at.file());
    match value {
        ast::Amount::Literal(literal) => {
            at.world.literal_amount(file, literal, Some(fallback)).or_report(at.world).map(|amount| (amount, None))
        }
        ast::Amount::Computed(root) => {
            let name = at.world.book.names.intern("assertion");
            let (program, roots) =
                crate::laws::compile_template(at.world, file, home, subject, name, &[], &[(root, Ty::AMOUNT)])?;
            let [root] = roots.as_ref() else {
                return None;
            };
            let unit = match program.nodes[*root].typed_ty() {
                Some(Ty::Amount(Dim::Of(unit))) => unit,
                _ => fallback,
            };
            let program = at.world.book.assertion_programs.push(program);
            Some((Amount::zero(unit), Some((program, *root))))
        }
    }
}

fn unsupported_computed_value(loc: Loc, subject: &str, diags: &mut Vec<Diagnostic>) {
    diags.push(
        Diagnostic::error("computed-value-subject", format!("computed values are not supported for {subject}"))
            .label(loc, "write a literal amount here"),
    );
}

/// What a value statement says about a gap between the book and it: `via` where it went, `!` that it is
/// unexplained, nothing that it is refused.
fn assertion_gap(at: &mut Stated<'_, '_, '_>) -> Option<Gap> {
    let (home, file) = (at.home(), at.file());
    let mut gap = Gap::Refused;
    for clause in &file[at.statement.tail] {
        match clause.kind {
            ClauseKind::Via(name) => {
                let end = at.world.end_on(home, Word::of(file, name.0), Some(at.statement.date)).or_report(at.world)?;
                gap = Gap::Via { place: end.place, loc: clause.at };
            }
            ClauseKind::Waive(waive) => {
                let reason = waive.reason.map(|reason| at.world.book.quoted_text(reason.0));
                gap = Gap::Unexplained(Waive { loc: waive.at, reason });
            }
            ClauseKind::Description(_) | ClauseKind::Code(_) => {}
            other => unreachable!("the parser keeps {other:?} off a value"),
        }
    }
    Some(gap)
}

// ─── Measures ───────────────────────────────────────────────────────────────

/// `worked 8h` and `used 120 kWh`: time or a thing spent, by a party, an entity, a place or an asset.
pub(super) fn lower_measure<'s>(at: &mut Stated<'_, '_, 's>, literal: ast::Literal<'s>, action: Action) {
    let Some(target) = statement_target(at) else {
        return;
    };
    let (subject, owner) = match target {
        StatementTarget::Place(place) => (ModelSubject::Place(place), at.world.book.places[place].owner),
        StatementTarget::Entity(entity) => (ModelSubject::Entity(entity), entity),
        StatementTarget::Asset(asset) => (ModelSubject::Asset(asset), at.world.book.assets[asset].owner),
        _ => {
            at.unsupported("a measure needs a named entity, place or asset");
            return;
        }
    };
    let Some(quantity) = at.world.literal_amount(at.file(), literal, None).or_report(at.world) else {
        return;
    };
    let diagnostic_start = at.world.diags.len();
    let (codes, tail) = read_tail(at.world, at.home(), at.file(), Line::Measure(at.code_index), at.statement.tail);
    if at.world.diags.len() != diagnostic_start {
        return;
    }
    at.world.book.measures.push(Measure {
        day: at.statement.date,
        action,
        subject,
        quantity,
        owner,
        party: tail.detail.hold,
        purpose: tail.purpose,
        description: tail.description,
        codes,
        against: tail.detail.against,
        loc: at.loc,
    });
}

// ─── Claims, waivers and endings ────────────────────────────────────────────

/// `^code waived`: a claim written off in full.
pub(super) fn lower_claim_change<'s>(at: &mut Stated<'_, '_, 's>, code: ast::Code<'s>) {
    if at.has_lines() {
        at.unsupported("a full claim write-off cannot include recovery lines");
        return;
    }
    let mut description = None;
    for clause in &at.file()[at.statement.tail] {
        match clause.kind {
            // The parser takes one description per statement.
            ClauseKind::Description(text) => description = Some(at.world.book.quoted_text(text.0)),
            _ => {
                at.unsupported("a full claim write-off only accepts a description");
                return;
            }
        }
    }
    let Some(target) = claim_target(at, code) else {
        return;
    };
    let (day, loc) = (at.statement.date, at.loc);
    at.world.book.claim_changes.push(ClaimChange {
        day,
        target,
        action: ClaimChangeAction::WriteOff,
        description,
        loc,
    });
}

/// The transaction a write-off is of, when it made a claim and is older than the write-off.
fn claim_target<'s>(at: &mut Stated<'_, '_, 's>, code: ast::Code<'s>) -> Option<Id<Txn>> {
    let reference_loc = at.file().loc(code.name());
    let target = at.code_index.resolve(at.world, code, reference_loc, CodeUse::ClaimWaiver)?;
    let (book, source) = (&at.world.book, &at.world.book.txns[target]);
    let flows = || source.flows.ids().map(|flow| &book.flows[flow]);
    if !flows().any(|flow| book.makes_claim(flow)) {
        at.world.diags.push(
            Diagnostic::error("claim-writeoff-target", "this transaction did not create an open claim")
                .label(reference_loc, "the referenced transaction has no claim flow")
                .context(source.loc, "the transaction identified by this code is here")
                .help("use the code on an earlier `owes` statement"),
        );
        return None;
    }
    if at.statement.date < source.day {
        at.world.diags.push(
            Diagnostic::error("claim-writeoff-date", "a claim cannot be waived before it exists")
                .label(at.loc, "this date precedes the claim transaction"),
        );
        return None;
    }
    Some(target)
}

/// What a contract waiver's tail says.
struct WaiverTail<'s> {
    last: Day,
    code: Option<ast::Code<'s>>,
    description: Option<crate::book::Text>,
}

/// `contract waived until 2026-06-30`: the days a contract's occurrences do not happen.
pub(super) fn lower_contract_change(at: &mut Stated<'_, '_, '_>) {
    let (statement, loc) = (at.statement, at.loc);
    let Subject::Name(name) = statement.subject else {
        at.unsupported("only a contract occurrence can be waived here");
        return;
    };
    let sym = at.world.book.names.intern(name.0);
    let Some(contract_id) = at.world.book.lookup.contracts.get(&sym).copied() else {
        at.unsupported("waiving a claim is not yet lowered natively");
        return;
    };
    if at.has_lines() {
        at.unsupported("a contract waiver cannot carry recovery lines yet");
        return;
    }
    let Some(tail) = waiver_tail(at) else {
        return;
    };
    let Some(days) = Days::new(statement.date, tail.last) else {
        at.world.diags.push(
            Diagnostic::error("waiver-span", "a waiver ends before it begins")
                .label(loc, "the `until` day must be on or after this day"),
        );
        return;
    };
    let code = tail.code.map(|code| at.world.book.names.intern(code.name()));
    let change = BookChange { days, description: tail.description, code, loc };
    if !waive_contract(&mut at.world.book.contracts[contract_id], change) {
        at.world.diags.push(
            Diagnostic::error("waiver-without-schedule", "this contract has no schedule to waive")
                .label(loc, "there is no regular or standing occurrence here"),
        );
    }
}

/// Whether a statement's subject names a contract: what `2029-03-01 mortgage now at 6.25%` is about, even where the name is
/// also a kind's.
pub(super) fn names_a_contract(world: &World<'_>, subject: Subject<'_>) -> bool {
    matches!(subject, Subject::Name(name) if world.book.contract(name.0).is_some())
}

/// `2029-03-01 mortgage now at 6.25%`: from this day the lender's yearly rate is that (LANGUAGE §7). The loan's schedule
/// refigures its payment over what is left from that day's balance.
pub(super) fn lower_rate_change<'s>(at: &mut Stated<'_, '_, 's>, line: &ast::Prop<'s>) {
    let Subject::Name(name) = at.statement.subject else { return };
    let Some(contract_id) = at.world.book.contract(name.0) else { return };
    if at.world.book.contracts[contract_id].loan.is_none() {
        at.world.diags.push(
            Diagnostic::error("contract-rate-change", format!("`{}` is no loan, so it has no rate to change", name.0))
                .label(at.loc, "only a contract with a `loan` line has a rate")
                .help("write the rate on the loan: `loan AMOUNT on DATE at 6.25% over SPAN`"),
        );
        return;
    }
    let mut args = Args::shaped(at.file(), line, "contract-loan-rate", "now at PERCENT");
    let Some(rate) = args.rate().and_then(|rate| args.done().map(|()| rate)).or_report(at.world) else {
        return;
    };
    at.world.book.contracts[contract_id].rates.push(RateChange { day: at.statement.date, rate, loc: at.loc });
}

/// The parser takes one `until` and one description per statement, and no clause a waiver has no use for; only
/// codes may be repeated, and a waiver names one.
fn waiver_tail<'s>(at: &mut Stated<'_, '_, 's>) -> Option<WaiverTail<'s>> {
    let mut tail = WaiverTail { last: at.statement.date, code: None, description: None };
    let mut first_code = None;
    for clause in &at.file()[at.statement.tail] {
        match clause.kind {
            ClauseKind::Until(until) => tail.last = until,
            ClauseKind::Code(written) => {
                if let Some(first) = first_code {
                    at.world.diags.push(problem::twice("waiver code", clause.at, first));
                    return None;
                }
                (tail.code, first_code) = (Some(written), Some(clause.at));
            }
            ClauseKind::Description(text) => tail.description = Some(at.world.book.quoted_text(text.0)),
            ClauseKind::Purpose(_) => {
                at.unsupported("a contract waiver has no claim-recovery purpose");
                return None;
            }
            other => unreachable!("the parser keeps {other:?} off a waiver"),
        }
    }
    Some(tail)
}

/// Waives the contract's regular and standing schedules for `change.days`; whether there were any to waive.
fn waive_contract(contract: &mut Contract, change: BookChange) -> bool {
    let scheduled = contract.terms.is_some() || contract.standing.is_some();
    if scheduled {
        contract.waived.paint(change.days, Some(change));
    }
    scheduled
}

/// `thing ended`: a promise, a place or an asset stops on this day.
pub(super) fn lower_end(at: &mut Stated<'_, '_, '_>) {
    let (statement, loc) = (at.statement, at.loc);
    if at.has_lines() {
        at.unsupported("an ending cannot carry journal lines");
        return;
    }
    let Subject::Name(name) = statement.subject else {
        at.unsupported("this subject cannot end here");
        return;
    };
    let Some(target) = end_target(at, name) else {
        return;
    };
    let (codes, tail) = read_tail(at.world, at.home(), at.file(), Line::Ending, statement.tail);
    let event = EndEvent { day: statement.date, target, codes, description: tail.description, loc };
    close(at.world, target, statement.date, loc);
    at.world.book.endings.push(event);
}

fn end_target<'s>(at: &mut Stated<'_, '_, 's>, name: ast::Name<'s>) -> Option<EndTarget> {
    let sym = at.world.book.names.intern(name.0);
    if let Some(contract_id) = at.world.book.lookup.contracts.get(&sym).copied() {
        if at.statement.date < at.world.book.contracts[contract_id].days.first() {
            at.world.diags.push(
                Diagnostic::error("end-before-contract", "a contract cannot end before it begins")
                    .label(at.loc, "this date precedes the contract's first day"),
            );
            return None;
        }
        return Some(EndTarget::Contract(contract_id));
    }
    if let Some(asset) = at.world.book.asset(name.0) {
        return Some(EndTarget::Asset(asset));
    }
    let end = at.world.end_on(at.home(), Word::of(at.file(), name.0), Some(at.statement.date)).or_report(at.world)?;
    Some(match at.world.book.places[end.place].role {
        Role::Asset(asset) => EndTarget::Asset(asset),
        _ => EndTarget::Place(end.place),
    })
}

/// What an ending does to what it ends: a contract stops on that day, a place or an asset's place closes.
fn close(world: &mut World<'_>, target: EndTarget, day: Day, loc: Loc) {
    match target {
        EndTarget::Contract(contract_id) => {
            let contract = &mut world.book.contracts[contract_id];
            if let Some(days) = Days::new(contract.days.first(), day.min(contract.days.last())) {
                contract.days = days;
                contract.ended = Some(loc);
            }
        }
        EndTarget::Place(place) => world.say(Holder::Place(place), builtin::CLOSED, day),
        EndTarget::Asset(asset) => {
            let place = world.book.assets[asset].place;
            world.say(Holder::Place(place), builtin::CLOSED, day);
        }
    }
}

// ─── Basis ──────────────────────────────────────────────────────────────────

/// What an asset cost, as a basis statement says it.
enum Cost {
    Stated(Qty),
    Computed(NodeId),
}

/// `ASSET basis 400 USD`: an asset arrives from the unknown party at what it cost.
pub(super) fn lower_basis<'s>(at: &mut Stated<'_, '_, 's>, written: ast::Amount<'s>, since: Option<Day>) {
    let Some(asset) = basis_asset(at) else {
        return;
    };
    let (file, statement, loc) = (at.file(), at.statement, at.loc);
    let mut exprs = Vec::new();
    if let ast::Amount::Computed(expr) = written {
        exprs.push((expr, Ty::AMOUNT));
    }
    push_tail_roots(file, statement.tail, &mut exprs);
    let name = at.world.book.names.intern("journal");
    let compiled = super::compile_roots(at.world, file, at.home(), Ty::Asset, name, &[], &exprs);
    let Some((program, roots)) = compiled else {
        return;
    };
    let mut staged = Staged::open(at.world);
    let txn = Id::new(staged.book.txns.len() as u32);
    let cx =
        FlowCx { file, home: at.site.home, day: statement.date, txn, loc, roots: &roots, code_index: at.code_index };
    let diagnostic_start = staged.diags.len();
    let (header_codes, mut tail) = cx.lower_tail(&mut staged, statement.tail);
    tail.detail.since = since.or(tail.detail.since);
    let basis_root = match basis_cost(&mut staged, file, written, (&program, &roots)) {
        None => return,
        Some(Cost::Stated(qty)) => {
            tail.detail.basis = Some(qty);
            None
        }
        Some(Cost::Computed(root)) => Some(root),
    };
    if !tail.valid {
        return;
    }
    let Some(flow) = arrival_flow(&mut staged, &cx, asset, header_codes, tail) else {
        return;
    };
    let waive = flow.waive;
    staged.book.flows.push(flow);
    let mut flow_roots = Vec::new();
    push_flow_expressions(&mut flow_roots, 0, None, None, basis_root);
    if staged.diags.len() != diagnostic_start {
        return;
    }
    let program_id = keep_program(&mut staged, program, flow_roots, None);
    let record = Txn {
        program: program_id,
        codes: header_codes,
        waive,
        ..super::record::journal_txn(&staged, statement.date, loc)
    };
    staged.book.txns.push(record);
    staged.commit();
}

/// The asset a basis statement names, when it names one and has nothing indented under it.
fn basis_asset(at: &mut Stated<'_, '_, '_>) -> Option<Id<Asset>> {
    let Subject::Name(name) = at.statement.subject else {
        at.unsupported("a basis statement must name an asset");
        return None;
    };
    let Some(asset) = at.world.book.asset(name.0) else {
        at.world.diags.push(
            Diagnostic::error("basis-asset", "a basis statement must name an asset")
                .label(at.file().loc(name.0), "this name is not a declared asset"),
        );
        return None;
    };
    if at.has_lines() {
        at.unsupported("a basis statement cannot have indented journal lines");
        return None;
    }
    Some(asset)
}

/// What the basis amount is: stated, or computed by one of the program's roots. It must be in the base currency.
fn basis_cost<'s>(
    world: &mut World<'s>,
    file: &ast::File<'s>,
    written: ast::Amount<'s>,
    (program, roots): (&Program, &Map<ast::ExprId, NodeId>),
) -> Option<Cost> {
    match written {
        ast::Amount::Literal(literal) => {
            let amount = world.literal_amount(file, literal, None).or_report(world)?;
            if amount.unit != world.book.base {
                world.diags.push(
                    Diagnostic::error("basis-unit", "asset basis must be in the base currency")
                        .label(file.loc(literal.0), "convert this amount to the book's base unit"),
                );
                return None;
            }
            Some(Cost::Stated(amount.qty))
        }
        ast::Amount::Computed(expr) => {
            let Some(&root) = roots.get(&expr) else {
                world.diags.push(
                    Diagnostic::error("basis-expression", "the basis expression was not compiled")
                        .label(file.exprs[expr].loc, "the expression is not available here"),
                );
                return None;
            };
            if let Some(Ty::Amount(Dim::Of(unit))) = program.nodes[root].typed_ty()
                && unit != world.book.base
            {
                world.diags.push(
                    Diagnostic::error("basis-unit", "asset basis must be in the base currency")
                        .label(file.exprs[expr].loc, "this expression has another unit"),
                );
                return None;
            }
            Some(Cost::Computed(root))
        }
    }
}

/// The flow that brings the asset in: one of it, from the unknown party to its place.
fn arrival_flow<'s>(
    staged: &mut Staged<'_, 's>,
    cx: &FlowCx<'_, 's>,
    asset: Id<Asset>,
    header_codes: Run<Sym>,
    tail: Tail,
) -> Option<Flow> {
    let asset = &staged.book.assets[asset];
    let (place, owner, unit) = (asset.place, asset.owner, asset.unit);
    let unknown = staged.book.roots.unknown;
    let Some(unknown_place) = staged.book.entities[unknown].place else {
        staged.diags.push(
            Diagnostic::error("basis-source", "the unknown party has no flow endpoint")
                .label(cx.loc, "cannot record this asset's arrival"),
        );
        return None;
    };
    let empty = Run::new(Id::new(0), 0);
    let from = ResolvedEnd { place: unknown_place, entity: Some(unknown), select: empty };
    let to = ResolvedEnd { place, entity: None, select: empty };
    let quantity = Amount::new(Qty(1), unit);
    let shape =
        Shape { ends: Ends { from, to }, out: quantity, arrive: quantity, infer: Infer::Known, mode: Mode::Actual };
    let codes = Codes { header: header_codes, local: empty_codes(staged) };
    let mut flow = make_resolved_flow(staged, cx, shape, codes, tail, cx.loc)?;
    flow.owner = owner;
    Some(flow)
}
