use super::*;
use radix_hir::DefId;

#[test]
fn gaps_empty_when_sets_agree() {
    let referenced = [DefId(1), DefId(7)].into_iter().collect();
    let emit = [DefId(1), DefId(7), DefId(9)].into_iter().collect();
    assert_eq!(def_id_closure_gaps(&referenced, &emit), []);
}

#[test]
fn gaps_name_missing_defs_in_id_order() {
    let referenced = [DefId(12), DefId(3), DefId(8)].into_iter().collect();
    let emit = [DefId(8)].into_iter().collect();
    assert_eq!(
        def_id_closure_gaps(&referenced, &emit),
        vec![DefId(3), DefId(12)]
    );
}

#[test]
fn diagnostic_names_the_missing_def() {
    let diag = missing_emit_def_diagnostic(DefId(4127), Some("Cognita"), "render.fab");
    assert_eq!(diag.code, Some(DEF_ID_CLOSURE_CODE));
    assert_eq!(diag.issue(), Some(DEF_ID_CLOSURE_ISSUE));
    assert_eq!(diag.file, "render.fab");
    assert!(
        diag.args
            .iter()
            .any(|arg| arg.name == "def_id" && arg.value == "4127")
    );
    assert!(
        diag.args
            .iter()
            .any(|arg| arg.name == "def_name" && arg.value == "Cognita")
    );
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
    assert_eq!(diags[0].issue(), Some(DEF_ID_CLOSURE_ISSUE));
    assert!(
        diags[0]
            .args
            .iter()
            .any(|arg| arg.name == "def_name" && arg.value == "Known")
    );
}
