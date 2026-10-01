//! S5 contract declarations are lowered in two passes. Names and ids exist
//! before any template expression compiles, so a template may mention a
//! contract declared later in the project.

use axiom_core::{Day, Days, Diagnostic, Id, Loc, Map, Qty, Ratio, Run, Span, Sym, Timeline};
use axiom_syntax as ast;
use axiom_syntax::{ClauseKind, Direction, ExprKind, ItemKind, Name};

use super::{JournalSurvey, compile_roots, contract_roots, inputs};
use crate::book::{
    Amount, At, Cadence, Contract, Coverage, Deadline, Escalation, FlowSide, Input, Relative,
    Share, TemplateAmount, TemplateFlow, TemplateItem, TemplateItemParent, TemplateLeg,
    TemplateProgram, TemplateQuantity, Terms, TermsState,
};
use crate::declare::World;
use crate::errors::{Word, duplicate};
use crate::journal::{
    Detail, Flow, Infer, Mode, Origin, Provenance, Purposed, Select, TEMPLATE_TXN, Waive,
};
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
                    Word {
                        text: node.name.0,
                        loc: file.loc(node.name.0),
                    },
                    Some(world.book.contracts[first].loc),
                    None,
                ));
                continue;
            }
            let id = world
                .book
                .contracts
                .push(empty_contract(name, item.loc, world.book.roots.me));
            world.book.lookup.contracts.insert(name, id);
            written.push(WrittenContract {
                site,
                node,
                id,
                name,
                loc: item.loc,
            });
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
    let name_word = Word {
        text: node.name.0,
        loc: file.loc(node.name.0),
    };
    let party_word = node.party.map_or(name_word, |party| Word {
        text: party.0,
        loc: file.loc(party.0),
    });
    let party = match world.entity(written.site.home, party_word) {
        Ok(party) => party,
        Err(problem) => {
            diags.push(problem);
            world.book.roots.me
        }
    };
    let days = contract_days(file, node.props, diags);
    let anchor = days.first();
    let purpose = node.purpose.and_then(|purpose| {
        let purpose_word = Word {
            text: purpose.name.0,
            loc: file.loc(purpose.name.0),
        };
        match world.purpose(written.site.home, purpose_word) {
            Ok(id) => Some(At {
                value: Purposed {
                    purpose: id,
                    of: purpose.of.and_then(|object| {
                        resolve_object(world, written.site.home, file, object, diags)
                    }),
                    source: Provenance::Contract(written.id),
                },
                loc: purpose_word.loc,
            }),
            Err(problem) => {
                diags.push(problem);
                None
            }
        }
    });
    let description = node
        .description
        .map(|description| world.book.quoted_text(description.0));
    let contract_inputs = inputs(world, file, node.props, diags);
    let roots = contract_roots(file, node);
    let (regular, standing) = (
        compile_roots(
            world,
            file,
            written.site.home,
            Ty::Flow,
            written.name,
            &contract_inputs,
            &roots.regular,
            diags,
        ),
        compile_roots(
            world,
            file,
            written.site.home,
            Ty::Flow,
            written.name,
            &contract_inputs,
            &roots.standing,
            diags,
        ),
    );

    let owner = schedule_owner(
        world,
        written.site.home,
        file,
        node.schedule.or(node.standing),
        party,
        diags,
    )
    .unwrap_or(world.book.roots.me);
    let mut contract = empty_contract(
        written.name,
        written.site.source.file.loc(node.name.0),
        owner,
    );
    contract.party = party;
    contract.owner = owner;
    contract.days = days;
    contract.purpose = purpose;
    contract.description = description;
    contract.doc = node_doc(world, written.site, written.loc);
    contract.loc = written.loc;
    contract.buys = node
        .standing
        .and_then(|schedule| match schedule.terms.payment {
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
            diags,
        )?;
        contract.standing = Some(Timeline::new(terms));
    }
    Some(contract)
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
    diags: &mut Vec<Diagnostic>,
) -> Option<Terms> {
    let hold = schedule.terms.holding?;
    let holding = resolve_endpoint(
        world,
        written.site.home,
        file,
        hold.name,
        schedule.at,
        diags,
    )?;
    let party_place = world.book.entities[party].place?;
    let (from, to, side) = match hold.direction {
        Direction::From => (holding, party_place, FlowSide::Arrive),
        Direction::Into => (party_place, holding, FlowSide::Out),
    };
    let owner = world.book.places[holding].owner;
    let ((out_quantity, arrive_quantity), header_amount, buys) =
        schedule_amount(world, file, schedule, &roots, diags)?;
    let header_flow = template_flow(
        anchor,
        from,
        to,
        header_amount,
        owner,
        if hold.direction == Direction::From {
            Some(party)
        } else {
            Some(owner)
        },
        purpose.map(|at| at.value),
        description,
        schedule.at,
    );
    let mut legs = Vec::new();
    for leg in &file[body.legs] {
        let endpoint =
            resolve_endpoint(world, written.site.home, file, leg.end.name, leg.loc, diags)?;
        let (leg_from, leg_to) = match side {
            FlowSide::Arrive => (holding, endpoint),
            FlowSide::Out => (endpoint, holding),
        };
        let (quantity, amount) =
            template_quantity(world, file, leg.amount, &roots, header_amount.unit, diags)?;
        let mut flow = template_flow(
            anchor,
            leg_from,
            leg_to,
            amount,
            owner,
            None,
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
        flow.purpose = leg_purpose.or(flow.purpose);
        flow.description = leg_description.or(flow.description);
        legs.push(TemplateLeg {
            flow,
            side,
            quantity,
        });
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
        grace: span_property(file, written.node.props, "grace").unwrap_or(Span::default()),
        period: relative_property(file, written.node.props),
        covers: coverage_property(file, written.node.props),
        prorated: has_property(file, written.node.props, "prorated"),
        escalation: escalation_property(world, written.site.home, file, written.node.props, diags),
        shares: shares(world, written.site.home, file, written.node.props, diags)
            .into_boxed_slice(),
        also: Box::default(),
        rate: None,
        change: None,
    })
}

fn schedule_amount<'s>(
    world: &World<'s>,
    file: &ast::File<'s>,
    schedule: ast::Schedule<'s>,
    roots: &Map<ast::ExprId, crate::law::NodeId>,
    diags: &mut Vec<Diagnostic>,
) -> Option<(
    (TemplateQuantity, TemplateQuantity),
    Amount,
    Option<Id<crate::book::Commodity>>,
)> {
    let (quantity, amount, buys) = match schedule.terms.payment {
        Some(ast::Payment::Fixed(amount)) => {
            let (quantity, amount) =
                template_amount(world, file, amount, roots, world.book.base, diags)?;
            (quantity, amount, None)
        }
        Some(ast::Payment::Buy { unit, spend }) => {
            let buy_unit = resolve_commodity(world, file, unit, diags)?;
            let (quantity, amount) =
                template_amount(world, file, spend, roots, world.book.base, diags)?;
            (quantity, amount, Some(buy_unit))
        }
        None => (
            TemplateQuantity::Derived,
            Amount::zero(world.book.base),
            None,
        ),
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
            (
                TemplateQuantity::All(unit),
                Amount::zero(unit.unwrap_or(fallback)),
            )
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
            let unit = literal.unit().map_or(Some(fallback), |unit| {
                resolve_commodity(world, file, unit, diags)
            })?;
            let amount = world
                .amount(literal.num(), unit, file.loc(literal.0))
                .map_err(|problem| diags.push(problem))
                .ok()?;
            Some((TemplateQuantity::Amount(None), amount))
        }
        ast::Amount::Computed(root) => {
            let Some(&node) = roots.get(&root) else {
                diags.push(
                    Diagnostic::error(
                        "template-root",
                        "a computed template amount was not compiled",
                    )
                    .label(
                        file.exprs[root].loc,
                        "this amount has no typed program node",
                    ),
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
                ast::Amount::Literal(literal) => {
                    resolve_amount(world, file, literal, fallback, diags)?
                }
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
        purpose,
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
) -> (
    Run<Sym>,
    Run<Select>,
    Option<Id<Detail>>,
    Option<Waive>,
    Option<Purposed>,
    Option<crate::book::Text>,
) {
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
                let word = Word {
                    text: purpose_ast.name.0,
                    loc: file.loc(purpose_ast.name.0),
                };
                match world.purpose(home, word) {
                    Ok(id) => {
                        purpose = Some(Purposed {
                            purpose: id,
                            of: purpose_ast
                                .of
                                .and_then(|name| resolve_object(world, home, file, name, diags)),
                            source: Provenance::Written,
                        });
                    }
                    Err(problem) => diags.push(problem),
                }
            }
            ClauseKind::Description(text) => description = Some(world.book.quoted_text(text.0)),
            ClauseKind::Waive(written) => {
                waive = Some(Waive {
                    loc: written.at,
                    reason: written.reason.map(|text| world.book.quoted_text(text.0)),
                });
            }
            ClauseKind::Due(_) => details.due = None,
            ClauseKind::For(_)
            | ClauseKind::Via(_)
            | ClauseKind::Basis(_)
            | ClauseKind::Since(_) => {}
            ClauseKind::Against(_) | ClauseKind::Price(_) | ClauseKind::Until(_) => {}
        }
    }
    let codes = Run::new(
        Id::new(code_start as u32),
        (world.book.codes.len() - code_start) as u32,
    );
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
    let unit = literal.unit().map_or(Some(fallback), |unit| {
        resolve_commodity(world, file, unit, diags)
    })?;
    world
        .amount(literal.num(), unit, file.loc(literal.0))
        .map_err(|problem| diags.push(problem))
        .ok()
}

fn resolve_commodity<'s>(
    world: &World<'s>,
    file: &ast::File<'s>,
    name: Name<'s>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Id<crate::book::Commodity>> {
    world
        .commodity_of(Word {
            text: name.0,
            loc: file.loc(name.0),
        })
        .map_err(|problem| diags.push(problem))
        .ok()
}

fn resolve_endpoint<'s>(
    world: &World<'s>,
    _home: Home,
    file: &ast::File<'s>,
    name: Name<'s>,
    loc: Loc,
    diags: &mut Vec<Diagnostic>,
) -> Option<Id<crate::book::Place>> {
    match world.find_end(name.0) {
        Ok(end) => Some(end.place),
        Err(cause) => {
            diags.push(world.explain(cause, Word { text: name.0, loc }, 1));
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
) -> Days {
    let mut first = Day::MIN;
    let mut last = Day::MAX;
    for prop in &file[props] {
        let target = match prop.name.0 {
            "from" => &mut first,
            "until" => &mut last,
            _ => continue,
        };
        let value = file[prop.args]
            .first()
            .and_then(|id| match file.exprs[*id].kind {
                ExprKind::Date(day) => Some(day),
                _ => None,
            });
        if let Some(day) = value {
            *target = day;
        } else {
            diags.push(
                Diagnostic::error(
                    "contract-date",
                    "a contract's `from` and `until` need a date",
                )
                .label(prop.loc, "write a date here"),
            );
        }
    }
    Days::new(first, last).unwrap_or_else(|| {
        diags.push(
            Diagnostic::error("contract-range", "a contract ends before it begins").label(
                file[props].first().map_or(Loc::default(), |prop| prop.loc),
                "these dates do not overlap",
            ),
        );
        Days::ALWAYS
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
    match world.entity(
        home,
        Word {
            text: name.0,
            loc: file.loc(name.0),
        },
    ) {
        Ok(entity) => Some(crate::journal::Object::Entity(entity)),
        Err(problem) => {
            diags.push(problem);
            None
        }
    }
}

fn node_doc<'s>(world: &World<'s>, site: &Site<'_, 's>, loc: Loc) -> Option<Sym> {
    let item = site.source.file.items.iter().find(|item| item.loc == loc)?;
    item.doc.map(|doc| world.sym(doc.0))
}

fn span_property(
    file: &ast::File<'_>,
    props: axiom_syntax::Many<ast::Prop<'_>>,
    name: &str,
) -> Option<Span> {
    file[props]
        .iter()
        .find(|prop| prop.name.0 == name)
        .and_then(|prop| file[prop.args].first())
        .and_then(|id| match file.exprs[*id].kind {
            ExprKind::Span(span) => Some(span),
            _ => None,
        })
}

fn has_property(
    file: &ast::File<'_>,
    props: axiom_syntax::Many<ast::Prop<'_>>,
    name: &str,
) -> bool {
    file[props].iter().any(|prop| prop.name.0 == name)
}

fn relative_property(
    file: &ast::File<'_>,
    props: axiom_syntax::Many<ast::Prop<'_>>,
) -> Option<Relative> {
    let property = file[props].iter().find(|prop| prop.name.0 == "for")?;
    let words: Vec<_> = file[property.args]
        .iter()
        .filter_map(|id| match file.exprs[*id].kind {
            ExprKind::Name(name) => Some(name.0),
            _ => None,
        })
        .collect();
    match words.as_slice() {
        ["last", "month"] => Some(Relative::Last(axiom_core::Period::Month)),
        ["last", "quarter"] => Some(Relative::LastQuarter),
        ["last", "year"] => Some(Relative::Last(axiom_core::Period::Year)),
        _ => None,
    }
}

fn coverage_property(
    file: &ast::File<'_>,
    props: axiom_syntax::Many<ast::Prop<'_>>,
) -> Option<Coverage> {
    let property = file[props].iter().find(|prop| prop.name.0 == "covers")?;
    let arg = *file[property.args].first()?;
    match file.exprs[arg].kind {
        ExprKind::Span(span) => Some(Coverage::Span(span)),
        ExprKind::Name(Name("month")) => Some(Coverage::Calendar(axiom_core::Period::Month)),
        ExprKind::Name(Name("quarter")) => Some(Coverage::Quarter),
        ExprKind::Name(Name("year")) => Some(Coverage::Calendar(axiom_core::Period::Year)),
        _ => None,
    }
}

fn escalation_property<'s>(
    world: &World<'s>,
    home: Home,
    file: &ast::File<'s>,
    props: axiom_syntax::Many<ast::Prop<'s>>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Escalation> {
    for prop in &file[props] {
        let first = *file[prop.args].first()?;
        match prop.name.0 {
            "rising" => {
                let ExprKind::Pct(percent) = file.exprs[first].kind else {
                    continue;
                };
                let rate = percent
                    .to_ratio()
                    .and_then(|rate| rate.checked_div(Ratio::new(100, 1)?));
                if let Some(rate) = rate {
                    return Some(Escalation::Rising(rate));
                }
                diags.push(
                    Diagnostic::error(
                        "contract-rate",
                        "a contract's yearly rise must be a finite percentage",
                    )
                    .label(file.exprs[first].loc, "this rate cannot be represented"),
                );
            }
            "indexed" => {
                let name = match file.exprs[first].kind {
                    ExprKind::Name(name) => name,
                    _ => continue,
                };
                if let crate::names::Found::One(param) =
                    world
                        .book
                        .lookup
                        .params
                        .find(&world.book.names, world.scopes.of(home), name.0)
                {
                    return Some(Escalation::Indexed(param));
                }
            }
            _ => {}
        }
    }
    let _ = home;
    None
}

fn shares<'s>(
    world: &World<'s>,
    home: Home,
    file: &ast::File<'s>,
    props: axiom_syntax::Many<ast::Prop<'s>>,
    diags: &mut Vec<Diagnostic>,
) -> Vec<Share> {
    let mut shares = Vec::new();
    for prop in &file[props] {
        if prop.name.0 != "share" {
            continue;
        }
        let args = &file[prop.args];
        let Some((&rate_expr, rest)) = args.split_first() else {
            continue;
        };
        let rate = match file.exprs[rate_expr].kind {
            ExprKind::Pct(percent) => percent
                .to_ratio()
                .and_then(|rate| rate.checked_div(Ratio::new(100, 1)?)),
            ExprKind::Fraction(top, bottom) => Ratio::new(i128::from(top), i128::from(bottom)),
            _ => None,
        };
        let Some(rate) = rate else {
            diags.push(
                Diagnostic::error(
                    "contract-share",
                    "a contract share must be a percentage or fraction",
                )
                .label(file.exprs[rate_expr].loc, "write `60%` or `3/5`"),
            );
            continue;
        };
        let Some(entity_expr) = rest
            .iter()
            .find(|&&id| matches!(file.exprs[id].kind, ExprKind::Name(_)))
        else {
            diags.push(
                Diagnostic::error("contract-share", "a contract share needs an owner")
                    .label(prop.loc, "write `share RATE for ENTITY`"),
            );
            continue;
        };
        let ExprKind::Name(entity_name) = file.exprs[*entity_expr].kind else {
            continue;
        };
        if entity_name.0 == "for" {
            continue;
        }
        match world.entity(
            home,
            Word {
                text: entity_name.0,
                loc: file.loc(entity_name.0),
            },
        ) {
            Ok(entity) => shares.push(Share {
                rate,
                entity,
                measure: None,
                loc: prop.loc,
            }),
            Err(problem) => diags.push(problem),
        }
    }
    shares
}
