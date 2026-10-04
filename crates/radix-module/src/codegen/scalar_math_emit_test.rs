//! A2-LIFT G01: the built-in math functions as scalar float methods lower on
//! the MIR runner and Rust. The TypeScript, Go, Python, Swift and Haskell
//! leaves have no scalar math lowering and fail closed with a named
//! diagnostic, never an emitted `x.radix()` method call on a number. Rust maps
//! every function onto the std `f64` function (IEEE: NaN on a domain miss,
//! infinity on overflow), computing `f32` through `f64` as the runner does.

use crate::codegen::Target;
use crate::{Compiler, Config, Output};

fn source(function: &str, ty: &str) -> String {
    format!("functio lifted({ty} x) → {ty} {{\n    redde x.{function}()\n}}\nincipit {{\n}}\n")
}

fn compile(target: Target, source: &str) -> crate::CompileResult {
    Compiler::new(Config::default().with_target(target)).compile_str("scalar_math.fab", source)
}

fn failure_diagnostics(target: Target, source: &str) -> Vec<crate::Diagnostic> {
    let result = compile(target, source);
    assert!(
        result.output.is_none(),
        "expected fail-closed for {target:?}"
    );
    result.diagnostics
}

fn has_args(diagnostics: &[crate::Diagnostic], expected: &[(&str, &str)]) -> bool {
    diagnostics.iter().any(|d| {
        expected.iter().all(|(name, value)| {
            d.args
                .iter()
                .any(|arg| arg.name == *name && arg.value == *value)
        })
    })
}

#[cfg(all(feature = "hir-ts", feature = "hir-go", feature = "hir-swift"))]
#[test]
fn typescript_go_python_and_swift_fail_closed_for_every_function() {
    for function in radix_types::MirMathFunction::SCALAR {
        let source = source(function.latin_name(), "f32");
        for (target, label) in [
            (Target::HirTypeScript, "TypeScript"),
            (Target::HirGo, "Go"),
            (Target::HirPython, "Python"),
            (Target::HirSwift, "Swift"),
        ] {
            let diagnostics = failure_diagnostics(target, &source);
            assert!(
                has_args(
                    &diagnostics,
                    &[
                        ("issue", "scalar_math_function_unsupported"),
                        ("function", function.name()),
                        ("target", label),
                    ]
                ),
                "{label} {}: {diagnostics:?}",
                function.name()
            );
        }
    }
}

#[cfg(feature = "hir-haskell")]
#[test]
fn haskell_fails_closed_for_every_function() {
    for function in radix_types::MirMathFunction::SCALAR {
        // Bare `float` so the Haskell leaf reaches the call: it rejects a sized
        // numeric in the signature before any body.
        let diagnostics = failure_diagnostics(
            Target::HirHaskell,
            &source(function.latin_name(), "fractus"),
        );
        assert!(
            has_args(
                &diagnostics,
                &[("issue", "haskell_target_scalar_math_unsupported")]
            ),
            "Haskell {}: {diagnostics:?}",
            function.name()
        );
    }
}

#[cfg(feature = "hir-rust")]
#[test]
fn rust_f64_calls_the_std_function_directly() {
    for function in radix_types::MirMathFunction::SCALAR {
        let result = compile(Target::HirRust, &source(function.latin_name(), "f64"));
        let Some(Output::Rust(out)) = result.output else {
            panic!(
                "rust output for {}: {:?}",
                function.name(),
                result.diagnostics
            );
        };
        assert!(
            out.code.contains(&format!("x.{}()", function.name())),
            "{}:\n{}",
            function.name(),
            out.code
        );
    }
}

#[cfg(feature = "hir-rust")]
#[test]
fn rust_f32_computes_in_f64_and_rounds_back() {
    for function in radix_types::MirMathFunction::SCALAR {
        let result = compile(Target::HirRust, &source(function.latin_name(), "f32"));
        let Some(Output::Rust(out)) = result.output else {
            panic!(
                "rust output for {}: {:?}",
                function.name(),
                result.diagnostics
            );
        };
        assert!(
            out.code
                .contains(&format!("((x as f64).{}() as f32)", function.name())),
            "{}:\n{}",
            function.name(),
            out.code
        );
        // Never the carrier's trapping methods or the unresolved Latin name.
        assert!(
            !out.code.contains(&format!(".{}(", function.latin_name())),
            "{}",
            out.code
        );
    }
}
