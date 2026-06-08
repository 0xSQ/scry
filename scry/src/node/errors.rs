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

    /// Generic message for an invalid value at a given path.
    #[error("invalid value for {}: {}", fmt_path(path), message)]
    InvalidValue { path: KeyPath, message: String },

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

    /// Unknown (unvisited) keys found in the config.
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
mod tests {
    use super::*;
    use crate::node::{Node, Value};

    fn path(s: &str) -> KeyPath {
        s.parse().unwrap()
    }

    // ------------------------------------------------------------------------------------------ //
    // Display - with path

    #[test]
    fn display_message() {
        let err = NodeError::new("something went wrong");
        assert_eq!(err.to_string(), "something went wrong");
    }

    #[test]
    fn display_invalid_value() {
        let err = NodeError::invalid_value(&path("server.port"), "must be between 1 and 65535");
        assert_eq!(err.to_string(), "invalid value for 'server.port': must be between 1 and 65535");
    }

    #[test]
    fn display_missing_required() {
        let err = NodeError::missing_required(&path("database.host"));
        assert_eq!(err.to_string(), "missing value for 'database.host'");
    }

    #[test]
    fn display_type_mismatch() {
        let err = NodeError::type_mismatch(&path("server.port"), "string", "i64");
        assert_eq!(err.to_string(), "expected string for 'server.port', found type i64");
    }

    #[test]
    fn display_kind_mismatch() {
        let map_node = Node::new_map(KeyPath::new(), indexmap::IndexMap::new());
        let err = NodeError::kind_mismatch(&path("server"), "array", &map_node.kind);
        assert_eq!(err.to_string(), "expected array for 'server', found type map");
    }

    #[test]
    fn display_invalid_conversion() {
        let err = NodeError::invalid_conversion(&path("timeout"), "u16", "i64", "-5");
        assert_eq!(err.to_string(), "cannot convert 'timeout' to u16 (from i64 '-5')");
    }

    #[test]
    fn display_array_length() {
        let err = NodeError::array_length(&path("color"), 3, 2);
        assert_eq!(err.to_string(), "expected 'color' to be an array of length 3, found 2");
    }

    #[test]
    fn display_key_on_array() {
        let err = NodeError::key_on_array(&path("items"), "name");
        assert_eq!(err.to_string(), "'items' is an array, cannot look up key 'name'");
    }

    #[test]
    fn display_index_on_map() {
        let err = NodeError::index_on_map(&path("server"), 0);
        assert_eq!(err.to_string(), "'server' is a map, cannot look up index 0");
    }

    #[test]
    fn display_index_out_of_bounds() {
        let err = NodeError::index_out_of_bounds(&path("items"), 5, 3);
        assert_eq!(err.to_string(), "index 5 is out of bounds, 'items' has 3 elements");
    }

    #[test]
    fn display_descend_into_leaf() {
        let err = NodeError::descend_into_leaf(&path("server.port"), "i64");
        assert_eq!(err.to_string(), "'server.port' has type i64, cannot descend into it");
    }

    #[test]
    fn display_cannot_remove_root() {
        let err = NodeError::cannot_remove_root();
        assert_eq!(err.to_string(), "cannot remove root node");
    }

    #[test]
    fn display_unknown_keys() {
        let err = NodeError::unknown_keys(&[path("server.foo"), path("server.bar")]);
        assert_eq!(err.to_string(), "unknown config keys:\n  server.foo\n  server.bar");
    }

    #[test]
    fn display_missing_file_extension() {
        let err = NodeError::from(FormatError::missing_file_extension(Path::new("config")));
        assert_eq!(
            err.to_string(),
            "cannot determine config format: file has no extension: config"
        );
    }

    #[test]
    fn display_unknown_file_extension() {
        let err = NodeError::from(FormatError::unknown_file_extension(
            Path::new("config.yml"),
            "yml",
            &["json", "json5", "rhai"],
        ));
        assert_eq!(
            err.to_string(),
            "unknown file extension '.yml' for file: config.yml (supported: json, json5, rhai)"
        );
    }

    #[test]
    fn display_unknown_output_format_id() {
        let err = NodeError::from(FormatError::unknown_format_id(
            crate::node::FormatUsage::Output,
            "yaml",
            &["json", "rhai"],
        ));
        assert_eq!(err.to_string(), "unknown output format 'yaml' (supported: json, rhai)");
    }

    #[test]
    fn display_invalid_path() {
        let source = "server..port".parse::<KeyPath>().unwrap_err();
        let err = NodeError::invalid_path(source);
        assert_eq!(err.to_string(), "invalid path");
    }

    #[test]
    fn display_read_file() {
        let source = std::io::Error::new(std::io::ErrorKind::NotFound, "missing");
        let err = NodeError::read_file(Path::new("config.toml"), source);
        assert_eq!(err.to_string(), "failed to read file: config.toml");
    }

    #[test]
    fn display_parse_format() {
        let source = std::io::Error::new(std::io::ErrorKind::InvalidData, "bad");
        let err = NodeError::parse_format("JSON", source);
        assert_eq!(err.to_string(), "failed to parse JSON");
    }

    #[test]
    fn display_serialize_format() {
        let source = std::io::Error::other("boom");
        let err = NodeError::serialize_format("Rhai", source);
        assert_eq!(err.to_string(), "failed to serialize as Rhai");
    }

    // ------------------------------------------------------------------------------------------ //
    // Display - empty path (root)

    #[test]
    fn display_invalid_value_at_root() {
        let err = NodeError::invalid_value(&KeyPath::new(), "expected a map");
        assert_eq!(err.to_string(), "invalid value for config: expected a map");
    }

    #[test]
    fn display_missing_required_at_root() {
        let err = NodeError::missing_required(&KeyPath::new());
        assert_eq!(err.to_string(), "missing value for config");
    }

    #[test]
    fn display_type_mismatch_at_root() {
        let err = NodeError::type_mismatch(&KeyPath::new(), "map", "array");
        assert_eq!(err.to_string(), "expected map for config, found type array");
    }

    #[test]
    fn display_array_length_at_root() {
        let err = NodeError::array_length(&KeyPath::new(), 3, 2);
        assert_eq!(err.to_string(), "expected config to be an array of length 3, found 2");
    }

    #[test]
    fn display_index_out_of_bounds_at_root() {
        let err = NodeError::index_out_of_bounds(&KeyPath::new(), 5, 3);
        assert_eq!(err.to_string(), "index 5 is out of bounds, config has 3 elements");
    }

    // ------------------------------------------------------------------------------------------ //
    // Debug - error chain rendering

    #[test]
    fn debug_without_source() {
        let err = NodeError::new("top-level error");
        assert_eq!(format!("{err:?}"), "top-level error");
    }

    #[test]
    fn debug_with_single_cause() {
        let inner = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err = NodeError::with_context("failed to load config", inner);
        assert_eq!(format!("{err:?}"), "failed to load config\n\nCaused by:\n    file not found");
    }

    #[test]
    fn debug_with_multiple_causes() {
        let innermost =
            std::io::Error::new(std::io::ErrorKind::PermissionDenied, "permission denied");
        let middle = NodeError::with_context("failed to read file", innermost);
        let outer = NodeError::with_context_boxed("failed to load config", Box::new(middle));
        assert_eq!(
            format!("{outer:?}"),
            "failed to load config\n\nCaused by:\n    0: failed to read file\n    1: permission denied"
        );
    }

    // ------------------------------------------------------------------------------------------ //
    // kind_name

    #[test]
    fn kind_name_reports_leaf_type() {
        let node = Node::new_leaf(KeyPath::new(), Value::Bool(true));
        let err = NodeError::kind_mismatch(&KeyPath::new(), "string", &node.kind);
        assert_eq!(err.to_string(), "expected string for config, found type bool");
    }
}
