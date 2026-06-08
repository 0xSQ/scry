//! Error types for the CLI setup layer.

use std::error::Error;
use std::fmt;
use std::path::PathBuf;

use crate::desc::DescPathError;
use crate::key_path::KeyPathError;
use crate::node::NodeError;
use crate::BoxedError;

// ---------------------------------------------------------------------------------------------- //
// SetupError

/// Unified error type for running CLI bundles created via [`Setup`](super::Setup).
///
/// Aggregates all error types that can occur when running a setup-composed bundle.
/// Each variant transparently wraps a dedicated error type.
#[derive(thiserror::Error)]
pub enum SetupError {
    /// Error from node operations.
    #[error(transparent)]
    Node(#[from] NodeError),

    /// Failed to load a config file through the configured loader.
    #[error("failed to load config file: {}", path.display())]
    LoadConfigFile {
        path: PathBuf,
        #[source]
        source: BoxedError,
    },

    /// Failed to evaluate the input configuration into the target type.
    #[error("failed to evaluate input configuration")]
    EvaluateInputConfig {
        #[source]
        source: BoxedError,
    },

    /// Config path was specified via both positional argument and option flag.
    #[error("config path specified both as positional argument and via option flag")]
    MultipleConfigPaths,

    /// Could not find a config file via discovery.
    #[error("could not find config file for command '{cmd_name}'")]
    ConfigNotFound {
        cmd_name: String,
        #[source]
        source: BoxedError,
    },

    /// No config file was specified and no discovery or empty-node policy is configured.
    #[error("no config file specified for command '{cmd_name}'")]
    NoConfigFile { cmd_name: String },

    /// Key path parsing error.
    #[error("invalid key path")]
    KeyPath {
        #[source]
        source: KeyPathError,
    },

    /// Description path validation error.
    #[error("invalid key path")]
    DescPath {
        #[source]
        source: DescPathError,
    },

    /// Requested config path does not exist in the tree.
    #[error("{message}")]
    GetNotFound { message: String },

    /// Attempted to remove a config path that doesn't exist.
    #[error("{message}")]
    RemoveNotFound { message: String },

    /// Invalid format string for `--get-as` argument. (TODO: print format list automatically).
    #[error("unknown config format '{format_str}'")]
    UnknownGetAsFormat { format_str: String },
}

impl fmt::Debug for SetupError {
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
