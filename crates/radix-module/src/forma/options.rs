//! Format options for forma (author vs canonical, check mode, etc.).
//!
//! Consumed by `compile.rs` to select output mode. Does not carry
//! per-invocation state beyond the mode toggle.

/// Named format policies — the rule-slug registry (FORMAT-PRETTY S1).
///
/// A policy is an immutable semantic identity with exactly one byte-exact
/// output contract; it is not a name for whichever defaults are current.
/// `normalise-v1` is the promoted name of the current author-mode normalizer
/// (`forma/author/`) and must stay byte-identical to `compile_author` output.
/// `pretty-v1` is the pretty layout policy (`forma/pretty/`), selected through
/// the same registry — never a silent fallback between policies.
///
/// The registry lives in radix so faber's `--policy` flag and the package
/// manifest `[format]` schema resolve against the same source of truth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FormatPolicy {
    /// Current author-mode normalizer; the engine baseline policy.
    NormaliseV1,
    /// Pretty-layout policy (the `forma::pretty` engine).
    PrettyV1,
}

/// Errors from the rule-slug registry and policy selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyError {
    /// The slug is not a registered policy at all.
    UnknownSlug(String),
    /// The slug is a registered policy that has no engine support yet.
    NotImplemented(FormatPolicy),
}

impl FormatPolicy {
    /// Registered policy slugs in canonical order (the rule-slug registry).
    pub const ALL: &'static [FormatPolicy] = &[FormatPolicy::NormaliseV1, FormatPolicy::PrettyV1];

    /// The slug spelling used by CLI and manifest surfaces.
    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            FormatPolicy::NormaliseV1 => "normalise-v1",
            FormatPolicy::PrettyV1 => "pretty-v1",
        }
    }

    /// Resolve a slug against the registry.
    ///
    /// # Errors
    ///
    /// Returns [`PolicyError::UnknownSlug`] when the slug is not registered.
    /// Every registered slug resolves successfully here; a registered policy
    /// without engine support fails clearly with [`PolicyError::NotImplemented`]
    /// at the compile entry.
    pub fn from_slug(slug: &str) -> Result<FormatPolicy, PolicyError> {
        match slug {
            "normalise-v1" => Ok(FormatPolicy::NormaliseV1),
            "pretty-v1" => Ok(FormatPolicy::PrettyV1),
            other => Err(PolicyError::UnknownSlug(other.to_owned())),
        }
    }

    /// Whether the engine can compile with this policy today.
    #[must_use]
    pub const fn is_implemented(self) -> bool {
        matches!(self, FormatPolicy::NormaliseV1 | FormatPolicy::PrettyV1)
    }
}

impl Default for FormatPolicy {
    /// The radix-side no-argument baseline: the current author-mode normalizer,
    /// byte-identical to today's `compile_author` output. Product-level
    /// default selection and precedence are faber's concern (CTO seam).
    fn default() -> Self {
        FormatPolicy::NormaliseV1
    }
}

impl std::fmt::Display for FormatPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.slug())
    }
}

impl std::fmt::Display for PolicyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PolicyError::UnknownSlug(slug) => write!(
                f,
                "unknown format policy slug `{slug}`; registered slugs: {}",
                FormatPolicy::ALL
                    .iter()
                    .map(|policy| policy.slug())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            PolicyError::NotImplemented(policy) => {
                write!(f, "format policy `{policy}` is registered but not yet implemented")
            }
        }
    }
}

impl std::error::Error for PolicyError {}

/// Options controlling forma output mode and behavior.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FormatOptions {
    /// Emit canonical HIR-backed surface (default: author mode).
    pub canonical: bool,
}

impl FormatOptions {
    #[must_use]
    pub fn author() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn canonical() -> Self {
        Self { canonical: true }
    }
}
