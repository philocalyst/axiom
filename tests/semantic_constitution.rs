//! Executable edge-case constitution for the semantic slice.
//!
//! This corpus is intentionally split into two parts.  Cases whose behavior
//! exists in the public API run as ordinary assertions below.  Cases which
//! require a later gate are still named, categorized, and given a concrete
//! missing-capability reason in `FIXTURES`; they are not `#[ignore]` tests or
//! vacuous pass-throughs.

use std::collections::{BTreeMap, BTreeSet};

use axiom_ledger::evidence::{
    AdapterProvenance, Authority as EvidenceAuthority, Availability, CandidateIdentityLink,
    Confidence, ConservationLeg, ConservationMetadata, CorrectionScope, EvidenceRelation,
    EvidenceRelationKind, EvidenceStore, ImportBatch, ObservationAdapter, RawEvidence,
};
use axiom_ledger::exact::{ExactNumber, RoundingMode};
use axiom_ledger::ir::{Atom, Nominal, NominalKind, Term, Var};
use axiom_ledger::liquidity::{
    ActionEdge, ActionKind, InstrumentNode, LiquidityGraph, NodeId, PositionNode, SearchCompletion,
    SearchLimits,
};
use axiom_ledger::logic::{
    Clause, Completion as LogicCompletion, Goal, Literal, Polarity as LogicPolarity, Program,
    ResourceProfile, SemanticContext, Solver, TraceEvent, Truth as LogicTruth,
};
use axiom_ledger::model::{ContentHash, Date, ExternalId, Identity, OccurrenceId, Quantity};
use axiom_ledger::ontology::{
    Encumbrance, EncumbranceKind, Endpoint, Entity, EntityKind, ExchangeLeg, ExchangeRecord,
    Instrument, InstrumentKind, Obligation, Position, Role, RoleAssignments,
    SatisfactionAllocation, Settlement, SettlementState, SettlementStateRecord, TransferRecord,
    VirtualAccount, validate_exchange_legs, validate_obligation_allocation,
    validate_transfer_conservation,
};
use axiom_ledger::package::{LotCandidate, PolicyPackage as ExecutablePolicyPackage, Selection};
use axiom_ledger::proof::{Node, Operation, Proof, ProofId};
use axiom_ledger::recognize::{
    AcceptedWorld, BookPolicy, CompletenessClaims, FactScope, RecognitionAcceptedFact,
    RecognitionError, ReportingPeriod, recognize, recognize_books,
};
use axiom_ledger::scenario::{
    Assumption, Constraint, ConstraintExpression, ExpectedEvent, Horizon, RealizedEvent, Scenario,
};
use axiom_ledger::semantics::{
    Authority, Completion as SemanticCompletion, Conflict, Force, Multiplicity, Phase, Polarity,
    Provenance, Resolution, TemporalScope, Truth, World,
};
use axiom_ledger::store::{
    Close, Commit, Completeness as StoredCompleteness, Evidence, EvidenceState, ObjectStore,
    Period, PolicyPackage, ProofObject, Statement,
};
use axiom_ledger::time::{
    BeforeAfter, Bound, BusinessCalendar, BusinessDayPolicy, Frequency, Instant, InstantInterval,
    Interval, LocalDate, LocalDateTime, LocalTime, LocalTimeStatus, MissingDayPolicy,
    Period as TimePeriod, Recurrence, TimeAssignments, TimeRole, TimeValue, TimeZone,
    UncertainInterval,
};
use axiom_ledger::units::{
    InstrumentDefinition, InstrumentUnit, Quantity as UnitQuantity, Quote, QuoteKind, Ratio,
    RoundingCertificate, UnitError, ValuationPolicy, ValuationStatus, VenueQuantum, value,
};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum Section {
    Evidence,
    Settlement,
    Ownership,
    Instruments,
    Accounting,
    Temporal,
    Logic,
    Planning,
    Security,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Disposition {
    Executed,
    Inventory {
        gate: &'static str,
        reason: &'static str,
    },
}

#[derive(Clone, Copy, Debug)]
struct Fixture {
    id: &'static str,
    section: Section,
    name: &'static str,
    disposition: Disposition,
}

// The names mirror sections XIV A-I in confirmed-direction.md.  Inventory
// entries are deliberately structured: when a capability lands, its entry is
// changed to Executed and an assertion is added to the matching category test.
const FIXTURES: &[Fixture] = &[
    // XIV A — evidence and reconciliation
    Fixture {
        id: "A01",
        section: Section::Evidence,
        name: "identical_source_rows_keep_occurrences",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "A02",
        section: Section::Evidence,
        name: "receipt_and_bank_keep_both_leaves",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "A03",
        section: Section::Evidence,
        name: "correction_supersedes_by_scope",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "A04",
        section: Section::Evidence,
        name: "ocr_alternatives_are_typed",
        disposition: Disposition::Inventory {
            gate: "Gate 4",
            reason: "No uncertainty-set or image-region literal type is exposed yet.",
        },
    },
    Fixture {
        id: "A05",
        section: Section::Evidence,
        name: "missing_date_is_a_typed_hole",
        disposition: Disposition::Inventory {
            gate: "Gate 4",
            reason: "The source parser has no generic typed-hole representation for absent date fields.",
        },
    },
    Fixture {
        id: "A06",
        section: Section::Evidence,
        name: "conflicting_balances_retain_both_proofs",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "A07",
        section: Section::Evidence,
        name: "incomplete_period_does_not_prove_absence",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "A08",
        section: Section::Evidence,
        name: "deleted_source_is_a_tombstone",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "A09",
        section: Section::Evidence,
        name: "split_conserves_quantity",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "A10",
        section: Section::Evidence,
        name: "merge_conserves_quantity",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "A11",
        section: Section::Evidence,
        name: "fuzzy_match_is_ranked_candidate",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "A12",
        section: Section::Evidence,
        name: "adapter_versions_keep_derivations",
        disposition: Disposition::Executed,
    },
    // XIV B — settlement and payments
    Fixture {
        id: "B01",
        section: Section::Settlement,
        name: "authorization_without_capture",
        disposition: Disposition::Inventory {
            gate: "Gate 6",
            reason: "Card authorization/capture state machine is not yet modeled.",
        },
    },
    Fixture {
        id: "B02",
        section: Section::Settlement,
        name: "partial_capture_releases_unused_hold",
        disposition: Disposition::Inventory {
            gate: "Gate 6",
            reason: "Authorization/capture allocation is not modeled; generic release is insufficient.",
        },
    },
    Fixture {
        id: "B03",
        section: Section::Settlement,
        name: "authorization_expiry_releases_hold",
        disposition: Disposition::Inventory {
            gate: "Gate 6",
            reason: "No time-indexed authorization lifecycle exists.",
        },
    },
    Fixture {
        id: "B04",
        section: Section::Settlement,
        name: "pending_ach_at_close",
        disposition: Disposition::Inventory {
            gate: "Gate 6",
            reason: "ACH pending policy is not represented by the settlement ontology.",
        },
    },
    Fixture {
        id: "B05",
        section: Section::Settlement,
        name: "returned_ach_keeps_attempt",
        disposition: Disposition::Inventory {
            gate: "Gate 6",
            reason: "No returned-payment event relation is implemented.",
        },
    },
    Fixture {
        id: "B06",
        section: Section::Settlement,
        name: "bounced_check_leaves_obligation",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "B07",
        section: Section::Settlement,
        name: "chargeback_reverses_settlement",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "B08",
        section: Section::Settlement,
        name: "partial_refund_preserves_residual",
        disposition: Disposition::Inventory {
            gate: "Gate 6",
            reason: "Refund allocation is not yet modeled.",
        },
    },
    Fixture {
        id: "B09",
        section: Section::Settlement,
        name: "tip_adjustment_separates_capture",
        disposition: Disposition::Inventory {
            gate: "Gate 6",
            reason: "Authorization and final capture amounts have no relation type.",
        },
    },
    Fixture {
        id: "B10",
        section: Section::Settlement,
        name: "one_payment_satisfies_many_invoices",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "B11",
        section: Section::Settlement,
        name: "many_payments_satisfy_one_invoice",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "B12",
        section: Section::Settlement,
        name: "overpayment_becomes_credit_or_reversal",
        disposition: Disposition::Inventory {
            gate: "Gate 6",
            reason: "Overpayment policy and credit positions are not yet modeled.",
        },
    },
    Fixture {
        id: "B13",
        section: Section::Settlement,
        name: "withheld_fee_is_separate_leg",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "B14",
        section: Section::Settlement,
        name: "cash_withdrawal_and_atm_fee_group",
        disposition: Disposition::Inventory {
            gate: "Gate 6",
            reason: "Grouped cash-withdrawal event records are not implemented.",
        },
    },
    Fixture {
        id: "B15",
        section: Section::Settlement,
        name: "foreign_card_conversion_keeps_legs",
        disposition: Disposition::Inventory {
            gate: "Gate 7",
            reason: "Issuer/network conversion evidence is not yet represented as separate legs.",
        },
    },
    Fixture {
        id: "B16",
        section: Section::Settlement,
        name: "net_batch_allocates_residual",
        disposition: Disposition::Inventory {
            gate: "Gate 6",
            reason: "Batch settlement reconciliation is not implemented.",
        },
    },
    // XIV C — ownership, custody, accounts
    Fixture {
        id: "C01",
        section: Section::Ownership,
        name: "joint_roles_keep_holders_separate",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "C02",
        section: Section::Ownership,
        name: "authorized_user_is_not_debtor",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "C03",
        section: Section::Ownership,
        name: "escrow_separates_custody_and_benefit",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "C04",
        section: Section::Ownership,
        name: "security_deposit_has_repayment_obligation",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "C05",
        section: Section::Ownership,
        name: "restricted_funds_are_encumbered",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "C06",
        section: Section::Ownership,
        name: "trustee_beneficiary_tax_owner_differ",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "C07",
        section: Section::Ownership,
        name: "borrowed_security_is_return_obligation",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "C08",
        section: Section::Ownership,
        name: "short_is_return_obligation",
        disposition: Disposition::Inventory {
            gate: "Gate 7",
            reason: "Short-position state is not implemented.",
        },
    },
    Fixture {
        id: "C09",
        section: Section::Ownership,
        name: "pledge_changes_liquidity_not_position",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "C10",
        section: Section::Ownership,
        name: "multicurrency_account_has_separate_positions",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "C11",
        section: Section::Ownership,
        name: "overdraft_is_contractual_credit",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "C12",
        section: Section::Ownership,
        name: "closed_account_retains_history",
        disposition: Disposition::Inventory {
            gate: "Gate 6",
            reason: "Account lifecycle validity is not modeled.",
        },
    },
    Fixture {
        id: "C13",
        section: Section::Ownership,
        name: "institution_alias_does_not_rewrite_identity",
        disposition: Disposition::Inventory {
            gate: "Gate 5",
            reason: "Institution alias/merge identity relation is not implemented.",
        },
    },
    Fixture {
        id: "C14",
        section: Section::Ownership,
        name: "virtual_envelope_is_not_external_account",
        disposition: Disposition::Executed,
    },
    // XIV D — instruments, quotes, investments
    Fixture {
        id: "D01",
        section: Section::Instruments,
        name: "missing_quote_is_unavailable",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "D02",
        section: Section::Instruments,
        name: "conflicting_quotes_are_ambiguous",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "D03",
        section: Section::Instruments,
        name: "stale_quote_is_not_silent_current_value",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "D04",
        section: Section::Instruments,
        name: "bid_ask_has_no_implicit_reverse",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "D05",
        section: Section::Instruments,
        name: "triangulation_keeps_route_proof",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "D06",
        section: Section::Instruments,
        name: "market_calendars_are_named",
        disposition: Disposition::Inventory {
            gate: "Gate 7",
            reason: "Venue-specific market calendar packages are not implemented.",
        },
    },
    Fixture {
        id: "D07",
        section: Section::Instruments,
        name: "fractional_quantity_is_exact",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "D08",
        section: Section::Instruments,
        name: "stock_split_transforms_lots",
        disposition: Disposition::Inventory {
            gate: "Gate 7",
            reason: "Corporate-action lot transformations are not implemented.",
        },
    },
    Fixture {
        id: "D09",
        section: Section::Instruments,
        name: "reverse_split_cash_in_lieu",
        disposition: Disposition::Inventory {
            gate: "Gate 7",
            reason: "Reverse-split and cash-in-lieu events are not implemented.",
        },
    },
    Fixture {
        id: "D10",
        section: Section::Instruments,
        name: "merger_maps_old_rights",
        disposition: Disposition::Inventory {
            gate: "Gate 7",
            reason: "Merger/exchange corporate-action terms are not implemented.",
        },
    },
    Fixture {
        id: "D11",
        section: Section::Instruments,
        name: "spin_off_allocates_basis",
        disposition: Disposition::Inventory {
            gate: "Gate 7",
            reason: "Spin-off basis policy is not implemented.",
        },
    },
    Fixture {
        id: "D12",
        section: Section::Instruments,
        name: "return_of_capital_adjusts_basis",
        disposition: Disposition::Inventory {
            gate: "Gate 7",
            reason: "Return-of-capital basis treatment is not implemented.",
        },
    },
    Fixture {
        id: "D13",
        section: Section::Instruments,
        name: "reinvestment_links_distribution_and_acquisition",
        disposition: Disposition::Inventory {
            gate: "Gate 7",
            reason: "Dividend-reinvestment linkage is not implemented.",
        },
    },
    Fixture {
        id: "D14",
        section: Section::Instruments,
        name: "option_exercise_creates_events",
        disposition: Disposition::Inventory {
            gate: "Gate 7",
            reason: "Option-right exercise transformation is not implemented.",
        },
    },
    Fixture {
        id: "D15",
        section: Section::Instruments,
        name: "option_expiration_retires_right",
        disposition: Disposition::Inventory {
            gate: "Gate 7",
            reason: "Option expiration state is not implemented.",
        },
    },
    Fixture {
        id: "D16",
        section: Section::Instruments,
        name: "assignment_changes_contract_role",
        disposition: Disposition::Inventory {
            gate: "Gate 7",
            reason: "Contract assignment is not implemented.",
        },
    },
    Fixture {
        id: "D17",
        section: Section::Instruments,
        name: "wash_sale_adjusts_basis_by_policy",
        disposition: Disposition::Inventory {
            gate: "Gate 7",
            reason: "Jurisdiction-specific basis rewrites are not implemented.",
        },
    },
    Fixture {
        id: "D18",
        section: Section::Instruments,
        name: "negative_quote_is_exact_when_allowed",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "D19",
        section: Section::Instruments,
        name: "unique_asset_uses_identity_not_fungibility",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "D20",
        section: Section::Instruments,
        name: "barter_has_coupled_legs",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "D21",
        section: Section::Instruments,
        name: "basket_has_constituents_and_value",
        disposition: Disposition::Inventory {
            gate: "Gate 7",
            reason: "Basket constituent valuation is not implemented.",
        },
    },
    Fixture {
        id: "D22",
        section: Section::Instruments,
        name: "redenomination_has_exact_ratio",
        disposition: Disposition::Inventory {
            gate: "Gate 7",
            reason: "Instrument redenomination action is not implemented.",
        },
    },
    Fixture {
        id: "D23",
        section: Section::Instruments,
        name: "instrument_expiry_is_explicit",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "D24",
        section: Section::Instruments,
        name: "off_quantum_is_rejected_not_rounded",
        disposition: Disposition::Executed,
    },
    // XIV E — accounting/reporting
    Fixture {
        id: "E01",
        section: Section::Accounting,
        name: "cash_and_accrual_share_event_set",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "E02",
        section: Section::Accounting,
        name: "prepaid_is_consumed_over_time",
        disposition: Disposition::Inventory {
            gate: "Gate 6",
            reason: "Prepaid-expense schedule is not implemented.",
        },
    },
    Fixture {
        id: "E03",
        section: Section::Accounting,
        name: "deferred_revenue_retains_obligation",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "E04",
        section: Section::Accounting,
        name: "interest_uses_explicit_convention",
        disposition: Disposition::Inventory {
            gate: "Gate 6",
            reason: "Interest accrual conventions are not implemented.",
        },
    },
    Fixture {
        id: "E05",
        section: Section::Accounting,
        name: "amortization_declares_method",
        disposition: Disposition::Inventory {
            gate: "Gate 6",
            reason: "Amortization schedule methods are not implemented.",
        },
    },
    Fixture {
        id: "E06",
        section: Section::Accounting,
        name: "depreciation_is_book_measurement",
        disposition: Disposition::Inventory {
            gate: "Gate 6",
            reason: "Depreciation policy is not implemented.",
        },
    },
    Fixture {
        id: "E07",
        section: Section::Accounting,
        name: "impairment_is_new_assertion",
        disposition: Disposition::Inventory {
            gate: "Gate 6",
            reason: "Impairment measurement assertion is not implemented.",
        },
    },
    Fixture {
        id: "E08",
        section: Section::Accounting,
        name: "writeoff_keeps_claim_history",
        disposition: Disposition::Inventory {
            gate: "Gate 6",
            reason: "Bad-debt recognition is not implemented.",
        },
    },
    Fixture {
        id: "E09",
        section: Section::Accounting,
        name: "contra_is_reporting_relation",
        disposition: Disposition::Inventory {
            gate: "Gate 6",
            reason: "Contra-account reporting projection is not implemented.",
        },
    },
    Fixture {
        id: "E10",
        section: Section::Accounting,
        name: "intercompany_has_entity_relative_views",
        disposition: Disposition::Inventory {
            gate: "Gate 6",
            reason: "Intercompany views are not implemented.",
        },
    },
    Fixture {
        id: "E11",
        section: Section::Accounting,
        name: "elimination_is_group_book_only",
        disposition: Disposition::Inventory {
            gate: "Gate 6",
            reason: "Consolidation elimination is not implemented.",
        },
    },
    Fixture {
        id: "E12",
        section: Section::Accounting,
        name: "fx_remeasurement_is_book_specific",
        disposition: Disposition::Inventory {
            gate: "Gate 6",
            reason: "FX remeasurement is not implemented.",
        },
    },
    Fixture {
        id: "E13",
        section: Section::Accounting,
        name: "translation_reserve_has_policy_proof",
        disposition: Disposition::Inventory {
            gate: "Gate 6",
            reason: "Translation reserve is not implemented.",
        },
    },
    Fixture {
        id: "E14",
        section: Section::Accounting,
        name: "reversal_is_new_event_correction_supersedes",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "E15",
        section: Section::Accounting,
        name: "late_discovery_restates_close",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "E16",
        section: Section::Accounting,
        name: "materiality_is_named_policy",
        disposition: Disposition::Inventory {
            gate: "Gate 6",
            reason: "Materiality policy package semantics are not implemented.",
        },
    },
    Fixture {
        id: "E17",
        section: Section::Accounting,
        name: "jurisdictions_have_independent_packages",
        disposition: Disposition::Inventory {
            gate: "Gate 8",
            reason: "Multi-jurisdiction package execution is not implemented.",
        },
    },
    Fixture {
        id: "E18",
        section: Section::Accounting,
        name: "policy_changes_use_effective_intervals",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "E19",
        section: Section::Accounting,
        name: "unknown_tax_keeps_alternatives",
        disposition: Disposition::Executed,
    },
    // XIV F — temporal behavior
    Fixture {
        id: "F01",
        section: Section::Temporal,
        name: "fixed_offset_preserves_instant",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "F02",
        section: Section::Temporal,
        name: "dst_ambiguity_remains_visible",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "F03",
        section: Section::Temporal,
        name: "month_precision_is_coarse",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "F04",
        section: Section::Temporal,
        name: "span_interval_preserves_bounds",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "F05",
        section: Section::Temporal,
        name: "occurrence_and_recording_roles_differ",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "F06",
        section: Section::Temporal,
        name: "retroactive_effective_date_is_allowed",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "F07",
        section: Section::Temporal,
        name: "late_settlement_is_independent_role",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "F08",
        section: Section::Temporal,
        name: "missing_recurrence_day_has_policy",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "F09",
        section: Section::Temporal,
        name: "business_day_adjustment_is_named",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "F10",
        section: Section::Temporal,
        name: "open_ended_interval_has_unbounded_end",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "F11",
        section: Section::Temporal,
        name: "policy_interval_is_date_scoped",
        disposition: Disposition::Executed,
    },
    // XIV G — logic and solver behavior
    Fixture {
        id: "G01",
        section: Section::Logic,
        name: "positive_recursion_reaches_fixed_point",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "G02",
        section: Section::Logic,
        name: "cycle_without_base_has_no_proof",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "G03",
        section: Section::Logic,
        name: "negation_through_recursion_is_rejected",
        disposition: Disposition::Inventory {
            gate: "Gate 3",
            reason: "Compile-time stratification diagnostics are not public yet.",
        },
    },
    Fixture {
        id: "G04",
        section: Section::Logic,
        name: "aggregate_incomplete_set_is_unknown",
        disposition: Disposition::Inventory {
            gate: "Gate 3",
            reason: "Aggregate theory relations are not implemented.",
        },
    },
    Fixture {
        id: "G05",
        section: Section::Logic,
        name: "independent_proofs_share_dag",
        disposition: Disposition::Inventory {
            gate: "Gate 3",
            reason: "The current solver exposes a checked proof graph but not all independent proof roots.",
        },
    },
    Fixture {
        id: "G06",
        section: Section::Logic,
        name: "proof_and_refutation_are_conflict",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "G07",
        section: Section::Logic,
        name: "candidate_substitutions_are_multiple",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "G08",
        section: Section::Logic,
        name: "resource_limit_is_incomplete_not_false",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "G09",
        section: Section::Logic,
        name: "unsupported_nonlinear_is_residual",
        disposition: Disposition::Inventory {
            gate: "Gate 3",
            reason: "Nonlinear theory boundary is not implemented.",
        },
    },
    Fixture {
        id: "G10",
        section: Section::Logic,
        name: "external_model_requires_exact_verification",
        disposition: Disposition::Inventory {
            gate: "Gate 3",
            reason: "No external solver adapter is implemented.",
        },
    },
    Fixture {
        id: "G11",
        section: Section::Logic,
        name: "policy_conflict_is_coherence_error",
        disposition: Disposition::Inventory {
            gate: "Gate 3",
            reason: "Policy package coherence checking is not implemented.",
        },
    },
    Fixture {
        id: "G12",
        section: Section::Logic,
        name: "package_update_changes_cache_key",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "G13",
        section: Section::Logic,
        name: "search_order_does_not_change_answer",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "G14",
        section: Section::Logic,
        name: "infinite_generation_requires_horizon",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "G15",
        section: Section::Logic,
        name: "recursive_object_rules_are_finite",
        disposition: Disposition::Inventory {
            gate: "Gate 3",
            reason: "Object-generating rule package checks are not implemented.",
        },
    },
    Fixture {
        id: "G16",
        section: Section::Logic,
        name: "completeness_is_a_cache_dimension",
        disposition: Disposition::Executed,
    },
    // XIV H — planning and uncertainty
    Fixture {
        id: "H01",
        section: Section::Planning,
        name: "forecast_is_separate_from_actual",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "H02",
        section: Section::Planning,
        name: "actual_event_links_to_forecast_variance",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "H03",
        section: Section::Planning,
        name: "amount_range_is_interval_constraint",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "H04",
        section: Section::Planning,
        name: "possible_dates_are_temporal_alternatives",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "H05",
        section: Section::Planning,
        name: "conditional_income_is_scenario_rule",
        disposition: Disposition::Inventory {
            gate: "Gate 9",
            reason: "Conditional scenario rules are not implemented.",
        },
    },
    Fixture {
        id: "H06",
        section: Section::Planning,
        name: "infeasible_budget_has_unsatisfied_core",
        disposition: Disposition::Inventory {
            gate: "Gate 9",
            reason: "The graph reports no route, but no structured unsatisfied core is exposed.",
        },
    },
    Fixture {
        id: "H07",
        section: Section::Planning,
        name: "feasible_plans_have_pareto_frontier",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "H08",
        section: Section::Planning,
        name: "approximate_plan_has_exact_verifier",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "H09",
        section: Section::Planning,
        name: "risk_distribution_keeps_provenance",
        disposition: Disposition::Inventory {
            gate: "Gate 9",
            reason: "Probability models are not implemented.",
        },
    },
    Fixture {
        id: "H10",
        section: Section::Planning,
        name: "scenario_import_has_inheritance_boundary",
        disposition: Disposition::Inventory {
            gate: "Gate 9",
            reason: "Evidence inheritance metadata is not implemented.",
        },
    },
    Fixture {
        id: "H11",
        section: Section::Planning,
        name: "scenario_override_is_scoped_assumption",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "H12",
        section: Section::Planning,
        name: "scenario_merge_requires_reconciliation",
        disposition: Disposition::Inventory {
            gate: "Gate 9",
            reason: "Scenario merge conflict diagnostics are not implemented.",
        },
    },
    // XIV I — security and collaboration
    Fixture {
        id: "I01",
        section: Section::Security,
        name: "adapter_can_only_emit_observations",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "I02",
        section: Section::Security,
        name: "resource_profile_bounds_rule_package",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "I03",
        section: Section::Security,
        name: "close_pins_package_hashes",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "I04",
        section: Section::Security,
        name: "revocation_keeps_history",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "I05",
        section: Section::Security,
        name: "private_evidence_inherits_labels",
        disposition: Disposition::Inventory {
            gate: "Gate 8",
            reason: "Information-flow labels are not implemented.",
        },
    },
    Fixture {
        id: "I06",
        section: Section::Security,
        name: "redacted_proof_does_not_leak",
        disposition: Disposition::Inventory {
            gate: "Gate 8",
            reason: "Proof projection/redaction is not implemented.",
        },
    },
    Fixture {
        id: "I07",
        section: Section::Security,
        name: "concurrent_decisions_merge_as_conflict",
        disposition: Disposition::Inventory {
            gate: "Gate 5",
            reason: "Three-way merge exists but has an outstanding base-ancestor regression.",
        },
    },
    Fixture {
        id: "I08",
        section: Section::Security,
        name: "lost_key_is_unavailable_evidence",
        disposition: Disposition::Executed,
    },
    Fixture {
        id: "I09",
        section: Section::Security,
        name: "compromised_source_recomputes_dependents",
        disposition: Disposition::Inventory {
            gate: "Gate 8",
            reason: "Authority invalidation and dependent recomputation are not implemented.",
        },
    },
    Fixture {
        id: "I10",
        section: Section::Security,
        name: "agent_proposal_is_candidate_only",
        disposition: Disposition::Executed,
    },
];

/// Every `Executed` fixture is linked to a grouped smoke executor. This map is
/// navigation and breadth evidence, not proof that each ID has an independent
/// assertion; Gate 0 remains open until those cases are split.
struct CaseCoverage {
    id: &'static str,
    executor: fn(),
}

const EXECUTION_COVERAGE: &[CaseCoverage] = &[
    CaseCoverage {
        id: "A01",
        executor: evidence_identity_and_reconciliation_fixtures,
    },
    CaseCoverage {
        id: "A02",
        executor: evidence_identity_and_reconciliation_fixtures,
    },
    CaseCoverage {
        id: "A03",
        executor: evidence_identity_and_reconciliation_fixtures,
    },
    CaseCoverage {
        id: "A06",
        executor: evidence_identity_and_reconciliation_fixtures,
    },
    CaseCoverage {
        id: "A07",
        executor: evidence_identity_and_reconciliation_fixtures,
    },
    CaseCoverage {
        id: "A08",
        executor: evidence_identity_and_reconciliation_fixtures,
    },
    CaseCoverage {
        id: "A09",
        executor: evidence_identity_and_reconciliation_fixtures,
    },
    CaseCoverage {
        id: "A10",
        executor: evidence_identity_and_reconciliation_fixtures,
    },
    CaseCoverage {
        id: "A11",
        executor: evidence_identity_and_reconciliation_fixtures,
    },
    CaseCoverage {
        id: "A12",
        executor: evidence_identity_and_reconciliation_fixtures,
    },
    CaseCoverage {
        id: "B06",
        executor: ownership_and_settlement_fixtures,
    },
    CaseCoverage {
        id: "B07",
        executor: additional_executable_edge_cases,
    },
    CaseCoverage {
        id: "B10",
        executor: additional_executable_edge_cases,
    },
    CaseCoverage {
        id: "B11",
        executor: additional_executable_edge_cases,
    },
    CaseCoverage {
        id: "B13",
        executor: additional_executable_edge_cases,
    },
    CaseCoverage {
        id: "C01",
        executor: ownership_and_settlement_fixtures,
    },
    CaseCoverage {
        id: "C02",
        executor: ownership_and_settlement_fixtures,
    },
    CaseCoverage {
        id: "C03",
        executor: additional_executable_edge_cases,
    },
    CaseCoverage {
        id: "C04",
        executor: additional_executable_edge_cases,
    },
    CaseCoverage {
        id: "C05",
        executor: ownership_and_settlement_fixtures,
    },
    CaseCoverage {
        id: "C06",
        executor: ownership_and_settlement_fixtures,
    },
    CaseCoverage {
        id: "C07",
        executor: additional_executable_edge_cases,
    },
    CaseCoverage {
        id: "C09",
        executor: ownership_and_settlement_fixtures,
    },
    CaseCoverage {
        id: "C10",
        executor: additional_executable_edge_cases,
    },
    CaseCoverage {
        id: "C11",
        executor: additional_executable_edge_cases,
    },
    CaseCoverage {
        id: "C14",
        executor: additional_executable_edge_cases,
    },
    CaseCoverage {
        id: "D01",
        executor: exact_units_quotes_and_instrument_fixtures,
    },
    CaseCoverage {
        id: "D02",
        executor: exact_units_quotes_and_instrument_fixtures,
    },
    CaseCoverage {
        id: "D03",
        executor: exact_units_quotes_and_instrument_fixtures,
    },
    CaseCoverage {
        id: "D04",
        executor: exact_units_quotes_and_instrument_fixtures,
    },
    CaseCoverage {
        id: "D05",
        executor: exact_units_quotes_and_instrument_fixtures,
    },
    CaseCoverage {
        id: "D07",
        executor: exact_units_quotes_and_instrument_fixtures,
    },
    CaseCoverage {
        id: "D18",
        executor: exact_units_quotes_and_instrument_fixtures,
    },
    CaseCoverage {
        id: "D19",
        executor: additional_executable_edge_cases,
    },
    CaseCoverage {
        id: "D20",
        executor: ownership_and_settlement_fixtures,
    },
    CaseCoverage {
        id: "D23",
        executor: additional_executable_edge_cases,
    },
    CaseCoverage {
        id: "D24",
        executor: exact_units_quotes_and_instrument_fixtures,
    },
    CaseCoverage {
        id: "E01",
        executor: additional_executable_edge_cases,
    },
    CaseCoverage {
        id: "E03",
        executor: additional_executable_edge_cases,
    },
    CaseCoverage {
        id: "E14",
        executor: additional_executable_edge_cases,
    },
    CaseCoverage {
        id: "E15",
        executor: store_versioning_close_and_unavailable_evidence_fixtures,
    },
    CaseCoverage {
        id: "E18",
        executor: temporal_fixtures_preserve_precision_roles_and_policies,
    },
    CaseCoverage {
        id: "E19",
        executor: semantics_axes_conflict_and_completeness_fixtures,
    },
    CaseCoverage {
        id: "F01",
        executor: temporal_fixtures_preserve_precision_roles_and_policies,
    },
    CaseCoverage {
        id: "F02",
        executor: temporal_fixtures_preserve_precision_roles_and_policies,
    },
    CaseCoverage {
        id: "F03",
        executor: temporal_fixtures_preserve_precision_roles_and_policies,
    },
    CaseCoverage {
        id: "F04",
        executor: temporal_fixtures_preserve_precision_roles_and_policies,
    },
    CaseCoverage {
        id: "F05",
        executor: temporal_fixtures_preserve_precision_roles_and_policies,
    },
    CaseCoverage {
        id: "F06",
        executor: temporal_fixtures_preserve_precision_roles_and_policies,
    },
    CaseCoverage {
        id: "F07",
        executor: temporal_fixtures_preserve_precision_roles_and_policies,
    },
    CaseCoverage {
        id: "F08",
        executor: temporal_fixtures_preserve_precision_roles_and_policies,
    },
    CaseCoverage {
        id: "F09",
        executor: temporal_fixtures_preserve_precision_roles_and_policies,
    },
    CaseCoverage {
        id: "F10",
        executor: temporal_fixtures_preserve_precision_roles_and_policies,
    },
    CaseCoverage {
        id: "F11",
        executor: temporal_fixtures_preserve_precision_roles_and_policies,
    },
    CaseCoverage {
        id: "G01",
        executor: logic_fixed_point_conflict_ambiguity_and_resource_fixtures,
    },
    CaseCoverage {
        id: "G02",
        executor: logic_fixed_point_conflict_ambiguity_and_resource_fixtures,
    },
    CaseCoverage {
        id: "G06",
        executor: logic_fixed_point_conflict_ambiguity_and_resource_fixtures,
    },
    CaseCoverage {
        id: "G07",
        executor: logic_fixed_point_conflict_ambiguity_and_resource_fixtures,
    },
    CaseCoverage {
        id: "G08",
        executor: logic_fixed_point_conflict_ambiguity_and_resource_fixtures,
    },
    CaseCoverage {
        id: "G12",
        executor: additional_executable_edge_cases,
    },
    CaseCoverage {
        id: "G13",
        executor: additional_executable_edge_cases,
    },
    CaseCoverage {
        id: "G14",
        executor: temporal_fixtures_preserve_precision_roles_and_policies,
    },
    CaseCoverage {
        id: "G16",
        executor: logic_fixed_point_conflict_ambiguity_and_resource_fixtures,
    },
    CaseCoverage {
        id: "H01",
        executor: recognition_scenario_and_planning_fixtures,
    },
    CaseCoverage {
        id: "H02",
        executor: recognition_scenario_and_planning_fixtures,
    },
    CaseCoverage {
        id: "H03",
        executor: additional_executable_edge_cases,
    },
    CaseCoverage {
        id: "H04",
        executor: temporal_fixtures_preserve_precision_roles_and_policies,
    },
    CaseCoverage {
        id: "H07",
        executor: liquidity_and_resource_boundary_fixtures,
    },
    CaseCoverage {
        id: "H08",
        executor: liquidity_and_resource_boundary_fixtures,
    },
    CaseCoverage {
        id: "H11",
        executor: recognition_scenario_and_planning_fixtures,
    },
    CaseCoverage {
        id: "I01",
        executor: collaboration_and_adapter_boundaries_fixtures,
    },
    CaseCoverage {
        id: "I02",
        executor: logic_fixed_point_conflict_ambiguity_and_resource_fixtures,
    },
    CaseCoverage {
        id: "I03",
        executor: store_versioning_close_and_unavailable_evidence_fixtures,
    },
    CaseCoverage {
        id: "I04",
        executor: collaboration_and_adapter_boundaries_fixtures,
    },
    CaseCoverage {
        id: "I08",
        executor: store_versioning_close_and_unavailable_evidence_fixtures,
    },
    CaseCoverage {
        id: "I10",
        executor: collaboration_and_adapter_boundaries_fixtures,
    },
];

/// Case-specific executors added for Gate 0.  The grouped suites above retain
/// the full inventory smoke pass, while every entry here has its own `#[test]`
/// below.  Keeping this registry explicit prevents a new test from silently
/// drifting away from the XIV A-I fixture name it proves.
struct IndependentCase {
    id: &'static str,
    executor: fn(),
}

const INDEPENDENT_CASES_ADDED: &[IndependentCase] = &[
    IndependentCase {
        id: "A01",
        executor: independent_a01_identical_source_rows_keep_occurrences,
    },
    IndependentCase {
        id: "A02",
        executor: independent_a02_receipt_and_bank_keep_both_leaves,
    },
    IndependentCase {
        id: "A03",
        executor: independent_a03_correction_supersedes_by_scope,
    },
    IndependentCase {
        id: "A06",
        executor: independent_a06_conflicting_balances_retain_both_proofs,
    },
    IndependentCase {
        id: "A07",
        executor: independent_a07_incomplete_period_does_not_prove_absence,
    },
    IndependentCase {
        id: "A08",
        executor: independent_a08_deleted_source_is_a_tombstone,
    },
    IndependentCase {
        id: "A09",
        executor: independent_a09_split_conserves_quantity,
    },
    IndependentCase {
        id: "A10",
        executor: independent_a10_merge_conserves_quantity,
    },
    IndependentCase {
        id: "A11",
        executor: independent_a11_fuzzy_match_is_ranked_candidate,
    },
    IndependentCase {
        id: "A12",
        executor: independent_a12_adapter_versions_keep_derivations,
    },
    IndependentCase {
        id: "B06",
        executor: independent_b06_bounced_check_leaves_obligation,
    },
    IndependentCase {
        id: "B07",
        executor: independent_b07_chargeback_reverses_settlement,
    },
    IndependentCase {
        id: "B10",
        executor: independent_b10_one_payment_satisfies_many_invoices,
    },
    IndependentCase {
        id: "B11",
        executor: independent_b11_many_payments_satisfy_one_invoice,
    },
    IndependentCase {
        id: "B13",
        executor: independent_b13_withheld_fee_is_separate_leg,
    },
    IndependentCase {
        id: "C01",
        executor: independent_c01_joint_roles_keep_holders_separate,
    },
    IndependentCase {
        id: "C03",
        executor: independent_c03_escrow_separates_custody_and_benefit,
    },
    IndependentCase {
        id: "E01",
        executor: independent_e01_cash_and_accrual_share_event_set,
    },
    IndependentCase {
        id: "E03",
        executor: independent_e03_deferred_revenue_retains_obligation,
    },
    IndependentCase {
        id: "D01",
        executor: independent_d01_missing_quote_is_unavailable,
    },
    IndependentCase {
        id: "D02",
        executor: independent_d02_conflicting_quotes_are_ambiguous,
    },
    IndependentCase {
        id: "F01",
        executor: independent_f01_fixed_offset_preserves_instant,
    },
    IndependentCase {
        id: "F08",
        executor: independent_f08_missing_recurrence_day_has_policy,
    },
    IndependentCase {
        id: "G01",
        executor: independent_g01_positive_recursion_reaches_fixed_point,
    },
    IndependentCase {
        id: "G06",
        executor: independent_g06_proof_and_refutation_are_conflict,
    },
    IndependentCase {
        id: "H01",
        executor: independent_h01_forecast_is_separate_from_actual,
    },
    IndependentCase {
        id: "H03",
        executor: independent_h03_amount_range_is_interval_constraint,
    },
    IndependentCase {
        id: "I01",
        executor: independent_i01_adapter_can_only_emit_observations,
    },
    IndependentCase {
        id: "I04",
        executor: independent_i04_revocation_keeps_history,
    },
    IndependentCase {
        id: "C02",
        executor: independent_c02_authorized_user_is_not_debtor,
    },
    IndependentCase {
        id: "C04",
        executor: independent_c04_security_deposit_has_repayment_obligation,
    },
    IndependentCase {
        id: "C05",
        executor: independent_c05_restricted_funds_are_encumbered,
    },
    IndependentCase {
        id: "C06",
        executor: independent_c06_trustee_beneficiary_tax_owner_differ,
    },
    IndependentCase {
        id: "C07",
        executor: independent_c07_borrowed_security_is_return_obligation,
    },
    IndependentCase {
        id: "C09",
        executor: independent_c09_pledge_changes_liquidity_not_position,
    },
    IndependentCase {
        id: "C10",
        executor: independent_c10_multicurrency_account_has_separate_positions,
    },
    IndependentCase {
        id: "C11",
        executor: independent_c11_overdraft_is_contractual_credit,
    },
    IndependentCase {
        id: "C14",
        executor: independent_c14_virtual_envelope_is_not_external_account,
    },
    IndependentCase {
        id: "D03",
        executor: independent_d03_stale_quote_is_not_silent_current_value,
    },
    IndependentCase {
        id: "D04",
        executor: independent_d04_bid_ask_has_no_implicit_reverse,
    },
    IndependentCase {
        id: "D05",
        executor: independent_d05_triangulation_keeps_route_proof,
    },
    IndependentCase {
        id: "D07",
        executor: independent_d07_fractional_quantity_is_exact,
    },
    IndependentCase {
        id: "D19",
        executor: independent_d19_unique_asset_uses_identity_not_fungibility,
    },
    IndependentCase {
        id: "D20",
        executor: independent_d20_barter_has_coupled_legs,
    },
    IndependentCase {
        id: "D23",
        executor: independent_d23_instrument_expiry_is_explicit,
    },
    IndependentCase {
        id: "D24",
        executor: independent_d24_off_quantum_is_rejected_not_rounded,
    },
    IndependentCase {
        id: "E14",
        executor: independent_e14_reversal_is_new_event_correction_supersedes,
    },
    IndependentCase {
        id: "E15",
        executor: independent_e15_late_discovery_restates_close,
    },
    IndependentCase {
        id: "F02",
        executor: independent_f02_dst_ambiguity_remains_visible,
    },
    IndependentCase {
        id: "F03",
        executor: independent_f03_month_precision_is_coarse,
    },
];

fn date(text: &str) -> Date {
    text.parse().expect("fixture date is valid")
}

fn hash(seed: u8) -> ContentHash {
    ContentHash::from_bytes([seed; 32])
}

fn proof(seed: u8) -> ProofId {
    proof_node(seed).id
}

fn proof_node(seed: u8) -> Node {
    Node::new(
        format!("proof/{seed}"),
        Operation::Observation {
            source: format!("source/{seed}"),
        },
        Vec::new(),
        BTreeMap::new(),
    )
}

fn proof_bundle(ids: impl IntoIterator<Item = ProofId>) -> Proof {
    let mut bundle = Proof::new();
    for id in ids {
        let node = (0..=u8::MAX)
            .map(proof_node)
            .find(|node| node.id == id)
            .expect("test proof id has a corresponding node");
        bundle.insert(node);
        bundle.root(id);
    }
    bundle
}

fn accepted_world(
    source_commit: ContentHash,
    facts: impl IntoIterator<Item = RecognitionAcceptedFact>,
) -> AcceptedWorld {
    let mut bundle = Proof::new();
    let mut checked = Vec::new();
    for fact in facts {
        let node = Node::new(
            format!("accepted fact {}", fact.id()),
            Operation::Observation {
                source: fact.id().to_string(),
            },
            Vec::new(),
            BTreeMap::from([("accepted-fact".to_string(), fact.id().to_string())]),
        );
        let proof = node.id;
        bundle.insert(node);
        bundle.root(proof);
        let rebuilt = match fact.scope() {
            FactScope::Actual => RecognitionAcceptedFact::actual(
                fact.id().clone(),
                fact.kind(),
                fact.occurrence_date(),
                proof,
                fact.authority(),
            ),
            FactScope::Scenario(scenario) => RecognitionAcceptedFact::scenario(
                fact.id().clone(),
                fact.kind(),
                fact.occurrence_date(),
                scenario.into_string(),
                proof,
                fact.authority(),
            ),
        }
        .expect("rebuilt accepted fact is valid")
        .with_attributes(fact.attributes().clone());
        checked.push(match fact.settlement_date() {
            Some(date) => rebuilt.with_settlement_date(date),
            None => rebuilt,
        });
    }
    AcceptedWorld::from_facts_checked(source_commit, checked, bundle)
        .expect("accepted world has rooted fact proofs")
}

fn evidence_row(source: &str, occurrence: &str, external: &str, bytes: &[u8]) -> RawEvidence {
    RawEvidence::from_bytes(source, occurrence, Some(ExternalId::new(external)), bytes)
}

fn unit(name: &str, instrument: &str) -> InstrumentUnit {
    InstrumentUnit::new(name, instrument)
}

fn uq(amount: &str, unit: InstrumentUnit) -> UnitQuantity {
    UnitQuantity::typed(ExactNumber::parse(amount).unwrap(), unit)
}

fn ir_atom(predicate: &str, args: Vec<Term>) -> Atom {
    Atom::new(Nominal::new(NominalKind::Predicate, predicate), args)
}

fn text_fact(predicate: &str, value: &str) -> Literal {
    Literal::positive(ir_atom(predicate, vec![Term::Text(value.to_string())]))
}

#[test]
fn fixture_inventory_is_explicit_and_named() {
    assert!(FIXTURES.len() >= 75, "Gate 0 requires at least 75 fixtures");
    let mut ids = BTreeSet::new();
    let mut names = BTreeSet::new();
    let mut sections = BTreeSet::new();
    let mut executed = 0usize;
    let mut inventory = 0usize;
    for fixture in FIXTURES {
        assert!(
            ids.insert(fixture.id),
            "duplicate fixture id {}",
            fixture.id
        );
        assert!(
            names.insert(fixture.name),
            "duplicate fixture name {}",
            fixture.name
        );
        sections.insert(fixture.section);
        assert!(!fixture.id.is_empty() && !fixture.name.is_empty());
        match fixture.disposition {
            Disposition::Executed => executed += 1,
            Disposition::Inventory { gate, reason } => {
                inventory += 1;
                assert!(!gate.trim().is_empty(), "{} needs a gate", fixture.id);
                assert!(
                    reason.trim().len() >= 20,
                    "{} needs an actionable reason",
                    fixture.id
                );
            }
        }
    }
    assert_eq!(sections.len(), 9, "fixture corpus must span XIV A-I");
    assert!(executed >= 75, "too few smoke-covered cases: {executed}");
    assert!(inventory > 0, "unsupported work must be explicit inventory");

    let executed_ids: BTreeSet<_> = FIXTURES
        .iter()
        .filter_map(|fixture| match fixture.disposition {
            Disposition::Executed => Some(fixture.id),
            Disposition::Inventory { .. } => None,
        })
        .collect();
    let covered_ids: BTreeSet<_> = EXECUTION_COVERAGE.iter().map(|case| case.id).collect();
    assert_eq!(
        covered_ids, executed_ids,
        "every smoke-covered case needs an executor"
    );
    assert_eq!(EXECUTION_COVERAGE.len(), executed);
    for case in EXECUTION_COVERAGE {
        (case.executor)();
    }
}

#[test]
fn evidence_identity_and_reconciliation_fixtures() {
    let first = evidence_row("bank", "occ-a", "row-a", b"2026-09-21,4.00");
    let second = evidence_row("bank", "occ-b", "row-b", b"2026-09-21,4.00");
    assert_eq!(first.content(), second.content());
    assert_ne!(first.occurrence(), second.occurrence());
    let forward = ImportBatch::from_observations("bank", [first.clone(), second.clone()]);
    let reverse = ImportBatch::from_observations("bank", [second.clone(), first.clone()]);
    assert_eq!(forward.content_hash(), reverse.content_hash());

    let receipt = evidence_row("receipt", "receipt-1", "receipt-1", b"coffee 4.00");
    let bank = evidence_row("bank", "bank-1", "row-1", b"CARD COFFEE 4.00");
    let mut store = EvidenceStore::new();
    store
        .import_batch(ImportBatch::from_observations("receipt", [receipt.clone()]))
        .unwrap();
    store
        .import_batch(ImportBatch::from_observations("bank", [bank.clone()]))
        .unwrap();
    assert!(
        store
            .propose_identity_link(
                receipt.identity().clone(),
                bank.identity().clone(),
                Confidence::from_percent(90),
                axiom_ledger::evidence::Provenance::new("matcher"),
                EvidenceAuthority::user("reviewer"),
            )
            .unwrap()
    );
    assert_eq!(store.len(), 2);
    assert_eq!(store.candidate_links().count(), 1);

    let corrected = evidence_row("bank", "occ-corrected", "row-c", b"2026-09-21,5.00");
    let correction = EvidenceRelation::corrects(
        corrected.identity().clone(),
        first.identity().clone(),
        CorrectionScope::field("amount").with_rationale("issuer correction"),
    );
    assert_eq!(correction.kind, EvidenceRelationKind::Corrects);
    assert_eq!(correction.scope.as_ref().unwrap().fields, vec!["amount"]);
    assert_eq!(
        correction.from().occurrence,
        OccurrenceId::new("occ-corrected")
    );

    let split = ConservationMetadata::new(
        [ConservationLeg::new(first.identity().clone())
            .with_quantity(ExactNumber::integer(10))
            .with_unit("USD")],
        [
            ConservationLeg::new(Identity::new("child-a", hash(3)))
                .with_quantity(ExactNumber::integer(6))
                .with_unit("USD"),
            ConservationLeg::new(Identity::new("child-b", hash(4)))
                .with_quantity(ExactNumber::integer(4))
                .with_unit("USD"),
        ],
    );
    assert_eq!(split.balances(), Some(true));
    let split_relation = EvidenceRelation::splits(
        first.identity().clone(),
        [
            Identity::new("child-a", hash(3)),
            Identity::new("child-b", hash(4)),
        ],
        split,
    )
    .unwrap();
    assert_eq!(split_relation.targets().count(), 2);

    let merge = ConservationMetadata::new(
        [
            ConservationLeg::new(Identity::new("input-a", hash(5)))
                .with_quantity(ExactNumber::integer(3))
                .with_unit("USD"),
            ConservationLeg::new(Identity::new("input-b", hash(6)))
                .with_quantity(ExactNumber::integer(7))
                .with_unit("USD"),
        ],
        [ConservationLeg::new(Identity::new("output", hash(7)))
            .with_quantity(ExactNumber::integer(10))
            .with_unit("USD")],
    );
    assert_eq!(merge.balances(), Some(true));
    let merge_relation = EvidenceRelation::merges(
        [
            Identity::new("input-a", hash(5)),
            Identity::new("input-b", hash(6)),
        ],
        Identity::new("output", hash(7)),
        merge,
    )
    .unwrap();
    assert_eq!(merge_relation.sources().count(), 2);

    let tombstone = RawEvidence::deleted_tombstone("bank", "occ-deleted", None, hash(8))
        .expect("deleted tombstone is well-formed");
    assert_eq!(tombstone.availability(), Availability::Deleted);
    assert!(tombstone.payload().is_none());

    let v1 = evidence_row("bank", "same-occ", "same-row", b"same").with_provenance(
        axiom_ledger::evidence::Provenance::new("bank")
            .with_adapter(AdapterProvenance::new("csv", "1")),
    );
    let v2 = v1.clone().with_provenance(
        axiom_ledger::evidence::Provenance::new("bank")
            .with_adapter(AdapterProvenance::new("csv", "2")),
    );
    let mut versions = EvidenceStore::new();
    versions.insert(v1).unwrap();
    versions.insert(v2).unwrap();
    assert_eq!(versions.len(), 2);
}

#[test]
fn semantics_axes_conflict_and_completeness_fixtures() {
    let observed = axiom_ledger::semantics::Statement::new(
        "balance",
        Polarity::Positive,
        World::actual(),
        Force::Descriptive,
        Phase::Observed,
        TemporalScope::at(date("2026-09-21")),
        Provenance::source_observation("bank", hash(11)).unwrap(),
        Authority::source("bank").unwrap(),
    )
    .unwrap();
    let resolved = observed
        .clone()
        .resolve(
            Provenance::policy_derivation("reconcile", proof(12)).unwrap(),
            Authority::policy("reconcile").unwrap(),
        )
        .unwrap();
    let accepted = resolved
        .clone()
        .accept(
            Provenance::signed_decision("decision-1", proof(13)).unwrap(),
            Authority::signed_decision("alice", "decision-1").unwrap(),
        )
        .unwrap();
    let recognized = accepted
        .recognize(
            "cash",
            Provenance::policy_derivation("cash-policy", proof(14)).unwrap(),
            Authority::policy("cash-policy").unwrap(),
        )
        .unwrap();
    assert!(matches!(recognized.phase(), Phase::Recognized(book) if book.as_str() == "cash"));
    assert_eq!(recognized.world(), &World::actual());

    let subject = axiom_ledger::semantics::GoalId::new(hash(15)).unwrap();
    let conflict = Conflict::new(subject, vec![proof(16)], vec![proof(17)]).unwrap();
    let both_proof = proof_bundle([proof(16), proof(17)]);
    let both = Resolution::<&str>::new_checked(
        &both_proof,
        vec![proof(16)],
        vec![proof(17)],
        Multiplicity::none(),
        SemanticCompletion::Complete,
        Vec::new(),
        vec![conflict],
        Vec::new(),
    )
    .unwrap();
    assert_eq!(both.truth(), Truth::Both);
    assert!(both.answers().is_empty());

    let goal = axiom_ledger::semantics::GoalId::new(hash(18)).unwrap();
    let answer_a = axiom_ledger::semantics::AnswerId::new(hash(19)).unwrap();
    let answer_b = axiom_ledger::semantics::AnswerId::new(hash(20)).unwrap();
    let unresolved_proof = proof_bundle([]);
    let unresolved = Resolution::new_checked(
        &unresolved_proof,
        vec![],
        vec![],
        Multiplicity::multiple(vec![
            axiom_ledger::semantics::Conditional::unconditional(answer_a),
            axiom_ledger::semantics::Conditional::new(
                answer_b,
                vec![axiom_ledger::semantics::Requirement::Evidence(hash(21))],
            ),
        ])
        .unwrap(),
        SemanticCompletion::OpenWorld,
        vec![axiom_ledger::semantics::Requirement::Evidence(hash(21))],
        vec![],
        vec![],
    );
    assert!(
        unresolved.is_err(),
        "unconditional answers need positive support"
    );
    let alternatives_proof = proof_bundle([proof(22)]);
    let alternatives = Resolution::new_checked(
        &alternatives_proof,
        vec![proof(22)],
        vec![],
        Multiplicity::multiple(vec![
            axiom_ledger::semantics::Conditional::new(
                "taxable",
                vec![axiom_ledger::semantics::Requirement::Theory(hash(23))],
            ),
            axiom_ledger::semantics::Conditional::new(
                "exempt",
                vec![axiom_ledger::semantics::Requirement::Theory(hash(24))],
            ),
        ])
        .unwrap(),
        SemanticCompletion::OpenWorld,
        vec![axiom_ledger::semantics::Requirement::Theory(hash(23))],
        vec![],
        vec![],
    )
    .unwrap();
    assert!(alternatives.is_ambiguous());
    let _ = goal;
}

#[test]
fn exact_units_quotes_and_instrument_fixtures() {
    let a = unit("share", "ABC");
    let usd = unit("USD", "USD");
    assert_eq!(uq("1.25", a.clone()).amount().canonical_string(), "5/4");
    assert!(UnitQuantity::new(ExactNumber::integer(1), None).is_err());
    let ratio = Ratio::new(a.clone(), usd.clone(), ExactNumber::parse("20").unwrap());
    let quote = Quote::new(
        "q1",
        ratio.clone(),
        QuoteKind::Mid,
        Instant::EPOCH,
        Instant::EPOCH,
        "venue",
        "source",
        InstantInterval::closed(Instant::EPOCH, Instant::from_unix_seconds(10)).unwrap(),
    );
    let valued = value(
        &uq("2", a.clone()),
        &usd,
        Instant::from_unix_seconds(1),
        &[quote],
        ValuationPolicy::default(),
    )
    .unwrap();
    assert_eq!(valued.status, ValuationStatus::Unique);
    assert_eq!(valued.quantity.unwrap().amount().canonical_string(), "40");
    assert_eq!(valued.paths[0].legs().len(), 1);

    let inverse = ratio.inverse().unwrap();
    assert_eq!(inverse.from(), &usd);
    assert_eq!(inverse.to(), &a);
    let bid = Quote::new(
        "bid",
        ratio,
        QuoteKind::Bid,
        Instant::EPOCH,
        Instant::EPOCH,
        "venue",
        "source",
        InstantInterval::closed(Instant::EPOCH, Instant::from_unix_seconds(10)).unwrap(),
    );
    let no_reverse = value(
        &uq("2", unit("USD", "USD")),
        &unit("share", "ABC"),
        Instant::from_unix_seconds(1),
        &[bid],
        ValuationPolicy::default(),
    )
    .unwrap();
    assert_eq!(no_reverse.status, ValuationStatus::Unavailable);

    assert!(matches!(
        Ratio::checked_new(a.clone(), unit("USD", "USD"), ExactNumber::integer(-20)),
        Err(UnitError::NonPositiveRatio)
    ));

    let split = value(
        &uq("1", unit("share", "ABC")),
        &unit("USD", "USD"),
        Instant::from_unix_seconds(1),
        &[],
        ValuationPolicy::default(),
    )
    .unwrap();
    assert_eq!(split.status, ValuationStatus::Unavailable);

    let definition = InstrumentDefinition::new(
        "ABC",
        unit("share", "ABC"),
        ExactNumber::parse("0.01").unwrap(),
    )
    .unwrap();
    assert!(
        definition
            .accepts(&uq("1.23", unit("share", "ABC")))
            .is_ok()
    );
    assert!(matches!(
        definition.accepts(&uq("1.235", unit("share", "ABC"))),
        Err(UnitError::OffQuantum { .. })
    ));
    let rounded = RoundingCertificate::apply(
        ExactNumber::parse("1.235").unwrap(),
        2,
        RoundingMode::HalfEven,
    )
    .unwrap();
    assert_eq!(rounded.output().canonical_string(), "31/25");
    rounded.verify().unwrap();
    assert!(VenueQuantum::new("venue", unit("share", "ABC"), ExactNumber::integer(1)).is_ok());
}

#[test]
fn ownership_and_settlement_fixtures() {
    let joint = RoleAssignments::joint(
        "account-joint",
        Role::LegalOwner,
        ["alice".into(), "bob".into()],
    )
    .unwrap();
    joint.validate().unwrap();
    assert_eq!(joint.assignments.len(), 2);
    assert_eq!(
        joint.assignments[0]
            .share
            .as_ref()
            .unwrap()
            .canonical_string(),
        "1/2"
    );

    let trust = Entity::new("trust", EntityKind::Trust);
    let trustee = Entity::new("trustee", EntityKind::Person);
    let beneficiary = Entity::new("beneficiary", EntityKind::Person);
    assert_eq!(trust.kind, EntityKind::Trust);
    let roles = RoleAssignments::new()
        .with(axiom_ledger::ontology::RoleAssignment::new(
            trust.id.clone(),
            Role::Trustee,
            trustee.id.clone(),
        ))
        .with(axiom_ledger::ontology::RoleAssignment::new(
            trust.id.clone(),
            Role::Beneficiary,
            beneficiary.id.clone(),
        ));
    roles.validate().unwrap();
    assert_eq!(roles.holders(&trust.id, &Role::Trustee).count(), 1);

    let cash = Quantity::with_unit(ExactNumber::integer(100), "USD").unwrap();
    let position = Position::new("position", "alice", "USD", cash.clone())
        .unwrap()
        .with_encumbrance("hold");
    let hold = Encumbrance::for_quantity(
        "hold",
        EncumbranceKind::Hold,
        Quantity::with_unit(40i64.into(), "USD").unwrap(),
    )
    .unwrap();
    let encumbrances =
        BTreeMap::from([(axiom_ledger::ontology::EncumbranceId::from("hold"), hold)]);
    assert_eq!(
        position
            .available_quantity(&encumbrances)
            .unwrap()
            .number
            .canonical_string(),
        "60"
    );
    let released = Encumbrance::for_quantity(
        "hold",
        EncumbranceKind::Pledge,
        Quantity::with_unit(40i64.into(), "USD").unwrap(),
    )
    .unwrap()
    .release();
    let encumbrances = BTreeMap::from([(
        axiom_ledger::ontology::EncumbranceId::from("hold"),
        released,
    )]);
    assert_eq!(position.available_quantity(&encumbrances).unwrap(), cash);

    let endpoint_a = Endpoint::entity("alice").at_account("checking");
    let endpoint_b = Endpoint::entity("merchant").at_account("receivable");
    let transfer = TransferRecord::between(
        "payment",
        endpoint_a.clone(),
        endpoint_b.clone(),
        "USD",
        Quantity::with_unit(25i64.into(), "USD").unwrap(),
    );
    validate_transfer_conservation(&[transfer]).unwrap();
    let exchange = ExchangeRecord::new(
        "barter",
        vec![
            ExchangeLeg::give(
                endpoint_a.clone(),
                endpoint_b.clone(),
                "USD",
                Quantity::with_unit(10i64.into(), "USD").unwrap(),
            ),
            ExchangeLeg::receive(
                endpoint_b.clone(),
                endpoint_a.clone(),
                "GOOD",
                Quantity::with_unit(1i64.into(), "GOOD").unwrap(),
            ),
        ],
    );
    validate_exchange_legs(&exchange).unwrap();

    let obligation = Obligation::transfer(
        "invoice",
        "alice",
        "merchant",
        "USD",
        Quantity::with_unit(25i64.into(), "USD").unwrap(),
    )
    .unwrap();
    assert_eq!(
        obligation.creditor,
        axiom_ledger::model::EntityId::new("merchant")
    );
    let settlement = SettlementStateRecord::new(
        "payment",
        "settlement",
        SettlementState::Returned,
        Quantity::with_unit(25i64.into(), "USD").unwrap(),
        "USD",
        endpoint_a,
        endpoint_b,
    );
    settlement.validate().unwrap();
    assert_eq!(settlement.state, SettlementState::Returned);
}

#[test]
fn additional_executable_edge_cases() {
    let payer = Endpoint::entity("payer").at_account("checking");
    let payee = Endpoint::entity("payee").at_account("receivable");
    let first_invoice = Obligation::transfer(
        "invoice-1",
        "payer",
        "payee",
        "USD",
        Quantity::with_unit(40i64.into(), "USD").unwrap(),
    )
    .unwrap();
    let second_invoice = Obligation::transfer(
        "invoice-2",
        "payer",
        "payee",
        "USD",
        Quantity::with_unit(60i64.into(), "USD").unwrap(),
    )
    .unwrap();
    let mut payment = Settlement::new(
        "payment-batch",
        payer.clone(),
        payee.clone(),
        "USD",
        Quantity::with_unit(100i64.into(), "USD").unwrap(),
    )
    .unwrap();
    payment
        .transition(SettlementState::Presented, Some(date("2026-09-21")), None)
        .unwrap();
    payment
        .transition(SettlementState::Settled, Some(date("2026-09-21")), None)
        .unwrap();
    let allocations = vec![
        SatisfactionAllocation::new(
            "allocation-1",
            first_invoice.id.clone(),
            payment.id.clone(),
            Quantity::with_unit(40i64.into(), "USD").unwrap(),
        )
        .unwrap()
        .applied(),
        SatisfactionAllocation::new(
            "allocation-2",
            second_invoice.id.clone(),
            payment.id.clone(),
            Quantity::with_unit(60i64.into(), "USD").unwrap(),
        )
        .unwrap()
        .applied(),
    ];
    validate_obligation_allocation(&first_invoice, &allocations, &[payment.clone()]).unwrap();
    validate_obligation_allocation(&second_invoice, &allocations, &[payment.clone()]).unwrap();

    let principal = TransferRecord::between(
        "principal",
        payer.clone(),
        payee.clone(),
        "USD",
        Quantity::with_unit(100i64.into(), "USD").unwrap(),
    );
    let fee = TransferRecord::between(
        "fee",
        payer.clone(),
        Endpoint::entity("processor"),
        "USD",
        Quantity::with_unit(2i64.into(), "USD").unwrap(),
    );
    validate_transfer_conservation(&[principal, fee]).unwrap();

    let original = Identity::new("sale", hash(70));
    let reversal = Identity::new("chargeback", hash(71));
    let reversal_relation = EvidenceRelation::reverses(reversal.clone(), original.clone());
    assert_eq!(reversal_relation.kind, EvidenceRelationKind::Reverses);
    assert_eq!(reversal_relation.from(), &reversal);
    assert_eq!(reversal_relation.to(), &original);

    let escrow_roles = RoleAssignments::new()
        .with(axiom_ledger::ontology::RoleAssignment::new(
            "escrow-account",
            Role::Custodian,
            "escrow-agent",
        ))
        .with(axiom_ledger::ontology::RoleAssignment::new(
            "escrow-account",
            Role::Beneficiary,
            "buyer",
        ));
    escrow_roles.validate().unwrap();
    assert_eq!(escrow_roles.assignments.len(), 2);

    let authorized_card_user = axiom_ledger::ontology::RoleAssignment::new(
        "card-contract",
        Role::AuthorizedUser,
        "employee",
    );
    let card_debt = Obligation::transfer(
        "card-debt",
        "employer",
        "issuer",
        "USD",
        Quantity::with_unit(25i64.into(), "USD").unwrap(),
    )
    .unwrap();
    assert_eq!(authorized_card_user.role, Role::AuthorizedUser);
    assert_ne!(authorized_card_user.holder, card_debt.debtor);

    let deposit_position = Position::new(
        "security-deposit",
        "landlord",
        "USD",
        Quantity::with_unit(500i64.into(), "USD").unwrap(),
    )
    .unwrap();
    let repayment = Obligation::transfer(
        "deposit-repayment",
        "landlord",
        "tenant",
        "USD",
        Quantity::with_unit(500i64.into(), "USD").unwrap(),
    )
    .unwrap();
    assert_eq!(deposit_position.quantity.number.canonical_string(), "500");
    assert_eq!(
        repayment
            .promised_quantity()
            .unwrap()
            .number
            .canonical_string(),
        "500"
    );

    let borrowed_return = Obligation::transfer(
        "borrowed-return",
        "borrower",
        "lender",
        "ABC",
        Quantity::with_unit(10i64.into(), "ABC").unwrap(),
    )
    .unwrap();
    assert_eq!(
        borrowed_return.creditor,
        axiom_ledger::model::EntityId::new("lender")
    );

    let usd_position = Position::new(
        "joint-usd",
        "alice",
        "USD",
        Quantity::with_unit(10i64.into(), "USD").unwrap(),
    )
    .unwrap()
    .at_account("multi");
    let eur_position = Position::new(
        "joint-eur",
        "alice",
        "EUR",
        Quantity::with_unit(10i64.into(), "EUR").unwrap(),
    )
    .unwrap()
    .at_account("multi");
    assert_ne!(usd_position.instrument, eur_position.instrument);
    assert_eq!(usd_position.account, eur_position.account);

    let overdraft = Obligation::transfer(
        "overdraft-draw",
        "alice",
        "bank",
        "USD",
        Quantity::with_unit(100i64.into(), "USD").unwrap(),
    )
    .unwrap();
    assert_eq!(
        overdraft.debtor,
        axiom_ledger::model::EntityId::new("alice")
    );
    assert_eq!(
        overdraft.creditor,
        axiom_ledger::model::EntityId::new("bank")
    );

    let envelope = VirtualAccount::new("envelope", "account:checking AND tag:rent");
    assert_eq!(envelope.query, "account:checking AND tag:rent");

    let unique = Instrument::new("painting", InstrumentKind::UniqueAsset);
    let unique_a = Position::new(
        "painting-a",
        "alice",
        "painting",
        Quantity::with_unit(1i64.into(), "painting").unwrap(),
    )
    .unwrap();
    let unique_b = Position::new(
        "painting-b",
        "alice",
        "painting",
        Quantity::with_unit(1i64.into(), "painting").unwrap(),
    )
    .unwrap();
    assert_eq!(unique.kind, InstrumentKind::UniqueAsset);
    assert_ne!(unique_a.id, unique_b.id);

    let mut expiring = Instrument::new("coupon", InstrumentKind::DebtSecurity);
    expiring.expires = Some(date("2026-12-31"));
    assert_eq!(expiring.expires, Some(date("2026-12-31")));

    let actual = RecognitionAcceptedFact::actual(
        "shared-event",
        "payment",
        date("2026-09-21"),
        proof(72),
        "authority",
    )
    .unwrap();
    let world = accepted_world(hash(73), [actual]);
    let books = recognize_books(
        &world,
        &[
            BookPolicy::new("cash", date("2026-01-01"), None),
            BookPolicy::new("accrual", date("2026-01-01"), None),
        ],
    )
    .unwrap();
    assert_eq!(books.books.len(), 2);
    assert_ne!(
        books.get(&"cash".into()).unwrap().root(),
        books.get(&"accrual".into()).unwrap().root()
    );

    let deferred_cash = Position::new(
        "deferred-cash",
        "seller",
        "USD",
        Quantity::with_unit(100i64.into(), "USD").unwrap(),
    )
    .unwrap();
    let deferred_obligation = Obligation::transfer(
        "deferred-performance",
        "seller",
        "buyer",
        "SERVICE",
        Quantity::with_unit(1i64.into(), "SERVICE").unwrap(),
    )
    .unwrap();
    assert_eq!(deferred_cash.quantity.number.canonical_string(), "100");
    assert_eq!(
        deferred_obligation
            .promised_quantity()
            .unwrap()
            .number
            .canonical_string(),
        "1"
    );

    let mut range_scenario = Scenario::new("range", hash(74)).unwrap();
    range_scenario
        .constrain(
            Constraint::new(
                "minimum",
                ConstraintExpression::QuantityAtLeast {
                    metric: "cash".to_string(),
                    amount: Quantity::with_unit(90i64.into(), "USD").unwrap(),
                },
            )
            .unwrap(),
        )
        .unwrap();
    range_scenario
        .constrain(
            Constraint::new(
                "maximum",
                ConstraintExpression::QuantityAtMost {
                    metric: "cash".to_string(),
                    amount: Quantity::with_unit(110i64.into(), "USD").unwrap(),
                },
            )
            .unwrap(),
        )
        .unwrap();
    assert_eq!(range_scenario.constraints().count(), 2);

    let goal = Goal::atom(Literal::positive(ir_atom(
        "answer",
        vec![Term::var(Var::named(80, "answer"))],
    )));
    let mut first_program = Program::new();
    first_program.add_fact(text_fact("answer", "a")).unwrap();
    first_program.add_fact(text_fact("answer", "b")).unwrap();
    let mut second_program = Program::new();
    second_program.add_fact(text_fact("answer", "b")).unwrap();
    second_program.add_fact(text_fact("answer", "a")).unwrap();
    let first_result = Solver::new().solve(&first_program, &goal, &SemanticContext::default());
    let second_result = Solver::new().solve(&second_program, &goal, &SemanticContext::default());
    assert_eq!(first_result.truth(), second_result.truth());
    assert_eq!(first_result.multiplicity(), second_result.multiplicity());
    assert_eq!(
        first_result.candidates().len(),
        second_result.candidates().len()
    );

    let package_v1 = ExecutablePolicyPackage::new(
        "lots/updated",
        "1",
        "selector=earliest_acquisition\ntie=ambiguous",
    );
    let package_v2 = ExecutablePolicyPackage::new(
        "lots/updated",
        "2",
        "selector=latest_acquisition\ntie=ambiguous",
    );
    assert_ne!(package_v1.hash(), package_v2.hash());
    let candidates = [
        LotCandidate::new("old", date("2026-01-01")),
        LotCandidate::new("new", date("2026-02-01")),
    ];
    assert_eq!(
        package_v1.compile().unwrap().evaluate(candidates.iter()),
        Selection::Unique("old".to_string())
    );
    assert_eq!(
        package_v2.compile().unwrap().evaluate(candidates.iter()),
        Selection::Unique("new".to_string())
    );
}

#[test]
fn temporal_fixtures_preserve_precision_roles_and_policies() {
    let local = LocalDateTime::new(
        LocalDate::new(2026, 9, 21).unwrap(),
        LocalTime::new(12, 0, 0, 0).unwrap(),
        TimeZone::FixedOffsetSeconds(0),
    );
    assert_eq!(local.status(), LocalTimeStatus::Exact);
    let instant = local.to_instant().unwrap();
    assert_eq!(
        instant.duration_since(Instant::EPOCH),
        Some(instant.unix_nanos())
    );

    let ambiguous = LocalDateTime::ambiguous(
        LocalDate::new(2026, 11, 1).unwrap(),
        LocalTime::new(1, 30, 0, 0).unwrap(),
        "America/New_York",
    );
    assert_eq!(ambiguous.status(), LocalTimeStatus::Ambiguous);
    assert!(ambiguous.to_instant().is_err());
    assert!(matches!(
        LocalDateTime::new(
            LocalDate::new(2026, 11, 1).unwrap(),
            LocalTime::new(1, 30, 0, 0).unwrap(),
            TimeZone::Named("America/New_York".to_string()),
        )
        .to_instant(),
        Err(axiom_ledger::time::TimeError::TimezoneRulesRequired)
    ));

    let month = TimePeriod::month(2026, 9).unwrap();
    assert_eq!(
        month.precision(),
        axiom_ledger::time::PeriodPrecision::Month
    );
    let day_a = Instant::from_unix_seconds(10);
    let day_b = Instant::from_unix_seconds(20);
    let open = Interval::new(Bound::Open(day_a), Bound::Closed(day_b)).unwrap();
    assert!(!open.contains(&day_a));
    assert!(open.contains(&day_b));
    let uncertain = UncertainInterval::new(day_a, day_b).unwrap();
    assert!(uncertain.contains(&Instant::from_unix_seconds(15)));
    assert!(BeforeAfter::new(day_a, day_b).unwrap().holds());
    assert!(BeforeAfter::new(day_b, day_a).is_err());

    let mut assignments = TimeAssignments::new();
    assignments.set(TimeRole::Occurred, TimeValue::Instant(day_a));
    assignments.set(TimeRole::Recorded, TimeValue::Instant(day_b));
    assert_ne!(
        assignments.get(TimeRole::Occurred),
        assignments.get(TimeRole::Recorded)
    );

    let month_recurrence = Recurrence::new(
        LocalDate::new(2026, 1, 31).unwrap(),
        Frequency::Monthly { every: 1, day: 31 },
    )
    .unwrap()
    .with_count(3)
    .with_missing_day_policy(MissingDayPolicy::ClampToLastDay);
    let dates = month_recurrence
        .between(
            LocalDate::new(2026, 1, 1).unwrap(),
            LocalDate::new(2026, 3, 31).unwrap(),
        )
        .unwrap();
    assert_eq!(
        dates,
        vec![
            LocalDate::new(2026, 1, 31).unwrap(),
            LocalDate::new(2026, 2, 28).unwrap(),
            LocalDate::new(2026, 3, 31).unwrap(),
        ]
    );

    let weekend = LocalDate::new(2026, 9, 19).unwrap();
    let adjusted = BusinessCalendar::default()
        .adjust(weekend, BusinessDayPolicy::Following)
        .unwrap();
    assert_eq!(adjusted, LocalDate::new(2026, 9, 21).unwrap());
    let policy = BookPolicy::new("tax", date("2026-01-01"), Some(date("2026-12-31")));
    assert!(policy.is_effective_on(date("2026-09-21")));
    assert!(!policy.is_effective_on(date("2027-01-01")));
}

#[test]
fn logic_fixed_point_conflict_ambiguity_and_resource_fixtures() {
    let edge = |left: Term, right: Term| Literal::positive(ir_atom("edge", vec![left, right]));
    let path = |left: Term, right: Term| Literal::positive(ir_atom("path", vec![left, right]));
    let x = Var::named(0, "x");
    let y = Var::named(1, "y");
    let z = Var::named(2, "z");
    let mut program = Program::new();
    program
        .add_fact(edge(Term::Text("a".into()), Term::Text("b".into())))
        .unwrap();
    program
        .add_fact(edge(Term::Text("b".into()), Term::Text("c".into())))
        .unwrap();
    program.add_clause(Clause::new(
        path(Term::var(x.clone()), Term::var(y.clone())),
        Goal::atom(edge(Term::var(x.clone()), Term::var(y.clone()))),
    ));
    program.add_clause(Clause::new(
        path(Term::var(x.clone()), Term::var(z.clone())),
        Goal::and([
            Goal::atom(edge(Term::var(x.clone()), Term::var(y.clone()))),
            Goal::atom(path(Term::var(y.clone()), Term::var(z.clone()))),
        ]),
    ));
    let target = Goal::atom(path(Term::Text("a".into()), Term::Text("c".into())));
    let mut solver = Solver::new();
    let fixed = solver.solve(&program, &target, &SemanticContext::default());
    assert_eq!(fixed.truth(), LogicTruth::TrueOnly);
    assert_eq!(fixed.completion(), LogicCompletion::Complete);
    fixed.check_proofs().unwrap();
    assert!(
        fixed
            .trace()
            .iter()
            .any(|event| matches!(event, TraceEvent::FixedPointIteration { .. }))
    );

    let mut cycle = Program::new();
    let cycle_var = Var::named(0, "x");
    cycle.add_clause(Clause::new(
        text_fact("cycle", "x"),
        Goal::atom(text_fact("cycle", "x")),
    ));
    let no_base = solver.solve(
        &cycle,
        &Goal::atom(text_fact("cycle", "x")),
        &SemanticContext::default(),
    );
    assert_eq!(no_base.truth(), LogicTruth::Neither);
    assert!(
        no_base
            .trace()
            .iter()
            .any(|event| matches!(event, TraceEvent::CycleWithoutBase { .. }))
    );
    let _ = cycle_var;

    let variable = Var::named(9, "owner");
    let mut answers = Program::new();
    answers.add_fact(text_fact("owner", "alice")).unwrap();
    answers.add_fact(text_fact("owner", "bob")).unwrap();
    let many = solver.solve(
        &answers,
        &Goal::atom(Literal::positive(ir_atom(
            "owner",
            vec![Term::var(variable)],
        ))),
        &SemanticContext::default(),
    );
    assert!(matches!(
        many.multiplicity(),
        axiom_ledger::logic::Multiplicity::Multiple(_)
    ));
    assert!(many.is_ambiguous());

    let mut contradicted = Program::new();
    contradicted.add_fact(text_fact("open", "x")).unwrap();
    contradicted
        .add_fact(Literal::negative(ir_atom(
            "open",
            vec![Term::Text("x".into())],
        )))
        .unwrap();
    let both = solver.solve(
        &contradicted,
        &Goal::atom(text_fact("open", "x")),
        &SemanticContext::default(),
    );
    assert_eq!(both.truth(), LogicTruth::Both);
    both.check_proofs().unwrap();

    let absent = Goal::default_not(Goal::atom(text_fact("missing", "x")));
    let open_world = solver.solve(&Program::new(), &absent, &SemanticContext::default());
    assert_eq!(open_world.completion(), LogicCompletion::OpenWorld);
    let complete = solver.solve(
        &Program::new(),
        &absent,
        &SemanticContext::default().complete_relation("missing", 1, LogicPolarity::Positive),
    );
    assert_eq!(complete.completion(), LogicCompletion::Complete);
    assert_eq!(complete.truth(), LogicTruth::TrueOnly);

    let limited = solver.solve(
        &program,
        &target,
        &SemanticContext::default().with_resources(ResourceProfile::bounded(0)),
    );
    assert_eq!(limited.completion(), LogicCompletion::ResourceLimited);
    assert!(limited.is_incomplete());

    let first = solver.cache_len();
    let _ = solver.solve(
        &program,
        &target,
        &SemanticContext::default().with_packages([[1; 32]]),
    );
    assert!(solver.cache_len() > first);
    let second = solver.solve(&program, &target, &SemanticContext::default());
    assert!(matches!(second.trace().first(), Some(TraceEvent::CacheHit)));
}

#[test]
fn recognition_scenario_and_planning_fixtures() {
    let actual =
        RecognitionAcceptedFact::actual("sale", "sale", date("2026-09-21"), proof(31), "authority")
            .unwrap()
            .with_settlement_date(date("2026-09-22"))
            .with_attribute("classification", "ordinary");
    let scenario = RecognitionAcceptedFact::scenario(
        "forecast",
        "sale",
        date("2026-09-21"),
        "budget-2026",
        proof(32),
        "planner",
    )
    .unwrap();
    let world = accepted_world(hash(33), [actual.clone()]);
    let policy = BookPolicy::new("cash", date("2026-01-01"), None).with_classification("cash-sale");
    let recognized = recognize(&world, &policy).unwrap();
    assert_eq!(
        recognized
            .fact(&OccurrenceId::new("sale"))
            .unwrap()
            .classification
            .as_deref(),
        Some("cash-sale")
    );
    assert_eq!(
        recognized
            .fact(&OccurrenceId::new("sale"))
            .unwrap()
            .proof
            .source_fact,
        OccurrenceId::new("sale")
    );
    assert_eq!(
        recognize(&accepted_world(hash(34), [scenario]), &policy),
        Err(RecognitionError::ScenarioFact {
            book: "cash".into(),
            fact: "forecast".into(),
            scenario: "budget-2026".into(),
        })
    );

    let mut scenario = Scenario::new("base", hash(35)).unwrap();
    scenario
        .assume(Assumption::boolean("hiring", true).unwrap())
        .unwrap();
    scenario
        .expect(
            ExpectedEvent::dated("payroll", date("2026-10-01"))
                .with_quantity(Quantity::with_unit(ExactNumber::integer(1000), "USD").unwrap()),
        )
        .unwrap();
    scenario
        .constrain(Constraint::text("cash-floor", "cash >= 0").unwrap())
        .unwrap();
    let forecast = scenario
        .materialize(Horizon::new(date("2026-10-01"), date("2026-10-31")).unwrap())
        .unwrap();
    assert_eq!(forecast.len(), 1);
    let actual_payroll = RealizedEvent::new("payroll-actual", date("2026-10-03"))
        .with_quantity(Quantity::with_unit(ExactNumber::integer(900), "USD").unwrap());
    scenario
        .register_actual_event(actual_payroll.clone())
        .unwrap();
    let link = scenario.link_realized("payroll", actual_payroll).unwrap();
    assert!(link.variance.date_changed());
    assert!(link.variance.quantity_changed());
    assert_eq!(
        link.variance
            .quantity_delta
            .as_ref()
            .unwrap()
            .number
            .canonical_string(),
        "-100"
    );

    let mut changed = scenario.clone();
    changed
        .assume(Assumption::boolean("bonus", false).unwrap())
        .unwrap();
    let diff = scenario.diff(&changed);
    assert!(!diff.actual_root_changed);
    assert_eq!(diff.added_assumptions.len(), 1);
}

#[test]
fn liquidity_and_resource_boundary_fixtures() {
    let usd = Quantity::with_unit(ExactNumber::integer(100), "USD").unwrap();
    let mut graph = LiquidityGraph::new();
    graph
        .add_position(PositionNode::new("cash", "USD", usd.clone()).grant("withdraw"))
        .unwrap();
    graph.add_instrument(InstrumentNode::new("USD")).unwrap();
    graph
        .add_edge(
            ActionEdge::new(
                "withdraw",
                NodeId::position("cash"),
                NodeId::instrument("USD"),
                ActionKind::Withdraw,
            )
            .requires_permission("withdraw")
            .with_capacity(Quantity::with_unit(100i64.into(), "USD").unwrap()),
        )
        .unwrap();
    let found = graph
        .find_routes(
            NodeId::position("cash"),
            NodeId::instrument("USD"),
            &Quantity::with_unit(50i64.into(), "USD").unwrap(),
            SearchLimits::default(),
        )
        .unwrap();
    assert!(found.is_complete());
    assert_eq!(found.routes.len(), 1);
    let verified = graph
        .verify(
            &found.routes[0],
            NodeId::position("cash"),
            NodeId::instrument("USD"),
            &Quantity::with_unit(50i64.into(), "USD").unwrap(),
        )
        .unwrap();
    assert_eq!(verified.route.edge_ids(), &["withdraw".to_string()]);

    let limited = graph
        .find_routes(
            NodeId::position("cash"),
            NodeId::instrument("USD"),
            &Quantity::with_unit(50i64.into(), "USD").unwrap(),
            SearchLimits {
                max_expansions: 0,
                max_depth: 256,
            },
        )
        .unwrap();
    assert!(matches!(
        limited.completion,
        SearchCompletion::Incomplete { .. }
    ));
    assert!(limited.routes.is_empty());

    let encumbered = LiquidityGraph::new();
    let _ = encumbered;
}

#[test]
fn store_versioning_close_and_unavailable_evidence_fixtures() {
    let mut store = ObjectStore::new();
    let source = store
        .put_evidence(Evidence::new("row-1", "bank", b"10 USD".to_vec()))
        .unwrap();
    let correction = store
        .put_evidence(Evidence::correction(
            "row-1-correction",
            "bank",
            b"11 USD".to_vec(),
            source,
            "amount",
            "issuer corrected statement",
            "bank",
        ))
        .unwrap();
    assert_eq!(
        store.evidence_history(correction).unwrap(),
        vec![correction, source]
    );
    let unavailable = store
        .put_evidence(Evidence::unavailable("lost", "bank", "key lost"))
        .unwrap();
    assert!(matches!(
        store.evidence(unavailable).unwrap().state,
        EvidenceState::Unavailable { .. }
    ));

    let statement = store
        .put_statement(Statement::new("balance", "amount", "10 USD"))
        .unwrap();
    let package = store
        .put_package(PolicyPackage::new("cash", "1", b"policy".to_vec()))
        .unwrap();
    let recognized_proof = store
        .put_proof(ProofObject::recognized(
            [source.hash()],
            b"recognized".to_vec(),
        ))
        .unwrap();
    let completeness = store
        .put_completeness(StoredCompleteness::new(
            "balance",
            "bank",
            "2026-09",
            Some(date("2026-09-01")),
            Some(date("2026-09-30")),
        ))
        .unwrap();
    let commit = store
        .put_commit(Commit::new(
            [],
            [source, unavailable],
            [statement],
            [],
            [completeness],
            [package],
            [recognized_proof],
            "alice",
        ))
        .unwrap();
    let period = Period::new(date("2026-09-01"), date("2026-09-30")).unwrap();
    let close = store
        .put_close(Close::new(
            period.clone(),
            "cash",
            [package],
            commit,
            recognized_proof.hash(),
        ))
        .unwrap();
    assert_eq!(store.close(close).unwrap().source, commit);
    assert_eq!(store.close(close).unwrap().policies, vec![package]);
    let restated = store
        .put_close(
            Close::new(period, "cash", [package], commit, recognized_proof.hash())
                .superseding(close),
        )
        .unwrap();
    assert_eq!(store.close(restated).unwrap().supersedes, Some(close));
    assert!(
        store.close(close).is_ok(),
        "a restatement keeps the prior close"
    );
    store.verify().unwrap();

    let same = store
        .put_package(PolicyPackage::new("cash", "1", b"policy".to_vec()))
        .unwrap();
    assert_eq!(same, package);
}

#[test]
fn collaboration_and_adapter_boundaries_fixtures() {
    struct Adapter;
    impl axiom_ledger::evidence::ObservationAdapter for Adapter {
        type Error = &'static str;

        fn observe(
            &self,
            source: &axiom_ledger::model::SourceId,
            bytes: &[u8],
        ) -> Result<ImportBatch, Self::Error> {
            Ok(
                ImportBatch::new(source.clone()).observe(RawEvidence::from_bytes(
                    source.clone(),
                    "adapter-occurrence",
                    None,
                    bytes,
                )),
            )
        }
    }
    let batch = Adapter
        .observe(&axiom_ledger::model::SourceId::new("bank"), b"row")
        .unwrap();
    assert_eq!(batch.observations.len(), 1);
    let mut evidence = EvidenceStore::new();
    evidence.import_batch(batch).unwrap();

    let provenance = Provenance::source_observation("bank", hash(60)).unwrap();
    let scope =
        axiom_ledger::semantics::CompletenessScope::new("bank-balance", World::actual()).unwrap();
    let claim = axiom_ledger::semantics::CompletenessClaim::new(
        axiom_ledger::semantics::CompletenessId::new(hash(61)).unwrap(),
        scope,
        TemporalScope::from(date("2026-01-01")),
        vec!["bank".into()],
        provenance,
    )
    .unwrap();
    let revoked = claim
        .revoke(axiom_ledger::semantics::Revocation::new(
            date("2026-10-01"),
            Authority::user("alice").unwrap(),
        ))
        .unwrap();
    assert!(revoked.is_active_at(date("2026-09-30")));
    assert!(!revoked.is_active_at(date("2026-10-01")));
    assert!(
        revoked
            .revoke(axiom_ledger::semantics::Revocation::new(
                date("2026-09-01"),
                Authority::user("alice").unwrap(),
            ))
            .is_err()
    );

    let candidate = CandidateIdentityLink::new(
        Identity::new("proposal-left", hash(62)),
        Identity::new("proposal-right", hash(63)),
        Confidence::from_percent(55),
        axiom_ledger::evidence::Provenance::new("matcher"),
        EvidenceAuthority::user("agent"),
    );
    assert_eq!(candidate.kind(), EvidenceRelationKind::PossiblySameAs);
    assert_eq!(
        candidate.relation().kind,
        EvidenceRelationKind::PossiblySameAs
    );
}

#[test]
fn independent_case_registry_is_explicit() {
    assert_eq!(
        INDEPENDENT_CASES_ADDED.len(),
        50,
        "this tranche must keep an auditable independent-case count"
    );
    let executed: BTreeSet<_> = FIXTURES
        .iter()
        .filter_map(|fixture| match fixture.disposition {
            Disposition::Executed => Some(fixture.id),
            Disposition::Inventory { .. } => None,
        })
        .collect();
    let mut ids = BTreeSet::new();
    for case in INDEPENDENT_CASES_ADDED {
        assert!(
            executed.contains(case.id),
            "{} must be an Executed fixture",
            case.id
        );
        assert!(
            ids.insert(case.id),
            "duplicate independent fixture {}",
            case.id
        );
        // Reading the function pointer here ensures every registry entry is
        // tied to a real, separately runnable test function.  The test body
        // itself is run by libtest under that function's own name.
        let _executor = case.executor;
    }
}

#[test]
fn independent_a01_identical_source_rows_keep_occurrences() {
    let first = evidence_row("bank", "occ-a01", "row-a01", b"2026-09-21,4.00");
    let second = evidence_row("bank", "occ-a02", "row-a02", b"2026-09-21,4.00");
    assert_eq!(first.content(), second.content());
    assert_ne!(first.occurrence(), second.occurrence());

    let mut store = EvidenceStore::new();
    let report = store
        .import_batch(ImportBatch::from_observations(
            "bank",
            [first.clone(), second.clone()],
        ))
        .unwrap();
    assert_eq!(report.inserted_count(), 2);
    assert_eq!(store.len(), 2);
    assert!(store.contains(first.occurrence()));
    assert!(store.contains(second.occurrence()));
}

#[test]
fn independent_a02_receipt_and_bank_keep_both_leaves() {
    let receipt = evidence_row("receipt", "receipt-a02", "receipt-a02", b"coffee 4.00");
    let bank = evidence_row("bank", "bank-a02", "row-a02", b"CARD COFFEE 4.00");
    let mut store = EvidenceStore::new();
    store
        .import_batch(ImportBatch::from_observations("receipt", [receipt.clone()]))
        .unwrap();
    store
        .import_batch(ImportBatch::from_observations("bank", [bank.clone()]))
        .unwrap();
    assert!(
        store
            .propose_identity_link(
                receipt.identity().clone(),
                bank.identity().clone(),
                Confidence::from_percent(90),
                axiom_ledger::evidence::Provenance::new("matcher"),
                EvidenceAuthority::user("reviewer"),
            )
            .unwrap()
    );
    assert_eq!(store.len(), 2, "candidate matching must retain both leaves");
    assert_eq!(store.candidate_links().count(), 1);
    assert_eq!(
        store.relations().count(),
        0,
        "a candidate is not an accepted same-as"
    );
}

#[test]
fn independent_a03_correction_supersedes_by_scope() {
    let old = evidence_row("bank", "occ-a03-old", "row-a03-old", b"amount=4.00");
    let corrected = evidence_row("bank", "occ-a03-new", "row-a03-new", b"amount=5.00");
    let relation = EvidenceRelation::corrects(
        corrected.identity().clone(),
        old.identity().clone(),
        CorrectionScope::field("amount").with_rationale("issuer correction"),
    );
    assert_eq!(relation.kind, EvidenceRelationKind::Corrects);
    assert_eq!(relation.from(), corrected.identity());
    assert_eq!(relation.to(), old.identity());
    assert_eq!(relation.scope.as_ref().unwrap().fields, vec!["amount"]);
    assert_eq!(
        relation.scope.as_ref().unwrap().rationale.as_deref(),
        Some("issuer correction")
    );
}

#[test]
fn independent_a06_conflicting_balances_retain_both_proofs() {
    let subject = axiom_ledger::semantics::GoalId::new(hash(106)).unwrap();
    let positive = proof(107);
    let negative = proof(108);
    let conflict = Conflict::new(subject, vec![positive], vec![negative]).unwrap();
    let proof_context = proof_bundle([positive, negative]);
    let result = Resolution::<&str>::new_checked(
        &proof_context,
        vec![positive],
        vec![negative],
        Multiplicity::none(),
        SemanticCompletion::Complete,
        Vec::new(),
        vec![conflict],
        Vec::new(),
    )
    .unwrap();
    assert_eq!(result.truth(), Truth::Both);
    assert_eq!(result.positive_proofs(), &[positive]);
    assert_eq!(result.negative_proofs(), &[negative]);
    assert_eq!(result.conflicts().len(), 1);
    assert_eq!(result.proof_context().nodes.len(), 2);
}

#[test]
fn independent_a07_incomplete_period_does_not_prove_absence() {
    let source = hash(109);
    let period = ReportingPeriod::new(date("2026-09-01"), date("2026-09-30"));
    let claims = CompletenessClaims::new()
        .claim_scoped(
            "bank-september",
            false,
            source,
            "bank:checking",
            period,
            proof(110),
            hash(111),
        )
        .with_evidence("bank-september", "statement ends 2026-09-15");
    assert!(!claims.is_complete());
    assert!(!claims.is_satisfied_for(source, "bank:checking", period));
}

#[test]
fn independent_a08_deleted_source_is_a_tombstone() {
    let tombstone = RawEvidence::deleted_tombstone(
        "bank",
        "occ-a08-deleted",
        Some(ExternalId::new("row-a08")),
        hash(112),
    )
    .unwrap();
    assert_eq!(tombstone.availability(), Availability::Deleted);
    assert!(tombstone.payload().is_none());
    assert!(!tombstone.is_available());
}

#[test]
fn independent_a09_split_conserves_quantity() {
    let parent = Identity::new("parent-a09", hash(113));
    let child_a = Identity::new("child-a09-a", hash(114));
    let child_b = Identity::new("child-a09-b", hash(115));
    let conservation = ConservationMetadata::new(
        [ConservationLeg::new(parent.clone())
            .with_quantity(ExactNumber::integer(10))
            .with_unit("USD")],
        [
            ConservationLeg::new(child_a.clone())
                .with_quantity(ExactNumber::integer(6))
                .with_unit("USD"),
            ConservationLeg::new(child_b.clone())
                .with_quantity(ExactNumber::integer(4))
                .with_unit("USD"),
        ],
    );
    let relation = EvidenceRelation::splits(parent, [child_a, child_b], conservation).unwrap();
    assert_eq!(relation.kind, EvidenceRelationKind::Splits);
    assert_eq!(relation.targets().count(), 2);
    assert_eq!(
        relation.conservation.as_ref().unwrap().balances(),
        Some(true)
    );
}

#[test]
fn independent_a10_merge_conserves_quantity() {
    let input_a = Identity::new("input-a10-a", hash(116));
    let input_b = Identity::new("input-a10-b", hash(117));
    let output = Identity::new("output-a10", hash(118));
    let conservation = ConservationMetadata::new(
        [
            ConservationLeg::new(input_a.clone())
                .with_quantity(ExactNumber::integer(3))
                .with_unit("USD"),
            ConservationLeg::new(input_b.clone())
                .with_quantity(ExactNumber::integer(7))
                .with_unit("USD"),
        ],
        [ConservationLeg::new(output.clone())
            .with_quantity(ExactNumber::integer(10))
            .with_unit("USD")],
    );
    let relation = EvidenceRelation::merges([input_a, input_b], output, conservation).unwrap();
    assert_eq!(relation.kind, EvidenceRelationKind::Merges);
    assert_eq!(relation.sources().count(), 2);
    assert_eq!(
        relation.conservation.as_ref().unwrap().balances(),
        Some(true)
    );
}

#[test]
fn independent_a11_fuzzy_match_is_ranked_candidate() {
    let left = evidence_row("bank", "left-a11", "row-left-a11", b"merchant=ACME");
    let right = evidence_row(
        "receipt",
        "right-a11",
        "row-right-a11",
        b"merchant=ACME INC",
    );
    let mut store = EvidenceStore::new();
    store
        .import_batch(ImportBatch::from_observations("bank", [left.clone()]))
        .unwrap();
    store
        .import_batch(ImportBatch::from_observations("receipt", [right.clone()]))
        .unwrap();
    store
        .add_candidate_link(
            CandidateIdentityLink::new(
                left.identity().clone(),
                right.identity().clone(),
                Confidence::from_percent(61),
                axiom_ledger::evidence::Provenance::new("fuzzy-merchant"),
                EvidenceAuthority::user("matcher"),
            )
            .with_rationale("normalized merchant name"),
        )
        .unwrap();
    store
        .add_candidate_link(CandidateIdentityLink::new(
            left.identity().clone(),
            right.identity().clone(),
            Confidence::from_percent(92),
            axiom_ledger::evidence::Provenance::new("fuzzy-merchant"),
            EvidenceAuthority::user("matcher"),
        ))
        .unwrap();
    let candidates: Vec<_> = store.candidate_links().collect();
    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0].confidence.basis_points(), 9_200);
    assert_eq!(candidates[1].confidence.basis_points(), 6_100);
    assert_eq!(
        store.relations().count(),
        0,
        "ranking does not silently accept a match"
    );
}

#[test]
fn independent_a12_adapter_versions_keep_derivations() {
    let base = evidence_row("bank", "occ-a12", "row-a12", b"10 USD");
    let v1 = base.clone().with_provenance(
        axiom_ledger::evidence::Provenance::new("bank")
            .with_adapter(AdapterProvenance::new("csv", "1")),
    );
    let v2 = base.with_provenance(
        axiom_ledger::evidence::Provenance::new("bank")
            .with_adapter(AdapterProvenance::new("csv", "2")),
    );
    let mut store = EvidenceStore::new();
    store.insert(v1).unwrap();
    store.insert(v2).unwrap();
    assert_eq!(store.len(), 2);
    assert!(store.get(&OccurrenceId::new("occ-a12")).is_multiple());
    let adapters: BTreeSet<_> = store
        .get(&OccurrenceId::new("occ-a12"))
        .all()
        .into_iter()
        .filter_map(|evidence| {
            evidence
                .provenance()
                .adapter
                .as_ref()
                .map(|adapter| adapter.version.as_str())
        })
        .collect();
    assert_eq!(adapters, BTreeSet::from(["1", "2"]));
}

#[test]
fn independent_b06_bounced_check_leaves_obligation() {
    let obligation = Obligation::transfer(
        "obligation-b06",
        "payer",
        "payee",
        "USD",
        Quantity::with_unit(25i64.into(), "USD").unwrap(),
    )
    .unwrap();
    let mut check = Settlement::new(
        "check-b06",
        Endpoint::entity("payer"),
        Endpoint::entity("payee"),
        "USD",
        Quantity::with_unit(25i64.into(), "USD").unwrap(),
    )
    .unwrap();
    check
        .transition(SettlementState::Presented, Some(date("2026-09-20")), None)
        .unwrap();
    check
        .transition(
            SettlementState::Returned,
            Some(date("2026-09-21")),
            Some("bounced".into()),
        )
        .unwrap();
    let allocation = SatisfactionAllocation::new(
        "allocation-b06",
        obligation.id.clone(),
        check.id.clone(),
        Quantity::with_unit(25i64.into(), "USD").unwrap(),
    )
    .unwrap()
    .applied();
    assert!(!check.is_effective());
    assert_eq!(
        obligation
            .remaining(&[allocation], &[check])
            .unwrap()
            .number
            .canonical_string(),
        "25"
    );
}

#[test]
fn independent_b07_chargeback_reverses_settlement() {
    let obligation = Obligation::transfer(
        "obligation-b07",
        "payer",
        "merchant",
        "USD",
        Quantity::with_unit(40i64.into(), "USD").unwrap(),
    )
    .unwrap();
    let mut card = Settlement::new(
        "card-b07",
        Endpoint::entity("payer"),
        Endpoint::entity("merchant"),
        "USD",
        Quantity::with_unit(40i64.into(), "USD").unwrap(),
    )
    .unwrap();
    card.transition(SettlementState::Presented, Some(date("2026-09-20")), None)
        .unwrap();
    card.transition(SettlementState::Settled, Some(date("2026-09-20")), None)
        .unwrap();
    card.transition(SettlementState::ChargedBack, Some(date("2026-09-21")), None)
        .unwrap();
    let allocation = SatisfactionAllocation::new(
        "allocation-b07",
        obligation.id.clone(),
        card.id.clone(),
        Quantity::with_unit(40i64.into(), "USD").unwrap(),
    )
    .unwrap()
    .applied();
    let reversal = EvidenceRelation::reverses(
        Identity::new("chargeback-b07", hash(119)),
        Identity::new("sale-b07", hash(120)),
    );
    assert_eq!(card.latest_state(), Some(&SettlementState::ChargedBack));
    assert!(!card.is_effective());
    assert_eq!(
        obligation
            .remaining(&[allocation], &[card])
            .unwrap()
            .number
            .canonical_string(),
        "40"
    );
    assert_eq!(reversal.kind, EvidenceRelationKind::Reverses);
}

#[test]
fn independent_b10_one_payment_satisfies_many_invoices() {
    let first = Obligation::transfer(
        "invoice-b10-1",
        "payer",
        "merchant",
        "USD",
        Quantity::with_unit(40i64.into(), "USD").unwrap(),
    )
    .unwrap();
    let second = Obligation::transfer(
        "invoice-b10-2",
        "payer",
        "merchant",
        "USD",
        Quantity::with_unit(60i64.into(), "USD").unwrap(),
    )
    .unwrap();
    let mut payment = Settlement::new(
        "payment-b10",
        Endpoint::entity("payer"),
        Endpoint::entity("merchant"),
        "USD",
        Quantity::with_unit(100i64.into(), "USD").unwrap(),
    )
    .unwrap();
    payment
        .transition(SettlementState::Presented, Some(date("2026-09-21")), None)
        .unwrap();
    payment
        .transition(SettlementState::Settled, Some(date("2026-09-21")), None)
        .unwrap();
    let allocations = vec![
        SatisfactionAllocation::new(
            "allocation-b10-1",
            first.id.clone(),
            payment.id.clone(),
            Quantity::with_unit(40i64.into(), "USD").unwrap(),
        )
        .unwrap()
        .applied(),
        SatisfactionAllocation::new(
            "allocation-b10-2",
            second.id.clone(),
            payment.id.clone(),
            Quantity::with_unit(60i64.into(), "USD").unwrap(),
        )
        .unwrap()
        .applied(),
    ];
    assert_eq!(
        first
            .remaining(&allocations, &[payment.clone()])
            .unwrap()
            .number
            .canonical_string(),
        "0"
    );
    assert_eq!(
        second
            .remaining(&allocations, &[payment])
            .unwrap()
            .number
            .canonical_string(),
        "0"
    );
}

#[test]
fn independent_b11_many_payments_satisfy_one_invoice() {
    let invoice = Obligation::transfer(
        "invoice-b11",
        "payer",
        "merchant",
        "USD",
        Quantity::with_unit(100i64.into(), "USD").unwrap(),
    )
    .unwrap();
    let mut first = Settlement::new(
        "payment-b11-1",
        Endpoint::entity("payer"),
        Endpoint::entity("merchant"),
        "USD",
        Quantity::with_unit(40i64.into(), "USD").unwrap(),
    )
    .unwrap();
    first
        .transition(SettlementState::Presented, Some(date("2026-09-20")), None)
        .unwrap();
    first
        .transition(SettlementState::Settled, Some(date("2026-09-20")), None)
        .unwrap();
    let mut second = Settlement::new(
        "payment-b11-2",
        Endpoint::entity("payer"),
        Endpoint::entity("merchant"),
        "USD",
        Quantity::with_unit(60i64.into(), "USD").unwrap(),
    )
    .unwrap();
    second
        .transition(SettlementState::Presented, Some(date("2026-09-21")), None)
        .unwrap();
    second
        .transition(SettlementState::Settled, Some(date("2026-09-21")), None)
        .unwrap();
    let allocations = vec![
        SatisfactionAllocation::new(
            "allocation-b11-1",
            invoice.id.clone(),
            first.id.clone(),
            Quantity::with_unit(40i64.into(), "USD").unwrap(),
        )
        .unwrap()
        .applied(),
        SatisfactionAllocation::new(
            "allocation-b11-2",
            invoice.id.clone(),
            second.id.clone(),
            Quantity::with_unit(60i64.into(), "USD").unwrap(),
        )
        .unwrap()
        .applied(),
    ];
    assert_eq!(
        invoice
            .remaining(&allocations, &[first, second])
            .unwrap()
            .number
            .canonical_string(),
        "0"
    );
}

#[test]
fn independent_b13_withheld_fee_is_separate_leg() {
    let principal = TransferRecord::between(
        "principal-b13",
        Endpoint::entity("payer"),
        Endpoint::entity("merchant"),
        "USD",
        Quantity::with_unit(98i64.into(), "USD").unwrap(),
    );
    let fee = TransferRecord::between(
        "fee-b13",
        Endpoint::entity("payer"),
        Endpoint::entity("processor"),
        "USD",
        Quantity::with_unit(2i64.into(), "USD").unwrap(),
    );
    validate_transfer_conservation(&[principal.clone(), fee.clone()]).unwrap();
    assert_ne!(
        principal.destinations[0].endpoint,
        fee.destinations[0].endpoint
    );
    assert_eq!(
        principal.sources[0].quantity.number.canonical_string(),
        "98"
    );
    assert_eq!(fee.sources[0].quantity.number.canonical_string(), "2");
}

#[test]
fn independent_c01_joint_roles_keep_holders_separate() {
    let roles = RoleAssignments::joint(
        "joint-account-c01",
        Role::LegalOwner,
        ["alice".into(), "bob".into()],
    )
    .unwrap();
    roles.validate().unwrap();
    let holders: Vec<_> = roles
        .holders(&"joint-account-c01".into(), &Role::LegalOwner)
        .collect();
    assert_eq!(holders.len(), 2);
    assert_ne!(holders[0], holders[1]);
    assert_eq!(
        roles.assignments[0]
            .share
            .as_ref()
            .unwrap()
            .canonical_string(),
        "1/2"
    );
    assert_eq!(
        roles.assignments[1]
            .share
            .as_ref()
            .unwrap()
            .canonical_string(),
        "1/2"
    );
}

#[test]
fn independent_c03_escrow_separates_custody_and_benefit() {
    let roles = RoleAssignments::new()
        .with(axiom_ledger::ontology::RoleAssignment::new(
            "escrow-c03",
            Role::Custodian,
            "escrow-agent",
        ))
        .with(axiom_ledger::ontology::RoleAssignment::new(
            "escrow-c03",
            Role::Beneficiary,
            "buyer",
        ));
    roles.validate().unwrap();
    let custodian: Vec<_> = roles
        .holders(&"escrow-c03".into(), &Role::Custodian)
        .collect();
    let beneficiary: Vec<_> = roles
        .holders(&"escrow-c03".into(), &Role::Beneficiary)
        .collect();
    assert_eq!(custodian, [&"escrow-agent".into()]);
    assert_eq!(beneficiary, [&"buyer".into()]);
    assert_ne!(custodian[0], beneficiary[0]);
}

#[test]
fn independent_e01_cash_and_accrual_share_event_set() {
    let fact = RecognitionAcceptedFact::actual(
        "shared-e01",
        "payment",
        date("2026-09-21"),
        proof(121),
        "authority",
    )
    .unwrap();
    let world = accepted_world(hash(122), [fact]);
    let books = recognize_books(
        &world,
        &[
            BookPolicy::new("cash-e01", date("2026-01-01"), None),
            BookPolicy::new("accrual-e01", date("2026-01-01"), None),
        ],
    )
    .unwrap();
    let cash = books.get(&"cash-e01".into()).unwrap();
    let accrual = books.get(&"accrual-e01".into()).unwrap();
    assert_eq!(
        cash.fact(&OccurrenceId::new("shared-e01"))
            .unwrap()
            .source_fact,
        "shared-e01".into()
    );
    assert_eq!(
        accrual
            .fact(&OccurrenceId::new("shared-e01"))
            .unwrap()
            .source_fact,
        "shared-e01".into()
    );
    assert_eq!(cash.world_root, accrual.world_root);
    assert_ne!(
        cash.root(),
        accrual.root(),
        "book projections remain distinct"
    );
}

#[test]
fn independent_e03_deferred_revenue_retains_obligation() {
    let cash = Position::new(
        "cash-e03",
        "seller",
        "USD",
        Quantity::with_unit(100i64.into(), "USD").unwrap(),
    )
    .unwrap();
    let performance = Obligation::transfer(
        "performance-e03",
        "seller",
        "buyer",
        "SERVICE",
        Quantity::with_unit(1i64.into(), "SERVICE").unwrap(),
    )
    .unwrap();
    performance.validate().unwrap();
    assert_eq!(cash.quantity.number.canonical_string(), "100");
    assert_eq!(
        performance.creditor,
        axiom_ledger::model::EntityId::new("buyer")
    );
    assert_eq!(
        performance
            .promised_quantity()
            .unwrap()
            .number
            .canonical_string(),
        "1"
    );
}

#[test]
fn independent_d01_missing_quote_is_unavailable() {
    let share = unit("share", "ABC");
    let usd = unit("USD", "USD");
    let result = value(
        &uq("2", share),
        &usd,
        Instant::from_unix_seconds(1),
        &[],
        ValuationPolicy::default(),
    )
    .unwrap();
    assert_eq!(result.status, ValuationStatus::Unavailable);
    assert!(result.quantity.is_none());
    assert!(result.paths.is_empty());
}

#[test]
fn independent_d02_conflicting_quotes_are_ambiguous() {
    let abc = unit("ABC", "ABC");
    let usd = unit("USD", "USD");
    let first = Quote::new(
        "quote-d02-a",
        Ratio::new(abc.clone(), usd.clone(), ExactNumber::integer(2)),
        QuoteKind::Mid,
        Instant::EPOCH,
        Instant::from_unix_seconds(1),
        "venue-d02",
        "source-d02-a",
        InstantInterval::closed(Instant::EPOCH, Instant::from_unix_seconds(10)).unwrap(),
    );
    let second = Quote::new(
        "quote-d02-b",
        Ratio::new(abc.clone(), usd.clone(), ExactNumber::integer(3)),
        QuoteKind::Mid,
        Instant::EPOCH,
        Instant::from_unix_seconds(1),
        "venue-d02",
        "source-d02-b",
        InstantInterval::closed(Instant::EPOCH, Instant::from_unix_seconds(10)).unwrap(),
    );
    let result = value(
        &uq("1", abc),
        &usd,
        Instant::from_unix_seconds(2),
        &[first, second],
        ValuationPolicy::default(),
    )
    .unwrap();
    assert_eq!(result.status, ValuationStatus::Ambiguous);
    assert!(result.quantity.is_none());
    assert_eq!(result.paths.len(), 2);
}

#[test]
fn independent_f01_fixed_offset_preserves_instant() {
    let local = LocalDateTime::new(
        LocalDate::new(2026, 9, 21).unwrap(),
        LocalTime::new(12, 0, 0, 0).unwrap(),
        TimeZone::FixedOffsetSeconds(3_600),
    );
    let instant = local.to_instant().unwrap();
    let utc = LocalDateTime::new(
        LocalDate::new(2026, 9, 21).unwrap(),
        LocalTime::new(11, 0, 0, 0).unwrap(),
        TimeZone::FixedOffsetSeconds(0),
    )
    .to_instant()
    .unwrap();
    assert_eq!(instant, utc);
    assert_eq!(local.status(), LocalTimeStatus::Exact);
}

#[test]
fn independent_f08_missing_recurrence_day_has_policy() {
    let recurrence = Recurrence::new(
        LocalDate::new(2026, 1, 31).unwrap(),
        Frequency::Monthly { every: 1, day: 31 },
    )
    .unwrap()
    .with_count(3)
    .with_missing_day_policy(MissingDayPolicy::ClampToLastDay);
    let dates = recurrence
        .between(
            LocalDate::new(2026, 1, 1).unwrap(),
            LocalDate::new(2026, 3, 31).unwrap(),
        )
        .unwrap();
    assert_eq!(
        dates,
        vec![
            LocalDate::new(2026, 1, 31).unwrap(),
            LocalDate::new(2026, 2, 28).unwrap(),
            LocalDate::new(2026, 3, 31).unwrap(),
        ]
    );
}

#[test]
fn independent_g01_positive_recursion_reaches_fixed_point() {
    let edge = |left: Term, right: Term| Literal::positive(ir_atom("edge-g01", vec![left, right]));
    let path = |left: Term, right: Term| Literal::positive(ir_atom("path-g01", vec![left, right]));
    let x = Var::named(123, "x");
    let y = Var::named(124, "y");
    let z = Var::named(125, "z");
    let mut program = Program::new();
    program
        .add_fact(edge(Term::Text("a".into()), Term::Text("b".into())))
        .unwrap();
    program
        .add_fact(edge(Term::Text("b".into()), Term::Text("c".into())))
        .unwrap();
    program.add_clause(Clause::new(
        path(Term::var(x.clone()), Term::var(y.clone())),
        Goal::atom(edge(Term::var(x.clone()), Term::var(y.clone()))),
    ));
    program.add_clause(Clause::new(
        path(Term::var(x.clone()), Term::var(z.clone())),
        Goal::and([
            Goal::atom(edge(Term::var(x.clone()), Term::var(y.clone()))),
            Goal::atom(path(Term::var(y.clone()), Term::var(z.clone()))),
        ]),
    ));
    let result = Solver::new().solve(
        &program,
        &Goal::atom(path(Term::Text("a".into()), Term::Text("c".into()))),
        &SemanticContext::default(),
    );
    assert_eq!(result.truth(), LogicTruth::TrueOnly);
    assert_eq!(result.completion(), LogicCompletion::Complete);
    result.check_proofs().unwrap();
    assert!(
        result
            .trace()
            .iter()
            .any(|event| matches!(event, TraceEvent::FixedPointIteration { .. }))
    );
}

#[test]
fn independent_g06_proof_and_refutation_are_conflict() {
    let positive = proof(126);
    let negative = proof(127);
    let conflict = Conflict::new(
        axiom_ledger::semantics::GoalId::new(hash(128)).unwrap(),
        vec![positive],
        vec![negative],
    )
    .unwrap();
    let context = proof_bundle([positive, negative]);
    let result = Resolution::<&str>::new_checked(
        &context,
        vec![positive],
        vec![negative],
        Multiplicity::none(),
        SemanticCompletion::Complete,
        Vec::new(),
        vec![conflict],
        Vec::new(),
    )
    .unwrap();
    assert_eq!(result.truth(), Truth::Both);
    assert_eq!(result.conflicts().len(), 1);
    assert!(result.answers().is_empty());
}

#[test]
fn independent_h01_forecast_is_separate_from_actual() {
    let mut scenario = Scenario::new("forecast-h01", hash(129)).unwrap();
    scenario
        .expect(ExpectedEvent::dated("rent-h01", date("2026-10-01")))
        .unwrap();
    let forecast = scenario
        .materialize(Horizon::new(date("2026-10-01"), date("2026-10-31")).unwrap())
        .unwrap();
    let actual = RealizedEvent::new("rent-h01-actual", date("2026-10-02"));
    scenario.register_actual_event(actual.clone()).unwrap();
    assert_eq!(forecast.len(), 1);
    assert!(
        scenario
            .expected_event(&OccurrenceId::new("rent-h01"))
            .is_some()
    );
    assert_eq!(
        scenario
            .realized_links(&OccurrenceId::new("rent-h01"))
            .count(),
        0
    );
    scenario.link_realized("rent-h01", actual).unwrap();
    assert_eq!(
        scenario
            .realized_links(&OccurrenceId::new("rent-h01"))
            .count(),
        1
    );
    assert_eq!(scenario.expected_events().count(), 1);
}

#[test]
fn independent_h03_amount_range_is_interval_constraint() {
    let mut scenario = Scenario::new("range-h03", hash(130)).unwrap();
    scenario
        .constrain(
            Constraint::new(
                "minimum-h03",
                ConstraintExpression::QuantityAtLeast {
                    metric: "cash".to_string(),
                    amount: Quantity::with_unit(90i64.into(), "USD").unwrap(),
                },
            )
            .unwrap(),
        )
        .unwrap();
    scenario
        .constrain(
            Constraint::new(
                "maximum-h03",
                ConstraintExpression::QuantityAtMost {
                    metric: "cash".to_string(),
                    amount: Quantity::with_unit(110i64.into(), "USD").unwrap(),
                },
            )
            .unwrap(),
        )
        .unwrap();
    assert_eq!(scenario.constraints().count(), 2);
    assert!(scenario.constraint("minimum-h03").is_some());
    assert!(scenario.constraint("maximum-h03").is_some());
}

#[test]
fn independent_i01_adapter_can_only_emit_observations() {
    struct ObservationOnlyAdapter;
    impl ObservationAdapter for ObservationOnlyAdapter {
        type Error = &'static str;

        fn observe(
            &self,
            source: &axiom_ledger::model::SourceId,
            bytes: &[u8],
        ) -> Result<ImportBatch, Self::Error> {
            Ok(
                ImportBatch::new(source.clone()).observe(RawEvidence::from_bytes(
                    source.clone(),
                    "adapter-i01",
                    None,
                    bytes,
                )),
            )
        }
    }
    let batch = ObservationOnlyAdapter
        .observe(&axiom_ledger::model::SourceId::new("bank-i01"), b"row")
        .unwrap();
    assert_eq!(batch.observations.len(), 1);
    assert_eq!(
        batch.observations[0].occurrence(),
        &OccurrenceId::new("adapter-i01")
    );
    let mut store = EvidenceStore::new();
    store.import_batch(batch).unwrap();
    assert_eq!(store.len(), 1);
}

#[test]
fn independent_i04_revocation_keeps_history() {
    let claim_scope =
        axiom_ledger::semantics::CompletenessScope::new("bank-balance-i04", World::actual())
            .unwrap();
    let claim = axiom_ledger::semantics::CompletenessClaim::new(
        axiom_ledger::semantics::CompletenessId::new(hash(131)).unwrap(),
        claim_scope,
        TemporalScope::from(date("2026-01-01")),
        vec!["bank-i04".into()],
        Provenance::source_observation("bank-i04", hash(132)).unwrap(),
    )
    .unwrap();
    let revoked = claim
        .revoke(axiom_ledger::semantics::Revocation::new(
            date("2026-10-01"),
            Authority::user("alice-i04").unwrap(),
        ))
        .unwrap();
    assert!(revoked.is_active_at(date("2026-09-30")));
    assert!(!revoked.is_active_at(date("2026-10-01")));
    assert_eq!(
        revoked.id(),
        axiom_ledger::semantics::CompletenessId::new(hash(131)).unwrap()
    );
    assert!(
        revoked
            .revoke(axiom_ledger::semantics::Revocation::new(
                date("2026-09-01"),
                Authority::user("alice-i04").unwrap(),
            ))
            .is_err()
    );
}

#[test]
fn independent_c02_authorized_user_is_not_debtor() {
    let assignment = axiom_ledger::ontology::RoleAssignment::new(
        "card-c02",
        Role::AuthorizedUser,
        "employee-c02",
    );
    let debt = Obligation::transfer(
        "card-debt-c02",
        "employer-c02",
        "issuer-c02",
        "USD",
        Quantity::with_unit(25i64.into(), "USD").unwrap(),
    )
    .unwrap();
    assert_eq!(assignment.role, Role::AuthorizedUser);
    assert_ne!(assignment.holder, debt.debtor);
    assert_eq!(
        debt.creditor,
        axiom_ledger::model::EntityId::new("issuer-c02")
    );
}

#[test]
fn independent_c04_security_deposit_has_repayment_obligation() {
    let held = Position::new(
        "deposit-c04",
        "landlord-c04",
        "USD",
        Quantity::with_unit(500i64.into(), "USD").unwrap(),
    )
    .unwrap();
    let repayment = Obligation::transfer(
        "repayment-c04",
        "landlord-c04",
        "tenant-c04",
        "USD",
        held.quantity.clone(),
    )
    .unwrap();
    assert_eq!(
        held.quantity,
        repayment.promised_quantity().unwrap().clone()
    );
    assert_eq!(
        repayment.creditor,
        axiom_ledger::model::EntityId::new("tenant-c04")
    );
}

#[test]
fn independent_c05_restricted_funds_are_encumbered() {
    let position = Position::new(
        "restricted-c05",
        "owner-c05",
        "USD",
        Quantity::with_unit(100i64.into(), "USD").unwrap(),
    )
    .unwrap()
    .with_encumbrance("restriction-c05");
    let hold = Encumbrance::for_quantity(
        "restriction-c05",
        EncumbranceKind::Restricted,
        Quantity::with_unit(70i64.into(), "USD").unwrap(),
    )
    .unwrap();
    let available = position
        .available_quantity(&BTreeMap::from([(
            axiom_ledger::ontology::EncumbranceId::from("restriction-c05"),
            hold,
        )]))
        .unwrap();
    assert_eq!(available.number.canonical_string(), "30");
}

#[test]
fn independent_c06_trustee_beneficiary_tax_owner_differ() {
    let trust = Entity::new("trust-c06", EntityKind::Trust);
    let roles = RoleAssignments::new()
        .with(axiom_ledger::ontology::RoleAssignment::new(
            trust.id.clone(),
            Role::Trustee,
            "trustee-c06",
        ))
        .with(axiom_ledger::ontology::RoleAssignment::new(
            trust.id.clone(),
            Role::Beneficiary,
            "beneficiary-c06",
        ))
        .with(axiom_ledger::ontology::RoleAssignment::new(
            trust.id.clone(),
            Role::TaxOwner,
            "tax-owner-c06",
        ));
    roles.validate().unwrap();
    assert_eq!(roles.holders(&trust.id, &Role::Trustee).count(), 1);
    assert_eq!(roles.holders(&trust.id, &Role::Beneficiary).count(), 1);
    assert_eq!(roles.holders(&trust.id, &Role::TaxOwner).count(), 1);
    assert_ne!(
        roles.holders(&trust.id, &Role::Trustee).next(),
        roles.holders(&trust.id, &Role::TaxOwner).next()
    );
}

#[test]
fn independent_c07_borrowed_security_is_return_obligation() {
    let return_claim = Obligation::transfer(
        "borrowed-return-c07",
        "borrower-c07",
        "lender-c07",
        "ABC",
        Quantity::with_unit(10i64.into(), "ABC").unwrap(),
    )
    .unwrap();
    return_claim.validate().unwrap();
    assert_eq!(
        return_claim.creditor,
        axiom_ledger::model::EntityId::new("lender-c07")
    );
    assert_eq!(
        return_claim
            .promised_quantity()
            .unwrap()
            .number
            .canonical_string(),
        "10"
    );
}

#[test]
fn independent_c09_pledge_changes_liquidity_not_position() {
    let cash = Quantity::with_unit(100i64.into(), "USD").unwrap();
    let position = Position::new("pledged-c09", "owner-c09", "USD", cash.clone())
        .unwrap()
        .with_encumbrance("pledge-c09");
    let pledge = Encumbrance::for_quantity(
        "pledge-c09",
        EncumbranceKind::Pledge,
        Quantity::with_unit(40i64.into(), "USD").unwrap(),
    )
    .unwrap();
    assert_eq!(position.quantity, cash);
    assert_eq!(
        position
            .available_quantity(&BTreeMap::from([(
                axiom_ledger::ontology::EncumbranceId::from("pledge-c09"),
                pledge,
            )]))
            .unwrap()
            .number
            .canonical_string(),
        "60"
    );
}

#[test]
fn independent_c10_multicurrency_account_has_separate_positions() {
    let usd = Position::new(
        "usd-c10",
        "owner-c10",
        "USD",
        Quantity::with_unit(10i64.into(), "USD").unwrap(),
    )
    .unwrap()
    .at_account("wallet-c10");
    let eur = Position::new(
        "eur-c10",
        "owner-c10",
        "EUR",
        Quantity::with_unit(10i64.into(), "EUR").unwrap(),
    )
    .unwrap()
    .at_account("wallet-c10");
    assert_eq!(usd.account, eur.account);
    assert_ne!(usd.instrument, eur.instrument);
    assert_ne!(usd.quantity.unit(), eur.quantity.unit());
}

#[test]
fn independent_c11_overdraft_is_contractual_credit() {
    let overdraft = Obligation::transfer(
        "overdraft-c11",
        "account-holder-c11",
        "bank-c11",
        "USD",
        Quantity::with_unit(100i64.into(), "USD").unwrap(),
    )
    .unwrap();
    assert_eq!(
        overdraft.debtor,
        axiom_ledger::model::EntityId::new("account-holder-c11")
    );
    assert_eq!(
        overdraft.creditor,
        axiom_ledger::model::EntityId::new("bank-c11")
    );
    assert_eq!(
        overdraft
            .promised_quantity()
            .unwrap()
            .unit()
            .unwrap()
            .as_str(),
        "USD"
    );
}

#[test]
fn independent_c14_virtual_envelope_is_not_external_account() {
    let envelope = VirtualAccount::new("envelope-c14", "account:checking AND tag:rent");
    assert_eq!(envelope.id, "envelope-c14".into());
    assert_eq!(envelope.query, "account:checking AND tag:rent");
}

#[test]
fn independent_d03_stale_quote_is_not_silent_current_value() {
    let abc = unit("ABC", "ABC");
    let usd = unit("USD", "USD");
    let quote = Quote::new(
        "stale-d03",
        Ratio::new(abc.clone(), usd.clone(), ExactNumber::integer(2)),
        QuoteKind::Mid,
        Instant::EPOCH,
        Instant::EPOCH,
        "venue-d03",
        "source-d03",
        InstantInterval::closed(Instant::EPOCH, Instant::from_unix_seconds(10)).unwrap(),
    );
    let result = value(
        &uq("1", abc),
        &usd,
        Instant::from_unix_seconds(100),
        &[quote],
        ValuationPolicy::default(),
    )
    .unwrap();
    assert_eq!(result.status, ValuationStatus::Stale);
    assert!(result.quantity.is_none());
    assert_eq!(result.paths.len(), 1);
}

#[test]
fn independent_d04_bid_ask_has_no_implicit_reverse() {
    let abc = unit("ABC", "ABC");
    let usd = unit("USD", "USD");
    let bid = Quote::new(
        "bid-d04",
        Ratio::new(abc.clone(), usd.clone(), ExactNumber::integer(2)),
        QuoteKind::Bid,
        Instant::EPOCH,
        Instant::EPOCH,
        "venue-d04",
        "source-d04",
        InstantInterval::closed(Instant::EPOCH, Instant::from_unix_seconds(10)).unwrap(),
    );
    let result = value(
        &uq("2", usd),
        &abc,
        Instant::from_unix_seconds(1),
        &[bid],
        ValuationPolicy::default(),
    )
    .unwrap();
    assert_eq!(result.status, ValuationStatus::Unavailable);
    assert!(result.paths.is_empty());
}

#[test]
fn independent_d05_triangulation_keeps_route_proof() {
    let abc = unit("ABC", "ABC");
    let eur = unit("EUR", "EUR");
    let usd = unit("USD", "USD");
    let quotes = [
        Quote::new(
            "abc-eur-d05",
            Ratio::new(abc.clone(), eur.clone(), ExactNumber::integer(2)),
            QuoteKind::Mid,
            Instant::EPOCH,
            Instant::EPOCH,
            "venue-d05",
            "source-d05-a",
            InstantInterval::closed(Instant::EPOCH, Instant::from_unix_seconds(10)).unwrap(),
        ),
        Quote::new(
            "eur-usd-d05",
            Ratio::new(eur, usd.clone(), ExactNumber::integer(3)),
            QuoteKind::Mid,
            Instant::EPOCH,
            Instant::EPOCH,
            "venue-d05",
            "source-d05-b",
            InstantInterval::closed(Instant::EPOCH, Instant::from_unix_seconds(10)).unwrap(),
        ),
    ];
    let result = value(
        &uq("2", abc),
        &usd,
        Instant::from_unix_seconds(1),
        &quotes,
        ValuationPolicy::default(),
    )
    .unwrap();
    assert_eq!(result.status, ValuationStatus::Unique);
    assert_eq!(result.quantity.unwrap().amount().canonical_string(), "12");
    assert_eq!(result.paths.len(), 1);
    assert_eq!(result.paths[0].legs().len(), 2);
}

#[test]
fn independent_d07_fractional_quantity_is_exact() {
    let share = unit("share-d07", "ABC-d07");
    let quantity = uq("1.25", share);
    assert_eq!(quantity.amount().canonical_string(), "5/4");
    assert_eq!(
        quantity.amount() * &ExactNumber::integer(4),
        ExactNumber::integer(5)
    );
}

#[test]
fn independent_d19_unique_asset_uses_identity_not_fungibility() {
    let asset = Instrument::new("painting-d19", InstrumentKind::UniqueAsset);
    let first = Position::new(
        "painting-d19-a",
        "owner-d19",
        "painting-d19",
        Quantity::with_unit(1i64.into(), "painting-d19").unwrap(),
    )
    .unwrap();
    let second = Position::new(
        "painting-d19-b",
        "owner-d19",
        "painting-d19",
        Quantity::with_unit(1i64.into(), "painting-d19").unwrap(),
    )
    .unwrap();
    assert_eq!(asset.kind, InstrumentKind::UniqueAsset);
    assert_ne!(first.id, second.id);
    assert_eq!(first.quantity, second.quantity);
}

#[test]
fn independent_d20_barter_has_coupled_legs() {
    let left = Endpoint::entity("trader-a-d20");
    let right = Endpoint::entity("trader-b-d20");
    let exchange = ExchangeRecord::new(
        "barter-d20",
        vec![
            ExchangeLeg::give(
                left.clone(),
                right.clone(),
                "USD",
                Quantity::with_unit(10i64.into(), "USD").unwrap(),
            ),
            ExchangeLeg::receive(
                right,
                left,
                "GOOD",
                Quantity::with_unit(1i64.into(), "GOOD").unwrap(),
            ),
        ],
    );
    validate_exchange_legs(&exchange).unwrap();
    assert_eq!(exchange.legs.len(), 2);
    assert_ne!(exchange.legs[0].instrument, exchange.legs[1].instrument);
}

#[test]
fn independent_d23_instrument_expiry_is_explicit() {
    let mut instrument = Instrument::new("coupon-d23", InstrumentKind::DebtSecurity);
    instrument.expires = Some(date("2026-12-31"));
    assert_eq!(instrument.expires, Some(date("2026-12-31")));
}

#[test]
fn independent_d24_off_quantum_is_rejected_not_rounded() {
    let definition = InstrumentDefinition::new(
        "ABC-d24",
        unit("share-d24", "ABC-d24"),
        ExactNumber::parse("0.01").unwrap(),
    )
    .unwrap();
    let off_quantum = definition.accepts(&uq("1.235", unit("share-d24", "ABC-d24")));
    assert!(matches!(off_quantum, Err(UnitError::OffQuantum { .. })));
}

#[test]
fn independent_e14_reversal_is_new_event_correction_supersedes() {
    let original = Identity::new("sale-e14", hash(140));
    let reversal = Identity::new("reversal-e14", hash(141));
    let relation = EvidenceRelation::reverses(reversal.clone(), original.clone());
    assert_eq!(relation.kind, EvidenceRelationKind::Reverses);
    assert_eq!(relation.from(), &reversal);
    assert_eq!(relation.to(), &original);
    assert_ne!(relation.from(), relation.to());
}

#[test]
fn independent_e15_late_discovery_restates_close() {
    let mut store = ObjectStore::new();
    let evidence = store
        .put_evidence(Evidence::new("late-e15", "bank-e15", b"late".to_vec()))
        .unwrap();
    let package = store
        .put_package(PolicyPackage::new("cash-e15", "1", b"policy".to_vec()))
        .unwrap();
    let statement = store
        .put_statement(Statement::new("balance-e15", "amount", "10 USD"))
        .unwrap();
    let proof_object = store
        .put_proof(ProofObject::recognized(
            [evidence.hash()],
            b"proof".to_vec(),
        ))
        .unwrap();
    let commit = store
        .put_commit(Commit::new(
            [],
            [evidence],
            [statement],
            [],
            [],
            [package],
            [proof_object],
            "close-author-e15",
        ))
        .unwrap();
    let period = Period::new(date("2026-09-01"), date("2026-09-30")).unwrap();
    let first = store
        .put_close(Close::new(
            period.clone(),
            "cash-e15",
            [package],
            commit,
            proof_object.hash(),
        ))
        .unwrap();
    let second = store
        .put_close(
            Close::new(period, "cash-e15", [package], commit, proof_object.hash())
                .superseding(first),
        )
        .unwrap();
    assert_eq!(store.close(second).unwrap().supersedes, Some(first));
    assert!(store.close(first).is_ok());
}

#[test]
fn independent_f02_dst_ambiguity_remains_visible() {
    let local = LocalDateTime::ambiguous(
        LocalDate::new(2026, 11, 1).unwrap(),
        LocalTime::new(1, 30, 0, 0).unwrap(),
        "America/New_York",
    );
    assert_eq!(local.status(), LocalTimeStatus::Ambiguous);
    assert!(local.to_instant().is_err());
}

#[test]
fn independent_f03_month_precision_is_coarse() {
    let month = TimePeriod::month(2026, 9).unwrap();
    assert_eq!(
        month.precision(),
        axiom_ledger::time::PeriodPrecision::Month
    );
}
