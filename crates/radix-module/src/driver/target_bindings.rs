//! Opaque target-binding maps carried by [`Config`](super::Config) and their
//! resolution to bound-call keys for the WGSL graphics path.
//!
//! The maps are already-resolved route-to-spelling text. This module reads no
//! manifest and names no library vocabulary: a route is the opaque string
//! `<import specifier>.<name>`, matched against the provider calls a lowered
//! stage body actually contains, and a spelling is emitted verbatim by the
//! leaf emitter. A call with no entry stays unbound and fails closed there.

use std::collections::BTreeMap;

/// Route-to-spelling maps handed to the compiler for one target.
///
/// `functions` keys a bound function call by `<import specifier>.<name>`
/// (for example `provider:module.name`); `types` is carried for the type
/// binding unit and is not read by call resolution.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TargetBindings {
    /// Function route to opaque target spelling.
    pub functions: BTreeMap<String, String>,
    /// Type route to opaque target spelling.
    pub types: BTreeMap<String, String>,
}

impl TargetBindings {
    /// True when no route is bound.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.functions.is_empty() && self.types.is_empty()
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
