//! S5 contract declarations are lowered in two passes. Names and ids exist
//! before any template expression compiles, so a template may mention a
//! contract declared later in the project.

use axiom_core::{Day, Days, Diagnostic, Dim, Id, Loc, Map, Qty, Ratio, Run, Span, Sym, Timeline};
use axiom_syntax as ast;
use axiom_syntax::{BinOp, ClauseKind, Direction, ExprKind, ItemKind, Name};

use super::{JournalSurvey, compile_roots, contract_roots, inputs};
use crate::book::{
    AlsoOn, Amount, At, Cadence, Class, Contract, Coverage, Deadline, Escalation, FlowSide, Input, Loan, Prepay,
    Relative, Reset, Share, TemplateAmount, TemplateFlow, TemplateItem, TemplateItemParent, TemplateLeg,
    TemplateProgram, TemplateQuantity, Terms, TermsState,
};
use crate::declare::World;
use crate::errors::{Word, duplicate};
use crate::journal::{Detail, Flow, Infer, Mode, Origin, Provenance, Purposed, Select, TEMPLATE_TXN, Waive};
use crate::law::{Owner, Ty};
use crate::scope::Home;
use crate::sources::Site;

#[derive(Clone, Copy)]
struct WrittenContract<'a, 's> {
    site: &'a Site<'a, 's>,
    node: &'a ast::Contract<'s>,
    id: Id<Contract>,
    name: Sym,
    loc: Loc,
}

/// Reserves every contract id before compiling any contract body. Terms and
/// their computed roots are then compiled against the complete contract
/// namespace, while occurrences are handled by [`super::record`].
pub(crate) fn contracts<'a, 's>(
    world: &mut World<'s>,
    sites: &'a [Site<'a, 's>],
    _survey: &JournalSurvey<'s>,
    diags: &mut Vec<Diagnostic>,
) {
    let mut written = Vec::new();
    for site in sites {
        let file = &site.source.file;
        for item in &file.items {
            let ItemKind::Contract(reference) = item.kind else {
                continue;
            };
            let node = &file[reference];
            let name = world.book.names.intern(node.name.0);
            if let Some(first) = world.book.lookup.contracts.get(&name).copied() {
                diags.push(duplicate(
                    "contract",
                    Word { text: node.name.0, loc: file.loc(node.name.0) },
                    Some(world.book.contracts[first].loc),
                    None,
                ));
                continue;
            }
            let id = world.book.contracts.push(empty_contract(name, item.loc, world.book.roots.me));
            world.book.lookup.contracts.insert(name, id);
            written.push(WrittenContract { site, node, id, name, loc: item.loc });
        }
    }

    for written in written.iter().copied() {
        let file = &written.site.source.file;
        if let Some(contract) = lower_contract(world, written, file, diags) {
            world.book.contracts[written.id] = contract;
        }
    }

    // Nested contract laws follow their parent contract's declaration, after
    // its id exists and before the shared law index is finalized.
    for written in written.iter().copied() {
        let file = &written.site.source.file;
        let mut laws = Vec::new();
        for law in &file[written.node.laws] {
            if let Some(id) = crate::laws::compile_native(
                world,
                diags,
                file,
                written.site.home,
                Owner::Contract(written.id),
                Ty::Flow,
                law,
            ) {
                laws.push(id);
            }
        }
        world.book.contracts[written.id].laws = laws.into_boxed_slice();
    }
}

fn empty_contract(name: Sym, loc: Loc, me: axiom_core::Id<crate::book::Entity>) -> Contract {
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

fn lower_contract<'a, 's>(
    world: &mut World<'s>,
    written: WrittenContract<'a, 's>,
    file: &ast::File<'s>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Contract> {
    let node = written.node;
    let name_word = Word { text: node.name.0, loc: file.loc(node.name.0) };
    let party_word = node.party.map_or(name_word, |party| Word { text: party.0, loc: file.loc(party.0) });
    let party = match world.entity(written.site.home, party_word) {
        Ok(party) => party,
        Err(problem) => {
            diags.push(problem);
            return None;
        }
    };
    let days = contract_days(file, node.props, diags)?;
    let area = match contract_area(world, file, node.props, diags) {
        Ok(area) => area,
        Err(()) => return None,
    };
    let anchor = days.first();
    let purpose = if let Some(purpose) = node.purpose {
        let purpose_word = Word { text: purpose.name.0, loc: file.loc(purpose.name.0) };
        match world.purpose(written.site.home, purpose_word) {
            Ok(id) => Some(At {
                value: Purposed {
                    purpose: id,
                    of: purpose.of.and_then(|object| resolve_object(world, written.site.home, file, object, diags)),
                    source: Provenance::Contract(written.id),
                },
                loc: purpose_word.loc,
            }),
            Err(problem) => {
                diags.push(problem);
                return None;
            }
        }
    } else {
        None
    };
    let description = node.description.map(|description| world.book.quoted_text(description.0));
    let contract_inputs = inputs(world, file, node.props, diags);
    let roots = contract_roots(file, node);
    let (regular, standing) = (
        compile_roots(world, file, written.site.home, Ty::Flow, written.name, &contract_inputs, &roots.regular, diags),
        compile_roots(world, file, written.site.home, Ty::Flow, written.name, &contract_inputs, &roots.standing, diags),
    );

    let owner = match node.schedule.or(node.standing) {
        Some(schedule) => schedule_owner(world, written.site.home, file, Some(schedule), party, diags)?,
        None => world.book.roots.me,
    };
    let deposit = contract_deposit(
        world,
        file,
        node.props,
        written.site.home,
        owner,
        node.schedule
            .or(node.standing)
            .and_then(|schedule| schedule.terms.holding.map(|holding| (holding.name, schedule.at))),
        diags,
    )
    .ok()?;
    let also = crate::laws::lower_alsos(
        world,
        diags,
        file,
        written.site.home,
        node.alsos,
        Owner::Contract(written.id),
        AlsoOn::Contract(written.id),
        &contract_inputs,
        world.book.entities[owner].currency,
    );
    let loan = contract_loan(world, file, node.props, party, owner, written.site.home, diags)?;
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

    if let (Some(schedule), Some((program, ids))) = (node.schedule, regular) {
        let terms = lower_terms(
            world,
            written,
            file,
            schedule,
            node.body,
            node.deadline.as_ref(),
            &contract_inputs,
            program,
            ids,
            anchor,
            party,
            purpose,
            description,
            area,
            loan.map(|(_, rate)| rate),
            &also,
            diags,
        )?;
        contract.terms = Some(Timeline::new(terms));
    }
    if let (Some(schedule), Some((program, ids))) = (node.standing, standing) {
        let terms = lower_terms(
            world,
            written,
            file,
            schedule,
            node.body,
            node.deadline.as_ref(),
            &contract_inputs,
            program,
            ids,
            anchor,
            party,
            purpose,
            description,
            area,
            loan.map(|(_, rate)| rate),
            &also,
            diags,
        )?;
        contract.standing = Some(Timeline::new(terms));
    }
    Some(contract)
}

fn contract_loan<'s>(
    world: &World<'s>,
    file: &ast::File<'s>,
    props: ast::Many<ast::Prop<'s>>,
    party: axiom_core::Id<crate::book::Entity>,
    owner: axiom_core::Id<crate::book::Entity>,
    home: Home,
    diags: &mut Vec<Diagnostic>,
) -> Option<Option<(Loan, Ratio)>> {
    let mut written = file[props].iter().filter(|prop| prop.name.0 == "loan");
    let Some(prop) = written.next() else {
        return Some(None);
    };
    if let Some(duplicate) = written.next() {
        diags.push(
            Diagnostic::error("duplicate-contract-loan", "a contract has one loan definition")
                .label(duplicate.loc, "a second loan cannot replace the first")
                .context(prop.loc, "the first loan is here"),
        );
        return None;
    }

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
    let principal = match file.exprs[principal_expr].kind {
        ExprKind::Amount(literal) => {
            let unit = match literal.unit() {
                Some(unit) => match world.commodity_of(Word { text: unit.0, loc: file.loc(unit.0) }) {
                    Ok(unit) => unit,
                    Err(problem) => {
                        diags.push(problem);
                        return None;
                    }
                },
                None => world.book.base,
            };
            match world.amount(literal.num(), unit, file.exprs[principal_expr].loc) {
                Ok(amount) if amount.qty.0 > 0 => amount,
                Ok(_) => {
                    diags.push(
                        Diagnostic::error("contract-loan-principal", "a loan principal must be positive")
                            .label(file.exprs[principal_expr].loc, "this amount is not positive"),
                    );
                    return None;
                }
                Err(problem) => {
                    diags.push(problem);
                    return None;
                }
            }
        }
        _ => {
            diags.push(
                Diagnostic::error("contract-loan-principal", "a loan principal must be a literal amount")
                    .label(file.exprs[principal_expr].loc, "write an amount such as `3_000 USD`"),
            );
            return None;
        }
    };
    if args.len() != 7 && args.len() != 9 || !keyword(1, "on") || !keyword(3, "at") || !keyword(5, "over") {
        diags.push(
            Diagnostic::error("contract-loan", "the loan definition has missing or extra fields")
                .label(prop.loc, "write `loan AMOUNT on DATE at RATE over SPAN [for ASSET]`"),
        );
        return None;
    }
    let on = match file.exprs[args[2]].kind {
        ExprKind::Date(day) => day,
        _ => {
            diags.push(
                Diagnostic::error("contract-loan-date", "a loan start needs a date")
                    .label(file.exprs[args[2]].loc, "write the date after `on`"),
            );
            return None;
        }
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
    let term = match file.exprs[args[6]].kind {
        ExprKind::Span(span) if positive_loan_term(span) => span,
        _ => {
            diags.push(
                Diagnostic::error("contract-loan-term", "a loan term must be a positive span")
                    .label(file.exprs[args[6]].loc, "write a term such as `30y`"),
            );
            return None;
        }
    };
    let asset = if args.len() == 9 {
        if !keyword(7, "for") {
            diags.push(
                Diagnostic::error("contract-loan-asset", "a financed asset follows `for`")
                    .label(file.exprs[args[7]].loc, "write `for ASSET` here"),
            );
            return None;
        }
        let name = match file.exprs[args[8]].kind {
            ExprKind::Name(name) => name.0,
            _ => {
                diags.push(
                    Diagnostic::error("contract-loan-asset", "a financed asset needs a name")
                        .label(file.exprs[args[8]].loc, "write the declared asset name"),
                );
                return None;
            }
        };
        match world.book.asset(name) {
            Some(asset) => Some(asset),
            None => {
                diags.push(
                    Diagnostic::error("contract-loan-asset", format!("asset `{name}` is not declared"))
                        .label(file.exprs[args[8]].loc, "declare this asset before the loan"),
                );
                return None;
            }
        }
    } else {
        None
    };

    let resets = loan_resets(world, home, file, prop.lines, on, diags)?;
    let mut prepay = Prepay::Shortens;
    let mut prepay_loc = None;
    for nested in &file[prop.lines] {
        let nested = &nested.0;
        match nested.name.0 {
            "prepay" => {
                if let Some(first) = prepay_loc {
                    diags.push(
                        Diagnostic::error("duplicate-loan-prepay", "a loan has one prepayment rule")
                            .label(nested.loc, "a second rule cannot replace the first")
                            .context(first, "the first rule is here"),
                    );
                    return None;
                }
                prepay_loc = Some(nested.loc);
                let value = file[nested.args].first().and_then(|&id| match file.exprs[id].kind {
                    ExprKind::Name(name) if name.0 == "shortens" => Some(Prepay::Shortens),
                    ExprKind::Name(name) if name.0 == "recasts" => Some(Prepay::Recasts),
                    _ => None,
                });
                if file[nested.args].len() != 1 || value.is_none() {
                    diags.push(
                        Diagnostic::error("contract-loan-prepay", "prepay must be `shortens` or `recasts`")
                            .label(nested.loc, "write exactly one supported prepayment rule"),
                    );
                    return None;
                }
                prepay = value.unwrap();
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
    let debt = world.tab(party, owner, Class::Debt, prop.loc).map_err(|problem| diags.push(problem)).ok()?;
    Some(Some((Loan { principal, on, term, asset, debt, resets, prepay }, rate)))
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
        diags.push(
            Diagnostic::error("duplicate-loan-resets", "a loan has one reset rule")
                .label(second.0.loc, "a second reset cannot replace the first")
                .context(first.0.loc, "the first reset is here"),
        );
        return None;
    }

    let nested = &first.0;
    let args = &file[nested.args];
    let mut cursor = 0;
    let Some(&every_expr) = args.get(cursor) else {
        return invalid_reset(diags, nested.loc, "write `resets 1y from DATE to PARAM + PERCENT`");
    };
    let every = match file.exprs[every_expr].kind {
        ExprKind::Span(span) if positive_loan_term(span) => span,
        _ => return invalid_reset(diags, file.exprs[every_expr].loc, "the reset interval must be a positive span"),
    };
    cursor += 1;

    if !reset_keyword(file, args, cursor, "from") {
        return invalid_reset(diags, nested.loc, "write `from DATE` after the reset interval");
    }
    cursor += 1;
    let Some(&from_expr) = args.get(cursor) else {
        return invalid_reset(diags, nested.loc, "write the first reset date after `from`");
    };
    let from = match file.exprs[from_expr].kind {
        ExprKind::Date(day) if day >= loan_on => day,
        ExprKind::Date(_) => {
            return invalid_reset(diags, file.exprs[from_expr].loc, "the first reset cannot precede the loan");
        }
        _ => return invalid_reset(diags, file.exprs[from_expr].loc, "write a full date for the first reset"),
    };
    cursor += 1;

    if !reset_keyword(file, args, cursor, "to") {
        return invalid_reset(diags, nested.loc, "write `to PARAM + PERCENT` after the reset date");
    }
    cursor += 1;
    let Some(&rate_expr) = args.get(cursor) else {
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
    let word = Word { text: index_name.0, loc: file.loc(index_name.0) };
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
    cursor += 1;

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

    Some(Some(Reset { every, from, index, margin, cap, life }))
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

fn lower_terms<'a, 's>(
    world: &mut World<'s>,
    written: WrittenContract<'a, 's>,
    file: &ast::File<'s>,
    schedule: ast::Schedule<'s>,
    body: ast::Body<'s>,
    deadline: Option<&ast::Deadline<'s>>,
    inputs: &[Input],
    program: TemplateProgram,
    roots: Map<ast::ExprId, crate::law::NodeId>,
    anchor: Day,
    party: axiom_core::Id<crate::book::Entity>,
    purpose: Option<At<Purposed>>,
    description: Option<crate::book::Text>,
    contract_area: Option<Amount>,
    loan_rate: Option<Ratio>,
    also: &[Id<crate::book::Also>],
    diags: &mut Vec<Diagnostic>,
) -> Option<Terms> {
    let hold = schedule.terms.holding?;
    let holding = resolve_endpoint(world, written.site.home, file, hold.name, schedule.at, diags)?;
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
    let ((out_quantity, arrive_quantity), header_amount, buys) = schedule_amount(world, file, schedule, &roots, diags)?;
    let mut header_flow = template_flow(
        anchor,
        from,
        to,
        header_amount,
        owner,
        Some(party),
        purpose.map(|at| at.value),
        description,
        schedule.at,
    );
    let (from_party, to_party) = match hold.direction {
        Direction::From => (None, Some(party)),
        Direction::Into => (Some(party), None),
    };
    header_flow.purpose = super::record::infer_for_flow(
        world,
        from,
        from_party,
        to,
        to_party,
        purpose.map(|at| (at.value, at.loc)),
        schedule.at,
        diags,
    )
    .ok()?;
    let mut legs = Vec::new();
    for leg in &file[body.legs] {
        let endpoint = resolve_endpoint(world, written.site.home, file, leg.end.name, leg.loc, diags)?;
        // A promised split leg names the recipient. Keep the source end of
        // the scheduled header and send that portion to the named endpoint:
        // an employer's paycheck leg is `lumen -> retirement`, and an owner
        // payment leg is `checking -> escrow`.
        let leg_from = from;
        let leg_to = endpoint;
        let (quantity, amount) = template_quantity(world, file, leg.amount, &roots, header_amount.unit, diags)?;
        let mut flow = template_flow(
            anchor,
            leg_from,
            leg_to,
            amount,
            owner,
            Some(party),
            purpose.map(|at| at.value),
            description,
            leg.loc,
        );
        let (codes, selectors, detail, waive, leg_purpose, leg_description) =
            lower_tail(world, written.site.home, file, leg.tail, leg.loc, diags);
        flow.codes = codes;
        flow.select = selectors;
        flow.detail = detail;
        flow.waive = waive;
        flow.purpose = super::record::infer_for_flow(
            world,
            leg_from,
            from_party,
            leg_to,
            None,
            leg_purpose.or_else(|| purpose.map(|at| At { value: at.value, loc: at.loc })).map(|at| (at.value, at.loc)),
            leg.loc,
            diags,
        )
        .ok()?;
        flow.description = leg_description.or(flow.description);
        legs.push(TemplateLeg { flow, side, quantity });
    }
    let mut items = Vec::new();
    for item in &file[body.items] {
        if let Some(item) = lower_item(
            world,
            written.site.home,
            file,
            item,
            TemplateItemParent::Header,
            side,
            header_amount.unit,
            &roots,
            diags,
        ) {
            items.push(item);
        }
    }
    let template = TemplateFlow {
        flow: header_flow.clone(),
        out: out_quantity,
        arrive: match buys {
            Some(unit) => TemplateQuantity::Unknown(unit),
            None => arrive_quantity,
        },
        legs: legs.into_boxed_slice(),
        items: items.into_boxed_slice(),
    };

    let due = deadline.map(|deadline| Deadline {
        after: deadline.span,
        otherwise: deadline.otherwise.as_ref().and_then(|item| {
            lower_item(
                world,
                written.site.home,
                file,
                item,
                TemplateItemParent::Header,
                side,
                header_amount.unit,
                &roots,
                diags,
            )
        }),
    });
    let every = match schedule.terms.cadence {
        ast::Cadence::Every(span) => Cadence::Every(span),
        ast::Cadence::TwiceMonthly => Cadence::TwiceMonthly,
    };
    let grace = grace_property(file, written.node.props, diags)?;
    Some(Terms {
        state: TermsState::Active,
        every,
        on: file[schedule.terms.on].to_vec().into_boxed_slice(),
        anchor,
        template: Box::new([template]),
        program,
        inputs: inputs.to_vec().into_boxed_slice(),
        estimate: schedule.terms.about,
        due,
        grace,
        period: relative_property(file, written.node.props, diags),
        covers: coverage_property(file, written.node.props, diags),
        prorated: has_property(file, written.node.props, "prorated"),
        escalation: escalation_property(world, written.site.home, file, written.node.props, diags),
        shares: shares(world, written.site.home, file, written.node.props, purpose, contract_area, anchor, diags)
            .into_boxed_slice(),
        also: also.to_vec().into_boxed_slice(),
        rate: loan_rate,
        change: None,
    })
}

fn schedule_amount<'s>(
    world: &World<'s>,
    file: &ast::File<'s>,
    schedule: ast::Schedule<'s>,
    roots: &Map<ast::ExprId, crate::law::NodeId>,
    diags: &mut Vec<Diagnostic>,
) -> Option<((TemplateQuantity, TemplateQuantity), Amount, Option<Id<crate::book::Commodity>>)> {
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
    Some(((quantity, quantity), amount, buys))
}

fn template_flow(
    day: Day,
    from: Id<crate::book::Place>,
    to: Id<crate::book::Place>,
    amount: Amount,
    owner: Id<crate::book::Entity>,
    payee: Option<Id<crate::book::Entity>>,
    purpose: Option<Purposed>,
    description: Option<crate::book::Text>,
    loc: Loc,
) -> Flow {
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
        payee,
        owner,
        purpose,
        description,
        origin: Origin::Written,
        select: Run::new(Id::new(0), 0),
        header_codes: Run::new(Id::new(0), 0),
        codes: Run::new(Id::new(0), 0),
        loc,
        waive: None,
        detail: None,
    }
}

fn template_quantity<'s>(
    world: &World<'s>,
    file: &ast::File<'s>,
    quantity: ast::Quantity<'s>,
    roots: &Map<ast::ExprId, crate::law::NodeId>,
    fallback: Id<crate::book::Commodity>,
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
    fallback: Id<crate::book::Commodity>,
    diags: &mut Vec<Diagnostic>,
) -> Option<(TemplateQuantity, Amount)> {
    match amount {
        ast::Amount::Literal(literal) => {
            let unit = literal.unit().map_or(Some(fallback), |unit| resolve_commodity(world, file, unit, diags))?;
            let amount =
                world.amount(literal.num(), unit, file.loc(literal.0)).map_err(|problem| diags.push(problem)).ok()?;
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

fn lower_item<'s>(
    world: &mut World<'s>,
    home: Home,
    file: &ast::File<'s>,
    item: &ast::LineItem<'s>,
    parent: TemplateItemParent,
    side: FlowSide,
    fallback: Id<crate::book::Commodity>,
    roots: &Map<ast::ExprId, crate::law::NodeId>,
    diags: &mut Vec<Diagnostic>,
) -> Option<TemplateItem> {
    let (amount, _) = template_amount(world, file, item.amount, roots, fallback, diags)?;
    let (amount, _) = match amount {
        TemplateQuantity::Amount(Some(root)) => (TemplateAmount::Computed(root), ()),
        _ => (
            TemplateAmount::Literal(match item.amount {
                ast::Amount::Literal(literal) => resolve_amount(world, file, literal, fallback, diags)?,
                ast::Amount::Computed(_) => return None,
            }),
            (),
        ),
    };
    let (codes, select, detail, waive, purpose, description) =
        lower_tail(world, home, file, item.tail, item.loc, diags);
    Some(TemplateItem {
        sign: match item.sign {
            ast::Sign::Carve => crate::book::Sign::Carve,
            ast::Sign::Add => crate::book::Sign::Add,
            ast::Sign::Less => crate::book::Sign::Less,
        },
        parent,
        side,
        amount,
        purpose: purpose.map(|at| at.value),
        description,
        codes,
        select,
        detail,
        waive,
        loc: item.loc,
    })
}

fn lower_tail<'s>(
    world: &mut World<'s>,
    home: Home,
    file: &ast::File<'s>,
    tail: axiom_syntax::Many<ast::Clause<'s>>,
    loc: Loc,
    diags: &mut Vec<Diagnostic>,
) -> (Run<Sym>, Run<Select>, Option<Id<Detail>>, Option<Waive>, Option<At<Purposed>>, Option<crate::book::Text>) {
    let code_start = world.book.codes.len();
    let mut details = Detail::NONE;
    let (mut purpose, mut description, mut waive) = (None, None, None);
    for clause in &file[tail] {
        match clause.kind {
            ClauseKind::Code(code) => {
                let sym = world.book.names.intern(code.name());
                world.book.codes.push(sym);
            }
            ClauseKind::Purpose(purpose_ast) => {
                let word = Word { text: purpose_ast.name.0, loc: file.loc(purpose_ast.name.0) };
                match world.purpose(home, word) {
                    Ok(id) => {
                        purpose = Some(At {
                            value: Purposed {
                                purpose: id,
                                of: purpose_ast.of.and_then(|name| resolve_object(world, home, file, name, diags)),
                                source: Provenance::Written,
                            },
                            loc: clause.at,
                        });
                    }
                    Err(problem) => diags.push(problem),
                }
            }
            ClauseKind::Description(text) => description = Some(world.book.quoted_text(text.0)),
            ClauseKind::Waive(written) => {
                waive =
                    Some(Waive { loc: written.at, reason: written.reason.map(|text| world.book.quoted_text(text.0)) });
            }
            ClauseKind::Due(_) => details.due = None,
            ClauseKind::For(_) | ClauseKind::Via(_) | ClauseKind::Basis(_) | ClauseKind::Since(_) => {}
            ClauseKind::Against(_) | ClauseKind::Price(_) | ClauseKind::Until(_) => {}
        }
    }
    let codes = Run::new(Id::new(code_start as u32), (world.book.codes.len() - code_start) as u32);
    let select = Run::new(Id::new(world.book.selectors.len() as u32), 0);
    let detail = (details != Detail::NONE).then(|| world.book.details.push(details));
    (codes, select, detail, waive, purpose, description)
}

fn resolve_amount<'s>(
    world: &World<'s>,
    file: &ast::File<'s>,
    literal: ast::Literal<'s>,
    fallback: Id<crate::book::Commodity>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Amount> {
    let unit = literal.unit().map_or(Some(fallback), |unit| resolve_commodity(world, file, unit, diags))?;
    world.amount(literal.num(), unit, file.loc(literal.0)).map_err(|problem| diags.push(problem)).ok()
}

fn resolve_commodity<'s>(
    world: &World<'s>,
    file: &ast::File<'s>,
    name: Name<'s>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Id<crate::book::Commodity>> {
    world.commodity_of(Word { text: name.0, loc: file.loc(name.0) }).map_err(|problem| diags.push(problem)).ok()
}

fn resolve_endpoint<'s>(
    world: &World<'s>,
    home: Home,
    file: &ast::File<'s>,
    name: Name<'s>,
    loc: Loc,
    diags: &mut Vec<Diagnostic>,
) -> Option<Id<crate::book::Place>> {
    match world.end(home, Word { text: name.0, loc: file.loc(name.0) }) {
        Ok(end) => Some(end.place),
        Err(problem) => {
            diags.push(problem);
            None
        }
    }
}

fn schedule_owner<'s>(
    world: &World<'s>,
    home: Home,
    file: &ast::File<'s>,
    schedule: Option<ast::Schedule<'s>>,
    _party: Id<crate::book::Entity>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Id<crate::book::Entity>> {
    let holding = schedule?.terms.holding?;
    let place = resolve_endpoint(world, home, file, holding.name, schedule?.at, diags)?;
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
        let target = match prop.name.0 {
            "from" => &mut first,
            "until" => &mut last,
            _ => continue,
        };
        if let Some(previous) = seen.insert(prop.name.0, prop.loc) {
            diags.push(
                Diagnostic::error("duplicate-contract-date", "a contract date is written twice")
                    .label(prop.loc, "written again here")
                    .context(previous, "first written here"),
            );
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

fn resolve_object<'s>(
    world: &World<'s>,
    home: Home,
    file: &ast::File<'s>,
    name: Name<'s>,
    diags: &mut Vec<Diagnostic>,
) -> Option<crate::journal::Object> {
    let sym = world.book.names.get(name.0);
    if let Some(sym) = sym
        && let Some(&asset) = world.book.lookup.assets.get(&sym)
    {
        return Some(crate::journal::Object::Asset(asset));
    }
    match world.entity(home, Word { text: name.0, loc: file.loc(name.0) }) {
        Ok(entity) => Some(crate::journal::Object::Entity(entity)),
        Err(problem) => {
            diags.push(problem);
            None
        }
    }
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
        diags.push(
            Diagnostic::error("duplicate-contract-grace", "a contract has one grace interval")
                .label(second.loc, "a second interval cannot replace the first")
                .context(first.loc, "the first interval is here"),
        );
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
                    let word = Word { text: name.0, loc: file.loc(name.0) };
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
            diags.push(
                Diagnostic::error("contract-area-duplicate", "a contract's area is declared twice")
                    .label(prop.loc, "remove this repeated area")
                    .context(first, "the first area is here"),
            );
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
        let unit = match world.commodity_of(Word { text: unit_name.0, loc: file.loc(unit_name.0) }) {
            Ok(unit) => unit,
            Err(problem) => {
                diags.push(problem);
                return Err(());
            }
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

fn contract_deposit<'s>(
    world: &mut World<'s>,
    file: &ast::File<'s>,
    props: axiom_syntax::Many<ast::Prop<'s>>,
    home: Home,
    owner: Id<crate::book::Entity>,
    default_holding: Option<(ast::Name<'s>, Loc)>,
    diags: &mut Vec<Diagnostic>,
) -> Result<Option<(Amount, Id<crate::book::Place>)>, ()> {
    let mut deposit = None;
    let mut first_loc = None;
    for prop in &file[props] {
        if prop.name.0 != "deposit" {
            continue;
        }
        if let Some(first) = first_loc {
            diags.push(
                Diagnostic::error("contract-deposit-duplicate", "a contract has one deposit")
                    .label(prop.loc, "remove this repeated deposit")
                    .context(first, "the first deposit is here"),
            );
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
        let ExprKind::Amount(literal) = file.exprs[args[0]].kind else {
            diags.push(
                Diagnostic::error("contract-deposit-amount", "a deposit must be a literal amount")
                    .label(file.exprs[args[0]].loc, "write the amount the contract will hold"),
            );
            return Err(());
        };
        let amount = match resolve_amount(world, file, literal, world.book.entities[owner].currency, diags) {
            Some(amount) if amount.qty.0 > 0 => amount,
            Some(_) => {
                diags.push(
                    Diagnostic::error("contract-deposit-positive", "a contract deposit must be positive")
                        .label(file.exprs[args[0]].loc, "write an amount greater than zero"),
                );
                return Err(());
            }
            None => return Err(()),
        };
        let (name, name_loc) = if args.len() == 3 {
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
            (name, file.exprs[args[2]].loc)
        } else if let Some((name, loc)) = default_holding {
            (name, loc)
        } else {
            diags.push(
                Diagnostic::error("contract-deposit-holding-required", "a deposit needs a holding account")
                    .label(prop.loc, "name `into HOLDING` or give this contract an active schedule with a holding"),
            );
            return Err(());
        };
        let place = match resolve_endpoint(world, home, file, name, name_loc, diags) {
            Some(place) => place,
            None => return Err(()),
        };
        if !matches!(world.book.places[place].role, crate::book::Role::Account { .. } | crate::book::Role::Holding(_)) {
            diags.push(
                Diagnostic::error("contract-deposit-holding", "a deposit is held in an account")
                    .label(name_loc, "choose an account or holding, not an asset or party"),
            );
            return Err(());
        }
        if world.book.places[place].owner != owner {
            diags.push(
                Diagnostic::error("contract-deposit-owner", "the deposit holding belongs to another owner")
                    .label(name_loc, "choose a holding owned by the contract owner")
                    .context(world.book.places[place].loc.unwrap_or(name_loc), "this place is declared here"),
            );
            return Err(());
        }
        if world.book.places[place].holds.as_ref().is_some_and(|units| !units.contains(&amount.unit)) {
            diags.push(
                Diagnostic::error("contract-deposit-unit", "the deposit holding does not accept this unit")
                    .label(file.exprs[args[0]].loc, "choose a unit the holding can keep"),
            );
            return Err(());
        }
        deposit = Some((amount, place));
    }
    Ok(deposit)
}

fn shares<'s>(
    world: &World<'s>,
    home: Home,
    file: &ast::File<'s>,
    props: axiom_syntax::Many<ast::Prop<'s>>,
    purpose: Option<At<Purposed>>,
    contract_area: Option<Amount>,
    anchor: Day,
    diags: &mut Vec<Diagnostic>,
) -> Vec<Share> {
    let mut shares = Vec::new();
    let mut total = Ratio::ZERO;
    for prop in &file[props] {
        if prop.name.0 != "share" {
            continue;
        }
        let args = &file[prop.args];
        if args.is_empty() {
            diags.push(
                Diagnostic::error("contract-share", "a contract share needs an amount")
                    .label(prop.loc, "write `share 60% for ENTITY`"),
            );
            continue;
        }

        let mut at = 0;
        while at < args.len() {
            let written_amount = args[at];
            at += 1;
            let (rate, measure) = match file.exprs[written_amount].kind {
                ExprKind::Pct(percent) => {
                    let rate = percent.to_ratio().and_then(|rate| rate.checked_div(Ratio::new(100, 1)?));
                    (rate, None)
                }
                ExprKind::Fraction(top, bottom) => (Ratio::new(i128::from(top), i128::from(bottom)), None),
                ExprKind::Amount(literal) => {
                    let numerator = literal.unit().and_then(|name| {
                        let unit = match world.commodity_of(Word { text: name.0, loc: file.loc(name.0) }) {
                            Ok(unit) => unit,
                            Err(problem) => {
                                diags.push(problem);
                                return None;
                            }
                        };
                        if !world.book.is_a(world.book.commodities[unit].kind, world.book.roots.kinds.measure) {
                            diags.push(
                                Diagnostic::error("contract-share-unit", "a measured share must use a measure unit")
                                    .label(file.loc(name.0), "this commodity is not a measure"),
                            );
                            return None;
                        }
                        world
                            .amount(literal.num(), unit, file.exprs[written_amount].loc)
                            .map_err(|problem| diags.push(problem))
                            .ok()
                    });
                    let denominator = contract_area.or_else(|| {
                        purpose.and_then(|at| at.value.of).and_then(|object| match object {
                            crate::journal::Object::Asset(asset) => asset_area(world, asset, anchor),
                            _ => None,
                        })
                    });
                    let ratio = match (numerator, denominator) {
                        (Some(numerator), Some(denominator))
                            if numerator.unit == denominator.unit && denominator.qty.0 > 0 =>
                        {
                            Ratio::new(i128::from(numerator.qty.0), i128::from(denominator.qty.0))
                        }
                        _ => {
                            diags.push(
                                Diagnostic::error(
                                    "contract-share-measure",
                                    "a measured share needs a positive contract or asset area in the same unit",
                                )
                                .label(file.exprs[written_amount].loc, "cannot resolve this measure"),
                            );
                            None
                        }
                    };
                    (ratio, numerator.zip(denominator))
                }
                _ => (None, None),
            };
            let Some(rate) = rate.filter(|rate| !rate.is_negative()) else {
                diags.push(
                    Diagnostic::error(
                        "contract-share",
                        "a share must be a nonnegative percentage, fraction, or measure",
                    )
                    .label(file.exprs[written_amount].loc, "write `60%`, `3/5` or `120 SQFT`"),
                );
                break;
            };
            let Some(for_word) = args.get(at).copied() else {
                diags.push(
                    Diagnostic::error("contract-share", "a contract share needs an owner")
                        .label(prop.loc, "write `share RATE for ENTITY`"),
                );
                break;
            };
            at += 1;
            if !matches!(file.exprs[for_word].kind, ExprKind::Name(Name("for"))) {
                diags.push(
                    Diagnostic::error("contract-share", "a share amount must be followed by `for ENTITY`")
                        .label(file.exprs[for_word].loc, "expected `for` here"),
                );
                break;
            }
            let Some(owner_expr) = args.get(at).copied() else {
                diags.push(
                    Diagnostic::error("contract-share", "a contract share needs an owner")
                        .label(prop.loc, "write an entity after `for`"),
                );
                break;
            };
            at += 1;
            let ExprKind::Name(owner_name) = file.exprs[owner_expr].kind else {
                diags.push(
                    Diagnostic::error("contract-share", "a share owner must be an entity name")
                        .label(file.exprs[owner_expr].loc, "write the owner here"),
                );
                break;
            };
            let Some(next_total) = total.checked_add(rate) else {
                diags.push(
                    Diagnostic::error("contract-share-total", "contract shares exceed exact arithmetic")
                        .label(prop.loc, "reduce the declared shares"),
                );
                break;
            };
            if next_total.checked_sub(Ratio::ONE).is_some_and(|excess| !excess.is_negative() && !excess.is_zero()) {
                diags.push(
                    Diagnostic::error("contract-share-total", "contract shares add up to more than 100%")
                        .label(prop.loc, "the total shares cannot exceed 100%"),
                );
                break;
            }
            total = next_total;
            match world.entity(home, Word { text: owner_name.0, loc: file.loc(owner_name.0) }) {
                Ok(entity) => shares.push(Share { rate, entity, measure, loc: prop.loc }),
                Err(problem) => diags.push(problem),
            }
        }
    }
    shares
}

fn asset_area(world: &World<'_>, asset: Id<crate::book::Asset>, day: Day) -> Option<Amount> {
    let area = world.book.names.get("area")?;
    let asset = &world.book.assets[asset];
    let own = crate::book::prop(&asset.props, area, day).and_then(|property| match property.value {
        crate::law::Value::Amount(amount) => Some(amount),
        _ => None,
    });
    own.or_else(|| {
        world.book.kinds.lineage(asset.kind).find_map(|kind| {
            crate::book::prop(&world.book.kinds[kind].props, area, day).and_then(|property| match property.value {
                crate::law::Value::Amount(amount) => Some(amount),
                _ => None,
            })
        })
    })
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
