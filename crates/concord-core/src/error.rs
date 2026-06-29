//! Error type for coordination operations.

use std::fmt;
use std::io;
use std::path::PathBuf;

/// Anything that can go wrong performing a coordination operation.
#[derive(Debug)]
pub enum ConcordError {
    /// An underlying filesystem error, annotated with the path it concerned.
    Io { path: PathBuf, source: io::Error },
    /// A required argument was missing (mirrors the shell's `${1:?...}`).
    MissingArg(&'static str),
}

impl ConcordError {
    pub(crate) fn io(path: impl Into<PathBuf>, source: io::Error) -> ConcordError {
        ConcordError::Io {
            path: path.into(),
            source,
        }
    }
}

impl fmt::Display for ConcordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConcordError::Io { path, source } => {
                write!(f, "{}: {}", path.display(), source)
            }
            ConcordError::MissingArg(what) => write!(f, "missing argument: {what}"),
        }
    }
}

impl std::error::Error for ConcordError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ConcordError::Io { source, .. } => Some(source),
            ConcordError::MissingArg(_) => None,
        }
    }
}

/// Convenience alias.
pub type Result<T> = std::result::Result<T, ConcordError>;
