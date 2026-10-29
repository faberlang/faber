//! Emission assembly tests (RP4, moved from faber's `compile_test.rs` Rust
//! cases). Each test drives the full package emission path through explicit
//! test seams — no ambient locale, manifest, or library-home reads.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(any(feature = "hir-rust", feature = "hir-go"))]
use radix_module::Output;
use radix_module::codegen::Target;
use radix_module::diagnostics::Diagnostic;
use radix_module::driver::Config;

use super::*;
use crate::test_support::test_analysis_seams;

#[cfg(feature = "hir-python")]
#[test]
fn package_result_python_modules_start_empty() {
    let result = compile_failure(vec![]);
    assert_eq!(result.python_modules, [] as [(String, String); 0]);
}

/// Minimal emission seams for the Rust emission tests: no product manifest
/// validation, no ambient library home, and a never-exercised Go emitter
/// (these tests only run the Rust path).
fn test_seams() -> EmissionSeams {
    EmissionSeams {
        analysis: test_analysis_seams(),
        #[cfg(feature = "hir-go")]
        emit_go: never_exercised_go_emission,
        #[cfg(feature = "hir-ts")]
        emit_ts: never_exercised_ts_emission,
    }
}

#[cfg(feature = "hir-go")]
fn never_exercised_go_emission(_package: &AnalyzedProgram, _input: &Path) -> PackageCompileResult {
    unreachable!("go emission is a faber-side seam; radix-program tests only run the Rust path")
}

#[cfg(feature = "hir-ts")]
fn never_exercised_ts_emission(
    _package: &AnalyzedProgram,
    _input: &Path,
) -> radix_module::CompileResult {
    unreachable!("ts emission is a faber-side seam; radix-program tests only run the Rust path")
}

/// Real Go emission seam for the single-stream merge test: one full Go file
/// per unit, the raw shape `radix-module` emits before product assembly wraps
/// (or drops) the sibling files.
#[cfg(feature = "hir-go")]
fn go_package_test_emission(package: &AnalyzedProgram, _input: &Path) -> PackageCompileResult {
    let latin = radix_module::locale::latin_locale_pack();
    let surface = radix_module::locale::KeywordSurface::new(&latin);
    let mut entry_code = None;
    let mut go_modules = Vec::new();
    for unit in package.nodes.values() {
        if let Ok(Output::Go(go)) =
            radix_module::codegen::generate_from_analyzed(Target::HirGo, &unit.analysis, &surface)
        {
            if unit.is_entry {
                entry_code = Some(go.code);
            } else {
                go_modules.push((format!("{}.go", unit.module_segments.join("_")), go.code));
            }
        }
    }
    let output = entry_code.map(|code| Output::Go(radix_module::GoOutput { code }));
    PackageCompileResult {
        compile_result: radix_module::CompileResult {
            output,
            diagnostics: package.diagnostics.clone(),
        },
        go_modules,
        #[cfg(feature = "hir-python")]
        python_modules: Vec::new(),
    }
}

fn temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("radix-program-emit-{label}-{nonce}"));
    fs::create_dir_all(&path).expect("create temp dir");
    path
}

#[test]
fn compile_package_deny_warnings_suppresses_output_after_promotion() {
    let dir = temp_dir("package-deny-warnings");
    fs::create_dir_all(dir.join("src")).expect("create src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "package-deny-warnings"

[paths]
source = "src"
entry = "main.fab"
"#,
    )
    .expect("write manifest");
    fs::write(dir.join("src/main.fab"), "main { let int unused ← 1 }\n").expect("write entry");

    let config = Config::default().with_warn_policy(radix_module::driver::WarnPolicy {
        deny_all_warnings: true,
        deny_codes: vec![],
    });
    let result = compile_package(&config, &dir, &test_seams());

    assert!(
        result.output.is_none(),
        "promoted package warnings must stop generated output"
    );
    assert!(
        result.diagnostics.iter().any(Diagnostic::is_error),
        "expected at least one promoted warning diagnostic"
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn rust_multi_unit_sibling_exports_resolve_via_package_index() {
    // FBR-P2-007: Rust sibling export resolution must use one package-wide
    // normalized path index (built once, current unit excluded by identity)
    // instead of rebuilding a candidate map per unit. A multi-unit package
    // with a cross-module call proves the sibling set is complete and
    // deterministic: the generated entry references the imported module.
    let dir = temp_dir("rust-sibling-index");
    fs::write(
        dir.join("main.fab"),
        r#"
import from "./lib" lib

main {
    print lib.answer()
}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("lib.fab"),
        r"
@ public
fn answer() → int {
    return 42
}
",
    )
    .expect("write lib");

    let result = compile_package(&Config::default(), &dir.join("main.fab"), &test_seams());
    assert!(
        result.success(),
        "expected Rust multi-unit compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };
    assert!(
        output.code.contains("pub mod lib"),
        "expected sibling module declaration in generated rust code"
    );
}

#[test]
fn compile_package_rust_targets_plan_go_and_python() {
    // Target-neutral planning parity: the Rust emission entry must reject
    // non-HIR package targets through the same plan gates the dispatcher
    // applies (unsupported targets are rejected; Go and TypeScript dispatch to
    // product seams, while python exercises the assembly-pending branch).
    let dir = temp_dir("plan-gates");
    fs::create_dir_all(dir.join("src")).expect("create src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "plan-gates"

[paths]
source = "src"
entry = "main.fab"
"#,
    )
    .expect("write manifest");
    fs::write(dir.join("src/main.fab"), "main { }\n").expect("write entry");

    let unsupported = compile_package(
        &Config::default().with_target(Target::MirWgsl),
        &dir,
        &test_seams(),
    );
    assert!(
        unsupported
            .diagnostics
            .iter()
            .any(|diag| diag.issue() == Some("package_target_unsupported")),
        "expected package_target_unsupported, got {:?}",
        unsupported
            .diagnostics
            .iter()
            .map(|diag| diag.issue())
            .collect::<Vec<_>>()
    );

    #[cfg(feature = "hir-python")]
    {
        let pending = compile_package(
            &Config::default().with_target(Target::HirPython),
            &dir,
            &test_seams(),
        );
        assert!(
            pending
                .diagnostics
                .iter()
                .any(|diag| diag.issue() == Some("package_target_assembly_pending")),
            "expected package_target_assembly_pending, got {:?}",
            pending
                .diagnostics
                .iter()
                .map(|diag| diag.issue())
                .collect::<Vec<_>>()
        );
    }
}

#[cfg(feature = "hir-rust")]
#[test]
fn valor_octeti_body_extraction_emits_octeti_match() {
    // Inferentia HTTP body seam: the provider delivers valor octeti; `value ↦
    // bytes` must extract `Valor::Octeti` (not the generic FromValor lista path).
    let dir = temp_dir("valor-octeti-extract");
    fs::write(
        dir.join("main.fab"),
        r"
main {
    let value body ← |09| ↦ value
    let list<b8> payload ← body ↦ list<b8>
    print payload.length()
}
",
    )
    .expect("write entry");

    let result = compile_package(&Config::default(), &dir.join("main.fab"), &test_seams());
    assert!(
        result.success(),
        "expected valor ↦ octeti compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };
    assert!(
        output.code.contains("faber::Valor::Octeti(__bytes)")
            && output
                .code
                .contains("faber::Trap::Other(\"valor to list<b8> conversion failed\")"),
        "expected valor octeti extraction, got:\n{}",
        output.code
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn rust_package_deduplicates_imported_tensor_edge_policy_providers() {
    let dir = temp_dir("tensor-edge-policy-provider-dedup");
    fs::write(
        dir.join("main.fab"),
        r#"+++
locale = "en"
+++

import from "./consumer_a" consumer_a
import from "./consumer_b" consumer_b

main {
    let tf32[3] source ← [1.0, 2.0, 3.0] ↦ tf32[3]
    print consumer_a.resolve_a(source).flatten()
    print consumer_b.resolve_b(source).flatten()
}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("policy.fab"),
        r"
@ public
class ZeroGhost {
    @ public
    fn remap(vector<int, size 1> coord) → vector<int, size 1> {
        return [0] ↦ vector<int, size 1>
    }
}
",
    )
    .expect("write policy");
    for (module, function) in [("consumer_a", "resolve_a"), ("consumer_b", "resolve_b")] {
        fs::write(
            dir.join(format!("{module}.fab")),
            format!(
                r#"+++
locale = "en"
+++

import from "./policy" policy

@ public
fn {function}(tf32[3] source) → tf32[3] {{
    return source.shift([1]).edge<policy.ZeroGhost>().materialize()
}}
"#
            ),
        )
        .expect("write consumer module");
    }

    let result = compile_package(
        &Config::default().with_target(Target::HirRust),
        &dir.join("main.fab"),
        &test_seams(),
    );
    assert!(
        result.success(),
        "expected multi-module tensor edge policy compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected Rust output");
    };
    let provider_impl = "impl faber::TensorEdgePolicyProvider<f32, 1> for crate::policy::ZeroGhost";
    assert_eq!(
        output.code.matches(provider_impl).count(),
        1,
        "duplicate imported custom-policy uses must have one package impl:\n{}",
        output.code
    );
    assert!(output.code.contains("pub mod consumer_a"));
    assert!(output.code.contains("pub mod consumer_b"));
    assert!(output.code.contains("fn resolve_a"));
    assert!(output.code.contains("fn resolve_b"));
    let _ = fs::remove_dir_all(&dir);
}

#[cfg(feature = "hir-go")]
#[test]
fn compile_package_go_single_stream_defines_imported_declarations() {
    // The single-stream emit path (`faber emit -t go <package>` and the
    // `faber corpus --target go` recipe that captures stdout into `main.go`)
    // receives one Go source. The entry names the imported interface and
    // genus, so the sibling module's declarations must be merged into that
    // source; dropping the sibling files leaves `go build` with undefined
    // names.
    let dir = temp_dir("go-contract-import");
    fs::write(
        dir.join("main.fab"),
        r#"
import from "./shapes"

fn render(shapes.Figurable f) → string {
    return f.draw()
}

main {
    let shapes.Circle c ← shapes.Circle { radius = 25 }
    print render(c)
}
"#,
    )
    .expect("write entry");
    fs::write(
        dir.join("shapes.fab"),
        r#"
module shapes

@ public
interface Figurable {
    fn draw() → string
}

@ public
class Circle implements Figurable {
    mut int radius = 10

    fn draw() → string {
        return "circle §"(self.radius)
    }
}
"#,
    )
    .expect("write shapes module");

    let seams = EmissionSeams {
        analysis: test_analysis_seams(),
        #[cfg(feature = "hir-go")]
        emit_go: go_package_test_emission,
        #[cfg(feature = "hir-ts")]
        emit_ts: never_exercised_ts_emission,
    };
    let result = compile_package(
        &Config::default().with_target(Target::HirGo),
        &dir.join("main.fab"),
        &seams,
    );
    assert!(
        result.success(),
        "go package compile failed: {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let Some(Output::Go(output)) = result.output else {
        panic!("expected Go output");
    };
    assert!(
        output.code.contains("type Figurable interface"),
        "single-stream Go output must define the imported interface:\n{}",
        output.code
    );
    assert!(
        output.code.contains("type Circle struct"),
        "single-stream Go output must define the imported genus:\n{}",
        output.code
    );
    assert!(
        output.code.contains("func (self *Circle) Draw() string"),
        "single-stream Go output must define the imported method:\n{}",
        output.code
    );
    assert!(
        !output.code.contains("/* unresolved_def */"),
        "no unresolved placeholder may reach emitted Go:\n{}",
        output.code
    );
    let _ = fs::remove_dir_all(&dir);
}
