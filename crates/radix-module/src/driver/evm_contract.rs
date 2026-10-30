//! Adapt resolved field-free contract metadata to the experimental EVM leaf.

#[cfg(feature = "mir-evm")]
use super::codegen;
use super::{AnalyzedModule, SpannedCodegenError, Target};
use crate::diagnostics::DiagnosticArg;

/// Enforce a contract environment at a consumer that executes or emits code.
/// Generic source analysis and inspection do not call this gate.
///
/// # Errors
/// Returns `PKG001.contract_target_mismatch` when the consumer cannot honor
/// the annotated class's required execution environment.
pub fn require_contract_execution_target(
    analysis: &AnalyzedModule,
    target: Target,
) -> Result<(), Box<crate::Diagnostic>> {
    reject_wrong_target(analysis, target).map_err(|error| {
        let mut diagnostic = crate::Diagnostic::codegen_error(&error.message)
            .with_code("PKG001")
            .with_args(error.args);
        if let Some(span) = error.span {
            diagnostic = diagnostic.with_span(span);
        }
        Box::new(diagnostic)
    })
}

pub(super) fn reject_wrong_target(analysis: &AnalyzedModule, target: Target) -> Result<(), SpannedCodegenError> {
    if target == Target::MirEvm || target == Target::HirFaber || target == Target::HirFhir {
        return Ok(());
    }
    let Some(item) = radix_semantic::evm_contract::selected(&analysis.hir, &analysis.interner) else {
        return Ok(());
    };
    Err(SpannedCodegenError {
        message: "an EVM contract requires the EVM execution target".to_owned(),
        args: vec![
            DiagnosticArg::new("issue", "contract_target_mismatch"),
            DiagnosticArg::new("required", "evm"),
            DiagnosticArg::new("target", target.as_str()),
        ],
        span: Some(item.span),
    })
}

#[cfg(feature = "mir-evm")]
pub(super) struct ContractMetadata {
    class: crate::hir::DefId,
    methods: Vec<(crate::hir::DefId, Option<[u8; 4]>)>,
}

#[cfg(feature = "mir-evm")]
pub(super) fn resolve_metadata(analysis: &AnalyzedModule) -> Result<Option<ContractMetadata>, SpannedCodegenError> {
    use crate::hir::HirItemKind;
    use radix_mir_evm::{abi_type_name, function_selector};

    let Some(item) = radix_semantic::evm_contract::selected(&analysis.hir, &analysis.interner) else {
        return Ok(None);
    };
    let HirItemKind::Struct(class) = &item.kind else {
        return Err(refusal("selected_class_missing"));
    };
    let mut methods = Vec::new();
    let mut selectors = std::collections::BTreeSet::new();
    for method in &class.methods {
        if !method
            .annotations
            .iter()
            .any(|annotation| analysis.interner.resolve(annotation.family) == "publica")
        {
            methods.push((method.def_id, None));
            continue;
        }
        // ABI names come from source method symbols and resolved parameter
        // types. MIR names and its implicit receiver never enter a signature.
        let mut parameters = Vec::new();
        for param in &method.func.params {
            parameters
                .push(abi_type_name(&analysis.types, radix_mir::MirType::semantic(param.ty)).map_err(backend_error)?);
        }
        let signature = format!("{}({})", analysis.interner.resolve(method.func.name), parameters.join(","));
        let selector = function_selector(&signature);
        if !selectors.insert(selector) {
            return Err(refusal("selector_collision"));
        }
        methods.push((method.def_id, Some(selector)));
    }
    Ok(Some(ContractMetadata { class: item.def_id, methods }))
}

#[cfg(feature = "mir-evm")]
pub(super) fn lower_metadata(
    metadata: Option<ContractMetadata>,
    mir: &radix_mir::ValidatedMir<'_>,
) -> Result<Option<radix_mir_evm::EvmContract>, SpannedCodegenError> {
    use radix_mir_evm::{EvmContract, EvmEntry};
    let Some(metadata) = metadata else { return Ok(None) };
    let mut methods = Vec::new();
    let mut entries = Vec::new();
    let mut receiver_ty = None;
    for (def_id, selector) in metadata.methods {
        let mut matching = mir
            .program()
            .functions
            .iter()
            .filter(|function| function.source == Some(def_id));
        let function = matching
            .next()
            .ok_or_else(|| refusal("method_mir_missing"))?;
        if matching.next().is_some() {
            return Err(refusal("method_mir_ambiguous"));
        }
        let receiver = function
            .params
            .first()
            .ok_or_else(|| refusal("method_receiver_missing"))?
            .ty;
        if !matches!(mir.validation().types.get(receiver.semantic_id()), radix_types::Type::Struct(def) if *def == metadata.class)
        {
            return Err(refusal("method_receiver_identity"));
        }
        if receiver_ty.is_some_and(|previous| previous != receiver) {
            return Err(refusal("method_receiver_type_mismatch"));
        }
        receiver_ty = Some(receiver);
        methods.push(function.id);
        if let Some(selector) = selector {
            entries.push(EvmEntry { function: function.id, selector });
        }
    }
    let receiver_ty = receiver_ty.ok_or_else(|| refusal("contract_methods_missing"))?;
    Ok(Some(EvmContract { receiver_ty, methods, entries }))
}

#[cfg(feature = "mir-evm")]
fn refusal(reason: &str) -> SpannedCodegenError {
    codegen::CodegenError {
        message: "unsupported experimental EVM contract shape".to_owned(),
        args: vec![
            DiagnosticArg::new("issue", "evm_unsupported"),
            DiagnosticArg::new("target", "evm"),
            DiagnosticArg::new("reason", reason),
            DiagnosticArg::new("shape", "contract"),
        ],
    }
    .into()
}

#[cfg(feature = "mir-evm")]
pub(super) fn backend_error(error: radix_mir_evm::EvmError) -> codegen::CodegenError {
    codegen::CodegenError {
        message: error.to_string(),
        args: vec![
            DiagnosticArg::new("issue", "evm_unsupported"),
            DiagnosticArg::new("target", "evm"),
            DiagnosticArg::new("reason", error.issue),
            DiagnosticArg::new("shape", error.shape),
        ],
    }
}
