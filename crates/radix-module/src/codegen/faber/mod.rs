//! Faber HIR re-emission backend, re-exported from foundation leaf
//! `radix-hir-faber`.

pub use radix_hir_faber::{FaberCodegen, FaberOutput, empty_presentation};

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
#[path = "compile_time_local_roundtrip_test.rs"]
mod compile_time_local_roundtrip_test;

#[cfg(test)]
#[path = "ranged_policy_roundtrip_test.rs"]
mod ranged_policy_roundtrip_test;

#[cfg(test)]
#[path = "ranged_list_roundtrip_test.rs"]
mod ranged_list_roundtrip_test;

#[cfg(test)]
#[path = "capacity_roundtrip_test.rs"]
mod capacity_roundtrip_test;

#[cfg(test)]
#[path = "vector_width_roundtrip_test.rs"]
mod vector_width_roundtrip_test;

#[cfg(test)]
#[path = "module_constant_roundtrip_test.rs"]
mod module_constant_roundtrip_test;

#[cfg(test)]
#[path = "corpus_reemit_gate_test.rs"]
mod corpus_reemit_gate_test;
#[cfg(test)]
#[path = "field_modifier_roundtrip_test.rs"]
mod field_modifier_roundtrip_test;
#[cfg(test)]
#[path = "lossless_reemit_test.rs"]
mod lossless_reemit_test;
#[cfg(test)]
#[path = "member_visibility_roundtrip_test.rs"]
mod member_visibility_roundtrip_test;

#[cfg(test)]
#[path = "region_roundtrip_test.rs"]
mod region_roundtrip_test;

#[cfg(test)]
#[path = "conditional_roundtrip_test.rs"]
mod conditional_roundtrip_test;

#[cfg(test)]
#[path = "frame_names_test.rs"]
mod frame_names_test;

#[cfg(test)]
#[path = "ad_annotation_roundtrip_test.rs"]
mod ad_annotation_roundtrip_test;

#[cfg(test)]
#[path = "return_thread_roundtrip_test.rs"]
mod return_thread_roundtrip_test;

use crate::lexer::TokenKind;
use crate::locale;
use radix_hir_faber::KeywordSurface as LeafKeywordSurface;

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

    fn type_surface(&self, kind: radix_semantic::locale::TypeBindingKind) -> &str {
        locale::KeywordSurface::type_surface(&self.0, kind)
    }
}

#[cfg(test)]
#[path = "reemit_fidelity_test.rs"]
mod reemit_fidelity_test;
