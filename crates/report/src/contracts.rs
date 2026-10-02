//! `contracts`: promises, their current terms and the next time due.

use std::collections::BTreeMap;

use axiom_core::{Days, Id, Qty};
use axiom_engine::Run;
use axiom_model::{
    Book, Cadence, Contract, FlowSide, On, TemplateAmount, TemplateFlow, TemplateItem, TemplateItemParent,
    TemplateQuantity, Terms, TermsState,
};

use crate::lens::Lens;
use crate::places::route;
use crate::{Cell, Column, Report, Row, Section, Style};

pub(crate) fn view_with_lens<'s>(lens: Lens<'s, '_, '_, '_>, run: &Run) -> Report<'s> {
    let book = lens.book();
    let mut section = Section::new([
        Column::left("Contract"),
        Column::left("Party"),
        Column::left("Terms"),
        Column::left("Next due"),
        Column::right("Kept"),
        Column::left("Late"),
        Column::right("Loan balance"),
    ]);
    for (id, contract) in book.contracts.iter() {
        if !lens.owns_entity(contract.owner) {
            continue;
        }
        let name = book.name(contract.name);
        let terms = contract.terms_on(run.today);
        let next = contract
            .days
            .intersect(Days::new(run.today, contract.days.last()).unwrap_or(Days::on(run.today)))
            .and_then(|days| {
                contract.occurrences(days).map(|occurrence| occurrence.day).find(|due| {
                    !run.promises
                        .iter()
                        .any(|promise| promise.contract == id && promise.due == *due && promise.kept.is_some())
                })
            });
        let promises = run.promises.iter().filter(|promise| promise.contract == id);
        let (kept, late, age) = promises.fold((0, 0, 0i64), |(kept, late, age), promise| {
            let late_days = promise.late(run.today);
            (kept + usize::from(promise.kept.is_some()), late + usize::from(late_days > 0), age + i64::from(late_days))
        });
        let loan_balance = loan_balance(lens, run, contract);
        let cells = [
            Cell::Name(name),
            Cell::Name(book.name(book.entities[contract.party].path)),
            terms.map_or(Cell::Blank, |terms| terms_cell(lens, contract, terms)),
            next.map_or(Cell::Blank, Cell::Day),
            Cell::Count(kept, "kept"),
            if late == 0 { Cell::Blank } else { Cell::text(format!("{late} occurrences, {age} days")) },
            loan_balance,
        ];
        section.push(Row::new(cells).style(if late > 0 { Style::Alert } else { Style::Normal }));
    }
    if section.rows.is_empty() {
        section.note("No contracts are declared.");
    }
    Report::new("Contracts").with(section)
}

pub(crate) fn terms_cell<'s>(lens: Lens<'s, '_, '_, '_>, contract: &'s Contract, terms: &'s Terms) -> Cell<'s> {
    let book = lens.book();
    if terms.state == TermsState::Waived {
        return Cell::Word("waived");
    }
    let mut parts: Vec<Cell<'s>> = vec![cadence(terms.every)];
    parts.extend(terms.on.iter().map(on_day));
    if let Some(purpose) = contract.purpose {
        parts.push(Cell::Purpose(book.name(book.purposes[purpose.value.purpose].name)));
    }
    if let Some(description) = contract.description {
        parts.push(Cell::text(book.text(description)));
    }
    parts.extend(
        terms.inputs.iter().map(|input| Cell::list(" ", [Cell::Word("input"), Cell::Name(book.name(input.name))])),
    );
    parts.extend(terms.template.iter().map(|flow| template_flow_cell(lens, flow)));
    Cell::list(" ", parts)
}

fn loan_balance<'s>(lens: Lens<'s, '_, '_, '_>, run: &Run, contract: &Contract) -> Cell<'s> {
    let book = lens.book();
    let Some(loan) = contract.loan else {
        return Cell::Blank;
    };
    let sign = lens.display_sign(loan.debt);
    let mut balances = BTreeMap::<Id<axiom_model::Commodity>, Qty>::new();
    for holding in run.holdings.iter().filter(|holding| holding.place == loan.debt) {
        *balances.entry(holding.unit).or_default() += Qty(lens.place_qty(loan.debt, holding.qty()).0 * sign);
    }
    let cells = balances
        .into_iter()
        .filter(|(_, qty)| !qty.is_zero())
        .map(|(unit, qty)| Cell::amount(book, axiom_model::Amount::new(qty, unit)));
    Cell::list_or_blank(" · ", cells)
}

/// Never render placeholder values for a term expression that the engine must
/// evaluate at the occurrence date.
pub(crate) fn template_flow_cell<'s>(lens: Lens<'s, '_, '_, '_>, template: &'s TemplateFlow) -> Cell<'s> {
    let book = lens.book();
    let flow = &template.flow;
    let header = Cell::list(
        " ",
        [
            Cell::text(route(book, flow)),
            template_quantity(lens, flow, template.out, flow.out),
            flow.is_exchange()
                .then(|| template_quantity(lens, flow, template.arrive, flow.arrive))
                .unwrap_or(Cell::Blank),
        ]
        .into_iter()
        .chain(crate::table::code_labels(book, book.flow_view(flow).codes())),
    );
    let legs = template.legs.iter().map(|leg| {
        let side = match leg.side {
            FlowSide::Out => "out",
            FlowSide::Arrive => "arrive",
        };
        Cell::list(
            " ",
            [
                Cell::Word("split"),
                Cell::text(route(book, &leg.flow)),
                Cell::Word(side),
                template_quantity(
                    lens,
                    &leg.flow,
                    leg.quantity,
                    match leg.side {
                        FlowSide::Out => leg.flow.out,
                        FlowSide::Arrive => leg.flow.arrive,
                    },
                ),
            ],
        )
    });
    let items = template.items.iter().map(|item| template_item_cell(lens, template, item));
    Cell::list("; ", std::iter::once(header).chain(legs).chain(items))
}

fn template_quantity<'s>(
    lens: Lens<'s, '_, '_, '_>,
    flow: &axiom_model::Flow,
    quantity: TemplateQuantity,
    literal: axiom_model::Amount,
) -> Cell<'s> {
    let book = lens.book();
    match quantity {
        TemplateQuantity::Amount(None) => Cell::amount(
            book,
            axiom_model::Amount::new(crate::flow::scoped_movement_qty(lens, flow, literal.qty), literal.unit),
        ),
        TemplateQuantity::Amount(Some(_)) => Cell::Word("computed per occurrence"),
        TemplateQuantity::Pending(None) => Cell::Word("pending amount"),
        TemplateQuantity::Pending(Some(_)) => Cell::Word("computed pending amount"),
        TemplateQuantity::Target(None) => Cell::Word("target amount"),
        TemplateQuantity::Target(Some(_)) => Cell::Word("computed target amount"),
        TemplateQuantity::Percent(rate) => Cell::Percent(rate),
        TemplateQuantity::Unknown(unit) => {
            Cell::list(" ", [Cell::Word("unknown"), Cell::Name(book.name(book.commodities[unit].symbol))])
        }
        TemplateQuantity::All(unit) => unit.map_or(Cell::Word("all"), |unit| {
            Cell::list(" ", [Cell::Word("all"), Cell::Name(book.name(book.commodities[unit].symbol))])
        }),
        TemplateQuantity::Rest => Cell::Word("rest"),
        TemplateQuantity::Whole => Cell::Word("whole"),
        TemplateQuantity::Derived => Cell::Word("derived by contract rule"),
    }
}

fn template_item_cell<'s>(lens: Lens<'s, '_, '_, '_>, template: &'s TemplateFlow, item: &'s TemplateItem) -> Cell<'s> {
    let book = lens.book();
    let sign = match item.sign {
        axiom_model::Sign::Carve => "carves",
        axiom_model::Sign::Add => "adds",
        axiom_model::Sign::Less => "takes off",
    };
    let parent = match item.parent {
        TemplateItemParent::Header => Cell::Word("header"),
        TemplateItemParent::Leg(index) => Cell::text(format!("split {}", u32::from(index) + 1)),
    };
    let side = match item.side {
        FlowSide::Out => "out",
        FlowSide::Arrive => "arrive",
    };
    let flow = match item.parent {
        TemplateItemParent::Header => &template.flow,
        TemplateItemParent::Leg(index) => template.legs.get(usize::from(index)).map_or(&template.flow, |leg| &leg.flow),
    };
    let amount = match item.amount {
        TemplateAmount::Literal(amount) => Cell::amount(
            book,
            axiom_model::Amount::new(crate::flow::scoped_movement_qty(lens, flow, amount.qty), amount.unit),
        ),
        TemplateAmount::Computed(_) => Cell::Word("computed per occurrence"),
    };
    let purpose =
        item.purpose.map_or(Cell::Blank, |purpose| Cell::Purpose(book.name(book.purposes[purpose.purpose].name)));
    Cell::list(
        " ",
        [Cell::Word(sign), parent, Cell::Word(side), amount, purpose]
            .into_iter()
            .chain(item.description.map(|description| Cell::text(book.text(description))))
            .chain(crate::table::code_labels(book, book.codes[item.codes].iter().copied()))
            .chain(std::iter::once(Cell::Source(item.loc))),
    )
}

fn cadence<'s>(cadence: Cadence) -> Cell<'s> {
    match cadence {
        Cadence::Every(span) => Cell::text(format!("every {span}")),
        Cadence::TwiceMonthly => Cell::Word("twice monthly"),
    }
}

fn on_day<'s>(on: &On) -> Cell<'s> {
    match *on {
        On::MonthDay(day) => Cell::text(format!("on {day}")),
        On::Last => Cell::Word("on last"),
        On::YearDay { month, day } => Cell::text(format!("on {month:02}-{day:02}")),
        On::Weekday(day) => Cell::text(format!(
            "on {}",
            ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"][usize::from(day).min(6)]
        )),
    }
}
