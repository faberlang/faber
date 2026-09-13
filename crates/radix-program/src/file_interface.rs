//! Cross-unit file-interface snapshot extraction for package compilation.
//!
//! RP2c: moved from `crates/faber/src/package/file_interface.rs`. Extracts
//! the portable export surface of an analyzed unit (functions, types,
//! interfaces, constants, `@ radix backward` companions) for import-boundary
//! contracts between package units and library interfaces.

use radix_module::diagnostics::{Diagnostic, DiagnosticPhase};
use radix_module::driver::AnalyzedModule;
use radix_module::file_interface::{
    FileExport, FileExportKind, FileInterface, FileInterfaceError, InterfaceAliasExport,
    InterfaceAnnotationContract, InterfaceAnnotationContractField, InterfaceBodyChannel,
    InterfaceDeviceContract, InterfaceDeviceRole, InterfaceDeviceVisibility, InterfaceEnumVariant,
    InterfaceGenericParam, InterfaceLibraryIdentity, InterfaceMethodExport, InterfaceNominalExport,
    InterfaceQualifiedIdentity, InterfaceStructExport, InterfaceStructField,
    snapshot_interface_callable_with_identity, snapshot_interface_type_with_identity,
};
use radix_module::hir::{
    DefId, HirConst, HirEnum, HirFunction, HirInterface, HirInterfaceMethod, HirItemKind,
    HirParamMode, HirStruct, HirTypeAlias, HirTypeParam, NucleumRole,
};
use radix_module::semantic::{FuncSig, ParamType, SemanticParamMode, Type};
use std::collections::BTreeSet;

/// Portable export identity for annotation contracts on library file interfaces.
///
/// WHY: importers match framework contracts by provider/package/module/export, not
/// unit-local `DefId`. Callers pass package/library provenance when extracting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportIdentityContext {
    pub provider: String,
    pub package: Option<String>,
    pub module_path: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AnalyzedCallableContract {
    pub def_id: radix_module::hir::DefId,
    pub span: radix_module::lexer::Span,
    pub name: String,
    pub callable: radix_module::file_interface::InterfaceCallable,
    pub has_body: bool,
    pub nucleum_role: Option<NucleumRole>,
}

pub fn extract_callable_contracts(
    analysis: &AnalyzedModule,
    export_names: &[String],
    file_label: &str,
) -> Result<Vec<AnalyzedCallableContract>, Diagnostic> {
    let public_exports = export_names.iter().collect::<BTreeSet<_>>();
    analysis
        .hir
        .items
        .iter()
        .filter_map(|item| {
            let HirItemKind::Function(function) = &item.kind else {
                return None;
            };
            let name = analysis.interner.resolve(function.name).to_owned();
            if !public_exports.contains(&name) {
                return None;
            }
            Some(
                reject_signature_only_fragment(function, &name, file_label).and_then(|()| {
                    snapshot_function(function, &item.annotations, analysis, file_label, None).map(
                        |callable| AnalyzedCallableContract {
                            def_id: item.def_id,
                            span: item.span,
                            name,
                            callable,
                            has_body: function.body.is_some(),
                            nucleum_role: function.nucleum_role(),
                        },
                    )
                }),
            )
        })
        .collect()
}

pub fn extract_file_interface_with_identity(
    analysis: &AnalyzedModule,
    export_names: &[String],
    file_label: &str,
    export_identity: Option<&ExportIdentityContext>,
) -> Result<FileInterface, Diagnostic> {
    let public_exports = export_names.iter().collect::<BTreeSet<_>>();
    let mut interface = FileInterface::new();
    if let Some(ctx) = export_identity {
        interface.identity = Some(InterfaceLibraryIdentity {
            provider: ctx.provider.clone(),
            package: ctx.package.clone(),
            module_path: ctx.module_path.clone(),
        });
    }

    for item in &analysis.hir.items {
        let Some(name) = hir_item_name(&item.kind, analysis) else {
            continue;
        };
        if !public_exports.contains(&name) {
            continue;
        }

        let kind = match &item.kind {
            HirItemKind::Function(func) => {
                reject_signature_only_fragment(func, &name, file_label)?;
                FileExportKind::Function(snapshot_function(
                    func,
                    &item.annotations,
                    analysis,
                    file_label,
                    export_identity,
                )?)
            }
            HirItemKind::TypeAlias(alias) => FileExportKind::TypeAlias(snapshot_alias(
                alias,
                analysis,
                file_label,
                export_identity,
            )?),
            HirItemKind::Struct(strukt) => FileExportKind::Struct(snapshot_struct(
                strukt,
                analysis,
                file_label,
                &name,
                item.def_id,
                export_identity,
            )?),
            HirItemKind::Enum(enm) => {
                FileExportKind::Enum(snapshot_enum(enm, analysis, file_label, export_identity)?)
            }
            HirItemKind::Interface(interface_decl) => FileExportKind::Interface(
                snapshot_interface(interface_decl, analysis, file_label, &name, export_identity)?,
            ),
            HirItemKind::Constant(konst) => FileExportKind::Const(snapshot_const(
                konst,
                analysis,
                file_label,
                &name,
                export_identity,
            )?),
            HirItemKind::Schema(_) => continue,
            HirItemKind::Import(_) => continue,
        };

        interface.insert(FileExport { name, kind });
    }

    // `@ radix backward` companions are compiler-generated synthetic functions
    // with no HIR item, so they never appear in the loop above. Their typed
    // signature is attached to the companion resolver symbol by radix's
    // companion-signature pass; snapshot it into the interface so consumers
    // can call the companion across the import boundary (SEM004).
    for (_, backward) in analysis.radix_lanes.iter_backward() {
        let name = analysis
            .interner
            .resolve(backward.companion_name)
            .to_owned();
        if !public_exports.contains(&name) {
            continue;
        }
        let Some(symbol) = analysis.resolver.get_symbol(backward.companion_def_id) else {
            continue;
        };
        let Some(ty) = symbol.ty else {
            continue;
        };
        let Type::Func(sig) = analysis.types.get(ty) else {
            continue;
        };
        let callable = snapshot_interface_callable_with_identity(
            sig,
            &analysis.types,
            &analysis.interner,
            &analysis.resolver,
            export_identity.map(identity_for).as_ref(),
        )
        .map_err(|err| interface_error(file_label, &name, err))?;
        interface.insert(FileExport {
            name,
            kind: FileExportKind::Function(callable),
        });
    }

    Ok(interface)
}

fn snapshot_alias(
    alias: &HirTypeAlias,
    analysis: &AnalyzedModule,
    file_label: &str,
    export_identity: Option<&ExportIdentityContext>,
) -> Result<InterfaceAliasExport, Diagnostic> {
    let name = analysis.interner.resolve(alias.name).to_owned();
    Ok(InterfaceAliasExport {
        name: name.clone(),
        params: interface_generic_params(&alias.type_params, &analysis.interner),
        body: snapshot_interface_type_with_identity(
            alias.ty,
            &analysis.types,
            &analysis.interner,
            &analysis.resolver,
            export_identity.map(identity_for).as_ref(),
        )
        .map_err(|err| interface_error(file_label, &name, err))?,
    })
}

/// Ordered `{name, kind}` declaration parameters for an exported type.
fn interface_generic_params(
    params: &[HirTypeParam],
    interner: &radix_module::lexer::Interner,
) -> Vec<InterfaceGenericParam> {
    params
        .iter()
        .map(|param| InterfaceGenericParam {
            name: interner.resolve(param.name).to_owned(),
            kind: param.kind,
        })
        .collect()
}

fn snapshot_interface(
    interface: &HirInterface,
    analysis: &AnalyzedModule,
    file_label: &str,
    name: &str,
    export_identity: Option<&ExportIdentityContext>,
) -> Result<InterfaceNominalExport, Diagnostic> {
    let methods = interface
        .methods
        .iter()
        .map(|method| {
            Ok(InterfaceMethodExport {
                name: analysis.interner.resolve(method.name).to_owned(),
                callable: snapshot_interface_method(
                    method,
                    analysis,
                    file_label,
                    name,
                    export_identity,
                )?,
            })
        })
        .collect::<Result<Vec<_>, Diagnostic>>()?;

    Ok(InterfaceNominalExport {
        name: analysis.interner.resolve(interface.name).to_owned(),
        params: interface_generic_params(&interface.type_params, &analysis.interner),
        methods,
        variants: Vec::new(),
        shared_fields: Vec::new(),
    })
}

/// Snapshot an exported enum: name + variant declarations (name + optional
/// payload field types). Variants ride the file interface so consumers can
/// reference and construct library enum variants as values (codex-gap S1 U2,
/// operator ruling O1).
fn snapshot_enum(
    enm: &HirEnum,
    analysis: &AnalyzedModule,
    file_label: &str,
    export_identity: Option<&ExportIdentityContext>,
) -> Result<InterfaceNominalExport, Diagnostic> {
    let name = analysis.interner.resolve(enm.name).to_owned();
    let variants = enm
        .variants
        .iter()
        .map(|variant| {
            let fields = variant
                .fields
                .iter()
                .map(|field| {
                    snapshot_interface_type_with_identity(
                        field.ty,
                        &analysis.types,
                        &analysis.interner,
                        &analysis.resolver,
                        export_identity.map(identity_for).as_ref(),
                    )
                    .map(|ty| InterfaceStructField {
                        name: analysis.interner.resolve(field.name).to_owned(),
                        ty,
                        optional: false,
                        required: true,
                        default: None,
                    })
                    .map_err(|err| {
                        interface_error(file_label, &variant_name(analysis, variant), err)
                    })
                })
                .collect::<Result<Vec<_>, Diagnostic>>()?;
            Ok(InterfaceEnumVariant {
                name: analysis.interner.resolve(variant.name).to_owned(),
                fields,
            })
        })
        .collect::<Result<Vec<_>, Diagnostic>>()?;
    let shared_fields = enm
        .commune_fields
        .iter()
        .map(|field| {
            snapshot_interface_type_with_identity(
                field.ty,
                &analysis.types,
                &analysis.interner,
                &analysis.resolver,
                export_identity.map(identity_for).as_ref(),
            )
            .map(|ty| InterfaceStructField {
                name: analysis.interner.resolve(field.name).to_owned(),
                ty,
                optional: false,
                required: true,
                default: None,
            })
            .map_err(|err| interface_error(file_label, &name, err))
        })
        .collect::<Result<Vec<_>, Diagnostic>>()?;
    Ok(InterfaceNominalExport {
        name,
        params: interface_generic_params(&enm.type_params, &analysis.interner),
        methods: Vec::new(),
        variants,
        shared_fields,
    })
}

fn variant_name(analysis: &AnalyzedModule, variant: &radix_module::hir::HirVariant) -> String {
    analysis.interner.resolve(variant.name).to_owned()
}

fn identity_for(ctx: &ExportIdentityContext) -> InterfaceLibraryIdentity {
    InterfaceLibraryIdentity {
        provider: ctx.provider.clone(),
        package: ctx.package.clone(),
        module_path: ctx.module_path.clone(),
    }
}

fn snapshot_interface_method(
    method: &HirInterfaceMethod,
    analysis: &AnalyzedModule,
    file_label: &str,
    interface_name: &str,
    export_identity: Option<&ExportIdentityContext>,
) -> Result<radix_module::file_interface::InterfaceCallable, Diagnostic> {
    let ret = method.ret_ty.unwrap_or_else(|| {
        analysis
            .types
            .primitive(radix_module::semantic::Primitive::Vacuum)
    });
    let sig = FuncSig {
        type_params: Vec::new(),
        params: method
            .params
            .iter()
            .map(|param| ParamType {
                ty: param.ty,
                mode: param_mode(param.mode),
                optional: param.optional,
            })
            .collect(),
        ret,
        err: method.err_ty,
        is_async: false,
        is_generator: false,
    };
    snapshot_interface_callable_with_identity(
        &sig,
        &analysis.types,
        &analysis.interner,
        &analysis.resolver,
        export_identity.map(identity_for).as_ref(),
    )
    .map_err(|err| interface_error(file_label, interface_name, err))
}

fn snapshot_struct(
    strukt: &HirStruct,
    analysis: &AnalyzedModule,
    file_label: &str,
    name: &str,
    def_id: DefId,
    export_identity: Option<&ExportIdentityContext>,
) -> Result<InterfaceStructExport, Diagnostic> {
    let fields = strukt
        .fields
        .iter()
        .map(|field| {
            Ok(InterfaceStructField {
                name: analysis.interner.resolve(field.name).to_owned(),
                ty: snapshot_interface_type_with_identity(
                    field.ty,
                    &analysis.types,
                    &analysis.interner,
                    &analysis.resolver,
                    export_identity.map(identity_for).as_ref(),
                )
                .map_err(|err| interface_error(file_label, name, err))?,
                optional: field.sponte,
                required: !field.sponte && field.init.is_none(),
                default: field.init.clone(),
            })
        })
        .collect::<Result<Vec<_>, Diagnostic>>()?;

    let annotation_contract = annotation_contract_export(analysis, def_id, name, export_identity);

    let methods = strukt
        .methods
        .iter()
        .map(|method| {
            Ok(InterfaceMethodExport {
                name: analysis.interner.resolve(method.func.name).to_owned(),
                callable: snapshot_struct_method(
                    method,
                    analysis,
                    file_label,
                    name,
                    export_identity,
                )?,
            })
        })
        .collect::<Result<Vec<_>, Diagnostic>>()?;

    Ok(InterfaceStructExport {
        name: analysis.interner.resolve(strukt.name).to_owned(),
        params: interface_generic_params(&strukt.type_params, &analysis.interner),
        fields,
        methods,
        annotation_contract,
    })
}

fn snapshot_struct_method(
    method: &radix_module::hir::HirMethod,
    analysis: &AnalyzedModule,
    file_label: &str,
    struct_name: &str,
    export_identity: Option<&ExportIdentityContext>,
) -> Result<radix_module::file_interface::InterfaceCallable, Diagnostic> {
    let ret = method.func.ret_ty.unwrap_or_else(|| {
        analysis
            .types
            .primitive(radix_module::semantic::Primitive::Vacuum)
    });
    let sig = FuncSig {
        type_params: HirTypeParam::descriptors(&method.func.type_params),
        params: method
            .func
            .params
            .iter()
            .map(|param| ParamType {
                ty: param.ty,
                mode: param_mode(param.mode),
                optional: param.optional,
            })
            .collect(),
        ret,
        err: method.func.err_ty,
        is_async: method.func.is_async,
        is_generator: method.func.is_generator,
    };
    snapshot_interface_callable_with_identity(
        &sig,
        &analysis.types,
        &analysis.interner,
        &analysis.resolver,
        export_identity.map(identity_for).as_ref(),
    )
    .map_err(|err| interface_error(file_label, struct_name, err))
}

fn annotation_contract_export(
    analysis: &AnalyzedModule,
    def_id: DefId,
    export_name: &str,
    export_identity: Option<&ExportIdentityContext>,
) -> Option<InterfaceAnnotationContract> {
    let contract = analysis
        .annotation_contracts
        .registry
        .get(def_id)
        .or_else(|| {
            analysis
                .annotation_contracts
                .registry
                .iter()
                .find(|contract| analysis.interner.resolve(contract.name) == export_name)
        })?;

    let fields = contract
        .fields
        .iter()
        .map(|field| InterfaceAnnotationContractField {
            name: analysis.interner.resolve(field.name).to_owned(),
            ty: field.ty.as_str().to_owned(),
            optional: field.optional,
        })
        .collect();

    let qualified_identity = export_identity.map(|ctx| InterfaceQualifiedIdentity {
        provider: ctx.provider.clone(),
        package: ctx.package.clone(),
        module_path: ctx.module_path.clone(),
        export_name: export_name.to_owned(),
    });

    Some(InterfaceAnnotationContract {
        target: contract.target.as_str().to_owned(),
        fields,
        qualified_identity,
    })
}

fn snapshot_function(
    func: &HirFunction,
    annotations: &[radix_module::hir::HirAnnotation],
    analysis: &AnalyzedModule,
    file_label: &str,
    export_identity: Option<&ExportIdentityContext>,
) -> Result<radix_module::file_interface::InterfaceCallable, Diagnostic> {
    let name = analysis.interner.resolve(func.name);
    let ret = func.ret_ty.ok_or_else(|| {
        crate::package_diagnostic_error(format!(
            "public export `{name}` in `{file_label}` does not have a resolved return type"
        ))
        .with_phase(DiagnosticPhase::Analysis)
        .with_file(file_label.to_owned())
    })?;
    let sig = FuncSig {
        type_params: HirTypeParam::descriptors(&func.type_params),
        params: func
            .params
            .iter()
            .map(|param| ParamType {
                ty: param.ty,
                mode: param_mode(param.mode),
                optional: param.optional,
            })
            .collect(),
        ret,
        err: func.err_ty,
        is_async: func.is_async,
        is_generator: func.is_generator,
    };
    let mut callable = snapshot_interface_callable_with_identity(
        &sig,
        &analysis.types,
        &analysis.interner,
        &analysis.resolver,
        export_identity.map(identity_for).as_ref(),
    )
    .map_err(|err| interface_error(file_label, name, err))?;
    // Library-cache parity (KPC-WIRE): the module-side import link attaches
    // the v2 device contract at its export-surface builder; this snapshot is
    // the library/package interface source, and without the same fact an
    // exported `@ kernel`/`@ nucleum` declaration arrives at the package seam
    // with `device: None` and its role can never cross the import boundary.
    if let Some(contract) = device_contract_from_hir(func, annotations, &analysis.interner) {
        callable.device = Some(contract);
    }
    Ok(callable)
}

/// v2 device contract for one exported HIR function declaration, mirroring
/// the module-side `device_contract_from_stmt` convention (DFV2-3): the
/// nucleum role bit sets the role, an explicit `@ publica`/`@ public`
/// annotation sets launch visibility (an unmarked device function is a
/// module-internal helper), and a missing body is an explicit
/// [`InterfaceBodyChannel::Missing`] — never a silent signature-only export.
fn device_contract_from_hir(
    func: &HirFunction,
    annotations: &[radix_module::hir::HirAnnotation],
    interner: &radix_module::lexer::Interner,
) -> Option<InterfaceDeviceContract> {
    let role = match func.nucleum_role()? {
        NucleumRole::Entry => InterfaceDeviceRole::Kernel,
        NucleumRole::Fragment => InterfaceDeviceRole::Fragment,
    };
    let visibility = if annotations
        .iter()
        .any(|annotation| interner.resolve(annotation.family) == "publica")
    {
        InterfaceDeviceVisibility::Public
    } else {
        InterfaceDeviceVisibility::Private
    };
    let body = if func.body.is_some() {
        InterfaceBodyChannel::Source
    } else {
        InterfaceBodyChannel::Missing
    };
    Some(InterfaceDeviceContract {
        role,
        visibility,
        body,
    })
}

fn snapshot_const(
    konst: &HirConst,
    analysis: &AnalyzedModule,
    file_label: &str,
    name: &str,
    export_identity: Option<&ExportIdentityContext>,
) -> Result<radix_module::file_interface::InterfaceTypeSnapshot, Diagnostic> {
    let ty = konst.ty.or(konst.value.ty).ok_or_else(|| {
        crate::package_diagnostic_error(format!(
            "public export `{name}` in `{file_label}` does not have a resolved constant type"
        ))
        .with_phase(DiagnosticPhase::Analysis)
        .with_file(file_label.to_owned())
    })?;
    snapshot_interface_type_with_identity(
        ty,
        &analysis.types,
        &analysis.interner,
        &analysis.resolver,
        export_identity.map(identity_for).as_ref(),
    )
    .map_err(|err| interface_error(file_label, name, err))
}

fn param_mode(mode: HirParamMode) -> SemanticParamMode {
    match mode {
        HirParamMode::Owned => SemanticParamMode::Owned,
        HirParamMode::De => SemanticParamMode::Ref,
        HirParamMode::In => SemanticParamMode::MutRef,
        HirParamMode::Ex => SemanticParamMode::Move,
    }
}

fn hir_item_name(item: &HirItemKind, analysis: &AnalyzedModule) -> Option<String> {
    let name = match item {
        HirItemKind::Function(func) => analysis.interner.resolve(func.name),
        HirItemKind::Struct(strukt) => analysis.interner.resolve(strukt.name),
        HirItemKind::Enum(enm) => analysis.interner.resolve(enm.name),
        HirItemKind::Interface(interface) => analysis.interner.resolve(interface.name),
        HirItemKind::TypeAlias(alias) => analysis.interner.resolve(alias.name),
        HirItemKind::Schema(_) => return None,
        HirItemKind::Constant(konst) => analysis.interner.resolve(konst.name),
        HirItemKind::Import(_) => return None,
    };
    Some(name.to_owned())
}

fn reject_signature_only_fragment(
    function: &HirFunction,
    name: &str,
    file_label: &str,
) -> Result<(), Diagnostic> {
    if function.nucleum_role() == Some(NucleumRole::Fragment) && function.body.is_none() {
        return Err(crate::package_diagnostic_error(format!(
            "signature-only nucleum fragment `{name}` cannot be imported"
        ))
        .with_phase(DiagnosticPhase::Analysis)
        .with_file(file_label.to_owned())
        .with_arg("issue", "nucleum_fragment_effect")
        .with_arg("reason", "missing_body")
        .with_arg("site", "import"));
    }
    Ok(())
}

fn interface_error(file_label: &str, export_name: &str, err: FileInterfaceError) -> Diagnostic {
    // Uncoded on purpose (P2): the standing renderer contract renders prose
    // for coded non-Codegen diagnostics only through locale-pack templates,
    // and no pack carries a PKG001 template — a coded diagnostic here renders
    // as an empty `error[PKG001]: <file>` body. The `issue` arg keeps the
    // failure machine-identifiable without a code.
    Diagnostic::error(format!(
        "public export `{export_name}` in `{file_label}` cannot be represented in a file interface: {err:?}"
    ))
    .with_phase(DiagnosticPhase::Analysis)
    .with_file(file_label.to_owned())
    .with_arg("issue", "interface_export_unrepresentable")
    .with_arg("export", export_name.to_owned())
    .with_arg("reason", format!("{err:?}"))
}

#[cfg(test)]
#[path = "file_interface_test.rs"]
mod tests;

#[cfg(test)]
#[path = "file_interface_s0r1_test.rs"]
mod file_interface_s0r1_test;
