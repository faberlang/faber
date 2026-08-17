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

impl TypeChecker<'_> {
    /// Check a pattern against an expected scrutinee or field type.
    ///
    /// The caller owns scope setup. This function only inserts the bindings
    /// introduced by the pattern and reports mismatches. Unknown variant metadata
    /// is treated as a semantic error and checking continues where possible so a
    /// bad arm can still expose additional type or arity diagnostics.
    pub(super) fn check_pattern(&mut self, pattern: &HirPattern, expected: TypeId, span: crate::lexer::Span) {
        match pattern {
            HirPattern::Wildcard => {}
            HirPattern::Binding(def_id, _name) => {
                self.insert_binding(*def_id, expected);
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
                            self.error_issue(SemanticErrorKind::UndefinedVariable, "unknown_variant", span);
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
                    self.error_issue(SemanticErrorKind::UndefinedVariable, "unknown_variant", span);
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
                    self.error_issue(SemanticErrorKind::UndefinedVariable, "unknown_variant_fields", span);
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
