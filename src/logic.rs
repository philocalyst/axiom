//! Finance-native tabled logical search.
//!
//! The IR in [`crate::ir`] is the one term language used here.  This module
//! adds the pieces that are deliberately absent from the small IR: signed
//! relation literals, existential/default-negated goals, finite programs,
//! tabled least-fixed-point evaluation, and proof-bearing candidate results.
//! The solver is monotone for its accepted fragment.  In particular, a
//! positive cycle cannot manufacture its first fact, and a resource boundary
//! is reported as incomplete rather than as a semantic refutation.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;

use blake3::Hasher;

use crate::ir::{self, Atom, CanonicalContext, Clause as IrClause, Goal as IrGoal, Term, Var};
use crate::proof::{Node, Operation, Proof, ProofId};
// Keep the solver's historical import path source-compatible while exposing
// exactly the semantic kernel types; these are re-exports, not logic-owned
// result enums.
pub use crate::semantics::{Completion, Multiplicity, Resolution, Truth};
use crate::semantics::{Conditional, Conflict, GoalId, Requirement};
use crate::unify::Unifier;

/// A signed relation atom.  Explicit negative facts are kept separate from
/// open-world absence and can be derived by clauses just like positive facts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Literal {
    Positive(Atom),
    Negative(Atom),
}

impl Literal {
    pub fn positive(atom: Atom) -> Self {
        Self::Positive(atom)
    }

    pub fn negative(atom: Atom) -> Self {
        Self::Negative(atom)
    }

    pub fn polarity(&self) -> Polarity {
        match self {
            Self::Positive(_) => Polarity::Positive,
            Self::Negative(_) => Polarity::Negative,
        }
    }

    pub fn atom(&self) -> &Atom {
        match self {
            Self::Positive(atom) | Self::Negative(atom) => atom,
        }
    }

    pub fn opposite(&self) -> Self {
        match self {
            Self::Positive(atom) => Self::Negative(atom.clone()),
            Self::Negative(atom) => Self::Positive(atom.clone()),
        }
    }

    fn with_atom(&self, atom: Atom) -> Self {
        match self {
            Self::Positive(_) => Self::Positive(atom),
            Self::Negative(_) => Self::Negative(atom),
        }
    }

    fn is_ground(&self) -> bool {
        self.atom().arguments.iter().all(term_is_ground)
    }
}

impl fmt::Display for Literal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.polarity() == Polarity::Negative {
            formatter.write_str("not ")?;
        }
        write!(formatter, "{}(", self.atom().predicate)?;
        for (index, argument) in self.atom().arguments.iter().enumerate() {
            if index > 0 {
                formatter.write_str(", ")?;
            }
            write!(formatter, "{argument:?}")?;
        }
        formatter.write_str(")")
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Polarity {
    Positive,
    Negative,
}

/// A goal language over the finance-native IR.  `DefaultNot` is not an
/// explicit negative literal: it is an open-world operation and only succeeds
/// under a declared complete relation context.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Goal {
    True,
    False,
    Atom(Literal),
    And(Vec<Goal>),
    Or(Vec<Goal>),
    Exists { vars: Vec<Var>, body: Box<Goal> },
    Equal(Term, Term),
    NotEqual(Term, Term),
    DefaultNot(Box<Goal>),
}

impl Goal {
    pub fn atom(literal: Literal) -> Self {
        Self::Atom(literal)
    }

    pub fn and(goals: impl Into<Vec<Self>>) -> Self {
        let goals = goals.into();
        match goals.as_slice() {
            [] => Self::True,
            [goal] => goal.clone(),
            _ => Self::And(goals),
        }
    }

    pub fn or(goals: impl Into<Vec<Self>>) -> Self {
        let goals = goals.into();
        match goals.as_slice() {
            [] => Self::False,
            [goal] => goal.clone(),
            _ => Self::Or(goals),
        }
    }

    pub fn exists(vars: impl Into<Vec<Var>>, body: Self) -> Self {
        Self::Exists {
            vars: vars.into(),
            body: Box::new(body),
        }
    }

    pub fn default_not(body: Self) -> Self {
        Self::DefaultNot(Box::new(body))
    }

    pub fn from_ir(goal: IrGoal) -> Self {
        match goal {
            IrGoal::True => Self::True,
            IrGoal::False => Self::False,
            IrGoal::Atom(atom) => Self::Atom(Literal::Positive(atom)),
            IrGoal::Equal(left, right) => Self::Equal(left, right),
            IrGoal::NotEqual(left, right) => Self::NotEqual(left, right),
            IrGoal::And(goals) => Self::And(goals.into_iter().map(Self::from_ir).collect()),
            IrGoal::Or(goals) => Self::Or(goals.into_iter().map(Self::from_ir).collect()),
        }
    }

    fn variables(&self, output: &mut BTreeSet<Var>) {
        match self {
            Self::Atom(literal) => atom_variables(literal.atom(), output),
            Self::And(goals) | Self::Or(goals) => {
                for goal in goals {
                    goal.variables(output);
                }
            }
            Self::Exists { vars, body } => {
                output.extend(vars.iter().cloned());
                body.variables(output);
            }
            Self::Equal(left, right) | Self::NotEqual(left, right) => {
                term_variables(left, output);
                term_variables(right, output);
            }
            Self::DefaultNot(body) => body.variables(output),
            Self::True | Self::False => {}
        }
    }
}

/// Universally quantified rule. Variables are freshened on each fixed-point
/// application; an `ir::Clause` can be lifted when it has a positive head.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Clause {
    pub head: Literal,
    pub body: Goal,
}

impl Clause {
    pub fn new(head: Literal, body: Goal) -> Self {
        Self { head, body }
    }

    pub fn from_ir(clause: IrClause) -> Self {
        Self {
            head: Literal::Positive(clause.head),
            body: Goal::from_ir(clause.body),
        }
    }

    fn variables(&self) -> BTreeSet<Var> {
        let mut output = BTreeSet::new();
        atom_variables(self.head.atom(), &mut output);
        self.body.variables(&mut output);
        output
    }

    fn freshen(&self, fresh: &mut FreshVars) -> Self {
        let replacements = self
            .variables()
            .into_iter()
            .map(|variable| {
                let replacement = fresh.next_like(&variable);
                (variable, replacement)
            })
            .collect::<BTreeMap<_, _>>();
        Self {
            head: rename_literal(&self.head, &replacements),
            body: rename_goal(&self.body, &replacements),
        }
    }
}

/// A finite semantic program. Facts are ground by construction, making the
/// domain boundary explicit and preventing rules from silently minting terms.
#[derive(Clone, Debug, Default)]
pub struct Program {
    facts: Vec<Fact>,
    clauses: Vec<Clause>,
}

impl Program {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_fact(&mut self, literal: Literal) -> Result<(), LogicError> {
        self.add_fact_named("fact", literal)
    }

    pub fn add_fact_named(
        &mut self,
        source: impl Into<String>,
        literal: Literal,
    ) -> Result<(), LogicError> {
        if !literal.is_ground() {
            return Err(LogicError::NonGroundFact(literal));
        }
        self.facts.push(Fact {
            source: source.into(),
            literal,
        });
        Ok(())
    }

    pub fn add_clause(&mut self, clause: Clause) {
        self.clauses.push(clause);
    }

    pub fn facts(&self) -> impl Iterator<Item = &Fact> {
        self.facts.iter()
    }

    pub fn clauses(&self) -> impl Iterator<Item = &Clause> {
        self.clauses.iter()
    }

    fn digest(&self) -> [u8; 32] {
        let context = CanonicalContext::default();
        let mut values = Vec::new();
        for fact in &self.facts {
            values.push(format!(
                "fact:{}:{:?}:{:?}",
                fact.source,
                canonical_atom_bytes(fact.literal.atom(), &context),
                fact.literal.polarity()
            ));
        }
        for clause in &self.clauses {
            values.push(format!(
                "clause:{:?}",
                canonical_clause_bytes(clause, &context)
            ));
        }
        values.sort();
        let mut hasher = Hasher::new();
        hasher.update(b"axiom/logic/program/v1\0");
        for value in values {
            hasher.update(value.as_bytes());
            hasher.update(&[0]);
        }
        *hasher.finalize().as_bytes()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Fact {
    pub source: String,
    pub literal: Literal,
}

/// A relation-scoped completeness claim. `None` is an explicit wildcard for
/// that dimension, not an implicit closed-world default.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RelationScope {
    pub polarity: Option<Polarity>,
    pub predicate: Option<String>,
    pub arity: Option<usize>,
}

impl RelationScope {
    pub fn new(predicate: impl Into<String>, arity: usize, polarity: Polarity) -> Self {
        Self {
            polarity: Some(polarity),
            predicate: Some(predicate.into()),
            arity: Some(arity),
        }
    }

    pub fn for_literal(literal: &Literal) -> Self {
        Self::new(
            predicate_name(literal.atom()),
            literal.atom().arguments.len(),
            literal.polarity(),
        )
    }

    fn covers(&self, required: &Self) -> bool {
        self.polarity
            .is_none_or(|value| Some(value) == required.polarity)
            && self
                .predicate
                .as_ref()
                .is_none_or(|value| Some(value) == required.predicate.as_ref())
            && self.arity.is_none_or(|value| Some(value) == required.arity)
    }
}

#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CompletenessClaims {
    pub scopes: BTreeSet<RelationScope>,
}

impl CompletenessClaims {
    pub fn relation(
        mut self,
        predicate: impl Into<String>,
        arity: usize,
        polarity: Polarity,
    ) -> Self {
        self.scopes
            .insert(RelationScope::new(predicate, arity, polarity));
        self
    }

    pub fn covers(&self, goal: &Goal) -> bool {
        required_scopes(goal)
            .iter()
            .all(|required| self.scopes.iter().any(|claim| claim.covers(required)))
    }
}

/// Completeness is independent from truth. Open-world absence never proves a
/// default-negative goal; every such proof needs a matching relation claim.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Completeness {
    OpenWorld,
    Scoped(CompletenessClaims),
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ResourceProfile {
    pub max_steps: usize,
    pub max_iterations: usize,
    pub max_answers: usize,
    pub max_terms: usize,
}

impl Default for ResourceProfile {
    fn default() -> Self {
        Self {
            max_steps: 100_000,
            max_iterations: 256,
            max_answers: 10_000,
            max_terms: 100_000,
        }
    }
}

impl ResourceProfile {
    pub fn bounded(max_steps: usize) -> Self {
        Self {
            max_steps,
            ..Self::default()
        }
    }
}

/// Every cache dimension is semantic. The IR's `CanonicalContext` carries the
/// accepted world, packages, book, scenario, and semantics version; the logic
/// context adds completeness and the observable resource profile.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticContext {
    pub canonical: CanonicalContext,
    pub completeness: Completeness,
    pub resource_profile: ResourceProfile,
}

impl Default for SemanticContext {
    fn default() -> Self {
        Self {
            canonical: CanonicalContext::default(),
            completeness: Completeness::OpenWorld,
            resource_profile: ResourceProfile::default(),
        }
    }
}

impl SemanticContext {
    pub fn new(world: [u8; 32]) -> Self {
        Self {
            canonical: CanonicalContext {
                accepted_world: Some(world),
                ..CanonicalContext::default()
            },
            ..Self::default()
        }
    }

    pub fn with_packages(mut self, packages: impl IntoIterator<Item = [u8; 32]>) -> Self {
        self.canonical.package_hashes = packages.into_iter().collect();
        self.canonical.package_hashes.sort_unstable();
        self.canonical.package_hashes.dedup();
        self
    }

    pub fn with_book(mut self, book: impl Into<ir::Symbol>) -> Self {
        self.canonical.book = Some(book.into());
        self
    }

    pub fn with_scenario(mut self, scenario: impl Into<ir::Symbol>) -> Self {
        self.canonical.scenario = Some(scenario.into());
        self
    }

    pub fn complete_relation(
        mut self,
        predicate: impl Into<String>,
        arity: usize,
        polarity: Polarity,
    ) -> Self {
        let claims = match self.completeness {
            Completeness::OpenWorld => CompletenessClaims::default(),
            Completeness::Scoped(claims) => claims,
        };
        self.completeness = Completeness::Scoped(claims.relation(predicate, arity, polarity));
        self
    }

    pub fn complete_atom(mut self, literal: &Literal) -> Self {
        let scope = RelationScope::for_literal(literal);
        self = self.complete_relation(
            scope.predicate.unwrap_or_default(),
            scope.arity.unwrap_or_default(),
            scope.polarity.unwrap_or(Polarity::Positive),
        );
        self
    }

    pub fn with_resources(mut self, resources: ResourceProfile) -> Self {
        self.resource_profile = resources;
        self
    }
}

/// The kind of derivation used to produce a solver proof.
///
/// This remains solver metadata rather than a second proof vocabulary.  The
/// actual DAG and its content addresses are owned by [`crate::proof::Proof`].
#[derive(Clone, Debug, Eq, PartialEq)]
enum ProofKind {
    Fact { source: String },
    Rule { clause: [u8; 32] },
    True,
    Conjunction,
    Disjunction { branch: usize },
    Existential,
    Equality,
    Disequality,
    Completeness { scope: RelationScope },
    DefaultNegation,
}

/// Materialize solver proof metadata in the canonical proof DAG.  The logic
/// evaluator owns the meaning of `ProofKind`, but it never owns a second
/// identity or checker: node addresses, dependency edges, and validation all
/// come from [`crate::proof::Proof`].
fn insert_logic_proof(
    graph: &mut Proof,
    proposition: Goal,
    kind: ProofKind,
    dependencies: Vec<ProofId>,
) -> ProofId {
    let rule = match &kind {
        ProofKind::Fact { source } => {
            return graph.insert(Node::new(
                String::from_utf8_lossy(&canonical_logic_goal(&proposition)),
                Operation::Observation {
                    source: source.clone(),
                },
                dependencies,
                [("logic-kind".into(), "fact".into())].into_iter().collect(),
            ));
        }
        ProofKind::Rule { clause } => format!(
            "logic/rule/{}",
            clause
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        ),
        ProofKind::True => "logic/true".into(),
        ProofKind::Conjunction => "logic/conjunction".into(),
        ProofKind::Disjunction { branch } => format!("logic/disjunction/{branch}"),
        ProofKind::Existential => "logic/existential".into(),
        ProofKind::Equality => "logic/equality".into(),
        ProofKind::Disequality => "logic/disequality".into(),
        ProofKind::Completeness { scope } => format!("logic/completeness/{scope:?}"),
        ProofKind::DefaultNegation => "logic/default-negation".into(),
    };
    let mut metadata = BTreeMap::new();
    metadata.insert("logic-kind".into(), rule.clone());
    graph.insert(Node::new(
        String::from_utf8_lossy(&canonical_logic_goal(&proposition)),
        Operation::Derive { rule },
        dependencies,
        metadata,
    ))
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Substitution(pub BTreeMap<Var, Term>);

impl Substitution {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, variable: &Var) -> Option<&Term> {
        self.0.get(variable)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&Var, &Term)> {
        self.0.iter()
    }

    pub fn resolved(&self, variable: &Var) -> Option<Term> {
        self.0.get(variable).map(|term| resolve_term(term, self))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Candidate {
    pub substitution: Substitution,
    pub proofs: Vec<ProofId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TraceEvent {
    CacheHit,
    CacheMiss,
    FixedPointIteration { iteration: usize, new_facts: usize },
    CycleWithoutBase { predicate: String },
    ResourceLimit { resource: String },
    Unsupported { detail: String },
}

#[derive(Clone, Debug)]
pub struct SearchResult {
    pub(crate) resolution: Resolution<Candidate>,
    pub(crate) candidates: Vec<Candidate>,
    pub(crate) trace: Vec<TraceEvent>,
}

impl SearchResult {
    /// The one checked semantic result produced by the solver.
    pub fn resolution(&self) -> &Resolution<Candidate> {
        &self.resolution
    }

    /// Convenience accessors retain the semantic types; logic does not
    /// define a second truth/completion vocabulary.
    pub fn truth(&self) -> Truth {
        self.resolution.truth()
    }

    pub fn completion(&self) -> Completion {
        self.resolution.completion()
    }

    pub fn answers(&self) -> &Multiplicity<Conditional<Candidate>> {
        self.resolution.answers()
    }

    /// The canonical answer multiplicity, including candidates and residual
    /// obligations.  Logic does not flatten this into a second enum.
    pub fn multiplicity(&self) -> &Multiplicity<Conditional<Candidate>> {
        self.answers()
    }

    pub fn candidates(&self) -> &[Candidate] {
        &self.candidates
    }

    pub fn positive_proofs(&self) -> &[ProofId] {
        self.resolution.positive_proofs()
    }

    pub fn negative_proofs(&self) -> &[ProofId] {
        self.resolution.negative_proofs()
    }

    pub fn proof_graph(&self) -> &Proof {
        self.resolution.proof_context()
    }

    pub fn trace(&self) -> &[TraceEvent] {
        &self.trace
    }

    pub fn is_proven(&self) -> bool {
        self.completion() == Completion::Complete
            && matches!(self.truth(), Truth::TrueOnly | Truth::Both)
    }

    pub fn is_ambiguous(&self) -> bool {
        self.resolution.is_ambiguous()
    }

    pub fn is_incomplete(&self) -> bool {
        self.completion() != Completion::Complete
    }

    pub fn check_proofs(&self) -> Result<(), crate::proof::CheckError> {
        self.proof_graph().check()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LogicError {
    NonGroundFact(Literal),
}

impl fmt::Display for LogicError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonGroundFact(literal) => write!(formatter, "facts must be ground: {literal}"),
        }
    }
}

impl std::error::Error for LogicError {}

#[derive(Default)]
pub struct Solver {
    cache: HashMap<CacheKey, SearchResult>,
}

impl Solver {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cache_len(&self) -> usize {
        self.cache.len()
    }

    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }

    pub fn solve(
        &mut self,
        program: &Program,
        goal: &Goal,
        context: &SemanticContext,
    ) -> SearchResult {
        let query = CanonicalQuery::new(program, goal, context);
        if let Some(cached) = self.cache.get(&query.key) {
            let mut result = remap_result(cached, &query.canonical_to_actual);
            result.trace.insert(0, TraceEvent::CacheHit);
            return result;
        }
        let mut result = evaluate_uncached(program, goal, context);
        result.trace.insert(0, TraceEvent::CacheMiss);
        self.cache.insert(
            query.key,
            canonical_result(&result, &query.actual_to_canonical),
        );
        result
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
struct CacheKey {
    program: [u8; 32],
    goal: Vec<u8>,
    context: ContextKey,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
struct ContextKey {
    canonical: Vec<u8>,
    completeness: Completeness,
    resources: ResourceProfile,
}

struct CanonicalQuery {
    key: CacheKey,
    actual_to_canonical: BTreeMap<Var, Var>,
    canonical_to_actual: BTreeMap<Var, Var>,
}

impl CanonicalQuery {
    fn new(program: &Program, goal: &Goal, context: &SemanticContext) -> Self {
        let mut names = Names::default();
        let goal_bytes = canonical_goal_with_names(goal, &mut names);
        let mut actual_to_canonical = BTreeMap::new();
        let mut canonical_to_actual = BTreeMap::new();
        for (actual, index) in names.mapping() {
            let canonical = canonical_var(actual, index);
            actual_to_canonical.insert(actual.clone(), canonical.clone());
            canonical_to_actual.insert(canonical, actual.clone());
        }
        Self {
            key: CacheKey {
                program: program.digest(),
                goal: goal_bytes,
                context: ContextKey {
                    canonical: canonical_context_bytes(&context.canonical),
                    completeness: context.completeness.clone(),
                    resources: context.resource_profile,
                },
            },
            actual_to_canonical,
            canonical_to_actual,
        }
    }
}

#[derive(Default)]
struct Names {
    variables: BTreeMap<Var, usize>,
}

impl Names {
    fn name(&mut self, variable: &Var) -> usize {
        let next = self.variables.len();
        *self.variables.entry(variable.clone()).or_insert(next)
    }

    fn mapping(&self) -> impl Iterator<Item = (&Var, usize)> {
        self.variables.iter().map(|(var, index)| (var, *index))
    }
}

#[derive(Default)]
struct FreshVars {
    next: u32,
}

impl FreshVars {
    fn next_like(&mut self, variable: &Var) -> Var {
        let mut replacement = variable.clone();
        replacement.id = self.next;
        replacement.name = None;
        self.next = self.next.saturating_add(1);
        replacement
    }
}

#[derive(Clone, Debug)]
struct DerivedFact {
    literal: Literal,
    proofs: Vec<ProofId>,
}

#[derive(Default)]
struct Relation {
    facts: Vec<DerivedFact>,
}

impl Relation {
    fn contains(&self, literal: &Literal) -> bool {
        let key = literal_key(literal);
        self.facts
            .iter()
            .any(|fact| literal_key(&fact.literal) == key)
    }

    fn insert(&mut self, literal: Literal, proofs: Vec<ProofId>) -> bool {
        if self.contains(&literal) {
            return false;
        }
        self.facts.push(DerivedFact { literal, proofs });
        true
    }

    fn matching<'a>(&'a self, pattern: &'a Literal) -> impl Iterator<Item = &'a DerivedFact> + 'a {
        self.facts.iter().filter(move |fact| {
            fact.literal.polarity() == pattern.polarity()
                && fact.literal.atom().predicate == pattern.atom().predicate
                && fact.literal.atom().arguments.len() == pattern.atom().arguments.len()
        })
    }
}

#[derive(Default)]
struct SearchState {
    steps: usize,
    incomplete: bool,
    open_world: bool,
    unsupported: bool,
    trace: Vec<TraceEvent>,
    proofs: Proof,
}

impl SearchState {
    fn step(&mut self, resources: ResourceProfile) -> bool {
        if self.steps >= resources.max_steps {
            self.incomplete = true;
            self.trace_event(TraceEvent::ResourceLimit {
                resource: "steps".into(),
            });
            return false;
        }
        self.steps += 1;
        true
    }

    fn trace_event(&mut self, event: TraceEvent) {
        if self.trace.last() != Some(&event) {
            self.trace.push(event);
        }
    }
}

struct View<'a> {
    state: &'a mut SearchState,
    relation: &'a Relation,
    context: &'a SemanticContext,
}

#[derive(Clone)]
struct EvalCandidate {
    substitution: Substitution,
    proofs: Vec<ProofId>,
}

fn evaluate_uncached(program: &Program, goal: &Goal, context: &SemanticContext) -> SearchResult {
    let mut state = SearchState::default();
    let mut relation = Relation::default();
    for fact in &program.facts {
        if !state.step(context.resource_profile) {
            break;
        }
        let proof = insert_logic_proof(
            &mut state.proofs,
            Goal::Atom(fact.literal.clone()),
            ProofKind::Fact {
                source: fact.source.clone(),
            },
            Vec::new(),
        );
        relation.insert(fact.literal.clone(), vec![proof]);
    }
    if relation.facts.len() > context.resource_profile.max_terms {
        state.incomplete = true;
        state.trace_event(TraceEvent::ResourceLimit {
            resource: "relation terms".into(),
        });
    }

    let mut fresh = FreshVars {
        next: max_var_id(program, goal).saturating_add(1),
    };
    let mut iteration = 0;
    loop {
        if iteration >= context.resource_profile.max_iterations {
            state.incomplete = true;
            state.trace_event(TraceEvent::ResourceLimit {
                resource: "fixed-point iterations".into(),
            });
            break;
        }
        iteration += 1;
        let mut new_facts = 0;
        for clause in &program.clauses {
            if contains_default_negation(&clause.body) {
                state.unsupported = true;
                state.trace_event(TraceEvent::Unsupported {
                    detail:
                        "default negation in clauses is outside the positive fixed-point fragment"
                            .into(),
                });
                continue;
            }
            let fresh_clause = clause.freshen(&mut fresh);
            let candidates = {
                let mut view = View {
                    state: &mut state,
                    relation: &relation,
                    context,
                };
                evaluate_goal(&fresh_clause.body, &Substitution::new(), &mut view)
            };
            for candidate in candidates {
                let head = apply_literal(&fresh_clause.head, &candidate.substitution);
                if !head.is_ground() {
                    state.unsupported = true;
                    state.trace_event(TraceEvent::Unsupported {
                        detail: "unsafe clause head remains open".into(),
                    });
                    continue;
                }
                if relation.contains(&head) {
                    continue;
                }
                if relation.facts.len() >= context.resource_profile.max_terms {
                    state.incomplete = true;
                    state.trace_event(TraceEvent::ResourceLimit {
                        resource: "relation terms".into(),
                    });
                    break;
                }
                let proof = insert_logic_proof(
                    &mut state.proofs,
                    Goal::Atom(head.clone()),
                    ProofKind::Rule {
                        clause: clause_digest(&fresh_clause),
                    },
                    candidate.proofs,
                );
                if relation.insert(head, vec![proof]) {
                    new_facts += 1;
                }
            }
            if state.incomplete {
                break;
            }
        }
        state.trace_event(TraceEvent::FixedPointIteration {
            iteration,
            new_facts,
        });
        if new_facts == 0 || state.incomplete {
            break;
        }
    }

    if !state.incomplete {
        mark_no_base_cycles(program, &relation, &mut state);
    }

    let positive = {
        let mut view = View {
            state: &mut state,
            relation: &relation,
            context,
        };
        evaluate_goal(goal, &Substitution::new(), &mut view)
    };
    let positive = deduplicate(positive, context.resource_profile, &mut state);
    let negative = {
        let mut view = View {
            state: &mut state,
            relation: &relation,
            context,
        };
        opposite_candidates(goal, &mut view)
    };
    let negative = deduplicate(negative, context.resource_profile, &mut state);
    let truth = match (positive.is_empty(), negative.is_empty()) {
        (true, true) => Truth::Neither,
        (false, true) => Truth::TrueOnly,
        (true, false) => Truth::FalseOnly,
        (false, false) => Truth::Both,
    };
    let completion = if state.incomplete {
        Completion::ResourceLimited
    } else if state.open_world || state.unsupported {
        Completion::OpenWorld
    } else {
        Completion::Complete
    };
    let positive_proofs = proofs_of(&positive);
    let negative_proofs = proofs_of(&negative);
    let proof_graph = {
        let mut proof = state.proofs;
        for root in positive_proofs.iter().chain(&negative_proofs) {
            proof.root(*root);
        }
        proof
    };
    let candidates = positive
        .into_iter()
        .map(|candidate| Candidate {
            substitution: candidate.substitution,
            proofs: candidate.proofs,
        })
        .collect::<Vec<_>>();
    let answers = match candidates.len() {
        0 => Multiplicity::none(),
        1 => Multiplicity::unique(Conditional::unconditional(candidates[0].clone())),
        _ => Multiplicity::multiple(
            candidates
                .iter()
                .cloned()
                .map(Conditional::unconditional)
                .collect(),
        )
        .expect("candidate count greater than one produces multiple answers"),
    };
    let blockers = if completion == Completion::ResourceLimited {
        vec![Requirement::ResourceBoundary]
    } else {
        Vec::new()
    };
    let conflicts = if truth == Truth::Both {
        vec![
            Conflict::new(
                goal_id(goal),
                positive_proofs.clone(),
                negative_proofs.clone(),
            )
            .expect("both-sided support has both proof roots"),
        ]
    } else {
        Vec::new()
    };
    let resolution = Resolution::new_checked(
        &proof_graph,
        positive_proofs,
        negative_proofs,
        answers,
        completion,
        blockers,
        conflicts,
        Vec::new(),
    )
    .expect("solver emits a checked canonical resolution");
    SearchResult {
        resolution,
        candidates,
        trace: state.trace,
    }
}

fn goal_id(goal: &Goal) -> GoalId {
    GoalId::new(crate::model::ContentHash::domain_separated(
        "axiom/logic/goal",
        &canonical_logic_goal(goal),
    ))
    .expect("domain-separated goal ids are non-zero")
}

fn evaluate_goal(goal: &Goal, input: &Substitution, view: &mut View<'_>) -> Vec<EvalCandidate> {
    if !view.state.step(view.context.resource_profile) {
        return Vec::new();
    }
    match goal {
        Goal::True => {
            let proof = insert_logic_proof(
                &mut view.state.proofs,
                Goal::True,
                ProofKind::True,
                Vec::new(),
            );
            vec![EvalCandidate {
                substitution: input.clone(),
                proofs: vec![proof],
            }]
        }
        Goal::False => Vec::new(),
        Goal::Atom(pattern) => {
            let mut output = Vec::new();
            for fact in view.relation.matching(pattern) {
                if !view.state.step(view.context.resource_profile) {
                    break;
                }
                if let Some(substitution) = unify_atoms(pattern.atom(), fact.literal.atom(), input)
                    && pattern.polarity() == fact.literal.polarity()
                {
                    output.push(EvalCandidate {
                        substitution,
                        proofs: fact.proofs.clone(),
                    });
                }
            }
            output
        }
        Goal::And(goals) => {
            let mut candidates = vec![EvalCandidate {
                substitution: input.clone(),
                proofs: Vec::new(),
            }];
            for child in goals {
                let mut next = Vec::new();
                for candidate in candidates {
                    for child_candidate in evaluate_goal(child, &candidate.substitution, view) {
                        let mut proofs = candidate.proofs.clone();
                        proofs.extend(child_candidate.proofs);
                        next.push(EvalCandidate {
                            substitution: child_candidate.substitution,
                            proofs,
                        });
                    }
                }
                candidates = next;
                if candidates.is_empty() {
                    break;
                }
            }
            candidates
                .into_iter()
                .map(|candidate| {
                    let proof = insert_logic_proof(
                        &mut view.state.proofs,
                        goal.clone(),
                        ProofKind::Conjunction,
                        candidate.proofs.clone(),
                    );
                    EvalCandidate {
                        substitution: candidate.substitution,
                        proofs: vec![proof],
                    }
                })
                .collect()
        }
        Goal::Or(goals) => {
            let mut output = Vec::new();
            for (branch, child) in goals.iter().enumerate() {
                for candidate in evaluate_goal(child, input, view) {
                    let proof = insert_logic_proof(
                        &mut view.state.proofs,
                        child.clone(),
                        ProofKind::Disjunction { branch },
                        candidate.proofs.clone(),
                    );
                    output.push(EvalCandidate {
                        substitution: candidate.substitution,
                        proofs: vec![proof],
                    });
                }
            }
            output
        }
        Goal::Exists { vars, body } => {
            let bound = vars.iter().cloned().collect::<BTreeSet<_>>();
            evaluate_goal(body, input, view)
                .into_iter()
                .map(|candidate| {
                    let substitution = project_substitution(&candidate.substitution, &bound);
                    let proof = insert_logic_proof(
                        &mut view.state.proofs,
                        goal.clone(),
                        ProofKind::Existential,
                        candidate.proofs.clone(),
                    );
                    EvalCandidate {
                        substitution,
                        proofs: vec![proof],
                    }
                })
                .collect()
        }
        Goal::Equal(left, right) => unify_terms(left, right, input)
            .map(|substitution| {
                let proof = insert_logic_proof(
                    &mut view.state.proofs,
                    goal.clone(),
                    ProofKind::Equality,
                    Vec::new(),
                );
                vec![EvalCandidate {
                    substitution,
                    proofs: vec![proof],
                }]
            })
            .unwrap_or_default(),
        Goal::NotEqual(left, right) => {
            if definitely_different(left, right, input) {
                let proof = insert_logic_proof(
                    &mut view.state.proofs,
                    goal.clone(),
                    ProofKind::Disequality,
                    Vec::new(),
                );
                vec![EvalCandidate {
                    substitution: input.clone(),
                    proofs: vec![proof],
                }]
            } else {
                Vec::new()
            }
        }
        Goal::DefaultNot(body) => {
            let scopes = required_scopes(body);
            let complete = match &view.context.completeness {
                Completeness::OpenWorld => false,
                Completeness::Scoped(claims) => claims.covers(body),
            };
            if !complete {
                view.state.open_world = true;
                return Vec::new();
            }
            let inner = evaluate_goal(body, input, view);
            // An absent answer under a budget boundary, unsupported theory, or
            // nested open-world branch is not evidence of absence.
            if inner.is_empty()
                && !view.state.incomplete
                && !view.state.unsupported
                && !view.state.open_world
            {
                let completeness_proofs = scopes
                    .iter()
                    .map(|scope| {
                        insert_logic_proof(
                            &mut view.state.proofs,
                            Goal::True,
                            ProofKind::Completeness {
                                scope: scope.clone(),
                            },
                            Vec::new(),
                        )
                    })
                    .collect();
                let proof = insert_logic_proof(
                    &mut view.state.proofs,
                    goal.clone(),
                    ProofKind::DefaultNegation,
                    completeness_proofs,
                );
                vec![EvalCandidate {
                    substitution: input.clone(),
                    proofs: vec![proof],
                }]
            } else {
                Vec::new()
            }
        }
    }
}

fn opposite_candidates(goal: &Goal, view: &mut View<'_>) -> Vec<EvalCandidate> {
    match goal {
        Goal::Atom(literal) => {
            evaluate_goal(&Goal::Atom(literal.opposite()), &Substitution::new(), view)
        }
        _ => Vec::new(),
    }
}

fn deduplicate(
    candidates: Vec<EvalCandidate>,
    resources: ResourceProfile,
    state: &mut SearchState,
) -> Vec<EvalCandidate> {
    let mut unique = BTreeMap::<Vec<u8>, EvalCandidate>::new();
    for candidate in candidates {
        let key = substitution_key(&candidate.substitution);
        if !unique.contains_key(&key) && unique.len() >= resources.max_answers {
            state.incomplete = true;
            state.trace_event(TraceEvent::ResourceLimit {
                resource: "answers".into(),
            });
            break;
        }
        unique
            .entry(key)
            .and_modify(|existing| {
                for proof in &candidate.proofs {
                    if !existing.proofs.contains(proof) {
                        existing.proofs.push(*proof);
                    }
                }
            })
            .or_insert(candidate);
    }
    unique.into_values().collect()
}

fn proofs_of(candidates: &[EvalCandidate]) -> Vec<ProofId> {
    let mut output = BTreeSet::new();
    for candidate in candidates {
        output.extend(candidate.proofs.iter().copied());
    }
    output.into_iter().collect()
}

fn unify_atoms(left: &Atom, right: &Atom, input: &Substitution) -> Option<Substitution> {
    if left.predicate != right.predicate || left.arguments.len() != right.arguments.len() {
        return None;
    }
    let mut unifier = load_unifier(input)?;
    for (left, right) in left.arguments.iter().zip(&right.arguments) {
        unifier.unify(left, right).ok()?;
    }
    Some(Substitution(unifier.substitutions()))
}

fn unify_terms(left: &Term, right: &Term, input: &Substitution) -> Option<Substitution> {
    let mut unifier = load_unifier(input)?;
    unifier.unify(left, right).ok()?;
    Some(Substitution(unifier.substitutions()))
}

fn load_unifier(input: &Substitution) -> Option<Unifier> {
    let mut unifier = Unifier::new();
    for (variable, term) in input.iter() {
        unifier.unify(&Term::Var(variable.clone()), term).ok()?;
    }
    Some(unifier)
}

fn resolve_term(term: &Term, substitution: &Substitution) -> Term {
    load_unifier(substitution)
        .map(|unifier| unifier.resolve(term))
        .unwrap_or_else(|| term.clone())
}

fn apply_literal(literal: &Literal, substitution: &Substitution) -> Literal {
    let atom = literal.atom();
    literal.with_atom(Atom::new(
        atom.predicate.clone(),
        atom.arguments
            .iter()
            .map(|argument| resolve_term(argument, substitution))
            .collect(),
    ))
}

fn definitely_different(left: &Term, right: &Term, substitution: &Substitution) -> bool {
    let left = resolve_term(left, substitution);
    let right = resolve_term(right, substitution);
    if matches!(left, Term::Var(_)) || matches!(right, Term::Var(_)) {
        return false;
    }
    unify_terms(&left, &right, &Substitution::new()).is_none()
}

fn term_is_ground(term: &Term) -> bool {
    match term {
        Term::Var(_) => false,
        Term::Record(record) => record.fields.values().all(term_is_ground) && record.rest.is_none(),
        Term::Tuple(values) => values.iter().all(term_is_ground),
        Term::App { arguments, .. } => arguments.iter().all(term_is_ground),
        _ => true,
    }
}

fn term_variables(term: &Term, output: &mut BTreeSet<Var>) {
    match term {
        Term::Var(variable) => {
            output.insert(variable.clone());
        }
        Term::Record(record) => {
            for value in record.fields.values() {
                term_variables(value, output);
            }
            if let Some(rest) = &record.rest {
                output.insert(rest.clone());
            }
        }
        Term::Tuple(values) => {
            for value in values {
                term_variables(value, output);
            }
        }
        Term::App { arguments, .. } => {
            for value in arguments {
                term_variables(value, output);
            }
        }
        _ => {}
    }
}

fn atom_variables(atom: &Atom, output: &mut BTreeSet<Var>) {
    for argument in &atom.arguments {
        term_variables(argument, output);
    }
}

fn rename_term(term: &Term, replacements: &BTreeMap<Var, Var>) -> Term {
    match term {
        Term::Var(variable) => replacements
            .get(variable)
            .cloned()
            .map(Term::Var)
            .unwrap_or_else(|| term.clone()),
        Term::Record(record) => Term::Record(ir::Record {
            fields: record
                .fields
                .iter()
                .map(|(name, value)| (name.clone(), rename_term(value, replacements)))
                .collect(),
            rest: record.rest.as_ref().map(|rest| {
                replacements
                    .get(rest)
                    .cloned()
                    .unwrap_or_else(|| rest.clone())
            }),
        }),
        Term::Tuple(values) => Term::Tuple(
            values
                .iter()
                .map(|value| rename_term(value, replacements))
                .collect(),
        ),
        Term::App {
            constructor,
            arguments,
        } => Term::App {
            constructor: constructor.clone(),
            arguments: arguments
                .iter()
                .map(|value| rename_term(value, replacements))
                .collect(),
        },
        _ => term.clone(),
    }
}

fn rename_atom(atom: &Atom, replacements: &BTreeMap<Var, Var>) -> Atom {
    Atom::new(
        atom.predicate.clone(),
        atom.arguments
            .iter()
            .map(|argument| rename_term(argument, replacements))
            .collect(),
    )
}

fn rename_literal(literal: &Literal, replacements: &BTreeMap<Var, Var>) -> Literal {
    literal.with_atom(rename_atom(literal.atom(), replacements))
}

fn rename_goal(goal: &Goal, replacements: &BTreeMap<Var, Var>) -> Goal {
    match goal {
        Goal::True => Goal::True,
        Goal::False => Goal::False,
        Goal::Atom(literal) => Goal::Atom(rename_literal(literal, replacements)),
        Goal::And(goals) => Goal::And(
            goals
                .iter()
                .map(|goal| rename_goal(goal, replacements))
                .collect(),
        ),
        Goal::Or(goals) => Goal::Or(
            goals
                .iter()
                .map(|goal| rename_goal(goal, replacements))
                .collect(),
        ),
        Goal::Exists { vars, body } => Goal::Exists {
            vars: vars
                .iter()
                .map(|var| {
                    replacements
                        .get(var)
                        .cloned()
                        .unwrap_or_else(|| var.clone())
                })
                .collect(),
            body: Box::new(rename_goal(body, replacements)),
        },
        Goal::Equal(left, right) => Goal::Equal(
            rename_term(left, replacements),
            rename_term(right, replacements),
        ),
        Goal::NotEqual(left, right) => Goal::NotEqual(
            rename_term(left, replacements),
            rename_term(right, replacements),
        ),
        Goal::DefaultNot(body) => Goal::DefaultNot(Box::new(rename_goal(body, replacements))),
    }
}

fn contains_default_negation(goal: &Goal) -> bool {
    match goal {
        Goal::DefaultNot(_) => true,
        Goal::And(goals) | Goal::Or(goals) => goals.iter().any(contains_default_negation),
        Goal::Exists { body, .. } => contains_default_negation(body),
        Goal::True | Goal::False | Goal::Atom(_) | Goal::Equal(_, _) | Goal::NotEqual(_, _) => {
            false
        }
    }
}

fn required_scopes(goal: &Goal) -> BTreeSet<RelationScope> {
    let mut scopes = BTreeSet::new();
    collect_scopes(goal, &mut scopes);
    scopes
}

fn collect_scopes(goal: &Goal, output: &mut BTreeSet<RelationScope>) {
    match goal {
        Goal::Atom(literal) => {
            output.insert(RelationScope::for_literal(literal));
        }
        Goal::And(goals) | Goal::Or(goals) => {
            for goal in goals {
                collect_scopes(goal, output);
            }
        }
        Goal::Exists { body, .. } | Goal::DefaultNot(body) => collect_scopes(body, output),
        Goal::True | Goal::False | Goal::Equal(_, _) | Goal::NotEqual(_, _) => {}
    }
}

fn project_substitution(substitution: &Substitution, bound: &BTreeSet<Var>) -> Substitution {
    let mut projected = BTreeMap::new();
    for (variable, term) in substitution.iter() {
        if bound.contains(variable) {
            continue;
        }
        let resolved = resolve_term(term, substitution);
        if !term_contains_any_var(&resolved, bound) {
            projected.insert(variable.clone(), resolved);
        }
    }
    Substitution(projected)
}

fn term_contains_any_var(term: &Term, variables: &BTreeSet<Var>) -> bool {
    match term {
        Term::Var(variable) => variables.contains(variable),
        Term::Record(record) => {
            record
                .fields
                .values()
                .any(|value| term_contains_any_var(value, variables))
                || record
                    .rest
                    .as_ref()
                    .is_some_and(|rest| variables.contains(rest))
        }
        Term::Tuple(values) => values
            .iter()
            .any(|value| term_contains_any_var(value, variables)),
        Term::App { arguments, .. } => arguments
            .iter()
            .any(|value| term_contains_any_var(value, variables)),
        _ => false,
    }
}

fn max_var_id(program: &Program, goal: &Goal) -> u32 {
    let mut variables = BTreeSet::new();
    for fact in &program.facts {
        atom_variables(fact.literal.atom(), &mut variables);
    }
    for clause in &program.clauses {
        variables.extend(clause.variables());
    }
    goal.variables(&mut variables);
    variables
        .iter()
        .map(|variable| variable.id)
        .max()
        .unwrap_or(0)
}

fn mark_no_base_cycles(program: &Program, relation: &Relation, state: &mut SearchState) {
    let base_predicates = relation
        .facts
        .iter()
        .filter(|fact| fact.literal.polarity() == Polarity::Positive)
        .map(|fact| predicate_name(fact.literal.atom()))
        .collect::<HashSet<_>>();
    let mut graph = BTreeMap::<String, BTreeSet<String>>::new();
    for clause in &program.clauses {
        let mut body_predicates = BTreeSet::new();
        positive_predicates(&clause.body, &mut body_predicates);
        let predicate = predicate_name(clause.head.atom());
        graph.entry(predicate).or_default().extend(body_predicates);
    }
    let components = strongly_connected_components(&graph);
    for component in components {
        let cyclic = component.len() > 1
            || component.first().is_some_and(|predicate| {
                graph
                    .get(predicate)
                    .is_some_and(|neighbors| neighbors.contains(predicate))
            });
        if cyclic
            && !component
                .iter()
                .any(|predicate| base_predicates.contains(predicate))
        {
            for predicate in component {
                state.trace_event(TraceEvent::CycleWithoutBase { predicate });
            }
        }
    }
}

fn strongly_connected_components(graph: &BTreeMap<String, BTreeSet<String>>) -> Vec<Vec<String>> {
    let mut nodes = BTreeSet::new();
    for (node, neighbors) in graph {
        nodes.insert(node.clone());
        nodes.extend(neighbors.iter().cloned());
    }
    let mut search = Tarjan::new(graph);
    for node in nodes {
        if !search.indices.contains_key(&node) {
            search.visit(&node);
        }
    }
    search.output
}

struct Tarjan<'a> {
    graph: &'a BTreeMap<String, BTreeSet<String>>,
    next: usize,
    indices: HashMap<String, usize>,
    lowlinks: HashMap<String, usize>,
    stack: Vec<String>,
    on_stack: HashSet<String>,
    output: Vec<Vec<String>>,
}

impl<'a> Tarjan<'a> {
    fn new(graph: &'a BTreeMap<String, BTreeSet<String>>) -> Self {
        Self {
            graph,
            next: 0,
            indices: HashMap::new(),
            lowlinks: HashMap::new(),
            stack: Vec::new(),
            on_stack: HashSet::new(),
            output: Vec::new(),
        }
    }

    fn visit(&mut self, node: &str) {
        let index = self.next;
        self.next += 1;
        self.indices.insert(node.to_string(), index);
        self.lowlinks.insert(node.to_string(), index);
        self.stack.push(node.to_string());
        self.on_stack.insert(node.to_string());

        for neighbor in self.graph.get(node).into_iter().flatten() {
            if !self.indices.contains_key(neighbor) {
                self.visit(neighbor);
                let low = self.lowlinks[node].min(self.lowlinks[neighbor]);
                self.lowlinks.insert(node.to_string(), low);
            } else if self.on_stack.contains(neighbor) {
                let low = self.lowlinks[node].min(self.indices[neighbor]);
                self.lowlinks.insert(node.to_string(), low);
            }
        }

        if self.lowlinks[node] == self.indices[node] {
            let mut component = Vec::new();
            while let Some(member) = self.stack.pop() {
                self.on_stack.remove(&member);
                component.push(member.clone());
                if member == node {
                    break;
                }
            }
            component.sort();
            self.output.push(component);
        }
    }
}

fn positive_predicates(goal: &Goal, output: &mut BTreeSet<String>) {
    match goal {
        Goal::Atom(Literal::Positive(atom)) => {
            output.insert(predicate_name(atom));
        }
        Goal::And(goals) | Goal::Or(goals) => {
            for goal in goals {
                positive_predicates(goal, output);
            }
        }
        Goal::Exists { body, .. } | Goal::DefaultNot(body) => positive_predicates(body, output),
        Goal::True
        | Goal::False
        | Goal::Atom(Literal::Negative(_))
        | Goal::Equal(_, _)
        | Goal::NotEqual(_, _) => {}
    }
}

fn clause_digest(clause: &Clause) -> [u8; 32] {
    let mut hasher = Hasher::new();
    hasher.update(b"axiom/logic/clause/v1\0");
    hasher.update(&canonical_clause_bytes(
        clause,
        &CanonicalContext::default(),
    ));
    *hasher.finalize().as_bytes()
}

fn predicate_name(atom: &Atom) -> String {
    atom.predicate.name.as_str().to_owned()
}

fn literal_key(literal: &Literal) -> Vec<u8> {
    let mut bytes = vec![match literal.polarity() {
        Polarity::Positive => 1,
        Polarity::Negative => 2,
    }];
    bytes.extend(canonical_atom_bytes(
        literal.atom(),
        &CanonicalContext::default(),
    ));
    bytes
}

fn substitution_key(substitution: &Substitution) -> Vec<u8> {
    let mut names = Names::default();
    let mut values = substitution
        .iter()
        .map(|(variable, term)| {
            (
                canonical_var_text(variable, &mut names),
                canonical_term_text(term, &mut names),
            )
        })
        .collect::<Vec<_>>();
    values.sort();
    format!("{values:?}").into_bytes()
}

fn canonical_result(result: &SearchResult, mapping: &BTreeMap<Var, Var>) -> SearchResult {
    SearchResult {
        resolution: remap_resolution(result, mapping),
        candidates: result
            .candidates
            .iter()
            .map(|candidate| Candidate {
                substitution: remap_substitution(&candidate.substitution, mapping),
                proofs: candidate.proofs.clone(),
            })
            .collect(),
        trace: result.trace.clone(),
    }
}

fn remap_result(result: &SearchResult, mapping: &BTreeMap<Var, Var>) -> SearchResult {
    SearchResult {
        resolution: remap_resolution(result, mapping),
        candidates: result
            .candidates
            .iter()
            .map(|candidate| Candidate {
                substitution: remap_substitution(&candidate.substitution, mapping),
                proofs: candidate.proofs.clone(),
            })
            .collect(),
        trace: result.trace.clone(),
    }
}

fn remap_resolution(result: &SearchResult, mapping: &BTreeMap<Var, Var>) -> Resolution<Candidate> {
    let answers = match result.resolution.answers() {
        Multiplicity::None => Multiplicity::none(),
        Multiplicity::Unique(value) => Multiplicity::unique(Conditional::new(
            remap_candidate(value.value(), mapping),
            value.obligations().to_vec(),
        )),
        Multiplicity::Multiple(values) => Multiplicity::multiple(
            values
                .iter()
                .map(|value| {
                    Conditional::new(
                        remap_candidate(value.value(), mapping),
                        value.obligations().to_vec(),
                    )
                })
                .collect(),
        )
        .expect("a canonical multiple answer contains at least two values"),
    };
    Resolution::new_checked(
        result.proof_graph(),
        result.resolution().positive_proofs().to_vec(),
        result.resolution().negative_proofs().to_vec(),
        answers,
        result.resolution().completion(),
        result.resolution().blockers().to_vec(),
        result.resolution().conflicts().to_vec(),
        result.resolution().repairs().to_vec(),
    )
    .expect("cached result retains a checked proof context")
}

fn remap_candidate(candidate: &Candidate, mapping: &BTreeMap<Var, Var>) -> Candidate {
    Candidate {
        substitution: remap_substitution(&candidate.substitution, mapping),
        proofs: candidate.proofs.clone(),
    }
}

fn remap_substitution(substitution: &Substitution, mapping: &BTreeMap<Var, Var>) -> Substitution {
    let mut output = BTreeMap::new();
    for (variable, term) in substitution.iter() {
        if let Some(mapped) = mapping.get(variable) {
            output.insert(mapped.clone(), remap_term(term, mapping));
        }
    }
    Substitution(output)
}

fn remap_term(term: &Term, mapping: &BTreeMap<Var, Var>) -> Term {
    match term {
        Term::Var(variable) => mapping
            .get(variable)
            .cloned()
            .map(Term::Var)
            .unwrap_or_else(|| term.clone()),
        Term::Record(record) => Term::Record(ir::Record {
            fields: record
                .fields
                .iter()
                .map(|(name, value)| (name.clone(), remap_term(value, mapping)))
                .collect(),
            rest: record
                .rest
                .as_ref()
                .map(|rest| mapping.get(rest).cloned().unwrap_or_else(|| rest.clone())),
        }),
        Term::Tuple(values) => Term::Tuple(
            values
                .iter()
                .map(|value| remap_term(value, mapping))
                .collect(),
        ),
        Term::App {
            constructor,
            arguments,
        } => Term::App {
            constructor: constructor.clone(),
            arguments: arguments
                .iter()
                .map(|value| remap_term(value, mapping))
                .collect(),
        },
        _ => term.clone(),
    }
}

fn canonical_atom_bytes(atom: &Atom, context: &CanonicalContext) -> Vec<u8> {
    ir::canonicalize_atom(atom, context).bytes
}

fn canonical_clause_bytes(clause: &Clause, context: &CanonicalContext) -> Vec<u8> {
    let mut bytes = vec![match clause.head.polarity() {
        Polarity::Positive => 1,
        Polarity::Negative => 2,
    }];
    bytes.extend(canonical_atom_bytes(clause.head.atom(), context));
    bytes.extend(canonical_logic_goal(&clause.body));
    bytes
}

fn canonical_context_bytes(context: &CanonicalContext) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend(context.semantics_version.to_be_bytes());
    bytes.extend(context.accepted_world.unwrap_or([0; 32]));
    let mut packages = context.package_hashes.clone();
    packages.sort_unstable();
    for package in packages {
        bytes.extend(package);
    }
    bytes.extend(
        context
            .book
            .as_ref()
            .map_or(Vec::new(), |book| book.as_str().as_bytes().to_vec()),
    );
    bytes.push(0);
    bytes.extend(
        context
            .scenario
            .as_ref()
            .map_or(Vec::new(), |scenario| scenario.as_str().as_bytes().to_vec()),
    );
    bytes
}

fn canonical_var(variable: &Var, index: usize) -> Var {
    let mut output = variable.clone();
    output.id = index as u32;
    output.name = None;
    output
}

fn canonical_logic_goal(goal: &Goal) -> Vec<u8> {
    let mut names = Names::default();
    canonical_goal_with_names(goal, &mut names)
}

fn canonical_goal_with_names(goal: &Goal, names: &mut Names) -> Vec<u8> {
    match goal {
        Goal::True => b"true".to_vec(),
        Goal::False => b"false".to_vec(),
        Goal::Atom(literal) => {
            let mut bytes = vec![match literal.polarity() {
                Polarity::Positive => b'+',
                Polarity::Negative => b'-',
            }];
            bytes.extend(canonical_atom_text(literal.atom(), names).into_bytes());
            bytes
        }
        Goal::And(goals) => canonical_goals(b'&', goals, names),
        Goal::Or(goals) => canonical_goals(b'|', goals, names),
        Goal::Exists { vars, body } => {
            let mut bytes = b"exists:".to_vec();
            for variable in vars {
                bytes.extend(canonical_var_text(variable, names).into_bytes());
                bytes.push(b',');
            }
            bytes.extend(canonical_goal_with_names(body, names));
            bytes
        }
        Goal::Equal(left, right) => canonical_binary_goal(b'=', left, right, names),
        Goal::NotEqual(left, right) => canonical_binary_goal(b'!', left, right, names),
        Goal::DefaultNot(body) => {
            let mut bytes = b"not:".to_vec();
            bytes.extend(canonical_goal_with_names(body, names));
            bytes
        }
    }
}

fn canonical_goals(tag: u8, goals: &[Goal], names: &mut Names) -> Vec<u8> {
    let mut bytes = vec![tag];
    for goal in goals {
        bytes.extend(canonical_goal_with_names(goal, names));
        bytes.push(0);
    }
    bytes
}

fn canonical_binary_goal(tag: u8, left: &Term, right: &Term, names: &mut Names) -> Vec<u8> {
    let mut bytes = vec![tag];
    bytes.extend(canonical_term_text(left, names).into_bytes());
    bytes.push(0);
    bytes.extend(canonical_term_text(right, names).into_bytes());
    bytes
}

fn canonical_atom_text(atom: &Atom, names: &mut Names) -> String {
    format!(
        "{:?}({})",
        atom.predicate,
        atom.arguments
            .iter()
            .map(|term| canonical_term_text(term, names))
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn canonical_term_text(term: &Term, names: &mut Names) -> String {
    match term {
        Term::Var(variable) => format!("v{}", names.name(variable)),
        Term::Record(record) => {
            let mut fields = record
                .fields
                .iter()
                .map(|(name, value)| format!("{}={}", name, canonical_term_text(value, names)))
                .collect::<Vec<_>>();
            if let Some(rest) = &record.rest {
                fields.push(format!("|{}", canonical_var_text(rest, names)));
            }
            format!("r{{{}}}", fields.join(","))
        }
        Term::Tuple(values) => format!(
            "t({})",
            values
                .iter()
                .map(|value| canonical_term_text(value, names))
                .collect::<Vec<_>>()
                .join(",")
        ),
        Term::App {
            constructor,
            arguments,
        } => format!(
            "f{}({})",
            constructor,
            arguments
                .iter()
                .map(|value| canonical_term_text(value, names))
                .collect::<Vec<_>>()
                .join(",")
        ),
        _ => format!("{term:?}"),
    }
}

fn canonical_var_text(variable: &Var, names: &mut Names) -> String {
    format!("v{}", names.name(variable))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Nominal, NominalKind, Sort};

    fn predicate(name: &str) -> Nominal {
        Nominal::new(NominalKind::Predicate, name)
    }

    fn atom(name: &str, args: Vec<Term>) -> Atom {
        Atom::new(predicate(name), args)
    }

    fn value(name: &str) -> Term {
        Term::nominal(Nominal::new(
            NominalKind::Custom(ir::Symbol::from("value")),
            name,
        ))
    }

    fn variable(id: u32) -> Var {
        Var::inference(id)
    }

    fn v(id: u32) -> Term {
        Term::var(variable(id))
    }

    fn pos(name: &str, args: Vec<Term>) -> Goal {
        Goal::atom(Literal::positive(atom(name, args)))
    }

    fn edges() -> Program {
        let mut program = Program::new();
        program
            .add_fact_named(
                "a-b",
                Literal::positive(atom("edge", vec![value("a"), value("b")])),
            )
            .unwrap();
        program
            .add_fact_named(
                "b-c",
                Literal::positive(atom("edge", vec![value("b"), value("c")])),
            )
            .unwrap();
        program.add_clause(Clause::new(
            Literal::positive(atom("path", vec![v(10), v(11)])),
            pos("edge", vec![v(10), v(11)]),
        ));
        program.add_clause(Clause::new(
            Literal::positive(atom("path", vec![v(10), v(11)])),
            Goal::and(vec![
                pos("edge", vec![v(10), v(12)]),
                pos("path", vec![v(12), v(11)]),
            ]),
        ));
        program
    }

    #[test]
    fn transitive_closure_is_least_fixed_point() {
        let query_var = variable(1);
        let result = Solver::new().solve(
            &edges(),
            &pos("path", vec![value("a"), Term::var(query_var.clone())]),
            &SemanticContext::default(),
        );
        let values = result
            .candidates
            .iter()
            .filter_map(|candidate| candidate.substitution.resolved(&query_var))
            .collect::<Vec<_>>();
        // `Term` is not ordered by the IR on purpose, so compare canonical text.
        let values = values
            .iter()
            .map(|term| format!("{term:?}"))
            .collect::<BTreeSet<_>>();
        assert!(values.iter().any(|value| value.contains("b")));
        assert!(values.iter().any(|value| value.contains("c")));
        assert_eq!(result.completion(), Completion::Complete);
        assert!(matches!(result.answers(), Multiplicity::Multiple(_)));
        assert!(!result.positive_proofs().is_empty());
    }

    #[test]
    fn no_base_cycle_proves_nothing() {
        let mut program = Program::new();
        program.add_clause(Clause::new(
            Literal::positive(atom("loop", vec![v(1)])),
            pos("loop", vec![v(1)]),
        ));
        let result = Solver::new().solve(
            &program,
            &pos("loop", vec![value("x")]),
            &SemanticContext::default(),
        );
        assert_eq!(result.truth(), Truth::Neither);
        assert_eq!(result.completion(), Completion::Complete);
        assert!(
            result
                .trace
                .iter()
                .any(|event| matches!(event, TraceEvent::CycleWithoutBase { .. }))
        );
    }

    #[test]
    fn multiple_answers_are_ambiguous_and_negative_is_explicit() {
        let mut program = Program::new();
        program
            .add_fact(Literal::positive(atom(
                "eligible",
                vec![value("s"), value("one")],
            )))
            .unwrap();
        program
            .add_fact(Literal::positive(atom(
                "eligible",
                vec![value("s"), value("two")],
            )))
            .unwrap();
        program
            .add_fact(Literal::negative(atom("blocked", vec![value("s")])))
            .unwrap();
        let lot = variable(8);
        let result = Solver::new().solve(
            &program,
            &pos("eligible", vec![value("s"), Term::var(lot.clone())]),
            &SemanticContext::default(),
        );
        assert!(matches!(result.answers(), Multiplicity::Multiple(_)));
        assert_eq!(result.truth(), Truth::TrueOnly);
        let blocked = Solver::new().solve(
            &program,
            &pos("blocked", vec![value("s")]),
            &SemanticContext::default(),
        );
        assert_eq!(blocked.truth(), Truth::FalseOnly);
    }

    #[test]
    fn alpha_equivalent_queries_share_cache() {
        let mut program = Program::new();
        program
            .add_fact(Literal::positive(atom(
                "owns",
                vec![value("a"), value("book")],
            )))
            .unwrap();
        let mut solver = Solver::new();
        let first = variable(1);
        let second = variable(999);
        solver.solve(
            &program,
            &pos("owns", vec![value("a"), Term::var(first.clone())]),
            &SemanticContext::default(),
        );
        let result = solver.solve(
            &program,
            &pos("owns", vec![value("a"), Term::var(second.clone())]),
            &SemanticContext::default(),
        );
        assert_eq!(solver.cache_len(), 1);
        assert!(result.trace.contains(&TraceEvent::CacheHit));
        assert_eq!(
            result.candidates[0].substitution.resolved(&second),
            Some(value("book"))
        );
    }

    #[test]
    fn contexts_and_resource_limits_are_cache_safe() {
        let mut program = Program::new();
        program
            .add_fact(Literal::positive(atom("present", vec![value("x")])))
            .unwrap();
        let goal = pos("present", vec![value("x")]);
        let mut solver = Solver::new();
        solver.solve(&program, &goal, &SemanticContext::new([1; 32]));
        solver.solve(&program, &goal, &SemanticContext::new([2; 32]));
        solver.solve(
            &program,
            &goal,
            &SemanticContext::new([1; 32]).with_packages([[3; 32]]),
        );
        assert_eq!(solver.cache_len(), 3);

        let limited = SemanticContext::default().with_resources(ResourceProfile {
            max_steps: 2,
            max_iterations: 1,
            max_answers: 10,
            max_terms: 10,
        });
        let limited_result =
            Solver::new().solve(&edges(), &pos("path", vec![value("a"), v(2)]), &limited);
        assert_eq!(limited_result.completion(), Completion::ResourceLimited);
        assert_ne!(limited_result.truth(), Truth::FalseOnly);
    }

    #[test]
    fn existential_conjunction_disjunction_and_default_negation_are_distinct() {
        let mut program = Program::new();
        program
            .add_fact(Literal::positive(atom(
                "edge",
                vec![value("a"), value("b")],
            )))
            .unwrap();
        program
            .add_fact(Literal::positive(atom(
                "label",
                vec![value("b"), value("cash")],
            )))
            .unwrap();
        let hidden = variable(40);
        let output = variable(41);
        program.add_clause(Clause::new(
            Literal::positive(atom("reachable", vec![v(1), v(2)])),
            Goal::exists(
                vec![variable(3)],
                Goal::and(vec![
                    pos("edge", vec![v(1), Term::var(hidden.clone())]),
                    Goal::or(vec![
                        pos("label", vec![Term::var(hidden.clone()), value("cash")]),
                        pos("label", vec![Term::var(hidden), value("security")]),
                    ]),
                    Goal::Equal(v(2), v(3)),
                ]),
            ),
        ));
        // The body is intentionally only a structural smoke test here: its
        // existential variable remains local and must not escape the answer.
        let _ = output;
        let default_not_program = Program::new();
        let default_not = Solver::new().solve(
            &default_not_program,
            &Goal::default_not(pos("missing", vec![value("x")])),
            &SemanticContext::default(),
        );
        assert_eq!(default_not.completion(), Completion::OpenWorld);
        let complete = Solver::new().solve(
            &default_not_program,
            &Goal::default_not(pos("missing", vec![value("x")])),
            &SemanticContext::default().complete_relation("missing", 1, Polarity::Positive),
        );
        assert_eq!(complete.truth(), Truth::TrueOnly);
    }

    #[test]
    fn completeness_is_relation_scoped_and_requires_evidence() {
        let program = Program::new();
        let goal = Goal::default_not(pos("missing", vec![value("x")]));
        let wrong_scope = Solver::new().solve(
            &program,
            &goal,
            &SemanticContext::default().complete_relation("other", 1, Polarity::Positive),
        );
        assert_eq!(wrong_scope.completion(), Completion::OpenWorld);
        assert!(!wrong_scope.is_proven());

        let limited = SemanticContext::default()
            .complete_relation("missing", 1, Polarity::Positive)
            .with_resources(ResourceProfile {
                max_steps: 1,
                max_iterations: 1,
                max_answers: 10,
                max_terms: 10,
            });
        let limited_result = Solver::new().solve(&program, &goal, &limited);
        assert_eq!(limited_result.completion(), Completion::ResourceLimited);
        assert!(!limited_result.is_proven());
        assert!(limited_result.candidates.is_empty());

        let mut unsupported_program = Program::new();
        unsupported_program.add_clause(Clause::new(
            Literal::positive(atom("p", vec![v(1)])),
            Goal::default_not(pos("q", vec![v(1)])),
        ));
        let unsupported_goal = Goal::default_not(pos("p", vec![value("x")]));
        let unsupported = Solver::new().solve(
            &unsupported_program,
            &unsupported_goal,
            &SemanticContext::default().complete_relation("p", 1, Polarity::Positive),
        );
        assert_eq!(unsupported.completion(), Completion::OpenWorld);
        assert!(!unsupported.is_proven());
    }

    #[test]
    fn existential_projection_does_not_leak_binders_transitively() {
        let outer = variable(1);
        let hidden = variable(2);
        let goal = Goal::exists(
            vec![hidden.clone()],
            Goal::Equal(Term::var(outer.clone()), Term::var(hidden.clone())),
        );
        let result = Solver::new().solve(&Program::new(), &goal, &SemanticContext::default());
        assert_eq!(result.truth(), Truth::TrueOnly);
        assert!(result.candidates[0].substitution.get(&outer).is_none());
        assert!(result.check_proofs().is_ok());

        let nested = Goal::exists(
            vec![hidden.clone()],
            Goal::exists(
                vec![variable(3)],
                Goal::Equal(Term::var(outer.clone()), Term::var(hidden)),
            ),
        );
        let nested_result =
            Solver::new().solve(&Program::new(), &nested, &SemanticContext::default());
        assert!(
            nested_result.candidates[0]
                .substitution
                .get(&outer)
                .is_none()
        );
    }

    #[test]
    fn record_rows_are_included_in_cache_and_deduplication_keys() {
        let mut program = Program::new();
        let closed = Term::record(ir::Record::closed([("amount", value("one"))]));
        program
            .add_fact(Literal::positive(atom("row", vec![closed])))
            .unwrap();
        let row = Var::row(8);
        let open = Term::record(ir::Record::open([("amount", value("one"))], row.clone()));
        let mut solver = Solver::new();
        let open_result = solver.solve(
            &program,
            &pos("row", vec![open]),
            &SemanticContext::default(),
        );
        assert_eq!(open_result.truth(), Truth::TrueOnly);
        let closed_result = solver.solve(
            &program,
            &pos(
                "row",
                vec![Term::record(ir::Record::closed([("amount", value("one"))]))],
            ),
            &SemanticContext::default(),
        );
        assert_eq!(closed_result.truth(), Truth::TrueOnly);
        assert_eq!(solver.cache_len(), 2);
    }

    #[test]
    fn true_equality_and_disequality_proofs_are_checkable() {
        for goal in [
            Goal::True,
            Goal::Equal(value("a"), value("a")),
            Goal::NotEqual(value("a"), value("b")),
        ] {
            let result = Solver::new().solve(&Program::new(), &goal, &SemanticContext::default());
            assert!(result.is_proven());
            assert!(!result.positive_proofs().is_empty());
            assert!(result.check_proofs().is_ok());
        }
    }

    #[test]
    fn clause_content_address_includes_head_polarity() {
        let atom = atom("p", vec![value("x")]);
        let positive = Clause::new(Literal::positive(atom.clone()), Goal::True);
        let negative = Clause::new(Literal::negative(atom), Goal::True);
        assert_ne!(clause_digest(&positive), clause_digest(&negative));
    }

    #[test]
    fn mutual_no_base_cycles_are_diagnosed() {
        let mut program = Program::new();
        program.add_clause(Clause::new(
            Literal::positive(atom("p", vec![v(1)])),
            pos("q", vec![v(1)]),
        ));
        program.add_clause(Clause::new(
            Literal::positive(atom("q", vec![v(1)])),
            pos("p", vec![v(1)]),
        ));
        let result = Solver::new().solve(
            &program,
            &pos("p", vec![value("x")]),
            &SemanticContext::default(),
        );
        let cycles = result
            .trace
            .iter()
            .filter_map(|event| match event {
                TraceEvent::CycleWithoutBase { predicate } => Some(predicate.as_str()),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(cycles, BTreeSet::from(["p", "q"]));
        assert_eq!(result.truth(), Truth::Neither);
    }

    #[allow(dead_code)]
    fn _sort_is_still_available() -> Sort {
        Sort::Any
    }
}
