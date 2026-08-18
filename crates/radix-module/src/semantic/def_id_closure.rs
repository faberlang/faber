//! Post-semantic def-id closure: every resolved reference must exist in the
//! emit def table.
//!
//! Semantic analysis installs importer-local `DefId`s (including imported
//! union-variant paths). Rust emit builds its resolvable set from the current
//! file `NameCatalog` plus imported namespace type-paths. When those layers
//! disagree, codegen defers `CODEGEN001`. This check is a set comparison over
//! the analyzed program — no codegen — so `faber check` turns red on the same
//! disagreement.

use crate::diagnostics::{Diagnostic, DiagnosticArg, DiagnosticPhase};
use crate::hir::DefId;
use crate::semantic::resolved_use::ResolvedUseIndex;
use rustc_hash::FxHashSet;

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
pub fn def_id_closure_gaps(referenced: &FxHashSet<DefId>, emit_defs: &FxHashSet<DefId>) -> Vec<DefId> {
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
        _ => format!("definition id {id} is referenced after semantic analysis but missing from the emit def table"),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hir::DefId;

    #[test]
    fn gaps_empty_when_sets_agree() {
        let referenced = [DefId(1), DefId(7)].into_iter().collect();
        let emit = [DefId(1), DefId(7), DefId(9)].into_iter().collect();
        assert!(def_id_closure_gaps(&referenced, &emit).is_empty());
    }

    #[test]
    fn gaps_name_missing_defs_in_id_order() {
        let referenced = [DefId(12), DefId(3), DefId(8)].into_iter().collect();
        let emit = [DefId(8)].into_iter().collect();
        assert_eq!(def_id_closure_gaps(&referenced, &emit), vec![DefId(3), DefId(12)]);
    }

    #[test]
    fn diagnostic_names_the_missing_def() {
        let diag = missing_emit_def_diagnostic(DefId(4127), Some("Cognita"), "render.fab");
        assert_eq!(diag.code, Some(DEF_ID_CLOSURE_CODE));
        assert_eq!(diag.issue(), Some(DEF_ID_CLOSURE_ISSUE));
        assert!(diag.message.contains("4127"));
        assert!(diag.message.contains("Cognita"));
        assert_eq!(diag.file, "render.fab");
        assert!(diag
            .args
            .iter()
            .any(|arg| arg.name == "def_id" && arg.value == "4127"));
        assert!(diag
            .args
            .iter()
            .any(|arg| arg.name == "def_name" && arg.value == "Cognita"));
    }

    #[test]
    fn check_reports_only_the_missing_side() {
        let referenced = [DefId(1), DefId(2)].into_iter().collect();
        let emit = [DefId(1)].into_iter().collect();
        let diags = check_def_id_closure(
            &referenced,
            &emit,
            |def_id| (def_id.0 == 2).then(|| "Known".to_owned()),
            "main.fab",
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, Some(DEF_ID_CLOSURE_CODE));
        assert!(diags[0].message.contains("Known"));
    }
}
