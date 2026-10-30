//! Parameter-mode and Rust borrow analysis for typed HIR.
//!
//! Two entry points implement the HIR v1 G2 split:
//!
//! - [`analyze_modes`] — **target-neutral** access-fact policy: use-after-
//!   consume, illegal mutation through `ref`, and incompatible access passing.
//!   Runs for every application-lane target.
//! - [`analyze`] — legacy Rust-target analysis: the declared access facts plus
//!   the retained Rust-lifetime approximation for unmarked values.
//!
//! Precise lifetime reasoning remains Rust's job. Declared access validity is
//! decided before target selection so TS/Go cannot silently accept programs
//! that violate Faber ownership facts.
//!
//! ERROR STRATEGY
//! ==============
//! - Ownership violations are emitted as semantic errors.
//! - Parameter-mode suggestions are emitted as semantic warnings.
//! - Declared `ref` type facts (MU-2) reject projection assignment and
//!   passing into `mut`/`own` positions. Mutating-receiver bans live with
//!   catalog lookup in typecheck; this pass does not key them on void return.
//! - Declared `own` type facts (MU-3) transfer at bind sites and own-param
//!   calls. SEM050 (`use_after_move`) is a language rule, mode-scoped to
//!   exclusive (`own`/`copy`) values. Unmarked Move-mode args still move only
//!   under the Rust-lifetime qualification.
//! - Declared `mut` type facts (MU-4) make the mutable-borrow-conflict family
//!   (SEM052) a language rule, mode-scoped to exclusive-borrow operations:
//!   mut-local binds, `mut`/`ref` dest calls, and explicit refs of `mut` roots.
//!   Unmarked overlapping `&`/`&mut` still fire only under Rust-lifetime
//!   qualification. `mut` locals parse and warn (OQ-3).
//! - The pass returns all findings together so the semantic driver can apply
//!   the repository-wide warning/error policy through `SemanticErrorKind`.
//!
//! INVARIANTS
//! ==========
//! - Borrow state is keyed by root `DefId`; field, index, and deref
//!   projections borrow or move the base value.
//! - Borrow lifetimes are scope approximations: explicit borrow expressions are
//!   registered in the current `BorrowScope` and released when that HIR block
//!   exits.
//! - Mutating method calls are receiver writes, not retained borrows; this
//!   keeps statement-position collection intrinsics such as `xs.appende(v)`
//!   reusable within one block. An intrinsic call is a write when the registry
//!   says so (`is_mutating_receiver`, including value-returning mutators like
//!   `detrahe`); only a method with no registry row falls back to its vacuum
//!   return.
//! - The `TypeTable` is consulted for callee signatures, because argument
//!   modes determine whether a call reads, mutably borrows, or moves an input,
//!   and for receiver types to resolve the intrinsic registry row.
//! - Marked-value borrows (`mut`/`ref` local binds and explicit refs of
//!   marked roots) route their conflict decisions through the BRI-2 region
//!   engine when region facts are supplied: a borrow conflicts only while its
//!   region is live at the consuming statement, so last-use release replaces
//!   the old block-exit approximation inside cyclic bodies. When no region
//!   facts are available the previous block-scope approximation decides.
//! - Warnings produced here remain non-fatal unless the semantic driver or
//!   diagnostic consumer treats warning-kind diagnostics as errors.

#[path = "borrow_own_arg.rs"]
mod own_arg;

use self::own_arg::OwnArgState;
use super::borrow_region::RegionFacts;
use super::borrow_region_cfg::CfgNodeId;

use crate::scope::ImportedFileInterfaceMethod;
use crate::{
    AccessMarker, FuncSig, ISSUE_ASSIGN_THROUGH_REF, ISSUE_ASSIGN_TO_DE_PARAMETER,
    ISSUE_MUT_INTO_OWN, ISSUE_MUT_LOCAL, ISSUE_REF_TO_MUT_OR_OWN_POSITION, ISSUE_UNMARKED_INTO_OWN,
    ParamType, Primitive, Resolver, SUGGESTION_COPY, SemanticError, SemanticErrorKind,
    SemanticParamMode, Type, TypeId, TypeTable, WarningKind,
};
use handroll::{FxHashMap, FxHashSet};
use radix_hir::visit::{HirVisitor, walk_expr};
use radix_hir::{
    DefId, HirBlock, HirCallArg, HirExpression, HirExpressionKind, HirFunction, HirItem,
    HirItemKind, HirIteraMode, HirModule, HirParamMode, HirStatement, HirStatementKind,
};
use radix_intrinsics::{is_mutating_receiver, lookup_method_for_type_id};
use radix_lexer::Interner;

const ISSUE_BORROW_OF_MOVED: &str = "borrow_of_moved";
const ISSUE_CANNOT_MOVE_WHILE_BORROWED: &str = "cannot_move_while_borrowed";
const ISSUE_DE_TO_IN_OR_EX_POSITION: &str = "de_to_in_or_ex_position";
const ISSUE_MUTABLE_BORROW_CONFLICT: &str = "mutable_borrow_conflict";
const ISSUE_SHARED_BORROW_CONFLICT: &str = "shared_borrow_conflict";
const ISSUE_UNUSED_OWN_PARAMETER: &str = "unused_own_parameter";
const ISSUE_UNUSED_MUT_PARAMETER: &str = "unused_mut_parameter";
const ISSUE_USE_AFTER_MOVE: &str = "use_after_move";
const ISSUE_USE_WHILE_MUTABLY_BORROWED: &str = "use_while_mutably_borrowed";
const ISSUE_WRITE_WHILE_BORROWED: &str = "write_while_borrowed";
const ISSUE_TENSOR_VIEW_ESCAPE: &str = "tensor_view_escape";
const ISSUE_TENSOR_VIEW_READ_ONLY: &str = "tensor_view_read_only";
/// R2: the left side of `⇇` is a view, which is never writable storage.
const ISSUE_COPY_INTO_TARGET_VIEW: &str = "copy_into_target_view";
/// R2: the left side of `⇇` is not a named storage place.
const ISSUE_COPY_INTO_TARGET_NOT_STORAGE: &str = "copy_into_target_not_storage";
/// R3: the right side of `⇇` reads the storage the left side writes.
const ISSUE_COPY_INTO_ALIAS: &str = "copy_into_alias";
/// R6: a whole tensor is rebound into a `mut` tensor parameter by `←` inside a kernel.
const ISSUE_MUT_TENSOR_PARAM_REBIND: &str = "mut_tensor_param_rebind";
const ISSUE_ASSIGN_TO_NONMUT_TENSOR_PARAM: &str = "assign_to_nonmut_tensor_param";
const ISSUE_IMMUTABLE_SECOND_ASSIGNMENT: &str = "immutable_second_assignment";
const ISSUE_MUT_ARGUMENT_NOT_WRITABLE: &str = "mut_argument_not_writable";
/// Ruling 7b25889f R6 (K1): a writable kernel argument overlaps another
/// argument of the same call.
const ISSUE_KERNEL_ARGUMENT_OVERLAP: &str = "kernel_argument_overlap";
const ISSUE_MUTATING_METHOD_ON_IMMUTABLE: &str =
    "imperativus_operator_method_requires_varia_receiver";

/// A named storage place: a binding plus the field path under it.
type PlaceKey = (DefId, Vec<radix_lexer::Symbol>);

/// Target-neutral Faber access-fact analysis.
///
/// Language `ref`/`mut`/`own`/`copy` facts run here. Rust lifetime/borrow
/// conflict approximation for unmarked values does not. Use this during base
/// analysis so all HIR targets share access validity.
///
/// # Errors
///
/// Returns access violations as a list of [`SemanticError`]s.
pub fn analyze_modes(
    hir: &HirModule,
    resolver: &Resolver,
    types: &TypeTable,
    interner: &Interner,
) -> Result<(), Vec<SemanticError>> {
    run(resolver, hir, types, interner, false, None)
}

/// Region-routed [`analyze_modes`]: marked-value borrow conflicts consult the
/// supplied region facts (produced by BRI-2 from the one `AccessFactTable`).
///
/// # Errors
///
/// Returns access violations as a list of [`SemanticError`]s.
pub fn analyze_modes_with_regions(
    hir: &HirModule,
    resolver: &Resolver,
    types: &TypeTable,
    interner: &Interner,
    regions: Option<&RegionFacts>,
) -> Result<(), Vec<SemanticError>> {
    run(resolver, hir, types, interner, false, regions)
}

/// Legacy Rust-target analysis: declared access facts plus the Rust lifetime
/// approximation used by [`PassConfig::for_target`](crate::PassConfig::for_target).
/// Target-neutral callers use [`analyze_modes`], while the explicit qualification
/// seam uses [`analyze_rust_lifetime`].
///
/// # Errors
///
/// Returns borrow/lifetime violations as a list of [`SemanticError`]s.
pub fn analyze(
    hir: &HirModule,
    resolver: &Resolver,
    types: &TypeTable,
    interner: &Interner,
) -> Result<(), Vec<SemanticError>> {
    run(resolver, hir, types, interner, true, None)
}

/// Region-routed [`analyze`]: marked-value borrow conflicts consult the
/// supplied region facts while the Rust-lifetime simulation keeps its
/// existing call-scoped behavior.
///
/// # Errors
///
/// Returns borrow/lifetime violations as a list of [`SemanticError`]s.
pub fn analyze_with_regions(
    hir: &HirModule,
    resolver: &Resolver,
    types: &TypeTable,
    interner: &Interner,
    regions: Option<&RegionFacts>,
) -> Result<(), Vec<SemanticError>> {
    run(resolver, hir, types, interner, true, regions)
}

/// Retired Rust lifetime simulation hook.
///
/// The driver still calls this target-qualification seam, but semantic
/// lifetime simulation is no longer policy. Generated Rust and its compiler
/// own precise borrow checking; declared `ref`/`mut`/`own`/`copy` facts are
/// enforced by [`analyze_modes`] instead.
///
/// # Errors
///
/// Always returns `Ok(())`; the result type remains for the qualification API.
pub fn analyze_rust_lifetime(
    _hir: &HirModule,
    _resolver: &Resolver,
    _types: &TypeTable,
) -> Result<(), Vec<SemanticError>> {
    Ok(())
}

fn run(
    resolver: &Resolver,
    hir: &HirModule,
    types: &TypeTable,
    interner: &Interner,
    rust_lifetime: bool,
    regions: Option<&RegionFacts>,
) -> Result<(), Vec<SemanticError>> {
    let mut checker = BorrowChecker::new(resolver, types, interner, rust_lifetime, regions);
    checker.check_program(hir);

    if checker.errors.is_empty() {
        Ok(())
    } else {
        Err(checker.errors)
    }
}

/// How a binding's `bind_alias_roots` entry is written.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Rebind {
    /// A new declaration: no earlier name or entry exists.
    Fresh,
    /// A rebind that runs whenever control reaches it.
    Must,
    /// A rebind that may not run, or whose effect may not stay local.
    May,
}

/// Per-definition ownership state tracked at the root value level.
///
/// The pass intentionally does not model partial moves or field-granular
/// borrowing. Projections collapse through `root_def_id`, which keeps the
/// pre-codegen check conservative and aligned with the current HIR surface.
#[derive(Clone, Default)]
struct BorrowState {
    moved: bool,
    shared: u32,
    mutable: bool,
    /// Marked-value borrows whose conflict decision is routed through the
    /// region engine, keyed by the borrowing local. Missing or live-less
    /// region facts fall back to the block-scope approximation.
    region_borrows: Vec<TrackedRegionBorrow>,
}

/// One region-routed borrow retained on the source's [`BorrowState`].
#[derive(Clone, Copy, PartialEq, Eq)]
struct TrackedRegionBorrow {
    borrower: DefId,
    source: DefId,
    kind: BorrowKind,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BorrowKind {
    Shared,
    Mutable,
}

/// A borrow registered against the innermost [`BorrowScope`].
///
/// `Region` entries borrow the block-scope lifetime (they are dropped when
/// the block exits) but their conflict decision while in scope belongs to the
/// region engine.
#[derive(Clone, Copy)]
enum ScopeBorrow {
    Flagged(DefId, BorrowKind),
    Region { source: DefId, borrower: DefId },
}

struct BorrowScope {
    borrows: Vec<ScopeBorrow>,
}

struct BorrowChecker<'a> {
    /// Semantic type table produced before analysis; call signatures determine
    /// whether each argument is read, borrowed, or moved.
    types: &'a TypeTable,
    /// Resolves method-call symbols so receiver writes key on the intrinsic
    /// registry (`is_mutating_receiver`), not on the call's return type.
    interner: &'a Interner,
    /// Legacy Rust-lifetime approximation: explicit refs of unmarked values,
    /// typed-callee argument borrows, and unmarked Move-mode consumption.
    /// [`analyze`] sets it; target-neutral [`analyze_modes`] does not.
    rust_lifetime: bool,
    states: FxHashMap<DefId, BorrowState>,
    scopes: Vec<BorrowScope>,
    param_usage: FxHashMap<DefId, ParamUsage>,
    /// Source names of parameters and locals, so conflict diagnostics can name
    /// the place whose borrow is live.
    names: FxHashMap<DefId, radix_lexer::Symbol>,
    /// Bindings whose declared type carries a `ref` access fact.
    ref_roots: FxHashSet<DefId>,
    /// Bindings whose declared type carries a `mut` access fact.
    mut_roots: FxHashSet<DefId>,
    /// Bindings whose declared type is an exclusive `own`/`copy` value.
    own_roots: FxHashSet<DefId>,
    /// Bindings whose declared type acquires a duplicate-then-own `copy` value.
    copy_roots: FxHashSet<DefId>,
    /// Unmarked sources used as an own-bind RHS; a later use proves them live.
    unmarked_own_binds: FxHashMap<DefId, radix_lexer::Span>,
    /// `own` call-argument rules: dead names, unpassable bindings, closures.
    own: OwnArgState,
    /// Unmarked reference-model locals (genus, contract, collection, tensor) bound (D12.1 alias) from a `de`
    /// parameter or another such alias. The alias carries the source's
    /// read-only access, so mutation through it is rejected like direct
    /// mutation of the parameter.
    de_aliases: FxHashSet<DefId>,
    /// Read-only tensor views and their backing place. Chained views and
    /// immutable aliases retain the original root until `materialize()`.
    tensor_view_roots: FxHashMap<DefId, FxHashSet<DefId>>,
    /// Bindings of an aliasing type (genus, contract, collection, tensor, each
    /// also through `T ∪ nihil`) bound by `←` to another place: the binding
    /// shares that place's storage (reference model), so a write through
    /// either name reaches both. Closed transitively when the alias is
    /// recorded. Value types (scalars, text) copy and never enter this table.
    bind_alias_roots: FxHashMap<DefId, FxHashSet<DefId>>,
    /// Containers initialised from a list or tuple literal, mapped to the
    /// bindings whose storage the literal's elements share. An element read
    /// `xs[i]` names every one of them (a conservative union: any element may
    /// alias any listed place). Written only by literal initialisers and
    /// rebinds of the container; no flow through calls or element writes.
    element_alias_roots: FxHashMap<DefId, FxHashSet<DefId>>,
    /// Nesting depth of the statement being checked: zero only for a
    /// straight-line `name ← place` rebind written as its own statement in a
    /// block that no branch, loop, closure or short-circuit guards. A rebind
    /// at depth zero is a must-rebind; anywhere deeper it only may happen.
    flow_depth: u32,
    /// Parameters declared `mut` (`in` mode or a `mut` access marker).
    mut_params: FxHashSet<DefId>,
    /// The function being checked is a kernel or a kernel fragment: its
    /// parameters are device buffers with no binding to rebind.
    in_kernel: bool,
    /// Tensor parameters written without any access marker (not `mut`, `own`,
    /// `copy`, `ref`, `in` or `de`): read-only, so no write reaches the caller.
    nonmut_tensor_params: FxHashSet<DefId>,
    /// Unmarked tensor `←` aliases of a non-`mut` tensor parameter (directly or
    /// through another alias), mapped to that parameter: a write through the
    /// alias is a write to the read-only parameter.
    nonmut_tensor_aliases: FxHashMap<DefId, DefId>,
    /// Non-tensor aggregate parameters (list, map, set, object, contract)
    /// without any access marker: shared read-only, like the tensor case. A
    /// direct write is rejected by typecheck; the borrow pass owns the alias
    /// shapes that slip past it.
    nonmut_agg_params: FxHashSet<DefId>,
    /// Unmarked `←` aliases of a non-`mut` aggregate parameter (directly or
    /// through another alias), mapped to that parameter.
    nonmut_agg_aliases: FxHashMap<DefId, DefId>,
    /// Aggregate aliases bound `const`: typecheck already rejects every write
    /// through them, so the alias write checks skip them (chains still pass
    /// through).
    const_agg_aliases: FxHashSet<DefId>,
    /// Locals declared `const`/`fixum`: not a writable place, so they cannot be
    /// passed to a `mut` parameter.
    immutable_locals: FxHashSet<DefId>,
    /// Parameters with no access marker (read-only to the callee): not a
    /// writable place either.
    unmarked_params: FxHashSet<DefId>,
    /// Function param contracts keyed by callee `DefId` (named calls leave
    /// `callee.ty` unset; this index supplies own-param dest facts).
    func_params: FxHashMap<DefId, Vec<ParamType>>,
    /// Kernel and kernel-fragment functions of this unit, keyed by callee
    /// `DefId`: their arguments are device buffers live simultaneously for a
    /// whole launch, so their call sites run the kernel argument exclusivity
    /// rule (ruling 7b25889f R6 K1).
    kernel_fns: FxHashSet<DefId>,
    /// Declared class-method param contracts keyed by `(class, method name)`: a
    /// user `MethodCall` carries no callee expression, so the call-site rules
    /// resolve its parameters from the receiver's class.
    method_params: FxHashMap<(DefId, radix_lexer::Symbol), Vec<ParamType>>,
    /// Imported class-method param contracts keyed like `method_params`
    /// (R02). The declared index holds only methods whose class is declared in
    /// this HIR unit; a receiver whose class arrives through a file interface
    /// resolves here so its call site gets the same overlap checking.
    imported_method_params: FxHashMap<(DefId, radix_lexer::Symbol), Vec<ParamType>>,
    /// Resolved names and imported file interfaces of the analyzed unit.
    resolver: &'a Resolver,
    /// BRI-2 region facts consumed (never reconstructed) for marked-value
    /// conflict decisions; `None` keeps the block-scope approximation.
    regions: Option<&'a RegionFacts>,
    /// CFG node of the statement under analysis, used for region-liveness
    /// queries. `None` (or a missing region) is conservatively "live".
    stmt_node: Option<CfgNodeId>,
    errors: Vec<SemanticError>,
}

/// Usage summary for one function parameter after traversing its body.
///
/// This is policy state rather than ownership state: it decides whether the
/// declared access fact is stronger than the function actually needs.
#[derive(Clone, Copy)]
struct ParamUsage {
    mode: HirParamMode,
    marker: Option<AccessMarker>,
    span: radix_lexer::Span,
    mutated: bool,
    passed_in_or_ex: bool,
    passed_ex: bool,
    returned: bool,
    moved: bool,
}

impl<'a> BorrowChecker<'a> {
    fn new(
        resolver: &'a Resolver,
        types: &'a TypeTable,
        interner: &'a Interner,
        rust_lifetime: bool,
        regions: Option<&'a RegionFacts>,
    ) -> Self {
        Self {
            types,
            interner,
            rust_lifetime,
            states: FxHashMap::default(),
            scopes: Vec::new(),
            param_usage: FxHashMap::default(),
            names: FxHashMap::default(),
            ref_roots: FxHashSet::default(),
            mut_roots: FxHashSet::default(),
            own_roots: FxHashSet::default(),
            copy_roots: FxHashSet::default(),
            unmarked_own_binds: FxHashMap::default(),
            own: OwnArgState::default(),
            de_aliases: FxHashSet::default(),
            tensor_view_roots: FxHashMap::default(),
            bind_alias_roots: FxHashMap::default(),
            element_alias_roots: FxHashMap::default(),
            flow_depth: 0,
            mut_params: FxHashSet::default(),
            in_kernel: false,
            nonmut_tensor_params: FxHashSet::default(),
            nonmut_tensor_aliases: FxHashMap::default(),
            nonmut_agg_params: FxHashSet::default(),
            nonmut_agg_aliases: FxHashMap::default(),
            const_agg_aliases: FxHashSet::default(),
            immutable_locals: FxHashSet::default(),
            unmarked_params: FxHashSet::default(),
            func_params: FxHashMap::default(),
            kernel_fns: FxHashSet::default(),
            method_params: FxHashMap::default(),
            imported_method_params: FxHashMap::default(),
            resolver,
            regions,
            stmt_node: None,
            errors: Vec::new(),
        }
    }

    fn check_program(&mut self, hir: &HirModule) {
        self.index_imported_methods();
        for item in &hir.items {
            self.index_item(item);
        }
        for item in &hir.items {
            self.check_item(item);
        }

        if let Some(entry) = &hir.entry {
            self.reset();
            self.check_block(entry);
        }
    }

    fn index_item(&mut self, item: &HirItem) {
        match &item.kind {
            HirItemKind::Function(func) => self.index_function(item.def_id, func),
            HirItemKind::Struct(strukt) => {
                for method in &strukt.methods {
                    self.index_function(method.def_id, &method.func);
                    self.method_params.insert(
                        (item.def_id, method.func.name),
                        Self::declared_params(&method.func),
                    );
                }
            }
            HirItemKind::Constant(_) => self.own.note_module_const(item.def_id),
            _ => {}
        }
    }

    fn index_function(&mut self, def_id: DefId, func: &HirFunction) {
        self.func_params.insert(def_id, Self::declared_params(func));
        if func.is_kernel || func.nucleum_fragment {
            self.kernel_fns.insert(def_id);
        }
    }

    /// Seed `imported_method_params` from the resolver's file interfaces.
    ///
    /// Both the consumer-locale surface spelling and the canonical library
    /// name key a row, mirroring `Resolver::imported_method_canonical_name`:
    /// the HIR method-call symbol is the source spelling, and either may be
    /// requested.
    fn index_imported_methods(&mut self) {
        let seed = |imported: &mut FxHashMap<(DefId, radix_lexer::Symbol), Vec<ParamType>>,
                    class: DefId,
                    methods: &[ImportedFileInterfaceMethod]| {
            for method in methods {
                let params = method.sig.params.clone();
                imported
                    .entry((class, method.surface_name))
                    .or_insert(params.clone());
                imported.entry((class, method.name)).or_insert(params);
            }
        };
        for (class, _, methods, _) in self.resolver.imported_file_structs() {
            seed(&mut self.imported_method_params, class, &methods);
        }
        for (class, methods) in self.resolver.imported_file_interfaces() {
            seed(&mut self.imported_method_params, class, &methods);
        }
    }

    fn declared_params(func: &HirFunction) -> Vec<ParamType> {
        func.params
            .iter()
            .map(|param| ParamType {
                ty: param.ty,
                mode: match param.mode {
                    HirParamMode::Owned | HirParamMode::Axis => SemanticParamMode::Owned,
                    HirParamMode::De => SemanticParamMode::Ref,
                    HirParamMode::In => SemanticParamMode::MutRef,
                    HirParamMode::Ex => SemanticParamMode::Move,
                },
                optional: param.optional,
                sponte: param.sponte,
            })
            .collect()
    }

    fn check_item(&mut self, item: &HirItem) {
        match &item.kind {
            HirItemKind::Function(func) => self.check_function(func),
            HirItemKind::Struct(strukt) => {
                for method in &strukt.methods {
                    self.check_function(&method.func);
                }
            }
            HirItemKind::Constant(const_item) => {
                self.reset();
                self.check_expr(&const_item.value);
            }
            _ => {}
        }
    }

    fn check_function(&mut self, func: &HirFunction) {
        self.reset();
        self.in_kernel = func.is_kernel || func.nucleum_fragment;
        for param in &func.params {
            self.names.insert(param.def_id, param.name);
            self.ensure_state(param.def_id);
            self.note_ref_root(param.def_id, param.ty);
            self.note_mut_root(param.def_id, param.ty);
            self.note_copy_root(param.def_id, param.ty);
            self.note_own_root(param.def_id, param.ty);
            if matches!(param.mode, HirParamMode::In) || self.types.is_mut_view(param.ty) {
                self.mut_params.insert(param.def_id);
            }
            let (marker, inner) = self.types.peel_access_marker(param.ty);
            if matches!(param.mode, HirParamMode::Owned | HirParamMode::Axis) && marker.is_none() {
                self.unmarked_params.insert(param.def_id);
                self.own.note_unmarked_param(param.def_id);
            }
            if matches!(param.mode, HirParamMode::Owned | HirParamMode::Axis)
                && marker.is_none()
                && matches!(self.types.get(inner), Type::Tensor(_, _))
            {
                self.nonmut_tensor_params.insert(param.def_id);
            }
            if matches!(param.mode, HirParamMode::Owned | HirParamMode::Axis)
                && marker.is_none()
                && !matches!(self.types.get(inner), Type::Tensor(_, _))
                && self.is_aliasing_type(inner)
            {
                self.nonmut_agg_params.insert(param.def_id);
            }
            self.param_usage.insert(
                param.def_id,
                ParamUsage {
                    mode: param.mode,
                    marker,
                    span: param.span,
                    mutated: false,
                    passed_in_or_ex: false,
                    passed_ex: false,
                    returned: false,
                    moved: false,
                },
            );
        }
        if let Some(body) = &func.body {
            self.check_block(body);
            self.emit_mode_lints();
        }
    }

    fn reset(&mut self) {
        self.states.clear();
        self.scopes.clear();
        self.param_usage.clear();
        self.names.clear();
        self.ref_roots.clear();
        self.mut_roots.clear();
        self.own_roots.clear();
        self.copy_roots.clear();
        self.unmarked_own_binds.clear();
        self.own.reset();
        self.de_aliases.clear();
        self.tensor_view_roots.clear();
        self.bind_alias_roots.clear();
        self.element_alias_roots.clear();
        self.flow_depth = 0;
        self.mut_params.clear();
        self.in_kernel = false;
        self.nonmut_tensor_params.clear();
        self.nonmut_tensor_aliases.clear();
        self.nonmut_agg_params.clear();
        self.nonmut_agg_aliases.clear();
        self.const_agg_aliases.clear();
        self.immutable_locals.clear();
        self.unmarked_params.clear();
        self.stmt_node = None;
    }

    fn check_block(&mut self, block: &HirBlock) {
        self.push_scope();
        for stmt in &block.statements {
            self.check_stmt(stmt);
        }
        if let Some(expr) = &block.expr {
            self.check_expr(expr);
        }
        self.pop_scope();
    }

    fn check_stmt(&mut self, stmt: &HirStatement) {
        // A statement-level `name ← value` keeps the depth: it is the one
        // statement shape whose rebind is a must-rebind. Any other statement
        // may guard what it contains, so everything nested in it is deeper.
        let straight = matches!(
            &stmt.kind,
            HirStatementKind::Expr(expr) if matches!(expr.kind, HirExpressionKind::Assign(..))
        );
        if !straight {
            self.flow_depth += 1;
        }
        self.check_stmt_inner(stmt);
        if !straight {
            self.flow_depth -= 1;
        }
    }

    fn check_stmt_inner(&mut self, stmt: &HirStatement) {
        self.stmt_node = self
            .regions
            .and_then(|facts| facts.cfg().node_for_hir_id(stmt.id));
        match &stmt.kind {
            HirStatementKind::Local(local) => {
                self.check_local_decl(local, stmt.span);
                if let Some(init) = &local.init {
                    self.check_local_init(local, init);
                }
            }
            HirStatementKind::Expr(expr) => {
                self.check_statement_fill(expr);
                self.check_expr(expr);
            }
            HirStatementKind::Redde(value) => {
                if let Some(expr) = value {
                    if self.stores_tensor_view(expr) {
                        self.error(
                            SemanticErrorKind::ModeMismatch,
                            ISSUE_TENSOR_VIEW_ESCAPE,
                            expr.span,
                        );
                    }
                    if let Some(def_id) = Self::root_def_id(expr)
                        && let Some(usage) = self.param_usage.get_mut(&def_id)
                    {
                        usage.returned = true;
                    }
                    self.check_expr(expr);
                }
            }
            HirStatementKind::ReddeFilum(value) => {
                if let Some(expr) = value {
                    self.check_expr(expr);
                }
            }
            HirStatementKind::Rumpe | HirStatementKind::Perge | HirStatementKind::Tacet => {}
            HirStatementKind::IncDec(inc_dec) => self.check_lvalue(&inc_dec.target),
            HirStatementKind::Custodi(custodi) => {
                for clause in &custodi.clauses {
                    self.check_expr(&clause.cond);
                    self.check_block(&clause.body);
                }
            }
        }
    }

    fn check_local_decl(&mut self, local: &radix_hir::HirLocal, stmt_span: radix_lexer::Span) {
        self.names.insert(local.def_id, local.name);
        if !local.mutable {
            self.immutable_locals.insert(local.def_id);
        }
        self.ensure_state(local.def_id);
        if let Some(HirExpression {
            kind: HirExpressionKind::Clausura(_, _, _, closure_body),
            ..
        }) = &local.init
        {
            let known = self.states.keys().copied().collect();
            self.note_closure_captures(local.def_id, closure_body, &known);
        }
        if let Some(ty) = local.ty {
            self.note_ref_root(local.def_id, ty);
            self.note_mut_root(local.def_id, ty);
            self.note_copy_root(local.def_id, ty);
            self.note_own_root(local.def_id, ty);
            if self.types.is_mut_view(ty) {
                self.warn_mut_local(stmt_span);
            }
        }
    }

    fn check_local_init(&mut self, local: &radix_hir::HirLocal, init: &HirExpression) {
        if local.ty.is_some_and(|ty| self.types.is_mut_view(ty))
            && let Some(param) = Self::root_def_id(init).and_then(|d| self.nonmut_tensor_origin(d))
        {
            self.nonmut_tensor_write_error(param, init.span);
        }
        if local.ty.is_some_and(|ty| self.types.is_mut_view(ty))
            && let Some(param) = Self::root_def_id(init).and_then(|d| self.nonmut_agg_origin(d))
        {
            self.nonmut_agg_write_error(param, init.span);
        }
        if self.stores_tensor_view(init) {
            if !self.is_tensor_view(init) {
                self.error(
                    SemanticErrorKind::ModeMismatch,
                    ISSUE_TENSOR_VIEW_ESCAPE,
                    init.span,
                );
                self.check_expr(init);
                return;
            }
            if local.mutable || local.ty.is_some_and(|ty| self.types.is_mut_or_own_view(ty)) {
                self.error(
                    SemanticErrorKind::ModeMismatch,
                    ISSUE_TENSOR_VIEW_ESCAPE,
                    init.span,
                );
                self.check_expr(init);
                return;
            }
            let roots = self.tensor_view_origins(init);
            if roots.is_empty() {
                self.error(
                    SemanticErrorKind::ModeMismatch,
                    ISSUE_TENSOR_VIEW_ESCAPE,
                    init.span,
                );
                self.check_expr(init);
                return;
            }
            self.tensor_view_roots.insert(local.def_id, roots.clone());
            self.check_expr(init);
            for root in roots {
                self.borrow_shared_with(root, Some(local.def_id), init.span);
            }
            return;
        }
        // Region-routed explicit-ref bind of a marked root: the
        // borrowing local's region releases the conflict at last
        // use instead of the old block-exit approximation.
        if let HirExpressionKind::Ref(kind, inner) = &init.kind
            && let Some(source) = Self::root_def_id(inner)
            && self.tracks_explicit_ref(source)
            && (self.mut_roots.contains(&source) || self.ref_roots.contains(&source))
        {
            self.check_projection_inputs(inner);
            match kind {
                radix_hir::HirRefKind::Shared => {
                    self.borrow_shared_with(source, Some(local.def_id), init.span);
                }
                radix_hir::HirRefKind::Mutable => {
                    self.borrow_mut_with(source, Some(local.def_id), init.span);
                }
            }
            return;
        }
        if local.ty.is_some_and(|ty| self.types.is_copy_view(ty)) {
            self.bind_copy_from(init);
        } else if local.ty.is_some_and(|ty| self.types.is_own_view(ty)) {
            self.bind_own_from(init);
        } else if local.ty.is_some_and(|ty| self.types.is_mut_view(ty)) {
            self.bind_mut_from(init, Some(local.def_id));
        } else if local.ty.is_some_and(|ty| self.types.is_ref_view(ty)) {
            self.bind_ref_from(init, Some(local.def_id));
        } else {
            self.check_expr(init);
        }
        self.note_de_alias(local.def_id, local.ty, init);
        self.note_bind_alias(local.def_id, local.ty, init, Rebind::Fresh);
        if !local.mutable {
            self.const_agg_aliases.insert(local.def_id);
        }
    }

    fn check_expr(&mut self, expr: &HirExpression) {
        self.visit_expr(expr);
    }

    fn callee_signature(types: &TypeTable, ty: TypeId) -> Option<&FuncSig> {
        match types.get(ty) {
            Type::Func(sig) => Some(sig),
            Type::Alias(_, resolved) => match types.get(*resolved) {
                Type::Func(sig) => Some(sig),
                _ => None,
            },
            _ => None,
        }
    }

    fn call_params(&self, callee: &HirExpression) -> Option<&[ParamType]> {
        if let Some(ty) = callee.ty {
            return Self::callee_signature(self.types, ty).map(|sig| sig.params.as_slice());
        }
        match &callee.kind {
            HirExpressionKind::Path(def_id) => self.func_params.get(def_id).map(Vec::as_slice),
            _ => None,
        }
    }

    fn check_call_args(&mut self, callee: &HirExpression, args: &[HirCallArg]) {
        let Some(params) = self.call_params(callee) else {
            for arg in args {
                self.check_arg_expr(arg);
            }
            return;
        };
        let params = params.to_vec();
        if matches!(&callee.kind, HirExpressionKind::Path(def_id)
            if self.kernel_fns.contains(def_id))
        {
            self.check_kernel_argument_exclusivity(&params, args);
        }
        // Named function Paths leave `callee.ty` unset. Applying the retired
        // lifetime simulation to those calls would reject D-1 unmarked
        // `const` → `mut` param use. Language `mut` dest borrows are
        // call-scoped so sequential use after a single `mut` param call stays
        // legal. Own-param transfers still run below.
        self.check_args_against(&params, callee.ty.is_some(), args);
    }

    /// The parameters a user `MethodCall` is checked against: the receiver's
    /// class declares the method in this module, or the class arrived through
    /// an imported file interface (R02).
    fn user_method_params(
        &self,
        receiver: &HirExpression,
        method: radix_lexer::Symbol,
    ) -> Option<Vec<ParamType>> {
        let class = self.receiver_class_def(receiver.ty?)?;
        if let Some(params) = self.method_params.get(&(class, method)) {
            return Some(params.clone());
        }
        self.imported_method_params.get(&(class, method)).cloned()
    }

    /// Definition a receiver type names for method lookup: the genus behind
    /// it, or the contract (`implendum`) when the receiver is contract-typed.
    ///
    /// `method_receiver_mutation::class_def` reports genera only, so an
    /// imported contract receiver (`Type::Interface`) would miss the
    /// contract's own parameter row seeded by `index_imported_methods`.
    fn receiver_class_def(&self, ty: TypeId) -> Option<DefId> {
        super::method_receiver_mutation::class_def(self.types, ty)
            .or_else(|| Self::contract_def(self.types, ty))
    }

    /// Contract definition behind a receiver type, peeling the same access
    /// markers, aliases, and generic applications `class_def` peels.
    fn contract_def(types: &TypeTable, ty: TypeId) -> Option<DefId> {
        match types.get(ty) {
            Type::Interface(def) => Some(*def),
            Type::Ref(_, inner) | Type::Alias(_, inner) => Self::contract_def(types, *inner),
            Type::Applied(base, _) => Self::contract_def(types, *base),
            _ => None,
        }
    }

    fn check_args_against(&mut self, params: &[ParamType], rust_sim: bool, args: &[HirCallArg]) {
        self.check_tensor_view_arg_aliasing(args, params);
        let language_call_borrows = !(rust_sim && self.rust_lifetime);
        // Their mut/ref dest-call borrows are call-scoped in every pass: real
        // Rust releases an argument's `&mut`/`&` reborrow when the call ends,
        // so push a call scope that releases these at call end instead of
        // holding the borrow to end-of-block.
        let call_scoped = !rust_sim;
        if language_call_borrows || call_scoped {
            self.push_scope();
        }

        for (index, (arg, param)) in args.iter().zip(params.iter()).enumerate() {
            if self.dest_is_mut(param) {
                self.check_mut_argument_writable(&arg.expr, index);
            }
            self.check_arg_use(arg, param);
            let dest_copy = self.types.is_copy_view(param.ty);
            let dest_own = self.types.is_own_view(param.ty);
            if self.arg_is_mut_view(&arg.expr) && dest_own {
                self.error(
                    SemanticErrorKind::ModeMismatch,
                    ISSUE_MUT_INTO_OWN,
                    arg.expr.span,
                );
            }
            if dest_copy {
                self.bind_copy_from(&arg.expr);
                continue;
            }
            if own_arg::is_own_marked(arg) {
                self.own_argument(arg);
                continue;
            }
            if dest_own && self.arg_is_exclusive(&arg.expr) {
                self.move_from_expr(&arg.expr);
                continue;
            }
            if language_call_borrows {
                if self.dest_is_mut(param) {
                    self.borrow_arg_storage(&arg.expr, BorrowKind::Mutable);
                } else if self.dest_is_ref(param) {
                    self.borrow_arg_storage(&arg.expr, BorrowKind::Shared);
                } else {
                    self.check_expr(&arg.expr);
                }
                continue;
            }
            match param.mode {
                SemanticParamMode::Ref => {
                    if self.rust_lifetime {
                        self.borrow_from_expr(&arg.expr, BorrowKind::Shared, None);
                    } else {
                        // Mode-only: treat as a read of the root value.
                        if let Some(def_id) = Self::root_def_id(&arg.expr) {
                            self.check_projection_inputs(&arg.expr);
                            self.read_use(def_id, arg.expr.span);
                        } else {
                            self.check_expr(&arg.expr);
                        }
                    }
                }
                SemanticParamMode::MutRef => {
                    if self.rust_lifetime {
                        self.borrow_from_expr(&arg.expr, BorrowKind::Mutable, None);
                    } else if let Some(def_id) = Self::root_def_id(&arg.expr) {
                        self.check_projection_inputs(&arg.expr);
                        // Mutate demand without registering a Rust borrow scope.
                        self.read_use(def_id, arg.expr.span);
                    } else {
                        self.check_expr(&arg.expr);
                    }
                }
                SemanticParamMode::Move => {
                    // Language: only exclusive values move. Rust lifetime still
                    // consumes every Move-mode argument (existing simulation).
                    if self.rust_lifetime || self.arg_is_exclusive(&arg.expr) {
                        self.move_from_expr(&arg.expr);
                    } else {
                        self.check_expr(&arg.expr);
                    }
                }
                SemanticParamMode::Owned if dest_own => {
                    if self.arg_is_exclusive(&arg.expr) {
                        self.move_from_expr(&arg.expr);
                    } else {
                        self.check_expr(&arg.expr);
                    }
                }
                SemanticParamMode::Owned => self.check_expr(&arg.expr),
            }
        }
        if language_call_borrows || call_scoped {
            self.pop_scope();
        }
    }

    fn check_tensor_view_arg_aliasing(&mut self, args: &[HirCallArg], params: &[ParamType]) {
        for (index, arg) in args.iter().enumerate() {
            if !self.stores_tensor_view(&arg.expr) {
                continue;
            }
            if params
                .get(index)
                .is_some_and(|param| self.dest_is_mut_or_own(param))
            {
                self.error(
                    SemanticErrorKind::ModeMismatch,
                    ISSUE_TENSOR_VIEW_READ_ONLY,
                    arg.expr.span,
                );
            }
            let roots = self.tensor_view_origins(&arg.expr);
            if roots.is_empty() {
                continue;
            }
            let view_storage = self.storage_of_roots(&roots);
            for (other_index, other) in args.iter().enumerate() {
                if other_index == index
                    || !params
                        .get(other_index)
                        .is_some_and(|param| self.dest_is_mut_or_own(param))
                {
                    continue;
                }
                let mut other_storage =
                    self.storage_of_roots(&self.tensor_view_origins(&other.expr));
                if let Some(other_root) = Self::root_def_id(&other.expr) {
                    other_storage.extend(self.storage_names(other_root));
                }
                if view_storage.is_disjoint(&other_storage) {
                    continue;
                }
                self.error(
                    SemanticErrorKind::ModeMismatch,
                    ISSUE_TENSOR_VIEW_READ_ONLY,
                    arg.expr.span,
                );
            }
        }
    }

    /// Kernel argument exclusivity (ruling 7b25889f R6 K1).
    ///
    /// WHY: a kernel's arguments are device buffers that are live
    /// simultaneously for the whole launch, so a writable argument (`mut`,
    /// `own`) must not overlap any other argument of the same call: not the
    /// whole tensor spelled twice, not a `←` alias of it, not a view of the
    /// same storage. Where overlap cannot be proven absent the call is
    /// refused at the call site. Read-only duplicates and read-only
    /// overlapping views stay legal — nothing writes them, so a launch may
    /// share that storage. This rule is the soundness argument every GPU
    /// emitter cites for placing `noalias` on kernel parameter bindings; the
    /// launch-time buffer check is the second, runtime half of the contract.
    ///
    /// EDGE: a view argument is skipped here in both directions — a view in
    /// a writable position and a view overlapping a writable argument are
    /// already named by `tensor_view_read_only`, so the pair is reported
    /// once, by the rule that owns it. A scalar or temporary argument
    /// carries no storage and never overlaps. When both arguments of an
    /// overlapping pair are writable, the pair is reported once, at the
    /// earlier position.
    fn check_kernel_argument_exclusivity(&mut self, params: &[ParamType], args: &[HirCallArg]) {
        let storages: Vec<FxHashSet<DefId>> = args
            .iter()
            .map(|arg| self.kernel_arg_storage(&arg.expr))
            .collect();
        for (index, arg) in args.iter().enumerate() {
            if !params
                .get(index)
                .is_some_and(|param| self.dest_is_mut_or_own(param))
                || self.stores_tensor_view(&arg.expr)
            {
                continue;
            }
            for other in 0..args.len() {
                if other == index || self.stores_tensor_view(&args[other].expr) {
                    continue;
                }
                if storages[index].is_disjoint(&storages[other]) {
                    continue;
                }
                if params
                    .get(other)
                    .is_some_and(|param| self.dest_is_mut_or_own(param))
                    && other < index
                {
                    continue;
                }
                self.kernel_argument_overlap_error(&arg.expr, index, other);
                break;
            }
        }
    }

    /// Storage one kernel argument names: the places its root binding
    /// aliases plus the backing roots of a view chain. A scalar or a
    /// temporary value carries no storage, so it never overlaps.
    fn kernel_arg_storage(&self, expr: &HirExpression) -> FxHashSet<DefId> {
        if !self.has_tensor_value_type(expr) {
            return FxHashSet::default();
        }
        let mut storage = self.storage_of_roots(&self.tensor_view_origins(expr));
        if let Some(root) = Self::root_def_id(expr) {
            storage.extend(self.storage_names(root));
        }
        storage
    }

    fn kernel_argument_overlap_error(
        &mut self,
        arg: &HirExpression,
        position: usize,
        overlap: usize,
    ) {
        let mut error = SemanticError::structured(SemanticErrorKind::ModeMismatch, arg.span)
            .with_arg("issue", ISSUE_KERNEL_ARGUMENT_OVERLAP)
            .with_arg("position", (position + 1).to_string())
            .with_arg("overlap", (overlap + 1).to_string());
        if let Some(spelling) = Self::root_def_id(arg).and_then(|root| self.def_spelling(root)) {
            error = error.with_arg("binding", spelling);
        }
        self.errors.push(error);
    }

    fn check_arg_use(&mut self, arg: &HirCallArg, param: &ParamType) {
        if let Some(arg_def_id) = Self::root_def_id(&arg.expr)
            && let Some(arg_usage) = self.param_usage.get_mut(&arg_def_id)
        {
            if matches!(
                param.mode,
                SemanticParamMode::MutRef | SemanticParamMode::Move
            ) {
                arg_usage.passed_in_or_ex = true;
            }
            if matches!(param.mode, SemanticParamMode::Move) {
                arg_usage.passed_ex = true;
            }
            if matches!(arg_usage.mode, HirParamMode::De)
                && matches!(
                    param.mode,
                    SemanticParamMode::MutRef | SemanticParamMode::Move
                )
            {
                self.error(
                    SemanticErrorKind::ModeMismatch,
                    ISSUE_DE_TO_IN_OR_EX_POSITION,
                    arg.expr.span,
                );
            }
        }
        if let Some(arg_def_id) = Self::root_def_id(&arg.expr)
            && self.de_aliases.contains(&arg_def_id)
            && matches!(
                param.mode,
                SemanticParamMode::MutRef | SemanticParamMode::Move
            )
        {
            self.error(
                SemanticErrorKind::ModeMismatch,
                ISSUE_DE_TO_IN_OR_EX_POSITION,
                arg.expr.span,
            );
        }
        if let Some(origin) = Self::root_def_id(&arg.expr)
            .and_then(|arg_def_id| self.nonmut_tensor_origin(arg_def_id))
            && self.dest_is_mut(param)
        {
            self.nonmut_tensor_write_error(origin, arg.expr.span);
        }
        if let Some(origin) = Self::root_def_id(&arg.expr).and_then(|d| self.nonmut_agg_origin(d))
            && self.dest_is_mut(param)
        {
            self.nonmut_agg_write_error(origin, arg.expr.span);
        }
        if self.arg_is_ref_view(&arg.expr) && self.dest_is_mut_or_own(param) {
            self.error(
                SemanticErrorKind::ModeMismatch,
                ISSUE_REF_TO_MUT_OR_OWN_POSITION,
                arg.expr.span,
            );
        }
    }

    fn check_lvalue(&mut self, target: &HirExpression) {
        if !self.tensor_view_origins(target).is_empty() {
            self.error(
                SemanticErrorKind::ModeMismatch,
                ISSUE_TENSOR_VIEW_READ_ONLY,
                target.span,
            );
        }
        self.check_lvalue_access(target);
    }

    /// Access rules for writing `target`, shared by assignment and `⇇`.
    fn check_lvalue_access(&mut self, target: &HirExpression) {
        // One fault, one diagnostic: a write through an explicit `ref` view (a
        // `ref` binding or `ref` parameter) is `assign_through_ref`; a write
        // through a bare parameter or an alias of a `ref`/bare parameter is
        // `assign_to_de_parameter`. Re-pointing the name itself is legal. A
        // name that already took one of these read-only-through diagnostics
        // does not also take the non-`mut` alias row — the alias error is for
        // names whose own cell is writable (`mut alias of a bare parameter`),
        // not for a `ref` cursor that is read-only through on its own account.
        let mut refused_through_ref = false;
        if Self::is_projection(target)
            && let Some(def_id) = Self::root_def_id(target)
        {
            if self.ref_roots.contains(&def_id) {
                self.error(
                    SemanticErrorKind::AssignToImmutableBorrow,
                    ISSUE_ASSIGN_THROUGH_REF,
                    target.span,
                );
                refused_through_ref = true;
            } else if self.is_de_param(def_id) || self.de_aliases.contains(&def_id) {
                self.error(
                    SemanticErrorKind::AssignToImmutableBorrow,
                    ISSUE_ASSIGN_TO_DE_PARAMETER,
                    target.span,
                );
                refused_through_ref = true;
            }
        }
        if let Some(def_id) = Self::root_def_id(target) {
            if Self::is_projection(target)
                && let Some(origin) = self.nonmut_tensor_origin(def_id)
            {
                self.nonmut_tensor_write_error(origin, target.span);
            }
            if !refused_through_ref
                && Self::is_projection(target)
                && let Some(origin) = self.nonmut_agg_alias_origin(def_id)
            {
                self.nonmut_agg_write_error(origin, target.span);
            }
            self.check_projection_inputs(target);
            self.write_use(def_id, target.span);
        } else {
            self.check_expr(target);
        }
    }

    /// Statement-form `fill` (result dropped) is the in-place tensor store; the
    /// registry row is `TensorFill`, whose bound form returns a fresh tensor and
    /// so is not marked receiver-mutating. The statement posture is the write:
    /// it is rejected on a read-only parameter and counts as a write to a
    /// writable root (so a `mut` parameter it fills is not reported unused).
    fn check_statement_fill(&mut self, expr: &HirExpression) {
        let HirExpressionKind::MethodCall(receiver, method, _, args, _) = &expr.kind else {
            return;
        };
        let is_fill = receiver
            .ty
            .zip(self.interner.strings().get(method.0 as usize))
            .and_then(|(ty, name)| lookup_method_for_type_id(ty, self.types, name, args.len()))
            .is_some_and(|entry| {
                matches!(
                    entry.mir_kind,
                    radix_intrinsics::IntrinsicMirKind::Collection(
                        radix_types::MirCollectionOp::TensorFill
                    )
                )
            });
        if !is_fill {
            return;
        }
        let Some(root) = Self::root_def_id(receiver) else {
            return;
        };
        if let Some(origin) = self.nonmut_tensor_origin(root) {
            self.nonmut_tensor_write_error(origin, expr.span);
            return;
        }
        if let Some(usage) = self.param_usage.get_mut(&root) {
            usage.mutated = true;
        }
    }

    /// An argument bound to a `mut` parameter must be a writable place: a
    /// `var` binding, a `mut` parameter of the caller, or a field/element path
    /// rooted at one. A `const` binding, an unmarked (read-only) parameter and
    /// a temporary are rejected. A read-only tensor or aggregate parameter (and
    /// its `←` aliases) keeps the alias rules' own diagnostics (SEM056/SEM020)
    /// and is not reported twice.
    fn check_mut_argument_writable(&mut self, arg: &HirExpression, position: usize) {
        let origin = if let Some(root) = Self::root_def_id(arg) {
            let covered =
                self.nonmut_tensor_origin(root).is_some() || self.nonmut_agg_origin(root).is_some();
            if covered
                || !(self.immutable_locals.contains(&root) || self.unmarked_params.contains(&root))
            {
                return;
            }
            Some(root)
        } else if !matches!(
            arg.kind,
            HirExpressionKind::Literal(_)
                | HirExpressionKind::Array(_)
                | HirExpressionKind::Struct(..)
                | HirExpressionKind::Verte { .. }
                | HirExpressionKind::Tuple(..)
                | HirExpressionKind::Binary(..)
                | HirExpressionKind::Unary(..)
                | HirExpressionKind::Call(..)
                | HirExpressionKind::MethodCall(..)
        ) {
            return;
        } else {
            None
        };
        let mut error = SemanticError::structured(SemanticErrorKind::ImmutableAssignment, arg.span)
            .with_arg("issue", ISSUE_MUT_ARGUMENT_NOT_WRITABLE)
            .with_arg("position", (position + 1).to_string());
        if let Some(spelling) = origin.and_then(|root| self.def_spelling(root)) {
            error = error.with_arg("binding", spelling);
        }
        self.errors.push(error);
    }

    /// The non-`mut` tensor parameter a binding is or aliases, if any.
    fn nonmut_tensor_origin(&self, def_id: DefId) -> Option<DefId> {
        if self.nonmut_tensor_params.contains(&def_id) {
            return Some(def_id);
        }
        self.nonmut_tensor_aliases.get(&def_id).copied()
    }

    /// The non-`mut` aggregate parameter a binding is or aliases, if any.
    fn nonmut_agg_origin(&self, def_id: DefId) -> Option<DefId> {
        if self.nonmut_agg_params.contains(&def_id) {
            return Some(def_id);
        }
        self.nonmut_agg_aliases.get(&def_id).copied()
    }

    /// Like `nonmut_agg_origin`, but only for aliases: a direct write through
    /// the parameter itself is already rejected by typecheck.
    fn nonmut_agg_alias_origin(&self, def_id: DefId) -> Option<DefId> {
        if self.const_agg_aliases.contains(&def_id) {
            return None;
        }
        self.nonmut_agg_aliases.get(&def_id).copied()
    }

    /// Source spelling of a binding, for a diagnostic that names it.
    fn def_spelling(&self, def_id: DefId) -> Option<String> {
        self.names
            .get(&def_id)
            .and_then(|name| self.interner.strings().get(name.0 as usize))
            .map(ToString::to_string)
    }

    /// A write that reaches a non-`mut` aggregate parameter through an alias:
    /// the same SEM020 a direct write gets.
    fn nonmut_agg_write_error(&mut self, _origin: DefId, span: radix_lexer::Span) {
        self.error(
            SemanticErrorKind::ImmutableAssignment,
            ISSUE_IMMUTABLE_SECOND_ASSIGNMENT,
            span,
        );
    }

    /// An in-place method call that reaches a non-`mut` aggregate parameter
    /// through an alias.
    fn nonmut_agg_method_error(
        &mut self,
        origin: DefId,
        method: radix_lexer::Symbol,
        span: radix_lexer::Span,
    ) {
        let mut error = SemanticError::structured(SemanticErrorKind::ImmutableAssignment, span)
            .with_arg("issue", ISSUE_MUTATING_METHOD_ON_IMMUTABLE)
            .with_arg("binding", self.def_spelling(origin).unwrap_or_default());
        if let Some(name) = self.interner.strings().get(method.0 as usize) {
            error = error.with_arg("method", name.clone());
        }
        self.errors.push(error);
    }

    /// A write through a tensor parameter that is not marked `mut`: the
    /// diagnostic names the parameter.
    fn nonmut_tensor_write_error(&mut self, def_id: DefId, span: radix_lexer::Span) {
        let mut error = SemanticError::structured(SemanticErrorKind::AssignToImmutableBorrow, span)
            .with_arg("issue", ISSUE_ASSIGN_TO_NONMUT_TENSOR_PARAM);
        if let Some(spelling) = self.def_spelling(def_id) {
            error = error.with_arg("name", spelling);
        }
        self.errors.push(error);
    }

    /// The binding and field path of a named storage place (`out`, `self.buf`).
    fn place_key(expr: &HirExpression) -> Option<PlaceKey> {
        match &expr.kind {
            HirExpressionKind::Path(def_id) => Some((*def_id, Vec::new())),
            HirExpressionKind::Field(object, name) => {
                let (root, mut path) = Self::place_key(object)?;
                path.push(*name);
                Some((root, path))
            }
            _ => None,
        }
    }

    fn is_tensor_typed(&self, expr: &HirExpression) -> bool {
        expr.ty.is_some_and(|ty| {
            let (_, inner) = self.types.peel_access_marker(ty);
            matches!(self.types.get(inner), Type::Tensor(_, _))
        })
    }

    /// Record that a binding initialized from a place of an aliasing type
    /// shares that place's storage. `c ← a` makes `c` another name for `a`'s
    /// value, so `c` and `a` overlap for the call-site borrow rule and the
    /// `⇇` alias rule. Exclusive (`own`/`copy`) bindings duplicate and do not
    /// alias, and value types copy.
    ///
    /// The table is flow-aware in two steps. A must-rebind (`Rebind::Must`)
    /// replaces `local`'s entry and detaches `local` from every entry that
    /// named it, since those entries keep the storage `local` used to name.
    /// A may-rebind (`Rebind::May`: inside a branch, loop, closure or
    /// short-circuit, or of a `mut` parameter) keeps the old entry and adds
    /// the new roots, so both possible storages stay overlapping names.
    fn note_bind_alias(
        &mut self,
        local: DefId,
        ty: Option<TypeId>,
        init: &HirExpression,
        rebind: Rebind,
    ) {
        let must = match rebind {
            Rebind::Fresh => true,
            Rebind::Must => !self.mut_params.contains(&local),
            Rebind::May => false,
        };
        if must {
            self.bind_alias_roots.remove(&local);
            self.element_alias_roots.remove(&local);
            if rebind == Rebind::Must {
                self.detach_storage_name(local);
            }
        }
        let elements = self.element_sources(init);
        if !elements.is_empty() {
            self.element_alias_roots
                .entry(local)
                .or_default()
                .extend(elements);
        }
        self.nonmut_tensor_aliases.remove(&local);
        self.nonmut_agg_aliases.remove(&local);
        self.const_agg_aliases.remove(&local);
        let Some(ty) = ty.or(init.ty) else { return };
        if self.types.is_exclusive_value(ty) || !self.is_aliasing_type(ty) {
            return;
        }
        let place = Self::place_key(init).map(|(source, _)| source);
        let sources = match place {
            Some(source) => vec![source],
            None => self.call_result_sources(init),
        };
        if sources.is_empty() {
            return;
        }
        let mut roots = FxHashSet::default();
        for source in sources {
            if let Some(named) = self.bind_alias_roots.get(&source) {
                roots.extend(named.iter().copied());
            }
            roots.insert(source);
        }
        if let Some(prior) = self.bind_alias_roots.get(&local) {
            roots.extend(prior.iter().copied());
        }
        self.bind_alias_roots.insert(local, roots);
        let Some(source) = place else {
            return;
        };
        if self.is_tensor_typed(init)
            && let Some(origin) = self.nonmut_tensor_origin(source)
            && origin != local
        {
            self.nonmut_tensor_aliases.insert(local, origin);
        } else if !self.is_tensor_typed(init)
            && let Some(origin) = self.nonmut_agg_origin(source)
            && origin != local
        {
            self.nonmut_agg_aliases.insert(local, origin);
        }
    }

    /// An element binder of `for from xs` names an element of `xs`, so a
    /// read-only collection (a `const` local, a non-`mut` aggregate parameter,
    /// or an alias of one) makes the binder a read-only alias of that origin:
    /// const protects all the way down. A bare rebind of the binder is not a
    /// projection and stays legal.
    fn note_itera_binder_alias(
        &mut self,
        binding: DefId,
        mode: HirIteraMode,
        iter: &HirExpression,
        mutable: bool,
    ) {
        if !self.itera_element_can_alias(mode, iter) {
            return;
        }
        let Some(root) = Self::root_def_id(iter) else {
            return;
        };
        // A `mut` (`varia`) binder owns a mutable copy of the element, so it is
        // not a shared alias of a non-`mut` parameter and may be written
        // through. `const` is still deep: a binder over a read-only collection
        // stays a read-only alias whatever its spelling.
        if mutable && !self.immutable_locals.contains(&root) {
            return;
        }
        let origin = match self.nonmut_agg_origin(root) {
            Some(origin) => origin,
            None if self.immutable_locals.contains(&root) => root,
            None => return,
        };
        if origin != binding {
            self.nonmut_agg_aliases.insert(binding, origin);
        }
    }

    /// Whether the binder of an `itera` over `iter` can hold shared storage.
    /// `de` binds an index or key. `ex` over a list, set or map binds an
    /// element or value, which must itself alias; any other iterable keeps the
    /// alias, since a scalar binder has no projection to write through.
    fn itera_element_can_alias(&self, mode: HirIteraMode, iter: &HirExpression) -> bool {
        if !matches!(mode, HirIteraMode::Ex) {
            return false;
        }
        let Some(ty) = iter.ty else { return true };
        let (_, inner) = self.types.peel_access_marker(ty);
        match self.types.get(inner) {
            Type::Array(element) | Type::Set(element) | Type::Map(_, element) => {
                self.is_aliasing_type(*element)
            }
            _ => true,
        }
    }

    /// Whether a value of type `ty` shares storage when it is bound or listed
    /// (reference model); exclusive values and scalars copy.
    fn shares_storage(&self, ty: TypeId) -> bool {
        !self.types.is_exclusive_value(ty) && self.is_aliasing_type(ty)
    }

    /// The bindings whose storage the elements of a list or tuple literal
    /// share: each listed place of an aliasing type (with the bindings it
    /// already aliases), each listed call result's sources, and the elements
    /// of nested literals and spread places. Anything else is empty.
    fn element_sources(&self, init: &HirExpression) -> Vec<DefId> {
        let mut out = Vec::new();
        match &init.kind {
            HirExpressionKind::Array(elements) => {
                for element in elements {
                    match element {
                        radix_hir::HirArrayElement::Expr(expr) => {
                            self.collect_element_sources(expr, &mut out);
                        }
                        radix_hir::HirArrayElement::Spread(expr) => {
                            if let Some((root, _)) = Self::place_key(expr) {
                                for name in self.storage_names(root) {
                                    if let Some(roots) = self.element_alias_roots.get(&name) {
                                        out.extend(roots.iter().copied());
                                    }
                                }
                            }
                        }
                        radix_hir::HirArrayElement::Infer(_) => {}
                    }
                }
            }
            HirExpressionKind::Tuple(elements, _) => {
                for expr in elements {
                    self.collect_element_sources(expr, &mut out);
                }
            }
            _ => {}
        }
        out
    }

    fn collect_element_sources(&self, expr: &HirExpression, out: &mut Vec<DefId>) {
        if matches!(
            expr.kind,
            HirExpressionKind::Array(_) | HirExpressionKind::Tuple(..)
        ) {
            out.extend(self.element_sources(expr));
            return;
        }
        if !expr.ty.is_some_and(|ty| self.shares_storage(ty)) {
            return;
        }
        match Self::place_key(expr) {
            Some((root, _)) => out.extend(self.storage_names(root)),
            None => out.extend(self.call_result_sources(expr)),
        }
    }

    /// The bindings an element read `xs[i]` (or a tuple position) can share
    /// storage with: the element aliases recorded for any name of the
    /// container's storage.
    fn element_storage_names(&self, root: DefId) -> FxHashSet<DefId> {
        let mut names = FxHashSet::default();
        for holder in self.storage_names(root) {
            if let Some(roots) = self.element_alias_roots.get(&holder) {
                for element in roots {
                    names.extend(self.storage_names(*element));
                }
            }
        }
        names
    }

    /// An element read of a container whose elements alias a mutably borrowed
    /// place is a use while mutably borrowed, like a read of the place itself.
    fn read_element_storage(&mut self, expr: &HirExpression) {
        let Some(root) = Self::root_def_id(expr) else {
            return;
        };
        let borrowed = self
            .element_storage_names(root)
            .into_iter()
            .find(|name| self.states.get(name).is_some_and(|state| state.mutable));
        if let Some(name) = borrowed {
            self.conflict(ISSUE_USE_WHILE_MUTABLY_BORROWED, name, expr.span);
        }
    }

    /// Whether `expr` reads an element (index or field projection) rather
    /// than naming a binding itself.
    fn is_element_read(expr: &HirExpression) -> bool {
        match &expr.kind {
            HirExpressionKind::Field(..) | HirExpressionKind::Index(..) => true,
            HirExpressionKind::Deref(inner) | HirExpressionKind::NonNull(inner, _) => {
                Self::is_element_read(inner)
            }
            _ => false,
        }
    }

    /// The bindings a call result of an aliasing type can share storage with.
    ///
    /// Conservative rule: an argument may flow into the result when its
    /// parameter has the result's aliasing type and takes the argument by
    /// sharing. A `copy`/`own` parameter duplicates or consumes the argument,
    /// and a moved one leaves the name dead, so none of those alias. A call
    /// argument that is itself a call contributes its own sources.
    fn call_result_sources(&self, call: &HirExpression) -> Vec<DefId> {
        let Some(result) = call.ty else {
            return Vec::new();
        };
        if self.types.is_exclusive_value(result) || !self.is_aliasing_type(result) {
            return Vec::new();
        }
        let (params, args) = match &call.kind {
            HirExpressionKind::Call(callee, _, args, _) => {
                (self.call_params(callee).map(<[ParamType]>::to_vec), args)
            }
            HirExpressionKind::MethodCall(receiver, method, _, args, _) => {
                (self.user_method_params(receiver, *method), args)
            }
            _ => return Vec::new(),
        };
        let Some(params) = params else {
            return Vec::new();
        };
        let carrier = self.alias_carrier(result);
        let mut sources = Vec::new();
        for (arg, param) in args.iter().zip(&params) {
            if arg.spread {
                break;
            }
            if matches!(param.mode, SemanticParamMode::Move)
                || self.types.is_exclusive_value(param.ty)
                || !self.types.equals(self.alias_carrier(param.ty), carrier)
            {
                continue;
            }
            match Self::place_key(&arg.expr) {
                Some((root, _)) => sources.push(root),
                None => sources.extend(self.call_result_sources(&arg.expr)),
            }
        }
        sources
    }

    /// The storage-carrying type under access markers, aliases and `∪ nihil`.
    fn alias_carrier(&self, ty: TypeId) -> TypeId {
        let (_, mut inner) = self.types.peel_access_marker(ty);
        while let Type::Option(next) = self.types.get(inner) {
            inner = self.types.peel_access_marker(*next).1;
        }
        inner
    }

    /// `name` was rebound to a new place: the entries that named `name` keep
    /// naming its old storage, so one of them takes `name`'s place in the
    /// others and `name` leaves every entry.
    fn detach_storage_name(&mut self, name: DefId) {
        let mut holders: Vec<DefId> = self
            .bind_alias_roots
            .iter()
            .filter(|(holder, roots)| **holder != name && roots.contains(&name))
            .map(|(holder, _)| *holder)
            .collect();
        let rep = holders.iter().min_by_key(|holder| holder.0).copied();
        for roots in self.element_alias_roots.values_mut() {
            if roots.remove(&name)
                && let Some(rep) = rep
            {
                roots.insert(rep);
            }
        }
        let Some(rep) = rep else {
            return;
        };
        holders.retain(|holder| *holder != rep);
        for holder in holders {
            if let Some(roots) = self.bind_alias_roots.get_mut(&holder) {
                roots.remove(&name);
                roots.insert(rep);
            }
        }
        if let Some(roots) = self.bind_alias_roots.get_mut(&rep) {
            roots.remove(&name);
        }
    }

    /// The bindings whose storage a binding names: itself plus every binding
    /// that names the same storage. `bind_alias_roots` rows point from a
    /// binding to the places it was bound from, so the reverse edges are
    /// walked too: `x ← b` and `b ← a` put `x`, `b` and `a` in one set no
    /// matter which of them a conflict check starts from.
    fn storage_names(&self, def_id: DefId) -> FxHashSet<DefId> {
        let mut names = FxHashSet::default();
        let mut pending = vec![def_id];
        names.insert(def_id);
        while let Some(name) = pending.pop() {
            if let Some(roots) = self.bind_alias_roots.get(&name) {
                for root in roots {
                    if names.insert(*root) {
                        pending.push(*root);
                    }
                }
            }
            for (holder, roots) in &self.bind_alias_roots {
                if roots.contains(&name) && names.insert(*holder) {
                    pending.push(*holder);
                }
            }
        }
        names
    }

    /// The place whose live borrow conflicts with a use, write or new borrow
    /// of `def_id` by `acting`: another name of the same storage that is
    /// flagged borrowed or carries a live region borrow. `mutable_only`
    /// selects the read rule (only a flagged mutable borrow blocks a read).
    /// Region borrows conflict only when they are mutable: a shared view or
    /// `ref` borrow of an alias is the memory-independence pass's question,
    /// and the one-directional alias check this detection mirrors only ever
    /// treated a mutable borrow as a conflict. A region borrow held by
    /// `acting` itself is that binding's own borrow and does not conflict
    /// with it.
    fn conflicting_borrow_on_storage(
        &self,
        def_id: DefId,
        acting: DefId,
        mutable_only: bool,
    ) -> Option<DefId> {
        for name in self.storage_names(def_id) {
            let Some(state) = self.states.get(&name) else {
                continue;
            };
            if state.mutable {
                return Some(name);
            }
            if !mutable_only && state.shared > 0 {
                return Some(name);
            }
            for tracked in &state.region_borrows {
                if tracked.borrower == acting {
                    continue;
                }
                if !self.tracked_region_live(tracked.source, tracked.borrower) {
                    continue;
                }
                if !matches!(tracked.kind, BorrowKind::Mutable) {
                    continue;
                }
                return Some(tracked.source);
            }
        }
        None
    }

    /// The storage names of a set of view-origin roots: every root plus the
    /// bindings aliasing it.
    fn storage_of_roots(&self, roots: &FxHashSet<DefId>) -> FxHashSet<DefId> {
        roots
            .iter()
            .flat_map(|root| self.storage_names(*root))
            .collect()
    }

    /// Call-site borrow of an argument place. Overlap is judged on storage,
    /// not on the argument's own binding: the borrow is registered against
    /// every name of the argument's storage (itself first), so a `mut`
    /// argument conflicts with another argument naming the same tensor
    /// through `←` aliases exactly as it does for the direct spelling.
    fn borrow_arg_storage(&mut self, expr: &HirExpression, kind: BorrowKind) {
        let Some(root) = Self::root_def_id(expr) else {
            self.check_expr(expr);
            return;
        };
        self.check_projection_inputs(expr);
        let mut storage = self.storage_names(root);
        if Self::is_element_read(expr) {
            storage.extend(self.element_storage_names(root));
        }
        let mut names = vec![root];
        names.extend(storage.into_iter().filter(|n| *n != root));
        for name in names {
            let before = self.errors.len();
            match kind {
                BorrowKind::Shared => self.borrow_shared_with(name, None, expr.span),
                BorrowKind::Mutable => self.borrow_mut_with(name, None, expr.span),
            }
            if self.errors.len() != before {
                break;
            }
        }
    }

    /// The places a `⇇` value reads through: the value itself when it is a
    /// named place, the receiver place of a view chain, or the view origins.
    fn copy_into_value_places(&self, value: &HirExpression, out: &mut Vec<PlaceKey>) {
        if let HirExpressionKind::Path(def_id) = &value.kind
            && let Some(roots) = self.tensor_view_roots.get(def_id)
        {
            out.extend(roots.iter().map(|root| (*root, Vec::new())));
            return;
        }
        if let Some(key) = Self::place_key(value) {
            out.push(key);
            return;
        }
        if let HirExpressionKind::MethodCall(receiver, method, _, args, _) = &value.kind
            && self
                .tensor_view_method(receiver, *method, args.len())
                .is_some_and(Self::is_view_method)
        {
            self.copy_into_value_places(receiver, out);
            return;
        }
        out.extend(
            self.tensor_view_origins(value)
                .into_iter()
                .map(|root| (root, Vec::new())),
        );
    }

    /// Whether writing `target` can change what `value` reads. Two places on
    /// one binding overlap when one field path prefixes the other; places on
    /// different bindings overlap when `←` aliases join their storage.
    fn copy_into_overlaps(&self, target: &PlaceKey, value: &HirExpression) -> bool {
        let mut places = Vec::new();
        self.copy_into_value_places(value, &mut places);
        let target_names = self.storage_names(target.0);
        places.iter().any(|(root, path)| {
            if *root == target.0 {
                let shared = path.len().min(target.1.len());
                return path[..shared] == target.1[..shared];
            }
            !self.storage_names(*root).is_disjoint(&target_names)
        })
    }

    /// `target ⇇ value`: copy the values of `value` into the existing storage
    /// of `target`.
    ///
    /// R2: the target is a named storage place (a binding or a field path
    /// under one), never a view. R3: the value does not read the storage the
    /// target names. The target is written, so the ordinary write rules (a
    /// `de` parameter, a `ref` root, an outstanding borrow) apply to it.
    fn check_copy_into(&mut self, target: &HirExpression, value: &HirExpression) {
        let key = Self::place_key(target);
        if !self.tensor_view_origins(target).is_empty() || self.is_tensor_view(target) {
            self.error(
                SemanticErrorKind::ModeMismatch,
                ISSUE_COPY_INTO_TARGET_VIEW,
                target.span,
            );
        } else if key.is_none() {
            self.error(
                SemanticErrorKind::ModeMismatch,
                ISSUE_COPY_INTO_TARGET_NOT_STORAGE,
                target.span,
            );
        }
        if let Some(key) = &key
            && self.copy_into_overlaps(key, value)
        {
            self.error(
                SemanticErrorKind::ModeMismatch,
                ISSUE_COPY_INTO_ALIAS,
                value.span,
            );
        }
        self.visit_expr(value);
        let Some((root, _)) = key else {
            self.visit_expr(target);
            return;
        };
        if self.is_de_param(root) || self.de_aliases.contains(&root) {
            self.error(
                SemanticErrorKind::AssignToImmutableBorrow,
                ISSUE_ASSIGN_TO_DE_PARAMETER,
                target.span,
            );
        }
        if self.ref_roots.contains(&root) && !self.param_usage.contains_key(&root) {
            self.error(
                SemanticErrorKind::AssignToImmutableBorrow,
                ISSUE_ASSIGN_THROUGH_REF,
                target.span,
            );
        }
        if let Some(origin) = self.nonmut_tensor_origin(root) {
            self.nonmut_tensor_write_error(origin, target.span);
        }
        if let Some(origin) = self.nonmut_agg_alias_origin(root) {
            self.nonmut_agg_write_error(origin, target.span);
        }
        self.check_projection_inputs(target);
        self.write_use(root, target.span);
    }

    /// R6: a plain `←` of a whole tensor into a `mut` tensor parameter inside
    /// a kernel. A kernel parameter is a device buffer, so `⇇` is the one way
    /// to write it; on the host the same `←` is the ordinary rebind.
    fn check_mut_tensor_param_rebind(&mut self, target: &HirExpression) {
        if self.in_kernel
            && let HirExpressionKind::Path(def_id) = &target.kind
            && self.mut_params.contains(def_id)
            && self.is_tensor_typed(target)
        {
            self.error(
                SemanticErrorKind::ModeMismatch,
                ISSUE_MUT_TENSOR_PARAM_REBIND,
                target.span,
            );
        }
    }

    fn borrow_from_expr(
        &mut self,
        expr: &HirExpression,
        kind: BorrowKind,
        borrower: Option<DefId>,
    ) {
        if let Some(def_id) = Self::root_def_id(expr) {
            self.check_projection_inputs(expr);
            match kind {
                BorrowKind::Shared => self.borrow_shared_with(def_id, borrower, expr.span),
                BorrowKind::Mutable => self.borrow_mut_with(def_id, borrower, expr.span),
            }
        } else {
            self.check_expr(expr);
        }
    }

    fn move_from_expr(&mut self, expr: &HirExpression) {
        if let Some(def_id) = Self::root_def_id(expr) {
            self.check_projection_inputs(expr);
            self.move_use(def_id, expr.span);
        } else {
            self.check_expr(expr);
        }
    }

    #[allow(clippy::only_used_in_recursion)]
    fn root_def_id(expr: &HirExpression) -> Option<DefId> {
        match &expr.kind {
            HirExpressionKind::Path(def_id) => Some(*def_id),
            HirExpressionKind::Field(object, _) | HirExpressionKind::Index(object, _) => {
                Self::root_def_id(object)
            }
            HirExpressionKind::Deref(inner) | HirExpressionKind::NonNull(inner, _) => {
                Self::root_def_id(inner)
            }
            _ => None,
        }
    }

    fn check_projection_inputs(&mut self, expr: &HirExpression) {
        match &expr.kind {
            HirExpressionKind::Field(object, _) | HirExpressionKind::Deref(object) => {
                self.check_projection_inputs(object);
            }
            HirExpressionKind::Index(object, index) => {
                self.check_projection_inputs(object);
                self.check_expr(index);
            }
            HirExpressionKind::NonNull(object, chain) => {
                self.check_projection_inputs(object);
                match chain {
                    radix_hir::HirNonNullKind::Index(index) => self.check_expr(index),
                    radix_hir::HirNonNullKind::Call(args) => {
                        for arg in args {
                            self.check_expr(&arg.expr);
                        }
                    }
                    radix_hir::HirNonNullKind::Value | radix_hir::HirNonNullKind::Member(_) => {}
                }
            }
            _ => {}
        }
    }

    fn ensure_state(&mut self, def_id: DefId) {
        self.states.entry(def_id).or_default();
    }

    fn note_ref_root(&mut self, def_id: DefId, ty: TypeId) {
        if self.types.is_ref_view(ty) {
            self.ref_roots.insert(def_id);
        }
    }

    fn note_mut_root(&mut self, def_id: DefId, ty: TypeId) {
        if self.types.is_mut_view(ty) {
            self.mut_roots.insert(def_id);
        }
    }

    fn note_copy_root(&mut self, def_id: DefId, ty: TypeId) {
        if self.types.is_copy_view(ty) {
            self.copy_roots.insert(def_id);
        }
    }

    /// Record a bind that aliases a `de` place (D12.1: unmarked binds alias).
    ///
    /// Only reference-model types alias (genus, contract, collections,
    /// tensors, register-class `vector`/`matrix` per al2 / advisor ruling
    /// 2026-09-30 Q1, each also through `T ∪ nihil`); scalars and text copy.
    /// Exclusive (`own`/`copy`) destinations are handled by their own bind
    /// rules.
    fn note_de_alias(&mut self, local: DefId, ty: Option<TypeId>, init: &HirExpression) {
        let Some(ty) = ty else { return };
        if self.types.is_exclusive_value(ty) || !self.is_aliasing_type(ty) {
            return;
        }
        let Some(source) = Self::root_def_id(init) else {
            return;
        };
        let from_de_param = self
            .param_usage
            .get(&source)
            .is_some_and(|usage| matches!(usage.mode, HirParamMode::De));
        if from_de_param || self.de_aliases.contains(&source) {
            self.de_aliases.insert(local);
        }
    }

    /// A scalar place stores a copy of the assigned value, so assigning into
    /// it from another place retains no borrow of the source (`a ← b` on two
    /// `mut int` parameters reads `b`; it does not borrow it).
    fn is_scalar_place(&self, target: &HirExpression) -> bool {
        target.ty.is_some_and(|ty| {
            let (_, inner) = self.types.peel_access_marker(ty);
            self.types.is_scalar_for_access_marker(inner)
        })
    }

    fn is_aliasing_type(&self, ty: TypeId) -> bool {
        let (_, inner) = self.types.peel_access_marker(ty);
        if self.types.is_aliasing_container_through_option(inner) {
            return true;
        }
        match self.types.get(inner) {
            Type::Struct(_) | Type::Interface(_) => true,
            Type::Option(inner) | Type::Alias(_, inner) => self.is_aliasing_type(*inner),
            _ => false,
        }
    }

    fn note_own_root(&mut self, def_id: DefId, ty: TypeId) {
        if self.types.is_exclusive_value(ty) {
            self.own_roots.insert(def_id);
        }
    }

    /// Explicit `&`/`&mut` on unmarked values stay Rust-lifetime only.
    /// `mut` roots are language (MU-4). Combined analysis tracks both.
    fn tracks_explicit_ref(&self, def_id: DefId) -> bool {
        self.rust_lifetime || self.mut_roots.contains(&def_id)
    }

    /// Exclusive-borrow bind of a `mut` local from a path (or projection root).
    ///
    /// `borrower` is the bound local; when present the conflict decision is
    /// region-routed through that local's inferred borrow region.
    fn bind_mut_from(&mut self, rhs: &HirExpression, borrower: Option<DefId>) {
        self.borrow_from_expr(rhs, BorrowKind::Mutable, borrower);
    }

    /// Shared-borrow bind of a `ref` local from a path (or projection root).
    fn bind_ref_from(&mut self, rhs: &HirExpression, borrower: Option<DefId>) {
        self.borrow_from_expr(rhs, BorrowKind::Shared, borrower);
    }

    /// Bind-site duplicate acquisition for a `copy` destination.
    ///
    /// The source is read, never moved. MU-6 materializes the deep duplicate;
    /// this pass records the semantic event by preserving source liveness while
    /// the destination remains an exclusive value.
    fn bind_copy_from(&mut self, rhs: &HirExpression) {
        self.check_expr(rhs);
    }

    /// Bind-site move tracking for an `own` destination.
    ///
    /// Exclusive sources transfer (SEM050 on later use). Unmarked sources are
    /// tentatively accepted as provably-dead; a later use rejects the bind
    /// with a `copy` suggestion. Fresh RHS is a no-move read of the tree.
    fn bind_own_from(&mut self, rhs: &HirExpression) {
        match &rhs.kind {
            HirExpressionKind::Path(_) if self.arg_is_exclusive(rhs) => {
                self.move_from_expr(rhs);
            }
            HirExpressionKind::Path(def_id) => {
                self.check_expr(rhs);
                self.unmarked_own_binds.insert(*def_id, rhs.span);
            }
            _ => self.check_expr(rhs),
        }
    }

    fn arg_is_exclusive(&self, expr: &HirExpression) -> bool {
        if let Some(ty) = expr.ty
            && self.types.is_exclusive_value(ty)
        {
            return true;
        }
        match Self::root_def_id(expr) {
            Some(def_id) => self.own_roots.contains(&def_id),
            None => false,
        }
    }

    fn note_live_unmarked_own_bind(&mut self, def_id: DefId) {
        if let Some(span) = self.unmarked_own_binds.remove(&def_id) {
            self.error_own_bind(ISSUE_UNMARKED_INTO_OWN, span);
        }
    }

    fn dest_is_mut_or_own(&self, param: &ParamType) -> bool {
        matches!(
            param.mode,
            SemanticParamMode::MutRef | SemanticParamMode::Move
        ) || self.types.is_mut_or_own_view(param.ty)
    }

    fn dest_is_mut(&self, param: &ParamType) -> bool {
        matches!(param.mode, SemanticParamMode::MutRef) || self.types.is_mut_view(param.ty)
    }

    fn dest_is_ref(&self, param: &ParamType) -> bool {
        matches!(param.mode, SemanticParamMode::Ref) || self.types.is_ref_view(param.ty)
    }

    fn tensor_view_method(
        &self,
        receiver: &HirExpression,
        method: radix_lexer::Symbol,
        arity: usize,
    ) -> Option<&'static str> {
        let ty = receiver.ty?;
        // Only a tensor receiver can yield a borrowed view: text and list
        // slices and matrix transposes are fresh values, whatever the method
        // is named.
        let (_, inner) = self.types.peel_access_marker(ty);
        if !matches!(self.types.get(inner), Type::Tensor(_, _)) {
            return None;
        }
        let name = self.interner.strings().get(method.0 as usize)?;
        lookup_method_for_type_id(ty, self.types, name, arity).map(|entry| entry.faber_name)
    }

    fn is_view_method(name: &str) -> bool {
        matches!(
            name,
            "sectio"
                | "disloco"
                | "transpone"
                | "__faber_glyph_transpose"
                | "forma"
                | "expande"
                | "limes"
        )
    }

    fn has_tensor_value_type(&self, expr: &HirExpression) -> bool {
        expr.ty.is_some_and(|ty| {
            let (_, inner) = self.types.peel_access_marker(ty);
            matches!(
                self.types.get(inner),
                Type::Tensor(_, _) | Type::Matrix(_, _)
            )
        })
    }

    /// Whether this expression itself yields a borrowed tensor view. Element
    /// projections are tracked by `tensor_view_origin`, not treated as views.
    fn is_tensor_view(&self, expr: &HirExpression) -> bool {
        match &expr.kind {
            HirExpressionKind::Path(def_id) => self.tensor_view_roots.contains_key(def_id),
            HirExpressionKind::MethodCall(receiver, method, _, args, _) => self
                .tensor_view_method(receiver, *method, args.len())
                .is_some_and(Self::is_view_method),
            HirExpressionKind::NonNull(object, radix_hir::HirNonNullKind::Value)
                if self.has_tensor_value_type(expr) =>
            {
                self.is_tensor_view(object)
            }
            HirExpressionKind::Block(block) if self.has_tensor_value_type(expr) => block
                .expr
                .as_deref()
                .is_some_and(|expr| self.is_tensor_view(expr)),
            HirExpressionKind::Binary(radix_hir::HirBinOp::Coalesce, left, right)
                if self.has_tensor_value_type(expr) =>
            {
                self.is_tensor_view(left) || self.is_tensor_view(right)
            }
            HirExpressionKind::Si {
                then_block,
                else_block,
                ..
            } if self.has_tensor_value_type(expr) => {
                then_block
                    .expr
                    .as_deref()
                    .is_some_and(|expr| self.is_tensor_view(expr))
                    || else_block.as_ref().is_some_and(|block| {
                        block
                            .expr
                            .as_deref()
                            .is_some_and(|expr| self.is_tensor_view(expr))
                    })
            }
            HirExpressionKind::CallDefault { body, default, .. }
                if self.has_tensor_value_type(expr) =>
            {
                body.expr
                    .as_deref()
                    .is_some_and(|expr| self.is_tensor_view(expr))
                    || self.is_tensor_view(default)
            }
            HirExpressionKind::Verte { source, .. }
            | HirExpressionKind::Conversio { source, .. }
                if self.has_tensor_value_type(expr) =>
            {
                self.is_tensor_view(source)
            }
            _ => false,
        }
    }

    fn stores_tensor_view(&self, expr: &HirExpression) -> bool {
        match &expr.kind {
            HirExpressionKind::Array(elements) => elements.iter().any(|element| match element {
                radix_hir::HirArrayElement::Expr(value)
                | radix_hir::HirArrayElement::Spread(value) => self.stores_tensor_view(value),
                radix_hir::HirArrayElement::Infer(_) => false,
            }),
            HirExpressionKind::Tuple(elements, _) => {
                elements.iter().any(|value| self.stores_tensor_view(value))
            }
            HirExpressionKind::Struct(_, fields) => fields
                .iter()
                .any(|(_, value)| self.stores_tensor_view(value)),
            HirExpressionKind::Verte {
                entries: Some(entries),
                ..
            } => entries.iter().any(|entry| {
                entry
                    .value
                    .as_ref()
                    .is_some_and(|value| self.stores_tensor_view(value))
            }),
            _ => self.is_tensor_view(expr),
        }
    }

    /// Backing roots for a view expression, preserving branch alternatives.
    fn tensor_view_origins(&self, expr: &HirExpression) -> FxHashSet<DefId> {
        match &expr.kind {
            HirExpressionKind::Path(def_id) => self
                .tensor_view_roots
                .get(def_id)
                .cloned()
                .unwrap_or_default(),
            HirExpressionKind::MethodCall(receiver, method, _, args, _) => {
                let mut roots = FxHashSet::default();
                let Some(name) = self.tensor_view_method(receiver, *method, args.len()) else {
                    return roots;
                };
                if Self::is_view_method(name) {
                    roots = self.tensor_view_origins(receiver);
                    if roots.is_empty()
                        && let Some(root) = Self::root_def_id(receiver)
                    {
                        roots.insert(root);
                    }
                }
                roots
            }
            HirExpressionKind::Block(HirBlock {
                expr: Some(object), ..
            })
            | HirExpressionKind::Field(object, _)
            | HirExpressionKind::Index(object, _)
            | HirExpressionKind::Deref(object)
            | HirExpressionKind::NonNull(object, _) => self.tensor_view_origins(object),
            HirExpressionKind::Binary(radix_hir::HirBinOp::Coalesce, left, right) => {
                let mut roots = self.tensor_view_origins(left);
                roots.extend(self.tensor_view_origins(right));
                roots
            }
            HirExpressionKind::Si {
                then_block,
                else_block,
                ..
            } => {
                let mut roots = then_block
                    .expr
                    .as_deref()
                    .map(|expr| self.tensor_view_origins(expr))
                    .unwrap_or_default();
                if let Some(expr) = else_block.as_ref().and_then(|block| block.expr.as_deref()) {
                    roots.extend(self.tensor_view_origins(expr));
                }
                roots
            }
            HirExpressionKind::CallDefault { body, default, .. } => {
                let mut roots = body
                    .expr
                    .as_deref()
                    .map(|expr| self.tensor_view_origins(expr))
                    .unwrap_or_default();
                roots.extend(self.tensor_view_origins(default));
                roots
            }
            HirExpressionKind::Verte { source, .. }
            | HirExpressionKind::Conversio { source, .. } => self.tensor_view_origins(source),
            _ => FxHashSet::default(),
        }
    }

    fn arg_is_ref_view(&self, expr: &HirExpression) -> bool {
        if let Some(ty) = expr.ty
            && self.types.is_ref_view(ty)
        {
            return true;
        }
        if let Some(def_id) = Self::root_def_id(expr)
            && self.ref_roots.contains(&def_id)
        {
            return true;
        }
        match &expr.kind {
            HirExpressionKind::Field(object, _)
            | HirExpressionKind::Index(object, _)
            | HirExpressionKind::Deref(object) => self.arg_is_ref_view(object),
            _ => false,
        }
    }

    fn arg_is_mut_view(&self, expr: &HirExpression) -> bool {
        if let Some(ty) = expr.ty
            && self.types.is_mut_view(ty)
        {
            return true;
        }
        if let Some(def_id) = Self::root_def_id(expr)
            && self.mut_roots.contains(&def_id)
        {
            return true;
        }
        match &expr.kind {
            HirExpressionKind::Field(object, _)
            | HirExpressionKind::Index(object, _)
            | HirExpressionKind::Deref(object) => self.arg_is_mut_view(object),
            _ => false,
        }
    }

    fn is_projection(expr: &HirExpression) -> bool {
        matches!(
            expr.kind,
            HirExpressionKind::Field(_, _)
                | HirExpressionKind::Index(_, _)
                | HirExpressionKind::Deref(_)
        )
    }

    fn read_use(&mut self, def_id: DefId, span: radix_lexer::Span) {
        self.note_live_unmarked_own_bind(def_id);
        self.note_live_own_arg(def_id);
        let (region_mut_live, _) = self.region_liveness(def_id);
        let state = self.states.entry(def_id).or_default();
        if state.moved {
            self.error(SemanticErrorKind::UseAfterMove, ISSUE_USE_AFTER_MOVE, span);
            return;
        }
        if state.mutable || region_mut_live {
            self.conflict(ISSUE_USE_WHILE_MUTABLY_BORROWED, def_id, span);
            return;
        }
        if let Some(name) = self.conflicting_borrow_on_storage(def_id, def_id, true) {
            self.conflict(ISSUE_USE_WHILE_MUTABLY_BORROWED, name, span);
        }
    }

    fn method_call_writes_receiver(
        &self,
        call: &HirExpression,
        receiver: &HirExpression,
        method: radix_lexer::Symbol,
        args: &[HirCallArg],
    ) -> bool {
        let registry_row = receiver
            .ty
            .zip(self.interner.strings().get(method.0 as usize))
            .and_then(|(ty, name)| lookup_method_for_type_id(ty, self.types, name, args.len()));
        match registry_row {
            Some(entry) => is_mutating_receiver(entry),
            None => call
                .ty
                .is_some_and(|ty| matches!(self.types.get(ty), Type::Primitive(Primitive::Vacuum))),
        }
    }

    fn is_de_param(&self, def_id: DefId) -> bool {
        self.param_usage
            .get(&def_id)
            .is_some_and(|usage| matches!(usage.mode, HirParamMode::De))
    }

    fn write_use(&mut self, def_id: DefId, span: radix_lexer::Span) {
        self.note_live_unmarked_own_bind(def_id);
        self.note_live_own_arg(def_id);
        if let Some(usage) = self.param_usage.get_mut(&def_id)
            && !matches!(usage.mode, HirParamMode::De)
        {
            // `mut` writes the caller's value; `own`/`copy` write the
            // callee's own value. Either way the parameter is mutated.
            usage.mutated = true;
        }
        let (_region_mut_live, region_any_live) = self.region_liveness(def_id);
        let state = self.states.entry(def_id).or_default();
        if state.moved {
            self.error(SemanticErrorKind::UseAfterMove, ISSUE_USE_AFTER_MOVE, span);
            return;
        }
        if state.mutable || state.shared > 0 || region_any_live {
            self.conflict(ISSUE_WRITE_WHILE_BORROWED, def_id, span);
        } else if let Some(name) = self.conflicting_borrow_on_storage(def_id, def_id, false) {
            self.conflict(ISSUE_WRITE_WHILE_BORROWED, name, span);
        }
    }

    fn move_use(&mut self, def_id: DefId, span: radix_lexer::Span) {
        self.note_live_unmarked_own_bind(def_id);
        self.note_live_own_arg(def_id);
        if let Some(usage) = self.param_usage.get_mut(&def_id) {
            usage.moved = true;
        }
        let (_, region_any_live) = self.region_liveness(def_id);
        let state = self.states.entry(def_id).or_default();
        if state.moved {
            self.error(SemanticErrorKind::UseAfterMove, ISSUE_USE_AFTER_MOVE, span);
            return;
        }
        if state.mutable || state.shared > 0 || region_any_live {
            self.error(
                SemanticErrorKind::CannotMoveOut,
                ISSUE_CANNOT_MOVE_WHILE_BORROWED,
                span,
            );
            return;
        }
        state.moved = true;
    }

    fn borrow_shared(&mut self, def_id: DefId, span: radix_lexer::Span) {
        self.borrow_shared_with(def_id, None, span);
    }

    fn borrow_shared_with(
        &mut self,
        def_id: DefId,
        borrower: Option<DefId>,
        span: radix_lexer::Span,
    ) {
        let (region_mut_live, _) = self.region_liveness(def_id);
        let alias_conflict =
            borrower.and_then(|acting| self.conflicting_borrow_on_storage(def_id, acting, true));
        let state = self.states.entry(def_id).or_default();
        if state.moved {
            // Use-after-consume is language policy even without Rust lifetime.
            self.error(
                if self.rust_lifetime {
                    SemanticErrorKind::BorrowOfMoved
                } else {
                    SemanticErrorKind::UseAfterMove
                },
                if self.rust_lifetime {
                    ISSUE_BORROW_OF_MOVED
                } else {
                    ISSUE_USE_AFTER_MOVE
                },
                span,
            );
            return;
        }
        if state.mutable || region_mut_live {
            self.conflict(ISSUE_SHARED_BORROW_CONFLICT, def_id, span);
            return;
        }
        if let Some(name) = alias_conflict {
            self.conflict(ISSUE_SHARED_BORROW_CONFLICT, name, span);
            return;
        }
        if let Some(borrower) = borrower {
            state.region_borrows.push(TrackedRegionBorrow {
                borrower,
                source: def_id,
                kind: BorrowKind::Shared,
            });
            if let Some(scope) = self.scopes.last_mut() {
                scope.borrows.push(ScopeBorrow::Region {
                    source: def_id,
                    borrower,
                });
            }
        } else {
            state.shared = state.shared.saturating_add(1);
            if let Some(scope) = self.scopes.last_mut() {
                scope
                    .borrows
                    .push(ScopeBorrow::Flagged(def_id, BorrowKind::Shared));
            }
        }
    }

    fn borrow_mut(&mut self, def_id: DefId, span: radix_lexer::Span) {
        self.borrow_mut_with(def_id, None, span);
    }

    fn borrow_mut_with(&mut self, def_id: DefId, borrower: Option<DefId>, span: radix_lexer::Span) {
        let (_, region_any_live) = self.region_liveness(def_id);
        let alias_conflict =
            borrower.and_then(|acting| self.conflicting_borrow_on_storage(def_id, acting, false));
        let state = self.states.entry(def_id).or_default();
        if state.moved {
            self.error(
                if self.rust_lifetime {
                    SemanticErrorKind::BorrowOfMoved
                } else {
                    SemanticErrorKind::UseAfterMove
                },
                if self.rust_lifetime {
                    ISSUE_BORROW_OF_MOVED
                } else {
                    ISSUE_USE_AFTER_MOVE
                },
                span,
            );
            return;
        }
        if state.mutable || state.shared > 0 || region_any_live {
            self.conflict(ISSUE_MUTABLE_BORROW_CONFLICT, def_id, span);
            return;
        }
        if let Some(name) = alias_conflict {
            self.conflict(ISSUE_MUTABLE_BORROW_CONFLICT, name, span);
            return;
        }
        if let Some(borrower) = borrower {
            state.region_borrows.push(TrackedRegionBorrow {
                borrower,
                source: def_id,
                kind: BorrowKind::Mutable,
            });
            if let Some(scope) = self.scopes.last_mut() {
                scope.borrows.push(ScopeBorrow::Region {
                    source: def_id,
                    borrower,
                });
            }
        } else {
            state.mutable = true;
            if let Some(scope) = self.scopes.last_mut() {
                scope
                    .borrows
                    .push(ScopeBorrow::Flagged(def_id, BorrowKind::Mutable));
            }
        }
    }

    /// Region-liveness flags for a source's outstanding region-routed borrows:
    /// `(mutable live, any live)`. Missing region facts or locations are
    /// treated as live, matching the old block-exit approximation.
    fn region_liveness(&self, def_id: DefId) -> (bool, bool) {
        let Some(state) = self.states.get(&def_id) else {
            return (false, false);
        };
        let mut mutable_live = false;
        let mut any_live = false;
        for tracked in &state.region_borrows {
            if self.tracked_region_live(tracked.source, tracked.borrower) {
                any_live = true;
                if matches!(tracked.kind, BorrowKind::Mutable) {
                    mutable_live = true;
                }
            }
        }
        (mutable_live, any_live)
    }

    fn tracked_region_live(&self, source: DefId, borrower: DefId) -> bool {
        let Some(facts) = self.regions else {
            return true;
        };
        match facts
            .regions_for(borrower)
            .find(|region| region.source == source)
        {
            None => true,
            Some(region) => match self.stmt_node {
                None => true,
                Some(node) => region.is_live_at(node),
            },
        }
    }

    fn push_scope(&mut self) {
        self.scopes.push(BorrowScope {
            borrows: Vec::new(),
        });
    }

    fn pop_scope(&mut self) {
        let Some(scope) = self.scopes.pop() else {
            return;
        };
        for borrow in scope.borrows {
            match borrow {
                ScopeBorrow::Flagged(def_id, kind) => {
                    if let Some(state) = self.states.get_mut(&def_id) {
                        match kind {
                            BorrowKind::Shared => {
                                state.shared = state.shared.saturating_sub(1);
                            }
                            BorrowKind::Mutable => {
                                state.mutable = false;
                            }
                        }
                    }
                }
                ScopeBorrow::Region { source, borrower } => {
                    // The borrowing block exited; region liveness no longer
                    // decides this borrow, matching the old release point.
                    if let Some(state) = self.states.get_mut(&source) {
                        state
                            .region_borrows
                            .retain(|tracked| tracked.borrower != borrower);
                    }
                }
            }
        }
    }

    /// A borrow conflict on `def_id`: the diagnostic names the place whose
    /// borrow is live when its source name is known.
    fn conflict(&mut self, issue: &str, def_id: DefId, span: radix_lexer::Span) {
        let mut error = SemanticError::structured(SemanticErrorKind::MutableBorrowConflict, span)
            .with_arg("issue", issue);
        if let Some(spelling) = self.def_spelling(def_id) {
            error = error.with_arg("name", spelling);
        }
        self.errors.push(error);
    }

    fn error(&mut self, kind: SemanticErrorKind, issue: &str, span: radix_lexer::Span) {
        self.errors
            .push(SemanticError::structured(kind, span).with_arg("issue", issue));
    }

    fn warn_mut_local(&mut self, span: radix_lexer::Span) {
        self.errors.push(
            SemanticError::structured(
                SemanticErrorKind::Warning(WarningKind::DiscouragedMutLocal),
                span,
            )
            .with_arg("issue", ISSUE_MUT_LOCAL),
        );
    }

    fn error_own_bind(&mut self, issue: &str, span: radix_lexer::Span) {
        self.errors.push(
            SemanticError::structured(SemanticErrorKind::ModeMismatch, span)
                .with_arg("issue", issue)
                .with_arg("suggestion", SUGGESTION_COPY),
        );
    }

    fn emit_mode_lints(&mut self) {
        let mut warnings = Vec::new();
        for usage in self.param_usage.values() {
            match usage.marker {
                Some(AccessMarker::Mut) => {
                    if !usage.mutated && !usage.passed_in_or_ex {
                        warnings.push(
                            SemanticError::structured(
                                SemanticErrorKind::Warning(WarningKind::UnusedMutRefParam),
                                usage.span,
                            )
                            .with_arg("issue", ISSUE_UNUSED_MUT_PARAMETER),
                        );
                    }
                }
                Some(AccessMarker::Own) => {
                    if !usage.passed_ex && !usage.returned && !usage.moved && !usage.mutated {
                        warnings.push(
                            SemanticError::structured(
                                SemanticErrorKind::Warning(WarningKind::UnusedMoveParam),
                                usage.span,
                            )
                            .with_arg("issue", ISSUE_UNUSED_OWN_PARAMETER),
                        );
                    }
                }
                Some(AccessMarker::Ref | AccessMarker::Copy) | None => {}
            }
        }
        self.errors.extend(warnings);
    }
}

impl HirVisitor for BorrowChecker<'_> {
    fn visit_block(&mut self, block: &HirBlock) {
        self.check_block(block);
    }

    fn visit_expr(&mut self, expr: &HirExpression) {
        match &expr.kind {
            HirExpressionKind::Path(def_id) => self.read_use(*def_id, expr.span),
            HirExpressionKind::Field(object, _) | HirExpressionKind::Index(object, _)
                if matches!(object.kind, HirExpressionKind::Path(_)) =>
            {
                self.read_element_storage(expr);
                walk_expr(self, expr);
            }
            HirExpressionKind::Call(callee, _, args, _) => {
                self.visit_expr(callee);
                self.check_call_args(callee, args);
                self.check_closure_call(callee, expr.span);
            }
            HirExpressionKind::Discerne {
                scrutinees, arms, ..
            } => {
                for scrutinee in scrutinees {
                    self.visit_expr(scrutinee);
                }
                let base = self.flow_marks();
                let mut joined = Vec::new();
                for arm in arms {
                    self.restore_marks(&base);
                    self.push_scope();
                    self.visit_expr(&arm.body);
                    self.pop_scope();
                    if !matches!(&arm.body.kind, HirExpressionKind::Block(block) if own_arg::block_diverges(block))
                    {
                        joined.push(self.flow_marks());
                    }
                }
                if joined.is_empty() {
                    joined.push(base);
                }
                self.join_marks(joined);
            }
            HirExpressionKind::Si {
                cond,
                then_block,
                then_catch,
                else_block,
            } => {
                self.visit_expr(cond);
                let base = self.flow_marks();
                self.check_block(then_block);
                if let Some(catch) = then_catch {
                    self.visit_cape(catch);
                }
                let mut joined = Vec::new();
                if !own_arg::block_diverges(then_block) {
                    joined.push(self.flow_marks());
                }
                self.restore_marks(&base);
                if let Some(else_block) = else_block {
                    self.check_block(else_block);
                    if !own_arg::block_diverges(else_block) {
                        joined.push(self.flow_marks());
                    }
                } else {
                    joined.push(self.flow_marks());
                }
                if joined.is_empty() {
                    joined.push(base);
                }
                self.join_marks(joined);
            }
            HirExpressionKind::Dum(cond, block) => {
                self.visit_expr(cond);
                let entry = self.loop_entry();
                self.check_block(block);
                self.finish_loop(&entry, block);
            }
            HirExpressionKind::Loop(block) => {
                let entry = self.loop_entry();
                self.check_block(block);
                self.finish_loop(&entry, block);
            }
            HirExpressionKind::IteraRange { ranges, body, .. } => {
                for range in ranges {
                    self.visit_expr(range);
                }
                let entry = self.loop_entry();
                self.check_block(body);
                self.finish_loop(&entry, body);
            }
            HirExpressionKind::IteraLockstep {
                sources,
                bindings,
                body,
                ..
            } => {
                for source in sources {
                    self.visit_expr(source);
                }
                for (binding, _) in bindings {
                    self.ensure_state(*binding);
                    self.own.note_loop_binder(*binding);
                }
                let entry = self.loop_entry();
                self.check_block(body);
                self.finish_loop(&entry, body);
            }
            HirExpressionKind::Itera {
                mode,
                binding,
                iter,
                body: block,
                mutable,
                ..
            } => {
                self.visit_expr(iter);
                self.ensure_state(*binding);
                self.note_itera_binder_alias(*binding, *mode, iter, *mutable);
                self.own.note_loop_binder(*binding);
                let entry = self.loop_entry();
                self.check_block(block);
                self.finish_loop(&entry, block);
            }
            HirExpressionKind::Assign(target, value) => {
                let rebind = if self.flow_depth == 0 {
                    Rebind::Must
                } else {
                    Rebind::May
                };
                self.flow_depth += 1;
                self.revive_on_assign(target);
                self.check_lvalue(target);
                if self.stores_tensor_view(value) {
                    self.error(
                        SemanticErrorKind::ModeMismatch,
                        ISSUE_TENSOR_VIEW_ESCAPE,
                        value.span,
                    );
                } else {
                    self.check_mut_tensor_param_rebind(target);
                }
                if let HirExpressionKind::Path(def_id) = &target.kind {
                    self.note_bind_alias(*def_id, target.ty, value, rebind);
                }
                if target.ty.is_some_and(|ty| self.types.is_copy_view(ty))
                    || Self::root_def_id(target).is_some_and(|id| self.copy_roots.contains(&id))
                {
                    self.bind_copy_from(value);
                } else if target.ty.is_some_and(|ty| self.types.is_own_view(ty))
                    || Self::root_def_id(target).is_some_and(|id| self.own_roots.contains(&id))
                {
                    self.bind_own_from(value);
                } else if !self.is_scalar_place(target)
                    && (target.ty.is_some_and(|ty| self.types.is_mut_view(ty))
                        || Self::root_def_id(target).is_some_and(|id| self.mut_roots.contains(&id)))
                {
                    self.bind_mut_from(value, None);
                } else {
                    self.visit_expr(value);
                }
                self.flow_depth -= 1;
            }
            HirExpressionKind::CopyInto { target, value } => {
                self.check_copy_into(target, value);
            }
            // `↤` writes the target place (declared ownership/access rules
            // apply) before visiting the conversion source and its `⇥`
            // recovery — no ordinary-assignment bypass.
            HirExpressionKind::ConversioAssign {
                target,
                source,
                recovery,
                ..
            } => {
                self.check_lvalue(target);
                if self.stores_tensor_view(source) {
                    self.error(
                        SemanticErrorKind::ModeMismatch,
                        ISSUE_TENSOR_VIEW_ESCAPE,
                        source.span,
                    );
                }
                self.visit_expr(source);
                if let Some(recovery) = recovery {
                    self.visit_expr(recovery);
                }
            }
            HirExpressionKind::Clausura(params, _, _, body) => {
                if closure_captures_tensor_view(body, &self.tensor_view_roots) {
                    self.error(
                        SemanticErrorKind::ModeMismatch,
                        ISSUE_TENSOR_VIEW_ESCAPE,
                        expr.span,
                    );
                }
                self.push_scope();
                for param in params {
                    self.ensure_state(param.def_id);
                    if matches!(param.mode, HirParamMode::Owned | HirParamMode::Axis)
                        && self.types.peel_access_marker(param.ty).0.is_none()
                    {
                        self.own.note_unmarked_param(param.def_id);
                    }
                }
                self.visit_expr(body);
                self.pop_scope();
            }
            HirExpressionKind::Ref(kind, inner) => match Self::root_def_id(inner) {
                Some(def_id) if self.tracks_explicit_ref(def_id) => {
                    self.check_projection_inputs(inner);
                    match kind {
                        radix_hir::HirRefKind::Shared => self.borrow_shared(def_id, expr.span),
                        radix_hir::HirRefKind::Mutable => self.borrow_mut(def_id, expr.span),
                    }
                }
                _ => self.visit_expr(inner),
            },
            HirExpressionKind::MethodCall(receiver, method, _type_args, args, _) => {
                // A call is a receiver write when the registry marks the
                // intrinsic as mutating its receiver (value-returning in-place
                // mutators such as `remove_last` included). A method with no
                // registry row (a user method) keeps the Vacuum-return
                // heuristic. The call may conflict with an existing borrow,
                // but it does not keep a mutable borrow alive after the
                // statement.
                let is_mutating = self.method_call_writes_receiver(expr, receiver, *method, args);
                if is_mutating && !self.tensor_view_origins(receiver).is_empty() {
                    self.error(
                        SemanticErrorKind::ModeMismatch,
                        ISSUE_TENSOR_VIEW_READ_ONLY,
                        expr.span,
                    );
                }
                if is_mutating
                    && self.is_tensor_typed(receiver)
                    && let Some(origin) =
                        Self::root_def_id(receiver).and_then(|d| self.nonmut_tensor_origin(d))
                {
                    self.nonmut_tensor_write_error(origin, expr.span);
                }
                if is_mutating
                    && let Some(origin) =
                        Self::root_def_id(receiver).and_then(|d| self.nonmut_agg_alias_origin(d))
                {
                    self.nonmut_agg_method_error(origin, *method, expr.span);
                }
                if is_mutating && let Some(receiver_root) = Self::root_def_id(receiver) {
                    for arg in args {
                        if self.is_tensor_view(&arg.expr)
                            && self.tensor_view_origins(&arg.expr).contains(&receiver_root)
                        {
                            self.error(
                                SemanticErrorKind::ModeMismatch,
                                ISSUE_TENSOR_VIEW_READ_ONLY,
                                arg.expr.span,
                            );
                        }
                    }
                }
                if is_mutating {
                    if let Some(def_id) = Self::root_def_id(receiver) {
                        self.check_projection_inputs(receiver);
                        if self.is_de_param(def_id) || self.de_aliases.contains(&def_id) {
                            self.error(
                                SemanticErrorKind::AssignToImmutableBorrow,
                                ISSUE_ASSIGN_TO_DE_PARAMETER,
                                expr.span,
                            );
                        }
                        self.write_use(def_id, expr.span);
                    } else {
                        self.visit_expr(receiver);
                    }
                } else {
                    self.visit_expr(receiver);
                }
                // A declared class method is held to its parameter modes like
                // a named function: a `mut` argument may not overlap another.
                if let Some(params) = self.user_method_params(receiver, *method) {
                    self.check_args_against(&params, false, args);
                } else {
                    for arg in args {
                        self.check_arg_expr(arg);
                    }
                }
            }
            _ => walk_expr(self, expr),
        }
    }
}

struct TensorViewCaptureFinder<'a> {
    views: &'a FxHashMap<DefId, FxHashSet<DefId>>,
    found: bool,
}

impl HirVisitor for TensorViewCaptureFinder<'_> {
    fn visit_expr(&mut self, expr: &HirExpression) {
        if matches!(&expr.kind, HirExpressionKind::Path(def_id) if self.views.contains_key(def_id))
        {
            self.found = true;
            return;
        }
        walk_expr(self, expr);
    }
}

fn closure_captures_tensor_view(
    body: &HirExpression,
    views: &FxHashMap<DefId, FxHashSet<DefId>>,
) -> bool {
    let mut finder = TensorViewCaptureFinder {
        views,
        found: false,
    };
    finder.visit_expr(body);
    finder.found
}
