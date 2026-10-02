//! S5 contract declarations are lowered in two passes. Names and ids exist
//! before any template expression compiles, so a template may mention a
//! contract declared later in the project.

use axiom_core::{Day, Days, Diagnostic, Dim, Id, Loc, Map, Qty, Ratio, Run, Span, Sym, Timeline};
use axiom_syntax as ast;
use axiom_syntax::{BinOp, ClauseKind, Direction, ExprKind, Name};

use super::also::{AlsoCx, lower_alsos};
use super::infer::infer_for_flow;
use super::tail::{Reach, resolve_object, written_purpose, written_waive};
use super::{compile_roots, contract_roots, inputs};
use crate::book::{
    Also, AlsoOn, Amount, Asset, At, Cadence, Class, Commodity, Contract, Coverage, Deadline, Entity, Escalation,
    FlowSide, Input, Loan, Param, Place, Prepay, Relative, Reset, Role, Share, TemplateAmount, TemplateFlow,
    TemplateItem, TemplateItemParent, TemplateLeg, TemplateProgram, TemplateQuantity, Terms, TermsState, Text,
};
use crate::collect::Collected;
use crate::declare::World;
use crate::errors::{Reported, Word};
use crate::journal::{Flow, Infer, Mode, Origin, Provenance, Purposed, Select, TEMPLATE_TXN, Waive};
use crate::law::{Owner, Ty};
use crate::laws::Placement;
use crate::problem::{self, Noun};
use crate::resolve::End;
use crate::scope::Home;
use crate::sources::Site;

/// A contract as written, with the id reserved for it and its name.
#[derive(Clone, Copy)]
struct WrittenContract<'a, 's> {
    site: &'a Site<'a, 's>,
    node: &'a ast::Contract<'s>,
    id: Id<Contract>,
    name: Sym,
    loc: Loc,
}

impl<'a, 's> WrittenContract<'a, 's> {
    fn file(&self) -> &'a ast::File<'s> {
        &self.site.source.file
    }

    fn home(&self) -> Home {
        self.site.home
    }
}

/// Reserves every contract id before compiling any contract body. Terms and
/// their computed roots are then compiled against the complete contract
/// namespace, while occurrences are handled by [`super::record`].
pub(crate) fn contracts<'a, 's>(world: &mut World<'s>, collected: &Collected<'a, 's>, diags: &mut Vec<Diagnostic>) {
    let mut written = Vec::new();
    for contract in &collected.contracts {
        let (site, node, loc) = (contract.site, contract.node, contract.item.loc);
        let name = world.book.names.intern(node.name.0);
        if let Some(first) = world.book.lookup.contracts.get(&name).copied() {
            let (word, first) = (Word::of(contract.file(), node.name.0), Some(world.book.contracts[first].loc));
            diags.push(problem::duplicate(Noun::Contract, word, first));
            continue;
        }
        let id = world.book.contracts.push(empty_contract(name, loc, world.book.roots.me));
        world.book.lookup.contracts.insert(name, id);
        written.push(WrittenContract { site, node, id, name, loc });
    }

    for written in written.iter().copied() {
        if let Some(contract) = lower_contract(world, written, diags) {
            world.book.contracts[written.id] = contract;
        }
    }

    // Nested contract laws follow their parent contract's declaration, after
    // its id exists and before the shared law index is finalized.
    for written in written.iter().copied() {
        let file = written.file();
        let placement = Placement { file, home: written.home(), owner: Owner::Contract(written.id), subject: Ty::Flow };
        let mut laws = Vec::new();
        for law in &file[written.node.laws] {
            laws.extend(crate::laws::compile_native(world, diags, &placement, law));
        }
        world.book.contracts[written.id].laws = laws.into_boxed_slice();
    }
}

fn empty_contract(name: Sym, loc: Loc, me: axiom_core::Id<Entity>) -> Contract {
    Contract {
        name,
        party: me,
        owner: me,
        purpose: None,
        description: None,
        area: None,
        days: Days::ALWAYS,
        terms: None,
        standing: None,
        buys: None,
        deposit: None,
        deposit_holding: None,
        loan: None,
        matching: None,
        ended: None,
        laws: Box::default(),
        doc: None,
        loc,
    }
}

/// What a contract says before its schedules are read: who it is with, when, how much space it is about, what it
/// is for, and the inputs its templates take.
struct Facts {
    party: Id<Entity>,
    days: Days,
    area: Option<Amount>,
    purpose: Option<At<Purposed>>,
    description: Option<Text>,
    inputs: Box<[Input]>,
}

fn contract_facts<'a, 's>(
    world: &mut World<'s>,
    written: WrittenContract<'a, 's>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Facts> {
    let (node, file) = (written.node, written.file());
    let name_word = Word::of(file, node.name.0);
    let party_word = node.party.map_or(name_word, |party| Word::of(file, party.0));
    let party = world.entity(written.site.home, party_word).or_report(diags)?;
    let days = contract_days(file, node.props, diags)?;
    let area = contract_area(world, file, node.props, diags).ok()?;
    let purpose = contract_purpose(world, written, diags)?;
    let description = node.description.map(|description| world.book.quoted_text(description.0));
    let inputs = inputs(world, file, node.props, diags);
    Some(Facts { party, days, area, purpose, description, inputs })
}

fn lower_contract<'a, 's>(
    world: &mut World<'s>,
    written: WrittenContract<'a, 's>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Contract> {
    let (node, file, home) = (written.node, written.file(), written.home());
    let Facts { party, days, area, purpose, description, inputs: contract_inputs } =
        contract_facts(world, written, diags)?;
    let anchor = days.first();
    let roots = contract_roots(file, node);
    let compile = |world: &mut World<'s>, roots: &_, diags: &mut Vec<Diagnostic>| {
        compile_roots(world, file, home, Ty::Flow, written.name, &contract_inputs, roots, diags)
    };
    let (regular, standing) = (compile(world, &roots.regular, diags), compile(world, &roots.standing, diags));

    let owner = match node.schedule.or(node.standing) {
        Some(schedule) => schedule_owner(world, home, file, Some(schedule), diags)?,
        None => world.book.roots.me,
    };
    let default_holding = node
        .schedule
        .or(node.standing)
        .and_then(|schedule| schedule.terms.holding.map(|holding| (holding.name, schedule.at)));
    let deposit = contract_deposit(world, written, Keeping { owner, default_holding }, diags).ok()?;
    let also_cx = AlsoCx {
        file,
        home,
        owner: Owner::Contract(written.id),
        on: AlsoOn::Contract(written.id),
        inputs: &contract_inputs,
        currency: world.book.currency(owner),
    };
    let also = lower_alsos(world, &also_cx, node.alsos, diags);
    let loan = contract_loan(world, written, party, owner, diags)?;
    let mut contract = empty_contract(written.name, written.site.source.file.loc(node.name.0), owner);
    contract.party = party;
    contract.owner = owner;
    contract.days = days;
    contract.purpose = purpose;
    contract.description = description;
    contract.area = area;
    if let Some((amount, holding)) = deposit {
        contract.deposit = Some(amount);
        contract.deposit_holding = Some(holding);
    }
    contract.loan = loan.map(|(loan, _)| loan);
    contract.doc = node_doc(world, written.site, written.loc);
    contract.loc = written.loc;
    contract.buys = node.standing.and_then(|schedule| match schedule.terms.payment {
        Some(ast::Payment::Buy { unit, .. }) => resolve_commodity(world, file, unit, diags),
        _ => None,
    });

    let cx = TermsCx {
        written,
        file,
        inputs: &contract_inputs,
        anchor,
        party,
        purpose,
        description,
        area,
        loan_rate: loan.map(|(_, rate)| rate),
        also: &also,
    };
    if let (Some(schedule), Some((program, ids))) = (node.schedule, regular) {
        contract.terms = Some(Timeline::new(lower_terms(world, &cx, schedule, program, ids, diags)?));
    }
    if let (Some(schedule), Some((program, ids))) = (node.standing, standing) {
        contract.standing = Some(Timeline::new(lower_terms(world, &cx, schedule, program, ids, diags)?));
    }
    Some(contract)
}

/// The purpose a contract is written for, with the object it is of: the purpose is the contract's, and nothing
/// at all comes of a contract whose purpose names none.
fn contract_purpose<'a, 's>(
    world: &mut World<'s>,
    written: WrittenContract<'a, 's>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Option<At<Purposed>>> {
    let Some(purpose) = written.node.purpose else {
        return Some(None);
    };
    let (file, home) = (written.file(), written.home());
    let word = Word::of(file, purpose.name.0);
    let id = world.purpose(home, word).or_report(diags)?;
    let of = purpose.of.and_then(|object| resolve_object(world, home, file, object, Reach::Parties, diags));
    Some(Some(At { value: Purposed { purpose: id, of, source: Provenance::Contract(written.id) }, loc: word.loc }))
}

fn contract_loan<'s>(
    world: &World<'s>,
    contract: WrittenContract<'_, 's>,
    party: Id<Entity>,
    owner: Id<Entity>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Option<(Loan, Ratio)>> {
    let (file, home) = (contract.file(), contract.home());
    let mut written = file[contract.node.props].iter().filter(|prop| prop.name.0 == "loan");
    let Some(prop) = written.next() else {
        return Some(None);
    };
    if let Some(duplicate) = written.next() {
        diags.push(problem::twice("loan", duplicate.loc, prop.loc));
        return None;
    }
    let LoanFields { principal, on, rate, term, asset } = loan_fields(world, file, prop, diags)?;
    let resets = loan_resets(world, home, file, prop.lines, on, diags)?;
    let prepay = loan_prepay(file, prop.lines, diags)?;
    let debt = world.tab(party, owner, Class::Debt, prop.loc).or_report(diags)?;
    Some(Some((Loan { principal, on, term, asset, debt, resets, prepay }, rate)))
}

/// What a `loan AMOUNT on DATE at RATE over SPAN [for ASSET]` line says.
struct LoanFields {
    principal: Amount,
    on: Day,
    rate: Ratio,
    term: Span,
    asset: Option<Id<Asset>>,
}

fn loan_fields<'s>(
    world: &World<'s>,
    file: &ast::File<'s>,
    prop: &ast::Prop<'s>,
    diags: &mut Vec<Diagnostic>,
) -> Option<LoanFields> {
    let args = &file[prop.args];
    let keyword = |index: usize, expected: &str| {
        args.get(index).is_some_and(|&id| matches!(file.exprs[id].kind, ExprKind::Name(name) if name.0 == expected))
    };
    let Some(&principal_expr) = args.first() else {
        diags.push(
            Diagnostic::error("contract-loan", "a loan needs its principal, start date, rate, and term")
                .label(prop.loc, "write `loan AMOUNT on DATE at RATE over SPAN`"),
        );
        return None;
    };
    let principal = loan_principal(world, file, principal_expr, diags)?;
    if args.len() != 7 && args.len() != 9 || !keyword(1, "on") || !keyword(3, "at") || !keyword(5, "over") {
        diags.push(
            Diagnostic::error("contract-loan", "the loan definition has missing or extra fields")
                .label(prop.loc, "write `loan AMOUNT on DATE at RATE over SPAN [for ASSET]`"),
        );
        return None;
    }
    let ExprKind::Date(on) = file.exprs[args[2]].kind else {
        diags.push(
            Diagnostic::error("contract-loan-date", "a loan start needs a date")
                .label(file.exprs[args[2]].loc, "write the date after `on`"),
        );
        return None;
    };
    let rate = match file.exprs[args[4]].kind {
        ExprKind::Pct(percent) => Ratio::percent(percent.mantissa as i128, percent.scale),
        _ => None,
    };
    let Some(rate) = rate.filter(|rate| !rate.is_negative()) else {
        diags.push(
            Diagnostic::error("contract-loan-rate", "a loan rate must be a nonnegative percentage")
                .label(file.exprs[args[4]].loc, "write a rate such as `5.875%`"),
        );
        return None;
    };
    let ExprKind::Span(term) = file.exprs[args[6]].kind else {
        return loan_term_error(file, args[6], diags);
    };
    if !positive_loan_term(term) {
        return loan_term_error(file, args[6], diags);
    }
    let asset = match args.get(7) {
        Some(_) => Some(loan_asset(world, file, args[7], args[8], diags)?),
        None => None,
    };
    Some(LoanFields { principal, on, rate, term, asset })
}

fn loan_term_error<T>(file: &ast::File<'_>, term: ast::ExprId, diags: &mut Vec<Diagnostic>) -> Option<T> {
    diags.push(
        Diagnostic::error("contract-loan-term", "a loan term must be a positive span")
            .label(file.exprs[term].loc, "write a term such as `30y`"),
    );
    None
}

/// The positive literal amount a loan is of.
fn loan_principal<'s>(
    world: &World<'s>,
    file: &ast::File<'s>,
    expr: ast::ExprId,
    diags: &mut Vec<Diagnostic>,
) -> Option<Amount> {
    let at = file.exprs[expr].loc;
    let ExprKind::Amount(literal) = file.exprs[expr].kind else {
        diags.push(
            Diagnostic::error("contract-loan-principal", "a loan principal must be a literal amount")
                .label(at, "write an amount such as `3_000 USD`"),
        );
        return None;
    };
    let unit = match literal.unit() {
        Some(unit) => world.commodity_of(Word::of(file, unit.0)).or_report(diags)?,
        None => world.book.base,
    };
    let amount = world.amount(literal.num(), unit, at).or_report(diags)?;
    if amount.qty.0 <= 0 {
        diags.push(
            Diagnostic::error("contract-loan-principal", "a loan principal must be positive")
                .label(at, "this amount is not positive"),
        );
        return None;
    }
    Some(amount)
}

/// The declared asset a loan financed, after its `for`.
fn loan_asset<'s>(
    world: &World<'s>,
    file: &ast::File<'s>,
    keyword: ast::ExprId,
    named: ast::ExprId,
    diags: &mut Vec<Diagnostic>,
) -> Option<Id<Asset>> {
    if !matches!(file.exprs[keyword].kind, ExprKind::Name(name) if name.0 == "for") {
        diags.push(
            Diagnostic::error("contract-loan-asset", "a financed asset follows `for`")
                .label(file.exprs[keyword].loc, "write `for ASSET` here"),
        );
        return None;
    }
    let ExprKind::Name(name) = file.exprs[named].kind else {
        diags.push(
            Diagnostic::error("contract-loan-asset", "a financed asset needs a name")
                .label(file.exprs[named].loc, "write the declared asset name"),
        );
        return None;
    };
    let asset = world.book.asset(name.0);
    if asset.is_none() {
        diags.push(
            Diagnostic::error("contract-loan-asset", format!("asset `{}` is not declared", name.0))
                .label(file.exprs[named].loc, "declare this asset before the loan"),
        );
    }
    asset
}

/// The nested `prepay` of a loan, and a refusal of any nested line a loan does not have.
fn loan_prepay<'s>(
    file: &ast::File<'s>,
    lines: ast::Many<ast::Nested<'s>>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Prepay> {
    let mut prepay = Prepay::Shortens;
    let mut prepay_loc = None;
    for nested in &file[lines] {
        let nested = &nested.0;
        match nested.name.0 {
            "prepay" => {
                if let Some(first) = prepay_loc {
                    diags.push(problem::twice("prepayment rule", nested.loc, first));
                    return None;
                }
                prepay_loc = Some(nested.loc);
                let value = file[nested.args].first().and_then(|&id| match file.exprs[id].kind {
                    ExprKind::Name(name) if name.0 == "shortens" => Some(Prepay::Shortens),
                    ExprKind::Name(name) if name.0 == "recasts" => Some(Prepay::Recasts),
                    _ => None,
                });
                match value {
                    Some(value) if file[nested.args].len() == 1 => prepay = value,
                    _ => {
                        diags.push(
                            Diagnostic::error("contract-loan-prepay", "prepay must be `shortens` or `recasts`")
                                .label(nested.loc, "write exactly one supported prepayment rule"),
                        );
                        return None;
                    }
                }
            }
            "resets" => {}
            _ => {
                diags.push(
                    Diagnostic::error("contract-loan-property", "this nested loan property is not supported")
                        .label(nested.loc, "remove or correct this property"),
                );
                return None;
            }
        }
    }
    Some(prepay)
}

fn loan_resets<'s>(
    world: &World<'s>,
    home: Home,
    file: &ast::File<'s>,
    lines: ast::Many<ast::Nested<'s>>,
    loan_on: Day,
    diags: &mut Vec<Diagnostic>,
) -> Option<Option<Reset>> {
    let mut resets = file[lines].iter().filter(|nested| nested.0.name.0 == "resets");
    let Some(first) = resets.next() else {
        return Some(None);
    };
    if let Some(second) = resets.next() {
        diags.push(problem::twice("reset rule", second.0.loc, first.0.loc));
        return None;
    }
    let nested = &first.0;
    let (every, from, index_name, margin, limits_at) = reset_schedule(file, nested, loan_on, diags)?;
    let index = reset_index(world, home, file, index_name, diags)?;
    let (cap, life) = reset_limits(file, &file[nested.args], limits_at, diags)?;
    Some(Some(Reset { every, from, index, margin, cap, life }))
}

/// `resets 1y from DATE to PARAM + PERCENT`: the interval, the first date, the index named and its margin, and
/// where in the arguments the limits that may follow begin.
fn reset_schedule<'s>(
    file: &ast::File<'s>,
    nested: &ast::Prop<'s>,
    loan_on: Day,
    diags: &mut Vec<Diagnostic>,
) -> Option<(Span, Day, Name<'s>, Ratio, usize)> {
    let args = &file[nested.args];
    let Some(&every_expr) = args.first() else {
        return invalid_reset(diags, nested.loc, "write `resets 1y from DATE to PARAM + PERCENT`");
    };
    let every = match file.exprs[every_expr].kind {
        ExprKind::Span(span) if positive_loan_term(span) => span,
        _ => return invalid_reset(diags, file.exprs[every_expr].loc, "the reset interval must be a positive span"),
    };
    if !reset_keyword(file, args, 1, "from") {
        return invalid_reset(diags, nested.loc, "write `from DATE` after the reset interval");
    }
    let Some(&from_expr) = args.get(2) else {
        return invalid_reset(diags, nested.loc, "write the first reset date after `from`");
    };
    let from = match file.exprs[from_expr].kind {
        ExprKind::Date(day) if day >= loan_on => day,
        ExprKind::Date(_) => {
            return invalid_reset(diags, file.exprs[from_expr].loc, "the first reset cannot precede the loan");
        }
        _ => return invalid_reset(diags, file.exprs[from_expr].loc, "write a full date for the first reset"),
    };
    if !reset_keyword(file, args, 3, "to") {
        return invalid_reset(diags, nested.loc, "write `to PARAM + PERCENT` after the reset date");
    }
    let Some(&rate_expr) = args.get(4) else {
        return invalid_reset(diags, nested.loc, "write the index and margin after `to`");
    };
    let (index_name, margin_expr) = match file.exprs[rate_expr].kind {
        ExprKind::Binary(BinOp::Add, index, margin) => match file.exprs[index].kind {
            ExprKind::Name(name) if is_percent(file, margin) => (name, margin),
            _ => return invalid_reset(diags, file.exprs[rate_expr].loc, "write `PARAM + PERCENT`"),
        },
        _ => return invalid_reset(diags, file.exprs[rate_expr].loc, "write `PARAM + PERCENT`"),
    };
    let Some(margin) = percent_ratio(file, margin_expr) else {
        return invalid_reset(diags, file.exprs[margin_expr].loc, "the reset margin cannot be represented");
    };
    if margin.is_negative() {
        return invalid_reset(diags, file.exprs[margin_expr].loc, "the reset margin cannot be negative");
    }
    Some((every, from, index_name, margin, 5))
}

/// The param a loan's rate resets by, which must be a rate.
fn reset_index<'s>(
    world: &World<'s>,
    home: Home,
    file: &ast::File<'s>,
    name: Name<'s>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Id<Param>> {
    let word = Word::of(file, name.0);
    let index = match world.seek_param(home, word) {
        Ok(Some(index)) => index,
        Ok(None) => {
            diags.push(world.missing_param(home, word));
            return None;
        }
        Err(problem) => {
            diags.push(problem);
            return None;
        }
    };
    if world.book.params[index].unit.is_some_and(|unit| unit != Dim::Number) {
        diags.push(
            Diagnostic::error("contract-loan-index-unit", "a loan reset index is a rate")
                .label(word.loc, "use a parameter with no unit or a percentage value"),
        );
        return None;
    }
    Some(index)
}

/// `cap PERCENT` and `life PERCENT`, each at most once, after the margin.
fn reset_limits(
    file: &ast::File<'_>,
    args: &[ast::ExprId],
    mut cursor: usize,
    diags: &mut Vec<Diagnostic>,
) -> Option<(Option<Ratio>, Option<Ratio>)> {
    let (mut cap, mut life) = (None, None);
    while cursor < args.len() {
        let clause_expr = args[cursor];
        let clause = match file.exprs[clause_expr].kind {
            ExprKind::Name(name) if name.0 == "cap" || name.0 == "life" => name.0,
            _ => {
                return invalid_reset(
                    diags,
                    file.exprs[clause_expr].loc,
                    "only `cap PERCENT` and `life PERCENT` follow the margin",
                );
            }
        };
        cursor += 1;
        let Some(&value_expr) = args.get(cursor) else {
            return invalid_reset(diags, file.exprs[clause_expr].loc, "write a percentage after this reset limit");
        };
        let Some(value) = percent_ratio(file, value_expr) else {
            return invalid_reset(diags, file.exprs[value_expr].loc, "a reset limit is a percentage");
        };
        if value.is_negative() {
            return invalid_reset(diags, file.exprs[value_expr].loc, "a reset limit cannot be negative");
        }
        let slot = if clause == "cap" { &mut cap } else { &mut life };
        if slot.replace(value).is_some() {
            return invalid_reset(diags, file.exprs[clause_expr].loc, "write each reset limit once");
        }
        cursor += 1;
    }
    Some((cap, life))
}

fn reset_keyword(file: &ast::File<'_>, args: &[ast::ExprId], at: usize, expected: &str) -> bool {
    args.get(at).is_some_and(|&expr| matches!(file.exprs[expr].kind, ExprKind::Name(name) if name.0 == expected))
}

fn is_percent(file: &ast::File<'_>, expr: ast::ExprId) -> bool {
    matches!(file.exprs[expr].kind, ExprKind::Pct(_))
}

fn percent_ratio(file: &ast::File<'_>, expr: ast::ExprId) -> Option<Ratio> {
    match file.exprs[expr].kind {
        ExprKind::Pct(percent) => Ratio::percent(percent.mantissa as i128, percent.scale),
        _ => None,
    }
}

fn invalid_reset<T>(diags: &mut Vec<Diagnostic>, loc: Loc, help: &str) -> Option<T> {
    diags.push(
        Diagnostic::error("contract-loan-resets", "a loan reset rule needs an interval, date, index and margin")
            .label(loc, help),
    );
    None
}

fn positive_loan_term(span: Span) -> bool {
    span.months >= 0 && span.days >= 0 && (span.months > 0 || span.days > 0)
}

/// What the terms of one contract are lowered against: the contract, and what of it has been lowered already.
struct TermsCx<'a, 's> {
    written: WrittenContract<'a, 's>,
    file: &'a ast::File<'s>,
    inputs: &'a [Input],
    anchor: Day,
    party: Id<Entity>,
    purpose: Option<At<Purposed>>,
    description: Option<Text>,
    area: Option<Amount>,
    loan_rate: Option<Ratio>,
    also: &'a [Id<Also>],
}

/// The header flow of a schedule, with what the legs and the items under it are made against.
struct Header {
    flow: Flow,
    out: TemplateQuantity,
    arrive: TemplateQuantity,
    /// The end the legs are paid from, which is the header's.
    from: Id<Place>,
    from_party: Option<Id<Entity>>,
    side: FlowSide,
    owner: Id<Entity>,
    unit: Id<Commodity>,
}

fn lower_terms<'a, 's>(
    world: &mut World<'s>,
    cx: &TermsCx<'a, 's>,
    schedule: ast::Schedule<'s>,
    program: TemplateProgram,
    roots: Map<ast::ExprId, crate::law::NodeId>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Terms> {
    let (file, home, node) = (cx.file, cx.written.site.home, cx.written.node);
    let header = template_header(world, cx, schedule, &roots, diags)?;
    let legs = template_legs(world, cx, &header, node.body.legs, &roots, diags)?;
    let lower = |world: &mut World<'s>, item, diags: &mut Vec<Diagnostic>| {
        lower_header_item(world, cx, &header, &roots, item, diags)
    };
    let items: Vec<_> = file[node.body.items].iter().filter_map(|item| lower(world, item, diags)).collect();
    let template = TemplateFlow {
        flow: header.flow.clone(),
        out: header.out,
        arrive: header.arrive,
        legs: legs.into_boxed_slice(),
        items: items.into_boxed_slice(),
    };
    let due = node.deadline.as_ref().map(|deadline| Deadline {
        after: deadline.span,
        otherwise: deadline.otherwise.as_ref().and_then(|item| lower(world, item, diags)),
    });
    let every = match schedule.terms.cadence {
        ast::Cadence::Every(span) => Cadence::Every(span),
        ast::Cadence::TwiceMonthly => Cadence::TwiceMonthly,
    };
    let grace = grace_property(file, node.props, diags)?;
    Some(Terms {
        state: TermsState::Active,
        every,
        on: file[schedule.terms.on].to_vec().into_boxed_slice(),
        anchor: cx.anchor,
        template: Box::new([template]),
        program,
        inputs: cx.inputs.to_vec().into_boxed_slice(),
        estimate: schedule.terms.about,
        due,
        grace,
        period: relative_property(file, node.props, diags),
        covers: coverage_property(file, node.props, diags),
        prorated: has_property(file, node.props, "prorated"),
        escalation: escalation_property(world, home, file, node.props, diags),
        shares: shares(world, cx, diags).into_boxed_slice(),
        also: cx.also.to_vec().into_boxed_slice(),
        rate: cx.loan_rate,
        change: None,
    })
}

/// The flow a schedule promises as a whole: between the holding and the party, in the direction written, with
/// the purpose its ends and the contract give it.
fn template_header<'a, 's>(
    world: &mut World<'s>,
    cx: &TermsCx<'a, 's>,
    schedule: ast::Schedule<'s>,
    roots: &Map<ast::ExprId, crate::law::NodeId>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Header> {
    let (file, party) = (cx.file, cx.party);
    let hold = schedule.terms.holding?;
    let holding = resolve_endpoint(world, cx.written.site.home, file, hold.name, diags)?;
    let Some(party_place) = world.book.entities[party].place else {
        diags.push(
            Diagnostic::error("contract-party-place", "the contract party has no usable place")
                .label(schedule.at, "the party's flow endpoint cannot be resolved"),
        );
        return None;
    };
    let (from, to, side) = match hold.direction {
        Direction::From => (holding, party_place, FlowSide::Arrive),
        Direction::Into => (party_place, holding, FlowSide::Out),
    };
    let owner = world.book.places[holding].owner;
    let ScheduleAmount { quantity, amount, buys } = schedule_amount(world, file, schedule, roots, diags)?;
    let (out, arrive) = (quantity, quantity);
    let mut flow = cx.flow(from, to, amount, owner, schedule.at);
    let (from_party, to_party) = match hold.direction {
        Direction::From => (None, Some(party)),
        Direction::Into => (Some(party), None),
    };
    let purpose = cx.purpose.map(|at| (at.value, at.loc));
    let (from_end, to_end) = (End { place: from, entity: from_party }, End { place: to, entity: to_party });
    flow.purpose = infer_for_flow(world, from_end, to_end, purpose, schedule.at, diags).ok()?;
    let arrive = buys.map_or(arrive, TemplateQuantity::Unknown);
    Some(Header { flow, out, arrive, from, from_party, side, owner, unit: amount.unit })
}

/// A promised split leg names the recipient. The source end of the scheduled header is kept and that portion is
/// sent to the named endpoint: an employer's paycheck leg is `lumen -> retirement`, and an owner payment leg is
/// `checking -> escrow`.
fn template_legs<'a, 's>(
    world: &mut World<'s>,
    cx: &TermsCx<'a, 's>,
    header: &Header,
    legs: ast::Many<ast::Leg<'s>>,
    roots: &Map<ast::ExprId, crate::law::NodeId>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Vec<TemplateLeg>> {
    let (file, home) = (cx.file, cx.written.site.home);
    let mut lowered = Vec::new();
    for leg in &file[legs] {
        let to = resolve_endpoint(world, home, file, leg.end.name, diags)?;
        let (quantity, amount) = template_quantity(world, file, leg.amount, roots, header.unit, diags)?;
        let mut flow = cx.flow(header.from, to, amount, header.owner, leg.loc);
        let tail = lower_term_tail(world, home, file, leg.tail, diags);
        flow.codes = tail.codes;
        flow.select = tail.select;
        flow.waive = tail.waive;
        let inferred = tail.purpose.or(cx.purpose).map(|at| (at.value, at.loc));
        let ends = (End { place: header.from, entity: header.from_party }, End { place: to, entity: None });
        flow.purpose = infer_for_flow(world, ends.0, ends.1, inferred, leg.loc, diags).ok()?;
        flow.description = tail.description.or(flow.description);
        lowered.push(TemplateLeg { flow, side: header.side, quantity });
    }
    Some(lowered)
}

/// What a schedule pays: how the header says it, in what amount, and, for a standing buy, the commodity bought.
struct ScheduleAmount {
    quantity: TemplateQuantity,
    amount: Amount,
    buys: Option<Id<Commodity>>,
}

fn schedule_amount<'s>(
    world: &World<'s>,
    file: &ast::File<'s>,
    schedule: ast::Schedule<'s>,
    roots: &Map<ast::ExprId, crate::law::NodeId>,
    diags: &mut Vec<Diagnostic>,
) -> Option<ScheduleAmount> {
    let (quantity, amount, buys) = match schedule.terms.payment {
        Some(ast::Payment::Fixed(amount)) => {
            let (quantity, amount) = template_amount(world, file, amount, roots, world.book.base, diags)?;
            (quantity, amount, None)
        }
        Some(ast::Payment::Buy { unit, spend }) => {
            let buy_unit = resolve_commodity(world, file, unit, diags)?;
            let (quantity, amount) = template_amount(world, file, spend, roots, world.book.base, diags)?;
            (quantity, amount, Some(buy_unit))
        }
        None => (TemplateQuantity::Derived, Amount::zero(world.book.base), None),
    };
    Some(ScheduleAmount { quantity, amount, buys })
}

impl TermsCx<'_, '_> {
    /// A flow the contract promises between two places: the party is the payee, and the purpose and description
    /// the contract gives its flows are the flow's own until a line says otherwise.
    fn flow(&self, from: Id<Place>, to: Id<Place>, amount: Amount, owner: Id<Entity>, loc: Loc) -> Flow {
        let day = self.anchor;
        Flow {
            day,
            recognized: Days::on(day),
            from,
            to,
            out: amount,
            arrive: amount,
            mode: Mode::Planned,
            infer: Infer::Known,
            txn: TEMPLATE_TXN,
            payee: Some(self.party),
            owner,
            purpose: self.purpose.map(|at| at.value),
            description: self.description,
            origin: Origin::Written,
            select: Run::new(Id::new(0), 0),
            header_codes: Run::new(Id::new(0), 0),
            codes: Run::new(Id::new(0), 0),
            loc,
            waive: None,
            detail: None,
        }
    }
}

fn template_quantity<'s>(
    world: &World<'s>,
    file: &ast::File<'s>,
    quantity: ast::Quantity<'s>,
    roots: &Map<ast::ExprId, crate::law::NodeId>,
    fallback: Id<Commodity>,
    diags: &mut Vec<Diagnostic>,
) -> Option<(TemplateQuantity, Amount)> {
    if let ast::Quantity::Amount(ast::Amount::Computed(expr)) = quantity
        && let ExprKind::Pct(percent) = file.exprs[expr].kind
    {
        let Some(rate) = Ratio::percent(percent.mantissa as i128, percent.scale) else {
            diags.push(
                Diagnostic::error("contract-percentage", "this percentage cannot be represented")
                    .label(file.exprs[expr].loc, "use a smaller percentage"),
            );
            return None;
        };
        if rate.is_negative() || rate > Ratio::ONE {
            diags.push(
                Diagnostic::error("contract-percentage-range", "a split percentage must be between 0% and 100%")
                    .label(file.exprs[expr].loc, "this share would exceed the parent amount"),
            );
            return None;
        }
        return Some((TemplateQuantity::Percent(rate), Amount::zero(fallback)));
    }
    Some(match quantity {
        ast::Quantity::Amount(amount) => {
            let (quantity, amount) = template_amount(world, file, amount, roots, fallback, diags)?;
            (as_quantity(quantity, 0), amount)
        }
        ast::Quantity::Pending(amount) => {
            let (quantity, amount) = template_amount(world, file, amount, roots, fallback, diags)?;
            (as_quantity(quantity, 1), amount)
        }
        ast::Quantity::Target(amount) => {
            let (quantity, amount) = template_amount(world, file, amount, roots, fallback, diags)?;
            (as_quantity(quantity, 2), amount)
        }
        ast::Quantity::Unknown(unit) => {
            let unit = resolve_commodity(world, file, unit, diags)?;
            (TemplateQuantity::Unknown(unit), Amount::zero(unit))
        }
        ast::Quantity::All(unit) => {
            let unit = match unit {
                Some(unit) => Some(resolve_commodity(world, file, unit, diags)?),
                None => None,
            };
            (TemplateQuantity::All(unit), Amount::zero(unit.unwrap_or(fallback)))
        }
        ast::Quantity::Rest => (TemplateQuantity::Rest, Amount::zero(fallback)),
        ast::Quantity::Whole => (TemplateQuantity::Whole, Amount::new(Qty(1), fallback)),
    })
}

fn as_quantity(amount: TemplateQuantity, kind: u8) -> TemplateQuantity {
    let root = match amount {
        TemplateQuantity::Amount(root) => root,
        _ => None,
    };
    match kind {
        1 => TemplateQuantity::Pending(root),
        2 => TemplateQuantity::Target(root),
        _ => TemplateQuantity::Amount(root),
    }
}

fn template_amount<'s>(
    world: &World<'s>,
    file: &ast::File<'s>,
    amount: ast::Amount<'s>,
    roots: &Map<ast::ExprId, crate::law::NodeId>,
    fallback: Id<Commodity>,
    diags: &mut Vec<Diagnostic>,
) -> Option<(TemplateQuantity, Amount)> {
    match amount {
        ast::Amount::Literal(literal) => {
            let amount = world.literal_amount(file, literal, Some(fallback)).or_report(diags)?;
            Some((TemplateQuantity::Amount(None), amount))
        }
        ast::Amount::Computed(root) => {
            let Some(&node) = roots.get(&root) else {
                diags.push(
                    Diagnostic::error("template-root", "a computed template amount was not compiled")
                        .label(file.exprs[root].loc, "this amount has no typed program node"),
                );
                return None;
            };
            Some((TemplateQuantity::Amount(Some(node)), Amount::zero(fallback)))
        }
    }
}

/// One item under a schedule's header: carved from it, added to it or taken from it.
fn lower_header_item<'s>(
    world: &mut World<'s>,
    cx: &TermsCx<'_, 's>,
    header: &Header,
    roots: &Map<ast::ExprId, crate::law::NodeId>,
    item: &ast::LineItem<'s>,
    diags: &mut Vec<Diagnostic>,
) -> Option<TemplateItem> {
    let (file, home) = (cx.file, cx.written.site.home);
    let (parent, side, fallback) = (TemplateItemParent::Header, header.side, header.unit);
    let (quantity, literal) = template_amount(world, file, item.amount, roots, fallback, diags)?;
    let amount = match quantity {
        TemplateQuantity::Amount(Some(root)) => TemplateAmount::Computed(root),
        _ => TemplateAmount::Literal(literal),
    };
    let tail = lower_term_tail(world, home, file, item.tail, diags);
    Some(TemplateItem {
        sign: match item.sign {
            ast::Sign::Carve => crate::book::Sign::Carve,
            ast::Sign::Add => crate::book::Sign::Add,
            ast::Sign::Less => crate::book::Sign::Less,
        },
        parent,
        side,
        amount,
        purpose: tail.purpose.map(|at| at.value),
        description: tail.description,
        codes: tail.codes,
        select: tail.select,
        detail: None,
        waive: tail.waive,
        loc: item.loc,
    })
}

/// What a contract's term line says about the flow it promises.
struct TermTail {
    codes: Run<Sym>,
    select: Run<Select>,
    purpose: Option<At<Purposed>>,
    description: Option<Text>,
    waive: Option<Waive>,
}

/// Reads the clauses a term line keeps: codes, a purpose, a description and a waiver. It takes no others: a line
/// that promises a flow does not say what an occurrence of it says about itself (`for`, `due`, `via`, `basis`,
/// `since`, `against`, `@` and `until`), and what is written there is left unread, as it always has been.
fn lower_term_tail<'s>(
    world: &mut World<'s>,
    home: Home,
    file: &ast::File<'s>,
    tail: axiom_syntax::Many<ast::Clause<'s>>,
    diags: &mut Vec<Diagnostic>,
) -> TermTail {
    let code_start = world.book.codes.len();
    let (mut purpose, mut description, mut waive) = (None, None, None);
    for clause in &file[tail] {
        match clause.kind {
            ClauseKind::Code(code) => {
                let sym = world.book.names.intern(code.name());
                world.book.codes.push(sym);
            }
            ClauseKind::Purpose(written) => {
                if let Some(value) = written_purpose(world, home, file, written, Reach::Parties, diags) {
                    purpose = Some(At { value, loc: clause.at });
                }
            }
            ClauseKind::Description(text) => description = Some(world.book.quoted_text(text.0)),
            ClauseKind::Waive(written) => waive = Some(written_waive(world, written)),
            ClauseKind::Due(_)
            | ClauseKind::For(_)
            | ClauseKind::Via(_)
            | ClauseKind::Basis(_)
            | ClauseKind::Since(_)
            | ClauseKind::Against(_)
            | ClauseKind::Price(_)
            | ClauseKind::Until(_) => {}
        }
    }
    let codes = Run::new(Id::new(code_start as u32), (world.book.codes.len() - code_start) as u32);
    let select = Run::new(Id::new(world.book.selectors.len() as u32), 0);
    TermTail { codes, select, purpose, description, waive }
}

fn resolve_commodity<'s>(
    world: &World<'s>,
    file: &ast::File<'s>,
    name: Name<'s>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Id<Commodity>> {
    world.commodity_of(Word::of(file, name.0)).or_report(diags)
}

fn resolve_endpoint<'s>(
    world: &World<'s>,
    home: Home,
    file: &ast::File<'s>,
    name: Name<'s>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Id<Place>> {
    world.end(home, Word::of(file, name.0)).or_report(diags).map(|end| end.place)
}

fn schedule_owner<'s>(
    world: &World<'s>,
    home: Home,
    file: &ast::File<'s>,
    schedule: Option<ast::Schedule<'s>>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Id<Entity>> {
    let holding = schedule?.terms.holding?;
    let place = resolve_endpoint(world, home, file, holding.name, diags)?;
    Some(world.book.places[place].owner)
}

fn contract_days(
    file: &ast::File<'_>,
    props: axiom_syntax::Many<ast::Prop<'_>>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Days> {
    let mut first = Day::MIN;
    let mut last = Day::MAX;
    let mut valid = true;
    let mut seen = Map::default();
    for prop in &file[props] {
        let (target, what) = match prop.name.0 {
            "from" => (&mut first, "start date"),
            "until" => (&mut last, "end date"),
            _ => continue,
        };
        if let Some(previous) = seen.insert(prop.name.0, prop.loc) {
            diags.push(problem::twice(what, prop.loc, previous));
            valid = false;
            continue;
        }
        let args = &file[prop.args];
        let value = (args.len() == 1).then(|| args[0]).and_then(|id| match file.exprs[id].kind {
            ExprKind::Date(day) => Some(day),
            _ => None,
        });
        if let Some(day) = value {
            *target = day;
        } else {
            valid = false;
            diags.push(
                Diagnostic::error("contract-date", "a contract's `from` and `until` need a date")
                    .label(prop.loc, "write exactly one date here"),
            );
        }
    }
    if !valid {
        return None;
    }
    Days::new(first, last).or_else(|| {
        diags.push(
            Diagnostic::error("contract-range", "a contract ends before it begins")
                .label(file[props].first().map_or(Loc::default(), |prop| prop.loc), "these dates do not overlap"),
        );
        None
    })
}

fn node_doc<'s>(world: &mut World<'s>, site: &Site<'_, 's>, loc: Loc) -> Option<Sym> {
    let item = site.source.file.items.iter().find(|item| item.loc == loc)?;
    item.doc.map(|doc| world.book.names.intern(doc.0))
}

fn span_property(
    file: &ast::File<'_>,
    props: axiom_syntax::Many<ast::Prop<'_>>,
    name: &str,
    diags: &mut Vec<Diagnostic>,
) -> Option<Span> {
    let prop = file[props].iter().find(|prop| prop.name.0 == name)?;
    let args = &file[prop.args];
    let value = (args.len() == 1).then(|| args[0]).and_then(|id| match file.exprs[id].kind {
        ExprKind::Span(span) => Some(span),
        _ => None,
    });
    if value.is_none() {
        diags.push(
            Diagnostic::error("contract-span", format!("`{name}` needs one time span"))
                .label(prop.loc, format!("write `{name} 5d`")),
        );
    }
    value
}

fn grace_property(
    file: &ast::File<'_>,
    props: axiom_syntax::Many<ast::Prop<'_>>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Option<Span>> {
    let mut written = file[props].iter().filter(|prop| prop.name.0 == "grace");
    let Some(first) = written.next() else {
        return Some(None);
    };
    if let Some(second) = written.next() {
        diags.push(problem::twice("grace interval", second.loc, first.loc));
        return None;
    }
    let span = span_property(file, props, "grace", diags)?;
    if span.months < 0 || span.days < 0 {
        diags.push(
            Diagnostic::error("contract-grace", "a grace interval cannot be negative")
                .label(first.loc, "write a nonnegative span such as `5d`"),
        );
        return None;
    }
    Some(Some(span))
}

fn has_property(file: &ast::File<'_>, props: axiom_syntax::Many<ast::Prop<'_>>, name: &str) -> bool {
    file[props].iter().any(|prop| prop.name.0 == name)
}

fn relative_property(
    file: &ast::File<'_>,
    props: axiom_syntax::Many<ast::Prop<'_>>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Relative> {
    let property = file[props].iter().find(|prop| prop.name.0 == "for")?;
    let args = &file[property.args];
    let period = match args {
        [first, second] if matches!(&file.exprs[*first].kind, ExprKind::Name(Name("last"))) => {
            match &file.exprs[*second].kind {
                ExprKind::Name(Name("month")) => Some(Relative::Last(axiom_core::Period::Month)),
                ExprKind::Name(Name("quarter")) => Some(Relative::LastQuarter),
                ExprKind::Name(Name("year")) => Some(Relative::Last(axiom_core::Period::Year)),
                _ => None,
            }
        }
        _ => None,
    };
    if period.is_none() {
        diags.push(
            Diagnostic::error(
                "contract-period",
                "a contract recognition period must be `last month`, `last quarter` or `last year`",
            )
            .label(property.loc, "write one of the supported periods"),
        );
    }
    period
}

fn coverage_property(
    file: &ast::File<'_>,
    props: axiom_syntax::Many<ast::Prop<'_>>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Coverage> {
    let property = file[props].iter().find(|prop| prop.name.0 == "covers")?;
    let args = &file[property.args];
    let coverage = match args {
        [arg] => match &file.exprs[*arg].kind {
            ExprKind::Span(span) => Some(Coverage::Span(*span)),
            _ => None,
        },
        [the, period] if matches!(&file.exprs[*the].kind, ExprKind::Name(Name("the"))) => {
            match &file.exprs[*period].kind {
                ExprKind::Name(Name("month")) => Some(Coverage::Calendar(axiom_core::Period::Month)),
                ExprKind::Name(Name("quarter")) => Some(Coverage::Quarter),
                ExprKind::Name(Name("year")) => Some(Coverage::Calendar(axiom_core::Period::Year)),
                _ => None,
            }
        }
        _ => None,
    };
    if coverage.is_none() {
        diags.push(
            Diagnostic::error("contract-covers", "`covers` needs a span, month, quarter or year")
                .label(property.loc, "write `covers 1y` or `covers the year`"),
        );
    }
    coverage
}

fn escalation_property<'s>(
    world: &World<'s>,
    home: Home,
    file: &ast::File<'s>,
    props: axiom_syntax::Many<ast::Prop<'s>>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Escalation> {
    for prop in &file[props] {
        if !matches!(prop.name.0, "rising" | "indexed") {
            continue;
        }
        let args = &file[prop.args];
        let Some(&first) = args.first() else {
            diags.push(
                Diagnostic::error("contract-escalation", "an escalation needs a rate or index")
                    .label(prop.loc, "write `rising 3%` or `indexed to cpi`"),
            );
            continue;
        };
        match prop.name.0 {
            "rising" => {
                if args.len() != 2 || !matches!(file.exprs[args[1]].kind, ExprKind::Name(Name("yearly"))) {
                    diags.push(
                        Diagnostic::error("contract-rate", "a yearly rise takes one percentage")
                            .label(prop.loc, "write `rising 3%`"),
                    );
                    continue;
                }
                let ExprKind::Pct(percent) = file.exprs[first].kind else {
                    diags.push(
                        Diagnostic::error("contract-rate", "a yearly rise must be a percentage")
                            .label(file.exprs[first].loc, "write a percentage such as `3%`"),
                    );
                    continue;
                };
                let rate = percent.to_ratio().and_then(|rate| rate.checked_div(Ratio::new(100, 1)?));
                if let Some(rate) = rate.filter(|rate| !rate.is_negative()) {
                    return Some(Escalation::Rising(rate));
                }
                diags.push(
                    Diagnostic::error("contract-rate", "a contract's yearly rise must be a finite percentage")
                        .label(file.exprs[first].loc, "this rate cannot be represented"),
                );
            }
            "indexed" => {
                let index = match (args.len(), &file.exprs[first].kind) {
                    (3, ExprKind::Name(Name("to")))
                        if matches!(&file.exprs[args[2]].kind, ExprKind::Name(Name("yearly"))) =>
                    {
                        match &file.exprs[args[1]].kind {
                            ExprKind::Name(name) => Some(*name),
                            _ => None,
                        }
                    }
                    _ => None,
                };
                if let Some(name) = index {
                    let word = Word::of(file, name.0);
                    match world.seek_param(home, word) {
                        Ok(Some(param)) => return Some(Escalation::Indexed(param)),
                        Ok(None) => diags.push(world.missing_param(home, word)),
                        Err(problem) => diags.push(problem),
                    }
                } else {
                    diags.push(
                        Diagnostic::error("contract-index", "an index escalation needs one parameter")
                            .label(prop.loc, "write `indexed to cpi yearly`"),
                    );
                }
            }
            _ => {}
        }
    }
    None
}

fn contract_area<'s>(
    world: &mut World<'s>,
    file: &ast::File<'s>,
    props: axiom_syntax::Many<ast::Prop<'s>>,
    diags: &mut Vec<Diagnostic>,
) -> Result<Option<Amount>, ()> {
    let mut area = None;
    let mut first_loc = None;
    for prop in &file[props] {
        if prop.name.0 != "area" {
            continue;
        }
        if let Some(first) = first_loc {
            diags.push(problem::twice("area", prop.loc, first));
            return Err(());
        }
        first_loc = Some(prop.loc);
        let args = &file[prop.args];
        if args.len() != 1 || !file[prop.lines].is_empty() {
            diags.push(
                Diagnostic::error("contract-area", "a contract area needs one literal measure")
                    .label(prop.loc, "write `area 1_000 SQFT`"),
            );
            return Err(());
        }
        let expression = args[0];
        let ExprKind::Amount(literal) = file.exprs[expression].kind else {
            diags.push(
                Diagnostic::error("contract-area", "a contract area needs a literal amount")
                    .label(file.exprs[expression].loc, "write a positive measure such as `1_000 SQFT`"),
            );
            return Err(());
        };
        let Some(unit_name) = literal.unit() else {
            diags.push(
                Diagnostic::error("contract-area-unit", "a contract area needs a measure unit")
                    .label(file.exprs[expression].loc, "write the unit after the area"),
            );
            return Err(());
        };
        let Some(unit) = world.commodity_of(Word::of(file, unit_name.0)).or_report(diags) else {
            return Err(());
        };
        if !world.book.is_a(world.book.commodities[unit].kind, world.book.roots.kinds.measure) {
            diags.push(
                Diagnostic::error("contract-area-unit", "a contract area must use a measure unit")
                    .label(file.loc(unit_name.0), "this commodity is not a measure"),
            );
            return Err(());
        }
        let amount = match world.amount(literal.num(), unit, file.exprs[expression].loc) {
            Ok(amount) if amount.qty.0 > 0 => amount,
            Ok(_) => {
                diags.push(
                    Diagnostic::error("contract-area-positive", "a contract area must be positive")
                        .label(file.exprs[expression].loc, "write an area greater than zero"),
                );
                return Err(());
            }
            Err(problem) => {
                diags.push(problem);
                return Err(());
            }
        };
        area = Some(amount);
    }
    Ok(area)
}

/// Where a contract's deposit may be kept: by whom, and in the holding its schedule has, for a deposit that names
/// none.
#[derive(Clone, Copy)]
struct Keeping<'s> {
    owner: Id<Entity>,
    default_holding: Option<(ast::Name<'s>, Loc)>,
}

fn contract_deposit<'s>(
    world: &mut World<'s>,
    contract: WrittenContract<'_, 's>,
    keeping: Keeping<'s>,
    diags: &mut Vec<Diagnostic>,
) -> Result<Option<(Amount, Id<Place>)>, ()> {
    let (file, owner) = (contract.file(), keeping.owner);
    let mut deposit = None;
    let mut first_loc = None;
    for prop in file[contract.node.props].iter().filter(|prop| prop.name.0 == "deposit") {
        if let Some(first) = first_loc {
            diags.push(problem::twice("deposit", prop.loc, first));
            return Err(());
        }
        first_loc = Some(prop.loc);
        let args = &file[prop.args];
        if !file[prop.lines].is_empty() || !(args.len() == 1 || args.len() == 3) {
            diags.push(
                Diagnostic::error("contract-deposit", "a deposit needs one amount and an optional holding")
                    .label(prop.loc, "write `deposit AMOUNT [into HOLDING]`"),
            );
            return Err(());
        }
        let amount = deposit_amount(world, file, args[0], owner, diags)?;
        let place = deposit_holding(world, contract, prop, keeping, amount, diags)?;
        deposit = Some((amount, place));
    }
    Ok(deposit)
}

/// The positive literal amount a contract deposit is, in the owner's currency unless it names a unit.
fn deposit_amount<'s>(
    world: &World<'s>,
    file: &ast::File<'s>,
    expr: ast::ExprId,
    owner: Id<Entity>,
    diags: &mut Vec<Diagnostic>,
) -> Result<Amount, ()> {
    let ExprKind::Amount(literal) = file.exprs[expr].kind else {
        diags.push(
            Diagnostic::error("contract-deposit-amount", "a deposit must be a literal amount")
                .label(file.exprs[expr].loc, "write the amount the contract will hold"),
        );
        return Err(());
    };
    let currency = Some(world.book.currency(owner));
    match world.literal_amount(file, literal, currency).or_report(diags) {
        Some(amount) if amount.qty.0 > 0 => Ok(amount),
        Some(_) => {
            diags.push(
                Diagnostic::error("contract-deposit-positive", "a contract deposit must be positive")
                    .label(file.exprs[expr].loc, "write an amount greater than zero"),
            );
            Err(())
        }
        None => Err(()),
    }
}

/// The name of the account a deposit is kept in, and where it is written: the one after `into`, or the schedule's.
fn holding_name<'s>(
    file: &ast::File<'s>,
    prop: &ast::Prop<'s>,
    default_holding: Option<(ast::Name<'s>, Loc)>,
    diags: &mut Vec<Diagnostic>,
) -> Result<(ast::Name<'s>, Loc), ()> {
    let args = &file[prop.args];
    if args.len() != 3 {
        return default_holding.ok_or_else(|| {
            diags.push(
                Diagnostic::error("contract-deposit-holding-required", "a deposit needs a holding account")
                    .label(prop.loc, "name `into HOLDING` or give this contract an active schedule with a holding"),
            );
        });
    }
    let into = matches!(file.exprs[args[1]].kind, ExprKind::Name(name) if name.0 == "into");
    let ExprKind::Name(name) = file.exprs[args[2]].kind else {
        diags.push(
            Diagnostic::error("contract-deposit-holding", "a deposit holding needs a place name")
                .label(file.exprs[args[2]].loc, "name the account or holding that keeps the deposit"),
        );
        return Err(());
    };
    if !into {
        diags.push(
            Diagnostic::error("contract-deposit-holding", "name a deposit holding after `into`")
                .label(file.exprs[args[1]].loc, "write `into` here"),
        );
        return Err(());
    }
    Ok((name, file.exprs[args[2]].loc))
}

/// The account that keeps a deposit: the one named after `into`, else the holding of the contract's schedule,
/// which must be an account of the owner's that accepts the unit.
fn deposit_holding<'s>(
    world: &World<'s>,
    contract: WrittenContract<'_, 's>,
    prop: &ast::Prop<'s>,
    keeping: Keeping<'s>,
    amount: Amount,
    diags: &mut Vec<Diagnostic>,
) -> Result<Id<Place>, ()> {
    let (file, home) = (contract.file(), contract.home());
    let (name, name_loc) = holding_name(file, prop, keeping.default_holding, diags)?;
    let place = resolve_endpoint(world, home, file, name, diags).ok_or(())?;
    let kept = &world.book.places[place];
    if !matches!(kept.role, Role::Account { .. } | Role::Holding(_)) {
        diags.push(
            Diagnostic::error("contract-deposit-holding", "a deposit is held in an account")
                .label(name_loc, "choose an account or holding, not an asset or party"),
        );
        return Err(());
    }
    if kept.owner != keeping.owner {
        diags.push(
            Diagnostic::error("contract-deposit-owner", "the deposit holding belongs to another owner")
                .label(name_loc, "choose a holding owned by the contract owner")
                .context(kept.loc.unwrap_or(name_loc), "this place is declared here"),
        );
        return Err(());
    }
    if world.book.holds(place).is_some_and(|mut units| !units.any(|unit| unit == amount.unit)) {
        diags.push(
            Diagnostic::error("contract-deposit-unit", "the deposit holding does not accept this unit")
                .label(file.exprs[file[prop.args][0]].loc, "choose a unit the holding can keep"),
        );
        return Err(());
    }
    Ok(place)
}

/// The shares a contract divides what it brings in by: each `share RATE for ENTITY`, as a percentage, a fraction or
/// a measure of the area the contract or the asset it is about has.
fn shares<'a, 's>(world: &World<'s>, cx: &TermsCx<'a, 's>, diags: &mut Vec<Diagnostic>) -> Vec<Share> {
    let file = cx.file;
    let mut shares = Vec::new();
    let mut total = Ratio::ZERO;
    for prop in file[cx.written.node.props].iter().filter(|prop| prop.name.0 == "share") {
        if file[prop.args].is_empty() {
            diags.push(
                Diagnostic::error("contract-share", "a contract share needs an amount")
                    .label(prop.loc, "write `share 60% for ENTITY`"),
            );
            continue;
        }
        read_share_line(world, cx, prop, &mut total, &mut shares, diags);
    }
    shares
}

/// The `RATE for ENTITY` pairs of one `share` line, which end at the first that is wrong.
fn read_share_line<'a, 's>(
    world: &World<'s>,
    cx: &TermsCx<'a, 's>,
    prop: &ast::Prop<'s>,
    total: &mut Ratio,
    shares: &mut Vec<Share>,
    diags: &mut Vec<Diagnostic>,
) {
    let file = cx.file;
    let args = &file[prop.args];
    let mut at = 0;
    while at < args.len() {
        let written_amount = args[at];
        at += 1;
        let (rate, measure) = share_rate(world, cx, written_amount, diags);
        let Some(rate) = rate.filter(|rate| !rate.is_negative()) else {
            diags.push(
                Diagnostic::error("contract-share", "a share must be a nonnegative percentage, fraction, or measure")
                    .label(file.exprs[written_amount].loc, "write `60%`, `3/5` or `120 SQFT`"),
            );
            return;
        };
        let Some(owner) = share_owner(file, prop, &mut at, diags) else {
            return;
        };
        let Some(next_total) = add_share(*total, rate, prop.loc, diags) else {
            return;
        };
        *total = next_total;
        if let Some(entity) = world.entity(cx.written.site.home, Word::of(file, owner.0)).or_report(diags) {
            shares.push(Share { rate, entity, measure, loc: prop.loc });
        }
    }
}

/// What a share is of the whole, and for a measure the two amounts it is the ratio of.
fn share_rate<'a, 's>(
    world: &World<'s>,
    cx: &TermsCx<'a, 's>,
    expr: ast::ExprId,
    diags: &mut Vec<Diagnostic>,
) -> (Option<Ratio>, Option<(Amount, Amount)>) {
    match cx.file.exprs[expr].kind {
        ExprKind::Pct(percent) => (percent.to_ratio().and_then(|rate| rate.checked_div(Ratio::new(100, 1)?)), None),
        ExprKind::Fraction(top, bottom) => (Ratio::new(i128::from(top), i128::from(bottom)), None),
        ExprKind::Amount(literal) => {
            let numerator = measured_numerator(world, cx.file, literal, expr, diags);
            let denominator = cx.area.or_else(|| {
                cx.purpose.and_then(|at| at.value.of).and_then(|object| match object {
                    crate::journal::Object::Asset(asset) => asset_area(world, asset, cx.anchor),
                    _ => None,
                })
            });
            let ratio = match (numerator, denominator) {
                (Some(numerator), Some(denominator)) if numerator.unit == denominator.unit && denominator.qty.0 > 0 => {
                    Ratio::new(i128::from(numerator.qty.0), i128::from(denominator.qty.0))
                }
                _ => {
                    diags.push(
                        Diagnostic::error(
                            "contract-share-measure",
                            "a measured share needs a positive contract or asset area in the same unit",
                        )
                        .label(cx.file.exprs[expr].loc, "cannot resolve this measure"),
                    );
                    None
                }
            };
            (ratio, numerator.zip(denominator))
        }
        _ => (None, None),
    }
}

/// `120 SQFT`: the measured amount a share is of an area, which must be in a measure unit.
fn measured_numerator<'s>(
    world: &World<'s>,
    file: &ast::File<'s>,
    literal: ast::Literal<'s>,
    expr: ast::ExprId,
    diags: &mut Vec<Diagnostic>,
) -> Option<Amount> {
    let name = literal.unit()?;
    let unit = world.commodity_of(Word::of(file, name.0)).or_report(diags)?;
    if !world.book.is_a(world.book.commodities[unit].kind, world.book.roots.kinds.measure) {
        diags.push(
            Diagnostic::error("contract-share-unit", "a measured share must use a measure unit")
                .label(file.loc(name.0), "this commodity is not a measure"),
        );
        return None;
    }
    world.amount(literal.num(), unit, file.exprs[expr].loc).or_report(diags)
}

/// The `for ENTITY` after a share's rate: the name of the entity, `at` moved past it.
fn share_owner<'s>(
    file: &ast::File<'s>,
    prop: &ast::Prop<'s>,
    at: &mut usize,
    diags: &mut Vec<Diagnostic>,
) -> Option<Name<'s>> {
    let args = &file[prop.args];
    let Some(for_word) = args.get(*at).copied() else {
        diags.push(
            Diagnostic::error("contract-share", "a contract share needs an owner")
                .label(prop.loc, "write `share RATE for ENTITY`"),
        );
        return None;
    };
    *at += 1;
    if !matches!(file.exprs[for_word].kind, ExprKind::Name(Name("for"))) {
        diags.push(
            Diagnostic::error("contract-share", "a share amount must be followed by `for ENTITY`")
                .label(file.exprs[for_word].loc, "expected `for` here"),
        );
        return None;
    }
    let Some(owner_expr) = args.get(*at).copied() else {
        diags.push(
            Diagnostic::error("contract-share", "a contract share needs an owner")
                .label(prop.loc, "write an entity after `for`"),
        );
        return None;
    };
    *at += 1;
    let ExprKind::Name(owner) = file.exprs[owner_expr].kind else {
        diags.push(
            Diagnostic::error("contract-share", "a share owner must be an entity name")
                .label(file.exprs[owner_expr].loc, "write the owner here"),
        );
        return None;
    };
    Some(owner)
}

/// The total of the shares with this one in it, or nothing after it is said that it is too much.
fn add_share(total: Ratio, rate: Ratio, loc: Loc, diags: &mut Vec<Diagnostic>) -> Option<Ratio> {
    let Some(next) = total.checked_add(rate) else {
        diags.push(
            Diagnostic::error("contract-share-total", "contract shares exceed exact arithmetic")
                .label(loc, "reduce the declared shares"),
        );
        return None;
    };
    if next.checked_sub(Ratio::ONE).is_some_and(|excess| !excess.is_negative() && !excess.is_zero()) {
        diags.push(
            Diagnostic::error("contract-share-total", "contract shares add up to more than 100%")
                .label(loc, "the total shares cannot exceed 100%"),
        );
        return None;
    }
    Some(next)
}

fn asset_area(world: &World<'_>, asset: Id<Asset>, day: Day) -> Option<Amount> {
    let area = world.book.names.get("area")?;
    match world.book.said(asset, area, day)? {
        crate::law::Value::Amount(amount) => Some(amount),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::positive_loan_term;
    use axiom_core::Span;

    #[test]
    fn loan_term_requires_nonnegative_components_and_positive_total() {
        assert!(positive_loan_term(Span::months(360)));
        assert!(positive_loan_term(Span::days(30)));
        assert!(!positive_loan_term(Span::default()));
        assert!(!positive_loan_term(Span { months: 1, days: -2 }));
        assert!(!positive_loan_term(Span { months: -1, days: 32 }));
    }
}
