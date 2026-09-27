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

use crate::CodegenError;
