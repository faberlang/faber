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

    /// Whether template application (`"template"(args)`) is emitted as the
    /// postfix sugar rather than the expanded named `scriptum("template", …)`
    /// call. Canonical emission expands (default); reader-locale emission
    /// preserves the glyph shape while localizing surrounding keywords.
    fn preserve_template_application_sugar(&self) -> bool {
        false
    }

    /// Localized (or Latin-fallback) spelling for a type identity.
    fn type_surface(&self, kind: TypeBindingKind) -> &str;
}
