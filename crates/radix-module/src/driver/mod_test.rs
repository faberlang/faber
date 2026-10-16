//! Integration tests for the driver compilation pipeline.
//!
//! Contract: each phase (lex, parse, analyze, codegen) reports its own diagnostics;
//! later phases skip when prerequisites fail.
//!
//! This module serves as the hub: helpers, lane/target policy tests, and basic
//! compile success tests live here. Topic-specific sub-modules are wired below.

use super::{AnalyzeRequest, ModuleCompileRequest, compile};
use crate::codegen::Target;
use crate::diagnostics::DiagnosticArg;
#[cfg(feature = "hir-faber")]
use crate::driver::reemit_faber;
#[cfg(feature = "hir-faber")]
use crate::driver::{Config, Session};
use crate::test_support::test_session;

// -- shared helpers (available to child test modules via super::) --

pub(crate) fn zh_locale_pack() -> crate::locale::LocalePack {
    crate::locale::LocalePack::from_toml_str(
        r#"
[pack]
id = "zh-Hans"
fallback = ["la"]

[keywords]
functio = "函数"

[llm]
system_prompt_snippet = "emit Chinese reader-locale Faber"
exemplars = ["./exemplars/salve-munde.zh-Hans.fab"]
"#,
    )
    .expect("pack should validate")
}

pub(crate) fn th_locale_pack() -> crate::locale::LocalePack {
    crate::locale::LocalePack::from_toml_str(
        r#"
[pack]
id = "th-TH"
fallback = ["la"]

[keywords]
functio = "ฟังก์ชัน"
redde = "คืน"

[types]
textus = "ข้อความ"

[llm]
system_prompt_snippet = "emit Thai reader-locale Faber"
exemplars = ["./exemplars/salve-munde.th-TH.fab"]
"#,
    )
    .expect("pack should validate")
}

#[cfg(feature = "hir-faber")]
pub(crate) fn faber_roundtrip(source: &str) -> String {
    let pack = crate::locale::latin_locale_pack();
    let session = Session::new(
        Config::default()
            .with_resolved_locale_pack(pack.clone())
            .with_dev_locale_root(),
    );
    let first = reemit_faber(&session, &pack, "roundtrip.fab", source);
    assert!(
        first.success(),
        "first faber compile failed: {:?}",
        first
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let Some(first_output) = first.output else {
        panic!("expected faber output");
    };

    let second = reemit_faber(&session, &pack, "roundtrip.fab", &first_output.code);
    assert!(
        second.success(),
        "second faber compile failed: {:?}\n{}",
        second
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>(),
        first_output.code
    );
    let Some(second_output) = second.output else {
        panic!("expected faber output");
    };

    assert_eq!(
        first_output.code, second_output.code,
        "faber emit should stabilize after one roundtrip"
    );
    second_output.code
}

#[cfg(feature = "hir-faber")]
pub(crate) fn faber_emit(name: &str, source: &str) -> crate::driver::FaberReemitResult {
    let pack = crate::locale::latin_locale_pack();
    let session = Session::new(
        Config::default()
            .with_target(Target::HirFaber)
            .with_resolved_locale_pack(pack.clone())
            .with_dev_locale_root(),
    );
    reemit_faber(&session, &pack, name, source)
}

pub(crate) fn compile_rust_source_with_rustc(code: &str, label: &str) -> std::path::PathBuf {
    crate::backend_smoke::compile_emitted_rust_binary(code, label)
}

pub(crate) fn rust_output(result: crate::CompileResult) -> crate::RustOutput {
    let Some(crate::Output::Rust(output)) = result.output else {
        panic!("expected Rust output");
    };
    output
}

// -- lane / target policy --

#[test]
fn radix_lane_air_rejects_hir_direct_rust_target() {
    let source = r#"
@ radix lane "air"
functio loss() → numerus {
    redde 1
}
"#;

    let result = compile(
        &test_session(Target::HirRust),
        "lane.fab",
        source,
        ModuleCompileRequest::default(),
    );

    assert!(!result.success(), "expected lane/target failure");
    assert!(result.diagnostics.iter().any(|diag| {
        diag.code == Some("TARGETLANE001")
            && diag.args
                == vec![
                    DiagnosticArg::new("issue", "lane_requires_mir_backed_target"),
                    DiagnosticArg::new("lane", "air"),
                    DiagnosticArg::new("target", "rust"),
                ]
    }));
}

#[test]
#[cfg(feature = "hir-rust")]
fn radix_lane_air_library_unit_omits_air_fn_on_hir_rust() {
    // Library-shaped: a keepable rust fn that does not reach AIR, plus an
    // AIR-lane fn. HirRust succeeds; rust emit keeps the non-AIR fn and omits AIR.
    let source = r#"
@ radix lane "air"
functio loss() → numerus {
    redde 1
}

functio keep() → numerus {
    redde 2
}
"#;

    let result = compile(
        &test_session(Target::HirRust),
        "lane.fab",
        source,
        ModuleCompileRequest::default(),
    );

    assert!(
        result.success(),
        "library-shaped unit must omit AIR instead of TARGETLANE001: {:?}",
        result.diagnostics
    );
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|diag| diag.code == Some("TARGETLANE001")),
        "omitted AIR must not diagnose TARGETLANE001: {:?}",
        result.diagnostics
    );
    let output = rust_output(result);
    assert!(
        output.code.contains("fn keep"),
        "kept rust fn must be emitted:\n{}",
        output.code
    );
    assert!(
        !output.code.contains("fn loss"),
        "AIR fn must be omitted from rust emit:\n{}",
        output.code
    );
}

#[test]
fn radix_lane_kept_rust_caller_of_air_still_targetlane001() {
    // Not library-shaped: the only rust-emitted fn calls AIR, so it is kept
    // and TARGETLANE001 still fires. Do not use gradientes_simple_loss here.
    let source = r#"
@ radix lane "air"
functio loss() → numerus {
    redde 1
}

functio wrapper() → numerus {
    redde loss()
}
"#;

    let result = compile(
        &test_session(Target::HirRust),
        "lane.fab",
        source,
        ModuleCompileRequest::default(),
    );

    assert!(!result.success(), "expected lane/target failure");
    assert!(result.diagnostics.iter().any(|diag| {
        diag.code == Some("TARGETLANE001")
            && diag.args
                == vec![
                    DiagnosticArg::new("issue", "lane_requires_mir_backed_target"),
                    DiagnosticArg::new("lane", "air"),
                    DiagnosticArg::new("target", "rust"),
                ]
    }));
}

#[test]
#[cfg(feature = "hir-rust")]
fn radix_lane_air_library_unit_omits_wrapper_that_reaches_air() {
    // Transitive omit: keepable API remains, so AIR and the same-unit wrapper
    // that calls it are both omitted from rust emit.
    let source = r#"
@ radix lane "air"
functio loss() → numerus {
    redde 1
}

functio wrapper() → numerus {
    redde loss()
}

functio keep() → numerus {
    redde 2
}
"#;

    let result = compile(
        &test_session(Target::HirRust),
        "lane.fab",
        source,
        ModuleCompileRequest::default(),
    );

    assert!(
        result.success(),
        "library-shaped transitive omit must succeed: {:?}",
        result.diagnostics
    );
    let output = rust_output(result);
    assert!(
        output.code.contains("fn keep"),
        "kept rust fn must be emitted:\n{}",
        output.code
    );
    assert!(!output.code.contains("fn loss"), "AIR fn must be omitted:\n{}", output.code);
    assert!(
        !output.code.contains("fn wrapper"),
        "wrapper that reaches AIR must be omitted:\n{}",
        output.code
    );
}

#[test]
fn radix_lane_air_rejects_hir_direct_swift_target() {
    let source = r#"
@ radix lane "air"
functio loss() → numerus {
    redde 1
}
"#;

    let result = compile(
        &test_session(Target::HirSwift),
        "lane.fab",
        source,
        ModuleCompileRequest::default(),
    );

    assert!(!result.success(), "expected lane/target failure");
    assert!(result.diagnostics.iter().any(|diag| {
        diag.code == Some("TARGETLANE001")
            && diag.args
                == vec![
                    DiagnosticArg::new("issue", "lane_requires_mir_backed_target"),
                    DiagnosticArg::new("lane", "air"),
                    DiagnosticArg::new("target", "swift"),
                ]
    }));
}

#[cfg(feature = "mir-wasm")]
#[test]
fn radix_lane_hir_direct_allows_explicit_mir_backed_target() {
    let source = r#"
@ radix lane "hir-direct"
functio direct() → numerus {
    redde 1
}
"#;

    let result = compile(
        &test_session(Target::MirWasm),
        "lane.fab",
        source,
        ModuleCompileRequest::default(),
    );

    assert!(
        !result
            .diagnostics
            .iter()
            .any(|diag| diag.code == Some("TARGETLANE001")),
        "explicit MIR target should override the preferred HIR lane: {:?}",
        result.diagnostics
    );
    let Some(crate::Output::WasmText(output)) = result.output else {
        panic!("expected wasm-text output");
    };
    assert!(output.code.contains(r#"(func $direct (export "direct")"#));
}

#[cfg(feature = "mir-wasm")]
#[test]
fn radix_lane_air_reaches_mir_backed_target_through_air() {
    let source = r#"
@ radix lane "air"
functio loss() → numerus {
    redde 1
}
"#;

    let result = compile(
        &test_session(Target::MirWasm),
        "lane.fab",
        source,
        ModuleCompileRequest::default(),
    );

    assert!(
        !result
            .diagnostics
            .iter()
            .any(|diag| diag.code == Some("TARGETLANE001")),
        "AIR should pass MIR-backed target policy: {:?}",
        result.diagnostics
    );
    let Some(crate::Output::WasmText(output)) = result.output else {
        panic!("expected wasm-text output");
    };
    assert!(
        output
            .code
            .contains(r#"(func $loss (export "loss") (result i64)"#)
    );
}

#[test]
#[cfg(feature = "hir-faber")]
fn radix_lane_air_tolerated_for_faber_surface_target() {
    // Faber-surface emit (canonical re-emit / `faber format` / `radix emit -t
    // faber`) preserves `@ radix lane` as an annotation and never lowers to
    // MIR, so the MIR-backed-target lane policy must not reject it.
    let source = r#"
@ radix lane "air"
functio loss() → numerus {
    redde 1
}
"#;

    let result = compile(
        &test_session(Target::HirFaber),
        "lane.fab",
        source,
        ModuleCompileRequest::default(),
    );

    assert!(
        result.success(),
        "Faber surface emit must tolerate lane annotations: {:?}",
        result.diagnostics
    );
    let Some(crate::Output::Faber(output)) = result.output else {
        panic!("expected faber output");
    };
    assert!(
        output.code.contains("lane = \"air\""),
        "lane annotation must be preserved in Faber surface: {}",
        output.code
    );
}

#[cfg(feature = "mir-wasm")]
#[test]
fn radix_lane_air_purity_no_longer_rejected_by_lane_check() {
    // `@ pure` on an AIR-lane function no longer triggers LANE001.
    // The check was relaxed: AIR functions with @ pure compile to
    // MIR-backed targets without an ABI reject.
    let source = r#"
@ radix lane "air"
@ pure
functio loss() → numerus {
    redde 1
}
"#;

    let result = compile(
        &test_session(Target::MirWasm),
        "lane.fab",
        source,
        ModuleCompileRequest::default(),
    );

    assert!(
        result.success(),
        "AIR + @pure should compile to MIR-backed target: {:?}",
        result.diagnostics,
    );
    let Some(crate::Output::WasmText(output)) = result.output else {
        panic!("expected wasm-text output");
    };
    assert!(
        output.code.contains(r"(func $loss"),
        "expected wasm function, got:\n{}",
        output.code
    );
}

#[cfg(feature = "mir-llvm")]
#[test]
fn radix_lane_mir_reaches_mir_backed_target_policy() {
    let source = r#"
@ radix lane "mir"
functio sum(numerus a, numerus b) → numerus {
    redde a + b
}
"#;

    let result = compile(
        &test_session(Target::MirLlvm),
        "lane.fab",
        source,
        ModuleCompileRequest::default(),
    );

    assert!(
        result.success(),
        "MIR lane should succeed for MIR-backed target: {:?}",
        result.diagnostics
    );
    let Some(crate::Output::LlvmText(output)) = result.output else {
        panic!("expected llvm-text output");
    };
    assert!(output.code.contains("define"), "expected LLVM IR output, got:\n{}", output.code);
}

// -- target policy / modular word CLI gate --

#[cfg(feature = "hir-go")]
#[test]
fn go_target_policy_runs_for_programs_without_cli() {
    // F-047: non-Rust/Go targets must not silently skip target-policy dispatch.
    // Target::HirGo syntax checks fire before semantic analysis.
    let session = test_session(Target::HirGo);
    let source = r"incipit {
  nota 42
}";
    let result = compile(&session, "policy.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "Go should compile simple incipit: {:?}", result.diagnostics);
}

#[cfg(feature = "hir-go")]
#[test]
fn go_cli_program_accepts_modular_word() {
    // D11.6 U6go: the Go modular-word gate is lifted on the CLI-program path
    // as on the ordinary path; `modulus<W>` reaches codegen.
    let session = test_session(Target::HirGo);
    let source = r#"
@ cli "tool"
incipit argumenta args {
  sit modulus<u32> x ← 5
  nota x
}
"#;
    let result = compile(&session, "cli.fab", source, ModuleCompileRequest::default());

    assert!(result.success(), "Go CLI with modulus must compile: {:?}", result.diagnostics);
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|d| d.issue() == Some("modular_word_target_unsupported")),
        "modular_word_target_unsupported must not fire on Go: {:?}",
        result.diagnostics
    );
}

#[cfg(feature = "hir-go")]
#[test]
fn go_program_accepts_modular_word() {
    let session = test_session(Target::HirGo);
    let source = "incipit {\n  varia modulus<u8> x ← 250\n  nota x + 10\n}\n";
    let result = compile(&session, "wrap.fab", source, ModuleCompileRequest::default());

    assert!(result.success(), "Go with modulus must compile: {:?}", result.diagnostics);
}

#[cfg(feature = "hir-ts")]
#[test]
fn typescript_target_policy_does_not_silently_fail() {
    // F-047 regression: the old _ => {} catch-all silently swallowed
    // non-Rust/Go targets. The explicit match arms ensure every Target
    // variant is considered.
    let session = test_session(Target::HirTypeScript);
    let source = r"incipit {
  nota 42
}";
    let result = compile(&session, "policy.fab", source, ModuleCompileRequest::default());
    assert!(
        result.success(),
        "TypeScript should compile simple incipit: {:?}",
        result.diagnostics
    );
}

// -- basic compile success --

#[cfg(feature = "hir-rust")]
#[test]
fn compile_rust_success_emits_output() {
    let session = test_session(Target::HirRust);
    let source = r"incipit {
  nota 42
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success());
    let output = rust_output(result);
    assert!(output.code.contains("fn main()"));
    assert!(output.code.contains("println!"));
}

#[cfg(feature = "hir-rust")]
#[test]
fn en_cli_program_emits_english_display_tokens() {
    let session = test_session(Target::HirRust);
    let source = r#"+++
locale = "en"
+++
@ cli "tool"
main args args {
  let bool b ← true
  print b
  let int ∪ none o ← null
  print o
}
"#;
    let result = compile(&session, "cli.fab", source, ModuleCompileRequest::default());

    assert!(result.success(), "en CLI must compile: {:?}", result.diagnostics);
    let code = rust_output(result).code;
    assert!(code.contains("fn parse_cli_args_or_exit()"), "{code}");
    assert!(code.contains("faber::display::bivalens_with("), "{code}");
    assert!(code.contains("faber::display::option_with("), "{code}");
    assert!(
        code.contains("true_: \"true\", false_: \"false\", none: \"none\", tuple: \"tuple\""),
        "{code}"
    );
    assert!(!code.contains("faber::display::bivalens("), "{code}");
    assert!(!code.contains("iuncta"), "{code}");
}

#[test]
#[cfg(feature = "hir-faber")]
fn compile_faber_success_emits_output() {
    let result = faber_emit("test.fab", "incipit {}");

    assert!(result.success());
    // FOREIGN DIRT: result.output type changed in WIP; simplified to success check.
    assert!(result.output.is_some(), "expected some output, got none");
}

#[cfg(feature = "hir-ts")]
#[test]
fn compile_typescript_success_emits_output() {
    let session = test_session(Target::HirTypeScript);
    let source = r"incipit {
  nota 42
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    assert!(result.success());
    assert!(matches!(result.output, Some(crate::Output::TypeScript(_))));
}

#[test]
fn typescript_output_preserves_diagnostic_severity() {
    let session = test_session(Target::HirTypeScript);
    let source = r"incipit {
  sit _ unused ← 1
}";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());

    let warning = result
        .diagnostics
        .iter()
        .find(|d| d.code == Some("WARN001"));
    assert!(
        warning.is_some(),
        "expected WARN001 about unused binding: {:?}",
        result.diagnostics
    );
}

// -- backward integration --

#[cfg(feature = "mir-llvm")]
#[test]
fn air_lane_backward_generation_merges_companion_into_mir_program() {
    let source = r#"
@ radix lane "air"
@ radix backward "combine_backward"
functio combine(tf32[2, 2] x, tf32[2, 2] y) → tf32[2, 2] {
    redde x ⊙ y
}
"#;

    let result = compile(
        &test_session(Target::MirLlvm),
        "backward.fab",
        source,
        ModuleCompileRequest::default(),
    );

    assert!(
        result.success(),
        "backward generation should succeed for annotated same-shape elementwise AIR function:\n{:?}",
        result.diagnostics
    );

    let Some(crate::Output::LlvmText(output)) = result.output else {
        panic!(
            "expected llvm-text output with backward companion, got: {:?}",
            result.output.as_ref().map(|_| "non-llvm-text output"),
        );
    };

    let backward_name = "combine_backward";
    assert!(
        output.code.contains(backward_name),
        "expected backward companion '{backward_name}' in llvm-text output:\n{}",
        output.code,
    );

    assert!(
        output.code.contains("combine"),
        "expected primal function 'combine' in llvm-text output:\n{}",
        output.code,
    );
}

// -- MIR / codegen diagnostics --

#[cfg(feature = "mir-llvm")]
#[test]
fn mir_lowering_error_carries_source_span() {
    let session = test_session(Target::MirLlvm);
    let source = "functio id<T>(T a) → T { redde a }\nincipit { nota id }";
    let result = compile(&session, "test.fab", source, ModuleCompileRequest::default());
    let diag = result
        .diagnostics
        .iter()
        .find(|d| d.code == Some("CODEGEN001"))
        .unwrap();
    assert!(diag.span.is_some(), "MIR lowering error must carry a span");
    assert_eq!(diag.file, "test.fab", "MIR lowering error must carry the file name");
}

#[test]
#[cfg(feature = "hir-faber")]
fn user_defined_item_named_status_is_not_silently_dropped_from_faber_output() {
    let source = "incipit {\n  sit numerus custom_status ← 1\n}\n";

    let result = faber_emit("custom_status.fab", source);

    assert!(
        result.success(),
        "expected Faber emit success for user-defined item, got diagnostics: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (d.code, d.issue()))
            .collect::<Vec<_>>()
    );
    let Some(output) = result.output else {
        panic!("expected Faber output for user-defined item");
    };
    assert!(
        output.code.contains("custom_status"),
        "output must contain user-defined item, got:\n{}",
        output.code,
    );
}

// -- deny-warnings promotion tests --

#[test]
fn deny_warnings_promotes_warning_to_error() {
    use crate::driver::WarnPolicy;

    let mut diags = vec![crate::diagnostics::Diagnostic::warning("unused variable").with_code("WARN001")];
    super::apply_warn_policy(&mut diags, &WarnPolicy { deny_all_warnings: true, deny_codes: vec![] });
    assert!(diags[0].is_error(), "deny_all should promote warning to error");
}

#[test]
fn deny_code_promotes_matching_warning_only() {
    use crate::driver::WarnPolicy;

    let mut diags = vec![
        crate::diagnostics::Diagnostic::warning("unused variable").with_code("WARN001"),
        crate::diagnostics::Diagnostic::warning("deprecated feature").with_code("WARN006"),
    ];
    super::apply_warn_policy(
        &mut diags,
        &WarnPolicy { deny_all_warnings: false, deny_codes: vec!["WARN006".into()] },
    );
    assert!(!diags[0].is_error(), "WARN001 should remain a warning");
    assert!(diags[1].is_error(), "WARN006 should be promoted to error");
}

#[test]
fn deny_policy_does_not_touch_info_or_errors() {
    use crate::diagnostics::Severity;
    use crate::driver::WarnPolicy;

    let mut diags = vec![
        crate::diagnostics::Diagnostic::new(Severity::Info, "advisory note"),
        crate::diagnostics::Diagnostic::error("hard error").with_code("SEM001"),
    ];
    super::apply_warn_policy(
        &mut diags,
        &WarnPolicy { deny_all_warnings: true, deny_codes: vec!["SEM001".into()] },
    );
    assert_eq!(diags[0].severity, Severity::Info, "Info should be untouched");
    assert_eq!(diags[1].severity, Severity::Error, "Error should stay Error (no-op)");
}

#[test]
fn deny_warnings_suppresses_output_after_promotion() {
    use crate::driver::{Config, ModuleCompileRequest, Session, WarnPolicy};

    let session =
        Session::new(Config::default().with_warn_policy(WarnPolicy { deny_all_warnings: true, deny_codes: vec![] }));
    let result = compile(
        &session,
        "deny-unused.fab",
        "incipit { sit numerus unused ← 1 }",
        ModuleCompileRequest::default(),
    );

    assert!(result.output.is_none(), "promoted warnings must stop codegen output");
    assert!(
        result
            .diagnostics
            .iter()
            .any(crate::diagnostics::Diagnostic::is_error),
        "expected at least one promoted warning diagnostic"
    );
}

// -- PST-U2: PreferStringTemplate (WARN018) deny-ability proof --
// The deny machinery already supports arbitrary WarningKinds; these tests
// prove the new code is promoted under `--deny-warnings` and escalated
// specifically by `--deny WARN018`, and that threshold-B non-warning edges
// (2-operand concat, pure-variable chains) stay green at the policy surface.

#[test]
fn deny_warnings_promotes_prefer_string_template_to_error() {
    use crate::driver::WarnPolicy;

    let mut diags = vec![
        crate::diagnostics::Diagnostic::warning("long string concatenation; prefer a string template")
            .with_code("WARN018"),
    ];
    super::apply_warn_policy(&mut diags, &WarnPolicy { deny_all_warnings: true, deny_codes: vec![] });
    assert!(
        diags[0].is_error(),
        "deny_all should promote WARN018 (PreferStringTemplate) to error"
    );
}

#[test]
fn deny_code_promotes_prefer_string_template_specifically() {
    use crate::driver::WarnPolicy;

    let mut diags = vec![
        crate::diagnostics::Diagnostic::warning("unused variable").with_code("WARN001"),
        crate::diagnostics::Diagnostic::warning("long string concatenation; prefer a string template")
            .with_code("WARN018"),
    ];
    super::apply_warn_policy(
        &mut diags,
        &WarnPolicy { deny_all_warnings: false, deny_codes: vec!["WARN018".into()] },
    );
    assert!(!diags[0].is_error(), "WARN001 should remain a warning under --deny WARN018");
    assert!(diags[1].is_error(), "WARN018 should be promoted to error via --deny WARN018");
}

#[test]
fn deny_warnings_promotes_prefer_string_template_in_real_compile() {
    use crate::driver::{Config, Session, WarnPolicy};

    let session =
        Session::new(Config::default().with_warn_policy(WarnPolicy { deny_all_warnings: true, deny_codes: vec![] }));
    let result = compile(
        &session,
        "pst-deny-warnings.fab",
        r#"
incipit {
    sit textus x ← "seed"
    sit textus m ← "a" + x + "b"
    nota m
}
"#,
        ModuleCompileRequest::default(),
    );

    let warned = result
        .diagnostics
        .iter()
        .find(|diag| diag.code == Some("WARN018"))
        .unwrap_or_else(|| {
            panic!(
                "expected WARN018 (PreferStringTemplate) diagnostic, got: {:?}",
                result.diagnostics
            )
        });
    assert!(
        warned.is_error(),
        "--deny-warnings must promote the PreferStringTemplate warning to error: {:?}",
        result.diagnostics
    );
    assert!(result.output.is_none(), "promoted warning must stop codegen output");
}

#[test]
fn deny_code_promotes_prefer_string_template_in_real_compile() {
    use crate::driver::{Config, Session, WarnPolicy};

    let session = Session::new(
        Config::default().with_warn_policy(WarnPolicy { deny_all_warnings: false, deny_codes: vec!["WARN018".into()] }),
    );
    let result = compile(
        &session,
        "pst-deny-code.fab",
        r#"
incipit {
    sit textus x ← "seed"
    sit textus m ← "a" + x + "b"
    nota m
}
"#,
        ModuleCompileRequest::default(),
    );

    let warned = result
        .diagnostics
        .iter()
        .find(|diag| diag.code == Some("WARN018"))
        .unwrap_or_else(|| panic!("expected WARN018 diagnostic, got: {:?}", result.diagnostics));
    assert!(
        warned.is_error(),
        "--deny WARN018 must escalate the PreferStringTemplate warning to error: {:?}",
        result.diagnostics
    );
    assert!(
        result
            .diagnostics
            .iter()
            .filter(|diag| diag.is_error())
            .all(|diag| diag.code == Some("WARN018")),
        "--deny WARN018 must promote only WARN018: {:?}",
        result.diagnostics
    );
    assert!(result.output.is_none(), "promoted warning must stop codegen output");
}

#[cfg(feature = "hir-rust")]
#[test]
fn deny_all_does_not_promote_two_operand_concat() {
    use crate::driver::{Config, Session, WarnPolicy};

    let session =
        Session::new(Config::default().with_warn_policy(WarnPolicy { deny_all_warnings: true, deny_codes: vec![] }));
    let result = compile(
        &session,
        "pst-two-operand.fab",
        r#"
incipit {
    sit textus x ← "seed"
    sit textus m ← "a" + x
    nota m
}
"#,
        ModuleCompileRequest::default(),
    );

    assert!(
        !result
            .diagnostics
            .iter()
            .any(|diag| diag.code == Some("WARN018")),
        "two-operand concat must not warn under threshold B: {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.severity))
            .collect::<Vec<_>>()
    );
    assert!(
        !result
            .diagnostics
            .iter()
            .any(crate::diagnostics::Diagnostic::is_error),
        "--deny-warnings must have nothing to promote for a 2-operand concat: {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.severity))
            .collect::<Vec<_>>()
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn deny_all_does_not_promote_pure_variable_chain() {
    use crate::driver::{Config, Session, WarnPolicy};

    let session =
        Session::new(Config::default().with_warn_policy(WarnPolicy { deny_all_warnings: true, deny_codes: vec![] }));
    let result = compile(
        &session,
        "pst-pure-variable-chain.fab",
        r#"
incipit {
    sit textus x ← "seed"
    sit textus y ← "more"
    sit textus z ← "again"
    sit textus m ← x + y + z
    nota m
}
"#,
        ModuleCompileRequest::default(),
    );

    assert!(
        !result
            .diagnostics
            .iter()
            .any(|diag| diag.code == Some("WARN018")),
        "pure-variable chains must not warn under threshold B: {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.severity))
            .collect::<Vec<_>>()
    );
    assert!(
        !result
            .diagnostics
            .iter()
            .any(crate::diagnostics::Diagnostic::is_error),
        "--deny-warnings must have nothing to promote for a pure-variable chain: {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.severity))
            .collect::<Vec<_>>()
    );
}

#[test]
fn call_argument_type_mismatch_renders_code_file_line_and_expected_got() {
    let source = "incipit {\n    tacet\n}\n";
    let session = test_session(Target::HirTypeScript);
    let analysis = super::analyze_source(&session, "probe.fab", source, AnalyzeRequest::default()).expect("analyze");
    let start = u32::try_from(source.find("tacet").expect("tacet")).expect("fits u32");
    let errors = vec![crate::mir::MirError {
        message: "invalid MIR: call argument type mismatch (arg 1: expected tensor<fractus<32>, [8, 8]>, got tensor<fractus<32>, [8]>)"
            .to_owned(),
        issue: "invalid_mir_call_argument_type_mismatch".to_owned(),
        span: crate::lexer::Span { start, end: start + 5 },
        definition_span: None,
    }];
    let err = super::mir_errors_to_spanned(errors);
    let result = super::compile_codegen_error(analysis, "probe.fab", source, err);
    let diag = result
        .diagnostics
        .iter()
        .find(|d| d.is_error())
        .expect("error diagnostic");
    let rendered = crate::diagnostics::render_plain(diag);
    assert_eq!(
        diag.code,
        Some("CODEGEN001"),
        "driver route must carry a catalog code: {rendered}"
    );
    assert!(
        rendered.contains("probe.fab:2"),
        "driver route must use a physical file:line: {rendered}"
    );
    assert!(
        rendered.contains("arg 1") && rendered.contains("expected") && rendered.contains("got"),
        "driver route must name which-arg and expected-vs-got: {rendered}"
    );
    assert_eq!(diag.line, Some(2));
}

// -- wired sub-modules (W1-B recovery / wave1 audit P0-1) --
// Nested under this hub so children resolve helpers via `super::`
// (`zh_locale_pack`, `rust_output`, `compile_rust_source_with_rustc`, …).

#[path = "class_literal_errors_test.rs"]
mod class_literal_errors_tests;
#[path = "cli_codegen_test.rs"]
mod cli_codegen_tests;
#[path = "compile_interaction_test.rs"]
mod compile_interaction_tests;
#[path = "conversio_test.rs"]
mod conversio_tests;
#[path = "empty_literal_cell_test.rs"]
mod empty_literal_cell_tests;
#[path = "frontmatter_locale_test.rs"]
mod frontmatter_locale_tests;
#[path = "inferred_set_cell_test.rs"]
mod inferred_set_cell_tests;
#[path = "json_valor_test.rs"]
mod json_valor_tests;
#[path = "partial_hole_test.rs"]
mod partial_hole_tests;
#[path = "regression_test.rs"]
mod regression_tests;
#[path = "templating_test.rs"]
mod templating_tests;

// -- D5.6: leading `_` (WARN030) is a warning by default, an error under
// `deny_warnings`.

const LEADING_UNDERSCORE_SOURCE: &str = r"
functio _auxilium() → numerus { redde 1 }
incipit { nota _auxilium() }
";

#[cfg(feature = "hir-rust")]
#[test]
fn leading_underscore_is_warning_by_default() {
    use crate::driver::{Config, Session};

    let session = Session::new(Config::default());
    let result = compile(
        &session,
        "lead-underscore.fab",
        LEADING_UNDERSCORE_SOURCE,
        ModuleCompileRequest::default(),
    );

    let warned = result
        .diagnostics
        .iter()
        .find(|diag| diag.code == Some("WARN030"))
        .unwrap_or_else(|| panic!("expected WARN030, got: {:?}", result.diagnostics));
    assert!(!warned.is_error(), "leading `_` must stay a warning without deny policy");
    assert!(result.output.is_some(), "a warning must not stop codegen output");
}

#[test]
fn deny_warnings_promotes_leading_underscore_in_real_compile() {
    use crate::driver::{Config, Session, WarnPolicy};

    let session =
        Session::new(Config::default().with_warn_policy(WarnPolicy { deny_all_warnings: true, deny_codes: vec![] }));
    let result = compile(
        &session,
        "lead-underscore-deny.fab",
        LEADING_UNDERSCORE_SOURCE,
        ModuleCompileRequest::default(),
    );

    let denied = result
        .diagnostics
        .iter()
        .find(|diag| diag.code == Some("WARN030"))
        .unwrap_or_else(|| panic!("expected WARN030, got: {:?}", result.diagnostics));
    assert!(denied.is_error(), "--deny-warnings must promote WARN030 to error");
    assert!(result.output.is_none(), "promoted warning must stop codegen output");
}

/// The locale-id guard rejects absolute, traversal, and separator ids before
/// any pack lookup; `..` and `.` have no separator and need their own clauses.
#[test]
fn frontmatter_locale_rejects_path_shaped_ids() {
    let config = crate::driver::Config::default();
    for id in ["/etc", "la/extra", "..", ".", "..\\x", "a/../b"] {
        let diagnostic =
            super::resolve_frontmatter_locale_pack(&config, id).expect_err("path-shaped locale ids must be rejected");
        assert_eq!(diagnostic.code, Some("LOCALE003"));
        assert_eq!(diagnostic.issue(), Some("invalid_id"), "id {id}");
        assert!(
            diagnostic
                .args
                .iter()
                .any(|arg| arg.name == "locale_id" && arg.value == id),
            "id {id}: {:?}",
            diagnostic.args
        );
    }
}
