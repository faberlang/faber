//! Structured leaf errors. Kind is the identity; text is for the frame.

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

/// One failed leaf call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeafError {
    kind: LeafErrorKind,
    route: &'static str,
    path: String,
    source: String,
}

/// Fs-leaf name kept so existing fs tests and docs stay stable.
pub type FsError = LeafError;
/// Fs-leaf kind name kept so existing fs tests stay stable.
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

    #[must_use]
    pub fn invalid_argument(route: &'static str, message: impl Into<String>) -> Self {
        Self {
            kind: LeafErrorKind::InvalidArgument,
            route,
            path: String::new(),
            source: message.into(),
        }
    }

    #[must_use]
    pub fn closed(route: &'static str, message: impl Into<String>) -> Self {
        Self {
            kind: LeafErrorKind::Closed,
            route,
            path: String::new(),
            source: message.into(),
        }
    }

    #[must_use]
    pub fn unset(route: &'static str, message: impl Into<String>) -> Self {
        Self {
            kind: LeafErrorKind::Unset,
            route,
            path: String::new(),
            source: message.into(),
        }
    }

    #[must_use]
    pub fn unavailable(route: &'static str, message: impl Into<String>) -> Self {
        Self {
            kind: LeafErrorKind::Unavailable,
            route,
            path: String::new(),
            source: message.into(),
        }
    }

    #[must_use]
    pub fn cancelled(route: &'static str) -> Self {
        Self {
            kind: LeafErrorKind::Cancelled,
            route,
            path: String::new(),
            source: "cancelled".to_owned(),
        }
    }

    #[must_use]
    pub fn is_a_directory(route: &'static str, path: impl Into<String>) -> Self {
        Self {
            kind: LeafErrorKind::IsADirectory,
            route,
            path: path.into(),
            source: "is a directory".to_owned(),
        }
    }

    #[must_use]
    pub fn not_a_directory(route: &'static str, path: impl Into<String>) -> Self {
        Self {
            kind: LeafErrorKind::NotADirectory,
            route,
            path: path.into(),
            source: "not a directory".to_owned(),
        }
    }

    #[must_use]
    pub fn from_io(route: &'static str, path: impl Into<String>, err: &io::Error) -> Self {
        let path = path.into();
        let kind = match err.kind() {
            io::ErrorKind::NotFound => LeafErrorKind::NotFound,
            io::ErrorKind::PermissionDenied => LeafErrorKind::Permission,
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
            source: err.to_string(),
        }
    }
}

impl fmt::Display for LeafError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.path.is_empty() {
            write!(f, "{}: {}", self.route, self.source)
        } else {
            write!(f, "{} {}: {}", self.route, self.path, self.source)
        }
    }
}

impl std::error::Error for LeafError {}

#[cfg(test)]
#[path = "error_test.rs"]
mod tests;
