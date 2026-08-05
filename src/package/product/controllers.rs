use super::build::BrowserController;
use super::ts_rewrite::ts_module_file_name;
use super::{library_item_display_key, product_diag};
use super::{BTreeMap, Diagnostic, Path};

pub(super) fn discover_controllers(
    package: &super::super::AnalyzedPackage,
) -> Result<Vec<BrowserController>, Box<Diagnostic>> {
    let mut controllers = Vec::new();
    let mut selectors = BTreeMap::<String, String>::new();
    for unit in &package.units {
        let module = ts_module_file_name(unit);
        for item in &unit.analysis.hir.items {
            let radix::hir::HirItemKind::Function(function) = &item.kind else {
                continue;
            };
            let Some(selector) = web_controller_selector(unit, function) else {
                continue;
            };
            validate_selector(&selector, &unit.path)?;
            validate_controller_origin(unit, function)?;
            validate_controller_signature(unit, function)?;
            let name = unit.analysis.interner.resolve(function.name).to_owned();
            if let Some(existing) = selectors.insert(selector.clone(), name.clone()) {
                return Err(Box::new(
                    product_diag(format!(
                        "browser controllers `{existing}` and `{name}` both mount `{selector}`"
                    ))
                    .with_file(unit.path.display().to_string())
                    .with_arg("issue", "product_duplicate_mount")
                    .with_arg("selector", selector),
                ));
            }
            controllers.push(BrowserController {
                name: name.clone(),
                selector,
                module: format!("./{}", module.replace(".ts", ".js")),
                export: name,
            });
        }
    }
    if controllers.is_empty() {
        return Err(Box::new(
            product_diag("browser product declares no WebController functions")
                .with_file(package.spec.package_root.display().to_string())
                .with_arg("issue", "product_controller_missing"),
        ));
    }
    controllers.sort_by(|a, b| (&a.selector, &a.name).cmp(&(&b.selector, &b.name)));
    Ok(controllers)
}

fn web_controller_selector(
    unit: &super::super::AnalyzedPackageUnit,
    function: &radix::hir::HirFunction,
) -> Option<String> {
    function.annotations.iter().find_map(|annotation| {
        let contract_id = annotation.contract_id?;
        let contract = unit
            .analysis
            .annotation_contracts
            .registry
            .get(contract_id)?;
        if unit.analysis.interner.resolve(contract.name) != "WebController" {
            return None;
        }
        annotation.fields.iter().find_map(|field| {
            if unit.analysis.interner.resolve(field.name) != "selector" {
                return None;
            }
            match field.value {
                radix::hir::HirAnnotationValue::String(symbol) => {
                    Some(unit.analysis.interner.resolve(symbol).to_owned())
                }
                _ => None,
            }
        })
    })
}

/// Verify the WebController annotation contract originates from the `web`
/// package's `web` module — not a local shadowing definition.
fn validate_controller_origin(
    unit: &super::super::AnalyzedPackageUnit,
    function: &radix::hir::HirFunction,
) -> Result<(), Box<Diagnostic>> {
    for annotation in &function.annotations {
        let Some(contract_id) = annotation.contract_id else {
            continue;
        };
        let Some(contract) = unit.analysis.annotation_contracts.registry.get(contract_id) else {
            continue;
        };
        if unit.analysis.interner.resolve(contract.name) != "WebController" {
            continue;
        }
        let controller_name = unit.analysis.interner.resolve(function.name);
        match unit.analysis.libraries.items.get(&contract.def_id) {
            Some(item)
                if matches!(&item.identity.provider, radix::hir::LibraryProvider::Package(name) if name == "web")
                    && item.identity.module_path == ["web".to_owned()]
                    && item.exported_name == "WebController" =>
            {
                return Ok(());
            }
            Some(item) => {
                return Err(Box::new(
                    product_diag(format!(
                        "browser controller `{controller_name}` annotation `WebController` must originate from web:web; found `{}`",
                        library_item_display_key(item)
                    ))
                    .with_file(unit.path.display().to_string())
                    .with_arg("issue", "product_controller_unqualified_origin")
                    .with_arg("controller", controller_name.to_owned()),
                ));
            }
            None => {
                return Err(Box::new(
                    product_diag(format!(
                        "browser controller `{controller_name}` annotation `WebController` must be imported from web:web; local definitions are rejected"
                    ))
                    .with_file(unit.path.display().to_string())
                    .with_arg("issue", "product_controller_unqualified_origin")
                    .with_arg("controller", controller_name.to_owned()),
                ));
            }
        }
    }
    Ok(())
}

fn validate_selector(selector: &str, file: &Path) -> Result<(), Box<Diagnostic>> {
    let valid = !selector.is_empty()
        && !selector
            .chars()
            .any(|ch| ch.is_control() || ch.is_whitespace())
        && matches!(selector.as_bytes().first(), Some(b'#' | b'.' | b'['));
    if !valid {
        return Err(Box::new(
            product_diag(format!(
                "browser controller selector `{selector}` must be a static id, class, or attribute selector"
            ))
            .with_file(file.display().to_string())
            .with_arg("issue", "product_invalid_static_selector")
            .with_arg("selector", selector),
        ));
    }
    Ok(())
}

fn validate_controller_signature(
    unit: &super::super::AnalyzedPackageUnit,
    function: &radix::hir::HirFunction,
) -> Result<(), Box<Diagnostic>> {
    let name = unit.analysis.interner.resolve(function.name).to_owned();
    if function.params.len() != 1 {
        return Err(Box::new(
            product_diag(format!(
                "browser controller `{name}` must take exactly one DOM Scope parameter"
            ))
            .with_file(unit.path.display().to_string())
            .with_arg("issue", "product_invalid_controller_signature")
            .with_arg("controller", name),
        ));
    }
    if !param_is_dom_scope(unit, &function.params[0]) {
        return Err(Box::new(
            product_diag(format!(
                "browser controller `{name}` first parameter must be web:dom Scope"
            ))
            .with_file(unit.path.display().to_string())
            .with_arg("issue", "product_invalid_controller_signature")
            .with_arg("controller", name),
        ));
    }
    Ok(())
}

fn param_is_dom_scope(
    unit: &super::super::AnalyzedPackageUnit,
    param: &radix::hir::HirParam,
) -> bool {
    let radix::semantic::Type::Struct(def_id) = unit.analysis.types.get(param.ty) else {
        return false;
    };
    let symbol = match unit.analysis.resolver.get_symbol(*def_id) {
        Some(symbol) => symbol,
        None => return false,
    };
    if unit.analysis.interner.resolve(symbol.name) != "Scope" {
        return false;
    }
    // Provenance must originate from web:dom — reject local shadowing.
    matches!(
        unit.analysis.libraries.items.get(def_id),
        Some(item)
            if matches!(&item.identity.provider, radix::hir::LibraryProvider::Package(name) if name == "web")
                && item.identity.module_path == ["dom".to_owned()]
                && item.exported_name == "Scope"
    )
}
