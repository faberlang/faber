//! Keyword and type surface spelling for HIR→Faber emission.
//!
//! Package `radix` implements [`KeywordSurface`] for its reader-locale pack view.
//! The leaf does not depend on the full reader-pack machinery.

use radix_lexer::TokenKind;
use radix_types::{CollectionKind, Primitive, TensorFamilyKind};

/// Identity used when looking up localized type spellings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TypeBindingKind {
    Primitive(Primitive),
    Collection(CollectionKind),
    TensorFamily(TensorFamilyKind),
    ModularWord,
}

/// Borrowed keyword/type surface for canonical Faber emission.
pub trait KeywordSurface {
    /// Localized (or Latin-fallback) spelling for a keyword token kind.
    fn keyword(&self, kind: TokenKind) -> &str;

    /// Localized (or Latin-fallback) spelling for a contextual keyword identity.
    fn contextual_keyword(&self, latin_key: &'static str) -> &str {
        latin_key
    }

    /// Pack alias at `index` for a contextual keyword identity.
    ///
    /// Used when the canonical surface would capture a collected binding
    /// (`empty` vs a local named `empty`): emit an uncaptured alias instead.
    /// Default: no aliases.
    fn contextual_keyword_alias(&self, _latin_key: &'static str, _index: usize) -> Option<&str> {
        None
    }

    /// Whether template application (`"template"(args)`) is emitted as the
    /// postfix sugar rather than the expanded named `scriptum("template", …)`
    /// call. Canonical emission expands (default); reader-locale emission
    /// preserves the glyph shape while localizing surrounding keywords.
    fn preserve_template_application_sugar(&self) -> bool {
        false
    }

    /// Localized (or identity) surface spelling for a canonical intrinsic
    /// method name.
    ///
    /// HIR method symbols are canonical registry spellings; under a non-`la`
    /// reader pack those spellings are not re-parseable method names (the pack
    /// rejects the canonical Latin spelling in the wrong locale). Emission
    /// projects them back through the pack's intrinsic surface rows. `None`
    /// keeps the canonical spelling — identity for `la`/no pack and for names
    /// the pack does not own.
    fn intrinsic_surface(&self, _canonical: &str) -> Option<String> {
        None
    }

    /// Localized (or Latin-fallback) spelling for a type identity.
    fn type_surface(&self, kind: TypeBindingKind) -> &str;
}
