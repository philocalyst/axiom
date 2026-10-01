use axiom_core::{Day, FileId, Id};
use axiom_model::{Source, build};
use axiom_syntax::{Folder, parse};

#[test]
fn journal_codes_are_interned_when_records_are_lowered() {
    let path = "journal/2026/01.ax";
    let text = "\
base USD
commodity USD
account assets/checking
2026-01-01 checking -> ? 1 USD ^wire-code
2026-01-02 ^event-code settled
2026-01-03 checking -> checking 1 USD
";
    let (file, syntax) = parse(FileId(0), text, Folder::of(path));
    assert!(syntax.is_empty(), "{syntax:?}");

    let (book, diagnostics) = build(&[Source {
        path,
        file,
        embedded: false,
    }]);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert!(
        book.codes
            .iter()
            .any(|(_, code)| book.name(*code) == "wire-code")
    );
    assert_eq!(book.name(book.events[0].code), "event-code");
    let (flow_id, flow) = book.flows.iter().next().unwrap();
    assert!(book.touching[flow.from].contains(&flow_id));
    assert!(book.touching[flow.to].contains(&flow_id));
    assert_record_indices(&book);
}

#[test]
fn malformed_journal_text_returns_diagnostics_without_panicking() {
    let path = "journal/2026/01.ax";
    let text = "\
base USD
commodity USD
account assets/checking
2026-01-01 checking -> ? 1 USD ^still-lowerable ^
";
    let (file, syntax) = parse(FileId(0), text, Folder::of(path));
    assert!(
        !syntax.is_empty(),
        "the malformed trailing code needs a syntax diagnostic"
    );
    let (book, _diagnostics) = build(&[Source {
        path,
        file,
        embedded: false,
    }]);
    assert!(book.txns.len() <= 1);
}

#[test]
fn computed_contract_terms_use_the_native_amount_grammar() {
    let text = "\
base USD
commodity USD
commodity VTI
account assets/checking
contract c with p
  12% of 100 USD monthly from checking
  buy VTI for 3/4 of 100 USD monthly from checking
  + 5% of 100 USD
  due 5d else + 2% of 100 USD
";
    let (file, syntax) = parse(FileId(0), text, Folder::of("contracts.ax"));
    assert!(syntax.is_empty(), "{syntax:?}");
    assert_eq!(file.items.len(), 5);
    let (book, diagnostics) = build(&[Source {
        path: "contracts.ax",
        file,
        embedded: false,
    }]);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let contract = &book.contracts[Id::new(0)];
    let day = Day::from_ymd(2026, 1, 1).unwrap();
    assert!(
        !contract
            .terms
            .as_ref()
            .unwrap()
            .at(day)
            .program
            .nodes
            .is_empty()
    );
    assert!(
        !contract
            .standing
            .as_ref()
            .unwrap()
            .at(day)
            .program
            .nodes
            .is_empty()
    );
}

#[test]
fn computed_balance_assertions_keep_a_sparse_typed_program() {
    let path = "journal/2026/01.ax";
    let text = "\
base USD
commodity USD
account assets/checking
2026-01-31 checking = 50% of 100 USD
";
    let (file, syntax) = parse(FileId(0), text, Folder::of(path));
    assert!(syntax.is_empty(), "{syntax:?}");

    let (book, diagnostics) = build(&[Source {
        path,
        file,
        embedded: false,
    }]);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(book.assertion_programs.len(), 1);
    let assertion = &book.asserts[0];
    assert!(matches!(
        assertion.subject,
        axiom_model::law::Subject::Place(place) if place == assertion.place
    ));
    let Some((program, root)) = assertion.computed else {
        panic!("the computed amount should retain a program root");
    };
    assert_eq!(assertion.amount.unit, book.base);
    assert!(
        book.assertion_programs[program].nodes[root]
            .typed_ty()
            .is_some()
    );
    assert_record_indices(&book);
}

#[test]
fn computed_asset_assertions_compile_with_asset_fields() {
    let path = "journal/2026/01.ax";
    let text = "\
base USD
commodity USD
kind property : thing
asset condo : property
2026-01-31 condo = 50% of self.cost
";
    let (file, syntax) = parse(FileId(0), text, Folder::of(path));
    assert!(syntax.is_empty(), "{syntax:?}");

    let (book, diagnostics) = build(&[Source {
        path,
        file,
        embedded: false,
    }]);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let assertion = &book.asserts[0];
    assert!(matches!(
        assertion.subject,
        axiom_model::law::Subject::Asset(_)
    ));
    let Some((program, root)) = assertion.computed else {
        panic!("the computed amount should retain a program root");
    };
    assert!(matches!(
        book.assertion_programs[program].nodes[root].typed_ty(),
        Some(axiom_model::law::Ty::Amount(_))
    ));
}

#[test]
fn filed_returns_keep_reported_tallies_and_events() {
    let (system_file, system_errors) = parse(
        FileId(1),
        "system us\ncurrency USD\n",
        Folder::of("systems/us.ax"),
    );
    assert!(system_errors.is_empty(), "{system_errors:?}");
    let path = "journal/2026/04.ax";
    let (file, syntax) = parse(
        FileId(0),
        "base USD\ncommodity USD\n2026-04-15 us filed 2025\n  wages 124_200 USD\n  tax-withheld 11_952 USD\n2026-04-16 ^check-1041 settled\n",
        Folder::of(path),
    );
    assert!(syntax.is_empty(), "{syntax:?}");

    let (book, diagnostics) = build(&[
        Source {
            path: "systems/us.ax",
            file: system_file,
            embedded: false,
        },
        Source {
            path,
            file,
            embedded: false,
        },
    ]);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(book.filed.len(), 1);
    let filed = &book.filed[0];
    assert_eq!(filed.year, 2025);
    assert_eq!(filed.lines.len(), 2);
    assert_eq!(book.name(filed.lines[0].0), "wages");
    assert_eq!(filed.lines[0].1.qty.0, 124_200);
    assert_eq!(book.events.len(), 1);
    assert_eq!(book.name(book.events[0].code), "check-1041");
    assert_eq!(book.events[0].state, axiom_syntax::EventState::Settled);
}

fn assert_record_indices(book: &axiom_model::book::Book<'_>) {
    for (txn_id, txn) in book.txns.iter() {
        let first = txn.flows.start().index();
        let end = first + txn.flows.len() as usize;
        assert!(
            end <= book.flows.len(),
            "transaction {txn_id:?} exceeds the flow arena"
        );
        for flow_id in txn.flows.ids() {
            assert_eq!(book.flows[flow_id].txn, txn_id);
        }

        let Some(program_id) = txn.program else {
            continue;
        };
        let program = &book.journal_programs[program_id];
        let local_flow = |offset: u32| assert!(offset < txn.flows.len());
        for root in program.flow_roots.iter() {
            local_flow(root.flow);
            for node in [root.out, root.arrive, root.basis].into_iter().flatten() {
                assert!(program.program.nodes[node].typed_ty().is_some());
            }
        }
        for group in program.groups.iter() {
            if let Some(header) = group.header {
                local_flow(header);
            }
            for &leg in group.legs.iter() {
                local_flow(leg);
            }
            for item in group.items.iter() {
                if let Some(flow) = item.flow {
                    local_flow(flow);
                }
                if let axiom_model::book::TemplateAmount::Computed(root) = item.amount {
                    assert!(program.program.nodes[root].typed_ty().is_some());
                }
            }
        }
    }

    for (place, ids) in book.touching.iter() {
        assert!(
            ids.windows(2)
                .all(|pair| pair[0].index() <= pair[1].index())
        );
        for &flow_id in ids {
            let flow = &book.flows[flow_id];
            assert!(flow.from == place || flow.to == place);
        }
    }
    for (flow_id, flow) in book.flows.iter() {
        assert_eq!(
            book.touching[flow.from]
                .iter()
                .filter(|&&id| id == flow_id)
                .count(),
            1
        );
        if flow.from != flow.to {
            assert_eq!(
                book.touching[flow.to]
                    .iter()
                    .filter(|&&id| id == flow_id)
                    .count(),
                1
            );
        }
    }
}
