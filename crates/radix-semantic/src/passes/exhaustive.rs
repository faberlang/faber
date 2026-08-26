//! Exhaustiveness analysis for typed `discerne` expressions.
//!
//! This pass runs after typechecking has annotated HIR expressions and after
//! the optional Rust borrow pass in the default Rust pipeline. It is target
//! configurable through `PassConfig`, but its contract is target-neutral:
//! single-scrutinee enum matches should either cover every known variant or
//! provide an unguarded catchall arm.
//!
//! The pass depends on two earlier semantic products. Lowering provides HIR
//! pattern structure with variant `DefId`s, while the `TypeTable` identifies
//! the scrutinee's enum definition even through aliases, references, and
//! applied generic types. No name resolution happens here.
//!
//! ERROR STRATEGY
//! ==============
//! - Missing enum coverage is emitted as `NonExhaustiveMatch`.
//! - Duplicate variant arms and arms after a catchall are emitted as semantic
//!   diagnostics using their dedicated pattern kinds.
//! - Multi-scrutinee matches, untyped scrutinees, and non-enum scrutinees are
//!   skipped instead of guessed; missing type information is owned by an
//!   earlier pass, not recovered here.
//!
//! INVARIANTS
//! ==========
//! - Guarded arms never contribute to exhaustiveness because their guard may
//!   evaluate to false at runtime.
//! - Catchall detection is structural and unguarded: wildcard and binding
//!   patterns close coverage for the current single-scrutinee match.
//! - Variant coverage is collected by enum definition `DefId`, not by textual
//!   variant name, so later codegen can rely on resolved identity.

use crate::{Primitive, SemanticError, SemanticErrorKind, Type, TypeId, TypeTable, WarningKind};
use radix_hir::visit::{walk_expr, HirVisitor};
use radix_hir::{
    DefId, HirCasuArm, HirExpression, HirExpressionKind, HirItemKind, HirModule, HirPattern,
};
use rustc_hash::{FxHashMap, FxHashSet};

/// Check target-enabled `discerne` coverage and pattern reachability.
///
/// The current findings are hard semantic diagnostics: missing enum coverage,
/// duplicate variant arms, and arms made unreachable by a previous catchall.
/// The semantic driver appends them to the shared result stream and decides
/// success by `SemanticError::is_error`.
///
/// # Errors
///
/// Returns non-exhaustive match and unreachable pattern errors as a list of
/// [`SemanticError`]s.
pub fn check(hir: &HirModule, types: &TypeTable) -> Result<(), Vec<SemanticError>> {
    let enum_variants = collect_enum_variants(hir);
    let mut checker = ExhaustiveChecker {
        types,
        enum_variants,
        errors: Vec::new(),
    };
    checker.visit_program(hir);

    if checker.errors.is_empty() {
        Ok(())
    } else {
        Err(checker.errors)
    }
}

#[must_use]
pub fn collect_enum_variants(hir: &HirModule) -> FxHashMap<DefId, Vec<DefId>> {
    let mut map = FxHashMap::default();
    for item in &hir.items {
        if let HirItemKind::Enum(enum_item) = &item.kind {
            let variants = enum_item.variants.iter().map(|v| v.def_id).collect();
            map.insert(item.def_id, variants);
        }
    }
    map
}

/// Visitor state for a single exhaustiveness run.
///
/// `enum_variants` is captured before expression traversal so each match check
/// can stay local: resolve the scrutinee's enum definition through the type
/// table, then compare arm patterns with the known variant set.
struct ExhaustiveChecker<'a> {
    types: &'a TypeTable,
    enum_variants: FxHashMap<DefId, Vec<DefId>>,
    errors: Vec<SemanticError>,
}

impl HirVisitor for ExhaustiveChecker<'_> {
    fn visit_expr(&mut self, expr: &HirExpression) {
        if let HirExpressionKind::Discerne {
            scrutinees,
            arms,
            exhaustive,
        } = &expr.kind
        {
            check_match(
                scrutinees,
                arms,
                *exhaustive,
                self.types,
                &self.enum_variants,
                &mut self.errors,
            );
        }
        walk_expr(self, expr);
    }
}

fn check_match(
    scrutinees: &[HirExpression],
    arms: &[HirCasuArm],
    exhaustive: bool,
    types: &TypeTable,
    enum_variants: &FxHashMap<DefId, Vec<DefId>>,
    errors: &mut Vec<SemanticError>,
) {
    // `omnia` enforcement: the keyword is only valid over a single enum
    // scrutinee where every variant can be named explicitly. For any other
    // shape (multi-scrutinee, non-enum, untyped) the contract is ill-defined —
    // there is no finite variant set to enumerate (e.g. matching every
    // character of a `textus` is meaningless). These are hard errors, not
    // warnings, because `omnia` is an explicit author promise.
    if exhaustive {
        if scrutinees.len() != 1 {
            errors.push(
                SemanticError::structured(SemanticErrorKind::OmniaRequiresEnum, scrutinees[0].span)
                    .with_arg("issue", "single_enum_scrutinee")
                    .with_arg("keyword", "omnia"),
            );
            return;
        }
        if scrutinees[0]
            .ty
            .and_then(|ty| enum_def_from_type(ty, types))
            .is_none()
        {
            // If the scrutinee is untyped, upstream typecheck owns the
            // diagnostic; otherwise the type exists but is not an enum.
            if scrutinees[0].ty.is_some() {
                errors.push(
                    SemanticError::structured(
                        SemanticErrorKind::OmniaRequiresEnum,
                        scrutinees[0].span,
                    )
                    .with_arg("issue", "enum_scrutinee")
                    .with_arg("keyword", "omnia"),
                );
            }
            return;
        }
        // Every arm must name explicit variants only — no `casu _`, no bare
        // binding, no `ceterum` (which lowers to a wildcard arm), and no mixed
        // arm like `casu A, _` whose wildcard silently absorbs the remaining
        // variants. `any` (not `all`) catches a catchall hiding alongside named
        // variants in the same arm; `all` would let `casu A, _` slip through and
        // defeat the omnia invariant. Guards do not exempt a catchall.
        for arm in arms {
            if !arm.patterns.is_empty() && arm.patterns.iter().any(is_catchall_pattern) {
                errors.push(
                    SemanticError::structured(SemanticErrorKind::OmniaForbidsCatchall, arm.span)
                        .with_arg("issue", "catchall_arm")
                        .with_arg("keyword", "omnia"),
                );
            }
        }
    }

    if scrutinees.len() != 1 {
        return;
    }

    let Some(scrutinee_ty) = scrutinees[0].ty else {
        return;
    };

    let enum_def = enum_def_from_type(scrutinee_ty, types);
    let expected_variants = enum_def.and_then(|def_id| enum_variants.get(&def_id));
    let expected_members = union_members_from_type(scrutinee_ty, types);

    let mut covered = FxHashSet::default();
    let mut covered_members = Vec::new();
    // Width patterns form an ordered specificity lattice. A later bounded
    // pattern is unreachable after a covering bound or family catch-all, while
    // a family pattern remains reachable after a narrower bounded arm.
    let mut covered_width_patterns = Vec::new();
    let mut has_catchall = false;
    let mut catchall_span = None;
    let mut explicit_null_arm = false;

    for arm in arms {
        if has_catchall {
            errors.push(
                SemanticError::structured(SemanticErrorKind::UnreachablePattern, arm.span)
                    .with_arg("issue", "after_catchall"),
            );
            continue;
        }

        if arm.patterns.is_empty() {
            continue;
        }

        let is_guarded = arm.guard.is_some();
        for pattern in &arm.patterns {
            if let Some(def_id) = pattern_variant_id(pattern) {
                // F-022: guarded arms never contribute to exhaustiveness
                // because their guard may evaluate to false at runtime.
                if is_guarded {
                    continue;
                }
                if !covered.insert(def_id) {
                    errors.push(
                        SemanticError::structured(SemanticErrorKind::PatternDuplicate, arm.span)
                            .with_arg("issue", "duplicate_variant"),
                    );
                }
            } else if let Some(member) = pattern_type_id(pattern) {
                if is_guarded {
                    continue;
                }
                if covered_width_patterns
                    .iter()
                    .copied()
                    .any(|covered| radix_types::width_pattern_covers(types, covered, member))
                {
                    errors.push(
                        SemanticError::structured(SemanticErrorKind::UnreachablePattern, arm.span)
                            .with_arg("issue", "after_width_pattern"),
                    );
                    continue;
                }
                if let Some(width) = radix_types::width_pattern(types, member) {
                    if width.bound.is_some() {
                        covered_width_patterns.push(member);
                    } else {
                        // The unbounded family is the least-specific pattern,
                        // but it covers every member of that family from here.
                        covered_width_patterns.push(member);
                    }
                } else if covered_members
                    .iter()
                    .copied()
                    .any(|covered| types.equals(covered, member))
                {
                    errors.push(
                        SemanticError::structured(SemanticErrorKind::PatternDuplicate, arm.span)
                            .with_arg("issue", "duplicate_type_pattern"),
                    );
                } else {
                    covered_members.push(member);
                }
                if type_is_nil(member, types) {
                    explicit_null_arm = true;
                }
            } else if is_null_literal_pattern(pattern) {
                explicit_null_arm = true;
            } else if is_catchall_pattern(pattern) && !is_guarded {
                has_catchall = true;
                catchall_span = Some(arm.span);
            }
        }
    }

    // WHY: Faber's canonical nullable form is `T ∪ nihil` (AGENTS.md rule 5).
    // An unguarded catchall silently swallows the null case with no signal,
    // so flag it and steer authors toward an explicit `casu nihil` arm. This
    // is a warning, not an error: existing catchalls keep compiling. Once an
    // explicit null arm exists, a trailing catchall legitimately covers only
    // the remaining non-null values, so the warning is suppressed.
    if has_catchall && !explicit_null_arm && scrutinee_has_null_constituent(scrutinee_ty, types) {
        if let Some(span) = catchall_span {
            errors.push(
                SemanticError::structured(
                    SemanticErrorKind::Warning(WarningKind::WildcardAbsorbsNull),
                    span,
                )
                .with_arg("issue", "wildcard_absorbs_null")
                .with_arg("casu", "casu")
                .with_arg("nil", "nihil"),
            );
        }
    }

    if !has_catchall {
        if let Some(expected) = expected_variants {
            let missing = expected.iter().any(|def_id| !covered.contains(def_id));
            if missing {
                errors.push(
                    SemanticError::structured(
                        SemanticErrorKind::NonExhaustiveMatch,
                        scrutinees[0].span,
                    )
                    .with_arg("issue", "missing_variants"),
                );
            }
        } else if let Some(expected) = expected_members {
            let missing = expected.iter().any(|member| {
                !covered_members
                    .iter()
                    .copied()
                    .any(|covered| types.equals(covered, *member))
                    && !covered_width_patterns
                        .iter()
                        .copied()
                        .any(|covered| radix_types::width_pattern_covers(types, covered, *member))
            });
            if missing {
                errors.push(
                    SemanticError::structured(
                        SemanticErrorKind::NonExhaustiveMatch,
                        scrutinees[0].span,
                    )
                    .with_arg("issue", "missing_union_members"),
                );
            }
        }
    }
}

fn union_members_from_type(ty: TypeId, types: &TypeTable) -> Option<Vec<TypeId>> {
    match types.get(ty) {
        Type::Union(members) => Some(members.clone()),
        Type::Option(inner) => {
            let mut members =
                union_members_from_type(*inner, types).unwrap_or_else(|| vec![*inner]);
            members.push(types.primitive(Primitive::Nihil));
            Some(members)
        }
        Type::Alias(_, inner) | Type::Ref(_, inner) | Type::Applied(inner, _) => {
            union_members_from_type(*inner, types)
        }
        _ => None,
    }
}

fn pattern_type_id(pattern: &HirPattern) -> Option<TypeId> {
    match pattern {
        HirPattern::TypeTest {
            ty,
            candidate: None,
            ..
        } => Some(*ty),
        HirPattern::Alias(_, _, inner) => pattern_type_id(inner),
        _ => None,
    }
}

fn type_is_nil(ty: TypeId, types: &TypeTable) -> bool {
    matches!(types.get(ty), Type::Primitive(Primitive::Nihil))
}

pub fn enum_def_from_type(ty: TypeId, types: &TypeTable) -> Option<DefId> {
    match types.get(ty) {
        Type::Enum(def_id) => Some(*def_id),
        Type::Applied(base, _) => enum_def_from_type(*base, types),
        Type::Alias(_, resolved) => enum_def_from_type(*resolved, types),
        Type::Ref(_, inner) => enum_def_from_type(*inner, types),
        _ => None,
    }
}

/// Whether every possible scrutinee value is matched by some reachable arm.
///
/// This is the coverage input consumed by the return-path pass: a `discerne`
/// can only "definitely return" when control cannot fall out the bottom for
/// an uncovered value. Coverage is provable in two cases, mirroring the
/// exhaustiveness pass's coverage logic without depending on its diagnostics:
///
/// - an unguarded catchall arm (Wildcard or Binding) is present, closing the
///   match for any scrutinee type; or
/// - a single enum scrutinee where every variant is covered by an unguarded
///   variant arm.
///
/// Multi-scrutinee matches, non-enum scrutinees without a catchall, and
/// guarded-only coverage are reported as not-covered. This is sound for the
/// return-path consumer: "not covered" means the match may fall through, so
/// the enclosing function is treated as not definitely returning. The
/// exhaustiveness pass remains the authority on whether missing coverage is a
/// hard error; this helper only answers the yes/no reachability question.
pub fn discerne_definitely_covered(
    scrutinees: &[HirExpression],
    arms: &[HirCasuArm],
    types: &TypeTable,
    enum_variants: &FxHashMap<DefId, Vec<DefId>>,
) -> bool {
    // An unguarded catchall closes coverage for any scrutinee type.
    let has_unguarded_catchall = arms
        .iter()
        .any(|arm| arm.guard.is_none() && arm.patterns.iter().any(is_catchall_pattern));
    if has_unguarded_catchall {
        return true;
    }

    // Otherwise coverage is only provable for a single enum scrutinee whose
    // variants are all covered by unguarded variant arms.
    if scrutinees.len() != 1 {
        return false;
    }
    let Some(scrutinee_ty) = scrutinees[0].ty else {
        return false;
    };
    let covered: Vec<TypeId> = arms
        .iter()
        .filter(|arm| arm.guard.is_none())
        .flat_map(|arm| arm.patterns.iter())
        .filter_map(pattern_type_id)
        .collect();
    if let Some(expected) = union_members_from_type(scrutinee_ty, types) {
        return expected.iter().all(|member| {
            covered.iter().copied().any(|candidate| {
                types.equals(candidate, *member)
                    || radix_types::width_pattern_covers(types, candidate, *member)
            })
        });
    }
    if covered
        .iter()
        .copied()
        .any(|candidate| radix_types::width_pattern_covers(types, candidate, scrutinee_ty))
    {
        return true;
    }

    let Some(enum_def) = enum_def_from_type(scrutinee_ty, types) else {
        return false;
    };
    let Some(expected) = enum_variants.get(&enum_def) else {
        // F-077: imported enum — variant list is not available from local HIR.
        // Derive the expected set from the arm patterns: every arm must be
        // unguarded and contain at least one variant pattern with no catchall
        // or binding patterns. If all arms are pure variant arms, the discerne
        // is "definitely covered" with respect to the subset of variants the
        // author explicitly listed.
        return imported_enum_arms_all_variant(arms);
    };

    let mut covered: FxHashSet<DefId> = FxHashSet::default();
    for arm in arms {
        if arm.guard.is_some() {
            continue;
        }
        for pattern in &arm.patterns {
            if let Some(def_id) = pattern_variant_id(pattern) {
                covered.insert(def_id);
            }
        }
    }
    expected.iter().all(|def_id| covered.contains(def_id))
}

/// F-077: for imported enums where we lack the full variant list, derive the
/// "expected" set from the arm patterns. Every arm must be unguarded and carry
/// at least one variant pattern without any catchall or binding patterns.
/// Guarded arms and catchalls prevent definitive coverage because they leave
/// paths untested at runtime.
fn imported_enum_arms_all_variant(arms: &[radix_hir::HirCasuArm]) -> bool {
    use radix_hir::HirPattern;
    if arms.is_empty() {
        return false;
    }
    for arm in arms {
        if arm.guard.is_some() {
            return false;
        }
        let has_variant = arm
            .patterns
            .iter()
            .any(|p| matches!(p, HirPattern::Variant(_, _)));
        let has_catchall = arm.patterns.iter().any(is_catchall_pattern);
        // Every arm must have at least one variant pattern and no catchall.
        if !has_variant || has_catchall {
            return false;
        }
    }
    true
}

/// Extract the enum variant a pattern targets, descending through aliases.
#[must_use]
pub fn pattern_variant_id(pattern: &HirPattern) -> Option<DefId> {
    match pattern {
        HirPattern::Variant(def_id, _) => Some(*def_id),
        HirPattern::TypeTest {
            candidate: Some(def_id),
            ..
        } => Some(*def_id),
        HirPattern::Alias(_, _, inner) => pattern_variant_id(inner),
        _ => None,
    }
}

/// Whether a pattern is an unguarded catchall (`_`, a bare binding, or an
/// alias wrapping either). Guards are checked by callers; this only answers
/// the pattern-shape question.
fn is_catchall_pattern(pattern: &HirPattern) -> bool {
    match pattern {
        HirPattern::Wildcard | HirPattern::Binding(_, _) => true,
        HirPattern::Alias(_, _, inner) => is_catchall_pattern(inner),
        HirPattern::Variant(_, _) | HirPattern::TypeTest { .. } | HirPattern::Literal(_) => false,
    }
}

/// `casu nihil` lowers to a literal-nil pattern; an unguarded occurrence
/// counts as an explicit null arm and suppresses the wildcard-absorb warning.
fn is_null_literal_pattern(pattern: &HirPattern) -> bool {
    match pattern {
        HirPattern::Literal(radix_hir::HirLiteral::Nil) => true,
        HirPattern::Alias(_, _, inner) => is_null_literal_pattern(inner),
        _ => false,
    }
}

/// Whether a scrutinee type carries a null possibility that an unguarded
/// catchall would silently absorb. Nullable is written `T ∪ nihil` in Faber
/// and modeled as `Option(_)` or any `Union` containing `nihil`, possibly
/// wrapped in aliases, references, or applied generic arguments.
fn scrutinee_has_null_constituent(ty: TypeId, types: &TypeTable) -> bool {
    match types.get(ty) {
        Type::Option(_) => true,
        Type::Union(members) => {
            let nihil = types.primitive(Primitive::Nihil);
            members.contains(&nihil)
        }
        Type::Alias(_, resolved) => scrutinee_has_null_constituent(*resolved, types),
        Type::Ref(_, inner) => scrutinee_has_null_constituent(*inner, types),
        Type::Applied(base, _) => scrutinee_has_null_constituent(*base, types),
        _ => false,
    }
}
