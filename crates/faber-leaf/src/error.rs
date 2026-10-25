//! Structured leaf errors. Kind is the identity; text is for the frame.

#[cfg(test)]
#[path = "error_test.rs"]
mod tests;

use std::fmt;
use std::io;

/// Identity a conformance test asserts. The renderer owns prose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeafErrorKind {
    NotFound,
    Permission,
    NotADirectory,
    IsADirectory,
    InvalidArgument,
    AlreadyExists,
    TimedOut,
    ConnectionRefused,
    ConnectionReset,
    AddrInUse,
    Io,
    Closed,
    Unset,
    Unavailable,
    Cancelled,
}

impl LeafErrorKind {
    /// Every kind, in declaration order.
    pub const ALL: [LeafErrorKind; 15] = [
        LeafErrorKind::NotFound,
        LeafErrorKind::Permission,
        LeafErrorKind::NotADirectory,
        LeafErrorKind::IsADirectory,
        LeafErrorKind::InvalidArgument,
        LeafErrorKind::AlreadyExists,
        LeafErrorKind::TimedOut,
        LeafErrorKind::ConnectionRefused,
        LeafErrorKind::ConnectionReset,
        LeafErrorKind::AddrInUse,
        LeafErrorKind::Io,
        LeafErrorKind::Closed,
        LeafErrorKind::Unset,
        LeafErrorKind::Unavailable,
        LeafErrorKind::Cancelled,
    ];

    /// The fixed identity word the crossing into Faber carries: the Norma
    /// `IoFault` variant name.
    ///
    /// Spelling is interface.
    #[must_use]
    pub fn variant_name(self) -> &'static str {
        match self {
            LeafErrorKind::NotFound => "NotFound",
            LeafErrorKind::Permission => "Permission",
            LeafErrorKind::NotADirectory => "NotADirectory",
            LeafErrorKind::IsADirectory => "IsADirectory",
            LeafErrorKind::InvalidArgument => "InvalidArgument",
            LeafErrorKind::AlreadyExists => "AlreadyExists",
            LeafErrorKind::TimedOut => "TimedOut",
            LeafErrorKind::ConnectionRefused => "ConnectionRefused",
            LeafErrorKind::ConnectionReset => "ConnectionReset",
            LeafErrorKind::AddrInUse => "AddrInUse",
            LeafErrorKind::Io => "Io",
            LeafErrorKind::Closed => "Closed",
            LeafErrorKind::Unset => "Unset",
            LeafErrorKind::Unavailable => "Unavailable",
            LeafErrorKind::Cancelled => "Cancelled",
        }
    }
}

/// One failed leaf call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeafError {
    kind: LeafErrorKind,
    route: &'static str,
    path: String,
    text: String,
}

/// Fs-leaf name kept so existing fs tests and docs stay stable.
pub type FsError = LeafError;
/// Fs-leaf kind name kept so existing fs tests and docs stay stable.
pub type FsErrorKind = LeafErrorKind;

impl LeafError {
    #[must_use]
    pub fn kind(&self) -> LeafErrorKind {
        self.kind
    }

    #[must_use]
    pub fn route(&self) -> &'static str {
        self.route
    }

    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The two facts a failure carries into Faber: kind word and path.
    ///
    /// Platform text never crosses; it stays in `text` for the local frame.
    #[must_use]
    pub fn facts(&self) -> (&'static str, &str) {
        (self.kind.variant_name(), self.path())
    }

    #[must_use]
    pub fn invalid_argument(route: &'static str, message: impl Into<String>) -> Self {
        Self {
            kind: LeafErrorKind::InvalidArgument,
            route,
            path: String::new(),
            text: message.into(),
        }
    }

    #[must_use]
    pub fn closed(route: &'static str, message: impl Into<String>) -> Self {
        Self {
            kind: LeafErrorKind::Closed,
            route,
            path: String::new(),
            text: message.into(),
        }
    }

    #[must_use]
    pub fn unavailable(route: &'static str, message: impl Into<String>) -> Self {
        Self {
            kind: LeafErrorKind::Unavailable,
            route,
            path: String::new(),
            text: message.into(),
        }
    }

    #[must_use]
    pub fn cancelled(route: &'static str) -> Self {
        Self {
            kind: LeafErrorKind::Cancelled,
            route,
            path: String::new(),
            text: "cancelled".to_owned(),
        }
    }

    #[must_use]
    pub fn is_a_directory(route: &'static str, path: impl Into<String>) -> Self {
        Self {
            kind: LeafErrorKind::IsADirectory,
            route,
            path: path.into(),
            text: "is a directory".to_owned(),
        }
    }

    #[must_use]
    pub fn not_a_directory(route: &'static str, path: impl Into<String>) -> Self {
        Self {
            kind: LeafErrorKind::NotADirectory,
            route,
            path: path.into(),
            text: "not a directory".to_owned(),
        }
    }

    #[must_use]
    pub fn from_io(route: &'static str, path: impl Into<String>, err: &io::Error) -> Self {
        let path = path.into();
        let kind = match err.kind() {
            io::ErrorKind::NotFound => LeafErrorKind::NotFound,
            io::ErrorKind::PermissionDenied => LeafErrorKind::Permission,
            // The host reports a stream the peer closed as a broken pipe.
            io::ErrorKind::BrokenPipe => LeafErrorKind::Closed,
            io::ErrorKind::AlreadyExists => LeafErrorKind::AlreadyExists,
            io::ErrorKind::InvalidInput => LeafErrorKind::InvalidArgument,
            io::ErrorKind::TimedOut => LeafErrorKind::TimedOut,
            io::ErrorKind::ConnectionRefused => LeafErrorKind::ConnectionRefused,
            io::ErrorKind::ConnectionReset => LeafErrorKind::ConnectionReset,
            io::ErrorKind::AddrInUse => LeafErrorKind::AddrInUse,
            io::ErrorKind::IsADirectory => LeafErrorKind::IsADirectory,
            io::ErrorKind::NotADirectory => LeafErrorKind::NotADirectory,
            _ => LeafErrorKind::Io,
        };
        Self {
            kind,
            route,
            path,
            text: err.to_string(),
        }
    }
}

impl fmt::Display for LeafError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.path.is_empty() {
            write!(f, "{}: {}", self.route, self.text)
        } else {
            write!(f, "{} {}: {}", self.route, self.path, self.text)
        }
    }
}

impl std::error::Error for LeafError {}
