//! Codegen-owned view of one analyzed module.
//!
//! Code generation reads a fixed set of semantic facts: the lowered program,
//! its symbol and type arenas, resolver metadata, and the per-unit side tables
//! emission threads into backends. [`CodegenAnalysis`] names that surface
//! without naming the driver's `AnalyzedModule`, so HIR emission can depend on
//! the fact contract instead of the driver carrier and the driver keeps the
//! only dependency edge.
//!
//! # Invariants
//!
//! - Every accessor answers from one coherent analysis of the same module.
//! - [`CodegenAnalysis::provider`] resolves one import-path spelling to the
//!   analyzed unit the driver carried for that edge; a failed carriage stays
//!   fail-closed as [`CodegenProvider::Failed`] instead of an absent unit.
//! - The driver depends on codegen, never the reverse: this module names no
//!   driver type.

use crate::diagnostics::Diagnostic;
use crate::hir::{DefId, HirModule, HirPresentation, LibraryRegistry};
use crate::lexer::Interner;
use crate::runtime_contract::display_tokens::DisplayTokens;
use crate::semantic::{
    FunctionFactTable, FunctionProvenanceTable, ResolvedUseIndex, Resolver, ShapeObligationTable, TypeTable,
};
use radix_hir::TensorEdgePolicyTable;
use radix_types::MirKernelBuiltin;

/// The analyzed facts one code generation run reads.
///
/// The driver implements this for its analyzed module and for each locked
/// provider unit; codegen consumes it without importing the driver carrier.
pub trait CodegenAnalysis {
    /// Symbol table that owns names referenced by the lowered program.
    fn interner(&self) -> &Interner;

    /// Type information produced by semantic analysis.
    fn types(&self) -> &TypeTable;

    /// Lowered program consumed by backend code generators.
    fn hir(&self) -> &HirModule;

    /// Resolver metadata produced by semantic analysis.
    fn resolver(&self) -> &Resolver;

    /// Upstream function effect, capture, and call facts for HIR backends.
    fn function_facts(&self) -> &FunctionFactTable;

    /// Presentation sidecar produced by the same lowering pass that produced
    /// [`CodegenAnalysis::hir`].
    fn presentation(&self) -> &HirPresentation;

    /// Warnings and informational diagnostics accumulated before codegen.
    fn diagnostics(&self) -> &[Diagnostic];

    /// Target-neutral provenance for library imports and imported declarations.
    fn libraries(&self) -> &LibraryRegistry;

    /// Shifted-view dispatch and generic provider facts.
    fn tensor_edge_policies(&self) -> &TensorEdgePolicyTable;

    /// Target-neutral resolved definition and import-use index.
    fn resolved_uses(&self) -> &ResolvedUseIndex;

    /// Compiler-owned GPU builtin call identities keyed by resolved definition.
    fn gpu_builtins(&self) -> &[(DefId, MirKernelBuiltin)];

    /// Runtime shape equalities retained by semantic analysis.
    fn shape_obligations(&self) -> &ShapeObligationTable;

    /// Durable borrow-provenance and last-use summaries.
    fn provenance(&self) -> &FunctionProvenanceTable;

    /// Output tokens this module's print statements render through.
    fn display_tokens(&self) -> &DisplayTokens;

    /// The analyzed provider unit the driver carried for one import-path
    /// spelling.
    fn provider(&self, import_path: &str) -> Option<CodegenProvider<'_>>;

    /// Every import-path spelling with a carried provider unit.
    fn provider_keys(&self) -> Vec<String>;
}

/// One resolved provider import edge's carriage.
pub enum CodegenProvider<'a> {
    /// The edge carried no analyzed unit: emission fails closed with the
    /// carriage's message.
    Failed {
        /// What failed and the leading diagnostic message.
        message: String,
    },
    /// The analyzed provider unit, owned for the walk's duration.
    Unit(Box<dyn CodegenAnalysis + 'a>),
}
