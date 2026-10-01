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
        "use std\nbase USD\naccount checking : bank\naccount reserve : bank\naccount envelope : bank\npurpose groceries : spending\n  budget 100 USD monthly carries funded from checking into reserve\n  also + 5% of amount #fees when value(amount, USD) > 10 USD\n  also checking[^invoice] -> self 2 USD #fees\npurpose fees : spending\npurpose pay : income\npurpose hobbies : spending\n  budget 10% of #pay yearly carries\n2026-03-01 #groceries now budget 250 USD yearly funded from checking into envelope until 04-30\npurpose late : spending\n2026-03-01 #late now budget 30 USD monthly\n",
        false,
        1,
    );
    let (book, diagnostics) = build(&[std, project]);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(book.budgets.len(), 3);
    let purpose_order: Vec<_> = book
        .budgets
        .iter()
        .map(|(_, budget)| budget.purpose.index())
        .collect();
    assert!(purpose_order.windows(2).all(|pair| pair[0] <= pair[1]));
    let (budget_id, budget) = book
        .budgets
        .iter()
        .find(|(_, budget)| budget.terms.at(Day::MIN).period == Period::Month)
        .expect("a monthly budget");
    assert_eq!(budget.terms.at(Day::MIN).period, Period::Month);
    assert!(budget.terms.at(Day::MIN).carries);
    assert!(
        matches!(budget.terms.at(Day::MIN).limit, Limit::Amount(amount) if book.show(amount).to_string() == "100.00 USD")
    );
    let budget_law = &book.laws[budget.law];
    assert_eq!(budget_law.budget, Some(budget_id));
    assert!(budget_law.nodes.iter().any(|(_, node)| matches!(node.op, axiom_model::Op::Call(Func::BudgetLimit(id), _) if id == budget_id)));
    assert!(budget_law.nodes.iter().any(|(_, node)| matches!(node.op, axiom_model::Op::Call(Func::BudgetTotal(id), _) if id == budget_id)));
    let changed = Day::parse(b"2026-03-15").unwrap();
    let restored = Day::parse(b"2026-05-01").unwrap();
    assert_eq!(budget.terms.at(changed).period, Period::Year);
    assert!(
        budget.terms.at(changed).carries,
        "omitted carries inherits the prior setting"
    );
    assert!(
        matches!(budget.terms.at(changed).limit, Limit::Amount(amount) if book.show(amount).to_string() == "250.00 USD")
    );
    let initial_funding = budget.terms.at(Day::MIN).funded;
    assert_ne!(budget.terms.at(changed).funded, initial_funding);
    assert_eq!(budget.terms.at(restored).period, Period::Month);
    assert!(
        budget.terms.at(restored).carries,
        "until restores the entire prior terms row"
    );
    assert!(
        matches!(budget.terms.at(restored).limit, Limit::Amount(amount) if book.show(amount).to_string() == "100.00 USD")
    );
    assert_eq!(budget.terms.at(restored).funded, initial_funding);
    let late = book
        .budgets
        .iter()
        .find(|(_, budget)| book.name(book.purposes[budget.purpose].name) == "late")
        .map(|(_, budget)| budget)
        .expect("late budget");
    assert_eq!(late.starts, Day::parse(b"2026-03-01").unwrap());
    assert_eq!(
        late.terms.at(Day::MIN).limit,
        Limit::Amount(axiom_model::Amount::zero(book.base))
    );
    assert!(
        matches!(late.terms.at(late.starts).limit, Limit::Amount(amount) if book.show(amount).to_string() == "30.00 USD")
    );
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
        .find(|(_, budget)| matches!(budget.terms.at(Day::MIN).limit, Limit::Share { .. }))
        .expect("the share budget");
    assert!(matches!(
        share_budget.terms.at(Day::MIN).limit,
        Limit::Share { .. }
    ));
    assert_eq!(book.laws[share_budget.law].budget, Some(share_id));
    let pay = book
        .purposes
        .iter()
        .find(|(_, purpose)| book.name(purpose.name) == "pay")
        .map(|(id, _)| id)
        .expect("pay purpose");
    assert!(book.laws[share_budget.law].nodes.iter().any(|(_, node)| {
        matches!(
            node.op,
            axiom_model::Op::Call(
                Func::PurposeTotal { purpose: Some(of), window: axiom_model::Window::Year },
                _
            ) if of == pay
        )
    }));
    let source_selected_flow = book
        .also
        .iter()
        .find(|(_, implied)| matches!(implied.what, Implied::Flow { .. }))
        .map(|(_, implied)| implied)
        .expect("source-selected flow");
    let selectors = &book.selectors[source_selected_flow.select];
    assert_eq!(selectors.len(), 1);
    assert!(matches!(
        selectors[0],
        axiom_model::Select::Code(code) if book.name(code) == "invoice"
    ));
}

#[test]
fn invalid_budget_until_does_not_install_a_zero_budget_law() {
    let std = source(
        "std.ax",
        "system std\ncommodity USD\n  precision 2\n",
        true,
        0,
    );
    let (project_file, syntax_diagnostics) = parse(
        FileId(1),
        "use std\nbase USD\npurpose grocery : spending\n2026-03-01 #grocery now budget 100 USD monthly until 04-30 until 05-31\n",
        Folder::of("axiom.ax"),
    );
    assert!(
        syntax_diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "duplicate-clause")
    );
    let project = Source {
        path: "axiom.ax",
        file: project_file,
        embedded: false,
    };
    let (book, diagnostics) = build(&[std, project]);
    assert!(
        diagnostics.is_empty(),
        "the malformed statement was rejected by parsing: {diagnostics:?}"
    );
    assert!(
        book.budgets.is_empty(),
        "invalid terms must not install a budget"
    );
    assert!(
        book.laws.iter().all(|(_, law)| law.budget.is_none()),
        "the rejected budget must not leave a reachable zero-limit warning law"
    );
}

#[test]
fn invalid_initial_budget_terms_do_not_leave_a_zero_budget_law() {
    let std = source(
        "std.ax",
        "system std\ncommodity USD\n  precision 2\nkind bank : asset\n",
        true,
        0,
    );
    let project = source(
        "axiom.ax",
        "use std\nbase USD\naccount checking : bank\npurpose grocery : spending\n  budget 100 USD monthly funded from missing into checking\n",
        false,
        1,
    );
    let (book, diagnostics) = build(&[std, project]);
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "unknown-place"),
        "bad funding is refused: {diagnostics:?}"
    );
    assert!(
        book.budgets.is_empty(),
        "failed initial terms must not install a budget"
    );
    assert!(
        book.laws.iter().all(|(_, law)| law.budget.is_none()),
        "a failed initial term must not leave a zero-limit warning law"
    );
}

#[test]
fn invalid_declaration_also_metadata_does_not_leave_a_partial_rule() {
    let std = source(
        "std.ax",
        "system std\ncommodity USD\n  precision 2\n",
        true,
        0,
    );
    let project = source(
        "axiom.ax",
        "use std\nbase USD\naccount checking : asset\npurpose fees : spending\npurpose wages : income\n  also checking -> self 1 USD due 3d\n",
        false,
        1,
    );
    let (book, diagnostics) = build(&[std, project]);
    assert_eq!(
        diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_ref())
            .collect::<Vec<_>>(),
        ["also-relative-due"],
        "unsupported tail metadata is diagnosed"
    );
    assert!(
        book.also.is_empty(),
        "an invalid Also line must not enter the runtime book"
    );
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
