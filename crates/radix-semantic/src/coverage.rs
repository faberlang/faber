//! Fast `HIR`-to-target classification for corpus-wide coverage scanning.
//!
//! Mirrors the `MIR` target matrix (`radix_module::mir::coverage`) in shape but
//! operates on **typed `HIR`** rather than lowered `MIR`. One in-process pass
//! per exemplum, per target, no external toolchains.
//!
//! Each classifier inspects compiler-owned facts (`HIR` shape + type table)
//! and returns a [`Lowerability`] verdict with named [`CapabilityGap`]s.
//! This is the pre-emission decision layer; deep codegen still validates
//! at emission time, but the matrix uses the classifier result.
//!
//! TARGET: shared lowerability seam for `HIR`-backed application-lane targets
//! (rust, go, typescript, faber round-trip).

use std::collections::VecDeque;

use crate::{NumericWidth, Primitive, Type, TypeId, TypeTable};
use handroll::FxHashSet;
use radix_hir::visit::{HirVisitor, walk_block, walk_expr, walk_local};
use radix_hir::{HirExpression, HirExpressionKind, HirFunction, HirItemKind, HirLocal, HirModule};
use radix_target::Target;
use radix_types::{CapabilityGap, Lowerability};

/// `HIR`-backed application-lane targets covered by the coverage matrix.
///
/// Mirrors `radix_module::mir::MirCoverageTarget` in shape; names follow the
/// existing backend module names. The `MIR` targets live in the systems lane
/// and are not included here — they have their own classifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HirCoverageTarget {
    /// Native Rust source backend (primary application lane).
    Rust,
    /// TypeScript source backend.
    TypeScript,
    /// Go source backend.
    Go,
    /// Canonical Faber source re-emission (round-trip health check).
    Faber,
}

impl HirCoverageTarget {
    /// Display name matching the `MIR` matrix `name()` convention.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::TypeScript => "ts",
            Self::Go => "go",
            Self::Faber => "faber",
        }
    }

    /// All `HIR` coverage targets in canonical order.
    pub const ALL: &[Self] = &[Self::Rust, Self::Go, Self::TypeScript, Self::Faber];

    /// Exhaustive domain anchor for [`ALL`](Self::ALL): one arm per variant,
    /// no wildcard (defect-audit P6-4 / B6-T3).
    ///
    /// Adding a variant fails this match's compilation, and the coverage
    /// tests then fail until `ALL` carries the new variant — the `ALL.len()`
    /// assertion this replaces stayed green when `ALL` lagged the enum. The
    /// tests pin each spelling to [`name`](Self::name) so this anchor and
    /// the display name cannot drift apart.
    #[must_use]
    pub fn universe_spelling(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::TypeScript => "ts",
            Self::Go => "go",
            Self::Faber => "faber",
        }
    }
}

/// Map a [`Target`] to its `HIR` coverage counterpart, if one exists.
///
/// `MIR`-backed targets (wasm, llvm-text, metal-text, wgsl-text, sexp) are
/// systems-lane and classified by the `MIR` matrix. Returns `None` for them.
#[must_use]
pub fn from_target(target: Target) -> Option<HirCoverageTarget> {
    match target {
        Target::MirStepper
        | Target::HirSwift
        | Target::HirPython
        | Target::HirHaskell
        | Target::HirPascal
        | Target::HirCobol
        | Target::HirFortran
        | Target::HirOdin
        | Target::HirC
        | Target::MirWasm
        | Target::MirLlvm
        | Target::MirLlvmHost
        | Target::MirClif
        | Target::MirAmd
        | Target::MirMetal
        | Target::MirWgsl
        | Target::MirSexp
        | Target::MirEvm
        | Target::MirBasic
        | Target::MirScena
        | Target::MirFmir
        | Target::MirFmirBinary
        | Target::HirFhir => None,
        Target::HirRust => Some(HirCoverageTarget::Rust),
        Target::HirTypeScript => Some(HirCoverageTarget::TypeScript),
        Target::HirGo => Some(HirCoverageTarget::Go),
        Target::HirFaber => Some(HirCoverageTarget::Faber),
    }
}

/// Classify typed `HIR` against one `HIR`-backed target.
///
/// Returns [`Lowerability::Capable`] when the target has no known structural
/// rejection for this `HIR`. Returns [`Lowerability::Rejected`] with the named
/// gaps when the target policy or codegen preconditions reject surfaces
/// present in the program.
///
/// This function is pure: no I/O, no mutation, no side effects. It is
/// designed to be called once per exemplum in a corpus-wide scan.
#[must_use]
pub fn classify_hir_coverage(
    target: HirCoverageTarget,
    hir: &HirModule,
    types: &TypeTable,
) -> Lowerability {
    match target {
        HirCoverageTarget::Rust => classify_rust(hir, types),
        HirCoverageTarget::Go => classify_go(hir, types),
        HirCoverageTarget::TypeScript => classify_typescript(hir, types),
        HirCoverageTarget::Faber => classify_faber(hir, types),
    }
}

// ---------------------------------------------------------------------------
// Rust classifier
// ---------------------------------------------------------------------------

/// Rust target rejects a small, well-known set of `HIR` type surfaces that
/// have no Rust representation yet.
///
/// These rejections live in `codegen::reject_rust_unsupported_types` at
/// emission time; this classifier reproduces the same check at the
/// pre-emission boundary so the matrix can count them without running
/// codegen.
fn classify_rust(_hir: &HirModule, types: &TypeTable) -> Lowerability {
    let mut gaps = Vec::new();
    for index in 0..types.type_count() {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "TypeId is u32 by design; type_count is bounded by u32::MAX"
        )]
        let ty = TypeId(index as u32);
        match types.get(ty) {
            Type::SizedNumeric(Primitive::Fractus, NumericWidth::F16) => {
                gaps.push(CapabilityGap::TargetPolicyRejection {
                    shape: "f16".to_owned(),
                    slug: "rust_target_fractus_f16_unsupported",
                });
            }
            Type::SizedNumeric(Primitive::Fractus, NumericWidth::BF16) => {
                gaps.push(CapabilityGap::TargetPolicyRejection {
                    shape: "bf16".to_owned(),
                    slug: "rust_target_fractus_bf16_unsupported",
                });
            }
            Type::Atomic(_) => {
                gaps.push(CapabilityGap::TargetPolicyRejection {
                    shape: "atomic type".to_owned(),
                    slug: "rust_target_atomic_unsupported",
                });
            }
            _ => {}
        }
    }
    if gaps.is_empty() {
        Lowerability::Capable
    } else {
        Lowerability::Rejected(gaps)
    }
}

// ---------------------------------------------------------------------------
// Go classifier
// ---------------------------------------------------------------------------

/// Go target rejects `ad` expression usage and a handful of `HIR` surfaces
/// that have no Go mapping.
///
/// The Go backend rejects `ad` expressions at codegen time
/// (`HirExpressionKind::Ad` → `CodegenError { message: "cannot emit Go for
/// ad expression; Rust-only feature" }`). It also rejects matrix and vector
/// types that have no Go representation (`types::type_to_go` returns
/// `unsupported` for both). Tensor and sparsa are representable in Go
/// (`rt.TensorTensor[...]` / `rt.SparsaSparsa[...]`) and are not rejected here.
fn classify_go(hir: &HirModule, types: &TypeTable) -> Lowerability {
    let mut gaps = Vec::new();

    if has_modular_word(types) {
        gaps.push(CapabilityGap::TargetPolicyRejection {
            shape: "modulus<u32> type".to_owned(),
            slug: "go_target_modular_word_unsupported",
        });
    }

    // WHY: Go target rejects `ad` expressions — the same surfaces that
    // `codegen/go/expr/mod.rs` rejects at emission time. We scan HIR
    // expressions so the matrix catches these without running Go codegen.
    let ad_count = count_ad_expressions(hir);
    if ad_count > 0 {
        gaps.push(CapabilityGap::TargetPolicyRejection {
            shape: format!("{ad_count} ad expression(s)"),
            slug: "go_target_ad_unsupported",
        });
    }

    // WHY: Go does not support matrix or vector types in its mapping. Reject
    // programs that *reference* those families — not units whose interned
    // type table merely contains them. The table is per-unit and carries
    // compiler-owned types the program never uses (the GPU builtins register
    // `vector<f32,size 3>` / `vector<f32,size 4>` into every unit), so a whole-table
    // scan would reject every program for Go.
    for ty in collect_referenced_types(hir, types) {
        match types.get(ty) {
            Type::Matrix(_, _) => {
                gaps.push(CapabilityGap::TargetPolicyRejection {
                    shape: "matrix type".to_owned(),
                    slug: "go_target_matrix_unsupported",
                });
            }
            Type::Vector(_, _) => {
                gaps.push(CapabilityGap::TargetPolicyRejection {
                    shape: "vector type".to_owned(),
                    slug: "go_target_vector_unsupported",
                });
            }
            _ => {}
        }
    }

    if gaps.is_empty() {
        Lowerability::Capable
    } else {
        Lowerability::Rejected(gaps)
    }
}

/// Collect every `TypeId` the program actually references, closed over the
/// interned type graph.
///
/// Roots are the types named by HIR declarations and expressions (function
/// signatures, struct/enum fields, locals, constants, alias targets, and
/// every expression `ty`). The reachability closure then follows type
/// children, so a program that uses `lista<vector<f32,size 3>>` references both
/// the `lista` id and the nested `vector` id.
///
/// WHY: the per-unit `TypeTable` also interns compiler-owned types the
/// program never names (GPU builtins' register-class `vector<f32,size N>` types).
/// Target classifiers that reject a family must reject programs that use it,
/// not every unit whose table happens to contain it.
fn collect_referenced_types(hir: &HirModule, types: &TypeTable) -> FxHashSet<TypeId> {
    let mut roots = FxHashSet::default();

    if let Some(entry) = &hir.entry {
        walk_block(&mut TypeRefCollector { roots: &mut roots }, entry);
    }
    for item in &hir.items {
        match &item.kind {
            HirItemKind::Function(func) => collect_function_types(func, &mut roots),
            HirItemKind::Struct(strukt) => {
                for field in &strukt.fields {
                    roots.insert(field.ty);
                }
                for method in &strukt.methods {
                    collect_function_types(&method.func, &mut roots);
                }
            }
            HirItemKind::Enum(enumeratio) => {
                for variant in &enumeratio.variants {
                    for field in &variant.fields {
                        roots.insert(field.ty);
                    }
                }
            }
            HirItemKind::Interface(interfacium) => {
                for method in &interfacium.methods {
                    for param in &method.params {
                        roots.insert(param.ty);
                    }
                    if let Some(ret) = method.ret_ty {
                        roots.insert(ret);
                    }
                    if let Some(err) = method.err_ty {
                        roots.insert(err);
                    }
                }
            }
            HirItemKind::TypeAlias(alias) => {
                roots.insert(alias.ty);
            }
            HirItemKind::Schema(schema) => {
                for column in &schema.columns {
                    roots.insert(column.ty);
                }
            }
            HirItemKind::Constant(constant) => {
                if let Some(ty) = constant.ty {
                    roots.insert(ty);
                }
                walk_expr(&mut TypeRefCollector { roots: &mut roots }, &constant.value);
            }
            HirItemKind::Import(_) => {}
        }
    }

    let mut seen = roots.clone();
    let mut queue: VecDeque<TypeId> = roots.into_iter().collect();
    while let Some(id) = queue.pop_front() {
        for child in type_children(types.get(id)) {
            if seen.insert(child) {
                queue.push_back(child);
            }
        }
    }
    seen
}

/// Collect the type ids named by one function signature and body.
fn collect_function_types(func: &HirFunction, roots: &mut FxHashSet<TypeId>) {
    for param in &func.params {
        roots.insert(param.ty);
    }
    if let Some(cli) = &func.cli_args {
        roots.insert(cli.ty);
    }
    if let Some(ret) = func.ret_ty {
        roots.insert(ret);
    }
    if let Some(err) = func.err_ty {
        roots.insert(err);
    }
    if let Some(ref body) = func.body {
        walk_block(&mut TypeRefCollector { roots }, body);
    }
}

/// Visitor collecting expression and local binding types into the root set.
struct TypeRefCollector<'a> {
    roots: &'a mut FxHashSet<TypeId>,
}

impl HirVisitor for TypeRefCollector<'_> {
    fn visit_expr(&mut self, expr: &HirExpression) {
        if let Some(ty) = expr.ty {
            self.roots.insert(ty);
        }
        walk_expr(self, expr);
    }

    fn visit_local(&mut self, local: &HirLocal) {
        if let Some(ty) = local.ty {
            self.roots.insert(ty);
        }
        // `walk_local` descends into the initializer, so types referenced by
        // initializer expressions are collected through `visit_expr`.
        walk_local(self, local);
    }
}

/// Direct child `TypeId`s of one interned type.
///
/// Mirrors the child structure in `Type`'s `Hash` impl so the reachability
/// closure stays in lock-step with type identity. Struct/enum/interface
/// members are resolved through HIR declarations (collected as roots), not
/// through the type graph.
fn type_children(ty: &Type) -> Vec<TypeId> {
    match ty {
        Type::Primitive(_)
        | Type::SizedNumeric(_, _)
        | Type::Bits(_)
        | Type::ModularWord(_)
        | Type::SaturatingWord(_)
        | Type::RangedInt { .. }
        | Type::PolicyFloat(_, _)
        | Type::TensorEdgePolicy(_)
        | Type::SizedInstans(_)
        | Type::Param(_)
        | Type::Infer(_)
        | Type::InferUnion(_)
        | Type::MarkerHole { .. }
        | Type::Error
        | Type::Struct(_)
        | Type::Enum(_)
        | Type::Interface(_) => Vec::new(),
        Type::Array(element)
        | Type::Set(element)
        | Type::Promissum(element)
        | Type::Cursor(element)
        | Type::Queue(element)
        | Type::Stack(element)
        | Type::Atomic(element)
        | Type::Intervallum(element, _)
        | Type::Option(element)
        | Type::Ref(_, element)
        | Type::BoundedArray { element, .. }
        | Type::RangedArray { element, .. }
        | Type::BoundedQueue { element, .. }
        | Type::BoundedStack { element, .. } => vec![*element],
        Type::PromissumFailable(first, alternate) | Type::AsyncCursor(first, alternate) => {
            vec![*first, *alternate]
        }
        Type::Map(key, value) | Type::Ledger(key, value) => vec![*key, *value],
        Type::Record(fields) => fields.values().copied().collect(),
        Type::Tensor(element, _)
        | Type::Vector(element, _)
        | Type::Matrix(element, _)
        | Type::Sparsa(element, _) => {
            vec![*element]
        }
        Type::Alias(_, inner) => vec![*inner],
        Type::Func(signature) => {
            let mut children = signature
                .params
                .iter()
                .map(|param| param.ty)
                .collect::<Vec<_>>();
            children.push(signature.ret);
            if let Some(err) = signature.err {
                children.push(err);
            }
            children
        }
        Type::Applied(base, args) => {
            let mut children = vec![*base];
            children.extend(args.iter().filter_map(|arg| match arg {
                radix_types::AppliedArg::Type(id) => Some(*id),
                radix_types::AppliedArg::Size(_) => None,
            }));
            children
        }
        Type::Union(members) | Type::Intersection(members) => members.clone(),
        Type::Tuple(elements) => elements.clone(),
        Type::BoundedTextus { .. } | Type::BoundedAscii { .. } | Type::BoundedOcteti { .. } => {
            Vec::new()
        }
    }
}

// ---------------------------------------------------------------------------
// TypeScript classifier
// ---------------------------------------------------------------------------

/// TypeScript target rejects a small set of `HIR` surfaces that have no
/// TypeScript representation.
///
/// TS is the broadest `HIR` backend in terms of accepted types; its rejections
/// are primarily structural rather than type-surface based. `modulus<W>`
/// words are fully supported since TS4-1 (per-width wrapping emission), so
/// they are not rejected here.
fn classify_typescript(_hir: &HirModule, _types: &TypeTable) -> Lowerability {
    Lowerability::Capable
}

fn has_modular_word(types: &TypeTable) -> bool {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "TypeId is u32 by design; type_count is bounded by u32::MAX"
    )]
    (0..types.type_count())
        .any(|index| matches!(types.get(TypeId(index as u32)), Type::ModularWord(_)))
}

// ---------------------------------------------------------------------------
// Faber classifier
// ---------------------------------------------------------------------------

/// Faber round-trip accepts all typed `HIR`.
///
/// Canonical re-emission has no target-specific rejections; the only
/// rejection path is `reject_hir_errors` which runs before this classifier
/// is reached (the matrix only classifies exemplum rows that passed
/// frontend analysis).
fn classify_faber(_hir: &HirModule, _types: &TypeTable) -> Lowerability {
    Lowerability::Capable
}

// ---------------------------------------------------------------------------
// Expression counting helpers
// ---------------------------------------------------------------------------

/// Count occurrences of `HirExpressionKind::Ad` in the program.
///
/// Walks every function body, constants, and the entry block through the
/// standard block walker, so every expression position is covered: statement
/// exprs, local initializers, `redde` payloads, `custodi` clause guards and
/// bodies, `incdec` targets, and block tail expressions.
fn count_ad_expressions(hir: &HirModule) -> usize {
    let mut counter = AdCounter { count: 0 };
    if let Some(entry) = &hir.entry {
        walk_block(&mut counter, entry);
    }
    for item in &hir.items {
        match &item.kind {
            HirItemKind::Function(func) => {
                if let Some(ref body) = func.body {
                    walk_block(&mut counter, body);
                }
            }
            HirItemKind::Constant(c) => {
                walk_expr(&mut counter, &c.value);
            }
            _ => {}
        }
    }
    counter.count
}

struct AdCounter {
    count: usize,
}

impl HirVisitor for AdCounter {
    fn visit_expr(&mut self, expr: &HirExpression) {
        if matches!(&expr.kind, HirExpressionKind::Ad { .. }) {
            self.count += 1;
        }
        walk_expr(self, expr);
    }
}
