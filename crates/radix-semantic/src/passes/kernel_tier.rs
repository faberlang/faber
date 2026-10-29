//! The safe switch for kernel tiers (operator ruling 2026-10-28).
//!
//! `faber run --safe`, `faber build --safe` and `safe = true` in `faber.toml`
//! run every kernel at the default tier whatever its annotation says. The
//! switch can only lower a tier, so it removes the `tier` field from each
//! `@ nucleum` annotation; everything after this point (the requirement
//! report, MIR lowering, the emitters) then reads the default tier. It reads
//! no environment variable.

use radix_hir::{HirAnnotation, HirItemKind, HirModule};
use radix_lexer::Interner;

/// Lower every kernel in `hir` to the default tier.
pub fn deny_tiers(hir: &mut HirModule, interner: &Interner) {
    for item in &mut hir.items {
        match &mut item.kind {
            HirItemKind::Function(_) => strip_tier(&mut item.annotations, interner),
            HirItemKind::Struct(strukt) => {
                for method in &mut strukt.methods {
                    strip_tier(&mut method.annotations, interner);
                }
            }
            _ => {}
        }
    }
}

fn strip_tier(annotations: &mut [HirAnnotation], interner: &Interner) {
    for annotation in annotations {
        if interner.resolve(annotation.family) == "nucleum" {
            annotation
                .fields
                .retain(|field| interner.resolve(field.name) != "tier");
        }
    }
}
