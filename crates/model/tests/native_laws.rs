use axiom_core::{Day, FileId};
use axiom_model::{Func, Limit, Owner, Period, Source, build};
use axiom_syntax::{Folder, parse};

fn source(path: &'static str, text: &'static str, embedded: bool, id: u16) -> Source<'static> {
    let (file, syntax) = parse(FileId(id), text, Folder::of(path));
    assert!(syntax.is_empty(), "{path}: {syntax:?}");
    Source { path, file, embedded }
}

#[test]
fn native_budgets_are_linked_to_their_laws() {
    let std = source("std.ax", "system std\nkind bank : asset\ncommodity USD\n  precision 2\n", true, 0);
    let project = source(
        "axiom.ax",
        "use std\nbase USD\naccount checking : bank\naccount reserve : bank\naccount envelope : bank\npurpose groceries : spending\n  budget 100 USD monthly carries funded from checking into reserve\npurpose fees : spending\npurpose pay : income\npurpose hobbies : spending\n  budget 10% of #pay yearly carries\n2026-03-01 #groceries now budget 250 USD yearly funded from checking into envelope until 04-30\npurpose late : spending\n2026-03-01 #late now budget 30 USD monthly\n",
        false,
        1,
    );
    let (book, diagnostics) = build(&[std, project]);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(book.budgets.len(), 3);
    let purpose_order: Vec<_> = book.budgets.iter().map(|(_, budget)| budget.purpose.index()).collect();
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
    assert!(
        budget_law
            .nodes
            .iter()
            .any(|(_, node)| matches!(node.op, axiom_model::Op::Call(Func::BudgetLimit(id), _) if id == budget_id))
    );
    assert!(
        budget_law
            .nodes
            .iter()
            .any(|(_, node)| matches!(node.op, axiom_model::Op::Call(Func::BudgetTotal(id), _) if id == budget_id))
    );
    let changed = Day::parse(b"2026-03-15").unwrap();
    let restored = Day::parse(b"2026-05-01").unwrap();
    assert_eq!(budget.terms.at(changed).period, Period::Year);
    assert!(budget.terms.at(changed).carries, "omitted carries inherits the prior setting");
    assert!(
        matches!(budget.terms.at(changed).limit, Limit::Amount(amount) if book.show(amount).to_string() == "250.00 USD")
    );
    let initial_funding = budget.terms.at(Day::MIN).funded;
    assert_ne!(budget.terms.at(changed).funded, initial_funding);
    assert_eq!(budget.terms.at(restored).period, Period::Month);
    assert!(budget.terms.at(restored).carries, "until restores the entire prior terms row");
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
    assert_eq!(late.terms.at(Day::MIN).limit, Limit::Amount(axiom_model::Amount::zero(book.base)));
    assert!(
        matches!(late.terms.at(late.starts).limit, Limit::Amount(amount) if book.show(amount).to_string() == "30.00 USD")
    );
    let (share_id, share_budget) = book
        .budgets
        .iter()
        .find(|(_, budget)| matches!(budget.terms.at(Day::MIN).limit, Limit::Share { .. }))
        .expect("the share budget");
    assert!(matches!(share_budget.terms.at(Day::MIN).limit, Limit::Share { .. }));
    assert_eq!(book.laws[share_budget.law].budget, Some(share_id));
    assert!(
        book.laws[share_budget.law]
            .nodes
            .iter()
            .all(|(_, node)| !matches!(node.op, axiom_model::Op::Call(Func::PurposeTotal { .. }, _))),
        "Share dependencies are derived from Budget.terms by Plan, not duplicated as unused law nodes"
    );
}

#[test]
fn invalid_budget_until_does_not_install_a_zero_budget_law() {
    let std = source("std.ax", "system std\ncommodity USD\n  precision 2\n", true, 0);
    let (project_file, syntax_diagnostics) = parse(
        FileId(1),
        "use std\nbase USD\npurpose grocery : spending\n2026-03-01 #grocery now budget 100 USD monthly until 04-30 until 05-31\n",
        Folder::of("axiom.ax"),
    );
    assert!(syntax_diagnostics.iter().any(|diagnostic| diagnostic.code == "duplicate-clause"));
    let project = Source { path: "axiom.ax", file: project_file, embedded: false };
    let (book, diagnostics) = build(&[std, project]);
    assert!(diagnostics.is_empty(), "the malformed statement was rejected by parsing: {diagnostics:?}");
    assert!(book.budgets.is_empty(), "invalid terms must not install a budget");
    assert!(
        book.laws.iter().all(|(_, law)| law.budget.is_none()),
        "the rejected budget must not leave a reachable zero-limit warning law"
    );
}

#[test]
fn invalid_initial_budget_terms_do_not_leave_a_zero_budget_law() {
    let std = source("std.ax", "system std\ncommodity USD\n  precision 2\nkind bank : asset\n", true, 0);
    let project = source(
        "axiom.ax",
        "use std\nbase USD\naccount checking : bank\npurpose grocery : spending\n  budget 100 USD monthly funded from missing into checking\n",
        false,
        1,
    );
    let (book, diagnostics) = build(&[std, project]);
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.code == "unknown-place"),
        "bad funding is refused: {diagnostics:?}"
    );
    assert!(book.budgets.is_empty(), "failed initial terms must not install a budget");
    assert!(
        book.laws.iter().all(|(_, law)| law.budget.is_none()),
        "a failed initial term must not leave a zero-limit warning law"
    );
}

/// A declaration's `also` was once not read, and said so (`also-inert`). It is the law it abbreviates, so its lines are checked
/// as any law's lines are: the three of these say what is wrong with each.
#[test]
fn a_declarations_also_is_checked_as_the_law_it_abbreviates() {
    let std = source("std.ax", "system std\ncommodity USD\n  precision 2\nkind bank : asset\n", true, 0);
    let project = source(
        "axiom.ax",
        "use std\nbase USD\nentity me\nkind boss : entity\n  also + 5% of amount #nowhere\naccount checking : bank\npurpose fees : spending\n  also checking -> self 1 USD due 3d\naccount big : bank\n  also + 5% of amount #nowhere\n",
        false,
        1,
    );
    let (book, diagnostics) = build(&[std, project]);
    let codes: Vec<_> = diagnostics.iter().map(|diagnostic| diagnostic.code.as_ref()).collect();
    assert_eq!(codes, ["unknown-purpose", "also-relative-due", "unknown-purpose"], "{diagnostics:?}");
    assert!(
        book.laws.iter().all(|(_, law)| law.owner != Owner::Kind(book.kind("boss").unwrap())),
        "and a line that is wrong makes no law, as a law that is wrong makes none"
    );
}

#[test]
fn built_in_purpose_root_can_host_a_scoped_law_without_a_duplicate_node() {
    let std = source("std.ax", "system std\ncommodity USD\n  precision 2\n", true, 0);
    let project = source(
        "axiom.ax",
        "use std\nbase USD\nkind sole-proprietorship : entity\npurpose spending\n  law business-costs\n    on flow\n    when owner.kind is sole-proprietorship\n    count amount as schedule-c-expenses\n",
        false,
        1,
    );
    let (book, diagnostics) = build(&[std, project]);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");

    let spending = book.roots.purposes.spending;
    assert_eq!(
        book.purposes.iter().filter(|(_, purpose)| book.name(purpose.name) == "spending").count(),
        1,
        "the declaration extends the built-in identity"
    );
    let (law_id, law) = book
        .laws
        .iter()
        .find(|(_, law)| book.name(law.name) == "business-costs")
        .expect("the root purpose law was compiled");
    assert_eq!(law.owner, Owner::Purpose(spending));
    assert!(book.purposes[spending].laws.contains(&law_id));
}

#[test]
fn law_overrides_resolve_to_the_nearest_visible_system() {
    let std = source("std.ax", "system std\ncommodity USD\n  precision 2\n", true, 0);
    let parent = source("us.ax", "system us\nlaw standard\n  on in\n  warn value(amount, USD) > 1 USD\n", true, 1);
    let child = source(
        "ca.ax",
        "system us/ca\nlaw standard\n  on in\n  warn value(amount, USD) > 2 USD\nlaw state-standard overrides standard\n  on in\n  warn value(amount, USD) > 3 USD\n",
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
        "us/ca",
        "when a visible child and parent law share a name, override the nearest law"
    );
}

#[test]
fn law_override_names_ambiguous_between_equally_visible_systems() {
    let std = source("std.ax", "system std\ncommodity USD\n  precision 2\n", true, 0);
    let east = source("east.ax", "system us/east\nlaw shared\n  on in\n  warn value(amount, USD) > 1 USD\n", true, 1);
    let west = source("west.ax", "system us/west\nlaw shared\n  on in\n  warn value(amount, USD) > 1 USD\n", true, 2);
    let project = source(
        "axiom.ax",
        "use us/east\nuse us/west\nbase USD\nlaw project-rule overrides shared\n  on in\n  warn value(amount, USD) > 3 USD\n",
        false,
        3,
    );
    let (_, diagnostics) = build(&[std, east, west, project]);
    assert_eq!(
        diagnostics.iter().map(|diagnostic| diagnostic.code.as_ref()).collect::<Vec<_>>(),
        ["ambiguous-law"],
        "{diagnostics:?}"
    );
}
