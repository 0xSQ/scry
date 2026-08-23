//! The [`FromDefaults`] trait for constructing values from Scry defaults.

use crate::{KeyPath, NodeError};

// ---------------------------------------------------------------------------------------------- //

/// Constructs a value from its Scry-declared defaults at a logical config path.
///
/// Use `#[derive(scry::FromDefaults)]` or `#[derive(scry::Config)]` to generate implementations for
/// named config structs. Enums can select one unit variant with `#[scry(default)]`. The operation
/// is fallible because a struct may contain required fields that have no declared default.
pub trait FromDefaults: Sized {
    /// Attempts to construct the value, anchoring any error below `path`.
    ///
    /// The path is diagnostic context and should not affect the value being constructed.
    fn from_defaults_at(path: &KeyPath) -> Result<Self, NodeError>;
}
