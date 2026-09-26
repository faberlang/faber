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
    /// Compiler-owned conversation family (DECISIONS D6.16).
    Frame(FrameFamily),
}

/// Compiler-owned conversation-family types whose spelling projects through
/// the target pack (`sermo`, `scrinium`, and the `meus`/`tuus` views).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FrameFamily {
    Sermo,
    Scrinium,
    Meus,
    Tuus,
}

impl FrameFamily {
    /// Identity for a builtin struct's canonical Latin name.
    #[must_use]
    pub fn from_latin(text: &str) -> Option<Self> {
        match text {
            "sermo" => Some(Self::Sermo),
            "scrinium" => Some(Self::Scrinium),
            "meus" => Some(Self::Meus),
            "tuus" => Some(Self::Tuus),
            _ => None,
        }
    }
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
