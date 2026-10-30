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
const ISSUE_DISCERNE_PATTERN_NOT_IN_CLOSED_SET: &str = "discerne_pattern_not_in_closed_set";
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
    if let HirPattern::Or(alternatives) = pattern {
        for alternative in alternatives {
            resolve_type_test_target(checker, alternative, expected);
        }
        return None;
    }

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
    /// Substitute an applied scrutinee's generic witnesses into a variant's
    /// declared payload field types.
    ///
    /// `fields` are the definition's field types, which may mention the
    /// parent's `Type::Param` slots. When `expected` is an application of the
    /// same parent, each param is replaced by the corresponding applied
    /// argument (U-D1). Any other scrutinee shape returns the fields as they
    /// were declared.
    fn applied_variant_field_types(
        &mut self,
        parent: DefId,
        expected: TypeId,
        fields: Vec<TypeId>,
    ) -> Vec<TypeId> {
        let resolved = self.resolve_inferred_type(expected);
        let Type::Applied(base, args) = self.types.get(resolved).clone() else {
            return fields;
        };
        if !matches!(self.types.get(base), Type::Enum(found) if *found == parent) {
            return fields;
        }
        let Some(params) = self.type_params_by_def.get(&parent).cloned() else {
            return fields;
        };
        let mut vars = FxHashMap::default();
        for (param, arg) in params.iter().zip(args.iter()) {
            if let radix_types::AppliedArg::Type(witness) = arg {
                vars.insert(*param, *witness);
            }
        }
        if vars.is_empty() {
            return fields;
        }
        // `substitute_type_params` maps each declared param to its applied
        // witness (a param without a witness degrades to a fresh hole, never
        // a rigid binding), so the arm's payload bindings carry the
        // application's field types.
        fields
            .into_iter()
            .map(|field| self.substitute_type_params(field, &mut vars, None))
            .collect()
    }

    /// Whether a type-pattern target belongs to the scrutinee's closed set:
    /// a member of its union (looking through option and alias), or the
    /// scrutinee's own type when it is not a union.
    fn type_pattern_in_closed_set(&mut self, expected: TypeId, target: TypeId) -> bool {
        match self.union_members_of(expected) {
            Some(members) => members
                .into_iter()
                .any(|member| self.member_type_equals(member, target)),
            None => self.member_type_equals(expected, target),
        }
    }

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
            HirPattern::Or(alternatives) => self.check_or_pattern(alternatives, expected, span),
            HirPattern::TypeTest { ty, binding, .. } => {
                let target = self.resolve_inferred_type(*ty);
                let constrained = self.constrain_type_pattern(
                    expected,
                    target,
                    span,
                    ISSUE_PATTERN_TYPE_MISMATCH,
                );
                if !constrained && !self.type_pattern_in_closed_set(expected, target) {
                    // `discerne` discriminates a closed set: a union's members,
                    // an enum's variants, or the one type of a non-union
                    // scrutinee. Any other target type (a numeric width, a
                    // length-shaped collection) is not a member of that set.
                    let pattern = self.format_type_for_diagnostic(target);
                    let scrutinee = self.format_type_for_diagnostic(expected);
                    self.error_issue_with_args(
                        SemanticErrorKind::TypeMismatch,
                        ISSUE_DISCERNE_PATTERN_NOT_IN_CLOSED_SET,
                        span,
                        &[("pattern", pattern), ("scrutinee", scrutinee)],
                    );
                }
                if let Some((def_id, _name)) = binding {
                    self.insert_binding(*def_id, target);
                }
            }
            HirPattern::Alias(def_id, _name, pattern) => {
                self.insert_binding(*def_id, expected);
                if let HirPattern::Variant(variant_def, patterns) = pattern.as_ref()
                    && patterns.is_empty()
                {
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
                    if let Some(expected_parent) = self.enum_def_from_type(expected)
                        && expected_parent != parent
                    {
                        self.error_issue(
                            SemanticErrorKind::TypeMismatch,
                            ISSUE_VARIANT_PATTERN_SCRUTINEE_MISMATCH,
                            span,
                        );
                    }
                    return;
                }
                self.check_pattern(pattern, expected, span);
            }
            // A negated pattern types as the literal it negates; the parser
            // admits only number literals under the sign (D22.2).
            HirPattern::Literal(lit) | HirPattern::NegatedLiteral(lit) => {
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

                if let Some(expected_parent) = self.enum_def_from_type(expected)
                    && expected_parent != parent
                {
                    self.error_issue(
                        SemanticErrorKind::TypeMismatch,
                        ISSUE_VARIANT_PATTERN_SCRUTINEE_MISMATCH,
                        span,
                    );
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
                // U-D1 pattern twin: an applied scrutinee substitutes its
                // witnesses into the declared payload fields, so each binding
                // gets the field's type at this application — never the raw
                // `Type::Param` (the builtin `trapped<R, T>` variants and a
                // user `discretio Cista<T>` ride the same rail).
                let fields = self.applied_variant_field_types(parent, expected, fields);
                for (sub, field_ty) in patterns.iter().zip(fields.iter()) {
                    self.check_pattern(sub, *field_ty, span);
                }
            }
        }
    }
    /// Check every `aut` alternative in an isolated scope, then expose the
    /// common binding environment to the arm body.
    fn check_or_pattern(
        &mut self,
        alternatives: &[HirPattern],
        expected: TypeId,
        span: radix_lexer::Span,
    ) {
        let mut first_bindings: Option<Vec<(DefId, radix_lexer::Symbol, TypeId)>> = None;
        let mut agrees = !alternatives.is_empty();

        for alternative in alternatives {
            self.push_scope();
            self.check_pattern(alternative, expected, span);
            let bindings = or_pattern_bindings(alternative)
                .into_iter()
                .filter_map(|(def_id, name)| {
                    self.scopes
                        .last()
                        .and_then(|scope| scope.get(&def_id))
                        .map(|info| (def_id, name, info.ty))
                })
                .collect::<Vec<_>>();
            self.pop_scope();

            if let Some(first) = &first_bindings {
                agrees &= first.len() == bindings.len()
                    && first.iter().zip(&bindings).all(|(left, right)| {
                        left.1 == right.1 && self.types.equals(left.2, right.2)
                    });
            } else {
                first_bindings = Some(bindings);
            }
        }

        if !agrees {
            self.error_issue(
                SemanticErrorKind::TypeMismatch,
                "or_pattern_binding_mismatch",
                span,
            );
            return;
        }
        for (def_id, _, ty) in first_bindings.unwrap_or_default() {
            self.insert_binding(def_id, ty);
        }
    }
}

fn or_pattern_bindings(pattern: &HirPattern) -> Vec<(DefId, radix_lexer::Symbol)> {
    let mut bindings = Vec::new();
    collect_or_pattern_bindings(pattern, &mut bindings);
    bindings
}

fn collect_or_pattern_bindings(
    pattern: &HirPattern,
    bindings: &mut Vec<(DefId, radix_lexer::Symbol)>,
) {
    match pattern {
        HirPattern::Binding(def_id, name) => bindings.push((*def_id, *name)),
        HirPattern::Alias(def_id, name, inner) => {
            bindings.push((*def_id, *name));
            collect_or_pattern_bindings(inner, bindings);
        }
        HirPattern::TypeTest { binding, .. } => {
            if let Some((def_id, name)) = binding {
                bindings.push((*def_id, *name));
            }
        }
        HirPattern::Variant(_, patterns) => {
            for pattern in patterns {
                collect_or_pattern_bindings(pattern, bindings);
            }
        }
        HirPattern::Or(_)
        | HirPattern::Wildcard
        | HirPattern::Literal(_)
        | HirPattern::NegatedLiteral(_) => {}
    }
}
