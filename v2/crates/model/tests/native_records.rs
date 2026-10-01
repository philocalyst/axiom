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
fn opening_records_a_whole_asset_with_basis_and_acquisition_day() {
    let path = "journal/2026/01.ax";
    let text = "\
base USD
commodity USD
kind property : thing
asset condo : property
opening 2026-01-01
  condo basis 402_000 USD since 2024-02-20
";
    let (file, syntax) = parse(FileId(0), text, Folder::of(path));
    assert!(syntax.is_empty(), "{syntax:?}");

    let (book, diagnostics) = build(&[Source {
        path,
        file,
        embedded: false,
    }]);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let asset = book.asset("condo").unwrap();
    let (_, flow) = book.flows.iter().next().unwrap();
    assert_eq!(flow.to, book.assets[asset].place);
    assert_eq!(
        flow.arrive,
        axiom_model::Amount::new(axiom_core::Qty(1), book.assets[asset].unit)
    );
    assert_eq!(flow.mode, axiom_model::Mode::Opening);
    let detail = &book.details[flow.detail.unwrap()];
    assert_eq!(detail.basis, Some(axiom_core::Qty(402_000)));
    assert_eq!(detail.since, Some(Day::from_ymd(2024, 2, 20).unwrap()));
    assert_record_indices(&book);
}

#[test]
fn opening_claims_create_typed_tabs_with_due_dates() {
    let path = "journal/2026/01.ax";
    let text = "\
base USD
commodity USD
kind person : entity
entity jo : person
opening 2026-01-01
  jo owes me 600 USD due 2026-01-20 ^loan
";
    let (file, syntax) = parse(FileId(0), text, Folder::of(path));
    assert!(syntax.is_empty(), "{syntax:?}");

    let (book, diagnostics) = build(&[Source {
        path,
        file,
        embedded: false,
    }]);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(book.txns.len(), 2);
    let (_, flow) = book.flows.iter().next().unwrap();
    assert_eq!(flow.mode, axiom_model::Mode::Opening);
    let jo = book.entity("jo").unwrap();
    assert_eq!(book.places[flow.to].role, axiom_model::Role::Tab(jo));
    assert!(book.places[flow.to].claim, "claim tabs retain their built-in trait");
    assert_eq!(book.name(book.codes[flow.header_codes.start()]), "loan");
    let detail = &book.details[flow.detail.unwrap()];
    assert_eq!(detail.due, Some(Day::from_ymd(2026, 1, 20).unwrap()));
    assert_record_indices(&book);
}

#[test]
fn asset_basis_statement_records_one_new_parcel() {
    let path = "journal/2026/01.ax";
    let text = "\
base USD
commodity USD
kind property : thing
asset condo : property
2026-01-02 condo basis 402_000 USD since 2025-12-01
";
    let (file, syntax) = parse(FileId(0), text, Folder::of(path));
    assert!(syntax.is_empty(), "{syntax:?}");

    let (book, diagnostics) = build(&[Source {
        path,
        file,
        embedded: false,
    }]);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let asset = book.asset("condo").unwrap();
    let (_, flow) = book.flows.iter().next().unwrap();
    assert_eq!(flow.from, book.entities[book.roots.unknown].place.unwrap());
    assert_eq!(flow.to, book.assets[asset].place);
    assert_eq!(
        flow.arrive,
        axiom_model::Amount::new(axiom_core::Qty(1), book.assets[asset].unit)
    );
    assert_eq!(flow.mode, axiom_model::Mode::Actual);
    let detail = &book.details[flow.detail.unwrap()];
    assert_eq!(detail.basis, Some(axiom_core::Qty(402_000)));
    assert_eq!(detail.since, Some(Day::from_ymd(2025, 12, 1).unwrap()));
    assert_record_indices(&book);
}

#[test]
fn against_resolves_a_unique_earlier_transaction_for_flows_and_measures() {
    let path = "journal/2026/01.ax";
    let text = "\
base USD
commodity USD
commodity HR
account assets/checking
2026-01-01 checking -> ? 10 USD ^invoice
2026-01-02 checking -> ? 3 USD against ^invoice
2026-01-03 me worked 5 HR against ^invoice
";
    let (file, syntax) = parse(FileId(0), text, Folder::of(path));
    assert!(syntax.is_empty(), "{syntax:?}");

    let (book, diagnostics) = build(&[Source {
        path,
        file,
        embedded: false,
    }]);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let referenced = &book.details[book.flows[Id::new(1)].detail.unwrap()];
    assert_eq!(referenced.against, Some(Id::new(0)));
    assert_eq!(book.measures[Id::new(0)].against, Some(Id::new(0)));
}

#[test]
fn against_rejects_unknown_and_ambiguous_transaction_codes() {
    let cases = [
        (
            "2026-01-01 checking -> ? 3 USD against ^missing\n",
            "unknown-against",
        ),
        (
            "2026-01-01 checking -> ? 1 USD ^invoice\n2026-01-02 checking -> ? 2 USD ^invoice\n2026-01-03 checking -> ? 3 USD against ^invoice\n",
            "ambiguous-against",
        ),
    ];
    for (records, expected) in cases {
        let path = "journal/2026/01.ax";
        let text = format!("base USD\ncommodity USD\naccount assets/checking\n{records}");
        let (file, syntax) = parse(FileId(0), &text, Folder::of(path));
        assert!(syntax.is_empty(), "{syntax:?}");
        let (_, diagnostics) = build(&[Source {
            path,
            file,
            embedded: false,
        }]);
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == expected),
            "expected {expected}, got {diagnostics:?}"
        );
        if expected == "ambiguous-against" {
            let diagnostic = diagnostics
                .iter()
                .find(|diagnostic| diagnostic.code == expected)
                .unwrap();
            assert_eq!(diagnostic.labels.len(), 3, "{diagnostic:?}");
            assert!(diagnostic.labels[1].text.contains("matching transaction"));
            assert!(diagnostic.labels[2].text.contains("matching transaction"));
        }
    }
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
fn contract_area_is_the_typed_denominator_for_measured_shares() {
    let path = "contracts.ax";
    let text = "\
base USD
commodity USD
commodity SQFT : measure
kind person : entity
entity greystar : person
entity studio : person
account assets/checking
contract flat with greystar
  2_900 USD monthly from checking
  area 1_000 SQFT
  share 120 SQFT for studio
";
    let (file, syntax) = parse(FileId(0), text, Folder::of(path));
    assert!(syntax.is_empty(), "{syntax:?}");

    let (book, diagnostics) = build(&[Source {
        path,
        file,
        embedded: false,
    }]);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let contract = &book.contracts[Id::new(0)];
    let area = contract.area.expect("contract retains its typed area");
    let terms = contract.terms.as_ref().unwrap().at(Day::from_ymd(2026, 1, 1).unwrap());
    let share = &terms.shares[0];
    assert_eq!(share.rate, axiom_core::Ratio::new(3, 25).unwrap());
    assert_eq!(
        share.measure,
        Some((
            axiom_model::Amount::new(axiom_core::Qty(120), book.commodity("SQFT").unwrap()),
            area,
        ))
    );
}

#[test]
fn contract_area_rejects_zero_measure_before_lowering_the_contract() {
    let path = "contracts.ax";
    let text = "\
base USD
commodity USD
commodity SQFT : measure
kind person : entity
entity greystar : person
entity studio : person
account assets/checking
contract flat with greystar
  2_900 USD monthly from checking
  area 0 SQFT
  share 120 SQFT for studio
";
    let (file, syntax) = parse(FileId(0), text, Folder::of(path));
    assert!(syntax.is_empty(), "{syntax:?}");

    let (book, diagnostics) = build(&[Source {
        path,
        file,
        embedded: false,
    }]);
    assert!(diagnostics.iter().any(|diagnostic| diagnostic.code == "contract-area-positive"), "{diagnostics:?}");
    let contract = &book.contracts[Id::new(0)];
    assert!(contract.terms.is_none(), "an invalid area must not produce an active schedule");
    assert!(contract.area.is_none(), "an invalid area must not be retained");
}

#[test]
fn measured_shares_reject_missing_and_mismatched_denominators() {
    for (extra_unit, area, share) in [
        ("", "", "share 120 SQFT for studio"),
        ("commodity SQM : measure\n", "  area 1_000 SQFT\n", "share 120 SQM for studio"),
    ] {
        let path = "contracts.ax";
        let text = format!(
            "base USD\ncommodity USD\ncommodity SQFT : measure\n{extra_unit}kind person : entity\nentity greystar : person\nentity studio : person\naccount assets/checking\ncontract flat with greystar\n  2_900 USD monthly from checking\n{area}  {share}\n"
        );
        let (file, syntax) = parse(FileId(0), &text, Folder::of(path));
        assert!(syntax.is_empty(), "{syntax:?}");

        let (book, diagnostics) = build(&[Source {
            path,
            file,
            embedded: false,
        }]);
        assert!(
            diagnostics.iter().any(|diagnostic| diagnostic.code == "contract-share-measure"),
            "{diagnostics:?}"
        );
        let contract = &book.contracts[Id::new(0)];
        let terms = contract.terms.as_ref().unwrap().at(Day::from_ymd(2026, 1, 1).unwrap());
        assert!(terms.shares.is_empty(), "an invalid measured share must not be retained");
    }
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

#[test]
fn contract_waivers_and_endings_paint_timeline_and_bound_occurrences() {
    let path = "journal/2026/01.ax";
    let text = "\
base USD
commodity USD
account assets/checking
contract phone with carrier
  100 USD monthly on 1 from checking
2026-01-15 phone waived until 2026-02-15 ^pause \"pause\"
2026-02-20 phone ends
";
    let (file, syntax) = parse(FileId(0), text, Folder::of(path));
    assert!(syntax.is_empty(), "{syntax:?}");

    let (book, diagnostics) = build(&[Source {
        path,
        file,
        embedded: false,
    }]);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let contract = &book.contracts[Id::new(0)];
    let regular = contract.terms.as_ref().unwrap();
    let waived = Day::from_ymd(2026, 1, 20).unwrap();
    let restored = Day::from_ymd(2026, 2, 16).unwrap();
    assert_eq!(
        regular.at(waived).state,
        axiom_model::book::TermsState::Waived
    );
    assert_eq!(
        regular.at(restored).state,
        axiom_model::book::TermsState::Active
    );
    assert_eq!(contract.days.last(), Day::from_ymd(2026, 2, 20).unwrap());
    assert!(contract.ended.is_some());
    assert_eq!(
        book.name(regular.at(waived).change.unwrap().code.unwrap()),
        "pause"
    );
    assert_eq!(book.endings.len(), 1);
    assert_eq!(book.endings[0].day, Day::from_ymd(2026, 2, 20).unwrap());
    assert_eq!(
        book.endings[0].target,
        axiom_model::journal::EndTarget::Contract(Id::new(0))
    );
}

#[test]
fn end_events_retain_codes_and_descriptions() {
    let path = "journal/2026/01.ax";
    let text = "\
base USD
commodity USD
account assets/checking
kind property : thing
asset condo : property
2026-01-15 checking ends ^closed \"retired\"
2026-01-20 condo ends ^disposed \"given away\"
";
    let (file, syntax) = parse(FileId(0), text, Folder::of(path));
    assert!(syntax.is_empty(), "{syntax:?}");

    let (book, diagnostics) = build(&[Source {
        path,
        file,
        embedded: false,
    }]);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(book.endings.len(), 2);
    let ending = book.endings[0];
    assert_eq!(
        ending.target,
        axiom_model::journal::EndTarget::Place(book.place("checking").unwrap())
    );
    assert_eq!(book.name(book.codes[ending.codes.start()]), "closed");
    assert_eq!(book.text(ending.description.unwrap()), "retired");
    let disposal = book.endings[1];
    assert_eq!(
        disposal.target,
        axiom_model::journal::EndTarget::Asset(book.asset("condo").unwrap())
    );
    assert_eq!(book.name(book.codes[disposal.codes.start()]), "disposed");
    assert_eq!(book.text(disposal.description.unwrap()), "given away");
    assert!(
        book.flows.is_empty(),
        "an ending does not invent a monetary flow"
    );
}

#[test]
fn rejected_waiver_and_early_end_do_not_change_contract_terms() {
    let path = "journal/2026/01.ax";
    let cases = [
        (
            "base USD\ncommodity USD\ncontract phone with carrier\n  100 USD monthly from checking\n  from 2026-01-01\n2026-01-15 phone waived ^one ^two\n",
            "duplicate-waiver-code",
        ),
        (
            "base USD\ncommodity USD\ncontract phone with carrier\n  100 USD monthly from checking\n  from 2026-01-01\n2025-12-31 phone ends\n",
            "end-before-contract",
        ),
    ];
    for (text, expected) in cases {
        let (file, syntax) = parse(FileId(0), text, Folder::of(path));
        assert!(syntax.is_empty(), "{syntax:?}");
        let (book, diagnostics) = build(&[Source {
            path,
            file,
            embedded: false,
        }]);
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == expected),
            "expected {expected}, got {diagnostics:?}"
        );
        let contract = &book.contracts[Id::new(0)];
        assert_eq!(contract.days.first(), Day::from_ymd(2026, 1, 1).unwrap());
        assert_eq!(contract.days.last(), Day::MAX);
        assert_eq!(
            contract
                .terms
                .as_ref()
                .unwrap()
                .at(Day::from_ymd(2026, 1, 15).unwrap())
                .state,
            axiom_model::book::TermsState::Active
        );
        assert!(contract.ended.is_none());
        assert!(book.endings.is_empty());
    }
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
