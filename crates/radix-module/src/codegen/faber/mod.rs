//! Faber HIR re-emission backend, re-exported from foundation leaf
//! `radix-hir-faber`.

pub use radix_hir_faber::{FaberCodegen, FaberOutput, empty_presentation};

pub mod compile;
mod meaning;
pub use compile::{FaberReemitResult, emit_from_analyzed, reemit_faber};

#[doc(hidden)]
pub mod test_gate;

#[cfg(test)]
#[path = "convert_regression_test.rs"]
mod convert_regression_test;

#[cfg(test)]
#[path = "canonical_test.rs"]
mod canonical_test;

#[cfg(test)]
#[path = "cape_roundtrip_test.rs"]
mod cape_roundtrip_test;

#[cfg(test)]
#[path = "conditional_roundtrip_test.rs"]
mod conditional_roundtrip_test;

#[cfg(test)]
#[path = "frame_names_test.rs"]
mod frame_names_test;

#[cfg(test)]
#[path = "ad_annotation_roundtrip_test.rs"]
mod ad_annotation_roundtrip_test;

use crate::lexer::TokenKind;
use crate::locale::{self, TypeBindingKind};
use radix_hir_faber::FrameFamily;
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
            LeafTypeBindingKind::Frame(frame) => TypeBindingKind::Frame(match frame {
                FrameFamily::Sermo => locale::FrameTypeKind::Sermo,
                FrameFamily::Scrinium => locale::FrameTypeKind::Scrinium,
                FrameFamily::Meus => locale::FrameTypeKind::Meus,
                FrameFamily::Tuus => locale::FrameTypeKind::Tuus,
            }),
        };
        locale::KeywordSurface::type_surface(&self.0, pack_kind)
    }
}

#[cfg(test)]
#[path = "reemit_fidelity_test.rs"]
mod reemit_fidelity_test;
