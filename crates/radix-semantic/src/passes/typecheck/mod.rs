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
//! intentionally unchecked values. The checker follows [`TypeTable::is_assignable`]
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

use crate::locale::LocalePack;
use crate::{
    FuncSig, GenericBindingSource, GenericParamSort, GenericWitness, IndexExpr, IndexInferState,
    MarkerFamily, NumericWidth, ParamType, Primitive, ResolvedGenericBinding, ResolvedGenericCall,
    Resolver, SemanticError, SemanticErrorKind, SemanticGenericParamDefault, SemanticParamMode,
    Type, TypeId, TypeParamConstraint, TypeTable, WarningKind, types::InferVar,
};
use radix_hir::{
    DefId, HirArrayElement, HirAvailability, HirBinOp, HirBlock, HirCape, HirCasuArm,
    HirConversioTarget, HirExpression, HirExpressionKind, HirFunction, HirId, HirItem, HirItemKind,
    HirLiteral, HirLocal, HirModule, HirObjectField, HirObjectKey, HirParam, HirParamMode,
    HirPattern, HirPresentation, HirStatement, HirStatementKind, HirStruct, HirTypeForm,
    HirTypeFormArg, HirTypeParam, NucleumRole,
};
use radix_lexer::{Interner, Symbol};
use radix_target::Target;
use rustc_hash::{FxHashMap, FxHashSet};
use std::cell::RefCell;

pub mod access;
pub mod aggregate;
pub mod call;
#[cfg(test)]
#[path = "cfa_join_test.rs"]
mod cfa_join_test;
pub mod collect;
pub mod control;
pub mod convert;
#[cfg(test)]
#[path = "enum_access_honesty_test.rs"]
mod enum_access_honesty_test;
pub mod expr;
pub mod extremum;
pub mod finalize;
pub mod first_match;
pub mod format;
pub mod generic;
pub mod index_infer;
pub mod infer;
pub mod intervallum;
pub mod intrinsics;
pub mod item;
pub mod json_genus;
pub mod lockstep;
pub mod lookup;
pub mod ops;
pub mod pattern;
pub mod range;
#[cfg(test)]
pub mod relations_test;
pub mod scalar_text;
pub mod stmt;
pub mod summa;
pub mod tensor_index;
pub mod tensor_type_directed;
#[cfg(test)]
#[path = "trap_test.rs"]
mod trap_test;
#[cfg(test)]
#[path = "type_pattern_test.rs"]
mod type_pattern_test;
#[cfg(test)]
#[path = "union_member_refinement_test.rs"]
mod union_member_refinement_test;
#[cfg(test)]
#[path = "width_pattern_test.rs"]
mod width_pattern_test;

// Lane-gated: compiles through the full driver to the Rust target; a
// core-only build excludes the module.

/// Type information for a value currently visible in lexical scope.
#[derive(Clone, Copy)]
pub(crate) struct BindingInfo {
    /// Table-local type of the binding after declaration or pattern checking.
    ty: TypeId,
    /// Proven single-scalar `textus`/`ascii` (literal, unit `sectio`, predicate param, or interval bound param).
    scalar_text: bool,
    /// Whether the binding may be mutated ( `varia` local or `in` parameter ).
    mutable: bool,
}

/// Struct member information collected before expression checking begins.
pub(crate) struct StructInfo {
    /// Field contracts keyed by lowered field symbol.
    fields: FxHashMap<Symbol, StructFieldInfo>,

    /// Method signatures keyed by method name; receiver handling is performed
    /// by lookup/call checking rather than encoded in the signature table.
    methods: FxHashMap<Symbol, FuncSig>,
}

/// Interface method contract plus availability metadata.
#[derive(Clone)]
pub(crate) struct InterfaceMethodInfo {
    sig: FuncSig,
    availability: Vec<HirAvailability>,
    span: radix_lexer::Span,
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
    span: radix_lexer::Span,
}

/// Current alternate-exit context for `iace` and handled failable calls.
#[derive(Clone, Copy)]
pub(crate) enum ErrorSink {
    /// Function-level alternate-exit type declared on the current function.
    Function(TypeId),

    /// Local handler type introduced by `fac/cape` or handled blocks.
    Local(TypeId),

    /// Capture-boundary union hole introduced by `capta { ... }` (UT).
    ///
    /// Every alternate exit inside the trap body accumulates into this
    /// `Type::InferUnion` hole through the ordinary U-02 arms — no new
    /// inference. The hole also absorbs the body's success type, so the
    /// finalize solve produces `Success ∪ Errata` through the existing
    /// normalization.
    Trap(InferVar),
}

/// Mutable state for one HIR typecheck run.
///
/// The checker is both a visitor and an inference engine. Collection tables hold
/// declarations discovered before expression checking, while the scope stack
/// tracks lexical bindings introduced by blocks and patterns. Inference state is
/// deliberately separate from the shared type table: `Type::Infer` values are
/// stored in the table, but their substitutions are only meaningful through this
/// checker instance.
pub(crate) struct TypeChecker<'a> {
    /// Resolver snapshot from previous semantic passes. Kept with the checker so
    /// future lookup logic can consult resolved symbols without widening method
    /// signatures.
    pub resolver: &'a Resolver,

    /// Lexer-local symbol table used when typechecking name-sensitive stdlib
    /// method contracts. Symbols are not stable across lexer runs.
    pub interner: Option<&'a Interner>,

    /// Shared semantic type arena. All `TypeId`s read or written here are local
    /// to this table and must not be compared against IDs from another run.
    pub types: &'a mut TypeTable,

    /// Hard typechecking diagnostics collected while traversal continues.
    pub errors: Vec<SemanticError>,

    /// Lexical binding stack keyed by resolver `DefId`, innermost scope last.
    pub scopes: Vec<FxHashMap<DefId, BindingInfo>>,

    /// Nearest visible binding per `DefId` across the scope stack (O(1) lookup).
    /// Updated on insert; restored or removed on pop.
    pub binding_shadow: FxHashMap<DefId, BindingInfo>,

    /// Source spelling for HIR-local bindings (params/locals). Their DefIds
    /// start at 1_000_000 and are not in the resolver symbol table, so a
    /// Path that shadows an imported namespace must recover the name here.
    pub binding_names: FxHashMap<DefId, Symbol>,

    /// Function signatures collected before body checking so calls can be
    /// checked regardless of declaration order.
    pub functions: FxHashMap<DefId, FuncSig>,

    /// Nucleus roles keyed by function/method `DefId`. Call legality now lives
    /// on the MIR instantiation/composition seam; typecheck no longer rejects
    /// fragment calls.
    pub nucleum_roles: FxHashMap<DefId, NucleumRole>,

    /// Method nucleum roles keyed by (struct-or-interface def, method name).
    pub method_nucleum_roles: FxHashMap<(DefId, Symbol), NucleumRole>,

    /// Constant declaration types available to path lookup.
    pub consts: FxHashMap<DefId, TypeId>,

    /// Struct field and method contracts available to literals/access/calls.
    pub structs: FxHashMap<DefId, StructInfo>,

    /// Struct declarations explicitly marked with compiler-owned `@ json`.
    pub json_structs: FxHashSet<DefId>,

    /// Interface method contracts available to method lookup.
    pub interfaces: FxHashMap<DefId, FxHashMap<Symbol, InterfaceMethodInfo>>,

    /// Declared `implet` contracts of local genus declarations in item order,
    /// with their spans, so contract resolution reports deterministically.
    pub struct_implements: Vec<(DefId, radix_lexer::Span, Vec<DefId>)>,

    /// Kind-checked `implet` contracts keyed by struct def. There is no class
    /// inheritance (DECISIONS D8.1), so this is exactly the genus's own
    /// declaration, and interface-to-interface heritage is not in the grammar.
    pub implemented_interfaces: FxHashMap<DefId, FxHashSet<DefId>>,

    /// Type-alias declaration parameter names keyed by alias def-id, in
    /// declaration order. Populated from HIR items so applied alias types
    /// (`Type::Applied(Alias, args)`) can be expanded at the unification
    /// boundary by substituting each arg for its param name (tela-d0).
    pub alias_type_params: FxHashMap<DefId, Vec<Symbol>>,

    /// Import bindings that behave like module/provider receivers.
    pub import_receivers: FxHashSet<DefId>,

    /// Backend target selected for target-aware availability gates.
    pub target: Target,

    /// Enum variant payload types keyed by variant definition.
    pub variant_fields: FxHashMap<DefId, Vec<TypeId>>,

    /// Enum variant payload names keyed by variant definition.
    pub variant_field_names: FxHashMap<DefId, Vec<Symbol>>,

    /// Parent enum for each variant constructor.
    pub variant_parent: FxHashMap<DefId, DefId>,

    /// Generic type parameters keyed by their declaring struct/enum def, in
    /// declaration order. Absent or empty means the nominal is not generic.
    pub type_params_by_def: FxHashMap<DefId, Vec<Symbol>>,

    /// Sorts of the generic parameters on the function body currently being
    /// checked. Explicit symbolic size arguments carry only a `Symbol` in
    /// `Type::Param`, so the active declaration row is needed to distinguish
    /// a caller-owned `magnitudo` witness from a caller-owned type parameter.
    pub current_generic_param_kinds: FxHashMap<Symbol, radix_syntax::GenericParamKind>,

    /// Declared return type while checking a function body.
    pub current_return: Option<TypeId>,

    /// Active alternate-exit target while checking `iace` or failable calls.
    pub current_error: Option<ErrorSink>,

    /// Arm selected by the innermost checked failable call for function-level
    /// propagation. The expression dispatcher consumes and restores this slot
    /// so nested calls cannot leak their marker to an enclosing call.
    pub pending_conversion: Option<DefId>,

    /// Whether the active body is a systems device entry/helper.
    pub current_device_function: bool,

    /// Whether the active body is an `@ nucleum` device-entry kernel (not a
    /// fragment). The `summa … filum` placement law (SFR-3) admits the
    /// distributed form only inside kernels.
    pub current_nucleum_entry: bool,

    /// Whether the active body is a source-owned graphics shader entry.
    pub current_shader_function: bool,

    /// Whether the active body is a cursor generator.
    pub current_generator_function: bool,

    /// Whether the active body may consume promissum values with await forms.
    pub current_await_context: bool,

    /// Whether the active body may use `reddet` to await and return.
    pub current_await_return_context: bool,

    /// Synthesized return type for expression contexts that allow inferred returns.
    pub inferred_return: Option<TypeId>,

    /// Whether the active return context may synthesize its normal return type from `redde`.
    pub allow_inferred_return: bool,

    /// Whether the current interval expression may carry a `per` step.
    pub allow_intervallum_step: bool,

    /// Monotonic counter for fresh inference variables within this checker.
    pub next_infer: u32,

    /// Whether any alternate-exit payload flowed toward the innermost active
    /// `capta` trap sink (UT). Restored with `current_error` by `check_trap`;
    /// drives the T ≡ E collapse guard, which needs to distinguish "one
    /// member because success and error collapsed" from "one member because
    /// the body never failed" (the WARN017 degenerate solve).
    pub trap_error_flow: bool,

    /// Reverse map from inference variable token to its table-local `TypeId`.
    pub infer_ids: FxHashMap<InferVar, TypeId>,

    /// Inference substitutions established by unification.
    /// Interior-mutable so `resolve_inferred_type` can path-compress under `&self`.
    pub substitutions: RefCell<FxHashMap<InferVar, TypeId>>,

    /// Memo for `canonicalize_type` (input TypeId → canonical TypeId). BO-S-5.
    pub canonical_cache: FxHashMap<TypeId, TypeId>,

    /// Memo for `occurs_in` results keyed by `(InferVar, resolved TypeId)`. BO-S-6.
    /// Cleared when substitutions change (each successful bind).
    pub occurs_cache: FxHashMap<(InferVar, TypeId), bool>,

    /// Union-hole member accumulation for `Type::InferUnion` (`∪` holes).
    ///
    /// A `∪` hole is solved by accumulation, not binding: every direct flow
    /// into the slot contributes a member type here (deduped via
    /// `TypeTable::equals`), and finalization solves the accumulated set into
    /// a `Union` or a missing-annotation error. Unlike `substitutions`, this
    /// map is never read by `resolve_inferred_type` — union holes stay open
    /// until finalize so later flows keep accumulating.
    pub union_members: RefCell<FxHashMap<InferVar, Vec<TypeId>>>,

    /// Index shape inference state (parallel to type inference above).
    pub index_infer: IndexInferState,

    /// Compile-time `lista<numerus>` shape witnesses for index constraint sources.
    pub shape_witnesses: FxHashMap<DefId, crate::index::IndexId>,

    /// Lowering-produced error expressions already reported during checking.
    pub errored_exprs: FxHashSet<HirId>,

    /// Table-local sentinel used to keep traversing after hard failures.
    pub error_type: TypeId,

    /// When set, the next checked closure's first parameter is a chorda scalar predicate slot.
    pub mark_chorda_scalar_closure_param: bool,

    /// Active reader-locale pack for intrinsic surface→canonical resolution at
    /// `check_intrinsic_method_call`. `None` when no pack is selected: lookup
    /// keeps the surface-only (Latin-identity) behavior.
    pub locale_pack: Option<&'a LocalePack>,

    /// Live assert-remainder override frames, one per block being checked
    /// (U1R). `adfirma r est M` registers a narrowed binding here for the
    /// rest of its block; block exit restores every override in reverse.
    pub(crate) assert_refinements: Vec<Vec<(DefId, Option<BindingInfo>)>>,

    /// Declared binding state remembered when a cross-statement fact first
    /// narrows a binding. Assignment invalidation restores this state instead
    /// of treating the narrowed type as the binding's new declaration.
    pub(crate) flow_bases: FxHashMap<DefId, BindingInfo>,
}

/// Typecheck resolved HIR and attach table-local types to declarations and expressions.
///
/// Test helper around [`typecheck_with_interner`]. Production analysis uses
/// [`typecheck_with_config`].
///
/// # Errors
///
/// Returns type mismatch, arity, and other typecheck diagnostics as a list of
/// [`SemanticError`]s.
#[cfg(test)]
pub(crate) fn typecheck(
    hir: &mut HirModule,
    resolver: &Resolver,
    types: &mut TypeTable,
) -> Result<(), Vec<SemanticError>> {
    typecheck_with_interner(hir, resolver, types, None)
}
/// Typecheck HIR with an optional [`Interner`] for symbol-level diagnostics.
///
/// # Errors
///
/// Returns type mismatch and related diagnostics as a list of [`SemanticError`]s.
#[cfg(test)]
pub(crate) fn typecheck_with_interner(
    hir: &mut HirModule,
    resolver: &Resolver,
    types: &mut TypeTable,
    interner: Option<&Interner>,
) -> Result<(), Vec<SemanticError>> {
    let config = crate::PassConfig::for_target(Target::HirRust);
    typecheck_with_config(hir, resolver, types, interner, &config, None, None)
}

/// Typecheck HIR with a caller-supplied [`PassConfig`].
///
/// # Errors
///
/// Returns type mismatch and related diagnostics as a list of [`SemanticError`]s.
pub(crate) fn typecheck_with_config(
    hir: &mut HirModule,
    resolver: &Resolver,
    types: &mut TypeTable,
    interner: Option<&Interner>,
    config: &crate::PassConfig,
    companion_signatures: Option<&crate::passes::companion_signature::CompanionSignatures>,
    presentation: Option<&mut HirPresentation>,
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
    if let Some(presentation) = presentation {
        checker.finalize_authored_forms(presentation);
    }

    if checker.errors.is_empty() {
        Ok(())
    } else {
        Err(checker.errors)
    }
}

impl<'a> TypeChecker<'a> {
    pub fn new(
        resolver: &'a Resolver,
        types: &'a mut TypeTable,
        interner: Option<&'a Interner>,
        target: Target,
    ) -> Self {
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
            binding_names: FxHashMap::default(),
            functions: FxHashMap::default(),
            nucleum_roles: FxHashMap::default(),
            method_nucleum_roles: FxHashMap::default(),
            consts: FxHashMap::default(),
            structs: FxHashMap::default(),
            json_structs: FxHashSet::default(),
            interfaces: FxHashMap::default(),
            struct_implements: Vec::new(),
            implemented_interfaces: FxHashMap::default(),
            alias_type_params: FxHashMap::default(),
            import_receivers: FxHashSet::default(),
            target,
            variant_fields: FxHashMap::default(),
            variant_field_names: FxHashMap::default(),
            variant_parent: FxHashMap::default(),
            type_params_by_def: FxHashMap::default(),
            current_generic_param_kinds: FxHashMap::default(),
            current_return: None,
            current_error: None,
            pending_conversion: None,
            current_device_function: false,
            current_nucleum_entry: false,
            current_shader_function: false,
            current_generator_function: false,
            current_await_context: false,
            current_await_return_context: false,
            inferred_return: None,
            allow_inferred_return: false,
            allow_intervallum_step: false,
            next_infer: 0,
            trap_error_flow: false,
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
            assert_refinements: Vec::new(),
            flow_bases: FxHashMap::default(),
        }
    }

    /// Register companion function signatures for call-site type validation.
    ///
    /// Companion bodies are generated later by the MIR reverse-AD transform,
    /// but the function signature must be available during typecheck so call
    /// sites can validate argument counts and types.
    pub fn register_companion_signatures(
        &mut self,
        companions: &crate::passes::companion_signature::CompanionSignatures,
    ) {
        for (&def_id, sig) in &companions.functions {
            self.functions.insert(def_id, sig.clone());
        }
    }

    pub fn collect_imported_file_structs(&mut self) {
        self.json_structs
            .extend(self.resolver.imported_json_structs());
        for (def_id, fields, methods, type_params) in self.resolver.imported_file_structs() {
            // Imported generic structs substitute construction size args into
            // field types exactly like local ones (S0-G1); without this entry
            // `check_struct_fields` leaves `tensor<f32, [D]>` rigid and every
            // same-shape initializer fires SEM010.
            if !type_params.is_empty() {
                self.type_params_by_def.insert(def_id, type_params);
            }
            let fields = fields
                .into_iter()
                .map(|field| {
                    (
                        field.name,
                        StructFieldInfo {
                            ty: field.ty,
                            optional: field.optional,
                            required: field.required,
                            span: radix_lexer::Span::default(),
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
    pub fn collect_imported_enum_variants(&mut self) {
        for (enum_def, _, variants) in self.resolver.imported_file_enums() {
            let commune = self.resolver.commune_fields(enum_def);
            for variant in &variants {
                let mut fields: Vec<TypeId> = commune.iter().map(|field| field.ty).collect();
                fields.extend(variant.fields.iter().map(|f| f.ty));
                let mut field_names: Vec<Symbol> = commune.iter().map(|field| field.name).collect();
                field_names.extend(variant.fields.iter().map(|f| f.name));
                self.variant_fields.insert(variant.def_id, fields);
                self.variant_field_names.insert(variant.def_id, field_names);
                self.variant_parent.insert(variant.def_id, enum_def);
                self.types.variant_parent.insert(variant.def_id, enum_def);
            }
        }
    }

    pub fn collect_imported_file_interfaces(&mut self) {
        for (def_id, methods) in self.resolver.imported_file_interfaces() {
            let methods = methods
                .into_iter()
                .map(|method| {
                    (
                        method.name,
                        InterfaceMethodInfo {
                            sig: method.sig,
                            availability: Vec::new(),
                            span: radix_lexer::Span::default(),
                        },
                    )
                })
                .collect();
            self.interfaces.insert(def_id, methods);
        }
    }

    pub fn check_program(&mut self, hir: &mut HirModule) {
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

// Lane-gated: compiles through the full driver to the Rust target; a
// core-only build excludes the module.

#[cfg(test)]
#[path = "test_support.rs"]
pub mod test_support;

#[cfg(test)]
#[path = "access_operand_test.rs"]
mod access_operand_test;

#[cfg(test)]
#[path = "ascii_cursor_test.rs"]
mod ascii_cursor_test;

#[cfg(test)]
#[path = "ascii_index_test.rs"]
mod ascii_index_test;

#[cfg(test)]
#[path = "ascii_format_pack_conversio_test.rs"]
mod ascii_format_pack_conversio_test;

#[cfg(test)]
#[path = "atomic_intrinsic_test.rs"]
mod atomic_intrinsic_test;

#[cfg(test)]
#[path = "collection_closure_test.rs"]
mod collection_closure_test;

#[cfg(test)]
#[path = "bounded_capacity_slot_test.rs"]
mod bounded_capacity_slot_test;

#[cfg(test)]
#[path = "bounded_identity_test.rs"]
mod bounded_identity_test;

#[cfg(test)]
#[path = "bounded_lista_test.rs"]
mod bounded_lista_test;

#[cfg(test)]
#[path = "queue_stack_test.rs"]
mod queue_stack_test;

#[cfg(test)]
#[path = "lista_operator_test.rs"]
mod lista_operator_test;

#[cfg(test)]
#[path = "convert_test.rs"]
mod convert_test;

#[cfg(test)]
#[path = "element_relaxation_test.rs"]
mod element_relaxation_test;

#[cfg(test)]
#[path = "equality_exact_types_test.rs"]
mod equality_exact_types_test;

#[cfg(test)]
#[path = "decimal_width_literal_test.rs"]
mod decimal_width_literal_test;

#[cfg(test)]
#[path = "decimal_taxonomy_test.rs"]
mod decimal_taxonomy_test;

#[cfg(test)]
#[path = "decimal_conversio_test.rs"]
mod decimal_conversio_test;

#[cfg(test)]
#[path = "decimal_ordering_test.rs"]
mod decimal_ordering_test;

#[cfg(test)]
#[path = "decimal_equality_test.rs"]
mod decimal_equality_test;

#[cfg(test)]
#[path = "gate_law_totality_test.rs"]
mod gate_law_totality_test;

#[cfg(test)]
#[path = "generic_call_test.rs"]
mod generic_call_test;

#[cfg(test)]
#[path = "generic_construction_test.rs"]
mod generic_construction_test;

#[cfg(test)]
#[path = "generic_params_test.rs"]
mod generic_params_test;

#[cfg(test)]
#[path = "glyph_binary_test.rs"]
mod glyph_binary_test;

#[cfg(test)]
#[path = "alias_transparency_test.rs"]
mod alias_transparency_test;

#[cfg(test)]
#[path = "gradient_test.rs"]
mod gradient_test;

#[cfg(test)]
#[path = "infer_test.rs"]
mod infer_test;

#[cfg(test)]
#[path = "itera_apud_test.rs"]
mod itera_apud_test;

#[cfg(test)]
#[path = "summa_test.rs"]
mod summa_test;

#[cfg(test)]
#[path = "argmaxima_test.rs"]
mod argmaxima_test;

#[cfg(test)]
#[path = "predicate_twins_test.rs"]
mod predicate_twins_test;

#[cfg(test)]
#[path = "extremum_test.rs"]
mod extremum_test;

#[cfg(test)]
#[path = "literal_inference_test.rs"]
mod literal_inference_test;

#[cfg(test)]
#[path = "infinity_literal_test.rs"]
mod infinity_literal_test;

#[cfg(test)]
#[path = "marker_hole_test.rs"]
mod marker_hole_test;

#[cfg(test)]
#[path = "matrix_intrinsic_test.rs"]
mod matrix_intrinsic_test;

#[cfg(test)]
#[path = "namespace_shadow_test.rs"]
mod namespace_shadow_test;

#[cfg(test)]
#[path = "conditional_narrowing_test.rs"]
mod conditional_narrowing_test;

#[cfg(test)]
#[path = "null_narrowing_test.rs"]
mod null_narrowing_test;

#[cfg(test)]
#[path = "numeric_value_equality_test.rs"]
mod numeric_value_equality_test;

#[cfg(test)]
#[path = "octeti_endian_conversio_test.rs"]
mod octeti_endian_conversio_test;
#[cfg(test)]
#[path = "register_conversio_test.rs"]
mod register_conversio_test;

#[cfg(test)]
#[path = "bounded_string_conversio_test.rs"]
mod bounded_string_conversio_test;

#[cfg(test)]
#[path = "bits_conversio_test.rs"]
mod bits_conversio_test;

#[cfg(test)]
#[path = "bf16_conversio_test.rs"]
mod bf16_conversio_test;

#[cfg(test)]
#[path = "octeti_lista_conversio_test.rs"]
mod octeti_lista_conversio_test;

#[cfg(test)]
#[path = "octeti_index_test.rs"]
mod octeti_index_test;

#[cfg(test)]
#[path = "scalar_longitudo_test.rs"]
mod scalar_longitudo_test;

#[cfg(test)]
#[path = "scalar_order_test.rs"]
mod scalar_order_test;

#[cfg(test)]
#[path = "tensor_bracket_test.rs"]
mod tensor_bracket_test;

#[cfg(test)]
#[path = "tensor_index_test.rs"]
mod tensor_index_test;

#[cfg(test)]
#[path = "tensor_index_width_test.rs"]
mod tensor_index_width_test;

#[cfg(test)]
#[path = "index_param_value_test.rs"]
mod index_param_value_test;

#[cfg(test)]
#[path = "tensor_lattice_matrix_test.rs"]
mod tensor_lattice_matrix_test;

#[cfg(test)]
#[path = "tensor_minus_test.rs"]
mod tensor_minus_test;

#[cfg(test)]
#[path = "tensor_plus_test.rs"]
mod tensor_plus_test;

#[cfg(test)]
#[path = "tensor_type_directed_test.rs"]
mod tensor_type_directed_test;

#[cfg(test)]
#[path = "textus_cursor_test.rs"]
mod textus_cursor_test;

#[cfg(test)]
#[path = "type_hole_union_infer_test.rs"]
mod type_hole_union_infer_test;

#[cfg(test)]
#[path = "type_hole_union_join_test.rs"]
mod type_hole_union_join_test;

#[cfg(test)]
#[path = "type_hole_union_warnings_test.rs"]
mod type_hole_union_warnings_test;

#[cfg(test)]
#[path = "union_commune_test.rs"]
mod union_commune_test;

#[cfg(test)]
#[path = "union_conversio_test.rs"]
mod union_conversio_test;

#[cfg(test)]
#[path = "union_construction_test.rs"]
mod union_construction_test;

#[cfg(test)]
#[path = "union_pattern_test.rs"]
mod union_pattern_test;

#[cfg(test)]
#[path = "est_imported_type_test.rs"]
mod est_imported_type_test;

#[cfg(test)]
#[path = "union_value_test.rs"]
mod union_value_test;

#[cfg(test)]
#[path = "generic_defaults_test.rs"]
mod generic_defaults_test;

#[cfg(test)]
#[path = "named_type_defaults_test.rs"]
mod named_type_defaults_test;

#[cfg(test)]
#[path = "format_test.rs"]
mod format_test;
