//! Exhaustiveness analysis for typed `discerne` expressions.
//!
//! This pass runs after typechecking has annotated HIR expressions and after
//! the optional Rust borrow pass in the default Rust pipeline. It is target
//! configurable through `PassConfig`, but its contract is target-neutral:
//! every match — any number of scrutinees — covers every value of the
//! scrutinee product or provides a catchall arm (D4.8).
//!
//! The pass depends on two earlier semantic products. Lowering provides HIR
//! pattern structure with variant `DefId`s, while the `TypeTable` identifies
//! the scrutinee's enum definition even through aliases, references, and
//! applied generic types. Enum variant lists come from local HIR items plus
//! the resolver's imported and builtin enums. Coverage itself is the
//! pattern-matrix computation in [`super::coverage`].
//!
//! ERROR STRATEGY
//! ==============
//! - Missing coverage is emitted as `NonExhaustiveMatch` with a `missing`
//!   argument naming one uncovered value or combination.
//! - Duplicate variant arms and arms after a catchall are emitted as semantic
//!   diagnostics using their dedicated pattern kinds (single scrutinee).
//! - A match whose pattern kinds coverage cannot be computed for gets the
//!   `CoverageUnchecked` warning — never silence.
//! - Untyped scrutinees and pattern-arity mismatches are owned by earlier
//!   passes and skipped here.
//!
//! INVARIANTS
//! ==========
//! - Arms carry no guards (D4.1): every arm's patterns contribute coverage.
//! - `bivalens` is the closed set `{verum, falsum}`.
//! - Variant coverage is collected by enum definition `DefId`, not by textual
//!   variant name, so later codegen can rely on resolved identity.

use super::coverage::{Coverage, Ctor, Space, Witness, coverage, space_for};
use crate::locale::{LocalePack, TypeBindingKind};
use crate::{
    Primitive, Resolver, SemanticError, SemanticErrorKind, Type, TypeId, TypeTable, WarningKind,
};
use handroll::{FxHashMap, FxHashSet};
use radix_hir::visit::{HirVisitor, walk_expr};
use radix_hir::{
    DefId, HirCasuArm, HirExpression, HirExpressionKind, HirId, HirItemKind, HirModule, HirPattern,
};
use radix_lexer::Interner;

/// Name sources for rendering a coverage witness in the reader's locale.
pub struct CoverageNames<'a> {
    pub interner: &'a Interner,
    pub resolver: &'a Resolver,
    pub locale_pack: Option<&'a LocalePack>,
}

/// Check target-enabled `discerne` coverage and pattern reachability.
///
/// Findings are hard semantic diagnostics (missing coverage, duplicate variant
/// arms, arms made unreachable by a previous catchall) plus warnings (null
/// absorbed by a catchall, coverage not computable). The semantic driver
/// appends them to the shared result stream and decides success by
/// `SemanticError::is_error`. `enum_variants` is the closed variant list of
/// every enum in scope; `elige_ids` marks the matches authored as `elige`
/// (see [`check_coverage`]); `names`, when present, spells witnesses for
/// readers.
///
/// # Errors
///
/// Returns non-exhaustive match and unreachable pattern errors as a list of
/// [`SemanticError`]s.
pub fn check(
    hir: &HirModule,
    types: &TypeTable,
    enum_variants: &FxHashMap<DefId, Vec<DefId>>,
    elige_ids: &FxHashSet<HirId>,
    names: Option<&CoverageNames<'_>>,
) -> Result<(), Vec<SemanticError>> {
    let mut checker = ExhaustiveChecker {
        types,
        enum_variants,
        elige_ids,
        names,
        errors: Vec::new(),
    };
    checker.visit_program(hir);

    if checker.errors.is_empty() {
        Ok(())
    } else {
        Err(checker.errors)
    }
}

/// Variant lists of every enum in scope: local HIR items plus the resolver's
/// imported and builtin enums.
#[must_use]
pub fn collect_all_enum_variants(
    hir: &HirModule,
    resolver: &Resolver,
) -> FxHashMap<DefId, Vec<DefId>> {
    let mut map = collect_enum_variants(hir);
    for (enum_def, variants) in resolver.non_local_enum_variants() {
        map.entry(enum_def).or_insert(variants);
    }
    map
}

/// Variant lists of the enums this unit's HIR declares.
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
    enum_variants: &'a FxHashMap<DefId, Vec<DefId>>,
    elige_ids: &'a FxHashSet<HirId>,
    names: Option<&'a CoverageNames<'a>>,
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
                self.names,
                &mut self.errors,
            );
            let is_elige = self.elige_ids.contains(&expr.id);
            if !is_elige {
                check_multi_subject_closed_sets(scrutinees, self.types, &mut self.errors);
            }
            check_coverage(
                scrutinees,
                arms,
                is_elige,
                self.types,
                self.enum_variants,
                self.names,
                &mut self.errors,
            );
        }
        walk_expr(self, expr);
    }
}

/// Every subject of a multi-subject `discerne` must be a closed set (enum,
/// union, `bivalens`): the match is a product of closed sets, and a value
/// subject (number, text, collection) belongs to `elige` (operator ruling
/// 2026-10-02). A single-subject match keeps its pattern-level checks.
fn check_multi_subject_closed_sets(
    scrutinees: &[HirExpression],
    types: &TypeTable,
    errors: &mut Vec<SemanticError>,
) {
    if scrutinees.len() < 2 {
        return;
    }
    for (position, subject) in scrutinees.iter().enumerate() {
        if subject.ty.is_some_and(|ty| is_value_subject(ty, types)) {
            errors.push(
                SemanticError::structured(SemanticErrorKind::TypeMismatch, subject.span)
                    .with_arg("issue", "discerne_subject_not_in_closed_set")
                    .with_arg("position", (position + 1).to_string()),
            );
        }
    }
}

/// A value type (numeric, text, bytes, collection): an open set with no
/// closed-set constructors. Generic, inferred, unknown, product (struct,
/// tuple) and interface subjects are left to coverage.
fn is_value_subject(ty: TypeId, types: &TypeTable) -> bool {
    match types.get(ty) {
        Type::Alias(_, inner) | Type::Ref(_, inner) => is_value_subject(*inner, types),
        Type::Primitive(primitive) => matches!(
            primitive,
            Primitive::Ascii
                | Primitive::Textus
                | Primitive::Numerus
                | Primitive::Fractus
                | Primitive::Octeti
                | Primitive::Regex
                | Primitive::Json
                | Primitive::Valor
                | Primitive::Instans
        ),
        Type::SizedNumeric(..)
        | Type::ModularWord(_)
        | Type::SaturatingWord(_)
        | Type::RangedInt { .. }
        | Type::PolicyFloat(..)
        | Type::SizedInstans(_)
        | Type::BoundedTextus { .. }
        | Type::BoundedAscii { .. }
        | Type::BoundedOcteti { .. }
        | Type::Array(_)
        | Type::BoundedArray { .. }
        | Type::RangedArray { .. }
        | Type::Queue(_)
        | Type::BoundedQueue { .. }
        | Type::Stack(_)
        | Type::BoundedStack { .. }
        | Type::Map(..)
        | Type::Set(_)
        | Type::Tensor(..)
        | Type::Vector(..)
        | Type::Matrix(..) => true,
        _ => false,
    }
}

fn check_match(
    scrutinees: &[HirExpression],
    arms: &[HirCasuArm],
    exhaustive: bool,
    types: &TypeTable,
    names: Option<&CoverageNames<'_>>,
    errors: &mut Vec<SemanticError>,
) {
    if exhaustive && check_omnia_contract(scrutinees, arms, types, errors) {
        return;
    }

    if scrutinees.len() != 1 {
        return;
    }

    let Some(scrutinee_ty) = scrutinees[0].ty else {
        return;
    };

    let mut covered = FxHashSet::default();
    let mut covered_members = Vec::new();
    let mut covered_literals = FxHashSet::default();
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

        for pattern in arm.patterns.iter().flat_map(pattern_alternatives) {
            if let Some(def_id) = pattern_variant_id(pattern) {
                if !covered.insert(def_id) {
                    errors.push(
                        SemanticError::structured(SemanticErrorKind::PatternDuplicate, arm.span)
                            .with_arg("issue", "duplicate_variant"),
                    );
                }
            } else if let Some(member) = pattern_type_id(pattern) {
                if covered_members
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
            } else if let Some((key, spelling)) = literal_pattern_value(pattern, names) {
                if !covered_literals.insert(key) {
                    errors.push(
                        SemanticError::structured(SemanticErrorKind::PatternDuplicate, arm.span)
                            .with_arg("issue", "duplicate_literal")
                            .with_arg("value", spelling),
                    );
                }
            } else if is_catchall_pattern(pattern) {
                has_catchall = true;
                catchall_span = Some(arm.span);
            }
        }
    }

    // WHY: Faber's canonical nullable form is `T ∪ nihil` (AGENTS.md rule 5).
    // A catchall silently swallows the null case with no signal,
    // so flag it and steer authors toward an explicit `casu nihil` arm. This
    // is a warning, not an error: existing catchalls keep compiling. Once an
    // explicit null arm exists, a trailing catchall legitimately covers only
    // the remaining non-null values, so the warning is suppressed.
    if has_catchall
        && !explicit_null_arm
        && scrutinee_has_null_constituent(scrutinee_ty, types)
        && let Some(span) = catchall_span
    {
        errors.push(
            SemanticError::structured(
                SemanticErrorKind::Warning(WarningKind::WildcardAbsorbsNull),
                span,
            )
            .with_arg("issue", "wildcard_absorbs_null")
            .with_arg("casu", "casu"),
        );
    }
}

// `omnia` enforcement: the keyword is only valid over a single enum
// scrutinee where every variant can be named explicitly. For any other
// shape (multi-scrutinee, non-enum, untyped) the contract is ill-defined —
// there is no finite variant set to enumerate (e.g. matching every
// character of a `textus` is meaningless). These are hard errors, not
// warnings, because `omnia` is an explicit author promise.
//
// Returns `true` when the match was rejected and the caller must stop.
fn check_omnia_contract(
    scrutinees: &[HirExpression],
    arms: &[HirCasuArm],
    types: &TypeTable,
    errors: &mut Vec<SemanticError>,
) -> bool {
    if scrutinees.len() != 1 {
        errors.push(
            SemanticError::structured(SemanticErrorKind::OmniaRequiresEnum, scrutinees[0].span)
                .with_arg("issue", "single_enum_scrutinee")
                .with_arg("keyword", "omnia"),
        );
        return true;
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
                SemanticError::structured(SemanticErrorKind::OmniaRequiresEnum, scrutinees[0].span)
                    .with_arg("issue", "enum_scrutinee")
                    .with_arg("keyword", "omnia"),
            );
        }
        return true;
    }
    // Every arm must name explicit variants only — no `casu _`, no bare
    // binding, no `ceterum` (which lowers to a wildcard arm), and no mixed
    // arm like `casu A, _` whose wildcard silently absorbs the remaining
    // variants. `any` (not `all`) catches a catchall hiding alongside named
    // variants in the same arm; `all` would let `casu A, _` slip through and
    // defeat the omnia invariant.
    for arm in arms {
        if !arm.patterns.is_empty() && arm.patterns.iter().any(is_catchall_pattern) {
            errors.push(
                SemanticError::structured(SemanticErrorKind::OmniaForbidsCatchall, arm.span)
                    .with_arg("issue", "catchall_arm")
                    .with_arg("keyword", "omnia"),
            );
        }
    }
    false
}

pub(crate) fn union_members_from_type(ty: TypeId, types: &TypeTable) -> Option<Vec<TypeId>> {
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
/// an uncovered value. It is the same pattern-matrix computation the
/// exhaustiveness pass reports from, so an exhaustive `bivalens` or
/// multi-scrutinee match definitely returns exactly when it is exhaustive.
/// Uncomputable or upstream-owned shapes answer "not covered", which is sound
/// for the consumer: the enclosing function is treated as not definitely
/// returning.
pub fn discerne_definitely_covered(
    scrutinees: &[HirExpression],
    arms: &[HirCasuArm],
    types: &TypeTable,
    enum_variants: &FxHashMap<DefId, Vec<DefId>>,
) -> bool {
    matches!(
        coverage(scrutinees, arms, types, enum_variants),
        Coverage::Covered
    )
}

/// Report missing or uncomputable coverage for one match.
///
/// `elige` is a value switch: without `ceterum`, a value outside its cases
/// runs no arm (targets emit an empty default arm). Over an open space —
/// numbers, text — that implicit default is the construct's meaning, not a
/// gap, so an `elige` whose only witness is "any other value" is accepted.
/// Closed spaces (enum variants, `verum`/`falsum`, union members) are checked
/// for `elige` exactly as for `discerne`.
fn check_coverage(
    scrutinees: &[HirExpression],
    arms: &[HirCasuArm],
    is_elige: bool,
    types: &TypeTable,
    enum_variants: &FxHashMap<DefId, Vec<DefId>>,
    names: Option<&CoverageNames<'_>>,
    errors: &mut Vec<SemanticError>,
) {
    let Some(first) = scrutinees.first() else {
        return;
    };
    match coverage(scrutinees, arms, types, enum_variants) {
        Coverage::Covered | Coverage::Skipped => {}
        Coverage::Unchecked => errors.push(
            SemanticError::structured(
                SemanticErrorKind::Warning(WarningKind::CoverageUnchecked),
                first.span,
            )
            .with_arg("issue", "coverage_unchecked"),
        ),
        Coverage::Missing(witness) if is_elige && witness.iter().all(Option::is_none) => {}
        Coverage::Missing(witness) => {
            let issue = match (scrutinees, witness.first()) {
                ([only], _) => match only.ty.map(|ty| space_for(ty, types, enum_variants)) {
                    Some(Space::Enum(_)) => "missing_variants",
                    Some(Space::Union(_)) => "missing_union_members",
                    _ => "missing_cases",
                },
                _ => "missing_cases",
            };
            errors.push(
                SemanticError::structured(SemanticErrorKind::NonExhaustiveMatch, first.span)
                    .with_arg("issue", issue)
                    .with_arg("missing", render_witness(&witness, types, names)),
            );
        }
    }
}

/// Spell a witness: one value for a single scrutinee, `(a, b)` for several.
fn render_witness(
    witness: &Witness,
    types: &TypeTable,
    names: Option<&CoverageNames<'_>>,
) -> String {
    let parts: Vec<String> = witness
        .iter()
        .map(|ctor| render_ctor(*ctor, types, names))
        .collect();
    match parts.as_slice() {
        [only] => only.clone(),
        _ => format!("({})", parts.join(", ")),
    }
}

/// Reader-locale surface spelling for the null *type* (D23.2: en `none`,
/// la `nihil`), falling back to the Latin spelling when no pack is active.
fn nil_type_surface(names: Option<&CoverageNames<'_>>) -> String {
    names.and_then(|names| names.locale_pack).map_or_else(
        || "nihil".to_owned(),
        |pack| {
            pack.type_surface(TypeBindingKind::Primitive(Primitive::Nihil))
                .to_owned()
        },
    )
}

fn render_ctor(ctor: Option<Ctor>, types: &TypeTable, names: Option<&CoverageNames<'_>>) -> String {
    let keyword = |latin: &'static str| -> String {
        names
            .and_then(|names| names.locale_pack)
            .map_or(latin, |pack| pack.contextual_keyword_surface(latin))
            .to_owned()
    };
    let def_name = |def_id: DefId| -> String {
        names
            .and_then(|names| {
                let symbol = names.resolver.get_symbol(def_id)?;
                Some(names.interner.resolve(symbol.name).to_owned())
            })
            .unwrap_or_else(|| format!("Def{}", def_id.0))
    };
    match ctor {
        None => "_".to_owned(),
        Some(Ctor::Bool(true)) => keyword("verum"),
        Some(Ctor::Bool(false)) => keyword("falsum"),
        Some(Ctor::Variant(def_id)) => def_name(def_id),
        Some(Ctor::Member(member)) if type_is_nil(member, types) => nil_type_surface(names),
        Some(Ctor::Member(member)) => match names {
            Some(names) => crate::format_faber_type(member, types, names.interner, &def_name),
            None => format!("Type{}", member.0),
        },
    }
}

/// Extract the enum variant a pattern targets, descending through aliases.
#[must_use]
pub fn pattern_variant_id(pattern: &HirPattern) -> Option<DefId> {
    match pattern {
        HirPattern::Variant(def_id, _)
        | HirPattern::TypeTest {
            candidate: Some(def_id),
            ..
        } => Some(*def_id),
        HirPattern::Alias(_, _, inner) => pattern_variant_id(inner),
        _ => None,
    }
}

/// Whether a pattern is a catchall (`_`, a bare binding, or an alias wrapping
/// either).
fn is_catchall_pattern(pattern: &HirPattern) -> bool {
    match pattern {
        HirPattern::Wildcard | HirPattern::Binding(_, _) => true,
        HirPattern::Alias(_, _, inner) => is_catchall_pattern(inner),
        HirPattern::Or(alternatives) => alternatives.iter().any(is_catchall_pattern),
        HirPattern::Variant(_, _)
        | HirPattern::TypeTest { .. }
        | HirPattern::Literal(_)
        | HirPattern::NegatedLiteral(_) => false,
    }
}

/// `casu nihil` lowers to a literal-nil pattern; an occurrence
/// counts as an explicit null arm and suppresses the wildcard-absorb warning.
fn is_null_literal_pattern(pattern: &HirPattern) -> bool {
    match pattern {
        HirPattern::Literal(radix_hir::HirLiteral::Nil) => true,
        HirPattern::Alias(_, _, inner) => is_null_literal_pattern(inner),
        _ => false,
    }
}

/// Whether a scrutinee type carries a null possibility that a
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

/// Identity key and source spelling of a value-literal pattern, so a repeated
/// `case` value is rejected at check instead of reaching MIR switch lowering.
///
/// Only the numeric and text literals a value dispatch can switch on are
/// keyed; boolean, null, and other literal kinds are covered by the coverage
/// matrix.
fn literal_pattern_value(
    pattern: &HirPattern,
    names: Option<&CoverageNames<'_>>,
) -> Option<(String, String)> {
    use radix_hir::HirLiteral;
    let (negated, literal) = match pattern {
        HirPattern::Literal(literal) => (false, literal),
        HirPattern::NegatedLiteral(literal) => (true, literal),
        _ => return None,
    };
    let sign = if negated { "-" } else { "" };
    let (kind, spelling) = match literal {
        HirLiteral::Int(value) => ("int", value.to_string()),
        HirLiteral::BigInt(digits) => ("int", digits.to_string()),
        HirLiteral::Float(value) => ("float", value.to_string()),
        HirLiteral::FloatText { text, .. } => ("float", names?.interner.resolve(*text).to_owned()),
        HirLiteral::String(symbol) => ("string", format!("{:?}", names?.interner.resolve(*symbol))),
        HirLiteral::Ascii(symbol) => ("ascii", format!("{:?}", names?.interner.resolve(*symbol))),
        _ => return None,
    };
    let spelling = format!("{sign}{spelling}");
    Some((format!("{kind}:{spelling}"), spelling))
}

fn pattern_alternatives(pattern: &HirPattern) -> Vec<&HirPattern> {
    match pattern {
        HirPattern::Or(alternatives) => alternatives.iter().collect(),
        _ => vec![pattern],
    }
}
