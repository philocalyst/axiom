//! Source-driven checks for the native S5 declaration builder.
//!
//! Journal flows, grouped items and contract forecasts are tested beside their
//! lowerers and by the engine. These tests focus on the model boundary: typed
//! names, stable trees, ownership and once-stored property defaults.

use axiom_core::{Day, Days, Diagnostic, FileId, Ratio};
use axiom_syntax::{Folder, parse};

use crate::{
    Amount, Book, Conversion, ConversionError, PurposeRoot, RatePolicy, RateSource, Residence,
    Role, Sort, Source, Value, build, prop,
};

const STD: &str = "\
system std
kind bank : asset
kind employer : entity
kind person : entity
kind currency : commodity
kind property : thing
purpose groceries : spending
purpose wages : income
";

fn parsed_source<'s>(id: u16, path: &'s str, text: &'s str, embedded: bool) -> Source<'s> {
    let (file, diagnostics) = parse(FileId(id), text, Folder::default());
    assert!(
        diagnostics.is_empty(),
        "{path} should parse: {diagnostics:?}"
    );
    Source {
        path,
        file,
        embedded,
    }
}

fn build_book<'s>(std: &'s str, project: &'s str) -> (Book<'s>, Vec<Diagnostic>) {
    let sources = [
        parsed_source(0, "std.ax", std, true),
        parsed_source(1, "axiom.ax", project, false),
    ];
    build(&sources)
}

fn build_project(project: &str) -> (Book<'_>, Vec<Diagnostic>) {
    build_book(STD, project)
}

fn codes(diagnostics: &[Diagnostic]) -> Vec<&str> {
    diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code.as_ref())
        .collect()
}

#[test]
fn native_roots_and_declarations_have_typed_identity() {
    let project = "\
use std
base USD
commodity USD : currency
  precision 2
entity me : person
account checking : bank
asset condo : property
";
    let (book, diagnostics) = build_project(project);

    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(book.commodities[book.commodity("USD").unwrap()].scale, 2);
    assert_eq!(
        book.kinds[book.roots.kinds.asset].sort,
        Sort::Place(crate::Class::Asset)
    );
    assert_eq!(
        book.kinds[book.roots.kinds.debt].sort,
        Sort::Place(crate::Class::Debt)
    );
    assert_eq!(book.kinds[book.kind("property").unwrap()].sort, Sort::Thing);
    assert_eq!(
        [
            book.purposes[book.roots.purposes.income].root,
            book.purposes[book.roots.purposes.spending].root,
            book.purposes[book.roots.purposes.capital].root,
            book.purposes[book.roots.purposes.transfer].root,
        ],
        [
            PurposeRoot::Income,
            PurposeRoot::Spending,
            PurposeRoot::Capital,
            PurposeRoot::Transfer
        ],
    );

    let me = book.entity("me").unwrap();
    let checking = book.place("checking").unwrap();
    let condo = book.asset("condo").unwrap();
    assert_eq!(book.places[checking].owner, me);
    assert!(matches!(
        book.places[checking].role,
        Role::Account { institution: None }
    ));
    assert_eq!(book.assets[condo].kind, book.kind("property").unwrap());
}

#[test]
fn kind_defaults_inherit_by_reference_and_keep_the_written_source() {
    let std = "\
system std
kind durable : thing
  has service-life number
  service-life 30
kind durable-home : durable
  service-life 24
kind rental-home : durable-home
  service-life 18
kind person : entity
kind currency : commodity
";
    let project = "\
use std
base USD
commodity USD : currency
asset condo : rental-home
";
    let (book, diagnostics) = build_book(std, project);

    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let parent = book.kind("durable").unwrap();
    let middle = book.kind("durable-home").unwrap();
    let child = book.kind("rental-home").unwrap();
    let asset = book.asset("condo").unwrap();
    let name = book.names.get("service-life").unwrap();
    let parent_default = prop(&book.kinds[parent].props, name, Day::MAX).unwrap();
    let middle_default = prop(&book.kinds[middle].props, name, Day::MAX).unwrap();
    let child_default = prop(&book.kinds[child].props, name, Day::MAX).unwrap();

    assert_eq!(parent_default.value, Value::Num(Ratio::int(30)));
    assert_eq!(middle_default.value, Value::Num(Ratio::int(24)));
    assert_eq!(child_default.value, Value::Num(Ratio::int(18)));
    assert!(
        parent_default.loc.is_some() && middle_default.loc.is_some() && child_default.loc.is_some()
    );
    assert!(book.kinds[child].has.iter().any(|has| has.name == name));
    assert!(
        book.assets[asset].props.is_empty(),
        "defaults stay on their kinds"
    );
}

#[test]
fn kind_traits_inherit_and_nearest_explicit_traits_override() {
    let std = "\
system std
kind durable : asset
  deferred
  basis zero
  claim
  select fifo
kind residential : durable
  basis cost
  select hifo
kind person : entity
kind currency : commodity
";
    let project = "\
use std
base USD
commodity USD : currency
account retirement : residential
";
    let (book, diagnostics) = build_book(std, project);

    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let durable = book.kind("durable").unwrap();
    let residential = book.kind("residential").unwrap();
    let account = book.place("retirement").unwrap();
    assert!(book.kinds[durable].deferred);
    assert!(book.kinds[durable].claim);
    assert_eq!(book.kinds[durable].basis, Some(crate::Basis::Zero));
    assert_eq!(book.kinds[residential].basis, Some(crate::Basis::Cost));
    assert_eq!(book.kinds[residential].select, Some(crate::Policy::Hifo));
    assert!(book.places[account].deferred && book.places[account].claim);
    assert_eq!(book.places[account].basis, crate::Basis::Cost);
    assert_eq!(book.places[account].select, Some(crate::Policy::Hifo));
}

#[test]
fn typed_kind_purpose_takes_and_pays_are_resolved_and_inherited() {
    let std = "\
system std
kind payroll : entity
  purpose wages
kind retirement : asset
  takes transfer from wages
kind roth : retirement
  takes groceries from wages
kind college : asset
  pays groceries
kind person : entity
kind currency : commodity
purpose wages : income
purpose groceries : spending
";
    let project = "\
use std
base USD
commodity USD : currency
";
    let (book, diagnostics) = build_book(std, project);

    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let payroll = book.kinds[book.kind("payroll").unwrap()]
        .purpose
        .unwrap()
        .value;
    assert_eq!(book.name(book.purposes[payroll].name), "wages");
    let retirement = book.kind("retirement").unwrap();
    let roth = book.kind("roth").unwrap();
    let college = book.kind("college").unwrap();
    let wages = book.purpose("wages").unwrap();
    let groceries = book.purpose("groceries").unwrap();
    let transfer = book.purpose("transfer").unwrap();
    assert_eq!(book.kinds[retirement].takes.len(), 1);
    assert_eq!(book.kinds[retirement].takes[0].value.to, transfer);
    assert_eq!(book.kinds[roth].takes.len(), 1);
    assert_eq!(
        book.kinds[roth].takes[0].value,
        crate::Take {
            to: groceries,
            from: wages
        }
    );
    assert_eq!(book.kinds[college].pays.unwrap().value, groceries);
}

#[test]
fn kinds_with_digits_and_multiple_declared_currencies_keep_their_identity() {
    let std = "\
system std
kind person : entity
kind currency : commodity
kind retirement : asset
";
    let project = "\
use std
base USD
commodity USD : currency
commodity EUR : currency
kind 401k : retirement
account plan : 401k
";
    let (book, diagnostics) = build_book(std, project);

    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(
        book.name(book.commodities[book.commodity("USD").unwrap()].symbol),
        "USD"
    );
    assert_eq!(
        book.name(book.commodities[book.commodity("EUR").unwrap()].symbol),
        "EUR"
    );
    assert_eq!(
        book.name(book.kinds[book.kind("401k").unwrap()].name),
        "401k"
    );
    assert_eq!(
        book.places[book.place("plan").unwrap()].kind,
        book.kind("401k").unwrap()
    );
    assert_eq!(book.base, book.commodity("USD").unwrap());
}

#[test]
fn an_invalid_kind_share_reports_its_exact_source_span() {
    let project = "\
use std
base USD
commodity USD : currency
kind distributed : entity
  share 120% for me
";
    let (_, diagnostics) = build_project(project);

    assert_eq!(codes(&diagnostics), ["share-rate-range"], "{diagnostics:?}");
    let diagnostic = &diagnostics[0];
    let loc = diagnostic
        .anchor()
        .expect("the invalid rate is source anchored");
    assert_eq!(&project[loc.start as usize..loc.end as usize], "120%");
}

#[test]
fn native_asset_part_cycles_are_diagnosed_before_the_book_is_returned() {
    let project = "\
use std
base USD
commodity USD : currency
asset north : property
  part of south
asset south : property
  part of north
";
    let (book, diagnostics) = build_project(project);

    assert_eq!(codes(&diagnostics), ["asset-part-cycle"], "{diagnostics:?}");
    assert_eq!(
        book.assets
            .iter()
            .filter(|(_, asset)| asset.part_of.is_none())
            .count(),
        1
    );
}

#[test]
fn kind_parent_cycles_are_reported_and_cut_before_the_tree_is_frozen() {
    let std = "\
system std
kind alpha : beta
kind beta : alpha
kind person : entity
kind currency : commodity
";
    let project = "\
use std
base USD
commodity USD : currency
";
    let (book, diagnostics) = build_book(std, project);

    assert_eq!(codes(&diagnostics), ["kind-cycle"], "{diagnostics:?}");
    let alpha = book.kind("alpha").unwrap();
    let beta = book.kind("beta").unwrap();
    assert!(!book.is_a(alpha, beta));
    assert!(!book.is_a(beta, alpha));
}

#[test]
fn owner_shares_are_resolved_once_with_exact_weights_and_locations() {
    let project = "\
use std
base USD
commodity USD : currency
entity me : person
entity jo : person
entity studio : employer
  owner me 60%, jo 40%
";
    let (book, diagnostics) = build_project(project);

    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let studio = book.entity("studio").unwrap();
    let owners = &book.entities[studio].owned_by;
    assert_eq!(owners.len(), 2);
    assert_eq!(owners[0].entity, book.entity("me").unwrap());
    assert_eq!(owners[0].rate, Ratio::new(3, 5).unwrap());
    assert_eq!(owners[1].entity, book.entity("jo").unwrap());
    assert_eq!(owners[1].rate, Ratio::new(2, 5).unwrap());
    assert!(
        owners
            .iter()
            .all(|share| share.loc != axiom_core::Loc::default())
    );
}

#[test]
fn one_entity_declaration_registers_every_name_once() {
    let project = "\
use std
base USD
commodity USD : currency
entity alice, bob : person
";
    let (book, diagnostics) = build_project(project);

    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let alice = book.entity("alice").unwrap();
    let bob = book.entity("bob").unwrap();
    assert_ne!(alice, bob);
    assert_eq!(book.entities[alice].kind, book.entities[bob].kind);
}

#[test]
fn place_lookup_reports_ambiguous_suffix_but_accepts_the_qualified_path() {
    let project = "\
use std
base USD
commodity USD : currency
account personal/checking : bank
account business/checking : bank
";
    let (book, diagnostics) = build_project(project);

    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert!(matches!(
        book.place("checking"),
        Err(crate::Miss::Ambiguous(_))
    ));
    let business = book.place("business/checking").unwrap();
    assert_eq!(book.name(book.places[business].path), "business/checking");
}

#[test]
fn an_unknown_kind_parent_has_one_source_anchored_diagnostic() {
    let project = "\
use std
base USD
commodity USD : currency
kind deferred-account : missing-kind
";
    let (_, diagnostics) = build_project(project);

    assert_eq!(codes(&diagnostics), ["unknown-kind"], "{diagnostics:?}");
    let loc = diagnostics[0].anchor().unwrap();
    assert_eq!(
        &project[loc.start as usize..loc.end as usize],
        "missing-kind"
    );
}

#[test]
fn a_near_miss_kind_parent_has_one_source_anchored_edit() {
    let project = "\
use std
base USD
commodity USD : currency
kind checking : bankk
";
    let (_, diagnostics) = build_project(project);

    assert_eq!(codes(&diagnostics), ["unknown-kind"], "{diagnostics:?}");
    let diagnostic = &diagnostics[0];
    let edit = diagnostic
        .help
        .iter()
        .find_map(|help| help.edit.as_ref())
        .expect("near miss should have an exact source edit");
    assert_eq!(
        &project[edit.0.start as usize..edit.0.end as usize],
        "bankk"
    );
    assert_eq!(edit.1, "bank");
}

#[test]
fn reserved_entity_names_are_refused_at_the_declared_word() {
    let project = "\
use std
base USD
commodity USD : currency
entity opening : person
";
    let (_, diagnostics) = build_project(project);

    assert_eq!(
        codes(&diagnostics),
        ["reserved-entity-name"],
        "{diagnostics:?}"
    );
    let loc = diagnostics[0].anchor().unwrap();
    assert_eq!(&project[loc.start as usize..loc.end as usize], "opening");
}

#[test]
fn a_base_is_required_when_no_default_currency_is_declared() {
    let project = "\
use std
commodity EUR : currency
commodity GBP : currency
";
    let (_, diagnostics) = build_project(project);

    assert_eq!(
        codes(&diagnostics),
        ["base-currency-required"],
        "{diagnostics:?}"
    );
}

#[test]
fn journal_codes_are_interned_when_native_records_use_them() {
    let project = "\
use std
base USD
commodity USD : currency
account checking : bank
account savings : bank
2026-01-01 checking -> savings 2 USD ^unlisted-flow-code
2026-02-01 ^unlisted-event-code settled
";
    let (book, diagnostics) = build_project(project);

    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    for code in ["unlisted-flow-code", "unlisted-event-code"] {
        let symbol = book
            .names
            .get(code)
            .expect("the native record interns its code");
        assert_eq!(book.name(symbol), code);
    }
    assert_eq!(book.events.len(), 1);
}

#[test]
fn every_lives_line_adds_its_system_and_overlapping_residences_are_kept() {
    let std = "\
system std
kind person : entity
kind currency : commodity
commodity USD : currency
commodity CAD : currency
";
    let california = "\
system us/ca
use std
currency CAD
";
    let new_york = "\
system us/ny
use std
currency USD
";
    let project = "\
use std
base USD
entity jo : person
  lives us/ca from 2025-01-01 until 2025-12-31
  lives us/ny from 2025-07-01
";
    let sources = [
        parsed_source(0, "std.ax", std, true),
        parsed_source(1, "us/ca.ax", california, true),
        parsed_source(2, "us/ny.ax", new_york, true),
        parsed_source(3, "axiom.ax", project, false),
    ];
    let (book, diagnostics) = build(&sources);

    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let residences = &book.entities[book.entity("jo").unwrap()].lives;
    assert_eq!(residences.len(), 2);
    assert_eq!(book.name(book.systems[residences[0].system].path), "us/ca");
    assert_eq!(book.name(book.systems[residences[1].system].path), "us/ny");
    assert_eq!(
        book.systems[residences[0].system].currency,
        book.commodity("CAD")
    );
    assert_eq!(
        book.systems[residences[1].system].currency,
        book.commodity("USD")
    );
    assert_eq!(
        residences[0].days.first(),
        Day::from_ymd(2025, 1, 1).unwrap()
    );
    assert_eq!(
        residences[0].days.last(),
        Day::from_ymd(2025, 12, 31).unwrap()
    );
    assert_eq!(
        residences[1].days.first(),
        Day::from_ymd(2025, 7, 1).unwrap()
    );
    assert_eq!(residences[1].days.last(), Day::MAX);
}

fn build_rate_book<'s>(project: &'s str) -> (Book<'s>, Vec<Diagnostic>) {
    const US: &str = "system us\nuse std\ncurrency USD\nrates spot\n";
    const US_CA: &str = "system us/ca\nuse std\n";
    const DE: &str =
        "system de\nuse std\ncurrency EUR\nrates param fx\nparam fx\n  2026 EUR USD 1.1\n";
    let sources = [
        parsed_source(0, "std.ax", STD, true),
        parsed_source(1, "us.ax", US, true),
        parsed_source(2, "de.ax", DE, true),
        parsed_source(3, "us/ca.ax", US_CA, true),
        parsed_source(4, "axiom.ax", project, false),
    ];
    build(&sources)
}

#[test]
fn owner_rate_policy_converts_with_parameter_evidence_and_explicit_spot_override() {
    let project = "\
use std
use us
use de
base USD
commodity USD : currency
  precision 2
commodity EUR : currency
  precision 2
commodity GBP : currency
  precision 2
commodity CAD : currency
  precision 2
entity me : person
  lives de from 2026-01-01
";
    let (mut book, diagnostics) = build_rate_book(project);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let owner = book.roots.me;
    let day = Day::from_ymd(2026, 6, 30).unwrap();
    let usd = book.commodity("USD").unwrap();
    let eur = book.commodity("EUR").unwrap();
    let amount = Amount::new(axiom_core::Qty(1_000), eur);
    let converted = book.convert_for(amount, usd, owner, day, None).unwrap();
    assert_eq!(converted.amount(), Amount::new(axiom_core::Qty(1_100), usd));
    assert_eq!(converted.rate(), Some(Ratio::new(11, 10).unwrap()));
    let Some(path) = converted.path() else {
        panic!("a table conversion has rate evidence")
    };
    let (first, second) = (path.first, path.second);
    assert_eq!(second, None);
    assert_eq!((first.from, first.to), (eur, usd));
    let (param_id, param) = book.params.iter().next().unwrap();
    let row_loc = param.rows[0].loc;
    assert_eq!(
        first.source,
        RateSource::Param {
            param: param_id,
            row: 0,
            since: Some(Day::from_ymd(2026, 1, 1).unwrap()),
            inverted: false,
            loc: row_loc,
        }
    );

    let inverse = book
        .convert_for(
            Amount::new(axiom_core::Qty(1_000), usd),
            eur,
            owner,
            day,
            None,
        )
        .unwrap();
    assert_eq!(inverse.amount(), Amount::new(axiom_core::Qty(909), eur));
    assert_eq!(inverse.rate(), Some(Ratio::new(10, 11).unwrap()));
    assert!(matches!(
        inverse.path().unwrap().first.source,
        RateSource::Param { inverted: true, .. }
    ));

    // Dimensionless tables still carry an explicit unit declaration.
    book.params[param_id].unit = Some(axiom_core::Dim::Number);
    let dimensionless = book.convert_for(amount, usd, owner, day, None).unwrap();
    assert_eq!(dimensionless.rate(), Some(Ratio::new(11, 10).unwrap()));

    let gbp = book.commodity("GBP").unwrap();
    book.prices = crate::Prices::new(vec![
        crate::Quote {
            unit: eur,
            quote: usd,
            day,
            rate: Ratio::new(6, 5).unwrap(),
            implied: false,
            loc: axiom_core::Loc::default(),
        },
        crate::Quote {
            unit: usd,
            quote: gbp,
            day,
            rate: Ratio::new(4, 5).unwrap(),
            implied: false,
            loc: axiom_core::Loc::default(),
        },
    ]);
    let cross = book
        .convert_for(amount, gbp, owner, day, Some(RatePolicy::Spot))
        .unwrap();
    assert_eq!(cross.amount(), Amount::new(axiom_core::Qty(960), gbp));
    let cross_path = cross.path().unwrap();
    assert_eq!(cross_path.first.to, usd);
    assert_eq!(cross_path.second.unwrap().from, usd);

    assert_eq!(
        book.convert_for(
            Amount::new(axiom_core::Qty(1), book.commodity("CAD").unwrap()),
            gbp,
            owner,
            day,
            Some(RatePolicy::Spot),
        ),
        Err(ConversionError::Missing {
            from: book.commodity("CAD").unwrap(),
            to: gbp,
            day,
            policy: RatePolicy::Spot,
        })
    );

    let zero = book
        .convert_for(Amount::new(axiom_core::Qty(0), eur), usd, owner, day, None)
        .unwrap();
    assert_eq!(zero.amount(), Amount::new(axiom_core::Qty(0), usd));
    assert_eq!(zero.rate(), None);
    assert!(matches!(zero, Conversion::Zero { .. }));

    let quote_day = Day::from_ymd(2026, 6, 29).unwrap();
    book.prices = crate::Prices::new(vec![crate::Quote {
        unit: eur,
        quote: usd,
        day: quote_day,
        rate: Ratio::new(6, 5).unwrap(),
        implied: false,
        loc: axiom_core::Loc::default(),
    }]);
    let explicit = book
        .convert_for(amount, usd, owner, day, Some(RatePolicy::Spot))
        .unwrap();
    assert_eq!(explicit.amount(), Amount::new(axiom_core::Qty(1_200), usd));
    assert!(matches!(
        explicit.path().unwrap().first.source,
        RateSource::Spot {
            as_of,
            inverted: false,
            implied: false,
            ..
        } if as_of == quote_day
    ));
}

#[test]
fn descendant_residence_policy_supersedes_ancestor_independent_of_order() {
    let project = "\
use std
use us
use de
base USD
commodity USD : currency
  precision 2
commodity EUR : currency
  precision 2
entity me : person
";
    let (mut book, diagnostics) = build_rate_book(project);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let owner = book.roots.me;
    let system = |book: &Book<'_>, path| {
        book.systems
            .iter()
            .find_map(|(id, node)| (book.name(node.path) == path).then_some(id))
            .unwrap()
    };
    let (us, de) = (system(&book, "us"), system(&book, "de"));
    let child = system(&book, "us/ca");
    let param = book.params.iter().next().unwrap().0;
    book.systems[child].rates = Some(RatePolicy::Param(param));
    let residences = [
        Residence {
            days: Days::ALWAYS,
            system: de,
        },
        Residence {
            days: Days::ALWAYS,
            system: us,
        },
        Residence {
            days: Days::ALWAYS,
            system: child,
        },
    ];
    let day = Day::from_ymd(2026, 6, 30).unwrap();
    let amount = Amount::new(axiom_core::Qty(1_000), book.commodity("EUR").unwrap());
    for lives in [residences, [residences[2], residences[0], residences[1]]] {
        book.entities[owner].lives = lives.into();
        let converted = book
            .convert_for(amount, book.base, owner, day, None)
            .unwrap();
        assert_eq!(converted.rate(), Some(Ratio::new(11, 10).unwrap()));
    }
}

#[test]
fn conflicting_overlapping_residence_rate_policies_are_reported_deterministically() {
    let project = "\
use std
use us
use de
base USD
commodity USD : currency
  precision 2
commodity EUR : currency
  precision 2
entity me : person
";
    let (mut book, diagnostics) = build_rate_book(project);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let owner = book.roots.me;
    let system = |path| {
        book.systems
            .iter()
            .find_map(|(id, node)| (book.name(node.path) == path).then_some(id))
            .unwrap()
    };
    let (us, de) = (system("us"), system("de"));
    book.entities[owner].lives = vec![
        Residence {
            days: Days::ALWAYS,
            system: us,
        },
        Residence {
            days: Days::ALWAYS,
            system: de,
        },
    ]
    .into();
    let amount = Amount::new(axiom_core::Qty(100), book.commodity("EUR").unwrap());
    let day = Day::from_ymd(2026, 6, 30).unwrap();
    let expected = Err(ConversionError::PolicyConflict {
        first: us.min(de),
        second: us.max(de),
    });
    assert_eq!(
        book.convert_for(amount, book.base, owner, day, None),
        expected
    );
    book.entities[owner].lives.reverse();
    assert_eq!(
        book.convert_for(amount, book.base, owner, day, None),
        expected
    );
}

#[test]
fn known_exchange_adds_a_spot_quote_used_by_owner_conversion() {
    let project = "\
use std
base USD
commodity USD : currency
  precision 2
kind fund : commodity
commodity VTI : fund
account checking : bank
account brokerage : bank
2026-02-10 checking 1_999.90 USD -> brokerage 7 VTI
";
    let (book, diagnostics) = build_project(project);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let vti = book.commodity("VTI").unwrap();
    let usd = book.commodity("USD").unwrap();
    let day = Day::from_ymd(2026, 2, 10).unwrap();
    let quote = book
        .prices
        .quotes()
        .iter()
        .find(|quote| quote.unit == vti && quote.quote == usd && quote.day == day)
        .expect("a known actual exchange records an implied quote");
    assert!(quote.implied);
    assert_eq!(quote.rate, Ratio::new(2857, 10).unwrap());
    let conversion = book
        .convert_for(
            Amount::new(axiom_core::Qty(7), vti),
            usd,
            book.roots.me,
            day,
            None,
        )
        .unwrap();
    assert_eq!(
        conversion.amount(),
        Amount::new(axiom_core::Qty(199_990), usd)
    );
}

#[test]
fn written_same_day_price_beats_an_implied_exchange_quote() {
    let project = "\
use std
base USD
commodity USD : currency
  precision 2
kind fund : commodity
commodity VTI : fund
account checking : bank
account brokerage : bank
2026-02-10 VTI = 300 USD
2026-02-10 checking 1_999.90 USD -> brokerage 7 VTI
";
    let (book, diagnostics) = build_project(project);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let vti = book.commodity("VTI").unwrap();
    let usd = book.commodity("USD").unwrap();
    let day = Day::from_ymd(2026, 2, 10).unwrap();
    let pair: Vec<_> = book
        .prices
        .quotes()
        .iter()
        .filter(|quote| quote.unit == vti && quote.quote == usd && quote.day == day)
        .collect();
    assert_eq!(
        pair.len(),
        2,
        "the written quote and implied evidence are both retained"
    );
    assert!(pair[0].implied);
    let quote = pair[1];
    assert!(!quote.implied);
    assert_eq!(quote.rate, Ratio::new(300, 1).unwrap());
    let conversion = book
        .convert_for(
            Amount::new(axiom_core::Qty(7), vti),
            usd,
            book.roots.me,
            day,
            Some(RatePolicy::Spot),
        )
        .unwrap();
    assert_eq!(
        conversion.amount(),
        Amount::new(axiom_core::Qty(210_000), usd)
    );
}
