//! Generic unification over type-level index terms.
//!
//! Strict [`IndexUnifier::unify`] requires equal ranks and pairwise-compatible
//! dimensions. [`IndexUnifier::unify_broadcast`] implements the tensor
//! size-1-stretch rule (NumPy-style trailing alignment with left padding by
//! literal `1`). Inference metavariables bind through a checker-local
//! substitution map, parallel to `typecheck/infer.rs`.
//!
//! WHY: Broadcasting is shape unification, not a separate runtime policy. The
//! engine is index-kind-agnostic; tensor deployment consumes it in later stages.

use super::index::{IndexExpr, IndexId, IndexVar};
use super::TypeTable;
use rustc_hash::FxHashMap;

/// Result of an index unification attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexUnifyError {
    /// Shapes or dimensions are incompatible under the requested policy.
    Mismatch,
    /// Binding would create a cyclic index substitution.
    Occurs,
}

/// Checker-local index unifier over a shared [`TypeTable`] index arena.
pub struct IndexUnifier<'a> {
    types: &'a mut TypeTable,
    substitutions: &'a mut FxHashMap<IndexVar, IndexId>,
    /// BO-S-6: memo for `occurs_in` by `(IndexVar, resolved IndexId)`.
    occurs_cache: FxHashMap<(IndexVar, IndexId), bool>,
}

impl<'a> IndexUnifier<'a> {
    pub fn new(types: &'a mut TypeTable, substitutions: &'a mut FxHashMap<IndexVar, IndexId>) -> Self {
        Self { types, substitutions, occurs_cache: FxHashMap::default() }
    }

    /// Established index inference substitutions (read-only).
    #[must_use]
    pub fn substitutions(&self) -> &FxHashMap<IndexVar, IndexId> {
        self.substitutions
    }

    /// Resolve an index through the substitution map.
    ///
    /// BO-S-2: path-compress intermediate `IndexVar` substitutions after finding
    /// the representative (amortized O(α(n)) subsequent reads).
    pub fn resolve(&mut self, id: IndexId) -> IndexId {
        let mut current = id;
        let mut path: Vec<IndexVar> = Vec::new();
        let mut seen = 0u32;
        let limit = u32::try_from(self.types.index_count()).expect("index table fits u32");
        while let IndexExpr::Infer(var) = self.types.get_index(current) {
            let var = *var;
            let Some(next) = self.substitutions.get(&var).copied() else {
                break;
            };
            if next == current {
                break;
            }
            path.push(var);
            current = next;
            seen += 1;
            if seen > limit {
                break;
            }
        }
        let representative = current;
        if !path.is_empty() {
            for var in path {
                if self.substitutions.get(&var).copied() != Some(representative) {
                    self.substitutions.insert(var, representative);
                }
            }
        }
        representative
    }

    /// Strict structural unification (equal rank; no size-1 stretch).
    ///
    /// # Errors
    ///
    /// Returns [`IndexUnifyError::Mismatch`] when shapes or dimensions are
    /// incompatible, or [`IndexUnifyError::Occurs`] when binding would create
    /// a cyclic substitution.
    pub fn unify(&mut self, a: IndexId, b: IndexId) -> Result<IndexId, IndexUnifyError> {
        let a = self.resolve(a);
        let b = self.resolve(b);
        if a == b {
            return Ok(a);
        }

        match (self.types.get_index(a).clone(), self.types.get_index(b).clone()) {
            (IndexExpr::Infer(var), _) => self.bind(var, b),
            (_, IndexExpr::Infer(var)) => self.bind(var, a),
            (IndexExpr::Unspecified, IndexExpr::Unspecified) => Ok(a),
            (IndexExpr::Literal(la), IndexExpr::Literal(lb)) if la == lb => Ok(a),
            (IndexExpr::Param(pa), IndexExpr::Param(pb)) if pa == pb => Ok(a),
            (IndexExpr::Tuple(a_dims), IndexExpr::Tuple(b_dims)) => {
                if a_dims.len() != b_dims.len() {
                    return Err(IndexUnifyError::Mismatch);
                }
                let mut unified = Vec::with_capacity(a_dims.len());
                for (a_dim, b_dim) in a_dims.iter().zip(b_dims.iter()) {
                    unified.push(self.unify(*a_dim, *b_dim)?);
                }
                Ok(self.types.intern_index(IndexExpr::Tuple(unified)))
            }
            _ => Err(IndexUnifyError::Mismatch),
        }
    }

    /// Broadcast unification: trailing alignment, left-pad with literal `1`, stretch size-1 dims.
    ///
    /// # Errors
    ///
    /// Returns [`IndexUnifyError::Mismatch`] when shapes or dimensions are
    /// incompatible under the broadcast rule, or [`IndexUnifyError::Occurs`]
    /// when binding would create a cyclic substitution.
    pub fn unify_broadcast(&mut self, a: IndexId, b: IndexId) -> Result<IndexId, IndexUnifyError> {
        let a = self.resolve(a);
        let b = self.resolve(b);
        if a == b {
            return Ok(a);
        }

        match (self.types.get_index(a).clone(), self.types.get_index(b).clone()) {
            (IndexExpr::Infer(var), _) => self.bind(var, b),
            (_, IndexExpr::Infer(var)) => self.bind(var, a),
            (IndexExpr::Unspecified, IndexExpr::Unspecified) => Ok(a),
            (IndexExpr::Unspecified, _) => Ok(b),
            (_, IndexExpr::Unspecified) => Ok(a),
            (IndexExpr::Tuple(a_dims), IndexExpr::Tuple(b_dims)) => {
                let rank = a_dims.len().max(b_dims.len());
                let mut unified = Vec::with_capacity(rank);
                for pos in 0..rank {
                    let a_dim = self.padded_dim(&a_dims, rank, pos);
                    let b_dim = self.padded_dim(&b_dims, rank, pos);
                    unified.push(self.unify_dim_broadcast(a_dim, b_dim)?);
                }
                Ok(self.types.intern_index(IndexExpr::Tuple(unified)))
            }
            (IndexExpr::Tuple(_), IndexExpr::Literal(_)) | (IndexExpr::Literal(_), IndexExpr::Tuple(_)) => {
                Err(IndexUnifyError::Mismatch)
            }
            (IndexExpr::Literal(la), IndexExpr::Literal(lb)) if la == lb => Ok(a),
            (IndexExpr::Literal(_), IndexExpr::Literal(_)) => Err(IndexUnifyError::Mismatch),
            (IndexExpr::Param(pa), IndexExpr::Param(pb)) if pa == pb => Ok(a),
            _ => Err(IndexUnifyError::Mismatch),
        }
    }

    fn padded_dim(&mut self, dims: &[IndexId], rank: usize, pos: usize) -> IndexId {
        let pad = rank.saturating_sub(dims.len());
        if pos < pad {
            self.literal_one()
        } else {
            dims[pos - pad]
        }
    }

    fn unify_dim_broadcast(&mut self, a: IndexId, b: IndexId) -> Result<IndexId, IndexUnifyError> {
        let a = self.resolve(a);
        let b = self.resolve(b);
        if a == b {
            return Ok(a);
        }

        match (self.types.get_index(a).clone(), self.types.get_index(b).clone()) {
            (IndexExpr::Infer(var), _) => self.bind(var, b),
            (_, IndexExpr::Infer(var)) => self.bind(var, a),
            (IndexExpr::Literal(1), _) => Ok(b),
            (_, IndexExpr::Literal(1)) => Ok(a),
            (IndexExpr::Literal(la), IndexExpr::Literal(lb)) if la == lb => Ok(a),
            (IndexExpr::Param(pa), IndexExpr::Param(pb)) if pa == pb => Ok(a),
            _ => self.unify(a, b),
        }
    }

    fn literal_one(&mut self) -> IndexId {
        self.types.intern_index(IndexExpr::Literal(1))
    }

    fn bind(&mut self, var: IndexVar, target: IndexId) -> Result<IndexId, IndexUnifyError> {
        let target = self.resolve(target);
        if let Some(existing) = self.substitutions.get(&var).copied() {
            let existing = self.resolve(existing);
            return self.unify(existing, target);
        }
        if self.occurs_in(var, target) {
            return Err(IndexUnifyError::Occurs);
        }
        self.occurs_cache.clear();
        self.substitutions.insert(var, target);
        Ok(target)
    }

    /// BO-S-6: cache per `(IndexVar, resolved IndexId)`; no dims.clone().
    fn occurs_in(&mut self, var: IndexVar, id: IndexId) -> bool {
        let id = self.resolve(id);
        if let Some(&cached) = self.occurs_cache.get(&(var, id)) {
            return cached;
        }
        let result = match self.types.get_index(id) {
            IndexExpr::Infer(found) => *found == var,
            IndexExpr::Tuple(dims) => {
                let dims = dims.clone();
                dims.into_iter().any(|dim| self.occurs_in(var, dim))
            }
            _ => false,
        };
        self.occurs_cache.insert((var, id), result);
        result
    }
}

#[cfg(test)]
#[path = "index_unify_test.rs"]
mod tests;
