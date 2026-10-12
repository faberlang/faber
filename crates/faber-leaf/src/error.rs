//! Structured fs-leaf errors. Kind is the identity; text is for the frame.

use std::fmt;
use std::io;

/// Identity a conformance test asserts. The renderer owns prose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsErrorKind {
    NotFound,
    Permission,
    NotADirectory,
    IsADirectory,
    InvalidArgument,
    Io,
}

/// One failed fs leaf call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FsError {
    kind: FsErrorKind,
    route: &'static str,
    path: String,
    source: String,
}

impl FsError {
    #[must_use]
    pub fn kind(&self) -> FsErrorKind {
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
            kind: FsErrorKind::InvalidArgument,
            route,
            path: String::new(),
            source: message.into(),
        }
    }

    #[must_use]
    pub fn is_a_directory(route: &'static str, path: impl Into<String>) -> Self {
        Self {
            kind: FsErrorKind::IsADirectory,
            route,
            path: path.into(),
            source: "is a directory".to_owned(),
        }
    }

    #[must_use]
    pub fn not_a_directory(route: &'static str, path: impl Into<String>) -> Self {
        Self {
            kind: FsErrorKind::NotADirectory,
            route,
            path: path.into(),
            source: "not a directory".to_owned(),
        }
    }

    #[must_use]
    pub fn from_io(route: &'static str, path: impl Into<String>, err: &io::Error) -> Self {
        let path = path.into();
        let kind = match err.kind() {
            io::ErrorKind::NotFound => FsErrorKind::NotFound,
            io::ErrorKind::PermissionDenied => FsErrorKind::Permission,
            _ => FsErrorKind::Io,
        };
        Self {
            kind,
            route,
            path,
            source: err.to_string(),
        }
    }
}

impl fmt::Display for FsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.path.is_empty() {
            write!(f, "{}: {}", self.route, self.source)
        } else {
            write!(f, "{} {}: {}", self.route, self.path, self.source)
        }
    }
}

impl std::error::Error for FsError {}
