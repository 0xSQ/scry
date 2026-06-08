//! Core traits for Node conversion and implementations for primitive types.
//!
//! Provides [`FromNode`] for parsing, [`ToNode`] for serialization, and
//! [`Describe`] for configuration description.

mod describe;
mod from_node;
mod to_node;

// ---------------------------------------------------------------------------------------------- //

pub use describe::Describe;
pub use from_node::FromNode;
pub use to_node::ToNode;
