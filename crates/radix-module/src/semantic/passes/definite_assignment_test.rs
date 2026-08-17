//! Coverage for definite-assignment diagnostics.
//!
//! These tests prove that the pass rejects reads of uninitialized bindings,
//! enforces single-assignment for immutable (`fixum`/`sit`) deferred-init
//! bindings, and tracks initialization across branches (3a conservative
//! precision). Reassignment rules for `varia` and parameter modes are also
//! locked.

use crate::diagnostics::{semantic_spec, Diagnostic, DiagnosticArg};
use crate::semantic::{SemanticError, SemanticErrorKind};
use crate::test_support::args_have_no_legacy_message;

trait DefiniteAssignmentDiagnostic {
    fn has_kind_issue(&self, kind: SemanticErrorKind, issue: &str) -> bool;
    fn assert_boundary(&self);
}

impl DefiniteAssignmentDiagnostic for SemanticError {
    fn has_kind_issue(&self, kind: SemanticErrorKind, issue: &str) -> bool {
        self.kind == kind
            && self
                .args
                .iter()
                .any(|arg| arg.name == "issue" && arg.value == issue)
    }

    fn assert_boundary(&self) {
        assert!(
            args_have_no_legacy_message(&self.args),
            "migrated diagnostic should not choose prose at emit site"
        );
    }
}

impl DefiniteAssignmentDiagnostic for Diagnostic {
    fn has_kind_issue(&self, kind: SemanticErrorKind, issue: &str) -> bool {
        self.code == Some(semantic_spec(kind).code) && self.args.contains(&DiagnosticArg::new("issue", issue))
    }

    fn assert_boundary(&self) {
        assert!(
            args_have_no_legacy_message(&self.args),
            "normalized diagnostic should preserve structured args"
        );
        assert!(self.code.is_some(), "normalized diagnostic should preserve a stable code");
    }
}

fn expect_structured_error<'a, T: DefiniteAssignmentDiagnostic>(
    errors: &'a [T],
    kind: SemanticErrorKind,
    issue: &str,
) -> &'a T {
    let err = errors
        .iter()
        .find(|err| err.has_kind_issue(kind, issue))
        .expect("structured definite-assignment diagnostic");
    err.assert_boundary();
    err
}

#[test]
fn rejects_read_of_uninitialized_local() {
    let session = crate::driver::Session::new(
        crate::driver::Config::default().with_target(crate::codegen::Target::HirTypeScript),
    );
    let result = crate::driver::analyze_source(
        &session,
        "<test>",
        r"
incipit {
    fixum numerus value
    nota value
}
",
    );
    let Err(errors) = result else {
        panic!("expected uninitialized local read to be rejected");
    };
    expect_structured_error(&errors, SemanticErrorKind::UseBeforeInit, "possibly_uninitialized");
}

#[test]
fn allows_read_after_assignment() {
    let session = crate::driver::Session::new(
        crate::driver::Config::default().with_target(crate::codegen::Target::HirTypeScript),
    );
    let result = crate::driver::analyze_source(
        &session,
        "<test>",
        r"
incipit {
    varia numerus value
    value ← 1
    nota value
}
",
    );
    assert!(result.is_ok());
}

#[test]
fn allows_sponte_parameter_read_without_body_assignment() {
    let session = crate::driver::Session::new(
        crate::driver::Config::default().with_target(crate::codegen::Target::HirTypeScript),
    );
    let result = crate::driver::analyze_source(
        &session,
        "<test>",
        r"
functio pick(numerus value sponte) → numerus {
    redde value
}
",
    );
    assert!(result.is_ok());
}

#[test]
fn rejects_read_before_assignment_in_branch() {
    let session = crate::driver::Session::new(
        crate::driver::Config::default().with_target(crate::codegen::Target::HirTypeScript),
    );
    let result = crate::driver::analyze_source(
        &session,
        "<test>",
        r"
functio pick(bivalens cond) → numerus {
    fixum numerus value
    si cond {
        nota value
        value ← 1
    }
    redde 0
}
",
    );

    let Err(errors) = result else {
        panic!("expected uninitialized read inside branch");
    };
    expect_structured_error(&errors, SemanticErrorKind::UseBeforeInit, "possibly_uninitialized");
}

#[test]
fn rejects_read_after_conditional_assignment() {
    // TARGET: a binding assigned on only one branch must not read as initialized
    // after the join. This is the soundness case 3a previously missed.
    let session = crate::driver::Session::new(
        crate::driver::Config::default().with_target(crate::codegen::Target::HirTypeScript),
    );
    let result = crate::driver::analyze_source(
        &session,
        "<test>",
        r"
functio pick(bivalens cond) → numerus {
    varia numerus value
    si cond { value ← 1 }
    redde value
}
",
    );

    let Err(errors) = result else {
        panic!("expected uninitialized read after conditional assignment");
    };
    expect_structured_error(&errors, SemanticErrorKind::UseBeforeInit, "possibly_uninitialized");
}

#[test]
fn accepts_initialized_on_all_paths() {
    // TARGET: a binding assigned on both the then and else branches reads as
    // initialized after the join.
    let session = crate::driver::Session::new(
        crate::driver::Config::default().with_target(crate::codegen::Target::HirTypeScript),
    );
    let result = crate::driver::analyze_source(
        &session,
        "<test>",
        r"
functio pick(bivalens cond) → numerus {
    varia numerus value
    si cond { value ← 1 } secus { value ← 2 }
    redde value
}
",
    );
    if let Err(errors) = result {
        panic!("expected read after all-paths assignment to pass: {errors:?}");
    }
}

#[test]
fn rejects_read_after_partial_discerne_assignment() {
    // EDGE: only one discerne arm assigns the binding; the join must degrade.
    let session = crate::driver::Session::new(
        crate::driver::Config::default().with_target(crate::codegen::Target::HirTypeScript),
    );
    let result = crate::driver::analyze_source(
        &session,
        "<test>",
        r"
functio pick(numerus n) → numerus {
    varia numerus value
    discerne n {
        casu 1 { value ← 10 }
        casu 2 { tacet }
    }
    redde value
}
",
    );

    let Err(errors) = result else {
        panic!("expected uninitialized read after partial discerne assignment");
    };
    expect_structured_error(&errors, SemanticErrorKind::UseBeforeInit, "possibly_uninitialized");
}

#[test]
fn driver_accepts_initialized_local_with_initializer() {
    let session = crate::driver::Session::new(
        crate::driver::Config::default().with_target(crate::codegen::Target::HirTypeScript),
    );
    let result = crate::driver::analyze_source(&session, "<test>", r"incipit { fixum numerus n ← 1 nota n }");
    if let Err(errors) = result {
        panic!("expected initialized local to pass definite assignment: {errors:?}");
    }
}

#[test]
fn accepts_fixum_late_init_single_assignment() {
    // TARGET: `fixum` without an initializer may be assigned exactly once later,
    // then read. This is the deferred-init shape, unified with normal `fixum`.
    let session = crate::driver::Session::new(
        crate::driver::Config::default().with_target(crate::codegen::Target::HirTypeScript),
    );
    let result = crate::driver::analyze_source(
        &session,
        "<test>",
        r"
functio pick(bivalens cond) → numerus {
    fixum numerus value
    si cond { value ← 1 } secus { value ← 2 }
    redde value
}
",
    );
    if let Err(errors) = result {
        panic!("expected fixum late-init with single assignment to pass: {errors:?}");
    }
}

#[test]
fn rejects_second_assignment_to_fixum_with_initializer() {
    let session = crate::driver::Session::new(
        crate::driver::Config::default().with_target(crate::codegen::Target::HirTypeScript),
    );
    let result = crate::driver::analyze_source(
        &session,
        "<test>",
        r"
functio pick() → numerus {
    fixum numerus value ← 1
    value ← 2
    redde value
}
",
    );
    let Err(errors) = result else {
        panic!("expected second assignment to fixum to be rejected");
    };
    expect_structured_error(&errors, SemanticErrorKind::ImmutableAssignment, "immutable_second_assignment");
}

#[test]
fn rejects_second_assignment_to_fixum_late_init() {
    let session = crate::driver::Session::new(
        crate::driver::Config::default().with_target(crate::codegen::Target::HirTypeScript),
    );
    let result = crate::driver::analyze_source(
        &session,
        "<test>",
        r"
functio pick() → numerus {
    fixum numerus value
    value ← 1
    value ← 2
    redde value
}
",
    );
    let Err(errors) = result else {
        panic!("expected second late assignment to fixum to be rejected");
    };
    expect_structured_error(&errors, SemanticErrorKind::ImmutableAssignment, "immutable_second_assignment");
}

#[test]
fn accepts_varia_reassignment() {
    let session = crate::driver::Session::new(
        crate::driver::Config::default().with_target(crate::codegen::Target::HirTypeScript),
    );
    let result = crate::driver::analyze_source(
        &session,
        "<test>",
        r"
functio pick() → numerus {
    varia numerus value ← 1
    value ← 2
    value ← 3
    redde value
}
",
    );
    if let Err(errors) = result {
        panic!("expected varia reassignment to pass: {errors:?}");
    }
}

#[test]
fn rejects_reassignment_of_immutable_parameter() {
    // EDGE: owned/de/ex parameters are write-once and already initialized by the
    // caller; reassigning them in the body is a second assignment.
    let session = crate::driver::Session::new(
        crate::driver::Config::default().with_target(crate::codegen::Target::HirTypeScript),
    );
    let result = crate::driver::analyze_source(
        &session,
        "<test>",
        r"
functio pick(numerus value) → numerus {
    value ← 2
    redde value
}
",
    );
    let Err(errors) = result else {
        panic!("expected reassignment of owned parameter to be rejected");
    };
    expect_structured_error(&errors, SemanticErrorKind::ImmutableAssignment, "immutable_second_assignment");
}

#[test]
fn accepts_in_parameter_reassignment() {
    // EDGE: `in` parameters are mutable (they lower to Rust `mut` bindings) and
    // may be reassigned freely, unlike owned/de/ex parameters.
    let session = crate::driver::Session::new(
        crate::driver::Config::default().with_target(crate::codegen::Target::HirTypeScript),
    );
    let result = crate::driver::analyze_source(
        &session,
        "<test>",
        r"
functio pick(in numerus value) → numerus {
    value ← value * 2
    redde value
}
",
    );
    if let Err(errors) = result {
        panic!("expected `in` parameter reassignment to pass: {errors:?}");
    }
}

#[test]
fn accepts_sit_deferred_init_single_assignment() {
    // TARGET: `sit x` (≡ `fixum _ x`) is the inferred deferred immutable. It may
    // be assigned exactly once later, then read.
    let session = crate::driver::Session::new(
        crate::driver::Config::default().with_target(crate::codegen::Target::HirTypeScript),
    );
    let result = crate::driver::analyze_source(
        &session,
        "<test>",
        r"
functio pick(bivalens cond) → numerus {
    sit value
    si cond { value ← 1 } secus { value ← 2 }
    redde value
}
",
    );
    if let Err(errors) = result {
        panic!("expected sit deferred-init with single assignment to pass: {errors:?}");
    }
}

#[test]
fn rejects_second_assignment_to_sit_deferred_init() {
    let session = crate::driver::Session::new(
        crate::driver::Config::default().with_target(crate::codegen::Target::HirTypeScript),
    );
    let result = crate::driver::analyze_source(
        &session,
        "<test>",
        r"
functio pick() → numerus {
    sit value
    value ← 1
    value ← 2
    redde value
}
",
    );
    let Err(errors) = result else {
        panic!("expected second assignment to sit deferred binding to be rejected");
    };
    expect_structured_error(&errors, SemanticErrorKind::ImmutableAssignment, "immutable_second_assignment");
}

#[test]
fn rejects_read_before_fixum_late_init_assignment() {
    let session = crate::driver::Session::new(
        crate::driver::Config::default().with_target(crate::codegen::Target::HirTypeScript),
    );
    let result = crate::driver::analyze_source(
        &session,
        "<test>",
        r"
functio pick() → numerus {
    fixum numerus value
    nota value
    value ← 1
    redde value
}
",
    );
    let Err(errors) = result else {
        panic!("expected read before fixum late-init assignment to be rejected");
    };
    expect_structured_error(&errors, SemanticErrorKind::UseBeforeInit, "possibly_uninitialized");
}

#[test]
fn rejects_read_before_sit_deferred_init_assignment() {
    let session = crate::driver::Session::new(
        crate::driver::Config::default().with_target(crate::codegen::Target::HirTypeScript),
    );
    let result = crate::driver::analyze_source(
        &session,
        "<test>",
        r"
functio pick() → numerus {
    sit value
    nota value
    value ← 1
    redde value
}
",
    );
    let Err(errors) = result else {
        panic!("expected read before sit deferred-init assignment to be rejected");
    };
    expect_structured_error(&errors, SemanticErrorKind::UseBeforeInit, "possibly_uninitialized");
}

#[test]
fn rejects_fixum_late_init_after_partial_branch_assignment() {
    // EDGE: fixum deferred init must be assigned on all paths before read.
    let session = crate::driver::Session::new(
        crate::driver::Config::default().with_target(crate::codegen::Target::HirTypeScript),
    );
    let result = crate::driver::analyze_source(
        &session,
        "<test>",
        r"
functio pick(bivalens cond) → numerus {
    fixum numerus value
    si cond { value ← 1 }
    redde value
}
",
    );
    let Err(errors) = result else {
        panic!("expected fixum late-init read after partial branch assignment to be rejected");
    };
    expect_structured_error(&errors, SemanticErrorKind::UseBeforeInit, "possibly_uninitialized");
}

#[test]
fn rejects_sit_deferred_init_after_partial_branch_assignment() {
    let session = crate::driver::Session::new(
        crate::driver::Config::default().with_target(crate::codegen::Target::HirTypeScript),
    );
    let result = crate::driver::analyze_source(
        &session,
        "<test>",
        r"
functio pick(bivalens cond) → numerus {
    sit value
    si cond { value ← 1 }
    redde value
}
",
    );
    let Err(errors) = result else {
        panic!("expected sit deferred-init read after partial branch assignment to be rejected");
    };
    expect_structured_error(&errors, SemanticErrorKind::UseBeforeInit, "possibly_uninitialized");
}

#[test]
fn rejects_second_assignment_to_sit_with_initializer() {
    let session = crate::driver::Session::new(
        crate::driver::Config::default().with_target(crate::codegen::Target::HirTypeScript),
    );
    let result = crate::driver::analyze_source(
        &session,
        "<test>",
        r"
functio pick() → numerus {
    sit value ← 1
    value ← 2
    redde value
}
",
    );
    let Err(errors) = result else {
        panic!("expected second assignment to sit with initializer to be rejected");
    };
    expect_structured_error(&errors, SemanticErrorKind::ImmutableAssignment, "immutable_second_assignment");
}

#[test]
fn rejects_postfix_increment_before_fixum_late_init() {
    // EDGE: postfix increment reads the binding first; deferred slots reject
    // that read before their single plain assignment.
    let session = crate::driver::Session::new(
        crate::driver::Config::default().with_target(crate::codegen::Target::HirTypeScript),
    );
    let result = crate::driver::analyze_source(
        &session,
        "<test>",
        r"
functio pick() → numerus {
    fixum numerus total
    total ↑
    redde total
}
",
    );
    let Err(errors) = result else {
        panic!("expected postfix increment before fixum late-init to be rejected");
    };
    expect_structured_error(&errors, SemanticErrorKind::UseBeforeInit, "possibly_uninitialized");
}

#[test]
fn rejects_second_postfix_increment_to_fixum_late_init() {
    let session = crate::driver::Session::new(
        crate::driver::Config::default().with_target(crate::codegen::Target::HirTypeScript),
    );
    let result = crate::driver::analyze_source(
        &session,
        "<test>",
        r"
functio pick() → numerus {
    fixum numerus total
    total ← 1
    total ↑
    redde total
}
",
    );
    let Err(errors) = result else {
        panic!("expected second postfix increment to fixum late-init to be rejected");
    };
    expect_structured_error(&errors, SemanticErrorKind::ImmutableAssignment, "immutable_second_assignment");
}

#[test]
fn accepts_fixum_late_init_assigned_inside_dum_loop() {
    // EDGE: path-insensitive 3a treats a `dum` body as having run once, so a
    // deferred fixum assigned inside the loop reads as initialized afterward.
    let session = crate::driver::Session::new(
        crate::driver::Config::default().with_target(crate::codegen::Target::HirTypeScript),
    );
    let result = crate::driver::analyze_source(
        &session,
        "<test>",
        r"
functio pick() → numerus {
    fixum numerus value
    dum verum { value ← 1 }
    redde value
}
",
    );
    if let Err(errors) = result {
        panic!("expected fixum late-init assigned in dum loop to pass 3a: {errors:?}");
    }
}

#[test]
fn driver_accepts_fixum_and_sit_deferred_init_in_incipit() {
    let session = crate::driver::Session::new(
        crate::driver::Config::default().with_target(crate::codegen::Target::HirTypeScript),
    );
    let result = crate::driver::analyze_source(
        &session,
        "<test>",
        r"
incipit {
    fixum numerus typed
    sit inferred
    typed ← 7
    inferred ← 8
    nota typed
    nota inferred
}
",
    );
    if let Err(errors) = result {
        panic!("expected incipit deferred fixum/sit bindings to pass: {errors:?}");
    }
}

#[test]
fn rejects_read_after_nested_branch_partial_init() {
    // EDGE: nested branches where only the inner si assigns the binding.
    let session = crate::driver::Session::new(
        crate::driver::Config::default().with_target(crate::codegen::Target::HirTypeScript),
    );
    let result = crate::driver::analyze_source(
        &session,
        "<test>",
        r"
functio pick(bivalens outer, bivalens inner) → numerus {
    fixum numerus value
    si outer {
        si inner { value ← 1 }
    }
    redde value
}
",
    );
    let Err(errors) = result else {
        panic!("expected read after nested branch partial init to be rejected");
    };
    expect_structured_error(&errors, SemanticErrorKind::UseBeforeInit, "possibly_uninitialized");
}

#[test]
fn accepts_nested_all_paths_init() {
    // EDGE: nested branches with init on all paths.
    let session = crate::driver::Session::new(
        crate::driver::Config::default().with_target(crate::codegen::Target::HirTypeScript),
    );
    let result = crate::driver::analyze_source(
        &session,
        "<test>",
        r"
functio pick(bivalens outer, bivalens inner) → numerus {
    varia numerus value
    si outer {
        si inner { value ← 1 } secus { value ← 2 }
    } secus { value ← 3 }
    redde value
}
",
    );
    if let Err(errors) = result {
        panic!("expected nested all-paths init to pass: {errors:?}");
    }
}

// ── F-019: one-sided assigns across branch joins ──

#[test]
fn rejects_fixum_late_init_after_si_sin_partial_assignment() {
    // F-019 EDGE: fixum assigned in `si` branch only, not in `sin`;
    // the join must degrade to MaybeAssigned and reject the read.
    let session = crate::driver::Session::new(
        crate::driver::Config::default().with_target(crate::codegen::Target::HirTypeScript),
    );
    let result = crate::driver::analyze_source(
        &session,
        "<test>",
        r"
functio pick(bivalens c1, bivalens c2) → numerus {
    fixum numerus value
    si c1 { value ← 1 } sin c2 { tacet }
    redde value
}
",
    );
    let Err(errors) = result else {
        panic!("expected fixum read after si/sin partial assignment to be rejected");
    };
    expect_structured_error(&errors, SemanticErrorKind::UseBeforeInit, "possibly_uninitialized");
}

#[test]
fn rejects_fixum_late_init_after_sin_only_assignment() {
    // F-019 EDGE: fixum assigned only in `sin` branch, not in `si`;
    // the outer join must degrade to MaybeAssigned.
    let session = crate::driver::Session::new(
        crate::driver::Config::default().with_target(crate::codegen::Target::HirTypeScript),
    );
    let result = crate::driver::analyze_source(
        &session,
        "<test>",
        r"
functio pick(bivalens c1, bivalens c2) → numerus {
    fixum numerus value
    si c1 { tacet } sin c2 { value ← 1 }
    redde value
}
",
    );
    let Err(errors) = result else {
        panic!("expected fixum read after sin-only assignment to be rejected");
    };
    expect_structured_error(&errors, SemanticErrorKind::UseBeforeInit, "possibly_uninitialized");
}

#[test]
fn rejects_fixum_late_init_after_custodi_partial_assignment() {
    // F-019 EDGE: custodi guard section with one clause assigning and
    // another skipping; the sequential merge must degrade to MaybeAssigned.
    let session = crate::driver::Session::new(
        crate::driver::Config::default().with_target(crate::codegen::Target::HirTypeScript),
    );
    let result = crate::driver::analyze_source(
        &session,
        "<test>",
        r"
functio pick(bivalens c1, bivalens c2) → numerus {
    fixum numerus value
    custodi {
        si c1 ergo value ← 1
        si c2 ergo tacet
    }
    redde value
}
",
    );
    let Err(errors) = result else {
        panic!("expected fixum read after custodi partial assignment to be rejected");
    };
    expect_structured_error(&errors, SemanticErrorKind::UseBeforeInit, "possibly_uninitialized");
}

#[test]
fn rejects_varia_after_custodi_one_sided_assign() {
    // F-019 EDGE: varia declared without init, assigned in one custodi
    // clause only; the sequential merge must degrade.
    let session = crate::driver::Session::new(
        crate::driver::Config::default().with_target(crate::codegen::Target::HirTypeScript),
    );
    let result = crate::driver::analyze_source(
        &session,
        "<test>",
        r"
functio pick(bivalens c1, bivalens c2) → numerus {
    varia numerus value
    custodi {
        si c1 ergo value ← 1
        si c2 ergo tacet
    }
    redde value
}
",
    );
    let Err(errors) = result else {
        panic!("expected varia read after custodi one-sided assign to be rejected");
    };
    expect_structured_error(&errors, SemanticErrorKind::UseBeforeInit, "possibly_uninitialized");
}

#[test]
fn discerne_pattern_join_accepts_all_arms_initialized() {
    // F-019 regression: discerne arms with pattern bindings must not
    // introduce false positives — the fact that pattern bindings become
    // MaybeAssigned inside the join must not block reads of pre-existing
    // variables that are initialized on every arm.
    let session = crate::driver::Session::new(
        crate::driver::Config::default().with_target(crate::codegen::Target::HirTypeScript),
    );
    let result = crate::driver::analyze_source(
        &session,
        "<test>",
        r#"
functio pick() → numerus {
    sit value
    value ← 1
    discerne value {
        casu 1 { nota "a" }
        casu 2 { nota "b" }
    }
    redde value
}
"#,
    );
    if let Err(errors) = result {
        panic!("expected discerne with pre-initialized value to pass: {errors:?}");
    }
}

// ---- CDA-U5: `↤` stores through definite-assignment like `←` ----

#[test]
fn accepts_conversio_assign_single_fixum_late_init() {
    // NO-BYPASS (positive side): `↤` is a store, so a `fixum` deferred local
    // may receive its single value through `↤` exactly as through `←`.
    let session = crate::driver::Session::new(
        crate::driver::Config::default().with_target(crate::codegen::Target::HirTypeScript),
    );
    let result = crate::driver::analyze_source(
        &session,
        "<test>",
        r#"
functio pick() → numerus {
    fixum numerus value
    value ↤ "7"
    redde value
}
"#,
    );
    if let Err(errors) = result {
        panic!("expected single conversio-assign to fixum to pass: {errors:?}");
    }
}

#[test]
fn rejects_conversio_assign_second_assignment_to_fixum() {
    // NO-BYPASS: a second store through `↤` is a second assignment to a
    // write-once binding, matching the ordinary-assignment rule.
    let session = crate::driver::Session::new(
        crate::driver::Config::default().with_target(crate::codegen::Target::HirTypeScript),
    );
    let result = crate::driver::analyze_source(
        &session,
        "<test>",
        r#"
functio pick() → numerus {
    fixum numerus value ← 1
    value ↤ "2"
    redde value
}
"#,
    );
    let Err(errors) = result else {
        panic!("expected conversio-assign second assignment to fixum to be rejected");
    };
    expect_structured_error(&errors, SemanticErrorKind::ImmutableAssignment, "immutable_second_assignment");
}
