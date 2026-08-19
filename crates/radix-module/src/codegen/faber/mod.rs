//! Canonical Faber backend — re-exported from foundation leaf `radix-hir-faber`.
//!
//! LEGACY(canonical): name predates reader locales — this is the HIR→Faber
//! re-emit used by `faber format --locale <X>` and `radix emit -t faber`.
//! Rename in a later sweep.

pub use radix_hir_faber::{emit, empty_presentation, FaberCodegen, FaberOutput};

use crate::lexer::TokenKind;
use crate::locale::{self, TypeBindingKind};
use radix_hir_faber::KeywordSurface as LeafKeywordSurface;
use radix_hir_faber::TypeBindingKind as LeafTypeBindingKind;

/// Bridge package reader-locale surfaces into the leaf keyword trait.
///
/// Newtype adapter: the trait and the locale type now live in different
/// crates, so the blanket impl became an orphan. Wrapping keeps the bridge
/// local without adding a crate edge.
pub struct LocaleKeywordBridge<'a>(pub locale::KeywordSurface<'a>);

impl LeafKeywordSurface for LocaleKeywordBridge<'_> {
    fn keyword(&self, kind: TokenKind) -> &str {
        locale::KeywordSurface::keyword(&self.0, kind)
    }

    fn contextual_keyword(&self, latin_key: &'static str) -> &str {
        locale::KeywordSurface::contextual_keyword(&self.0, latin_key)
    }

    fn preserve_template_application_sugar(&self) -> bool {
        locale::KeywordSurface::preserve_template_application_sugar(&self.0)
    }

    fn intrinsic_surface(&self, canonical: &str) -> Option<String> {
        locale::KeywordSurface::intrinsic_surface(&self.0, canonical).map(str::to_owned)
    }

    fn type_surface(&self, kind: LeafTypeBindingKind) -> &str {
        let pack_kind = match kind {
            LeafTypeBindingKind::Primitive(prim) => TypeBindingKind::Primitive(prim),
            LeafTypeBindingKind::Collection(collection) => TypeBindingKind::Collection(collection),
            LeafTypeBindingKind::TensorFamily(family) => TypeBindingKind::TensorFamily(family),
            LeafTypeBindingKind::ModularWord => TypeBindingKind::ModularWord,
        };
        locale::KeywordSurface::type_surface(&self.0, pack_kind)
    }
}
