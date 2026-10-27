//! S05 family-1 CUDA-column probe, green half.
//!
//! The `norm_rope` spelling exists on the `en` locale surface and lowers all
//! the way to the NVVM device module: the compile succeeds and the emitted
//! text carries the kernel entry, the NVVM annotations, and the NORM recipe
//! body. The half-split form (`rope_half`) is the red half of this probe
//! family — its NVVM lowering is still missing.
//!
//! Locale note: under `locale = "en"` the canonical `rope_norm` method is
//! spelled `norm_rope` (`locale/en/pack.toml` maps `rope_norm = "norm_rope"`).
//! Writing `rope_norm` in an `en` source fails `unknown_method` — that is the
//! locale surface, not a device fact.

use super::compile;
use crate::codegen::Target;
use crate::driver::{Config, ModuleCompileRequest, Session};

fn rope_kernel_source(method: &str) -> String {
    format!(
        r#"+++
locale = "en"
+++

@ kernel
@ public
fn rope_probe(tensor<f32, [4]> x, tensor<f32, [1, 2, 3]> table) → tensor<f32, [4]> {{
    return x.{method}<size 4>(0, table)
}}
"#
    )
}

fn nvvm_session() -> Session {
    Session::new(
        Config::default()
            .with_target(Target::MirLlvm)
            .with_dev_locale_root()
            .with_nvvm_device_module(),
    )
}

fn diagnostic_args(result: &crate::CompileResult) -> Vec<(String, String)> {
    result
        .diagnostics
        .iter()
        .flat_map(|diagnostic| {
            diagnostic
                .args
                .iter()
                .map(|arg| (arg.name.to_owned(), arg.value.clone()))
        })
        .collect()
}

fn llvm_text(result: crate::CompileResult, label: &str) -> String {
    assert!(result.success(), "{label} compile failed: {:?}", result.diagnostics);
    let Some(crate::Output::LlvmText(output)) = result.output else {
        panic!("{label}: expected llvm-text output");
    };
    output.code
}

#[test]
fn rope_half_cuda_column_is_refused_at_emit() {
    let result = compile(
        &nvvm_session(),
        "rope_half_nvvm.fab",
        &rope_kernel_source("rope_half"),
        ModuleCompileRequest::default(),
    );
    assert!(
        !result.success(),
        "if this fires, rope_half now reaches the NVVM device module on this checkout, so pin the emitted device markers instead and report the divergence"
    );

    let error = result
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.is_error())
        .unwrap_or_else(|| panic!("expected an error diagnostic, got: {:?}", result.diagnostics));
    assert_eq!(error.code, Some("CODEGEN001"));
    assert_eq!(error.phase, radix_diagnostics::DiagnosticPhase::Codegen);

    let args = diagnostic_args(&result);
    assert!(args.contains(&("category".to_owned(), "unsupported-mir-shape".to_owned())));
    assert!(
        args.iter().any(|(name, value)| {
            name == "shape" && value.contains("TensorRopeHalf") && value.contains("rotate_half")
        })
    );
}

#[test]
fn rope_norm_reaches_nvvm_device_module() {
    let code = llvm_text(
        compile(
            &nvvm_session(),
            "rope_norm_nvvm.fab",
            &rope_kernel_source("norm_rope"),
            ModuleCompileRequest::default(),
        ),
        "rope norm nvvm",
    );
    assert!(
        code.contains("@nucleum device-entry"),
        "rope norm nvvm: device module has no kernel entry:\n{code}"
    );
    assert!(
        code.contains("!nvvm.annotations"),
        "rope norm nvvm: device module has no NVVM annotations:\n{code}"
    );
    assert!(
        code.contains("%faber.rp.pair"),
        "rope norm nvvm: NORM recipe body did not emit (only a kernel entry):\n{code}"
    );
}
