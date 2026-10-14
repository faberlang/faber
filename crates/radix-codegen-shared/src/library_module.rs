//! Library-module naming rule shared by TypeScript import emission and the
//! faber TypeScript product layer.
//!
//! A library module specifier (`tela:dom`, `triga:graph/object`) has one
//! emitted stem. The TypeScript emitter writes relative ESM imports from it,
//! and the product layer names emitted `.ts` files and its import map from
//! it. Both sides call these functions, so the product map can never drift
//! from what the compiler emits.

/// Dash-joined emitted module stem for a library module specifier
/// (`tela:dom` → `tela-dom`, `triga:graph/object` → `triga-graph-object`).
/// Returns `None` for non-library specifiers (relative paths, stdlib
/// `norma:` forms, kernel globs), which keep the historical named-import
/// emission.
#[must_use]
pub fn library_module_dash_name(path: &str) -> Option<String> {
    let (provider, module) = path.split_once(':')?;
    if provider.is_empty()
        || module.is_empty()
        || module.contains(':')
        || module.contains('*')
        || provider == "norma"
    {
        return None;
    }
    Some(format!("{provider}-{}", module.replace('/', "-")))
}

/// Deterministic relative ESM path for a library module specifier, built from
/// [`library_module_dash_name`] (`tela:dom` → `./tela-dom.js`,
/// `triga:graph/object` → `./triga-graph-object.js`). Returns `None` for
/// non-library specifiers (relative paths, stdlib `norma:` forms, kernel
/// globs), which keep the historical named-import emission.
#[must_use]
pub fn library_specifier_to_relative(path: &str) -> Option<String> {
    library_module_dash_name(path).map(|stem| format!("./{stem}.js"))
}

#[cfg(test)]
#[path = "library_module_test.rs"]
mod tests;
