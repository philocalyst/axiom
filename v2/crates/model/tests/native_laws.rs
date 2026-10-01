use axiom_core::{Day, FileId};
use axiom_model::{Func, Implied, Limit, Period, Source, TemplateAmount, build};
use axiom_syntax::{Folder, parse};

fn source(path: &'static str, text: &'static str, embedded: bool, id: u16) -> Source<'static> {
    let (file, syntax) = parse(FileId(id), text, Folder::of(path));
    assert!(syntax.is_empty(), "{path}: {syntax:?}");
    Source {
        path,
        file,
        embedded,
    }
}

#[test]
fn native_budget_and_declaration_also_are_linked() {
    let std = source(
        "std.ax",
        "system std\nkind bank : asset\ncommodity USD\n  precision 2\n",
        true,
        0,
    );
    let project = source(
        "axiom.ax",
        "use std\nbase USD\naccount checking : bank\npurpose groceries : spending\n  budget 100 USD monthly carries\n  also + 5% of amount #fees when value(amount, USD) > 10 USD\n  also checking -> self 2 USD #fees\npurpose fees : spending\npurpose pay : income\npurpose hobbies : spending\n  budget 10% of #pay yearly carries\n",
        false,
        1,
    );
    let (book, diagnostics) = build(&[std, project]);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(book.budgets.len(), 2);
    let (budget_id, budget) = book
        .budgets
        .iter()
        .find(|(_, budget)| budget.period == Period::Month)
        .expect("a monthly budget");
    assert_eq!(budget.period, Period::Month);
    assert!(budget.carries);
    assert!(
        matches!(budget.limits.at(Day::MIN), Limit::Amount(amount) if book.show(*amount).to_string() == "100.00 USD")
    );
    let budget_law = &book.laws[budget.law];
    assert_eq!(budget_law.budget, Some(budget_id));
    assert!(budget_law.nodes.iter().any(|(_, node)| matches!(node.op, axiom_model::Op::Call(Func::BudgetLimit(id), _) if id == budget_id)));
    assert_eq!(book.also.len(), 2);
    let implied = book
        .also
        .iter()
        .find(|(_, implied)| matches!(implied.what, Implied::Item { .. }))
        .map(|(_, implied)| implied)
        .expect("computed implied item");
    assert!(matches!(
        implied.what,
        Implied::Item {
            amount: TemplateAmount::Computed(_),
            ..
        }
    ));
    assert!(implied.purpose.is_some(), "the implied item keeps #fees");
    assert!(implied.when.is_some(), "the Also keeps its typed predicate");
    assert!(implied.law.index() < book.laws.len());
    let self_flow = book
        .also
        .iter()
        .find(|(_, implied)| matches!(implied.what, Implied::Flow { .. }))
        .map(|(_, implied)| implied)
        .expect("implied flow");
    assert!(matches!(
        self_flow.what,
        Implied::Flow {
            from: Some(_),
            to: None,
            ..
        }
    ));
    let (share_id, share_budget) = book
        .budgets
        .iter()
        .find(|(_, budget)| matches!(budget.limits.at(Day::MIN), Limit::Share { .. }))
        .expect("the share budget");
    assert!(matches!(
        share_budget.limits.at(Day::MIN),
        Limit::Share { .. }
    ));
    assert_eq!(book.laws[share_budget.law].budget, Some(share_id));
}

#[test]
fn law_overrides_resolve_to_the_nearest_visible_system() {
    let std = source(
        "std.ax",
        "system std\ncommodity USD\n  precision 2\n",
        true,
        0,
    );
    let parent = source(
        "us.ax",
        "system us\nlaw standard\n  on in\n  warn value(amount, USD) > 1 USD\n",
        true,
        1,
    );
    let child = source(
        "ca.ax",
        "system us/ca\nlaw state-standard overrides standard\n  on in\n  warn value(amount, USD) > 2 USD\n",
        true,
        2,
    );
    let project = source("axiom.ax", "use us/ca\nbase USD\n", false, 3);
    let (book, diagnostics) = build(&[std, parent, child, project]);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let state = book
        .laws
        .iter()
        .find(|(_, law)| book.name(law.name) == "state-standard")
        .map(|(id, law)| (id, law))
        .expect("child law");
    let parent = state.1.overrides.expect("resolved override");
    assert_eq!(book.name(book.laws[parent].name), "standard");
    assert_eq!(
        book.name(book.systems[book.laws[parent].system.unwrap()].path),
        "us"
    );
}

#[test]
fn law_override_names_ambiguous_between_equally_visible_systems() {
    let std = source(
        "std.ax",
        "system std\ncommodity USD\n  precision 2\n",
        true,
        0,
    );
    let east = source(
        "east.ax",
        "system us/east\nlaw shared\n  on in\n  warn value(amount, USD) > 1 USD\n",
        true,
        1,
    );
    let west = source(
        "west.ax",
        "system us/west\nlaw shared\n  on in\n  warn value(amount, USD) > 1 USD\n",
        true,
        2,
    );
    let project = source(
        "axiom.ax",
        "use us/east\nuse us/west\nbase USD\nlaw project-rule overrides shared\n  on in\n  warn value(amount, USD) > 3 USD\n",
        false,
        3,
    );
    let (_, diagnostics) = build(&[std, east, west, project]);
    assert_eq!(
        diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_ref())
            .collect::<Vec<_>>(),
        ["ambiguous-law"],
        "{diagnostics:?}"
    );
}
