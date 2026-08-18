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

/// Display name for a def on the DEFID001 surface.
///
/// Locale-pack TypeAlias surfaces are not imported type-paths. `name_of` used
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

    #[cfg(feature = "hir-rust")]
    #[test]
    fn rust_locale_pack_type_alias_def_reaches_emit_table() {
        use crate::codegen::rust::emit_def_table;
        use crate::codegen::Target;
        use crate::semantic::{analyze, analyze_resolved_uses, PassConfig};
        use rustc_hash::FxHashMap;

        let pack_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../stdlib/locale/th-TH/pack.toml");
        let pack = crate::locale::LocalePack::from_toml_path(&pack_path).expect("installed th-TH pack should load");
        let source = "ฟังก์ชัน ก(จำนวน n) → จำนวน {\n  คืน n\n}\n";
        let lex = crate::lexer::lex_with_locale_pack(source, &pack);
        let mut parsed = crate::parser::parse(lex);
        assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
        let program = parsed.program.expect("program");
        let mut config = PassConfig::for_target(Target::HirRust);
        config.locale_pack = Some(pack);
        let result = analyze(&program, &config, &mut parsed.interner);
        assert!(
            result.diagnostics.iter().all(|diag| !diag.is_error()),
            "analysis errors: {:?}",
            result.diagnostics
        );
        let hir = result.hir.expect("hir");
        let soan = parsed.interner.intern("จำนวน");
        let def_id = result
            .resolver
            .lookup_type(soan)
            .expect("th-TH numerus surface จำนวน must register a TypeAlias DefId");
        let surface = parsed.interner.resolve(soan).to_owned();
        let aliases = result
            .resolver
            .locale_pack_type_alias_names(&parsed.interner);
        assert_eq!(aliases.get(&def_id), Some(&surface));

        let imported = FxHashMap::default();
        let emit = emit_def_table(&hir, &parsed.interner, std::iter::empty(), &imported, aliases.keys().copied());
        assert!(
            emit.contains(&def_id),
            "locale-pack TypeAlias DefId {def_id:?} must reach emit_def_table"
        );

        let referenced = [def_id].into_iter().collect();
        let missing = check_def_id_closure(
            &referenced,
            &FxHashSet::default(),
            |id| emit_def_table_name(id, &aliases, &imported).map(str::to_owned),
            "main.fab",
        );
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].code, Some(DEF_ID_CLOSURE_CODE));
        assert!(
            missing[0]
                .args
                .iter()
                .any(|arg| arg.name == "def_name" && arg.value == surface),
            "DEFID001 must name the pack alias, got {:?}",
            missing[0].args
        );

        let uses = analyze_resolved_uses(&hir, &result.types, &crate::semantic::QualifiedIdentityTable::new());
        let live = resolved_reference_defs(&uses);
        let admitted = check_def_id_closure(
            &live,
            &emit,
            |id| emit_def_table_name(id, &aliases, &imported).map(str::to_owned),
            "main.fab",
        );
        assert!(
            admitted.is_empty(),
            "admitted pack aliases must not trip DEFID001: {admitted:?}"
        );
    }
}
