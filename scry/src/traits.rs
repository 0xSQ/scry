//! Core traits for Node conversion and implementations for primitive types.
//!
//! Provides [`FromNode`] for parsing, [`ToNode`] for serialization, [`Describe`] for
//! configuration description, and [`DefaultNode`] for default baselines.

mod default_node;
mod describe;
mod from_node;
mod to_node;

// ---------------------------------------------------------------------------------------------- //

pub use default_node::{
    baseline_insert, baseline_insert_structural, make_default_node_probe, DefaultNode,
    DefaultNodeFallback, DefaultNodeProbe,
};
pub use describe::Describe;
pub use from_node::FromNode;
pub use to_node::ToNode;
