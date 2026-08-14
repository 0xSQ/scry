//! Ready-made interface types for common config patterns.

pub mod files;
mod key_values;
mod one_or_many;

pub use files::{Files, SourceSpec};
pub use key_values::KeyValues;
pub use one_or_many::OneOrMany;
