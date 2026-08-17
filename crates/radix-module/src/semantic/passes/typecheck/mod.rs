//! HIR type inference and checking for Radix semantic analysis.
//!
//! This module owns the compiler phase that turns resolved HIR into typed HIR.
//! It runs after collection, name resolution, and AST-to-HIR lowering have
//! already assigned stable `DefId`s and lowered source type syntax into the
//! shared semantic [`TypeTable`]. Typecheck may synthesize additional table
//! entries while inferring expressions, but every `TypeId` it reads or writes is
//! table-local to the single `TypeTable` passed through semantic analysis.
//!
//! COMPILER PHASE: Semantic typecheck
//! INPUT: HIR with resolved `DefIds` but no type information
//! OUTPUT: HIR with finalized expression/declaration `TypeId`s, or collected
//! diagnostics that explain why a type could not be trusted
//!
//! DESIGN PHILOSOPHY
//! =================
//! Typecheck is intentionally local and bidirectional. Expressions normally
//! synthesize a type from their children; surrounding constructs may also pass an
//! expected type downward so empty collections, call arguments, blocks, and
//! branch bodies can be checked against declared context. This is not HM-style
//! whole-program inference: function parameters and other boundary contracts
//! remain explicit so diagnostics point at the source contract that failed.
//!
//! INFERENCE MODEL
//! ===============
//! Fresh [`InferVar`] entries stand for unknown expression types during a single
//! `TypeChecker` run. They live in the shared type table as ordinary `TypeId`s,
//! but the authoritative substitution map is held by `TypeChecker`; callers must
//! use `resolve_inferred_type` before making policy decisions. Finalization walks the HIR
//! after checking and replaces resolved inference variables with concrete
//! `TypeId`s. Unresolved inference or error states produce diagnostics instead
//! of changing the language's nullability, escape, or coercion rules.
//!
//! NULLABILITY AND ESCAPES
//! =======================
//! `nihil` participates in optional and coalescing rules; it is not the same as
//! an unknown type. `ignotum` remains the explicit escape hatch for interop or
//! intentionally unchecked values. The checker follows [`TypeTable::assignable`]
//! for widening, optional acceptance, union membership, and the one-way
//! `ignotum` policy, then uses targeted operator/call/access checks for syntax
//! whose rules are stricter than general assignment.
//!
//! UNIFICATION
//! ===========
//! `unify(a, b)` is the central compatibility operation. It resolves existing
//! substitutions, binds inference variables with an occurs check, structurally
//! unifies collection/function/reference shapes, allows numeric widening and
//! assignment-compatible flows, and records a type mismatch when no rule applies.
//! It returns `Type::Error` through the table after hard failures so later checks
//! can continue and report additional source errors without trusting bad types.
//!
//! ERROR RECOVERY
//! ==============
//! Typecheck collects errors and keeps traversing. The checker uses a dedicated
//! table-local error type, remembers lowered error expressions so they are only
//! reported once, and finalizes unresolved inference variables as missing
//! annotations. This preserves the HIR shape for downstream diagnostics while
//! preventing codegen from treating speculative types as proven.

use crate::codegen::Target;
use crate::hir::{
    DefId, HirArrayElement, HirAvailability, HirBinOp, HirBlock, HirCape, HirCasuArm, HirConversioTarget,
    HirExpression, HirExpressionKind, HirFunction, HirId, HirItem, HirItemKind, HirLiteral, HirLocal, HirModule,
    HirObjectField, HirObjectKey, HirParam, HirParamMode, HirPattern, HirStatement, HirStatementKind, HirStruct,
};
use crate::lexer::{Interner, Symbol};
use crate::locale::LocalePack;
use crate::semantic::{
    types::InferVar, FuncSig, IndexExpr, IndexInferState, NumericWidth, ParamType, Primitive, Resolver, SemanticError,
    SemanticErrorKind, SemanticParamMode, Type, TypeId, TypeParamConstraint, TypeTable,
};
use rustc_hash::{FxHashMap, FxHashSet};
use std::cell::RefCell;

mod access;
mod aggregate;
mod call;
mod collect;
mod control;
mod convert;
mod expr;
mod finalize;
mod generic;
mod index_infer;
mod infer;
mod intervallum;
mod intrinsics;
mod item;
mod json_genus;
mod lookup;
mod ops;
mod pattern;
mod scalar_text;
mod stmt;
mod tensor_index;
mod tensor_type_directed;

#[cfg(test)]
#[path = "generic_params_test.rs"]
mod generic_params_test;

#[cfg(test)]
#[path = "generic_call_test.rs"]
mod generic_call_test;

#[cfg(test)]
#[path = "generic_construction_test.rs"]
mod generic_construction_test;

// Lane-gated: compiles through the full driver to the Rust target; a
// core-only build excludes the module.
#[cfg(all(test, feature = "hir-rust"))]
#[path = "scalar_longitudo_test.rs"]
mod scalar_longitudo_test;

#[cfg(test)]
#[path = "type_hole_union_infer_test.rs"]
mod type_hole_union_infer_test;

#[cfg(test)]
#[path = "null_narrowing_test.rs"]
mod null_narrowing_tests;

/// Type information for a value currently visible in lexical scope.
#[derive(Clone, Copy)]
struct BindingInfo {
    /// Table-local type of the binding after declaration or pattern checking.
    ty: TypeId,
    /// Proven single-scalar `textus`/`ascii` (literal, unit `sectio`, predicate param, or interval bound param).
    scalar_text: bool,
    /// Whether the binding may be mutated ( `varia` local or `in` parameter ).
    mutable: bool,
}

/// Struct member information collected before expression checking begins.
struct StructInfo {
    /// Field contracts keyed by lowered field symbol.
    fields: FxHashMap<Symbol, StructFieldInfo>,

    /// Method signatures keyed by method name; receiver handling is performed
    /// by lookup/call checking rather than encoded in the signature table.
    methods: FxHashMap<Symbol, FuncSig>,
}

/// Interface method contract plus availability metadata.
#[derive(Clone)]
struct InterfaceMethodInfo {
    sig: FuncSig,
    availability: Vec<HirAvailability>,
    span: crate::lexer::Span,
}

/// Field contract used for struct literals and field access checks.
#[derive(Clone, Copy)]
struct StructFieldInfo {
    /// Declared field type in the shared type table.
    ty: TypeId,

    /// Whether ordinary field access produces an optional value.
    optional: bool,

    /// Whether a struct literal must provide this field explicitly.
    required: bool,

    /// Declaration span used when reporting missing required fields.
    span: crate::lexer::Span,
}

/// Current alternate-exit context for `iace` and handled failable calls.
#[derive(Clone, Copy)]
enum ErrorSink {
    /// Function-level alternate-exit type declared on the current function.
    Function(TypeId),

    /// Local handler type introduced by `fac/cape` or handled blocks.
    Local(TypeId),
}

/// Mutable state for one HIR typecheck run.
///
/// The checker is both a visitor and an inference engine. Collection tables hold
/// declarations discovered before expression checking, while the scope stack
/// tracks lexical bindings introduced by blocks and patterns. Inference state is
/// deliberately separate from the shared type table: `Type::Infer` values are
/// stored in the table, but their substitutions are only meaningful through this
/// checker instance.
struct TypeChecker<'a> {
    /// Resolver snapshot from previous semantic passes. Kept with the checker so
    /// future lookup logic can consult resolved symbols without widening method
    /// signatures.
    resolver: &'a Resolver,

    /// Lexer-local symbol table used when typechecking name-sensitive stdlib
    /// method contracts. Symbols are not stable across lexer runs.
    interner: Option<&'a Interner>,

    /// Shared semantic type arena. All `TypeId`s read or written here are local
    /// to this table and must not be compared against IDs from another run.
    types: &'a mut TypeTable,

    /// Hard typechecking diagnostics collected while traversal continues.
    errors: Vec<SemanticError>,

    /// Lexical binding stack keyed by resolver `DefId`, innermost scope last.
    scopes: Vec<FxHashMap<DefId, BindingInfo>>,

    /// Nearest visible binding per `DefId` across the scope stack (O(1) lookup).
    /// Updated on insert; restored or removed on pop.
    binding_shadow: FxHashMap<DefId, BindingInfo>,

    /// Function signatures collected before body checking so calls can be
    /// checked regardless of declaration order.
    functions: FxHashMap<DefId, FuncSig>,

    /// Constant declaration types available to path lookup.
    consts: FxHashMap<DefId, TypeId>,

    /// Struct field and method contracts available to literals/access/calls.
    structs: FxHashMap<DefId, StructInfo>,

    /// Struct declarations explicitly marked with compiler-owned `@ json`.
    json_structs: FxHashSet<DefId>,

    /// Interface method contracts available to method lookup.
    interfaces: FxHashMap<DefId, FxHashMap<Symbol, InterfaceMethodInfo>>,

    /// Type-alias declaration parameter names keyed by alias def-id, in
    /// declaration order. Populated from HIR items so applied alias types
    /// (`Type::Applied(Alias, args)`) can be expanded at the unification
    /// boundary by substituting each arg for its param name (tela-d0).
    alias_type_params: FxHashMap<DefId, Vec<Symbol>>,

    /// Import bindings that behave like module/provider receivers.
    import_receivers: FxHashSet<DefId>,

    /// Backend target selected for target-aware availability gates.
    target: Target,

    /// Enum variant payload types keyed by variant definition.
    variant_fields: FxHashMap<DefId, Vec<TypeId>>,

    /// Enum variant payload names keyed by variant definition.
    variant_field_names: FxHashMap<DefId, Vec<Symbol>>,

    /// Parent enum for each variant constructor.
    variant_parent: FxHashMap<DefId, DefId>,

    /// Generic type parameters keyed by their declaring struct/enum def, in
    /// declaration order. Absent or empty means the nominal is not generic.
    type_params_by_def: FxHashMap<DefId, Vec<Symbol>>,

    /// Declared return type while checking a function body.
    current_return: Option<TypeId>,

    /// Active alternate-exit target while checking `iace` or failable calls.
    current_error: Option<ErrorSink>,

    /// Whether the active body is a systems device entry/helper.
    current_device_function: bool,

    /// Whether the active body is a source-owned graphics shader entry.
    current_shader_function: bool,

    /// Whether the active body is a cursor generator.
    current_generator_function: bool,

    /// Whether the active body may consume promissum values with await forms.
    current_await_context: bool,

    /// Whether the active body may use `reddet` to await and return.
    current_await_return_context: bool,

    /// Synthesized return type for expression contexts that allow inferred returns.
    inferred_return: Option<TypeId>,

    /// Whether the active return context may synthesize its normal return type from `redde`.
    allow_inferred_return: bool,

    /// Whether the current interval expression may carry a `per` step.
    allow_intervallum_step: bool,

    /// Monotonic counter for fresh inference variables within this checker.
    next_infer: u32,

    /// Reverse map from inference variable token to its table-local `TypeId`.
    infer_ids: FxHashMap<InferVar, TypeId>,

    /// Inference substitutions established by unification.
    /// Interior-mutable so `resolve_inferred_type` can path-compress under `&self`.
    substitutions: RefCell<FxHashMap<InferVar, TypeId>>,

    /// Memo for `canonicalize_type` (input TypeId → canonical TypeId). BO-S-5.
    canonical_cache: FxHashMap<TypeId, TypeId>,

    /// Memo for `occurs_in` results keyed by `(InferVar, resolved TypeId)`. BO-S-6.
    /// Cleared when substitutions change (each successful bind).
    occurs_cache: FxHashMap<(InferVar, TypeId), bool>,

    /// Union-hole member accumulation for `Type::InferUnion` (`∪` holes).
    ///
    /// A `∪` hole is solved by accumulation, not binding: every direct flow
    /// into the slot contributes a member type here (deduped via
    /// `TypeTable::equals`), and finalization solves the accumulated set into
    /// a `Union` or a missing-annotation error. Unlike `substitutions`, this
    /// map is never read by `resolve_inferred_type` — union holes stay open
    /// until finalize so later flows keep accumulating.
    union_members: RefCell<FxHashMap<InferVar, Vec<TypeId>>>,

    /// Index shape inference state (parallel to type inference above).
    index_infer: IndexInferState,

    /// Compile-time `lista<numerus>` shape witnesses for index constraint sources.
    shape_witnesses: FxHashMap<DefId, crate::semantic::index::IndexId>,

    /// Lowering-produced error expressions already reported during checking.
    errored_exprs: FxHashSet<HirId>,

    /// Table-local sentinel used to keep traversing after hard failures.
    error_type: TypeId,

    /// When set, the next checked closure's first parameter is a chorda scalar predicate slot.
    mark_chorda_scalar_closure_param: bool,

    /// Active reader-locale pack for intrinsic surface→canonical resolution at
    /// `check_intrinsic_method_call`. `None` when no pack is selected: lookup
    /// keeps the surface-only (Latin-identity) behavior.
    locale_pack: Option<&'a LocalePack>,
}

/// Typecheck resolved HIR and attach table-local types to declarations and expressions.
///
/// This entrypoint owns the full typecheck lifecycle: collect declaration
/// contracts, check item/entry bodies, then finalize HIR annotations by resolving
/// aliases and inference substitutions. It returns all hard diagnostics together
/// so callers can present a useful batch instead of stopping at the first
/// mismatch.
///
/// # Errors
///
/// Returns type mismatch, arity, and other typecheck diagnostics as a list of
/// [`SemanticError`]s.
pub fn typecheck(hir: &mut HirModule, resolver: &Resolver, types: &mut TypeTable) -> Result<(), Vec<SemanticError>> {
    typecheck_with_interner(hir, resolver, types, None)
}

/// Typecheck HIR with an optional [`Interner`] for symbol-level diagnostics.
///
/// # Errors
///
/// Returns type mismatch and related diagnostics as a list of [`SemanticError`]s.
pub fn typecheck_with_interner(
    hir: &mut HirModule,
    resolver: &Resolver,
    types: &mut TypeTable,
    interner: Option<&Interner>,
) -> Result<(), Vec<SemanticError>> {
    let config = crate::semantic::PassConfig::for_target(Target::HirRust);
    typecheck_with_config(hir, resolver, types, interner, &config, None)
}

/// Typecheck HIR with a caller-supplied [`PassConfig`].
///
/// # Errors
///
/// Returns type mismatch and related diagnostics as a list of [`SemanticError`]s.
pub fn typecheck_with_config(
    hir: &mut HirModule,
    resolver: &Resolver,
    types: &mut TypeTable,
    interner: Option<&Interner>,
    config: &crate::semantic::PassConfig,
    companion_signatures: Option<&crate::semantic::passes::companion_signature::CompanionSignatures>,
) -> Result<(), Vec<SemanticError>> {
    let mut checker = TypeChecker::new(resolver, types, interner, config.target);
    checker.locale_pack = config.locale_pack.as_ref();
    checker.collect_imported_file_structs();
    checker.collect_imported_enum_variants();
    checker.collect_imported_file_interfaces();
    if let Some(companions) = companion_signatures {
        checker.register_companion_signatures(companions);
    }
    checker.collect_items(hir);
    checker.check_program(hir);

    if checker.errors.is_empty() {
        Ok(())
    } else {
        Err(checker.errors)
    }
}

impl<'a> TypeChecker<'a> {
    fn new(resolver: &'a Resolver, types: &'a mut TypeTable, interner: Option<&'a Interner>, target: Target) -> Self {
        let error_type = types.intern(Type::Error);
        let mut index_infer = IndexInferState::new();
        index_infer.bootstrap_from_types(types);
        Self {
            resolver,
            interner,
            types,
            errors: Vec::new(),
            scopes: Vec::new(),
            binding_shadow: FxHashMap::default(),
            functions: FxHashMap::default(),
            consts: FxHashMap::default(),
            structs: FxHashMap::default(),
            json_structs: FxHashSet::default(),
            interfaces: FxHashMap::default(),
            alias_type_params: FxHashMap::default(),
            import_receivers: FxHashSet::default(),
            target,
            variant_fields: FxHashMap::default(),
            variant_field_names: FxHashMap::default(),
            variant_parent: FxHashMap::default(),
            type_params_by_def: FxHashMap::default(),
            current_return: None,
            current_error: None,
            current_device_function: false,
            current_shader_function: false,
            current_generator_function: false,
            current_await_context: false,
            current_await_return_context: false,
            inferred_return: None,
            allow_inferred_return: false,
            allow_intervallum_step: false,
            next_infer: 0,
            infer_ids: FxHashMap::default(),
            substitutions: RefCell::new(FxHashMap::default()),
            canonical_cache: FxHashMap::default(),
            occurs_cache: FxHashMap::default(),
            union_members: RefCell::new(FxHashMap::default()),
            index_infer,
            shape_witnesses: FxHashMap::default(),
            errored_exprs: FxHashSet::default(),
            error_type,
            mark_chorda_scalar_closure_param: false,
            locale_pack: None,
        }
    }

    /// Register companion function signatures for call-site type validation.
    ///
    /// Companion bodies are generated later by the MIR reverse-AD transform,
    /// but the function signature must be available during typecheck so call
    /// sites can validate argument counts and types.
    fn register_companion_signatures(
        &mut self,
        companions: &crate::semantic::passes::companion_signature::CompanionSignatures,
    ) {
        for (&def_id, sig) in &companions.functions {
            self.functions.insert(def_id, sig.clone());
        }
    }

    fn collect_imported_file_structs(&mut self) {
        for (def_id, fields, methods) in self.resolver.imported_file_structs() {
            let fields = fields
                .into_iter()
                .map(|field| {
                    (
                        field.name,
                        StructFieldInfo {
                            ty: field.ty,
                            optional: field.optional,
                            required: field.required,
                            span: crate::lexer::Span::default(),
                        },
                    )
                })
                .collect();
            let methods = methods
                .into_iter()
                .map(|method| (method.name, method.sig))
                .collect();
            self.structs.insert(def_id, StructInfo { fields, methods });
        }
    }

    /// Register imported enum variants (file-interface enum exports) into the
    /// typechecker's variant tables so `module.VARIANT` value references
    /// typecheck as `Type::Enum(consumer enum def)` (codex-gap S1 U2 VALUE
    /// members).
    fn collect_imported_enum_variants(&mut self) {
        for (enum_def, _, variants) in self.resolver.imported_file_enums() {
            for variant in &variants {
                let fields = variant.fields.iter().map(|f| f.ty).collect();
                let field_names = variant.fields.iter().map(|f| f.name).collect();
                self.variant_fields.insert(variant.def_id, fields);
                self.variant_field_names.insert(variant.def_id, field_names);
                self.variant_parent.insert(variant.def_id, enum_def);
                self.types.variant_parent.insert(variant.def_id, enum_def);
            }
        }
    }

    fn collect_imported_file_interfaces(&mut self) {
        for (def_id, methods) in self.resolver.imported_file_interfaces() {
            let methods = methods
                .into_iter()
                .map(|method| {
                    (
                        method.name,
                        InterfaceMethodInfo {
                            sig: method.sig,
                            availability: Vec::new(),
                            span: crate::lexer::Span::default(),
                        },
                    )
                })
                .collect();
            self.interfaces.insert(def_id, methods);
        }
    }

    fn check_program(&mut self, hir: &mut HirModule) {
        self.reject_json_annotations_on_site(&hir.entry_annotations, "incipit");
        for item in &mut hir.items {
            self.check_item(item);
        }

        if let Some(entry) = &mut hir.entry {
            let prev_await_context = self.current_await_context;
            let prev_await_return_context = self.current_await_return_context;
            self.current_await_context = hir.entry_is_async;
            self.current_await_return_context = false;
            self.check_block(entry, None);
            self.current_await_context = prev_await_context;
            self.current_await_return_context = prev_await_return_context;
        }

        self.finalize_hir(hir);
    }
}

fn param_mode_from_hir(mode: HirParamMode) -> SemanticParamMode {
    match mode {
        HirParamMode::Owned => SemanticParamMode::Owned,
        HirParamMode::De => SemanticParamMode::Ref,
        HirParamMode::In => SemanticParamMode::MutRef,
        HirParamMode::Ex => SemanticParamMode::Move,
    }
}

#[cfg(test)]
mod test_support;

#[cfg(test)]
#[path = "../typecheck_test.rs"]
mod tests;

#[cfg(test)]
#[path = "element_relaxation_test.rs"]
mod element_relaxation_tests;

#[cfg(test)]
#[path = "scalar_order_test.rs"]
mod scalar_order_test;

#[cfg(test)]
#[path = "equality_exact_types_test.rs"]
mod equality_exact_types_test;

#[cfg(test)]
#[path = "ascii_index_test.rs"]
mod ascii_index_test;

// Lane-gated: compiles through the full driver to the Rust target; a
// core-only build excludes the module.
#[cfg(all(test, feature = "hir-rust"))]
#[path = "numeric_value_equality_test.rs"]
mod numeric_value_equality_test;

#[cfg(test)]
#[path = "scalar_numeric_width_test.rs"]
mod scalar_numeric_width_test;

#[cfg(test)]
#[path = "union_pattern_test.rs"]
mod union_pattern_test;

#[cfg(test)]
#[path = "union_construction_test.rs"]
mod union_construction_test;

#[cfg(test)]
#[path = "union_value_test.rs"]
mod union_value_test;

#[cfg(test)]
#[path = "unsigned_integer_test.rs"]
mod unsigned_integer_test;

#[cfg(test)]
#[path = "modular_word_width_test.rs"]
mod modular_word_width_test;

#[cfg(test)]
#[path = "literal_inference_test.rs"]
mod literal_inference_test;

#[cfg(test)]
#[path = "glyph_binary_test.rs"]
mod glyph_binary_test;

#[cfg(test)]
#[path = "gradient_test.rs"]
mod gradient_test;
