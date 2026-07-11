use radix::diagnostics::{Diagnostic, DiagnosticPhase};
use radix::driver::AnalyzedUnit;
use radix::file_interface::{
    snapshot_interface_callable_with_resolver, snapshot_interface_type_with_resolver, FileExport,
    FileExportKind, FileInterface, FileInterfaceError, InterfaceAnnotationContract,
    InterfaceAnnotationContractField, InterfaceMethodExport, InterfaceNominalExport,
    InterfaceQualifiedIdentity, InterfaceStructExport, InterfaceStructField,
};
use radix::hir::{
    DefId, HirConst, HirFunction, HirInterface, HirInterfaceMethod, HirItemKind, HirParamMode,
    HirStruct, HirTypeParamConstraint,
};
use radix::semantic::{FuncSig, ParamMode, ParamType, TypeParamConstraint};
use std::collections::BTreeSet;

/// Portable export identity for annotation contracts on library file interfaces.
///
/// WHY: importers match framework contracts by provider/package/module/export, not
/// unit-local `DefId`. Callers pass package/library provenance when extracting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExportIdentityContext {
    pub provider: String,
    pub package: Option<String>,
    pub module_path: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AnalyzedCallableContract {
    pub(crate) def_id: radix::hir::DefId,
    pub(crate) span: radix::lexer::Span,
    pub(crate) name: String,
    pub(crate) callable: radix::file_interface::InterfaceCallable,
    pub(crate) has_body: bool,
}

#[allow(clippy::result_large_err)]
pub(crate) fn extract_callable_contracts(
    analysis: &AnalyzedUnit,
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
                snapshot_function(function, analysis, file_label).map(|callable| {
                    AnalyzedCallableContract {
                        def_id: item.def_id,
                        span: item.span,
                        name,
                        callable,
                        has_body: function.body.is_some(),
                    }
                }),
            )
        })
        .collect()
}

#[allow(clippy::result_large_err)]
#[allow(dead_code)] // retained for tests / callers without package identity
pub(crate) fn extract_file_interface(
    analysis: &AnalyzedUnit,
    export_names: &[String],
    file_label: &str,
) -> Result<FileInterface, Diagnostic> {
    extract_file_interface_with_identity(analysis, export_names, file_label, None)
}

#[allow(clippy::result_large_err)]
pub(crate) fn extract_file_interface_with_identity(
    analysis: &AnalyzedUnit,
    export_names: &[String],
    file_label: &str,
    export_identity: Option<&ExportIdentityContext>,
) -> Result<FileInterface, Diagnostic> {
    let public_exports = export_names.iter().collect::<BTreeSet<_>>();
    let mut interface = FileInterface::new();

    for item in &analysis.hir.items {
        let Some(name) = hir_item_name(&item.kind, analysis) else {
            continue;
        };
        if !public_exports.contains(&name) {
            continue;
        }

        let kind = match &item.kind {
            HirItemKind::Function(func) => {
                FileExportKind::Function(snapshot_function(func, analysis, file_label)?)
            }
            HirItemKind::TypeAlias(alias) => FileExportKind::TypeAlias(
                snapshot_interface_type_with_resolver(
                    alias.ty,
                    &analysis.types,
                    &analysis.interner,
                    &analysis.resolver,
                )
                .map_err(|err| interface_error(file_label, &name, err))?,
            ),
            HirItemKind::Struct(strukt) => FileExportKind::Struct(snapshot_struct(
                strukt,
                analysis,
                file_label,
                &name,
                item.def_id,
                export_identity,
            )?),
            HirItemKind::Enum(enm) => FileExportKind::Enum(InterfaceNominalExport {
                name: analysis.interner.resolve(enm.name).to_owned(),
                methods: Vec::new(),
            }),
            HirItemKind::Interface(interface_decl) => FileExportKind::Interface(
                snapshot_interface(interface_decl, analysis, file_label, &name)?,
            ),
            HirItemKind::Const(konst) => {
                FileExportKind::Const(snapshot_const(konst, analysis, file_label, &name)?)
            }
            HirItemKind::Import(_) => continue,
        };

        interface.insert(FileExport { name, kind });
    }

    Ok(interface)
}

#[allow(clippy::result_large_err)]
fn snapshot_interface(
    interface: &HirInterface,
    analysis: &AnalyzedUnit,
    file_label: &str,
    name: &str,
) -> Result<InterfaceNominalExport, Diagnostic> {
    let methods = interface
        .methods
        .iter()
        .map(|method| {
            Ok(InterfaceMethodExport {
                name: analysis.interner.resolve(method.name).to_owned(),
                callable: snapshot_interface_method(method, analysis, file_label, name)?,
            })
        })
        .collect::<Result<Vec<_>, Diagnostic>>()?;

    Ok(InterfaceNominalExport {
        name: analysis.interner.resolve(interface.name).to_owned(),
        methods,
    })
}

#[allow(clippy::result_large_err)]
fn snapshot_interface_method(
    method: &HirInterfaceMethod,
    analysis: &AnalyzedUnit,
    file_label: &str,
    interface_name: &str,
) -> Result<radix::file_interface::InterfaceCallable, Diagnostic> {
    let ret = method
        .ret_ty
        .unwrap_or_else(|| analysis.types.primitive(radix::semantic::Primitive::Vacuum));
    let sig = FuncSig {
        type_params: Vec::new(),
        type_param_constraints: Vec::new(),
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
    snapshot_interface_callable_with_resolver(
        &sig,
        &analysis.types,
        &analysis.interner,
        &analysis.resolver,
    )
    .map_err(|err| interface_error(file_label, interface_name, err))
}

#[allow(clippy::result_large_err)]
fn snapshot_struct(
    strukt: &HirStruct,
    analysis: &AnalyzedUnit,
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
                ty: snapshot_interface_type_with_resolver(
                    field.ty,
                    &analysis.types,
                    &analysis.interner,
                    &analysis.resolver,
                )
                .map_err(|err| interface_error(file_label, name, err))?,
                optional: field.sponte,
                required: !field.sponte && field.init.is_none(),
            })
        })
        .collect::<Result<Vec<_>, Diagnostic>>()?;

    let annotation_contract = annotation_contract_export(analysis, def_id, name, export_identity);

    Ok(InterfaceStructExport {
        name: analysis.interner.resolve(strukt.name).to_owned(),
        fields,
        annotation_contract,
    })
}

fn annotation_contract_export(
    analysis: &AnalyzedUnit,
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

#[allow(clippy::result_large_err)]
fn snapshot_function(
    func: &HirFunction,
    analysis: &AnalyzedUnit,
    file_label: &str,
) -> Result<radix::file_interface::InterfaceCallable, Diagnostic> {
    let name = analysis.interner.resolve(func.name);
    let ret = func.ret_ty.ok_or_else(|| {
        Diagnostic::error(format!(
            "public export `{name}` in `{file_label}` does not have a resolved return type"
        ))
        .with_phase(DiagnosticPhase::Analysis)
        .with_file(file_label.to_owned())
    })?;
    let sig = FuncSig {
        type_params: func.type_params.iter().map(|param| param.name).collect(),
        type_param_constraints: func
            .type_params
            .iter()
            .map(|param| type_param_constraint(&param.constraint))
            .collect(),
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
    snapshot_interface_callable_with_resolver(
        &sig,
        &analysis.types,
        &analysis.interner,
        &analysis.resolver,
    )
    .map_err(|err| interface_error(file_label, name, err))
}

#[allow(clippy::result_large_err)]
fn snapshot_const(
    konst: &HirConst,
    analysis: &AnalyzedUnit,
    file_label: &str,
    name: &str,
) -> Result<radix::file_interface::InterfaceTypeSnapshot, Diagnostic> {
    let ty = konst.ty.or(konst.value.ty).ok_or_else(|| {
        Diagnostic::error(format!(
            "public export `{name}` in `{file_label}` does not have a resolved constant type"
        ))
        .with_phase(DiagnosticPhase::Analysis)
        .with_file(file_label.to_owned())
    })?;
    snapshot_interface_type_with_resolver(
        ty,
        &analysis.types,
        &analysis.interner,
        &analysis.resolver,
    )
    .map_err(|err| interface_error(file_label, name, err))
}

fn type_param_constraint(constraint: &HirTypeParamConstraint) -> TypeParamConstraint {
    match constraint {
        HirTypeParamConstraint::Any => TypeParamConstraint::Any,
        HirTypeParamConstraint::OneOf(types) => TypeParamConstraint::OneOf(types.clone()),
    }
}

fn param_mode(mode: HirParamMode) -> ParamMode {
    match mode {
        HirParamMode::Owned => ParamMode::Owned,
        HirParamMode::De => ParamMode::Ref,
        HirParamMode::In => ParamMode::MutRef,
        HirParamMode::Ex => ParamMode::Move,
    }
}

fn hir_item_name(item: &HirItemKind, analysis: &AnalyzedUnit) -> Option<String> {
    let name = match item {
        HirItemKind::Function(func) => analysis.interner.resolve(func.name),
        HirItemKind::Struct(strukt) => analysis.interner.resolve(strukt.name),
        HirItemKind::Enum(enm) => analysis.interner.resolve(enm.name),
        HirItemKind::Interface(interface) => analysis.interner.resolve(interface.name),
        HirItemKind::TypeAlias(alias) => analysis.interner.resolve(alias.name),
        HirItemKind::Const(konst) => analysis.interner.resolve(konst.name),
        HirItemKind::Import(_) => return None,
    };
    Some(name.to_owned())
}

fn interface_error(file_label: &str, export_name: &str, err: FileInterfaceError) -> Diagnostic {
    Diagnostic::error(format!(
        "public export `{export_name}` in `{file_label}` cannot be represented in a file interface: {err:?}"
    ))
    .with_phase(DiagnosticPhase::Analysis)
    .with_file(file_label.to_owned())
}
