//! Deterministic evaluation and replayable authority for the v2 ledger.

use crate::expr::{Budget, Fault, eval, preview};
use crate::model::{Decision, Expr, Instruction, Model, Row, Rule, Type, TypedDocument};
use crate::{Date, Diagnostic, Id, Value};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// The authority stage represented by a claim.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Accepted,
    Candidate,
    Recognized,
}

/// One schema-checked fact and the claim identities it depends on.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Claim {
    pub id: Id,
    pub definition: Id,
    pub row: Row,
    pub phase: Phase,
    pub inputs: Vec<Id>,
    pub rule: Option<String>,
}

/// The complete result status for one subject/rule evaluation.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Proven(Id),
    Alternatives(Vec<Value>),
    Missing(Vec<String>),
    Conflict(Vec<String>),
    Incomplete(String),
}

/// A result, ambiguity, or failure retained in the replay record.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Finding {
    pub subject: String,
    pub rule: String,
    pub outcome: Outcome,
    pub context: Option<FailureContext>,
}

/// Bounded, deterministic context for an instruction that failed at runtime.
/// Source spans live on the Model's nonsemantic rule map, not in the proof.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FailureContext {
    pub instruction: usize,
    pub expression: String,
    pub bindings: BTreeMap<String, String>,
    pub binding_origins: BTreeMap<String, String>,
    pub target: Option<String>,
}

/// Canonically ordered claims and findings from one evaluation.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Evaluation {
    pub claims: Vec<Claim>,
    pub findings: Vec<Finding>,
    /// Shared witnesses for complete relation reads. Claim inputs reference
    /// these roots instead of repeating every row id in every derivation.
    pub read_sets: BTreeMap<Id, Vec<Id>>,
}

/// Replay certificate. Its public payload is intentionally treated as hostile
/// by [`verify`]; every field is compared with a full reconstruction.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Certificate {
    pub revision: Id,
    pub model: Id,
    pub world: Id,
    pub evaluation: Evaluation,
}

/// Immutable semantic world. Its only constructor is the checker.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct World {
    id: Id,
    model: Id,
    evaluation: Evaluation,
}

impl World {
    pub fn id(&self) -> &Id {
        &self.id
    }

    pub fn evaluation(&self) -> &Evaluation {
        &self.evaluation
    }

    pub fn model_id(&self) -> &Id {
        &self.model
    }
}

/// A pure interpretation of a world under one named book.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct View {
    pub id: Id,
    pub world: Id,
    pub book: String,
    pub period: Option<(Date, Date)>,
    pub evaluation: Evaluation,
}

impl View {
    pub fn id(&self) -> &Id {
        &self.id
    }
    pub fn world_id(&self) -> &Id {
        &self.world
    }
    pub fn book(&self) -> &str {
        &self.book
    }
    pub fn period(&self) -> Option<(&Date, &Date)> {
        self.period.as_ref().map(|(from, to)| (from, to))
    }
    pub fn evaluation(&self) -> &Evaluation {
        &self.evaluation
    }
}

#[derive(Serialize)]
struct ClaimIdentity<'a> {
    occurrence: &'a str,
    schema: &'a str,
    definition: &'a Id,
    phase: Phase,
    values: &'a BTreeMap<String, Value>,
    rule: Option<&'a str>,
    inputs: &'a [Id],
}

#[derive(Serialize)]
struct WorldIdentity<'a> {
    model: &'a Id,
    decisions: &'a [Decision],
    evaluation: &'a Evaluation,
}

#[derive(Serialize)]
struct ViewIdentity<'a> {
    world: &'a Id,
    book: &'a str,
    period: &'a Option<(Date, Date)>,
    evaluation: &'a Evaluation,
}

/// Elaborate, evaluate all world rules, and produce a fully replayable close.
pub fn check(
    source: &str,
    model: &Model,
    limit: usize,
) -> Result<(TypedDocument, World, Certificate), Vec<Diagnostic>> {
    let document = model.elaborate(source)?;
    let mut budget = Budget::new(limit);
    let evaluation = evaluate_world(&document, model, &mut budget)?;
    let world_id = world_id(model, &document, &evaluation);
    let world = World {
        id: world_id.clone(),
        model: model.id(),
        evaluation: evaluation.clone(),
    };
    let certificate = Certificate {
        revision: Id::digest("axiom.v2.raw-source.v1", source.as_bytes()),
        model: model.id(),
        world: world_id,
        evaluation,
    };
    Ok((document, world, certificate))
}

/// Re-run parsing, elaboration, and every rule, then compare the whole payload.
/// This rejects certificates with omitted rows/findings, extras, changed order,
/// forged roots, or modified outcomes.
pub fn verify(
    source: &str,
    model: &Model,
    certificate: &Certificate,
    limit: usize,
) -> Result<(), Vec<Diagnostic>> {
    let (_, _, expected) = check(source, model, limit)?;
    if &expected == certificate {
        Ok(())
    } else {
        Err(vec![diagnostic(
            1,
            "certificate does not match complete source and model replay",
        )])
    }
}

/// Evaluate one book against a checked world. The supplied document is
/// re-evaluated and its semantic root must match the immutable world first.
pub fn view(
    world: &World,
    model: &Model,
    book: &str,
    limit: usize,
) -> Result<View, Vec<Diagnostic>> {
    view_period(world, model, book, None, limit)
}

/// Evaluate a book and optionally retain only outputs in an inclusive date
/// range. The full relation read witnesses remain attached to the view.
pub fn view_period(
    world: &World,
    model: &Model,
    book: &str,
    period: Option<(&Date, &Date)>,
    limit: usize,
) -> Result<View, Vec<Diagnostic>> {
    if let Some((from, to)) = period {
        if from > to {
            return Err(vec![diagnostic(
                1,
                "period start must not be after its end",
            )]);
        }
    }
    let includes = model
        .books
        .get(book)
        .ok_or_else(|| vec![diagnostic(1, format!("unknown book `{book}`"))])?;
    if model.id() != world.model {
        return Err(vec![diagnostic(
            1,
            "model root does not match the supplied immutable world",
        )]);
    }
    let base = &world.evaluation;

    let included: BTreeSet<String> = includes.iter().cloned().collect();
    let rules: Vec<&Rule> = model
        .rules
        .iter()
        .filter(|rule| rule.book.as_deref() == Some(book))
        .collect();
    let relevant_rules: Vec<&Rule> = model
        .rules
        .iter()
        .filter(|rule| rule.book.is_none() || rule.book.as_deref() == Some(book))
        .collect();
    let relevant = relevant_schema_closure(&relevant_rules, &included);

    let mut state = EvalState::from_evaluation(base, model);
    let mut recognized = Vec::new();
    let mut book_findings = Vec::new();
    let mut book_read_roots = BTreeSet::new();
    let mut budget = Budget::new(limit);
    for rule in rules {
        if !relevant.contains(&rule.output) {
            continue;
        }
        let read_schemas = rule_read_schemas(rule);
        let read_roots = match state.register_read_sets(&read_schemas, &mut budget) {
            Ok(roots) => roots,
            Err(fault) => {
                for subject in state.rows_for(&rule.source) {
                    book_findings.push(Finding {
                        subject: subject.row.id,
                        rule: rule.name.clone(),
                        outcome: failure_outcome(RuleFailure::from(fault.clone())),
                        context: None,
                    });
                }
                state.mark_incomplete(&rule.output, &rule.name, "*read-set construction*");
                continue;
            }
        };
        book_read_roots.extend(read_roots.values().cloned());
        let relation_values = match state.relations_for(&read_schemas, &mut budget) {
            Ok(relations) => relations,
            Err(fault) => {
                for subject in state.rows_for(&rule.source) {
                    book_findings.push(Finding {
                        subject: subject.row.id,
                        rule: rule.name.clone(),
                        outcome: failure_outcome(RuleFailure::from(fault.clone())),
                        context: None,
                    });
                }
                state.mark_incomplete(&rule.output, &rule.name, "*relation materialization*");
                continue;
            }
        };
        let relations =
            match RuleRelations::new(&relation_values, rule_has_choice(rule), &mut budget) {
                Ok(relations) => relations,
                Err(fault) => {
                    for subject in state.rows_for(&rule.source) {
                        book_findings.push(Finding {
                            subject: subject.row.id,
                            rule: rule.name.clone(),
                            outcome: failure_outcome(RuleFailure::from(fault.clone())),
                            context: None,
                        });
                    }
                    state.mark_incomplete(
                        &rule.output,
                        &rule.name,
                        "*candidate index construction*",
                    );
                    continue;
                }
            };
        book_findings.extend(propagate_binding_failures(rule, &mut state));
        let row_indices = state.row_indices_for(&rule.source);
        for index in row_indices {
            let subject = state.claims[index].clone();
            let (claim, finding) = evaluate_rule(
                rule,
                &subject,
                model,
                &state,
                &relations,
                &read_roots,
                Phase::Recognized,
                &mut budget,
            );
            if let Some(claim) = claim {
                state.add_claim(claim.clone());
                if included.contains(&claim.row.schema) {
                    recognized.push(claim);
                }
            }
            if let Some(finding) = finding {
                state.mark_incomplete(&rule.output, &rule.name, &subject.row.id);
                book_findings.push(finding);
            }
        }
    }

    // World failures are surfaced only when their output relation can affect
    // this book. Unrelated source holes and unrelated rules remain local.
    let mut findings: Vec<Finding> = base
        .findings
        .iter()
        .filter(|finding| {
            model
                .rules
                .iter()
                .any(|rule| rule.name == finding.rule && relevant.contains(&rule.output))
        })
        .cloned()
        .collect();
    findings.extend(book_findings.into_iter().filter(|finding| {
        model
            .rules
            .iter()
            .any(|rule| rule.name == finding.rule && relevant.contains(&rule.output))
    }));

    let period = period.map(|(from, to)| (from.clone(), to.clone()));
    if let Some((from, to)) = &period {
        recognized = claims_in_period(recognized, from, to)?;
        findings = findings_in_period(findings, &state.claims, from, to)?;
    }

    recognized.sort_by(|left, right| left.id.cmp(&right.id));
    findings.sort_by(|left, right| {
        (&left.subject, &left.rule, &left.outcome).cmp(&(
            &right.subject,
            &right.rule,
            &right.outcome,
        ))
    });
    let evaluation = Evaluation {
        claims: recognized,
        findings,
        read_sets: state
            .read_sets
            .into_iter()
            .filter(|(root, _)| book_read_roots.contains(root))
            .collect(),
    };
    let id = Id::of(
        "axiom.v2.book-view.v1",
        &ViewIdentity {
            world: &world.id,
            book,
            period: &period,
            evaluation: &evaluation,
        },
    );
    Ok(View {
        id,
        world: world.id.clone(),
        book: book.to_owned(),
        period,
        evaluation,
    })
}

fn evaluate_world(
    document: &TypedDocument,
    model: &Model,
    budget: &mut Budget,
) -> Result<Evaluation, Vec<Diagnostic>> {
    let mut state = EvalState::default();

    let mut authored = document.rows.clone();
    authored.sort_by(|a, b| (&a.schema, &a.id).cmp(&(&b.schema, &b.id)));
    let decisions = sorted_decisions(&document.decisions);
    let decision_ids_by_occurrence = decision_ids_by_occurrence(&decisions);
    for row in authored {
        let schema = model.schema(&row.schema).ok_or_else(|| {
            vec![diagnostic(
                location_line(document, &row.id),
                format!("unknown schema `{}` in elaborated source", row.schema),
            )]
        })?;
        if !schema.authored {
            return Err(vec![diagnostic(
                location_line(document, &row.id),
                format!("derived schema `{}` appears as authored source", row.schema),
            )]);
        }
        validate_row(schema, &row)
            .map_err(|message| vec![diagnostic(location_line(document, &row.id), message)])?;
        let inputs = source_inputs(&row, &decision_ids_by_occurrence);
        let definition = model.definition_id(&row.schema);
        let claim = make_claim(row, Phase::Accepted, None, inputs, definition);
        state.add_claim(claim);
    }

    for rule in model.rules.iter().filter(|rule| rule.book.is_none()) {
        let read_schemas = rule_read_schemas(rule);
        let read_roots = match state.register_read_sets(&read_schemas, budget) {
            Ok(roots) => roots,
            Err(fault) => {
                for subject in state.rows_for(&rule.source) {
                    state.findings.push(Finding {
                        subject: subject.row.id,
                        rule: rule.name.clone(),
                        outcome: failure_outcome(RuleFailure::from(fault.clone())),
                        context: None,
                    });
                }
                state.mark_incomplete(&rule.output, &rule.name, "*read-set construction*");
                continue;
            }
        };
        let relation_values = match state.relations_for(&read_schemas, budget) {
            Ok(relations) => relations,
            Err(fault) => {
                for subject in state.rows_for(&rule.source) {
                    state.findings.push(Finding {
                        subject: subject.row.id,
                        rule: rule.name.clone(),
                        outcome: failure_outcome(RuleFailure::from(fault.clone())),
                        context: None,
                    });
                }
                state.mark_incomplete(&rule.output, &rule.name, "*relation materialization*");
                continue;
            }
        };
        let relations = match RuleRelations::new(&relation_values, rule_has_choice(rule), budget) {
            Ok(relations) => relations,
            Err(fault) => {
                for subject in state.rows_for(&rule.source) {
                    state.findings.push(Finding {
                        subject: subject.row.id,
                        rule: rule.name.clone(),
                        outcome: failure_outcome(RuleFailure::from(fault.clone())),
                        context: None,
                    });
                }
                state.mark_incomplete(&rule.output, &rule.name, "*candidate index construction*");
                continue;
            }
        };
        let propagated = propagate_binding_failures(rule, &mut state);
        state.findings.extend(propagated);
        let row_indices = state.row_indices_for(&rule.source);
        for index in row_indices {
            let subject = state.claims[index].clone();
            let (claim, finding) = evaluate_rule(
                rule,
                &subject,
                model,
                &state,
                &relations,
                &read_roots,
                Phase::Candidate,
                budget,
            );
            if let Some(claim) = claim {
                state.add_claim(claim);
            }
            if let Some(finding) = finding {
                state.mark_incomplete(&rule.output, &rule.name, &subject.row.id);
                state.findings.push(finding);
            }
        }
    }
    state.finish()
}

#[derive(Default)]
struct EvalState {
    claims: Vec<Claim>,
    by_schema: BTreeMap<String, BTreeMap<String, usize>>,
    relation_values: BTreeMap<String, BTreeMap<String, Value>>,
    findings: Vec<Finding>,
    incomplete: BTreeMap<String, IncompleteStatus>,
    read_sets: BTreeMap<Id, Vec<Id>>,
    duplicate_row_key: bool,
}

/// A rule's immutable relation snapshot and, only for rules with `choose`, a
/// borrowed membership index proving candidate records came from that exact
/// snapshot. The index is built once per rule, never once per subject.
struct RuleRelations<'a> {
    values: &'a BTreeMap<String, Vec<Value>>,
    candidates: Option<BTreeMap<&'a Value, BTreeSet<&'a str>>>,
    ids: Option<BTreeSet<&'a str>>,
}

impl<'a> RuleRelations<'a> {
    fn new(
        values: &'a BTreeMap<String, Vec<Value>>,
        with_candidates: bool,
        budget: &mut Budget,
    ) -> Result<Self, Fault> {
        let (candidates, ids) = if with_candidates {
            let mut candidates: BTreeMap<&Value, BTreeSet<&str>> = BTreeMap::new();
            let mut ids = BTreeSet::new();
            for (schema, rows) in values {
                for value in rows {
                    budget.tick()?;
                    candidates.entry(value).or_default().insert(schema.as_str());
                    if let Value::Record(fields) = value {
                        if let Some(Value::Ref(id)) = fields.get("id") {
                            ids.insert(id.as_str());
                        }
                    }
                }
            }
            (Some(candidates), Some(ids))
        } else {
            (None, None)
        };
        Ok(Self {
            values,
            candidates,
            ids,
        })
    }

    fn has_id(&self, id: &str) -> bool {
        self.ids.as_ref().is_some_and(|ids| ids.contains(id))
    }
}

fn rule_has_choice(rule: &Rule) -> bool {
    rule.steps
        .iter()
        .any(|instruction| matches!(instruction, Instruction::Choose(_, _, _)))
}

fn rule_has_choice_after(rule: &Rule, start: usize) -> bool {
    rule.steps
        .iter()
        .skip(start)
        .any(|instruction| matches!(instruction, Instruction::Choose(_, _, _)))
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct IncompleteCause {
    rule: String,
    subject: String,
}

#[derive(Default)]
struct IncompleteStatus {
    causes: BTreeSet<IncompleteCause>,
    summary: String,
}

impl EvalState {
    fn from_evaluation(evaluation: &Evaluation, model: &Model) -> Self {
        let mut state = Self {
            findings: evaluation.findings.clone(),
            read_sets: evaluation.read_sets.clone(),
            ..Self::default()
        };
        for claim in &evaluation.claims {
            state.add_claim(claim.clone());
        }
        for finding in &evaluation.findings {
            if matches!(finding.outcome, Outcome::Proven(_)) {
                continue;
            }
            if let Some(rule) = model.rules.iter().find(|rule| rule.name == finding.rule) {
                state.mark_incomplete(&rule.output, &rule.name, &finding.subject);
            }
        }
        state
    }

    fn add_claim(&mut self, claim: Claim) {
        let index = self.claims.len();
        self.duplicate_row_key |= self
            .by_schema
            .entry(claim.row.schema.clone())
            .or_default()
            .insert(claim.row.id.clone(), index)
            .is_some();
        self.relation_values
            .entry(claim.row.schema.clone())
            .or_default()
            .insert(claim.row.id.clone(), row_value(&claim.row));
        self.claims.push(claim);
    }

    fn mark_incomplete(&mut self, schema: &str, rule: &str, subject: &str) {
        let status = self.incomplete.entry(schema.to_owned()).or_default();
        let inserted = status.causes.insert(IncompleteCause {
            rule: rule.to_owned(),
            subject: subject.to_owned(),
        });
        if inserted {
            let total = status.causes.len();
            let sample = status
                .causes
                .iter()
                .take(4)
                .map(|cause| format!("{} on {}", cause.rule, cause.subject))
                .collect::<Vec<_>>()
                .join(", ");
            status.summary = if total > 4 {
                format!("{sample}, and {} more", total - 4)
            } else {
                sample
            };
        }
    }

    fn rows_for(&self, schema: &str) -> Vec<Claim> {
        self.row_indices_for(schema)
            .into_iter()
            .map(|index| self.claims[index].clone())
            .collect()
    }

    fn row_indices_for(&self, schema: &str) -> Vec<usize> {
        self.by_schema
            .get(schema)
            .map(|by_id| by_id.values().copied().collect())
            .unwrap_or_default()
    }

    fn relations_for(
        &self,
        schemas: &BTreeSet<String>,
        budget: &mut Budget,
    ) -> Result<BTreeMap<String, Vec<Value>>, Fault> {
        let mut relations = BTreeMap::new();
        for schema in schemas {
            for value in self
                .relation_values
                .get(schema)
                .into_iter()
                .flat_map(|by_id| by_id.values())
            {
                charge_value_tree(value, budget)?;
            }
            let values: Vec<Value> = self
                .relation_values
                .get(schema)
                .into_iter()
                .flat_map(|by_id| by_id.values())
                .cloned()
                .collect();
            relations.insert(schema.clone(), values);
        }
        Ok(relations)
    }

    fn register_read_sets(
        &mut self,
        schemas: &BTreeSet<String>,
        budget: &mut Budget,
    ) -> Result<BTreeMap<String, Id>, Fault> {
        #[derive(Serialize)]
        struct RelationWitness<'a> {
            schema: &'a str,
            claims: &'a [Id],
        }
        let mut roots = BTreeMap::new();
        for schema in schemas {
            let indices = self
                .by_schema
                .get(schema)
                .map(|by_id| by_id.values().copied().collect::<Vec<_>>())
                .unwrap_or_default();
            let mut members = Vec::with_capacity(indices.len());
            for index in indices {
                budget.tick()?;
                members.push(self.claims[index].id.clone());
            }
            members.sort();
            members.dedup();
            let root = Id::of(
                "axiom.v2.read-set.v1",
                &RelationWitness {
                    schema,
                    claims: &members,
                },
            );
            if let Some(existing) = self.read_sets.get(&root) {
                if existing != &members {
                    return Err(Fault::Conflict("read-set identity collision".into()));
                }
            } else {
                self.read_sets.insert(root.clone(), members);
            }
            roots.insert(schema.clone(), root);
        }
        Ok(roots)
    }

    fn finish(mut self) -> Result<Evaluation, Vec<Diagnostic>> {
        self.claims.sort_by(|a, b| a.id.cmp(&b.id));
        self.findings.sort_by(|a, b| {
            (&a.subject, &a.rule, &a.outcome).cmp(&(&b.subject, &b.rule, &b.outcome))
        });
        // A content-address collision or duplicate claim id would make
        // coverage ambiguous, so reject it rather than silently deduplicate.
        if self.duplicate_row_key || self.claims.windows(2).any(|pair| pair[0].id == pair[1].id) {
            return Err(vec![diagnostic(1, "duplicate claim identity in replay")]);
        }
        Ok(Evaluation {
            claims: self.claims,
            findings: self.findings,
            read_sets: self.read_sets,
        })
    }
}

enum RuleFailure {
    Fault(Fault, Option<FailureContext>),
    Alternatives(Vec<Value>, Option<FailureContext>),
}

impl From<Fault> for RuleFailure {
    fn from(fault: Fault) -> Self {
        Self::Fault(fault, None)
    }
}

impl RuleFailure {
    fn at_instruction(
        fault: Fault,
        instruction: usize,
        expression: &Expr,
        binding: &str,
        environment: &BTreeMap<String, Value>,
    ) -> Self {
        let context = failure_context(instruction, expression, binding, environment);
        Self::Fault(fault, Some(context))
    }

    fn context(&self) -> Option<&FailureContext> {
        match self {
            Self::Fault(_, context) => context.as_ref(),
            Self::Alternatives(_, context) => context.as_ref(),
        }
    }
}

fn evaluate_rule(
    rule: &Rule,
    subject: &Claim,
    model: &Model,
    state: &EvalState,
    relations: &RuleRelations<'_>,
    read_roots: &BTreeMap<String, Id>,
    phase: Phase,
    budget: &mut Budget,
) -> (Option<Claim>, Option<Finding>) {
    let mut inputs = vec![subject.id.clone()];
    inputs.extend(read_roots.values().cloned());
    canonicalize_ids(&mut inputs);

    let result = budget
        .tick()
        .map_err(RuleFailure::from)
        .and_then(|()| {
            precheck_incomplete_relations(rule, state).map_or(Ok(()), |fault| Err(fault.into()))
        })
        .and_then(|()| {
            let mut environment = BTreeMap::new();
            environment.insert(rule.binding.clone(), row_value(&subject.row));
            evaluate_rule_body(
                rule,
                subject,
                model,
                state,
                relations,
                &mut environment,
                budget,
            )
        });

    match result {
        Ok(mut row) => {
            row.id = derived_occurrence_id(rule, subject);
            let claim = make_claim(
                row,
                phase,
                Some(rule.name.clone()),
                inputs,
                model.definition_id(&rule.output),
            );
            (Some(claim), None)
        }
        Err(failure) => {
            let context = failure.context().cloned();
            (
                None,
                Some(Finding {
                    subject: subject.row.id.clone(),
                    rule: rule.name.clone(),
                    outcome: failure_outcome(failure),
                    context,
                }),
            )
        }
    }
}

fn evaluate_rule_body(
    rule: &Rule,
    subject: &Claim,
    model: &Model,
    state: &EvalState,
    relations: &RuleRelations<'_>,
    environment: &mut BTreeMap<String, Value>,
    budget: &mut Budget,
) -> Result<Row, RuleFailure> {
    evaluate_rule_steps(
        rule,
        subject,
        model,
        state,
        relations,
        environment,
        budget,
        0,
    )
}

fn evaluate_rule_steps(
    rule: &Rule,
    subject: &Claim,
    model: &Model,
    state: &EvalState,
    relations: &RuleRelations<'_>,
    environment: &mut BTreeMap<String, Value>,
    budget: &mut Budget,
    start: usize,
) -> Result<Row, RuleFailure> {
    for (index, instruction) in rule.steps.iter().enumerate().skip(start) {
        match instruction {
            Instruction::Let(name, expression) => {
                let value = eval(expression, environment, relations.values, &mut *budget).map_err(
                    |fault| {
                        RuleFailure::at_instruction(
                            fault,
                            index,
                            expression,
                            &rule.binding,
                            environment,
                        )
                    },
                )?;
                environment.insert(name.clone(), value);
            }
            Instruction::Choose(name, selector, candidates) => {
                let candidate_value = eval(candidates, environment, relations.values, &mut *budget)
                    .map_err(|fault| {
                        RuleFailure::at_instruction(
                            fault,
                            index,
                            candidates,
                            &rule.binding,
                            environment,
                        )
                    })?;
                let selector_value = eval(selector, environment, relations.values, &mut *budget)
                    .map_err(|fault| {
                        RuleFailure::at_instruction(
                            fault,
                            index,
                            selector,
                            &rule.binding,
                            environment,
                        )
                    })?;
                let target_schema =
                    choice_target_schema(rule, model, selector).map_err(|fault| {
                        RuleFailure::at_instruction(
                            fault,
                            index,
                            selector,
                            &rule.binding,
                            environment,
                        )
                    })?;
                let candidate_rows =
                    candidate_rows(candidate_value, relations, target_schema, budget).map_err(
                        |fault| {
                            RuleFailure::at_instruction(
                                fault,
                                index,
                                candidates,
                                &rule.binding,
                                environment,
                            )
                        },
                    )?;
                match selector_value {
                    Value::Hole(_) => {
                        if candidate_rows.is_empty() {
                            return Err(RuleFailure::at_instruction(
                                Fault::Missing("no candidates are available".into()),
                                index,
                                candidates,
                                &rule.binding,
                                environment,
                            ));
                        }
                        let mut viable = Vec::new();
                        let mut missing = Vec::new();
                        for candidate in candidate_rows {
                            budget.tick()?;
                            let id = row_value_id(&candidate);
                            for value in environment.values() {
                                charge_value_tree(value, budget)?;
                            }
                            for value in relations.values.values().flatten() {
                                charge_value_tree(value, budget)?;
                            }
                            let mut branch = environment.clone();
                            let mut branch_relations = relations.values.clone();
                            apply_choice_to_selector(
                                selector,
                                &rule.binding,
                                &rule.source,
                                &subject.row.id,
                                &id,
                                &mut branch,
                                &mut branch_relations,
                            )
                            .map_err(|fault| {
                                RuleFailure::at_instruction(
                                    fault,
                                    index,
                                    selector,
                                    &rule.binding,
                                    environment,
                                )
                            })?;
                            branch.insert(name.clone(), candidate);
                            let branch_needs_candidates = rule_has_choice_after(rule, index + 1);
                            let branch_relations = RuleRelations::new(
                                &branch_relations,
                                branch_needs_candidates,
                                budget,
                            )?;
                            match evaluate_rule_steps(
                                rule,
                                subject,
                                model,
                                state,
                                &branch_relations,
                                &mut branch,
                                budget,
                                index + 1,
                            ) {
                                Ok(_) | Err(RuleFailure::Alternatives(_, _)) => {
                                    viable.push(Value::Ref(id));
                                }
                                Err(RuleFailure::Fault(Fault::Conflict(_), _)) => {}
                                Err(RuleFailure::Fault(Fault::Missing(reason), _)) => {
                                    missing.push(format!("{id}: {reason}"));
                                }
                                Err(error @ RuleFailure::Fault(Fault::Incomplete(_), _)) => {
                                    return Err(error);
                                }
                            }
                        }
                        viable.sort();
                        viable.dedup();
                        if !missing.is_empty() {
                            return Err(RuleFailure::at_instruction(
                                Fault::Missing(format!(
                                    "candidate eligibility is unresolved ({})",
                                    missing.join("; ")
                                )),
                                index,
                                selector,
                                &rule.binding,
                                environment,
                            ));
                        }
                        if viable.is_empty() {
                            return Err(RuleFailure::at_instruction(
                                Fault::Conflict(
                                    "no choice candidate satisfies the remaining rule".into(),
                                ),
                                index,
                                selector,
                                &rule.binding,
                                environment,
                            ));
                        }
                        let mut context =
                            failure_context(index, selector, &rule.binding, environment);
                        context.target = choice_target(rule, subject, selector);
                        return Err(RuleFailure::Alternatives(viable, Some(context)));
                    }
                    Value::Ref(target) => {
                        let selected = candidate_rows
                            .iter()
                            .find(|row| row_value_id(row) == target)
                            .cloned();
                        if let Some(selected) = selected {
                            environment.insert(name.clone(), selected);
                        } else if relations.has_id(&target) {
                            return Err(RuleFailure::at_instruction(
                                Fault::Conflict(format!(
                                    "explicit target `{target}` is not among the candidates"
                                )),
                                index,
                                selector,
                                &rule.binding,
                                environment,
                            ));
                        } else {
                            return Err(RuleFailure::at_instruction(
                                Fault::Missing(format!(
                                    "selected target `{target}` does not exist"
                                )),
                                index,
                                selector,
                                &rule.binding,
                                environment,
                            ));
                        }
                    }
                    value => {
                        return Err(RuleFailure::at_instruction(
                            Fault::Conflict(format!(
                                "choice selector must be an explicit reference or typed hole, got {value}"
                            )),
                            index,
                            selector,
                            &rule.binding,
                            environment,
                        ));
                    }
                }
            }
            Instruction::Require(expression) => {
                let result = eval(expression, environment, relations.values, &mut *budget)
                    .map_err(|fault| {
                        RuleFailure::at_instruction(
                            fault,
                            index,
                            expression,
                            &rule.binding,
                            environment,
                        )
                    })?;
                match result {
                    Value::Bool(true) => {}
                    Value::Bool(false) => {
                        return Err(RuleFailure::at_instruction(
                            Fault::Conflict(format!(
                                "requirement evaluated to false for `{}`",
                                subject.row.id
                            )),
                            index,
                            expression,
                            &rule.binding,
                            environment,
                        ));
                    }
                    Value::Hole(name) => {
                        return Err(RuleFailure::at_instruction(
                            Fault::Missing(format!(
                                "requirement depends on unresolved hole `{name}`"
                            )),
                            index,
                            expression,
                            &rule.binding,
                            environment,
                        ));
                    }
                    value => {
                        return Err(RuleFailure::at_instruction(
                            Fault::Conflict(format!("requirement must be boolean, got {value}")),
                            index,
                            expression,
                            &rule.binding,
                            environment,
                        ));
                    }
                }
            }
        }
    }

    let schema = model
        .schema(&rule.output)
        .ok_or_else(|| Fault::Conflict(format!("unknown output schema `{}`", rule.output)))?;
    let mut fields = BTreeMap::new();
    for (name, field_type) in &schema.fields {
        let expression = rule.fields.get(name).ok_or_else(|| {
            Fault::Conflict(format!(
                "rule `{}` does not supply required output field `{name}`",
                rule.name
            ))
        })?;
        let value = eval(expression, environment, relations.values, &mut *budget)?;
        if contains_hole(&value) {
            return Err(Fault::Missing(format!(
                "rule `{}` output field `{name}` still contains an unresolved hole",
                rule.name
            ))
            .into());
        }
        value.validate().map_err(|message| {
            Fault::Conflict(format!(
                "rule `{}` produced invalid `{name}`: {message}",
                rule.name
            ))
        })?;
        validate_derived_value(field_type, &value, state, budget).map_err(|fault| {
            RuleFailure::from(match fault {
                Fault::Conflict(message) => Fault::Conflict(format!(
                    "rule `{}` produced invalid `{name}`: {message}",
                    rule.name
                )),
                other => other,
            })
        })?;
        fields.insert(name.clone(), value);
    }
    if let Some(extra) = rule
        .fields
        .keys()
        .find(|name| !schema.fields.contains_key(*name))
    {
        return Err(Fault::Conflict(format!(
            "rule `{}` supplies unknown output field `{extra}`",
            rule.name
        ))
        .into());
    }
    Ok(Row {
        id: String::new(),
        schema: rule.output.clone(),
        fields,
    })
}

fn candidate_rows(
    value: Value,
    relations: &RuleRelations<'_>,
    target_schema: &str,
    budget: &mut Budget,
) -> Result<Vec<Value>, Fault> {
    match value {
        Value::List(values) => {
            let mut rows = Vec::with_capacity(values.len());
            for value in values {
                budget.tick()?;
                if row_value_id(&value).is_empty() {
                    return Err(Fault::Conflict(
                        "choice candidates must be records with reference `id` fields".into(),
                    ));
                }
                if !relations
                    .candidates
                    .as_ref()
                    .and_then(|candidates| candidates.get(&value))
                    .is_some_and(|schemas| schemas.contains(target_schema))
                {
                    return Err(Fault::Conflict(format!(
                        "choice candidate `{}` is not a row in the required `{target_schema}` relation",
                        row_value_id(&value)
                    )));
                }
                rows.push(value);
            }
            rows.sort_by_key(row_value_id);
            Ok(rows)
        }
        Value::Hole(name) => Err(Fault::Missing(format!(
            "choice candidate list is unresolved at `{name}`"
        ))),
        other => Err(Fault::Conflict(format!(
            "choice candidates must be a list, got {other}"
        ))),
    }
}

fn choice_target_schema<'a>(
    rule: &Rule,
    model: &'a Model,
    selector: &Expr,
) -> Result<&'a str, Fault> {
    let Expr::Name(path) = selector else {
        return Err(Fault::Conflict(
            "choice selector must be a source reference field".into(),
        ));
    };
    let mut parts = path.split('.');
    if parts.next() != Some(rule.binding.as_str()) {
        return Err(Fault::Conflict(
            "choice selector must belong to the current source binding".into(),
        ));
    }
    let Some(mut field_path) = parts.next() else {
        return Err(Fault::Conflict(
            "choice selector must identify a typed reference field".into(),
        ));
    };
    let schema = model
        .schema(&rule.source)
        .ok_or_else(|| Fault::Conflict(format!("unknown source schema `{}`", rule.source)))?;
    let mut ty = schema.fields.get(field_path).ok_or_else(|| {
        Fault::Conflict(format!(
            "choice selector field `{field_path}` is not declared on `{}`",
            rule.source
        ))
    })?;
    for part in parts {
        let Type::Record(fields) = ty else {
            return Err(Fault::Conflict(
                "choice selector path does not have a declared record type".into(),
            ));
        };
        field_path = part;
        ty = fields.get(field_path).ok_or_else(|| {
            Fault::Conflict(format!(
                "choice selector field `{field_path}` is not declared"
            ))
        })?;
    }
    match ty {
        Type::Ref(target) => Ok(target),
        _ => Err(Fault::Conflict(
            "choice selector must have a declared reference target".into(),
        )),
    }
}

fn apply_choice_to_selector(
    selector: &Expr,
    binding: &str,
    source_schema: &str,
    subject_id: &str,
    selected_id: &str,
    environment: &mut BTreeMap<String, Value>,
    relations: &mut BTreeMap<String, Vec<Value>>,
) -> Result<(), Fault> {
    let Expr::Name(path) = selector else {
        return Err(Fault::Missing(
            "choice hole is not a directly addressable source field".into(),
        ));
    };
    let mut parts = path.split('.');
    if parts.next() != Some(binding) {
        return Err(Fault::Missing(
            "choice hole does not belong to the current source binding".into(),
        ));
    }
    let fields: Vec<&str> = parts.collect();
    if fields.is_empty() {
        return Err(Fault::Missing(
            "choice selector is not a source field".into(),
        ));
    }
    let replacement = Value::Ref(selected_id.to_owned());
    let source = environment
        .get_mut(binding)
        .ok_or_else(|| Fault::Missing(format!("binding `{binding}` is unavailable")))?;
    if !set_record_path(source, &fields, replacement.clone()) {
        return Err(Fault::Missing(
            "choice source field cannot be resolved".into(),
        ));
    }
    let rows = relations.get_mut(source_schema).ok_or_else(|| {
        Fault::Missing(format!(
            "choice source relation `{source_schema}` is unavailable"
        ))
    })?;
    let row = rows
        .iter_mut()
        .find(|row| row_value_id(row) == subject_id)
        .ok_or_else(|| Fault::Missing(format!("choice subject `{subject_id}` is unavailable")))?;
    if !set_record_path(row, &fields, replacement) {
        return Err(Fault::Missing(
            "choice relation field cannot be resolved".into(),
        ));
    }
    Ok(())
}

fn set_record_path(value: &mut Value, fields: &[&str], replacement: Value) -> bool {
    let Some((field, rest)) = fields.split_first() else {
        return false;
    };
    let Value::Record(record) = value else {
        return false;
    };
    let Some(current) = record.get_mut(*field) else {
        return false;
    };
    if rest.is_empty() {
        *current = replacement;
        true
    } else {
        set_record_path(current, rest, replacement)
    }
}

fn precheck_incomplete_relations(rule: &Rule, state: &EvalState) -> Option<Fault> {
    for schema in referenced_relations(rule) {
        if let Some(status) = state.incomplete.get(&schema) {
            return Some(Fault::Missing(format!(
                "relation `{schema}` is incomplete after {} rule failure(s): {}",
                status.causes.len(),
                status.summary
            )));
        }
    }
    None
}

fn referenced_relations(rule: &Rule) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for instruction in &rule.steps {
        match instruction {
            Instruction::Let(_, expression) | Instruction::Require(expression) => {
                collect_rows_calls(expression, &mut found)
            }
            Instruction::Choose(_, selector, candidates) => {
                collect_rows_calls(selector, &mut found);
                collect_rows_calls(candidates, &mut found);
            }
        }
    }
    for expression in rule.fields.values() {
        collect_rows_calls(expression, &mut found);
    }
    found
}

fn rule_read_schemas(rule: &Rule) -> BTreeSet<String> {
    let mut schemas = referenced_relations(rule);
    schemas.insert(rule.source.clone());
    schemas
}

fn propagate_binding_failures(rule: &Rule, state: &mut EvalState) -> Vec<Finding> {
    let causes = state
        .incomplete
        .get(&rule.source)
        .map(|status| status.causes.clone())
        .unwrap_or_default();
    let mut findings = Vec::with_capacity(causes.len());
    for cause in causes {
        state.mark_incomplete(&rule.output, &rule.name, &cause.subject);
        findings.push(Finding {
            subject: cause.subject.clone(),
            rule: rule.name.clone(),
            outcome: Outcome::Missing(vec![format!(
                "binding relation `{}` is incomplete after rule `{}` failed on `{}`",
                rule.source, cause.rule, cause.subject
            )]),
            context: None,
        });
    }
    findings
}

fn collect_rows_calls(expression: &Expr, found: &mut BTreeSet<String>) {
    match expression {
        Expr::Call(name, arguments) => {
            if name == "rows" {
                if let Some(schema) = arguments.first().and_then(expr_atom) {
                    found.insert(schema.to_owned());
                }
            }
            for argument in arguments {
                collect_rows_calls(argument, found);
            }
        }
        Expr::Literal(_) | Expr::Name(_) => {}
    }
}

fn expr_atom(expression: &Expr) -> Option<&str> {
    match expression {
        Expr::Name(name) => Some(name),
        Expr::Literal(Value::Text(name)) => Some(name),
        Expr::Literal(Value::Ref(name)) => Some(name),
        _ => None,
    }
}

fn relevant_schema_closure(rules: &[&Rule], included: &BTreeSet<String>) -> BTreeSet<String> {
    let mut relevant = included.clone();
    loop {
        let previous = relevant.len();
        for rule in rules {
            if relevant.contains(&rule.output) {
                relevant.insert(rule.source.clone());
                relevant.extend(referenced_relations(rule));
            }
        }
        if relevant.len() == previous {
            break;
        }
    }
    relevant
}

fn claims_in_period(
    claims: Vec<Claim>,
    from: &Date,
    to: &Date,
) -> Result<Vec<Claim>, Vec<Diagnostic>> {
    let mut retained = Vec::new();
    for claim in claims {
        let date = row_date(&claim.row).map_err(|message| vec![diagnostic(1, message)])?;
        if from <= date && date <= to {
            retained.push(claim);
        }
    }
    Ok(retained)
}

fn findings_in_period(
    findings: Vec<Finding>,
    claims: &[Claim],
    from: &Date,
    to: &Date,
) -> Result<Vec<Finding>, Vec<Diagnostic>> {
    let mut retained = Vec::new();
    for finding in findings {
        let subject = claims
            .iter()
            .filter(|claim| claim.row.id == finding.subject)
            .min_by_key(|claim| claim.phase);
        let subject = subject.ok_or_else(|| {
            vec![diagnostic(
                1,
                format!("finding `{}` has no date-bearing subject row", finding.rule),
            )]
        })?;
        let date = row_date(&subject.row).map_err(|message| vec![diagnostic(1, message)])?;
        if from <= date && date <= to {
            retained.push(finding);
        }
    }
    Ok(retained)
}

fn row_date(row: &Row) -> Result<&Date, String> {
    match row.fields.get("date") {
        Some(Value::Date(date)) => Ok(date),
        Some(Value::Hole(name)) => Err(format!(
            "row `{}` has unresolved date hole `{name}` in a period view",
            row.id
        )),
        Some(_) => Err(format!("row `{}` has a non-date `date` field", row.id)),
        None => Err(format!(
            "row `{}` has no `date` field for a period view",
            row.id
        )),
    }
}

fn row_value(row: &Row) -> Value {
    let mut fields = row.fields.clone();
    fields.insert("id".into(), Value::Ref(row.id.clone()));
    Value::Record(fields)
}

fn row_value_id(value: &Value) -> String {
    match value {
        Value::Record(fields) => match fields.get("id") {
            Some(Value::Ref(id)) => id.clone(),
            _ => String::new(),
        },
        _ => String::new(),
    }
}

fn charge_value_tree(value: &Value, budget: &mut Budget) -> Result<(), Fault> {
    fn charge_bytes(bytes: usize, budget: &mut Budget) -> Result<(), Fault> {
        for _ in 0..bytes.div_ceil(256) {
            budget.tick()?;
        }
        Ok(())
    }
    budget.tick()?;
    match value {
        Value::Text(text) | Value::Ref(text) | Value::Hole(text) => {
            charge_bytes(text.len(), budget)
        }
        Value::Quantity(_, unit) => charge_bytes(unit.len(), budget),
        Value::List(values) => {
            for value in values {
                charge_value_tree(value, budget)?;
            }
            Ok(())
        }
        Value::Record(fields) => {
            for (name, value) in fields {
                charge_bytes(name.len(), budget)?;
                charge_value_tree(value, budget)?;
            }
            Ok(())
        }
        Value::Number(_) | Value::Date(_) | Value::Bool(_) => Ok(()),
    }
}

fn validate_row(schema: &crate::model::Schema, row: &Row) -> Result<(), String> {
    for (name, field_type) in &schema.fields {
        let value = row
            .fields
            .get(name)
            .ok_or_else(|| format!("row `{}` lacks field `{name}`", row.id))?;
        field_type
            .validate(value)
            .map_err(|message| format!("row `{}` field `{name}`: {message}", row.id))?;
    }
    if let Some(extra) = row
        .fields
        .keys()
        .find(|name| !schema.fields.contains_key(*name))
    {
        return Err(format!("row `{}` has unknown field `{extra}`", row.id));
    }
    Ok(())
}

fn contains_hole(value: &Value) -> bool {
    match value {
        Value::Hole(_) => true,
        Value::List(values) => values.iter().any(contains_hole),
        Value::Record(fields) => fields.values().any(contains_hole),
        _ => false,
    }
}

fn validate_derived_value(
    ty: &Type,
    value: &Value,
    state: &EvalState,
    budget: &mut Budget,
) -> Result<(), Fault> {
    budget.tick()?;
    validate_shallow_type(ty, value).map_err(Fault::Conflict)?;
    match (ty, value) {
        (Type::Any, Value::Ref(target)) => {
            if state
                .by_schema
                .values()
                .any(|rows| rows.contains_key(target))
            {
                Ok(())
            } else {
                Err(Fault::Missing(format!(
                    "reference target `{target}` does not exist"
                )))
            }
        }
        (Type::Any, Value::List(values)) => {
            for value in values {
                validate_derived_value(ty, value, state, budget)?;
            }
            Ok(())
        }
        (Type::Any, Value::Record(fields)) => {
            for value in fields.values() {
                validate_derived_value(ty, value, state, budget)?;
            }
            Ok(())
        }
        (Type::Any, _) => Ok(()),
        (Type::Ref(expected), Value::Ref(target)) => {
            if state
                .by_schema
                .get(expected)
                .is_some_and(|rows| rows.contains_key(target))
            {
                return Ok(());
            }
            if state
                .by_schema
                .values()
                .any(|rows| rows.contains_key(target))
            {
                return Err(Fault::Conflict(format!(
                    "reference `{target}` does not name a row of schema `{expected}`"
                )));
            }
            Err(Fault::Missing(format!(
                "reference target `{target}` does not exist"
            )))
        }
        (Type::List(item), Value::List(values)) => {
            for value in values {
                validate_derived_value(item, value, state, budget)?;
            }
            Ok(())
        }
        (Type::Record(types), Value::Record(values)) => {
            for (name, field_type) in types {
                let field = values
                    .get(name)
                    .ok_or_else(|| Fault::Conflict(format!("missing record field `{name}`")))?;
                validate_derived_value(field_type, field, state, budget)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn validate_shallow_type(ty: &Type, value: &Value) -> Result<(), String> {
    let valid = match (ty, value) {
        (Type::Any, _) => true,
        (Type::Number, Value::Number(_))
        | (Type::Quantity, Value::Quantity(_, _))
        | (Type::Date, Value::Date(_))
        | (Type::Text, Value::Text(_))
        | (Type::Bool, Value::Bool(_))
        | (Type::Ref(_), Value::Ref(_))
        | (Type::List(_), Value::List(_)) => true,
        (Type::Record(types), Value::Record(fields)) => {
            types.len() == fields.len() && types.keys().all(|key| fields.contains_key(key))
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(format!("expected {ty}, found {value}"))
    }
}

fn make_claim(
    row: Row,
    phase: Phase,
    rule: Option<String>,
    inputs: Vec<Id>,
    definition: Id,
) -> Claim {
    let id = claim_id(&row, &definition, phase, rule.as_deref(), &inputs);
    Claim {
        id,
        definition,
        row,
        phase,
        inputs,
        rule,
    }
}

fn claim_id(row: &Row, definition: &Id, phase: Phase, rule: Option<&str>, inputs: &[Id]) -> Id {
    Id::of(
        "axiom.v2.claim.v1",
        &ClaimIdentity {
            occurrence: &row.id,
            schema: &row.schema,
            definition,
            phase,
            values: &row.fields,
            rule,
            inputs,
        },
    )
}

fn derived_occurrence_id(rule: &Rule, subject: &Claim) -> String {
    #[derive(Serialize)]
    struct Occurrence<'a> {
        rule: &'a str,
        binding_schema: &'a str,
        binding_occurrence: &'a str,
    }
    let id = Id::of(
        "axiom.v2.derived-occurrence.v1",
        &Occurrence {
            rule: &rule.name,
            binding_schema: &subject.row.schema,
            binding_occurrence: &subject.row.id,
        },
    );
    format!("derived:{}", id.as_str())
}

fn world_id(model: &Model, document: &TypedDocument, evaluation: &Evaluation) -> Id {
    let decisions = sorted_decisions(&document.decisions);
    Id::of(
        "axiom.v2.world.v1",
        &WorldIdentity {
            model: &model.id(),
            decisions: &decisions,
            evaluation,
        },
    )
}

fn sorted_decisions(decisions: &[Decision]) -> Vec<Decision> {
    let mut sorted = decisions.to_vec();
    sorted.sort_by(|left, right| left.id.cmp(&right.id));
    sorted
}

fn decision_ids_by_occurrence(decisions: &[Decision]) -> BTreeMap<String, Vec<Id>> {
    let mut by_occurrence: BTreeMap<String, Vec<Id>> = BTreeMap::new();
    for decision in decisions {
        let occurrence = decision
            .target
            .rsplit_once('.')
            .map(|(occurrence, _)| occurrence)
            .unwrap_or(&decision.target);
        by_occurrence
            .entry(occurrence.to_owned())
            .or_default()
            .push(decision_claim_id(decision));
    }
    for ids in by_occurrence.values_mut() {
        canonicalize_ids(ids);
    }
    by_occurrence
}

fn source_inputs(row: &Row, decisions: &BTreeMap<String, Vec<Id>>) -> Vec<Id> {
    decisions.get(&row.id).cloned().unwrap_or_default()
}

/// Content identity for a source decision; `Decision.id` is only its authored key.
pub fn decision_claim_id(decision: &Decision) -> Id {
    Id::of("axiom.v2.decision.v1", decision)
}

fn canonicalize_ids(ids: &mut Vec<Id>) {
    ids.sort();
    ids.dedup();
}

fn failure_outcome(failure: RuleFailure) -> Outcome {
    match failure {
        RuleFailure::Alternatives(values, _) => Outcome::Alternatives(values),
        RuleFailure::Fault(Fault::Missing(message), _) => Outcome::Missing(vec![message]),
        RuleFailure::Fault(Fault::Conflict(message), _) => Outcome::Conflict(vec![message]),
        RuleFailure::Fault(Fault::Incomplete(message), _) => Outcome::Incomplete(message),
    }
}

fn failure_context(
    instruction: usize,
    expression: &Expr,
    binding: &str,
    environment: &BTreeMap<String, Value>,
) -> FailureContext {
    let mut names = BTreeSet::new();
    let mut remaining = 256;
    collect_expression_names(expression, &BTreeSet::new(), &mut names, &mut remaining);
    let expression = expression_excerpt(expression, 240);
    let mut bindings = BTreeMap::new();
    let mut binding_origins = BTreeMap::new();
    for name in names.into_iter().take(8) {
        if let Some(value) = resolve_binding_path(environment, &name) {
            if let Some(origin) = binding_origin(environment, binding, &name) {
                binding_origins.insert(name.clone(), origin);
            }
            bindings.insert(name, preview(value, 120));
        }
    }
    FailureContext {
        instruction,
        expression,
        bindings,
        binding_origins,
        target: None,
    }
}

fn binding_origin(
    environment: &BTreeMap<String, Value>,
    binding: &str,
    path: &str,
) -> Option<String> {
    let suffix = path.strip_prefix(&format!("{binding}."))?;
    let Value::Record(fields) = environment.get(binding)? else {
        return None;
    };
    let Some(Value::Ref(occurrence)) = fields.get("id") else {
        return None;
    };
    Some(format!("{occurrence}.{suffix}"))
}

fn choice_target(rule: &Rule, subject: &Claim, selector: &Expr) -> Option<String> {
    let Expr::Name(path) = selector else {
        return None;
    };
    let suffix = path.strip_prefix(&format!("{}.", rule.binding))?;
    if suffix.is_empty() {
        return None;
    }
    Some(format!("{}.{}", subject.row.id, suffix))
}

fn collect_expression_names(
    expression: &Expr,
    bound: &BTreeSet<String>,
    names: &mut BTreeSet<String>,
    remaining: &mut usize,
) {
    if *remaining == 0 {
        return;
    }
    *remaining -= 1;
    match expression {
        Expr::Name(name) => {
            let root = name.split('.').next().unwrap_or_default();
            if !bound.contains(root) && names.len() < 8 {
                names.insert(name.clone());
            }
        }
        Expr::Call(operator, arguments)
            if matches!(operator.as_str(), "filter" | "map" | "all" | "any" | "sort")
                && arguments.len() >= 3
                && let Expr::Name(binding) = &arguments[1] =>
        {
            collect_expression_names(&arguments[0], bound, names, remaining);
            let mut nested = bound.clone();
            nested.insert(binding.clone());
            collect_expression_names(&arguments[2], &nested, names, remaining);
            for argument in arguments.iter().skip(3) {
                collect_expression_names(argument, bound, names, remaining);
            }
        }
        Expr::Call(operator, arguments) => {
            for (index, argument) in arguments.iter().enumerate() {
                // Relation and field labels are static names, not bindings.
                if (operator == "rows" && index == 0) || (operator == "get" && index == 1) {
                    continue;
                }
                collect_expression_names(argument, bound, names, remaining);
            }
        }
        Expr::Literal(_) => {}
    }
}

fn resolve_binding_path<'a>(
    environment: &'a BTreeMap<String, Value>,
    path: &str,
) -> Option<&'a Value> {
    let mut names = path.split('.');
    let root = names.next()?;
    let mut value = environment.get(root)?;
    let mut depth = 0;
    for field in names {
        depth += 1;
        if depth > 16 {
            return None;
        }
        let Value::Record(fields) = value else {
            return None;
        };
        value = fields.get(field)?;
    }
    Some(value)
}

fn expression_excerpt(expression: &Expr, max_chars: usize) -> String {
    fn push(out: &mut String, remaining: &mut usize, text: &str) {
        if *remaining == 0 {
            return;
        }
        let mut chars = text.chars();
        let selected: String = chars.by_ref().take(*remaining).collect();
        let count = selected.chars().count();
        out.push_str(&selected);
        *remaining -= count;
        if chars.next().is_some() && *remaining > 0 {
            out.push('…');
            *remaining -= 1;
        }
    }

    fn render(expression: &Expr, out: &mut String, remaining: &mut usize) {
        if *remaining == 0 {
            return;
        }
        match expression {
            Expr::Literal(value) => push(out, remaining, &preview(value, *remaining)),
            Expr::Name(name) => push(out, remaining, name),
            Expr::Call(operator, arguments) => {
                push(out, remaining, "(");
                push(out, remaining, operator);
                for argument in arguments {
                    push(out, remaining, " ");
                    render(argument, out, remaining);
                    if *remaining == 0 {
                        break;
                    }
                }
                push(out, remaining, ")");
            }
        }
    }

    let mut out = String::new();
    let mut remaining = max_chars;
    render(expression, &mut out, &mut remaining);
    out
}

fn location_line(document: &TypedDocument, occurrence: &str) -> usize {
    document
        .locations
        .get(occurrence)
        .map(|span| span.line)
        .unwrap_or(1)
}

fn diagnostic(line: usize, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new(line.max(1), message)
}
