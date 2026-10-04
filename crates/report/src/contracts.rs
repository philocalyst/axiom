//! `contracts`: promises, their current terms and the next time due.

use axiom_core::{Day, Days, Id};
use axiom_engine::Run;
use axiom_model::{
    Cadence, Change, Contract, Cut, Expr, FlowSide, Item, On, Part, Promised, Quantity, Says, ScheduleKind, Sign, Terms,
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
        let terms = contract.terms.as_ref();
        let next = next_due(lens.book(), run, id, contract);
        let promises = run.promises.iter().filter(|promise| promise.contract == id);
        let (kept, late, age) = promises.fold((0, 0, 0i64), |(kept, late, age), promise| {
            let late_days = promise.late(run.today);
            (kept + usize::from(promise.kept.is_some()), late + usize::from(late_days > 0), age + i64::from(late_days))
        });
        let loan_balance = loan_balance(lens, run, id);
        let cells = [
            Cell::Name(name),
            Cell::Name(book.name(book.entities[contract.party].path)),
            terms.map_or(Cell::Blank, |terms| terms_cell(lens, contract, terms, contract.waiver_on(run.today))),
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

/// The next due day from today that no occurrence has kept, of either of the contract's schedules.
fn next_due(book: &axiom_model::Book<'_>, run: &Run, id: Id<Contract>, contract: &Contract) -> Option<Day> {
    let from_today = Days::new(run.today, contract.days.last()).unwrap_or(Days::on(run.today));
    let window = contract.days.intersect(from_today)?;
    let kept = |due: Day| {
        run.promises.iter().any(|promise| promise.contract == id && promise.due == due && promise.kept.is_some())
    };
    // A loan's regular stream is owed its payments and no more: none after the one that paid it off.
    let loan = book.promises.loan(id).map(|loan| loan.payments().map(|payment| payment.day).collect::<Vec<_>>());
    let owed_days = |kind: ScheduleKind| -> Box<dyn Iterator<Item = Day> + '_> {
        match (&loan, book.promises.schedule(id, kind)) {
            (Some(payments), _) if kind == ScheduleKind::Regular => {
                Box::new(payments.iter().copied().filter(move |day| window.contains(*day)))
            }
            (_, Some(schedule)) => Box::new(schedule.days(window)),
            (_, None) => Box::new(std::iter::empty()),
        }
    };
    [ScheduleKind::Regular, ScheduleKind::Standing]
        .into_iter()
        .filter_map(|kind| owed_days(kind).find(|due| !kept(*due)))
        .min()
}

/// The terms of one stretch of a contract's life: what they say, or that a statement waived them.
pub(crate) fn terms_cell<'s>(
    lens: Lens<'s, '_, '_, '_>,
    contract: &'s Contract,
    terms: &'s Terms,
    waiver: Option<&Change>,
) -> Cell<'s> {
    let book = lens.book();
    if waiver.is_some() {
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

/// What a loan owes today by its schedule: what its terms and every payment, prepayment and rate the book says make of it.
fn loan_balance<'s>(lens: Lens<'s, '_, '_, '_>, run: &Run, id: Id<Contract>) -> Cell<'s> {
    let book = lens.book();
    let owed = book.promises.loan(id).and_then(|loan| Some((loan.open_on(run.today)?, loan.terms().principal().unit)));
    match owed.filter(|(qty, _)| !qty.is_zero()) {
        Some((qty, unit)) => Cell::amount(book, axiom_model::Amount::new(qty, unit)),
        None => Cell::Blank,
    }
}

/// Never render placeholder values for a term expression that the engine must
/// evaluate at the occurrence date.
pub(crate) fn template_flow_cell<'s>(lens: Lens<'s, '_, '_, '_>, template: &'s Promised) -> Cell<'s> {
    let book = lens.book();
    let flow = &template.header.flow;
    let header = Cell::list(
        " ",
        [
            Cell::text(route(book, flow)),
            quantity_cell(lens, flow, template.header.out),
            flow.is_exchange().then(|| quantity_cell(lens, flow, template.header.arrive)).unwrap_or(Cell::Blank),
        ]
        .into_iter()
        .chain(crate::table::code_labels(book, book.flow_view(flow).codes())),
    );
    let legs = template.legs.iter().map(|leg| {
        Cell::list(
            " ",
            [
                Cell::Word("split"),
                Cell::text(route(book, &leg.flow)),
                Cell::Word(side_word(template.side)),
                part_cell(lens, &leg.flow, leg.part),
            ],
        )
    });
    let items = template.items.iter().map(|item| template_item_cell(lens, template, item));
    Cell::list("; ", std::iter::once(header).chain(legs).chain(items))
}

/// A header side as the terms promise it. A computed one is never shown as the placeholder the flow carries.
fn quantity_cell<'s>(lens: Lens<'s, '_, '_, '_>, flow: &axiom_model::Flow, quantity: Quantity) -> Cell<'s> {
    let book = lens.book();
    match quantity {
        Quantity::Amount(Expr::Literal(amount)) => Cell::amount(
            book,
            axiom_model::Amount::new(crate::flow::scoped_movement_qty(lens, flow, amount.qty), amount.unit),
        ),
        Quantity::Amount(Expr::Computed(_)) => Cell::Word("computed per occurrence"),
        Quantity::Pending(Expr::Literal(_)) => Cell::Word("pending amount"),
        Quantity::Pending(Expr::Computed(_)) => Cell::Word("computed pending amount"),
        Quantity::Target(Expr::Literal(_)) => Cell::Word("target amount"),
        Quantity::Target(Expr::Computed(_)) => Cell::Word("computed target amount"),
        Quantity::Unknown(unit) => {
            Cell::list(" ", [Cell::Word("unknown"), Cell::Name(book.name(book.commodities[unit].symbol))])
        }
        Quantity::All(unit) => unit.map_or(Cell::Word("all"), |unit| {
            Cell::list(" ", [Cell::Word("all"), Cell::Name(book.name(book.commodities[unit].symbol))])
        }),
        Quantity::Derived => Cell::Word("the loan's payment"),
        Quantity::Interest => Cell::Word("the loan's interest"),
    }
}

/// A leg as the terms promise it.
fn part_cell<'s>(lens: Lens<'s, '_, '_, '_>, flow: &axiom_model::Flow, part: Part) -> Cell<'s> {
    match part {
        Part::Of(quantity) => quantity_cell(lens, flow, quantity),
        Part::Share(rate) => Cell::Percent(rate),
        Part::Rest => Cell::Word("rest"),
    }
}

/// Which side of the header a leg or item takes from.
fn side_word(side: FlowSide) -> &'static str {
    match side {
        FlowSide::Out => "out",
        FlowSide::Arrive => "arrive",
    }
}

/// An item of a promise: every one is under its header.
fn template_item_cell<'s>(lens: Lens<'s, '_, '_, '_>, template: &'s Promised, item: &'s Item<Says>) -> Cell<'s> {
    let book = lens.book();
    let sign = match item.sign {
        Sign::Carve => "carves",
        Sign::Add => "adds",
        Sign::Less => "takes off",
    };
    let amount = match item.amount {
        Cut::Of(Expr::Literal(amount)) => Cell::amount(
            book,
            axiom_model::Amount::new(
                crate::flow::scoped_movement_qty(lens, &template.header.flow, amount.qty),
                amount.unit,
            ),
        ),
        Cut::Of(Expr::Computed(_)) => Cell::Word("computed per occurrence"),
        Cut::Share(_) => Cell::Word("a share of the header"),
    };
    let says = &item.flow;
    let purpose =
        says.purpose.map_or(Cell::Blank, |purpose| Cell::Purpose(book.name(book.purposes[purpose.purpose].name)));
    Cell::list(
        " ",
        [Cell::Word(sign), Cell::Word("header"), Cell::Word(side_word(template.side)), amount, purpose]
            .into_iter()
            .chain(says.description.map(|description| Cell::text(book.text(description))))
            .chain(crate::table::code_labels(book, book.codes[says.codes].iter().copied()))
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
