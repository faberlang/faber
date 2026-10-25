//! Index inference core: metavariable allocation, constraint solving, finalize.
//!
//! Parallel to `typecheck/infer.rs` for types. Constraint sources from tensor
//! deployment (constructors, broadcast ops, call-site `magnitudo` bind) land in
//! Stage 5; this module provides the solve/finalize machinery they plug into.

use super::TypeTable;
use super::index::{IndexExpr, IndexId, IndexVar};
use super::index_unify::{IndexUnifier, IndexUnifyError};
use handroll::FxHashMap;
use radix_types::{IndexArithmeticError, add_index_constant, divide_index_exact};

pub const MISSING_TENSOR_SHAPE_MSG: &str = "cannot infer tensor shape";
pub const INCOMPATIBLE_INDEX_MSG: &str = "index shapes are incompatible";
pub const TENSOR_SHAPE_CONVERSIO_MSG: &str =
    "tensor shape conversion is not supported for this pair";
pub const TENSOR_ELEMENT_CONVERSIO_MSG: &str =
    "tensor element-type conversion is not supported for this pair";
pub const TENSOR_TO_SCALAR_CONVERSIO_MSG: &str = "tensor to scalar conversion has no default element pick; use `accipe(index)` for indexed reads";
pub const SCALAR_TO_TENSOR_CONVERSIO_MSG: &str = "scalar to tensor conversion has no default broadcast shape; use `crea(shape, fill)` or `structa`";

/// A constraint between two index terms gathered during typechecking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexConstraint {
    /// Shapes must unify strictly (equal rank, no broadcast stretch).
    Equal(IndexId, IndexId),
    /// Shapes unify under the tensor broadcast rule (size-1 stretch).
    Broadcast(IndexId, IndexId),
}

/// Per-checker index inference state (parallel to `TypeChecker` type inference fields).
#[derive(Debug, Default)]
pub struct IndexInferState {
    infer_ids: FxHashMap<IndexVar, IndexId>,
    substitutions: FxHashMap<IndexVar, IndexId>,
}

impl IndexInferState {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register resolve/lowering inference holes already present in `types`.
    ///
    /// # Panics
    /// Panics when the index table holds more entries than `u32::MAX`.
    pub fn bootstrap_from_types(&mut self, types: &TypeTable) {
        for index in 0..types.index_count() {
            let id = IndexId(u32::try_from(index).expect("index table fits u32"));
            let IndexExpr::Infer(var) = types.get_index(id) else {
                continue;
            };
            self.infer_ids.entry(*var).or_insert(id);
        }
    }

    /// Allocate a fresh index hole (`IndexExpr::Infer`) in the type table.
    pub fn fresh_hole(&mut self, types: &mut TypeTable) -> IndexId {
        let var = types.fresh_index_infer_var();
        let id = types.intern_index(IndexExpr::Infer(var));
        self.infer_ids.insert(var, id);
        id
    }

    /// Map from inference variable to its hole id in the index arena.
    #[must_use]
    pub fn infer_ids(&self) -> &FxHashMap<IndexVar, IndexId> {
        &self.infer_ids
    }

    /// Active substitutions established by unification.
    #[must_use]
    pub fn substitutions(&self) -> &FxHashMap<IndexVar, IndexId> {
        &self.substitutions
    }

    /// Build a unifier that reads/writes this state's substitution map.
    pub fn unifier<'a>(&'a mut self, types: &'a mut TypeTable) -> IndexUnifier<'a> {
        IndexUnifier::new(types, &mut self.substitutions)
    }

    /// Rebind an inference variable to a witness, replacing any earlier
    /// substitution. Size-type Z3: a `_` extent parameter's shared hole is
    /// authoritative at its own function entry, so a stale binding from an
    /// earlier call's unification is overwritten with the formal runtime
    /// axis instead of failing a equality check.
    pub fn rebind(&mut self, var: IndexVar, id: IndexId) {
        self.substitutions.insert(var, id);
    }

    /// Resolve an index through the substitution map.
    pub fn resolve(&mut self, types: &mut TypeTable, id: IndexId) -> IndexId {
        self.unifier(types).resolve(id)
    }

    /// Extract the inference variable at the root of an index term, if any.
    #[must_use]
    pub fn infer_var_of(&self, types: &TypeTable, id: IndexId) -> Option<IndexVar> {
        match types.get_index(id) {
            IndexExpr::Infer(var) => Some(*var),
            _ => None,
        }
    }

    /// Whether `id` is still an unresolved inference hole after substitution chase.
    pub fn is_unresolved_hole(&mut self, types: &mut TypeTable, id: IndexId) -> bool {
        let resolved = self.resolve(types, id);
        self.infer_var_of(types, resolved).is_some()
    }

    /// Whether `id` or any nested arithmetic/tuple term remains unresolved.
    pub fn contains_unresolved_hole(&mut self, types: &mut TypeTable, id: IndexId) -> bool {
        let resolved = self.resolve(types, id);
        match types.get_index(resolved).clone() {
            IndexExpr::Infer(_) => true,
            IndexExpr::Tuple(dims) => dims
                .into_iter()
                .any(|dim| self.contains_unresolved_hole(types, dim)),
            IndexExpr::Affine { base, .. } => self.contains_unresolved_hole(types, base),
            IndexExpr::Divide { numerator, .. } => self.contains_unresolved_hole(types, numerator),
            _ => false,
        }
    }

    /// Apply constraints; returns `false` if any constraint is incompatible.
    pub fn solve_constraints(
        &mut self,
        types: &mut TypeTable,
        constraints: &[IndexConstraint],
    ) -> bool {
        let mut ok = true;
        for constraint in constraints {
            let mut unifier = self.unifier(types);
            let result = match *constraint {
                IndexConstraint::Equal(a, b) => unifier.unify(a, b),
                IndexConstraint::Broadcast(a, b) => unifier.unify_broadcast(a, b),
            };
            if result.is_err() {
                ok = false;
            }
        }
        ok
    }

    /// Rebuild compound index shapes so nested holes reflect current substitutions.
    pub fn canonicalize(&mut self, types: &mut TypeTable, id: IndexId) -> IndexId {
        let resolved = self.resolve(types, id);
        match types.get_index(resolved).clone() {
            IndexExpr::Infer(var) => {
                if let Some(subst) = self.substitutions.get(&var).copied() {
                    self.canonicalize(types, subst)
                } else {
                    resolved
                }
            }
            IndexExpr::Tuple(dims) => {
                let canonical = dims
                    .iter()
                    .map(|dim| self.canonicalize(types, *dim))
                    .collect();
                types.intern_index(IndexExpr::Tuple(canonical))
            }
            IndexExpr::Affine { base, offset } => {
                let base = self.canonicalize(types, base);
                add_index_constant(types, base, offset)
                    .unwrap_or_else(|_| types.intern_index(IndexExpr::Affine { base, offset }))
            }
            IndexExpr::Divide { numerator, divisor } => {
                let numerator = self.canonicalize(types, numerator);
                divide_index_exact(types, numerator, divisor).unwrap_or_else(|_| {
                    types.intern_index(IndexExpr::Divide { numerator, divisor })
                })
            }
            _ => resolved,
        }
    }

    /// Finalize an index term; returns `Err` when a hole remains unsolved.
    ///
    /// # Errors
    ///
    /// Returns [`IndexInferError::UnboundHole`] when the index term still
    /// contains an unresolved inference variable after substitution and
    /// canonicalization.
    pub fn finalize(
        &mut self,
        types: &mut TypeTable,
        id: IndexId,
    ) -> Result<IndexId, IndexInferError> {
        let canonical = self.canonicalize(types, id);
        if let Some(error) = index_arithmetic_error(canonical, types) {
            return Err(IndexInferError::InvalidArithmetic(error));
        }
        if self.contains_unresolved_hole(types, canonical) {
            Err(IndexInferError::UnboundHole)
        } else {
            Ok(canonical)
        }
    }

    /// Classify a constraint failure for diagnostics.
    ///
    /// Callers invoke this only after [`Self::solve_constraints`] reports failure;
    /// a successful unification here maps to [`IndexInferError::Mismatch`] as a
    /// conservative fallback when no binding was recorded.
    pub fn constraint_error(
        &mut self,
        types: &mut TypeTable,
        constraint: IndexConstraint,
    ) -> IndexInferError {
        let mut unifier = self.unifier(types);
        let classify = |result: Result<IndexId, IndexUnifyError>| match result {
            Err(IndexUnifyError::Occurs) => IndexInferError::Occurs,
            Err(IndexUnifyError::Mismatch) | Ok(_) => IndexInferError::Mismatch,
        };
        match constraint {
            IndexConstraint::Equal(a, b) => classify(unifier.unify(a, b)),
            IndexConstraint::Broadcast(a, b) => classify(unifier.unify_broadcast(a, b)),
        }
    }
}

fn index_arithmetic_error(id: IndexId, types: &TypeTable) -> Option<IndexArithmeticError> {
    match types.get_index(id) {
        IndexExpr::Tuple(dims) => dims
            .iter()
            .find_map(|dim| index_arithmetic_error(*dim, types)),
        IndexExpr::Affine { base, .. } => radix_types::evaluate_index_dimension(id, types)
            .err()
            .or_else(|| index_arithmetic_error(*base, types)),
        IndexExpr::Divide { numerator, .. } => radix_types::evaluate_index_dimension(id, types)
            .err()
            .or_else(|| index_arithmetic_error(*numerator, types)),
        _ => None,
    }
}

/// Index inference failure at finalize or constraint generation time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexInferError {
    UnboundHole,
    Mismatch,
    Occurs,
    InvalidArithmetic(IndexArithmeticError),
}
