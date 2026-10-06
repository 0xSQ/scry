//! Error type for node operations.

use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::key_path::KeyPath;
use crate::rhai::RhaiError;
use crate::util::PathError;
use crate::{BoxedError, KeyPathError};

use super::{FormatError, Kind};

// ---------------------------------------------------------------------------------------------- //
// NodeError

/// Error type for node operations (access, conversion, validation, loading, parsing).
#[derive(thiserror::Error)]
pub enum NodeError {
    /// Generic string message error with optional source.
    #[error("{message}")]
    Message {
        message: String,
        #[source]
        source: Option<BoxedError>,
    },

    /// A logical configuration location attached to an otherwise locationless error.
    #[error("error for {}: {source}", fmt_path(path))]
    AtPath {
        path: KeyPath,
        #[source]
        source: BoxedError,
    },

    /// A message for an invalid value at a given path, with an optional original cause.
    #[error("invalid value for {}: {}", fmt_path(path), message)]
    InvalidValue {
        path: KeyPath,
        message: String,
        #[source]
        source: Option<BoxedError>,
    },

    /// Required key is missing at the given path.
    #[error("missing value for {}", fmt_path(path))]
    MissingRequired { path: KeyPath },

    /// Type mismatch at the given path.
    #[error(
        "expected {} for {}, found type {}",
        target_type,
        fmt_path(path),
        source_type
    )]
    TypeMismatch {
        target_type: String,
        path: KeyPath,
        source_type: String,
    },

    /// Invalid conversion attempt at the given path.
    #[error(
        "cannot convert {} to {} (from {} '{}')",
        fmt_path(path),
        to,
        from,
        value
    )]
    InvalidConversion {
        path: KeyPath,
        to: String,
        from: String,
        value: String,
        #[source]
        source: Option<BoxedError>,
    },

    /// Array length does not match the expected size.
    #[error(
        "expected {} to be an array of length {expected}, found {found}",
        fmt_path(path)
    )]
    ArrayLength {
        path: KeyPath,
        expected: usize,
        found: usize,
    },

    /// Attempted to look up a string key in an array node.
    #[error("{} is an array, cannot look up key '{key}'", fmt_path(path))]
    KeyOnArray { path: KeyPath, key: String },

    /// Attempted to look up a numeric index in a map node.
    #[error("{} is a map, cannot look up index {index}", fmt_path(path))]
    IndexOnMap { path: KeyPath, index: usize },

    /// Array index is out of bounds.
    #[error(
        "index {index} is out of bounds, {} has {len} elements",
        fmt_path(path)
    )]
    IndexOutOfBounds {
        path: KeyPath,
        index: usize,
        len: usize,
    },

    /// Attempted to descend into a leaf (non-container) node.
    #[error("{} has type {found}, cannot descend into it", fmt_path(path))]
    DescendIntoLeaf { path: KeyPath, found: String },

    /// Attempted to remove the root node.
    #[error("cannot remove root node")]
    CannotRemoveRoot,

    /// Keys rejected by structural validation or an unread-input audit.
    #[error("unknown config keys:\n{}", fmt_unknown_keys(paths))]
    UnknownKeys { paths: Vec<KeyPath> },

    /// Format id/registry/dispatch error.
    #[error(transparent)]
    Format(#[from] FormatError),

    /// Error parsing a key path string.
    #[error(transparent)]
    KeyPath(#[from] KeyPathError),

    /// Path argument failed to parse as a key path.
    #[error("invalid path")]
    InvalidPath {
        #[source]
        source: KeyPathError,
    },

    /// Failed to read a file from disk.
    #[error("failed to read file: {}", path.display())]
    ReadFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// Failed while inspecting file extension metadata.
    #[error("failed to read file extension: {}", path.display())]
    ReadFileExtension {
        path: PathBuf,
        #[source]
        source: PathError,
    },

    /// Failed to parse input in a specific format.
    #[error("failed to parse {format}")]
    ParseFormat {
        format: &'static str,
        #[source]
        source: BoxedError,
    },

    /// Failed to serialize output in a specific format.
    #[error("failed to serialize as {format}")]
    SerializeFormat {
        format: &'static str,
        #[source]
        source: BoxedError,
    },

    /// Rhai-related error.
    #[error(transparent)]
    Rhai(#[from] RhaiError),
}

// ---------------------------------------------------------------------------------------------- //
// Debug

impl fmt::Debug for NodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self}")?;

        let Some(first) = self.source() else {
            return Ok(());
        };

        // Print causes, with numbers only if there are multiple.
        let multiple = first.source().is_some();
        write!(f, "\n\nCaused by:")?;

        if !multiple {
            return write!(f, "\n    {first}");
        }

        write!(f, "\n    0: {first}")?;
        let mut i = 1usize;
        let mut src = first.source();
        while let Some(e) = src {
            write!(f, "\n    {i}: {e}")?;
            src = e.source();
            i += 1;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------- //
// Constructors

impl NodeError {
    // --------------------------------------------------------------------------------------------- //
    // Generic message constructors

    /// Creates a freeform error with the given message.
    pub fn new(message: impl Into<String>) -> Self {
        NodeError::Message {
            message: message.into(),
            source: None,
        }
    }

    /// Creates an error that wraps an underlying cause with a contextual message.
    pub fn with_context(
        message: impl Into<String>,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        NodeError::Message {
            message: message.into(),
            source: Some(Box::new(source)),
        }
    }

    /// Creates an error that wraps an already-boxed cause with a contextual message.
    pub fn with_context_boxed(message: impl Into<String>, source: BoxedError) -> Self {
        NodeError::Message {
            message: message.into(),
            source: Some(source),
        }
    }

    // --------------------------------------------------------------------------------------------- //
    // Domain-specific constructors

    /// Generic error tied to a specific path.
    pub fn invalid_value(path: &KeyPath, message: impl Into<String>) -> Self {
        NodeError::InvalidValue {
            path: path.clone(),
            message: message.into(),
            source: None,
        }
    }

    /// Creates a located value error that retains its original cause.
    pub fn invalid_value_with_source(
        path: &KeyPath,
        message: impl Into<String>,
        source: impl Into<BoxedError>,
    ) -> Self {
        NodeError::InvalidValue {
            path: path.clone(),
            message: message.into(),
            source: Some(source.into()),
        }
    }

    /// Required value is missing at the given path.
    pub fn missing_required(path: &KeyPath) -> Self {
        NodeError::MissingRequired { path: path.clone() }
    }

    /// Expected one type or format, found another.
    pub fn type_mismatch(path: &KeyPath, target_type: &str, source_type: &str) -> Self {
        NodeError::TypeMismatch {
            path: path.clone(),
            target_type: target_type.to_string(),
            source_type: source_type.to_string(),
        }
    }

    /// Expected one type, found a different node kind.
    pub fn kind_mismatch(path: &KeyPath, target_type: &str, source_kind: &Kind) -> Self {
        Self::type_mismatch(path, target_type, kind_name(source_kind))
    }

    /// Invalid conversion attempt at the given path.
    pub fn invalid_conversion(path: &KeyPath, to: &str, from: &str, value: &str) -> Self {
        NodeError::InvalidConversion {
            path: path.clone(),
            to: to.to_string(),
            from: from.to_string(),
            value: value.to_string(),
            source: None,
        }
    }

    /// Creates a conversion error that retains its original cause.
    pub fn invalid_conversion_with_source(
        path: &KeyPath,
        to: &str,
        from: &str,
        value: &str,
        source: impl Into<BoxedError>,
    ) -> Self {
        NodeError::InvalidConversion {
            path: path.clone(),
            to: to.to_string(),
            from: from.to_string(),
            value: value.to_string(),
            source: Some(source.into()),
        }
    }

    /// Tried to use a key on an array.
    pub fn key_on_array(path: &KeyPath, key: &str) -> Self {
        NodeError::KeyOnArray {
            path: path.clone(),
            key: key.to_string(),
        }
    }

    /// Tried to use an index on a map.
    pub fn index_on_map(path: &KeyPath, index: usize) -> Self {
        NodeError::IndexOnMap {
            path: path.clone(),
            index,
        }
    }

    /// Index out of bounds.
    pub fn index_out_of_bounds(path: &KeyPath, index: usize, len: usize) -> Self {
        NodeError::IndexOutOfBounds {
            path: path.clone(),
            index,
            len,
        }
    }

    /// Cannot descend into a leaf node.
    pub fn descend_into_leaf(path: &KeyPath, found: &str) -> Self {
        NodeError::DescendIntoLeaf {
            path: path.clone(),
            found: found.to_string(),
        }
    }

    /// Cannot remove the root node.
    pub fn cannot_remove_root() -> Self {
        NodeError::CannotRemoveRoot
    }

    /// Array length mismatch.
    pub fn array_length(path: &KeyPath, expected: usize, found: usize) -> Self {
        NodeError::ArrayLength {
            path: path.clone(),
            expected,
            found,
        }
    }

    /// Unknown keys found in config.
    pub fn unknown_keys(paths: &[KeyPath]) -> Self {
        NodeError::UnknownKeys {
            paths: paths.to_vec(),
        }
    }

    /// Key path argument failed to parse.
    pub fn invalid_path(source: KeyPathError) -> Self {
        NodeError::InvalidPath { source }
    }

    /// Filesystem read failed for the given path.
    pub fn read_file(path: &Path, source: std::io::Error) -> Self {
        NodeError::ReadFile {
            path: path.to_path_buf(),
            source,
        }
    }

    /// File extension inspection failed for the given path.
    pub fn read_file_extension(path: &Path, source: PathError) -> Self {
        NodeError::ReadFileExtension {
            path: path.to_path_buf(),
            source,
        }
    }

    /// Parsing failed for the given config format.
    pub fn parse_format(
        format: &'static str,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        NodeError::ParseFormat {
            format,
            source: Box::new(source),
        }
    }

    /// Parsing failed for the given config format with an already-boxed source.
    pub fn parse_format_boxed(format: &'static str, source: BoxedError) -> Self {
        NodeError::ParseFormat { format, source }
    }

    /// Serialization failed for the given output format.
    pub fn serialize_format(
        format: &'static str,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        NodeError::SerializeFormat {
            format,
            source: Box::new(source),
        }
    }

    // ------------------------------------------------------------------------------------------ //
    // Logical Configuration Locations

    /// Returns the logical configuration path carried by this error, if it has one.
    ///
    /// Filesystem paths retain their own meaning and are not returned here. Aggregated unknown-key
    /// errors carry several locations in their `paths` field instead.
    pub fn path(&self) -> Option<&KeyPath> {
        match self {
            Self::AtPath { path, .. }
            | Self::InvalidValue { path, .. }
            | Self::MissingRequired { path }
            | Self::TypeMismatch { path, .. }
            | Self::InvalidConversion { path, .. }
            | Self::ArrayLength { path, .. }
            | Self::KeyOnArray { path, .. }
            | Self::IndexOnMap { path, .. }
            | Self::IndexOutOfBounds { path, .. }
            | Self::DescendIntoLeaf { path, .. } => Some(path),
            _ => None,
        }
    }

    /// Attaches an input location only when the error has no logical configuration location.
    ///
    /// Input Nodes already carry their full logical path. Existing locations, including an empty
    /// root path and aggregated unknown-key paths, are preserved. The original error stays in the
    /// source chain, including any filesystem path or parser cause it contains.
    pub fn at_path(self, path: &KeyPath) -> Self {
        if self.path().is_some() || matches!(self, Self::UnknownKeys { .. }) {
            self
        } else {
            Self::AtPath {
                path: path.clone(),
                source: Box::new(self),
            }
        }
    }

    /// Prepends an enclosing output location to this error's relative logical path.
    ///
    /// Each structured serializer adds the key or index through which it called its child.
    /// Existing relative segments are retained. Locationless errors gain an enclosing location
    /// without changing their original error or cause. Input decoding uses [`Self::at_path`]
    /// instead because input paths are already absolute within the logical configuration.
    pub fn prepend_path(mut self, prefix: &KeyPath) -> Self {
        if prefix.is_empty() {
            return self;
        }
        match &mut self {
            Self::AtPath { path, .. }
            | Self::InvalidValue { path, .. }
            | Self::MissingRequired { path }
            | Self::TypeMismatch { path, .. }
            | Self::InvalidConversion { path, .. }
            | Self::ArrayLength { path, .. }
            | Self::KeyOnArray { path, .. }
            | Self::IndexOnMap { path, .. }
            | Self::IndexOutOfBounds { path, .. }
            | Self::DescendIntoLeaf { path, .. } => *path = prefix.join(path),
            Self::UnknownKeys { paths } => {
                for path in paths {
                    *path = prefix.join(path);
                }
            }
            _ => return self.at_path(prefix),
        }
        self
    }
}

// ---------------------------------------------------------------------------------------------- //
// Formatting Helpers

/// Formats a KeyPath as a subject in error messages.
///
/// Returns `"'path'"` for non-empty paths, `"root"` for empty paths.
fn fmt_path(path: &KeyPath) -> String {
    if path.is_empty() {
        "config".to_string()
    } else {
        format!("'{path}'")
    }
}

/// Formats a list of unknown key paths for error display.
fn fmt_unknown_keys(paths: &[KeyPath]) -> String {
    paths.iter().map(|p| format!("  {p}")).collect::<Vec<_>>().join("\n")
}

/// Returns a human-readable name for a node kind.
fn kind_name(kind: &Kind) -> &str {
    match kind {
        Kind::Vec(_) => "array",
        Kind::Map(_) => "map",
        Kind::Leaf(leaf) => leaf.value.type_name(),
    }
}

// ---------------------------------------------------------------------------------------------- //
// Tests

#[cfg(test)]
mod tests;
