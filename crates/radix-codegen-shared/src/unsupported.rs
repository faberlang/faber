//! Shared diagnostics for HIR operators unsupported by a target backend.

use radix_diagnostics::DiagnosticArg;
use radix_hir::HirBinOp;
pub use radix_target::Target;

fn target_name(target: Target) -> &'static str {
    match target {
        Target::HirRust => "Rust",
        Target::HirTypeScript => "TypeScript",
        Target::HirGo => "Go",
        Target::HirSwift => "Swift",
        Target::HirHaskell => "Haskell",
        Target::HirPython => "Python",
        _ => "target",
    }
}

#[must_use]
pub fn glyph_binop(op: HirBinOp, target: Target) -> CodegenError {
    let glyph = op.glyph_spelling().unwrap_or("?");
    CodegenError {
        message: format!(
            "glyph product operator {glyph} is not supported on the {} target (Phase 3 parity)",
            target_name(target)
        ),
        args: vec![DiagnosticArg::new("op", glyph)],
    }
}

#[must_use]
pub fn addita_family(op: HirBinOp, target: Target) -> CodegenError {
    let token = match op {
        HirBinOp::Sub => "-",
        _ => "+",
    };
    CodegenError {
        message: format!(
            "tensor/vector/matrix {token} is not supported on the {} target (no scalar {token} on those families)",
            target_name(target)
        ),
        args: vec![DiagnosticArg::new("op", token)],
    }
}

/// The lifted tensor math function a method call spells, when its receiver is
/// a tensor (K5, gpu-reset rule 15). The method name is the canonical Latin
/// registry spelling, which typecheck has already written into the HIR.
#[must_use]
pub fn lifted_tensor_math(
    types: &radix_types::TypeTable,
    receiver_ty: Option<radix_types::TypeId>,
    method_name: &str,
) -> Option<radix_types::MirTensorMath> {
    let function = radix_types::MirTensorMath::from_latin_name(method_name)?;
    let mut id = receiver_ty?;
    loop {
        match types.get(id) {
            radix_types::Type::Alias(_, inner) | radix_types::Type::Ref(_, inner) => id = *inner,
            radix_types::Type::Tensor(_, _) => return Some(function),
            _ => return None,
        }
    }
}

/// Fail-closed diagnostic for a lifted tensor math function on a target with
/// no tensor math lowering (only the MIR runner and Rust lower them).
#[must_use]
pub fn tensor_math_function(function: radix_types::MirTensorMath, target: Target) -> CodegenError {
    CodegenError {
        message: format!(
            "tensor math function `{}` is not supported on the {} target",
            function.name(),
            target_name(target)
        ),
        args: vec![
            DiagnosticArg::new("issue", "tensor_math_function_unsupported"),
            DiagnosticArg::new("function", function.name()),
            DiagnosticArg::new("target", target_name(target)),
        ],
    }
}

use crate::CodegenError;
