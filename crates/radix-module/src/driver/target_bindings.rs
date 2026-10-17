//! Opaque target-binding maps carried by [`Config`](super::Config) and their
//! resolution to bound-call keys for the WGSL graphics path.
//!
//! The maps are already-resolved route-to-spelling text. This module reads no
//! manifest and names no library vocabulary: a route is the opaque string
//! `<import specifier>.<name>`, matched against the provider calls a lowered
//! stage body actually contains, and a spelling is emitted verbatim by the
//! leaf emitter. A call with no entry stays unbound and fails closed there.
//!
//! A type route is the same opaque string, `<import specifier>.<exported
//! name>`, matched against the types the imported module namespaces export;
//! the resolved key is the exported type's `DefId`, which is also what a stage
//! parameter's `Type::Struct` carries.

use std::collections::BTreeMap;

/// Route-to-spelling maps handed to the compiler for one target.
///
/// `functions` keys a bound function call by `<import specifier>.<name>`
/// (for example `provider:module.name`); `types` keys a bound type by the same
/// route shape and is read by stage-parameter admission, not call resolution.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TargetBindings {
    /// Function route to opaque target spelling.
    pub functions: BTreeMap<String, String>,
    /// Type route to opaque target spelling.
    pub types: BTreeMap<String, String>,
    /// Annotation-contract route to opaque target spelling (spike: stage
    /// words such as `vertex` / `fragment`, or an IO attribute such as
    /// `builtin(position)`). Read only by the WGSL stage path.
    pub annotations: BTreeMap<String, String>,
}

impl TargetBindings {
    /// True when no route is bound.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.functions.is_empty() && self.types.is_empty() && self.annotations.is_empty()
    }
}

/// Build the bound-call table for the functions of one lowered program.
///
/// A provider runtime call arrives with `module = [import specifier, import
/// alias]` and the called name; its route is `<specifier>.<name>`. A same-module
/// `Call` callee has no library route and is never bound here.
#[cfg(feature = "mir-wgsl")]
#[must_use]
pub(super) fn wgsl_bound_call_table(
    bindings: &TargetBindings,
    program: &crate::mir::MirProgram,
    interner: &crate::lexer::Interner,
) -> crate::mir::WgslBoundCallTable {
    use crate::mir::{MirIntrinsic, MirRuntimeCall, MirStatementKind, WgslCalleeKey};

    let mut table = crate::mir::WgslBoundCallTable::new();
    if bindings.functions.is_empty() {
        return table;
    }
    let statements = program
        .functions
        .iter()
        .flat_map(|function| &function.blocks)
        .flat_map(|block| &block.statements);
    for statement in statements {
        let MirStatementKind::RuntimeCall {
            call: MirRuntimeCall { intrinsic: MirIntrinsic::Provider(provider), .. },
            ..
        } = &statement.kind
        else {
            continue;
        };
        let Some(specifier) = provider.module.first() else {
            continue;
        };
        let route = format!("{}.{}", interner.resolve(*specifier), interner.resolve(provider.name));
        if let Some(spelling) = bindings.functions.get(&route) {
            table.insert(
                WgslCalleeKey::Provider { module: provider.module.clone(), name: provider.name },
                spelling.clone(),
            );
        }
    }
    table
}

/// A bound type resolved to its identity: the opaque target spelling and the
/// name the library declared the type with.
#[cfg(feature = "mir-wgsl")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct BoundType {
    /// Opaque target spelling, emitted verbatim by the leaf emitter.
    pub spelling: String,
    /// The type's exported name as the library declared it.
    pub type_name: String,
}

/// Type identity (`DefId` of the imported nominal) to its bound type.
#[cfg(feature = "mir-wgsl")]
pub(super) type BoundTypeTable = std::collections::HashMap<crate::hir::DefId, BoundType>;

/// Resolve `bindings.types` routes to the imported nominals' identities.
///
/// A route is `<import specifier>.<exported name>`. The import's namespace
/// (alias or name) is asked for its exported types; each export whose route
/// has a `types` entry is keyed by the export's `DefId`. A route naming no
/// export of any import in the unit binds nothing.
#[cfg(feature = "mir-wgsl")]
#[must_use]
pub(super) fn wgsl_bound_type_table(
    bindings: &TargetBindings,
    hir: &crate::hir::HirModule,
    resolver: &crate::semantic::Resolver,
    interner: &crate::lexer::Interner,
) -> BoundTypeTable {
    let mut table = BoundTypeTable::new();
    if bindings.types.is_empty() {
        return table;
    }
    for item in &hir.items {
        let crate::hir::HirItemKind::Import(import) = &item.kind else {
            continue;
        };
        let specifier = interner.resolve(import.path);
        for import_item in &import.items {
            let namespace = import_item.alias.unwrap_or(import_item.name);
            for export in resolver.imported_file_type_exports(namespace) {
                let type_name = interner.resolve(export.member);
                let Some(spelling) = bindings.types.get(&format!("{specifier}.{type_name}")) else {
                    continue;
                };
                table.insert(
                    export.def_id,
                    BoundType { spelling: spelling.clone(), type_name: type_name.to_owned() },
                );
            }
        }
    }
    table
}

/// Stage and IO facts resolved from manifest-bound annotation contracts.
///
/// SPIKE (shader-annotation-stages): the compiler core names no stage word.
/// A function item carries a stage only when one of its annotations resolves
/// (through the library registry) to a contract whose route the target
/// manifest binds to `vertex` or `fragment`; a struct field carries an IO
/// attribute only when a contract bound to `builtin(<name>)` is applied to it.
#[cfg(feature = "mir-wgsl")]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct WgslStageBindings {
    /// Function item `DefId` to its manifest-bound stage.
    pub stages: std::collections::HashMap<crate::hir::DefId, crate::mir::MirKernelShaderStage>,
    /// `(struct DefId, field name)` to the WGSL builtin name (`position`).
    pub builtin_fields: std::collections::HashMap<(crate::hir::DefId, crate::lexer::Symbol), String>,
}

/// Contract identity (`DefId` of the imported class) to its library route
/// `<import specifier>.<exported name>`, resolved the same way bound types are.
#[cfg(feature = "mir-wgsl")]
fn contract_routes(
    hir: &crate::hir::HirModule,
    resolver: &crate::semantic::Resolver,
    interner: &crate::lexer::Interner,
) -> std::collections::HashMap<crate::hir::DefId, String> {
    let mut routes = std::collections::HashMap::new();
    for item in &hir.items {
        let crate::hir::HirItemKind::Import(import) = &item.kind else {
            continue;
        };
        let specifier = interner.resolve(import.path);
        for import_item in &import.items {
            let namespace = import_item.alias.unwrap_or(import_item.name);
            for export in resolver.imported_file_type_exports(namespace) {
                routes.insert(export.def_id, format!("{specifier}.{}", interner.resolve(export.member)));
            }
        }
    }
    routes
}

/// Resolve every manifest-bound annotation in `hir` to stage / IO facts.
///
/// # Errors
/// A function annotation bound to an unknown spelling, or a field annotation
/// bound to anything but `builtin(<name>)`, is a named error (fail closed).
#[cfg(feature = "mir-wgsl")]
pub(super) fn wgsl_stage_bindings(
    bindings: &TargetBindings,
    hir: &crate::hir::HirModule,
    resolver: &crate::semantic::Resolver,
    interner: &crate::lexer::Interner,
) -> Result<WgslStageBindings, String> {
    let mut out = WgslStageBindings::default();
    if bindings.annotations.is_empty() {
        return Ok(out);
    }
    let routes = contract_routes(hir, resolver, interner);
    let bound = |annotation: &crate::hir::HirAnnotation| -> Option<(String, String)> {
        let route = routes.get(&annotation.contract_id?)?.clone();
        let spelling = bindings.annotations.get(&route)?.clone();
        Some((route, spelling))
    };
    for item in &hir.items {
        match &item.kind {
            crate::hir::HirItemKind::Function(_) => {
                for annotation in &item.annotations {
                    let Some((route, spelling)) = bound(annotation) else { continue };
                    let stage = match spelling.as_str() {
                        "vertex" => crate::mir::MirKernelShaderStage::Vertex,
                        "fragment" => crate::mir::MirKernelShaderStage::Fragment,
                        other => return Err(format!("annotation `{route}` is bound to `{other}`, which is not a function stage")),
                    };
                    if out.stages.insert(item.def_id, stage).is_some_and(|prior| prior != stage) {
                        return Err(format!("conflicting stage annotations on one function (`{route}`)"));
                    }
                }
            }
            crate::hir::HirItemKind::Struct(strukt) => {
                for field in &strukt.fields {
                    for annotation in &field.annotations {
                        let Some((route, spelling)) = bound(annotation) else { continue };
                        let Some(name) = spelling.strip_prefix("builtin(").and_then(|rest| rest.strip_suffix(')')) else {
                            return Err(format!("annotation `{route}` is bound to `{spelling}`, which is not a field attribute"));
                        };
                        out.builtin_fields.insert((item.def_id, field.name), name.to_owned());
                    }
                }
            }
            _ => {}
        }
    }
    Ok(out)
}
