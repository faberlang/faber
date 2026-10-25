//! Post-semantic def-id closure: every resolved reference must exist in the
//! emit def table.
//!
//! Semantic analysis installs importer-local `DefId`s (including imported
//! union-variant paths). Rust emit builds its resolvable set from the current
//! file `NameCatalog` plus imported namespace type-paths. When those layers
//! disagree, codegen defers `CODEGEN001`. This check is a set comparison over
//! the analyzed program — no codegen — so `faber check` turns red on the same
//! disagreement.

use crate::resolved_use::ResolvedUseIndex;
use radix_diagnostics::{Diagnostic, DiagnosticArg, DiagnosticPhase};
use radix_hir::DefId;
use rustc_hash::{FxHashMap, FxHashSet};

/// Stable diagnostic code for an emit-table miss.
pub const DEF_ID_CLOSURE_CODE: &str = "DEFID001";

/// Named issue for [`DEF_ID_CLOSURE_CODE`].
pub const DEF_ID_CLOSURE_ISSUE: &str = "emit_def_table_missing";

/// Resolved reference identities that emit must be able to name.
#[must_use]
pub fn resolved_reference_defs(uses: &ResolvedUseIndex) -> FxHashSet<DefId> {
    uses.locals_used()
}

/// `DefId`s referenced after semantic analysis but absent from the emit table.
#[must_use]
pub fn def_id_closure_gaps(
    referenced: &FxHashSet<DefId>,
    emit_defs: &FxHashSet<DefId>,
) -> Vec<DefId> {
    let mut missing = referenced
        .iter()
        .copied()
        .filter(|def_id| !emit_defs.contains(def_id))
        .collect::<Vec<_>>();
    missing.sort_by_key(|def_id| def_id.0);
    missing
}

/// Named diagnostic for one emit-table miss.
#[must_use]
pub fn missing_emit_def_diagnostic(def_id: DefId, name: Option<&str>, file: &str) -> Diagnostic {
    let id = def_id.0.to_string();
    let message = match name {
        Some(name) if !name.is_empty() => format!(
            "definition id {id} ({name}) is referenced after semantic analysis but missing from the emit def table"
        ),
        _ => format!(
            "definition id {id} is referenced after semantic analysis but missing from the emit def table"
        ),
    };
    let mut args = vec![
        DiagnosticArg::new("issue", DEF_ID_CLOSURE_ISSUE),
        DiagnosticArg::new("def_id", id),
    ];
    if let Some(name) = name.filter(|name| !name.is_empty()) {
        args.push(DiagnosticArg::new("def_name", name));
    }
    Diagnostic::error(message)
        .with_phase(DiagnosticPhase::Analysis)
        .with_code(DEF_ID_CLOSURE_CODE)
        .with_args(args)
        .with_file(file.to_owned())
}

/// Display name for a def on the DEFID001 surface.
///
/// Locale-pack `TypeAlias` surfaces are not imported type-paths. `name_of` used
/// to look only at path tails, so a missing pack alias reported `def_id=35`
/// with no `def_name`. Pack spellings win; imported path tails remain the
/// fallback for genuine import defs.
#[must_use]
pub fn emit_def_table_name<'a>(
    def_id: DefId,
    locale_pack_type_aliases: &'a FxHashMap<DefId, String>,
    imported_type_paths: &'a FxHashMap<DefId, String>,
) -> Option<&'a str> {
    locale_pack_type_aliases
        .get(&def_id)
        .map(String::as_str)
        .or_else(|| {
            imported_type_paths
                .get(&def_id)
                .and_then(|path| path.rsplit("::").next())
        })
}

/// Compare referenced defs against the emit table and emit one diagnostic per miss.
#[must_use]
pub fn check_def_id_closure(
    referenced: &FxHashSet<DefId>,
    emit_defs: &FxHashSet<DefId>,
    name_of: impl Fn(DefId) -> Option<String>,
    file: &str,
) -> Vec<Diagnostic> {
    def_id_closure_gaps(referenced, emit_defs)
        .into_iter()
        .map(|def_id| {
            let name = name_of(def_id);
            missing_emit_def_diagnostic(def_id, name.as_deref(), file)
        })
        .collect()
}
