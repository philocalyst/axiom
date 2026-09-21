//! Transactional first-order unification for the canonical IR.
//!
//! The implementation is intentionally small: a substitution plus a trail is
//! enough to support speculative candidate search, and open records are
//! handled as ordinary row terms.  A failed top-level operation always restores
//! the state it saw, including fresh-variable allocation and hole domains.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::ir::{Atom, ExactQuantity, Goal, Record, Sort, Term, Unit, Var, VarKind};

/// A position in the trail.  Entries are replayed backwards on rollback.
#[derive(Clone, Debug)]
enum TrailEntry {
    Binding {
        variable: Var,
        previous: Option<Term>,
    },
    Domain {
        variable: Var,
        previous: Option<Vec<Term>>,
    },
    Sort {
        variable: Var,
        previous: Option<Sort>,
    },
}

/// An opaque savepoint.  Savepoints are cheap and may be nested.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Snapshot {
    trail_len: usize,
    next_var: u32,
}

/// The candidates that remain for a gradual hole after narrowing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Narrowing {
    None,
    Ambiguous(Vec<Term>),
    Unique(Term),
}

impl Narrowing {
    pub fn candidates(&self) -> &[Term] {
        match self {
            Self::None | Self::Unique(_) => &[],
            Self::Ambiguous(candidates) => candidates,
        }
    }

    pub fn is_unique(&self) -> bool {
        matches!(self, Self::Unique(_))
    }
}

/// A unification failure carries enough structure for a caller to explain the
/// rejected candidate without inspecting mutable solver state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnifyError {
    pub kind: UnifyErrorKind,
    pub left: Box<Term>,
    pub right: Box<Term>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UnifyErrorKind {
    OccursCheck(Box<Var>),
    NominalMismatch,
    UnitMismatch {
        left: Option<Box<Unit>>,
        right: Option<Box<Unit>>,
    },
    QuantityMismatch,
    ConstructorMismatch,
    ArityMismatch,
    MissingField(String),
    ClosedRowMismatch,
    RowMismatch,
    SortMismatch {
        variable: Box<Var>,
        expected: Box<Sort>,
    },
    InvalidRowVariable(Box<Var>),
    PredicateMismatch,
    GoalMismatch,
    Disequality,
    InvalidSnapshot,
}

impl fmt::Display for UnifyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "cannot unify: {:?}: {:?} with {:?}",
            self.kind, self.left, self.right
        )
    }
}

impl std::error::Error for UnifyError {}

/// A rollback unifier.  It is cloneable so theories can cheaply probe a
/// candidate without perturbing their parent search state.
#[derive(Clone, Debug, Default)]
pub struct Unifier {
    substitutions: BTreeMap<Var, Term>,
    domains: BTreeMap<Var, Vec<Term>>,
    sorts: BTreeMap<Var, Sort>,
    trail: Vec<TrailEntry>,
    next_var: u32,
}

impl Unifier {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            trail_len: self.trail.len(),
            next_var: self.next_var,
        }
    }

    /// Keep all changes made after `snapshot`.  The savepoint remains valid as
    /// an outer savepoint; callers that need a one-shot transaction can simply
    /// discard it after this call.
    pub fn commit(&mut self, snapshot: Snapshot) -> Result<(), UnifyError> {
        if snapshot.trail_len > self.trail.len() || snapshot.next_var > self.next_var {
            return Err(UnifyError {
                kind: UnifyErrorKind::InvalidSnapshot,
                left: Box::new(Term::Bool(false)),
                right: Box::new(Term::Bool(false)),
            });
        }
        Ok(())
    }

    pub fn rollback(&mut self, snapshot: Snapshot) -> Result<(), UnifyError> {
        if snapshot.trail_len > self.trail.len() || snapshot.next_var > self.next_var {
            return Err(UnifyError {
                kind: UnifyErrorKind::InvalidSnapshot,
                left: Box::new(Term::Bool(false)),
                right: Box::new(Term::Bool(false)),
            });
        }
        while self.trail.len() > snapshot.trail_len {
            match self.trail.pop().expect("trail length checked") {
                TrailEntry::Binding { variable, previous } => match previous {
                    Some(previous) => {
                        self.substitutions.insert(variable, previous);
                    }
                    None => {
                        self.substitutions.remove(&variable);
                    }
                },
                TrailEntry::Domain { variable, previous } => match previous {
                    Some(previous) => {
                        self.domains.insert(variable, previous);
                    }
                    None => {
                        self.domains.remove(&variable);
                    }
                },
                TrailEntry::Sort { variable, previous } => match previous {
                    Some(previous) => {
                        self.sorts.insert(variable, previous);
                    }
                    None => {
                        self.sorts.remove(&variable);
                    }
                },
            }
        }
        self.next_var = snapshot.next_var;
        Ok(())
    }

    pub fn fresh(&mut self, sort: Sort) -> Var {
        let variable = Var {
            id: self.next_var,
            kind: VarKind::Inference,
            name: None,
            sort,
        };
        self.next_var = self.next_var.saturating_add(1);
        variable
    }

    pub fn fresh_row(&mut self) -> Var {
        let variable = Var::row(self.next_var);
        self.next_var = self.next_var.saturating_add(1);
        variable
    }

    /// Unify a pair transactionally: a failed pair cannot leak a partial
    /// substitution into the next candidate.
    pub fn unify(&mut self, left: &Term, right: &Term) -> Result<(), UnifyError> {
        let snapshot = self.snapshot();
        match self.unify_inner(left, right) {
            Ok(()) => {
                self.commit(snapshot)?;
                Ok(())
            }
            Err(error) => {
                self.rollback(snapshot)?;
                Err(error)
            }
        }
    }

    pub fn unify_atom(&mut self, left: &Atom, right: &Atom) -> Result<(), UnifyError> {
        let snapshot = self.snapshot();
        let result = if left.predicate != right.predicate {
            Err(UnifyError {
                kind: UnifyErrorKind::PredicateMismatch,
                left: Box::new(Term::Text(left.predicate.to_string())),
                right: Box::new(Term::Text(right.predicate.to_string())),
            })
        } else if left.arguments.len() != right.arguments.len() {
            Err(UnifyError {
                kind: UnifyErrorKind::ArityMismatch,
                left: Box::new(Term::Text(left.predicate.to_string())),
                right: Box::new(Term::Text(right.predicate.to_string())),
            })
        } else {
            left.arguments
                .iter()
                .zip(&right.arguments)
                .try_for_each(|(left, right)| self.unify_inner(left, right))
        };
        match result {
            Ok(()) => {
                self.commit(snapshot)?;
                Ok(())
            }
            Err(error) => {
                self.rollback(snapshot)?;
                Err(error)
            }
        }
    }

    /// Structural goal unification is useful for clause indexing and keeps
    /// equality goals in the same transactional state as their atoms.
    pub fn unify_goal(&mut self, left: &Goal, right: &Goal) -> Result<(), UnifyError> {
        let snapshot = self.snapshot();
        let result = self.unify_goal_inner(left, right);
        match result {
            Ok(()) => {
                self.commit(snapshot)?;
                Ok(())
            }
            Err(error) => {
                self.rollback(snapshot)?;
                Err(error)
            }
        }
    }

    /// Narrow a named hole against candidates.  Candidate probes are rolled
    /// back independently.  A unique survivor is committed as a binding;
    /// multiple survivors remain an explicit domain rather than an arbitrary
    /// choice.
    pub fn narrow_hole(
        &mut self,
        hole: &Var,
        candidates: impl IntoIterator<Item = Term>,
    ) -> Result<Narrowing, UnifyError> {
        if !hole.is_hole() {
            return Err(UnifyError {
                kind: UnifyErrorKind::SortMismatch {
                    variable: Box::new(hole.clone()),
                    expected: Box::new(Sort::Any),
                },
                left: Box::new(Term::Var(hole.clone())),
                right: Box::new(Term::Bool(false)),
            });
        }
        let snapshot = self.snapshot();
        let existing = self.domains.get(hole).cloned();
        let mut candidates = candidates.into_iter().collect::<Vec<_>>();
        candidates.sort_by_key(term_sort_key);
        candidates.dedup();
        if let Some(existing) = existing {
            candidates.retain(|candidate| existing.iter().any(|old| old == candidate));
        }

        let mut survivors = Vec::new();
        for candidate in candidates {
            let probe = self.snapshot();
            if self
                .unify_inner(&Term::Var(hole.clone()), &candidate)
                .is_ok()
            {
                survivors.push(self.resolve(&candidate));
            }
            self.rollback(probe)?;
        }

        if survivors.is_empty() {
            self.rollback(snapshot)?;
            return Ok(Narrowing::None);
        }
        self.record_domain(hole.clone(), survivors.clone());
        if survivors.len() == 1 {
            self.unify(&Term::Var(hole.clone()), &survivors[0])?;
            Ok(Narrowing::Unique(survivors.remove(0)))
        } else {
            Ok(Narrowing::Ambiguous(survivors))
        }
    }

    pub fn hole_domain(&self, hole: &Var) -> Option<Vec<Term>> {
        self.domains.get(hole).cloned()
    }

    pub fn substitution(&self, variable: &Var) -> Option<Term> {
        self.substitutions
            .get(variable)
            .map(|term| self.resolve(term))
    }

    pub fn substitutions(&self) -> BTreeMap<Var, Term> {
        self.substitutions
            .iter()
            .map(|(variable, term)| (variable.clone(), self.resolve(term)))
            .collect()
    }

    pub fn resolve(&self, term: &Term) -> Term {
        let mut seen = BTreeSet::new();
        self.resolve_with_seen(term, &mut seen)
    }

    fn resolve_with_seen(&self, term: &Term, seen: &mut BTreeSet<Var>) -> Term {
        match term {
            Term::Var(variable) => {
                if !seen.insert(variable.clone()) {
                    return term.clone();
                }
                let resolved = self
                    .substitutions
                    .get(variable)
                    .map(|term| self.resolve_with_seen(term, seen))
                    .unwrap_or_else(|| term.clone());
                seen.remove(variable);
                resolved
            }
            Term::Record(record) => Term::Record(Record {
                fields: record
                    .fields
                    .iter()
                    .map(|(name, value)| (name.clone(), self.resolve_with_seen(value, seen)))
                    .collect(),
                rest: record.rest.clone(),
            }),
            Term::Tuple(values) => Term::Tuple(
                values
                    .iter()
                    .map(|value| self.resolve_with_seen(value, seen))
                    .collect(),
            ),
            Term::App {
                constructor,
                arguments,
            } => Term::App {
                constructor: constructor.clone(),
                arguments: arguments
                    .iter()
                    .map(|argument| self.resolve_with_seen(argument, seen))
                    .collect(),
            },
            other => other.clone(),
        }
    }

    fn unify_goal_inner(&mut self, left: &Goal, right: &Goal) -> Result<(), UnifyError> {
        match (left, right) {
            (Goal::True, Goal::True) | (Goal::False, Goal::False) => Ok(()),
            (Goal::Atom(left), Goal::Atom(right)) => {
                if left.predicate != right.predicate
                    || left.arguments.len() != right.arguments.len()
                {
                    return Err(UnifyError {
                        kind: UnifyErrorKind::GoalMismatch,
                        left: Box::new(Term::Text(format!("{:?}", left.predicate))),
                        right: Box::new(Term::Text(format!("{:?}", right.predicate))),
                    });
                }
                left.arguments
                    .iter()
                    .zip(&right.arguments)
                    .try_for_each(|(left, right)| self.unify_inner(left, right))
            }
            (Goal::Equal(left_a, left_b), Goal::Equal(right_a, right_b)) => {
                self.unify_inner(left_a, right_a)?;
                self.unify_inner(left_b, right_b)
            }
            (Goal::NotEqual(left_a, left_b), Goal::NotEqual(right_a, right_b)) => {
                self.unify_inner(left_a, right_a)?;
                self.unify_inner(left_b, right_b)
            }
            (Goal::And(left), Goal::And(right)) | (Goal::Or(left), Goal::Or(right)) => {
                if left.len() != right.len() {
                    return Err(UnifyError {
                        kind: UnifyErrorKind::ArityMismatch,
                        left: Box::new(Term::Text(left.len().to_string())),
                        right: Box::new(Term::Text(right.len().to_string())),
                    });
                }
                left.iter()
                    .zip(right)
                    .try_for_each(|(left, right)| self.unify_goal_inner(left, right))
            }
            _ => Err(UnifyError {
                kind: UnifyErrorKind::GoalMismatch,
                left: Box::new(Term::Bool(matches!(left, Goal::True))),
                right: Box::new(Term::Bool(matches!(right, Goal::True))),
            }),
        }
    }

    fn unify_inner(&mut self, left: &Term, right: &Term) -> Result<(), UnifyError> {
        let left = self.walk(left);
        let right = self.walk(right);
        if left == right {
            return Ok(());
        }
        match (&left, &right) {
            (Term::Var(left), _) => self.bind(left, right),
            (_, Term::Var(right)) => self.bind(right, left),
            (Term::Nominal(left), Term::Nominal(right)) => {
                if left == right {
                    Ok(())
                } else {
                    Err(self.error(
                        UnifyErrorKind::NominalMismatch,
                        &Term::Nominal(left.clone()),
                        &Term::Nominal(right.clone()),
                    ))
                }
            }
            (Term::Unit(left), Term::Unit(right)) => {
                if left == right {
                    Ok(())
                } else {
                    Err(self.error(
                        UnifyErrorKind::UnitMismatch {
                            left: Some(Box::new(left.clone())),
                            right: Some(Box::new(right.clone())),
                        },
                        &Term::Unit(left.clone()),
                        &Term::Unit(right.clone()),
                    ))
                }
            }
            (Term::Quantity(left), Term::Quantity(right)) => self.unify_quantity(left, right),
            (Term::Record(left), Term::Record(right)) => self.unify_record(left, right),
            (Term::Tuple(left), Term::Tuple(right)) => {
                if left.len() != right.len() {
                    return Err(self.error(
                        UnifyErrorKind::ArityMismatch,
                        &Term::Tuple(left.clone()),
                        &Term::Tuple(right.clone()),
                    ));
                }
                left.iter()
                    .zip(right)
                    .try_for_each(|(left, right)| self.unify_inner(left, right))
            }
            (
                Term::App {
                    constructor: left_constructor,
                    arguments: left_arguments,
                },
                Term::App {
                    constructor: right_constructor,
                    arguments: right_arguments,
                },
            ) => {
                if left_constructor != right_constructor {
                    return Err(self.error(UnifyErrorKind::ConstructorMismatch, &left, &right));
                }
                if left_arguments.len() != right_arguments.len() {
                    return Err(self.error(UnifyErrorKind::ArityMismatch, &left, &right));
                }
                left_arguments
                    .iter()
                    .zip(right_arguments)
                    .try_for_each(|(left, right)| self.unify_inner(left, right))
            }
            (Term::Bool(left), Term::Bool(right)) if left == right => Ok(()),
            (Term::Text(left), Term::Text(right)) if left == right => Ok(()),
            _ => Err(self.error(UnifyErrorKind::ConstructorMismatch, &left, &right)),
        }
    }

    fn unify_quantity(
        &mut self,
        left: &ExactQuantity,
        right: &ExactQuantity,
    ) -> Result<(), UnifyError> {
        if left.value() != right.value() {
            return Err(self.error(
                UnifyErrorKind::QuantityMismatch,
                &Term::Quantity(left.clone()),
                &Term::Quantity(right.clone()),
            ));
        }
        if units_compatible(left, right) {
            Ok(())
        } else {
            Err(self.error(
                UnifyErrorKind::UnitMismatch {
                    left: left.unit().cloned().map(Box::new),
                    right: right.unit().cloned().map(Box::new),
                },
                &Term::Quantity(left.clone()),
                &Term::Quantity(right.clone()),
            ))
        }
    }

    fn unify_record(&mut self, left: &Record, right: &Record) -> Result<(), UnifyError> {
        let (left_fields, left_tail) = self.flatten_record(left)?;
        let (right_fields, right_tail) = self.flatten_record(right)?;

        for name in left_fields
            .keys()
            .filter(|name| right_fields.contains_key(*name))
        {
            self.unify_inner(
                left_fields.get(name).expect("key exists"),
                right_fields.get(name).expect("key exists"),
            )?;
        }

        let left_only = left_fields
            .keys()
            .filter(|name| !right_fields.contains_key(*name))
            .cloned()
            .collect::<Vec<_>>();
        let right_only = right_fields
            .keys()
            .filter(|name| !left_fields.contains_key(*name))
            .cloned()
            .collect::<Vec<_>>();

        match (left_tail, right_tail) {
            (None, None) if !left_only.is_empty() || !right_only.is_empty() => Err(self.error(
                UnifyErrorKind::ClosedRowMismatch,
                &Term::Record(left.clone()),
                &Term::Record(right.clone()),
            )),
            (None, None) => Ok(()),
            (Some(left_tail), None) => {
                let fields = right_only
                    .into_iter()
                    .map(|name| (name.clone(), right_fields[&name].clone()))
                    .collect::<Vec<_>>();
                self.bind(&left_tail, Term::Record(Record::closed(fields)))
            }
            (None, Some(right_tail)) => {
                let fields = left_only
                    .into_iter()
                    .map(|name| (name.clone(), left_fields[&name].clone()))
                    .collect::<Vec<_>>();
                self.bind(&right_tail, Term::Record(Record::closed(fields)))
            }
            (Some(left_tail), Some(right_tail)) if left_tail == right_tail => {
                if left_only.is_empty() && right_only.is_empty() {
                    Ok(())
                } else {
                    Err(self.error(
                        UnifyErrorKind::RowMismatch,
                        &Term::Record(left.clone()),
                        &Term::Record(right.clone()),
                    ))
                }
            }
            (Some(left_tail), Some(right_tail)) => {
                let shared_tail = self.fresh_row();
                let left_tail_fields = right_only
                    .into_iter()
                    .map(|name| (name.clone(), right_fields[&name].clone()))
                    .collect::<Vec<_>>();
                let right_tail_fields = left_only
                    .into_iter()
                    .map(|name| (name.clone(), left_fields[&name].clone()))
                    .collect::<Vec<_>>();
                self.bind(
                    &left_tail,
                    Term::Record(Record::open(left_tail_fields, shared_tail.clone())),
                )?;
                self.bind(
                    &right_tail,
                    Term::Record(Record::open(right_tail_fields, shared_tail)),
                )
            }
        }
    }

    fn flatten_record(
        &self,
        record: &Record,
    ) -> Result<(BTreeMap<crate::ir::Symbol, Term>, Option<Var>), UnifyError> {
        let mut fields = BTreeMap::new();
        let mut current = record.clone();
        loop {
            for (name, value) in current.fields {
                if fields.insert(name.clone(), value).is_some() {
                    // Duplicate row fields are not representable as a single
                    // record.  Keeping this strict avoids silently changing
                    // row semantics while the IR is still small.
                    return Err(self.error(
                        UnifyErrorKind::RowMismatch,
                        &Term::Record(record.clone()),
                        &Term::Record(record.clone()),
                    ));
                }
            }
            match current.rest {
                None => return Ok((fields, None)),
                Some(variable) => match self.walk(&Term::Var(variable.clone())) {
                    Term::Var(variable) if self.effective_sort(&variable) == Sort::Row => {
                        return Ok((fields, Some(variable)));
                    }
                    Term::Var(variable) => {
                        return Err(self.error(
                            UnifyErrorKind::InvalidRowVariable(Box::new(variable.clone())),
                            &Term::Record(record.clone()),
                            &Term::Var(variable),
                        ));
                    }
                    Term::Record(next) => current = next,
                    other => {
                        return Err(self.error(
                            UnifyErrorKind::InvalidRowVariable(Box::new(variable)),
                            &Term::Record(record.clone()),
                            &other,
                        ));
                    }
                },
            }
        }
    }

    fn bind(&mut self, variable: &Var, term: Term) -> Result<(), UnifyError> {
        let term = self.walk(&term);
        let variable_sort = self.effective_sort(variable);
        if variable.kind == VarKind::Row && variable_sort != Sort::Row {
            return Err(self.error(
                UnifyErrorKind::InvalidRowVariable(Box::new(variable.clone())),
                &Term::Var(variable.clone()),
                &term,
            ));
        }
        if let Term::Var(other) = &term {
            if variable == other {
                return Ok(());
            }
            let merged =
                match merge_sorts(&self.effective_sort(variable), &self.effective_sort(other)) {
                    Some(sort) => sort,
                    None => {
                        return Err(self.error(
                            UnifyErrorKind::SortMismatch {
                                variable: Box::new(variable.clone()),
                                expected: Box::new(self.effective_sort(variable)),
                            },
                            &Term::Var(variable.clone()),
                            &term,
                        ));
                    }
                };
            self.record_sort(variable.clone(), merged.clone());
            self.record_sort(other.clone(), merged);
        }
        if !accepts(&variable_sort, &term) {
            return Err(self.error(
                UnifyErrorKind::SortMismatch {
                    variable: Box::new(variable.clone()),
                    expected: Box::new(variable_sort),
                },
                &Term::Var(variable.clone()),
                &term,
            ));
        }
        if self.occurs(variable, &term) {
            return Err(self.error(
                UnifyErrorKind::OccursCheck(Box::new(variable.clone())),
                &Term::Var(variable.clone()),
                &term,
            ));
        }
        self.record_binding(variable.clone(), term);
        Ok(())
    }

    fn effective_sort(&self, variable: &Var) -> Sort {
        self.sorts
            .get(variable)
            .cloned()
            .unwrap_or_else(|| variable.sort.clone())
    }

    fn record_sort(&mut self, variable: Var, sort: Sort) {
        let previous = self.sorts.insert(variable.clone(), sort.clone());
        if previous.as_ref() != Some(&sort) {
            self.trail.push(TrailEntry::Sort { variable, previous });
        }
    }

    fn occurs(&self, variable: &Var, term: &Term) -> bool {
        match self.walk(term) {
            Term::Var(other) => variable == &other,
            Term::Record(record) => {
                record
                    .fields
                    .values()
                    .any(|value| self.occurs(variable, value))
                    || record.rest.as_ref().is_some_and(|rest| {
                        variable == rest || self.occurs(variable, &Term::Var(rest.clone()))
                    })
            }
            Term::Tuple(values) => values.iter().any(|value| self.occurs(variable, value)),
            Term::App { arguments, .. } => {
                arguments.iter().any(|value| self.occurs(variable, value))
            }
            _ => false,
        }
    }

    fn walk(&self, term: &Term) -> Term {
        match term {
            Term::Var(variable) => self
                .substitutions
                .get(variable)
                .map(|term| self.walk(term))
                .unwrap_or_else(|| term.clone()),
            other => other.clone(),
        }
    }

    fn record_binding(&mut self, variable: Var, term: Term) {
        let previous = self.substitutions.insert(variable.clone(), term);
        self.trail.push(TrailEntry::Binding { variable, previous });
    }

    fn record_domain(&mut self, variable: Var, domain: Vec<Term>) {
        let previous = self.domains.insert(variable.clone(), domain);
        self.trail.push(TrailEntry::Domain { variable, previous });
    }

    fn error(&self, kind: UnifyErrorKind, left: &Term, right: &Term) -> UnifyError {
        UnifyError {
            kind,
            left: Box::new(left.clone()),
            right: Box::new(right.clone()),
        }
    }
}

fn merge_sorts(left: &Sort, right: &Sort) -> Option<Sort> {
    if left == &Sort::Any {
        return Some(right.clone());
    }
    if right == &Sort::Any || left == right {
        return Some(left.clone());
    }
    match (left, right) {
        (Sort::Nominal(None), Sort::Nominal(Some(kind)))
        | (Sort::Nominal(Some(kind)), Sort::Nominal(None)) => {
            Some(Sort::Nominal(Some(kind.clone())))
        }
        (Sort::Unit(None), Sort::Unit(Some(unit))) | (Sort::Unit(Some(unit)), Sort::Unit(None)) => {
            Some(Sort::Unit(Some(unit.clone())))
        }
        (Sort::Quantity(None), Sort::Quantity(Some(unit)))
        | (Sort::Quantity(Some(unit)), Sort::Quantity(None)) => {
            Some(Sort::Quantity(Some(unit.clone())))
        }
        _ => None,
    }
}

fn accepts(sort: &Sort, term: &Term) -> bool {
    match sort {
        Sort::Any => true,
        Sort::Nominal(expected) => match term {
            Term::Nominal(nominal) => expected.as_ref().is_none_or(|kind| kind == &nominal.kind),
            Term::Var(_) => true,
            _ => false,
        },
        Sort::Unit(expected) => match term {
            Term::Unit(unit) => expected.as_ref().is_none_or(|expected| expected == unit),
            Term::Var(_) => true,
            _ => false,
        },
        Sort::Quantity(expected) => match term {
            Term::Quantity(quantity) => expected
                .as_ref()
                .is_none_or(|expected| quantity.unit().is_none_or(|unit| unit == expected)),
            Term::Var(_) => true,
            _ => false,
        },
        Sort::Record => matches!(term, Term::Record(_) | Term::Var(_)),
        Sort::Row => matches!(term, Term::Record(_) | Term::Var(_)),
        Sort::Atom => false,
        Sort::Goal => false,
    }
}

fn units_compatible(left: &ExactQuantity, right: &ExactQuantity) -> bool {
    if left.unit() == right.unit() {
        return true;
    }
    // A unit-less zero is polymorphic; an explicitly unit-tagged zero still
    // has the ordinary nominal identity of that unit.
    (left.is_zero() && left.unit().is_none()) || (right.is_zero() && right.unit().is_none())
}

fn term_sort_key(term: &Term) -> String {
    format!("{term:?}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{ExactQuantity, Nominal, NominalKind, Record, Sort, Symbol};
    use num_bigint::BigInt;
    use num_rational::BigRational;

    fn unit(name: &str) -> Unit {
        Unit::new(name)
    }

    fn quantity(value: i64, unit_name: &str) -> Term {
        Term::Quantity(ExactQuantity::integer(BigInt::from(value), unit(unit_name)))
    }

    #[test]
    fn rollback_restores_partial_bindings() {
        let mut unifier = Unifier::new();
        let x = Var::inference(1);
        let y = Var::inference(2);
        let before = unifier.snapshot();
        let left = Term::Tuple(vec![Term::Var(x.clone()), Term::Bool(true)]);
        let right = Term::Tuple(vec![Term::Bool(false), Term::Bool(false)]);
        assert!(unifier.unify(&left, &right).is_err());
        assert!(unifier.substitution(&x).is_none());
        assert!(unifier.substitution(&y).is_none());
        unifier.rollback(before).unwrap();
        assert!(unifier.substitutions().is_empty());
    }

    #[test]
    fn occurs_check_rejects_recursive_term_without_leaking_state() {
        let mut unifier = Unifier::new();
        let x = Var::inference(1);
        let recursive = Term::App {
            constructor: Symbol::from("list"),
            arguments: vec![Term::Var(x.clone())],
        };
        let error = unifier
            .unify(&Term::Var(x.clone()), &recursive)
            .unwrap_err();
        assert!(matches!(error.kind, UnifyErrorKind::OccursCheck(_)));
        assert!(unifier.substitution(&x).is_none());
    }

    #[test]
    fn nominal_and_unit_mismatches_are_distinct() {
        let mut unifier = Unifier::new();
        let entity = Term::Nominal(Nominal::new(NominalKind::Entity, "USD"));
        let instrument = Term::Nominal(Nominal::new(NominalKind::Instrument, "USD"));
        assert!(matches!(
            unifier.unify(&entity, &instrument).unwrap_err().kind,
            UnifyErrorKind::NominalMismatch
        ));
        let usd = quantity(1, "USD");
        let eur = quantity(1, "EUR");
        assert!(matches!(
            unifier.unify(&usd, &eur).unwrap_err().kind,
            UnifyErrorKind::UnitMismatch { .. }
        ));
    }

    #[test]
    fn open_records_bind_the_missing_row() {
        let mut unifier = Unifier::new();
        let row = Var::row(10);
        let left = Term::Record(Record::open([("id", Term::Bool(true))], row.clone()));
        let right = Term::Record(Record::closed([
            ("id", Term::Bool(true)),
            ("amount", quantity(5, "USD")),
        ]));
        unifier.unify(&left, &right).unwrap();
        let tail = unifier.resolve(&Term::Var(row));
        assert_eq!(
            tail,
            Term::Record(Record::closed([("amount", quantity(5, "USD").clone())]))
        );
    }

    #[test]
    fn holes_remain_ambiguous_until_one_candidate_survives() {
        let mut unifier = Unifier::new();
        let hole = Var::hole(4, "account", Sort::Nominal(Some(NominalKind::Account)));
        let checking = Term::Nominal(Nominal::new(NominalKind::Account, "checking"));
        let brokerage = Term::Nominal(Nominal::new(NominalKind::Account, "brokerage"));
        let result = unifier
            .narrow_hole(&hole, [checking.clone(), brokerage.clone()])
            .unwrap();
        assert!(matches!(result, Narrowing::Ambiguous(_)));
        assert!(unifier.substitution(&hole).is_none());
        let result = unifier.narrow_hole(&hole, [checking.clone()]).unwrap();
        assert_eq!(result, Narrowing::Unique(checking.clone()));
        assert_eq!(unifier.substitution(&hole), Some(checking));
    }

    #[test]
    fn sort_constraints_follow_deferred_variable_chains() {
        let mut unifier = Unifier::new();
        let account = Var::hole(1, "account", Sort::Nominal(Some(NominalKind::Account)));
        let middle = Var::inference(2);
        let tail = Var::inference(3);

        unifier
            .unify(&Term::Var(account.clone()), &Term::Var(middle.clone()))
            .unwrap();
        unifier
            .unify(&Term::Var(middle.clone()), &Term::Var(tail.clone()))
            .unwrap();

        let error = unifier
            .unify(&Term::Var(tail.clone()), &Term::Bool(true))
            .unwrap_err();
        assert!(matches!(error.kind, UnifyErrorKind::SortMismatch { .. }));
        assert!(unifier.substitution(&account).is_some());
        assert!(unifier.substitution(&middle).is_some());
        assert!(unifier.substitution(&tail).is_none());
    }

    #[test]
    fn quantity_exactness_does_not_round() {
        let mut unifier = Unifier::new();
        let usd = Unit::new("USD");
        let one_third = Term::Quantity(
            ExactQuantity::new(
                BigRational::new(BigInt::from(1), BigInt::from(3)),
                Some(usd.clone()),
            )
            .unwrap(),
        );
        let two_thirds = Term::Quantity(
            ExactQuantity::new(
                BigRational::new(BigInt::from(2), BigInt::from(3)),
                Some(usd),
            )
            .unwrap(),
        );
        assert!(unifier.unify(&one_third, &two_thirds).is_err());
    }
}
