//! Shared diagnostics for HIR operators unsupported by a target backend.

use radix_diagnostics::DiagnosticArg;
use radix_hir::HirBinOp;
pub use radix_target::Target;

fn target_name(target: Target) -> &'static str {
    match target.as_str() {
        "rust" => "Rust",
        "ts" => "TypeScript",
        "go" => "Go",
        "swift" => "Swift",
        "llvm-text" | "llvm-host" => "LLVM",
        "wasm-text" => "WebAssembly",
        "haskell" => "Haskell",
        "cobol" => "COBOL",
        "python" => "Python",
        _ => "target",
    }
}

/// Reject class values used as map keys or set elements on backends whose
/// native collections compare object identity instead of Faber value equality.
/// Rust and Go have dedicated value-key representations and do not use this
/// guard.
pub fn reject_genus_collection_keys(
    types: &radix_types::TypeTable,
    target: Target,
) -> Result<(), CodegenError> {
    fn contains_genus(
        types: &radix_types::TypeTable,
        type_id: radix_types::TypeId,
        seen: &mut handroll::FxHashSet<radix_types::TypeId>,
    ) -> bool {
        if !seen.insert(type_id) {
            return false;
        }
        match types.get(type_id) {
            radix_types::Type::Struct(_) => true,
            radix_types::Type::Option(inner)
            | radix_types::Type::Ref(_, inner)
            | radix_types::Type::Alias(_, inner) => contains_genus(types, *inner, seen),
            radix_types::Type::Tuple(parts) | radix_types::Type::Union(parts) => {
                parts.iter().any(|part| contains_genus(types, *part, seen))
            }
            radix_types::Type::Applied(base, args) => {
                contains_genus(types, *base, seen)
                    || args.iter().any(|arg| match arg {
                        radix_types::AppliedArg::Type(inner) => contains_genus(types, *inner, seen),
                        radix_types::AppliedArg::Size(_) => false,
                    })
            }
            _ => false,
        }
    }

    let issue = match target.as_str() {
        "ts" => "ts_genus_collection_key_unsupported",
        "python" => "python_genus_collection_key_unsupported",
        "swift" => "swift_genus_collection_key_unsupported",
        "llvm-text" | "llvm-host" => "llvm_genus_collection_key_unsupported",
        "wasm-text" => "wasm_genus_collection_key_unsupported",
        _ => return Ok(()),
    };
    for index in 0..types.type_count() {
        let type_id = radix_types::TypeId(index as u32);
        let key = match types.get(type_id) {
            radix_types::Type::Map(key, _) | radix_types::Type::Set(key) => *key,
            _ => continue,
        };
        if contains_genus(types, key, &mut handroll::FxHashSet::default()) {
            return Err(CodegenError {
                message: format!(
                    "class map/set keys are not value-keyed on the {} target",
                    target_name(target)
                ),
                args: vec![DiagnosticArg::new("issue", issue)],
            });
        }
    }
    Ok(())
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
) -> Option<radix_types::MirMathFunction> {
    let function = radix_types::MirMathFunction::from_latin_name(method_name)?;
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
pub fn tensor_math_function(
    function: radix_types::MirMathFunction,
    target: Target,
) -> CodegenError {
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

/// The built-in scalar math function a method call spells, when its receiver
/// is a scalar float (`x.sqrt()`, `x.sin()`). Scalar `absolutum` is a numeric
/// method every target already lowers, so it is never reported here. The
/// method name is the canonical Latin registry spelling, which typecheck has
/// already written into the HIR.
#[must_use]
pub fn scalar_math(
    types: &radix_types::TypeTable,
    receiver_ty: Option<radix_types::TypeId>,
    method_name: &str,
) -> Option<radix_types::MirMathFunction> {
    let function = radix_types::MirMathFunction::SCALAR
        .into_iter()
        .find(|function| function.latin_name() == method_name)?;
    let mut id = receiver_ty?;
    loop {
        match types.get(id) {
            radix_types::Type::Alias(_, inner) | radix_types::Type::Ref(_, inner) => id = *inner,
            radix_types::Type::Primitive(radix_types::Primitive::Fractus)
            | radix_types::Type::SizedNumeric(radix_types::Primitive::Fractus, _) => {
                return Some(function);
            }
            _ => return None,
        }
    }
}

/// Fail-closed diagnostic for a scalar math function on a target with no
/// scalar math lowering (only the MIR runner and Rust lower them).
#[must_use]
pub fn scalar_math_function(
    function: radix_types::MirMathFunction,
    target: Target,
) -> CodegenError {
    CodegenError {
        message: format!(
            "scalar math function `{}` is not supported on the {} target",
            function.name(),
            target_name(target)
        ),
        args: vec![
            DiagnosticArg::new("issue", "scalar_math_function_unsupported"),
            DiagnosticArg::new("function", function.name()),
            DiagnosticArg::new("target", target_name(target)),
        ],
    }
}

use crate::CodegenError;
