//! Top-level item selection for MIR lowering.
//!
//! MIR lowering is currently function-body centered: struct, enum, interface,
//! type-alias, and import declarations provide context maps, but they do not
//! produce standalone executable MIR items. This module keeps that policy
//! separate from expression and statement lowering so unsupported top-level
//! executable surfaces fail in a single, visible place.
//!
//! SUPPORTED SURFACE
//! =================
//! Functions lower into [`MirFunction`] values through [`MirLowerer`]. Structs,
//! enums, interfaces, type aliases, and imports are intentionally skipped here
//! because their data has already been harvested by context collection. Other
//! item kinds are reported as unsupported rather than silently discarded.

#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for lowering
use super::*;
use crate::hir::visit::HirVisitor;

/// Visitor that lowers the MIR-producing subset of HIR items.
///
/// The pass borrows whole-unit context collected before item lowering. It does
/// not discover semantic facts itself; it decides which HIR items are executable
/// MIR inputs and delegates supported functions to the main lowerer.
pub(super) struct ItemLoweringPass<'lowerer, 'unit, 'maps> {
    lowerer: &'lowerer mut MirLowerer<'unit>,
    body_ctx: &'maps BodyLoweringContext<'maps, 'unit>,
}

impl<'lowerer, 'unit, 'maps> ItemLoweringPass<'lowerer, 'unit, 'maps> {
    /// Create an item-lowering visitor for one whole-unit lowering run.
    pub(super) fn new(
        lowerer: &'lowerer mut MirLowerer<'unit>,
        body_ctx: &'maps BodyLoweringContext<'maps, 'unit>,
    ) -> Self {
        Self { lowerer, body_ctx }
    }

    /// Visit source items in HIR order and lower the MIR-producing subset.
    ///
    /// Source order becomes MIR function order for now, which keeps dumps stable
    /// and makes unsupported-item diagnostics deterministic.
    pub(super) fn lower_items(&mut self) {
        let items = &self.lowerer.unit.hir.items;
        for item in items {
            self.visit_item(item);
        }
    }
}

impl HirVisitor for ItemLoweringPass<'_, '_, '_> {
    fn visit_item(&mut self, item: &HirItem) {
        match &item.kind {
            HirItemKind::Function(function) => {
                self.lowerer.lower_function(item, function, self.body_ctx);
            }
            HirItemKind::Struct(strukt) => {
                for method in &strukt.methods {
                    self.lowerer
                        .lower_method(item, strukt, method, self.body_ctx);
                }
            }
            HirItemKind::Enum(_)
            | HirItemKind::Interface(_)
            | HirItemKind::TypeAlias(_)
            | HirItemKind::Schema(_)
            | HirItemKind::Constant(_)
            | HirItemKind::Import(_) => {}
        }
    }
}
