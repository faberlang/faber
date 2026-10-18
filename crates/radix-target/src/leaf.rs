//! Native effect-leaf identity.
//!
//! A leaf is one native capability family shared by the interpreter and
//! generated Rust (`crates/faber-leaf`). Targets declare the leaves they
//! support. A program that uses an undeclared leaf fails at compile time
//! (`leaf_unsupported_on_target`).
//!
//! Route prefixes are the identity: `solum` is fs, `consolum` is stdio,
//! `processus` is process, `tempus` is clock, `aleator` is entropy,
//! `machina` is machine. `processus:exi` and `runtime:*` are language
//! builtins, not leaves.

/// One native effect leaf. Spellings match the campaign contract names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Leaf {
    /// Path-based filesystem (`solum:*`).
    Fs,
    /// Process stdio (`consolum:*`).
    Stdio,
    /// Process control (`processus:*` except builtin `processus:exi`).
    Process,
    /// Clock (`tempus:*`).
    Clock,
    /// Entropy (`aleator:*`).
    Entropy,
    /// TCP/TLS (`caelum:*`).
    Tcp,
    /// HTTP/1.1 client and server (`http:*`).
    Http,
    /// WebSocket (`websocket:*`).
    Websocket,
    /// Host machine snapshot (`machina:*`).
    Machine,
}

impl Leaf {
    /// Every declared leaf, in declaration order.
    pub const ALL: &'static [Self] = &[
        Self::Fs,
        Self::Stdio,
        Self::Process,
        Self::Clock,
        Self::Entropy,
        Self::Tcp,
        Self::Http,
        Self::Websocket,
        Self::Machine,
    ];

    /// Canonical leaf spelling used in diagnostics and `faber targets`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Fs => "fs",
            Self::Stdio => "stdio",
            Self::Process => "process",
            Self::Clock => "clock",
            Self::Entropy => "entropy",
            Self::Tcp => "tcp",
            Self::Http => "http",
            Self::Websocket => "websocket",
            Self::Machine => "machine",
        }
    }

    /// The Latin route prefix this leaf owns.
    #[must_use]
    pub const fn route_prefix(self) -> &'static str {
        match self {
            Self::Fs => "solum",
            Self::Stdio => "consolum",
            Self::Process => "processus",
            Self::Clock => "tempus",
            Self::Entropy => "aleator",
            Self::Tcp => "caelum",
            Self::Http => "http",
            Self::Websocket => "websocket",
            Self::Machine => "machina",
        }
    }

    /// The leaf that owns `route`, if any.
    ///
    /// Builtins (`runtime:*`, `processus:exi`) are not leaves.
    #[must_use]
    pub fn from_route(route: &str) -> Option<Self> {
        if route == "processus:exi" || route.starts_with("runtime:") {
            return None;
        }
        let prefix = route.split([':', '/']).next()?;
        match prefix {
            "solum" => Some(Self::Fs),
            "consolum" => Some(Self::Stdio),
            "processus" => Some(Self::Process),
            "tempus" => Some(Self::Clock),
            "aleator" => Some(Self::Entropy),
            "caelum" => Some(Self::Tcp),
            "http" => Some(Self::Http),
            "websocket" => Some(Self::Websocket),
            "machina" => Some(Self::Machine),
            _ => None,
        }
    }
}

impl std::fmt::Display for Leaf {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
#[path = "leaf_test.rs"]
mod tests;
