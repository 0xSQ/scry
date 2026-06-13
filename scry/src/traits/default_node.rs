//! The [`DefaultNode`] trait and its specialization probe.

use std::marker::PhantomData;

use crate::node::{Node, NodeError};

// ---------------------------------------------------------------------------------------------- //

/// Provides the partial tree of default values for a config type.
///
/// The returned tree is the *default baseline*: a path present in it carries the default value
/// for that path, a path absent from it has no default. Required nested config structs appear
/// as maps containing only their defaultable children, which is what makes the baseline partial.
///
/// `Ok(None)` means the type has no defaults anywhere. Only derived config types implement this
/// trait; primitives and containers are handled by the probe fallback, which reports no baseline.
///
/// Use `#[derive(scry::Config)]` (or `#[derive(scry::DefaultNode)]`) to generate implementations.
pub trait DefaultNode {
    /// Returns the default baseline for this type, or None if it has no defaults.
    fn default_node() -> Result<Option<Node>, NodeError>;
}

// ---------------------------------------------------------------------------------------------- //
// Specialization Machinery

/// Probe for detecting DefaultNode implementation via specialization.
pub struct DefaultNodeProbe<T: ?Sized>(PhantomData<T>);

/// Creates a probe for the given type.
pub fn make_default_node_probe<T: ?Sized>() -> DefaultNodeProbe<T> {
    DefaultNodeProbe(PhantomData)
}

/// Fallback trait that reports no baseline for types without DefaultNode.
pub trait DefaultNodeFallback {
    fn default_node(&self) -> Result<Option<Node>, NodeError> {
        Ok(None)
    }
}

impl<T: ?Sized> DefaultNodeFallback for DefaultNodeProbe<T> {}

/// Inherent impl for types that implement DefaultNode; takes precedence over the fallback.
impl<T: DefaultNode> DefaultNodeProbe<T> {
    pub fn default_node(&self) -> Result<Option<Node>, NodeError> {
        T::default_node()
    }
}

// ---------------------------------------------------------------------------------------------- //
// Derive Support

/// Inserts an evaluated default into a baseline map, applying null pruning.
///
/// A default that serializes to `Null` means "the default is unset"; consistent with Scry's
/// null-as-absence semantics it is omitted from the baseline.
#[doc(hidden)]
pub fn baseline_insert(map: &mut indexmap::IndexMap<String, Node>, key: &str, child: Node) {
    if !child.is_null_leaf() {
        map.insert(key.to_string(), child);
    }
}

/// Inserts a structurally recursed child baseline, applying null and empty-map pruning.
///
/// Empty maps are pruned here (a required nested struct with no defaultable children carries
/// no baseline information), but never in [`baseline_insert`] - an explicit default expression
/// producing an empty map is a real baseline value.
#[doc(hidden)]
pub fn baseline_insert_structural(
    map: &mut indexmap::IndexMap<String, Node>,
    key: &str,
    child: Node,
) {
    if matches!(&child.kind, crate::node::Kind::Map(m) if m.is_empty()) {
        return;
    }
    baseline_insert(map, key, child);
}
