//! Faber `regex` runtime carrier.

/// Pattern carrier for Faber `regex` (compile-time literal only today).
#[derive(Clone, PartialEq, Eq)]
pub struct Regex {
    pattern: String,
}

impl Regex {
    #[must_use]
    pub fn new(pattern: &str) -> Self {
        Self {
            pattern: pattern.to_owned(),
        }
    }

    #[must_use]
    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    /// Whether the pattern matches `textus` (the Rust `regex` crate
    /// `is_match` semantics).
    ///
    /// G12 `consentit`: `faber::Regex` is a compile-time literal carrier, so
    /// the pattern is fixed per value. An invalid pattern deterministically
    /// matches nothing (`false`) rather than panicking — script mode surfaces
    /// the compile error loudly at the stepper eval; the compiled runtime has
    /// no error channel on this verb.
    #[must_use]
    pub fn consentit(&self, textus: String) -> bool {
        regex::Regex::new(&self.pattern)
            .map(|regex| regex.is_match(&textus))
            .unwrap_or(false)
    }
}

impl std::fmt::Debug for Regex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "regex {:?}", self.pattern)
    }
}

impl std::fmt::Display for Regex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.pattern)
    }
}
