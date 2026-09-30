//! The unbounded integer `inf` inside the type checker (inf-delivery
//! rulings 32, 34–41).
//!
//! `inf` is `SizedNumeric(Numerus, Inf)`: an integer width with no interval.
//! The range rule, the store rule and the conversion table each answer it
//! explicitly (`integer_range.rs`, `store.rs`, `convert.rs`); this module
//! holds the shared predicates.

use super::TypeChecker;
use radix_types::TypeId;

impl TypeChecker<'_> {
    /// Whether `ty` is the unbounded integer (through aliases, access markers
    /// and solved inference holes).
    pub(super) fn is_inf_type(&self, ty: TypeId) -> bool {
        let resolved = self.resolve_inferred_type(ty);
        let (_, resolved) = self.types.peel_access_marker(resolved);
        radix_types::numeric_width::is_unbounded_integer_type(self.types, resolved)
    }
}
