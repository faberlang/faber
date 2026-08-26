//! Pattern typing and binding for `discerne` arms.
//!
//! Patterns sit on the boundary between value typing and lexical binding. They
//! do not synthesize an independent type; instead, every pattern is checked
//! against the scrutinee or enum-field type supplied by the caller, and any
//! bindings introduced by the pattern inherit that expected type. This keeps
//! match arm bindings aligned with the value being destructured instead of
//! creating a second inference problem inside the pattern tree.
//!
//! INVARIANTS
//! ==========
//! - Pattern bindings are immutable and scoped by the enclosing match arm.
//! - Literal patterns must unify with the scrutinee type.
//! - Variant patterns validate the enum parent when the scrutinee type is known
//!   and validate arity against the collected variant field table.
//! - Alias patterns bind the whole matched value before checking the nested
//!   pattern, so `name @ Variant(...)`-style forms preserve both views.

#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for typechecking
use super::*;

const ISSUE_PATTERN_TYPE_MISMATCH: &str = "pattern_type_mismatch";
const ISSUE_VARIANT_PATTERN_SCRUTINEE_MISMATCH: &str = "variant_pattern_scrutinee_mismatch";

/// Select the effective member for a type-test pattern.
///
/// The parser preserves a same-spelled variant candidate. The scrutinee owns
/// the final decision: a candidate that is a member of this scrutinee wins;
/// otherwise the ordinary named type remains the predicate target.
pub fn resolve_type_test_target(
    checker: &mut TypeChecker<'_>,
    pattern: &mut HirPattern,
    expected: TypeId,
) -> Option<TypeId> {
    let HirPattern::TypeTest {
        ty,
        candidate,
        binding: _,
    } = pattern
    else {
        return None;
    };

    let named_ty = checker.resolve_inferred_type(*ty);
    let Some(candidate_def) = *candidate else {
        return Some(named_ty);
    };
    let candidate_ty = checker.types.intern(Type::Struct(candidate_def));
    let candidate_is_member = checker.union_members_of(expected).is_some_and(|members| {
        members
            .iter()
            .copied()
            .any(|member| checker.member_type_equals(member, candidate_ty))
    });
    if candidate_is_member {
        *ty = candidate_ty;
        return Some(candidate_ty);
    }

    // The named type wins when the candidate belongs to another union. Clear
    // the candidate so every emitter sees one settled HIR representation.
    *candidate = None;
    Some(named_ty)
}

impl TypeChecker<'_> {
    /// Check a pattern against an expected scrutinee or field type.
    ///
    /// The caller owns scope setup. This function only inserts the bindings
    /// introduced by the pattern and reports mismatches. Unknown variant metadata
    /// is treated as a semantic error and checking continues where possible so a
    /// bad arm can still expose additional type or arity diagnostics.
    pub fn check_pattern(
        &mut self,
        pattern: &HirPattern,
        expected: TypeId,
        span: radix_lexer::Span,
    ) {
        match pattern {
            HirPattern::Wildcard => {}
            HirPattern::Binding(def_id, _name) => {
                self.insert_binding(*def_id, expected);
            }
            HirPattern::TypeTest { ty, binding, .. } => {
                let target = self.resolve_inferred_type(*ty);
                let constrained = self.constrain_type_pattern(
                    expected,
                    target,
                    span,
                    ISSUE_PATTERN_TYPE_MISMATCH,
                );
                let width_applies =
                    radix_types::width_pattern_applies(self.types, expected, target);
                if !constrained && !width_applies {
                    if let Some(members) = self.union_members_of(expected) {
                        let is_member = members
                            .iter()
                            .copied()
                            .any(|member| self.member_type_equals(member, target));
                        if !is_member {
                            self.error_issue(
                                SemanticErrorKind::TypeMismatch,
                                ISSUE_PATTERN_TYPE_MISMATCH,
                                span,
                            );
                        }
                    } else if !self.member_type_equals(expected, target) {
                        self.error_issue(
                            SemanticErrorKind::TypeMismatch,
                            ISSUE_PATTERN_TYPE_MISMATCH,
                            span,
                        );
                    }
                }
                if let Some((def_id, _name)) = binding {
                    // A width guard narrows the runtime value without changing
                    // its carrier type. Keep the expected carrier in the arm.
                    self.insert_binding(*def_id, if width_applies { expected } else { target });
                }
            }
            HirPattern::Alias(def_id, _name, pattern) => {
                self.insert_binding(*def_id, expected);
                if matches!(pattern.as_ref(), HirPattern::Binding(_, _)) {
                    return;
                }
                if let HirPattern::Variant(variant_def, patterns) = pattern.as_ref() {
                    if patterns.is_empty() {
                        // Unit-variant aliases can validate the enum parent
                        // without recursing into an empty destructuring shape.
                        let Some(parent) = self.variant_parent.get(variant_def).copied() else {
                            self.error_issue(
                                SemanticErrorKind::VariableUndefined,
                                "unknown_variant",
                                span,
                            );
                            return;
                        };
                        if let Some(expected_parent) = self.enum_def_from_type(expected) {
                            if expected_parent != parent {
                                self.error_issue(
                                    SemanticErrorKind::TypeMismatch,
                                    ISSUE_VARIANT_PATTERN_SCRUTINEE_MISMATCH,
                                    span,
                                );
                            }
                        }
                        return;
                    }
                }
                self.check_pattern(pattern, expected, span);
            }
            HirPattern::Literal(lit) => {
                let lit_ty = self.literal_type(lit, span, Some(expected));
                self.unify_assign_issue(lit_ty, expected, span, ISSUE_PATTERN_TYPE_MISMATCH);
            }
            HirPattern::Variant(variant_def, patterns) => {
                let Some(parent) = self.variant_parent.get(variant_def).copied() else {
                    self.error_issue(
                        SemanticErrorKind::VariableUndefined,
                        "unknown_variant",
                        span,
                    );
                    return;
                };

                if let Some(expected_parent) = self.enum_def_from_type(expected) {
                    if expected_parent != parent {
                        self.error_issue(
                            SemanticErrorKind::TypeMismatch,
                            ISSUE_VARIANT_PATTERN_SCRUTINEE_MISMATCH,
                            span,
                        );
                    }
                }

                let Some(fields) = self.variant_fields.get(variant_def).cloned() else {
                    self.error_issue(
                        SemanticErrorKind::VariableUndefined,
                        "unknown_variant_fields",
                        span,
                    );
                    return;
                };
                if fields.len() != patterns.len() {
                    self.wrong_arity("variant_pattern_arity_mismatch", span);
                }
                for (sub, field_ty) in patterns.iter().zip(fields.iter()) {
                    self.check_pattern(sub, *field_ty, span);
                }
            }
        }
    }
}
