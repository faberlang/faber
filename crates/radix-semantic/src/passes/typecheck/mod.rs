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
    FuncSig, GenericBindingSource, GenericParamSort, GenericWitness, IndexExpr, IndexId,
    IndexInferState, MarkerFamily, NumericWidth, ParamType, Primitive, ResolvedGenericBinding,
    ResolvedGenericCall, Resolver, RuntimeSizeBinding, SemanticError, SemanticErrorKind,
    SemanticGenericParamDefault, SemanticParamMode, ShapeObligationTable, ShapeRelation,
    ShapeRelationBoundary, Type, TypeId, TypeParamConstraint, TypeTable, WarningKind,
    types::InferVar,
};
use handroll::{FxHashMap, FxHashSet};
use radix_hir::{
    DefId, HirArrayElement, HirAvailability, HirBinOp, HirBlock, HirCape, HirCasuArm,
    HirConversioTarget, HirExpression, HirExpressionKind, HirFunction, HirId, HirItem, HirItemKind,
    HirLiteral, HirLocal, HirModule, HirObjectField, HirObjectKey, HirParam, HirParamMode,
    HirPattern, HirPresentation, HirStatement, HirStatementKind, HirStruct, HirTypeForm,
    HirTypeFormArg, HirTypeParam, NucleumRole,
};
use radix_lexer::{Interner, Symbol};
use radix_target::Target;
use radix_types::RuntimeExtentId;
use std::cell::RefCell;

pub mod access;
pub mod aggregate;
#[cfg(test)]
#[path = "axis_reduction_multi_test.rs"]
mod axis_reduction_multi_test;
pub mod call;
#[cfg(test)]
#[path = "call_default_test.rs"]
mod call_default_test;

#[cfg(test)]
#[path = "at_read_test.rs"]
mod at_read_test;
#[cfg(test)]
#[path = "cfa_join_test.rs"]
mod cfa_join_test;
pub mod collect;
#[cfg(test)]
#[path = "const_share_into_var_test.rs"]
mod const_share_into_var_test;
mod contract_bound;
#[cfg(test)]
#[path = "contract_bound_test.rs"]
mod contract_bound_test;
pub mod control;
pub mod convert;
#[cfg(test)]
#[path = "copy_marked_operand_test.rs"]
mod copy_marked_operand_test;
mod empty_dense;
#[cfg(test)]
#[path = "enum_access_honesty_test.rs"]
mod enum_access_honesty_test;
pub mod expr;
#[cfg(test)]
#[path = "failable_intrinsic_typed_error_test.rs"]
mod failable_intrinsic_typed_error_test;
#[cfg(test)]
#[path = "field_modifier_write_test.rs"]
mod field_modifier_write_test;
#[cfg(test)]
#[path = "fieldless_type_access_test.rs"]
mod fieldless_type_access_test;
mod flow_range;
mod flow_range_access;
#[cfg(test)]
#[path = "flow_range_access_test.rs"]
mod flow_range_access_test;
mod flow_range_index;
#[cfg(test)]
#[path = "flow_range_index_test.rs"]
mod flow_range_index_test;
#[cfg(test)]
mod flow_range_probe;
mod flow_range_produce;
#[cfg(test)]
#[path = "flow_range_produce_test.rs"]
mod flow_range_produce_test;
#[cfg(test)]
#[path = "flow_range_test.rs"]
mod flow_range_test;
#[cfg(test)]
#[path = "loop_shape_test.rs"]
mod loop_shape_test;
#[cfg(test)]
#[path = "mutating_intrinsic_test.rs"]
mod mutating_intrinsic_test;
#[cfg(test)]
#[path = "non_null_mutating_receiver_test.rs"]
mod non_null_mutating_receiver_test;
#[cfg(test)]
#[path = "nondum_function_test.rs"]
mod nondum_function_test;
#[cfg(test)]
#[path = "optional_element_get_test.rs"]
mod optional_element_get_test;
#[cfg(test)]
#[path = "ponde_failable_test.rs"]
mod ponde_failable_test;
#[cfg(test)]
#[path = "register_shape_finalize_test.rs"]
mod register_shape_finalize_test;
#[cfg(test)]
#[path = "shape_obligations_test.rs"]
mod shape_obligations_test;
#[cfg(test)]
#[path = "slot_lift_test.rs"]
mod slot_lift_test;
#[cfg(test)]
#[path = "target_import_condition_test.rs"]
mod target_import_condition_test;
#[cfg(test)]
#[path = "target_import_ordering_test.rs"]
mod target_import_ordering_test;
#[cfg(test)]
#[path = "target_import_store_test.rs"]
mod target_import_store_test;
#[cfg(test)]
#[path = "target_import_structural_test.rs"]
mod target_import_structural_test;
#[cfg(test)]
#[path = "target_import_test.rs"]
mod target_import_test;
mod tensor_edge_policy_generic;
#[cfg(test)]
#[path = "text_get_test.rs"]
mod text_get_test;

mod bits;
mod closing;
#[cfg(test)]
#[path = "construction_source_test.rs"]
mod construction_source_test;
pub mod finalize;
pub mod format;
pub mod generic;
pub mod hashable;
pub mod index_infer;
mod inf;
pub mod infer;
mod insere;
pub mod integer_range;
pub mod intervallum;
pub mod intrinsics;
pub mod item;
pub mod itera_element;
pub mod json_genus;
pub mod littera_conversio;
pub mod lockstep;
pub mod lookup;
mod loop_shape;
mod member_privacy;
mod numeric_twin;
mod sort_expr;
#[cfg(test)]
#[path = "sort_expr_test.rs"]
mod sort_expr_test;
pub(crate) use numeric_twin::intern_place_temporaries;
pub use numeric_twin::{
    binary_twin_name, in_place_binary_twin_name, in_place_unary_twin_name, unary_twin_name,
};
pub mod ops;
mod ordering;
mod sinking;
mod store;

#[cfg(test)]
#[path = "closing_test.rs"]
mod closing_test;
#[cfg(test)]
#[path = "discerne_closed_set_test.rs"]
mod discerne_closed_set_test;
#[cfg(test)]
#[path = "empty_dense_test.rs"]
mod empty_dense_test;
pub mod expanded_axis;
#[cfg(test)]
#[path = "expanded_axis_test.rs"]
mod expanded_axis_test;
#[cfg(test)]
#[path = "kernel_index_warning_test.rs"]
mod kernel_index_warning_test;
#[cfg(test)]
#[path = "keyed_literal_test.rs"]
mod keyed_literal_test;
#[cfg(test)]
#[path = "ordering_test.rs"]
mod ordering_test;
pub mod ordo_conversio;
pub mod pattern;
pub mod range;
#[cfg(test)]
#[path = "record_anon_test.rs"]
mod record_anon_test;
pub mod reduction;
#[cfg(test)]
#[path = "reduction_test.rs"]
mod reduction_test;
#[cfg(test)]
pub mod relations_test;
pub mod scalar_text;
pub mod scan;
#[cfg(test)]
#[path = "scan_test.rs"]
mod scan_test;
pub mod sermo_route;
#[cfg(test)]
#[path = "sinking_test.rs"]
mod sinking_test;
pub mod stmt;
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
#[path = "union_overlap_access_test.rs"]
mod union_overlap_access_test;

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

    /// Visibility of methods that are not public (D5.3); absent is public.
    method_visibility: FxHashMap<Symbol, radix_syntax::MemberVisibility>,
}

/// Interface method contract plus availability metadata.
#[derive(Clone)]
pub(crate) struct InterfaceMethodInfo {
    sig: FuncSig,
    /// Declared `axis` markers in parameter order; the echo rule reads this
    /// for a contract method call (its signature's `SemanticParamMode` cannot
    /// carry the marker).
    axis: Vec<bool>,
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

    /// Authored field modifier (D16.1); `None` for unmarked and builtin
    /// fields. Gates writes outside construction literals.
    modifier: Option<radix_syntax::FieldModifier>,

    /// Member visibility (D5.2, D5.3); gates access outside construction
    /// literals.
    visibility: radix_syntax::MemberVisibility,
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

    /// Source spelling for HIR-local bindings (params/locals). Their `DefIds`
    /// start at `1_000_000` and are not in the resolver symbol table, so a
    /// Path that shadows an imported namespace must recover the name here.
    pub binding_names: FxHashMap<DefId, Symbol>,

    /// Immutable (`const`/`fixum`) local bindings. `const` is deep: nothing
    /// reachable through such a name may be modified, so a
    /// mutating receiver rooted in one is rejected.
    pub const_locals: FxHashSet<DefId>,

    /// Read-only parameters (unmarked or `de`) seen while checking bodies.
    /// Like a `const` local, such a parameter is a read-only value: a plain
    /// `mut T y ← p` of a shareable value is SEM065. `DefIds`
    /// are unique, so the set needs no per-body scope.
    pub read_only_param_roots: FxHashSet<DefId>,

    /// Function signatures collected before body checking so calls can be
    /// checked regardless of declaration order.
    pub functions: FxHashMap<DefId, FuncSig>,

    /// Nucleus roles keyed by function/method `DefId`. Call legality now lives
    /// on the MIR instantiation/composition seam; typecheck no longer rejects
    /// fragment calls.
    pub nucleum_roles: FxHashMap<DefId, NucleumRole>,

    /// Kernels (functions and methods, public or private) that rule 12.7
    /// inlines into their callers: `@ nucleum`, synchronous, infallible. A
    /// reduction body may call them; the filum pass proves the callee body
    /// writes nothing.
    pub inlinable_kernels: FxHashSet<DefId>,
    /// Per declared function or method, which parameters are `axis`
    /// markers: the call must echo `axis` on exactly those.
    pub axis_params: FxHashMap<DefId, Vec<bool>>,

    /// Method nucleum roles keyed by (struct-or-interface def, method name).
    pub method_nucleum_roles: FxHashMap<(DefId, Symbol), NucleumRole>,

    /// Source method identities keyed by (owning class, method name).
    /// Intrinsic-selected callbacks such as tensor edge policies keep their
    /// ordinary method identity without introducing a synthetic call edge.
    pub method_defs: FxHashMap<(DefId, Symbol), DefId>,

    /// Class methods that have an ordinary instance receiver. Intrinsics that
    /// select policy callbacks must not accidentally admit static methods.
    pub instance_method_defs: FxHashSet<(DefId, Symbol)>,

    /// Number of per-instance fields on each struct. Static `generis` fields
    /// do not occupy the runtime policy value selected by `tensor.limes`.
    pub instance_field_counts: FxHashMap<DefId, usize>,

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
    pub struct_implements: Vec<(DefId, radix_lexer::Span, Vec<radix_types::ContractRef>)>,

    /// Kind-checked `implet` contracts keyed by struct def. There is no class
    /// inheritance (DECISIONS D8.1), so this is exactly the genus's own
    /// declaration, and interface-to-interface heritage is not in the grammar.
    pub implemented_interfaces: FxHashMap<DefId, FxHashSet<DefId>>,

    /// Well-formed `implet` contracts with their type arguments, keyed by
    /// struct def (D1.2). Bound satisfaction compares against these (D1.3).
    pub implemented_contracts: FxHashMap<DefId, Vec<radix_types::ContractRef>>,

    /// `implet` bounds of the generic parameters on the body currently being
    /// checked (D1.1): bound members are callable on a `T` receiver, and a
    /// bounded `T` satisfies the same bound at a nested call.
    pub current_generic_bounds: FxHashMap<Symbol, Vec<radix_types::ContractRef>>,
    /// Next id for expressions the checker synthesizes (D1.4 glyph
    /// rewrites); seeded above the module's largest id in `check_program`.
    next_hir_id: u32,
    /// Next definition id for locals the checker synthesizes (ss1b place
    /// temporaries); seeded above the module's largest id in `check_program`.
    next_synthetic_def: u32,

    /// Store position (d11-6 ruling 21) of the expression about to be checked;
    /// consumed by the next `check_expr_with_expected` so only that node, not
    /// its operands, is treated as landing in the slot.
    pending_store: Option<store::StoreSite>,
    /// Initializer expression of the declaration store being checked; set by
    /// `check_local` so opaque axes can pin to the annotation's terms there.
    pub(crate) current_store_expr: Option<radix_hir::HirId>,
    /// Ring operators of a store into a `wrapping<W>` slot, keyed by the id of
    /// their first operand and mapped to the slot's word type. Where both
    /// operands are that word, the operator is typed in the word and reduces
    /// per operation, which cannot be observed because the store reduces the
    /// same value (numeric-model §8). Every other `wrapping` expression is
    /// exact.
    word_ring: FxHashMap<HirId, TypeId>,
    /// The synthesized type of the expression `check_expr_with_expected` last
    /// finished, before the expected-type unify replaced it with the error
    /// type on a mismatch. The K5 lifting diagnostic reads it.
    last_synthesized_type: Option<TypeId>,

    /// For a local whose type was inferred (`_`): the span of the expression
    /// the type came from, named by an implicit store's trap (ruling 19(i)).
    inferred_slot_sources: FxHashMap<DefId, radix_lexer::Span>,
    /// Analyzed source text, for quoting an inferring expression in a trap.
    pub(crate) source_text: Option<std::sync::Arc<str>>,
    /// `@ radix lane "air"` functions; the AIR subset admits no store node.
    pub(crate) air_lane_defs: FxHashSet<DefId>,
    /// Whether the function being checked is an AIR-lane function.
    pub(crate) current_air_function: bool,
    /// Locals and parameters iterated by the loops enclosing the expression
    /// being checked; the loop shape rule reads it (`loop_shape`).
    loop_iterated: Vec<DefId>,
    /// Declaration currently owning the body being checked.
    pub(crate) current_function_def: Option<DefId>,
    /// HIR identity of the call currently checking its argument contracts.
    pub(crate) current_shape_call: Option<HirId>,
    /// Receiver the numeric-twin probe already checked. Checking is not
    /// idempotent (an intrinsic call rewrites its method symbol to the
    /// canonical spelling), so the next check of that expression adopts this
    /// type instead of checking the receiver a second time.
    pub(crate) twin_probed_receiver: Option<(HirId, TypeId)>,
    /// Returned expression whose runtime shape is checked against the result contract.
    pub(crate) current_shape_return_expr: Option<HirId>,
    /// Zero-based value-argument ordinal currently being checked.
    pub(crate) current_shape_argument: Option<u32>,
    /// Field path while recursively checking a record-valued call argument.
    pub(crate) current_shape_fields: Vec<Symbol>,
    /// Runtime carrier axes that witness generic size symbols in this function.
    pub(crate) current_shape_param_sources: FxHashMap<Symbol, IndexId>,
    /// Caller-owned binding witnesses used inside a nested call's index space.
    pub(crate) current_shape_param_call_sources: FxHashMap<Symbol, IndexId>,
    /// Runtime shape relations discovered by typechecking.
    pub(crate) shape_obligations: ShapeObligationTable,
    /// Resolved edge-policy callbacks selected by tensor `limes` calls.
    pub(crate) tensor_edge_policies: crate::TensorEdgePolicyTable,
    /// Generic call bindings retained until deferred edge-policy obligations
    /// can be discharged after every local body has been checked.
    tensor_edge_generic_calls: Vec<tensor_edge_policy_generic::GenericPolicyCall>,
    /// Nesting depth of assignment-target checks. An index read inside a
    /// target is evaluated at the store, not where the checker meets it, so it
    /// records no proof (need, slice 4).
    index_proof_suppressed: u32,
    /// Generic edge-policy obligations imported with namespace functions.
    /// These remain separate from local HIR summaries and carry only
    /// consumer-arena type identities.
    tensor_edge_policy_imported_requirements:
        FxHashMap<DefId, Vec<radix_hir::TensorEdgePolicyRequirement>>,
    /// Generic type parameters observed in runtime type positions. Builtin
    /// edge-policy markers may bind only parameters with no such use.
    tensor_edge_policy_runtime_params: FxHashSet<(DefId, Symbol)>,
    /// Locals declared `fixum T x = e` (D5.10): their initializer is an `=`
    /// position, a compile-time store.
    compile_time_locals: FxHashSet<DefId>,
    /// Static `const size N = 3` values keyed by binding `DefId` (Z1): figura
    /// references to a declared size lower as `Runtime(ScalarBinding(def))`,
    /// so the fold keys on the binding; an affine extent over a static size
    /// that folds provably negative fails its lift obligation at the store.
    pub(crate) static_size_values: FxHashMap<radix_hir::DefId, u64>,
    /// Exact values of compile-time integer bindings (immutable `const int`
    /// locals and module constants), keyed by binding `DefId`. The `axis`
    /// argument of an axis reduction accepts one of these.
    pub(crate) const_int_values: FxHashMap<radix_hir::DefId, i128>,
    /// The module constants among `const_int_values`: with the
    /// `compile_time_locals`, the bindings whose value is a compile-time
    /// fact, so an index naming one is proven like the literal it holds.
    pub(crate) module_const_ints: FxHashSet<radix_hir::DefId>,
    /// Node ids of expressions desugared from numeric twin methods (operator,
    /// assignment, or effect-hoisted block), handed to the presentation
    /// sidecar when the check finishes.
    numeric_twin_ids: Vec<HirId>,
    /// Result type of each `t.sum(axis n)` expansion, read as the expected
    /// bucket shape when the synthesized reduction node is checked.
    axis_reduction_results: FxHashMap<HirId, TypeId>,
    /// Node ids of `empty` literals rewritten into the typed zero, handed to
    /// the presentation sidecar when the check finishes.
    pub(super) empty_dense_ids: Vec<HirId>,
    /// Node ids of inferred-constructor holes `_ { … }` the checker rewrote,
    /// handed to the presentation sidecar when the check finishes.
    pub(super) inferred_constructor_ids: Vec<HirId>,
    /// Authored axis and method of each `t.sum(axis n)` / `t.mean(axis n)`
    /// expansion, keyed by the expanded node id; handed to
    /// the presentation sidecar so emission can restore the authored call.
    pub(super) axis_sugar: Vec<radix_hir::HirAxisSugar>,

    /// Type-alias declaration parameter names keyed by alias def-id, in
    /// declaration order. Populated from HIR items so applied alias types
    /// (`Type::Applied(Alias, args)`) can be expanded at the unification
    /// boundary by substituting each arg for its param name (tela-d0).
    pub alias_type_params: FxHashMap<DefId, Vec<Symbol>>,

    /// Import bindings that behave like module/provider receivers.
    pub import_receivers: FxHashSet<DefId>,

    /// Bindings introduced by target-native imports (`import python "math"
    /// m`): trusted `ignotum` roots whose calls and member accesses are
    /// admitted unchecked.
    pub trusted_imports: FxHashSet<DefId>,

    /// Local free functions marked `@ nondum`, keyed by item definition.
    /// Every use is refused (SEM017 `nondum_function_for_target`) on a target
    /// the gate covers; the declaration itself is fine.
    pub unavailable_functions: FxHashMap<DefId, Vec<crate::file_interface::InterfaceUnavailable>>,

    /// `genus` methods marked `@ nondum`, keyed by (owning class, method
    /// name) like `method_defs`. Every call is refused the same way.
    pub unavailable_methods:
        FxHashMap<(DefId, Symbol), Vec<crate::file_interface::InterfaceUnavailable>>,

    /// Backend target selected for target-aware availability gates.
    pub target: Target,

    /// Enum variant payload types keyed by variant definition.
    pub variant_fields: FxHashMap<DefId, Vec<TypeId>>,

    /// Enum variant payload names keyed by variant definition.
    pub variant_field_names: FxHashMap<DefId, Vec<Symbol>>,

    /// Parent enum for each variant constructor.
    pub variant_parent: FxHashMap<DefId, DefId>,

    /// Local `ordo` members in declaration order, keyed by the enum def
    /// (D9.4 built-in conversion rows).
    pub ordo_members: FxHashMap<DefId, Vec<ordo_conversio::OrdoMemberFact>>,

    /// Generic type parameters keyed by their declaring struct/enum def, in
    /// declaration order. Absent or empty means the nominal is not generic.
    pub type_params_by_def: FxHashMap<DefId, Vec<Symbol>>,

    /// Sorts of the generic parameters on the function body currently being
    /// checked. Explicit symbolic size arguments carry only a `Symbol` in
    /// `Type::Param`, so the active declaration row is needed to distinguish
    /// a caller-owned `magnitudo` witness from a caller-owned type parameter.
    pub current_generic_param_kinds: FxHashMap<Symbol, radix_syntax::GenericParamKind>,
    /// Type parameters of the genus whose method is being checked. They stay
    /// rigid inside the method body exactly like the method's own generics:
    /// a callee signature that mentions one (`out.append(x)` over `list<T>`)
    /// keeps `T` rather than instantiating a fresh inference variable.
    pub current_genus_type_params: FxHashSet<Symbol>,

    /// `DefIds` of the active function's `magnitudo` (size) generic params.
    /// A size name in value position lowers as a `Path` on its `HirTypeParam`
    /// def, which has no resolver symbol; this set types it (size-type Z1).
    pub current_size_param_defs: FxHashSet<DefId>,
    /// The same `size` parameters by source name, for synthesized extent values.
    pub current_size_param_by_name: FxHashMap<Symbol, DefId>,

    /// Declared return type while checking a function body.
    pub current_return: Option<TypeId>,

    /// Active alternate-exit target while checking `iace` or failable calls.
    pub current_error: Option<ErrorSink>,

    /// Whether the innermost frame being checked is the `main {}` (`incipit`)
    /// body itself (D17.8). `main` declares no `⇥` and is the one frame that
    /// may leave a failable `↦` unhandled: the failure panics (runner, Rust
    /// and TS all abort). A failable call in `main` is still an error until
    /// the targets lower it as a panic (fi6c). Every function, closure, and
    /// `praefixum` body clears it, since errors do not unwind through frames
    /// that do not declare `⇥`.
    pub in_main_frame: bool,

    /// Count of failable calls (a signature with a `⇥` channel) checked so far.
    /// A construct that must reject a failable operand (`vel`/`coalesce`)
    /// compares the count around the operand's check.
    pub failable_calls_checked: u32,

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

    /// Memo for `canonicalize_type` (input `TypeId` → canonical `TypeId`). BO-S-5.
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

    /// Union holes synthesized for a `trap { … }` result whose body has been
    /// fully checked: their accumulated member set is complete, so an `is`
    /// test over a binding of that hole can narrow before finalize solves it.
    pub(super) closed_trap_holes: FxHashSet<InferVar>,

    /// Index shape inference state (parallel to type inference above).
    pub index_infer: IndexInferState,

    /// Compile-time `lista<numerus>` shape witnesses for index constraint sources.
    pub shape_witnesses: FxHashMap<DefId, crate::index::IndexId>,
    /// Statically known live lengths of list literals, retained for shape-hole
    /// inference when a flat source is first bound to a local.
    pub static_collection_lengths: FxHashMap<DefId, u64>,

    /// Loop coordinate binders (`apud [i, j]`) of a tensor iteration, mapped
    /// to the extent of the axis each one walks. A binder is in range of any
    /// tensor whose same axis has that extent (K11 unproven-index warning).
    pub(crate) coordinate_binder_extents: FxHashMap<DefId, IndexId>,

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

    /// Field-path narrowing facts in force for the current branch body (need
    /// c58fff9c): (base binding, root-to-leaf path) -> narrowed type. Inserted
    /// by `check_block_with_refinement`, removed at block exit; joins and the
    /// implicit fallthrough never carry them.
    pub(crate) field_facts: FxHashMap<(DefId, Vec<Symbol>), TypeId>,

    /// Integer interval facts in force at the current point, with the mutation
    /// versions of the sources their bounds name. Snapshotted,
    /// restored and joined with the type facts at every branch edge.
    range_flow: flow_range::RangeFlow,

    /// Conversation types of the `@ ad` routes this module can see, keyed
    /// by route (D6.11). Routes absent here type as bare `sermo`.
    pub ad_routes: FxHashMap<Symbol, sermo_route::AdRouteTypes>,

    /// The `genus` whose members are being checked (its methods and field
    /// defaults); its `@ privata` members are reachable here (D5.3).
    pub current_member_owner: Option<DefId>,

    /// Imported `genus` declarations from another package; their `@ interna`
    /// members are unreachable here (D5.3).
    pub cross_package_structs: FxHashSet<DefId>,

    /// Set by lvalue checking for the field access it is about to check, so
    /// the member-visibility gate reports a write rather than a read.
    pub(crate) place_write: bool,
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
    let mut shape_obligations = crate::ShapeObligationTable::default();
    let mut tensor_edge_policies = crate::TensorEdgePolicyTable::default();
    typecheck_with_config(
        hir,
        resolver,
        types,
        interner,
        &config,
        None,
        None,
        None,
        &mut shape_obligations,
        &mut tensor_edge_policies,
    )
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
    lanes: Option<&crate::RadixLaneMetadata>,
    shape_obligations: &mut crate::ShapeObligationTable,
    tensor_edge_policies: &mut crate::TensorEdgePolicyTable,
) -> Result<(), Vec<SemanticError>> {
    let mut checker = TypeChecker::new(resolver, types, interner, config.target);
    checker.locale_pack = config.locale_pack.as_ref();
    checker.source_text.clone_from(&config.source_text);
    if let Some(lanes) = lanes {
        checker.air_lane_defs = lanes
            .iter()
            .filter(|(_, lane)| *lane == crate::RadixLane::Air)
            .map(|(def_id, _)| def_id)
            .collect();
    }
    checker.collect_imported_file_structs();
    checker.collect_imported_enum_variants();
    checker.collect_imported_file_interfaces();
    if let Some(companions) = companion_signatures {
        checker.register_companion_signatures(companions);
    }
    checker.collect_items(hir);
    if let Some(presentation) = presentation.as_deref() {
        checker
            .compile_time_locals
            .extend(presentation.compile_time_locals.iter().copied());
        checker.collect_ordo_members(hir, presentation);
    }
    checker.check_program(hir);
    checker.discharge_tensor_edge_policy_obligations(hir);
    if let Some(presentation) = presentation {
        checker.finalize_authored_forms(presentation);
        presentation
            .numeric_twin_ids
            .append(&mut checker.numeric_twin_ids);
        presentation
            .empty_dense_ids
            .append(&mut checker.empty_dense_ids);
        presentation
            .inferred_constructor_ids
            .append(&mut checker.inferred_constructor_ids);
        presentation.axis_sugar.append(&mut checker.axis_sugar);
    }
    shape_obligations.clone_from(&checker.shape_obligations);
    tensor_edge_policies.clone_from(&checker.tensor_edge_policies);

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
            const_locals: FxHashSet::default(),
            read_only_param_roots: FxHashSet::default(),
            functions: FxHashMap::default(),
            nucleum_roles: FxHashMap::default(),
            inlinable_kernels: FxHashSet::default(),
            axis_params: FxHashMap::default(),
            method_nucleum_roles: FxHashMap::default(),
            method_defs: FxHashMap::default(),
            instance_method_defs: FxHashSet::default(),
            instance_field_counts: FxHashMap::default(),
            consts: FxHashMap::default(),
            structs: FxHashMap::default(),
            json_structs: FxHashSet::default(),
            interfaces: FxHashMap::default(),
            struct_implements: Vec::new(),
            implemented_interfaces: FxHashMap::default(),
            implemented_contracts: FxHashMap::default(),
            current_generic_bounds: FxHashMap::default(),
            next_hir_id: u32::MAX / 2,
            next_synthetic_def: radix_types::HIR_SYNTHETIC_DEF_ID_BASE,
            pending_store: None,
            current_store_expr: None,
            word_ring: FxHashMap::default(),
            last_synthesized_type: None,
            inferred_slot_sources: FxHashMap::default(),
            source_text: None,
            air_lane_defs: FxHashSet::default(),
            current_air_function: false,
            loop_iterated: Vec::new(),
            current_function_def: None,
            current_shape_call: None,
            twin_probed_receiver: None,
            current_shape_return_expr: None,
            current_shape_argument: None,
            current_shape_fields: Vec::new(),
            current_shape_param_sources: FxHashMap::default(),
            current_shape_param_call_sources: FxHashMap::default(),
            shape_obligations: ShapeObligationTable::default(),
            tensor_edge_policies: crate::TensorEdgePolicyTable::default(),
            tensor_edge_generic_calls: Vec::new(),
            index_proof_suppressed: 0,
            tensor_edge_policy_imported_requirements: FxHashMap::default(),
            tensor_edge_policy_runtime_params: FxHashSet::default(),
            compile_time_locals: FxHashSet::default(),
            static_size_values: FxHashMap::default(),
            const_int_values: FxHashMap::default(),
            module_const_ints: FxHashSet::default(),
            numeric_twin_ids: Vec::new(),
            axis_reduction_results: FxHashMap::default(),
            empty_dense_ids: Vec::new(),
            inferred_constructor_ids: Vec::new(),
            axis_sugar: Vec::new(),
            alias_type_params: FxHashMap::default(),
            import_receivers: FxHashSet::default(),
            trusted_imports: FxHashSet::default(),
            unavailable_functions: FxHashMap::default(),
            unavailable_methods: FxHashMap::default(),
            target,
            variant_fields: FxHashMap::default(),
            variant_field_names: FxHashMap::default(),
            variant_parent: FxHashMap::default(),
            ordo_members: FxHashMap::default(),
            type_params_by_def: FxHashMap::default(),
            current_generic_param_kinds: FxHashMap::default(),
            current_genus_type_params: FxHashSet::default(),
            current_size_param_defs: FxHashSet::default(),
            current_size_param_by_name: FxHashMap::default(),
            current_return: None,
            current_error: None,
            in_main_frame: false,
            failable_calls_checked: 0,
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
            closed_trap_holes: FxHashSet::default(),
            index_infer,
            shape_witnesses: FxHashMap::default(),
            static_collection_lengths: FxHashMap::default(),
            coordinate_binder_extents: FxHashMap::default(),
            errored_exprs: FxHashSet::default(),
            error_type,
            mark_chorda_scalar_closure_param: false,
            locale_pack: None,
            assert_refinements: Vec::new(),
            flow_bases: FxHashMap::default(),
            field_facts: FxHashMap::default(),
            range_flow: flow_range::RangeFlow::default(),
            ad_routes: FxHashMap::default(),
            current_member_owner: None,
            cross_package_structs: FxHashSet::default(),
            place_write: false,
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
        self.cross_package_structs
            .extend(self.resolver.imported_cross_package_structs());
        for (def_id, fields, methods, type_params) in self.resolver.imported_file_structs() {
            let instance_field_count = fields
                .iter()
                .filter(|field| field.modifier != Some(radix_syntax::FieldModifier::Const))
                .count();
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
                            modifier: field.modifier,
                            visibility: field.visibility,
                        },
                    )
                })
                .collect();
            let method_visibility = methods
                .iter()
                .filter(|method| method.visibility != radix_syntax::MemberVisibility::Publica)
                .map(|method| (method.name, method.visibility))
                .collect();
            for method in &methods {
                self.method_defs
                    .insert((def_id, method.name), method.def_id);
                // Imported methods keep their declared `axis` markers so a
                // call through this class echoes `axis` exactly like a local
                // method (polish A-202).
                self.axis_params.insert(method.def_id, method.axis.clone());
                if !method.unavailable.is_empty() {
                    self.unavailable_methods
                        .insert((def_id, method.name), method.unavailable.clone());
                }
            }
            let methods = methods
                .into_iter()
                .map(|method| (method.name, method.sig))
                .collect();
            self.structs.insert(
                def_id,
                StructInfo {
                    fields,
                    methods,
                    method_visibility,
                },
            );
            self.instance_field_counts
                .insert(def_id, instance_field_count);
        }
        // Declared conformance of imported structs: the same pair
        // `resolve_struct_implements` fills for local ones, so an imported
        // struct is assignable only to contracts its own declaration names.
        for (def_id, contracts) in self.resolver.imported_struct_contracts() {
            self.implemented_interfaces.insert(
                def_id,
                contracts.iter().map(|contract| contract.contract).collect(),
            );
            self.implemented_contracts.insert(def_id, contracts);
        }
    }

    /// Register imported enum variants (file-interface enum exports) into the
    /// typechecker's variant tables so `module.VARIANT` value references
    /// typecheck as `Type::Enum(consumer enum def)` (codex-gap S1 U2 VALUE
    /// members).
    pub fn collect_imported_enum_variants(&mut self) {
        for (enum_def, _, variants) in self.resolver.imported_file_enums() {
            // Same registration as a local enum (`collect.rs`): variant
            // construction substitutes applied witnesses through these names.
            if let Some(params) = self.resolver.imported_enum_type_params(enum_def) {
                self.type_params_by_def.insert(enum_def, params.to_vec());
            }
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
                            axis: method.axis,
                            availability: Vec::new(),
                            span: radix_lexer::Span::default(),
                        },
                    )
                })
                .collect();
            self.register_imported_contract_role(def_id);
            if let Some(params) = self.resolver.imported_contract_type_params(def_id) {
                self.type_params_by_def.insert(def_id, params.to_vec());
            }
            self.interfaces.insert(def_id, methods);
        }
    }

    pub fn check_program(&mut self, hir: &mut HirModule) {
        self.next_hir_id = crate::passes::statica::max_hir_id(hir) + 1;
        self.next_synthetic_def = (crate::passes::statica::max_def_id(hir) + 1)
            .max(radix_types::HIR_SYNTHETIC_DEF_ID_BASE);
        self.reject_json_annotations_on_site(&hir.entry_annotations, "incipit");
        self.collect_ad_route_types(hir);
        for item in &mut hir.items {
            self.check_item(item);
        }
        self.warn_closable_cycles(hir);

        if let Some(entry) = &mut hir.entry {
            let prev_await_context = self.current_await_context;
            let prev_await_return_context = self.current_await_return_context;
            self.current_await_context = hir.entry_is_async;
            self.current_await_return_context = false;
            self.in_main_frame = true;
            self.check_block(entry, None);
            self.in_main_frame = false;
            self.current_await_context = prev_await_context;
            self.current_await_return_context = prev_await_return_context;
        }

        self.finalize_hir(hir);
    }
}

fn param_mode_from_hir(mode: HirParamMode) -> SemanticParamMode {
    match mode {
        HirParamMode::Owned | HirParamMode::Axis => SemanticParamMode::Owned,
        HirParamMode::De => SemanticParamMode::Ref,
        HirParamMode::In => SemanticParamMode::MutRef,
        HirParamMode::Ex => SemanticParamMode::Move,
    }
}

#[cfg(test)]
#[path = "test_support.rs"]
pub mod test_support;

#[cfg(test)]
#[path = "access_operand_test.rs"]
mod access_operand_test;

#[cfg(test)]
#[path = "optional_chain_call_test.rs"]
mod optional_chain_call_test;

#[cfg(test)]
#[path = "axis_marker_test.rs"]
mod axis_marker_test;

#[cfg(test)]
#[path = "explicit_size_marker_test.rs"]
mod explicit_size_marker_test;

#[cfg(test)]
#[path = "axis_sugar_test.rs"]
mod axis_sugar_test;

#[cfg(test)]
#[path = "imported_generic_enum_test.rs"]
mod imported_generic_enum_test;

#[cfg(test)]
#[path = "imported_axis_marker_test.rs"]
mod imported_axis_marker_test;

#[cfg(test)]
#[path = "ascii_cursor_test.rs"]
mod ascii_cursor_test;

#[cfg(test)]
#[path = "generator_cursor_test.rs"]
mod generator_cursor_test;

#[cfg(test)]
#[path = "ascii_index_test.rs"]
mod ascii_index_test;

#[cfg(test)]
#[path = "littera_test.rs"]
mod littera_test;

#[cfg(test)]
#[path = "littera_conversio_test.rs"]
mod littera_conversio_test;

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
#[path = "ascription_optional_slot_test.rs"]
mod ascription_optional_slot_test;

#[cfg(test)]
#[path = "element_relaxation_test.rs"]
mod element_relaxation_test;

#[cfg(test)]
#[path = "type_argument_invariance_test.rs"]
mod type_argument_invariance_test;

#[cfg(test)]
#[path = "equality_exact_types_test.rs"]
mod equality_exact_types_test;

#[cfg(test)]
#[path = "decimal_width_literal_test.rs"]
mod decimal_width_literal_test;

#[cfg(test)]
#[path = "decimal_constant_join_test.rs"]
mod decimal_constant_join_test;

#[cfg(test)]
#[path = "float_family_crossing_test.rs"]
mod float_family_crossing_test;

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
#[path = "integer_range_test.rs"]
mod integer_range_test;

#[cfg(test)]
#[path = "potentia_exponent_test.rs"]
mod potentia_exponent_test;

#[cfg(test)]
#[path = "store_position_test.rs"]
mod store_position_test;

#[cfg(test)]
#[path = "g1_repair_test.rs"]
mod g1_repair_test;

#[cfg(test)]
#[path = "generic_call_test.rs"]
mod generic_call_test;

#[cfg(test)]
#[path = "generic_value_callee_test.rs"]
mod generic_value_callee_test;

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
#[path = "hashable_key_test.rs"]
mod hashable_key_test;

#[cfg(test)]
#[path = "tuple_key_index_test.rs"]
mod tuple_key_index_test;

#[cfg(test)]
#[path = "itera_element_test.rs"]
mod itera_element_test;

#[cfg(test)]
#[path = "argmaxima_test.rs"]
mod argmaxima_test;

#[cfg(test)]
#[path = "predicate_twins_test.rs"]
mod predicate_twins_test;

#[cfg(test)]
#[path = "list_extend_test.rs"]
mod list_extend_test;

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
#[path = "compound_guard_narrowing_test.rs"]
mod compound_guard_narrowing_test;
#[cfg(test)]
#[path = "diverging_guard_narrowing_test.rs"]
mod diverging_guard_narrowing_test;
#[cfg(test)]
#[path = "genus_optional_into_plain_test.rs"]
mod genus_optional_into_plain_test;
#[cfg(test)]
#[path = "member_privacy_test.rs"]
mod member_privacy_test;
#[cfg(test)]
#[path = "null_narrowing_test.rs"]
mod null_narrowing_test;
#[cfg(test)]
#[path = "optional_into_plain_test.rs"]
mod optional_into_plain_test;

#[cfg(test)]
#[path = "numeric_value_equality_test.rs"]
mod numeric_value_equality_test;

#[cfg(test)]
#[path = "true_division_test.rs"]
mod true_division_test;

#[cfg(test)]
#[path = "octeti_endian_conversio_test.rs"]
mod octeti_endian_conversio_test;
#[cfg(test)]
#[path = "octeti_tensor_conversio_test.rs"]
mod octeti_tensor_conversio_test;
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
#[path = "rounding_conversio_test.rs"]
mod rounding_conversio_test;

#[cfg(test)]
#[path = "bits_operator_test.rs"]
mod bits_operator_test;

#[cfg(test)]
#[path = "bits_same_width_conversio_test.rs"]
mod bits_same_width_conversio_test;

#[cfg(test)]
#[path = "octeti_tensor_bytes_list_test.rs"]
mod octeti_tensor_bytes_list_test;

#[cfg(test)]
#[path = "ordo_conversio_test.rs"]
mod ordo_conversio_test;

#[cfg(test)]
#[path = "ordo_order_test.rs"]
mod ordo_order_test;

#[cfg(test)]
#[path = "conversio_function_route_test.rs"]
mod conversio_function_route_test;

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
#[path = "ordina_key_test.rs"]
mod ordina_key_test;

#[cfg(test)]
#[path = "scalar_longitudo_test.rs"]
mod scalar_longitudo_test;

#[cfg(test)]
#[path = "chained_intrinsic_receiver_test.rs"]
mod chained_intrinsic_receiver_test;

#[cfg(test)]
#[path = "scalar_unknown_method_test.rs"]
mod scalar_unknown_method_test;

#[cfg(test)]
#[path = "builtin_unknown_method_test.rs"]
mod builtin_unknown_method_test;

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
#[path = "tensor_edge_policy_type_test.rs"]
mod tensor_edge_policy_type_test;

#[cfg(test)]
#[path = "tensor_edge_policy_call_test.rs"]
mod tensor_edge_policy_call_test;

#[cfg(test)]
#[path = "tensor_edge_policy_generic_test.rs"]
mod tensor_edge_policy_generic_test;

#[cfg(test)]
#[path = "tensor_index_width_test.rs"]
mod tensor_index_width_test;

#[cfg(test)]
#[path = "index_param_value_test.rs"]
mod index_param_value_test;

#[cfg(test)]
#[path = "z3_signature_lift_test.rs"]
mod z3_signature_lift_test;

#[cfg(test)]
#[path = "z3_annotation_extent_test.rs"]
mod z3_annotation_extent_test;

#[cfg(test)]
#[path = "z3_anonymous_param_test.rs"]
mod z3_anonymous_param_test;

#[cfg(test)]
#[path = "tensor_lattice_matrix_test.rs"]
mod tensor_lattice_matrix_test;

#[cfg(test)]
#[path = "k10_tensor_shape_law_test.rs"]
mod k10_tensor_shape_law_test;

#[cfg(test)]
#[path = "tensor_lift_test.rs"]
mod tensor_lift_test;

#[cfg(test)]
#[path = "tensor_minus_test.rs"]
mod tensor_minus_test;

#[cfg(test)]
#[path = "scalar_math_test.rs"]
mod scalar_math_test;

#[cfg(test)]
#[path = "float_literal_receiver_test.rs"]
mod float_literal_receiver_test;

#[cfg(test)]
#[path = "tensor_math_test.rs"]
mod tensor_math_test;

#[cfg(test)]
#[path = "aggregate_compare_test.rs"]
mod aggregate_compare_test;

#[cfg(test)]
#[path = "matrix_lift_test.rs"]
mod matrix_lift_test;

#[cfg(test)]
#[path = "vector_lift_test.rs"]
mod vector_lift_test;

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
#[path = "optional_list_literal_test.rs"]
mod optional_list_literal_test;

#[cfg(test)]
#[path = "union_pattern_test.rs"]
mod union_pattern_test;

#[cfg(test)]
#[path = "est_type_target_test.rs"]
mod est_type_target_test;

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

#[cfg(test)]
#[path = "failable_conversio_handler_test.rs"]
mod failable_conversio_handler_test;
#[cfg(test)]
#[path = "inc_dec_decimal_test.rs"]
mod inc_dec_decimal_test;
#[cfg(test)]
#[path = "regex_conversio_failable_test.rs"]
mod regex_conversio_failable_test;
#[cfg(test)]
#[path = "regex_literal_check_test.rs"]
mod regex_literal_check_test;

#[cfg(test)]
#[path = "float_constant_conversio_bounds_test.rs"]
mod float_constant_conversio_bounds_test;

#[cfg(test)]
#[path = "inf_typing_test.rs"]
mod inf_typing_test;

#[cfg(test)]
#[path = "inf_widen_test.rs"]
mod inf_widen_test;

#[cfg(test)]
#[path = "conversio_extra_width_argument_test.rs"]
mod conversio_extra_width_argument_test;

#[cfg(test)]
#[path = "sermo_route_typing_test.rs"]
mod sermo_route_typing_test;

#[cfg(test)]
#[path = "sermo_view_locale_test.rs"]
mod sermo_view_locale_test;

#[cfg(test)]
#[path = "pin_store_test.rs"]
mod pin_store_test;

#[cfg(test)]
#[path = "valor_map_key_test.rs"]
mod valor_map_key_test;

#[cfg(test)]
#[path = "sink_method_test.rs"]
mod sink_method_test;
