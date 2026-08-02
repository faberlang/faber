//! Kernel module identity carried on MIR providers (leaf-local copy).
//!
//! Package `radix` owns the full script-mode kernel manifest; MIR only needs
//! the closed module identity enum.

use serde::{Deserialize, Serialize};

/// Closed v1 kernel module identity carried on MIR providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum KernelModule {
    Solum,
    Processus,
    Aleator,
    Json,
    Consolum,
}

impl KernelModule {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Solum => "solum",
            Self::Processus => "processus",
            Self::Aleator => "aleator",
            Self::Json => "json",
            Self::Consolum => "consolum",
        }
    }

    #[must_use]
    pub const fn import_path(self) -> &'static str {
        match self {
            Self::Solum => "faber:solum",
            Self::Processus => "faber:processus",
            Self::Aleator => "faber:aleator",
            Self::Json => "faber:json",
            Self::Consolum => "faber:consolum",
        }
    }
}
