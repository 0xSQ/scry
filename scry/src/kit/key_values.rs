//! Key-value collection type for interface boundaries.
//!
//! Provides [`KeyValues<V>`], an ordered collection of string-keyed entries
//! that accepts config input as either a map or a list of pairs.

use crate::convert::{read, write};
use crate::desc::Desc;
use crate::node::{Kind, Node, NodeError};
use crate::traits::{Describe, FromNode, ToNode};

// ---------------------------------------------------------------------------------------------- //

/// Ordered key-value entries used at the interface boundary.
///
/// Accepts config input as either:
/// - a map/object (ergonomic): `{ "a": 1, "b": 2 }`
/// - or a list of pairs (explicit): `[["a", 1], ["b", 2]]`
///
/// Duplicates are preserved; the application can decide how to handle them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KeyValues<V>(pub Vec<(String, V)>);

impl<V> KeyValues<V> {
    /// Creates a new KeyValues from a vector of entries.
    pub fn new(entries: Vec<(String, V)>) -> Self {
        Self(entries)
    }

    /// Returns a slice of all entries.
    pub fn entries(&self) -> &[(String, V)] {
        &self.0
    }

    /// Consumes self and returns the inner vector.
    pub fn into_entries(self) -> Vec<(String, V)> {
        self.0
    }

    /// Returns an iterator over borrowed key-value pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &V)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// Returns an iterator over just the keys.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(|(k, _)| k.as_str())
    }

    /// Returns the number of entries.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns true if there are no entries.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

// ---------------------------------------------------------------------------------------------- //
// Shared shape operations

/// Reads a map or list of exact pairs using the supplied reader for each value.
///
/// Map keys become entry keys. Pair keys use native string decoding, and pair order and duplicate
/// keys are preserved. Missing callback error locations gain the actual value Node's full path.
/// The callback consumes accepted leaves through typed decoding or [`Node::read_leaf`].
pub fn read_with<V>(
    node: &Node,
    mut read_value: impl FnMut(&Node) -> Result<V, NodeError>,
) -> Result<KeyValues<V>, NodeError> {
    match &node.kind {
        Kind::Map(map) => {
            let mut entries = Vec::with_capacity(map.len());
            for (key, child) in map {
                let value = read_value(child).map_err(|error| error.at_path(&child.path))?;
                entries.push((key.clone(), value));
            }
            Ok(KeyValues::new(entries))
        }
        Kind::Vec(_) => read::vec(node, |entry| {
            // Pair shape errors retain the established KeyValues diagnostic.
            let pair = entry
                .as_vec()
                .map_err(|_| NodeError::invalid_value(&entry.path, "expected [key, value] pair"))?;
            if pair.len() != 2 {
                return Err(NodeError::array_length(&entry.path, 2, pair.len()));
            }
            let key = pair[0].as_type::<String>()?;
            let value = read_value(&pair[1]).map_err(|error| error.at_path(&pair[1].path))?;
            Ok((key, value))
        })
        .map(KeyValues::new),
        Kind::Leaf(_) => Err(NodeError::invalid_value(
            &node.path,
            "expected map {k: v} or list of pairs [[k, v], ...]",
        )),
    }
}

/// Writes borrowed entries as pairs using the supplied writer for each value.
///
/// Preserves ordering and duplicate keys without constructing an owned representation collection.
/// A value error receives its emitted `[entry][1]` position before any enclosing field location.
pub fn write_with<V>(
    values: &KeyValues<V>,
    mut write_value: impl FnMut(&V) -> Result<Node, NodeError>,
) -> Result<Node, NodeError> {
    write::list(values.entries(), |entry| write::tuple2(entry, String::to_node, &mut write_value))
}

/// Builds the key-value label from the supplied value description.
///
/// Retains the container's existing label convention, including its fallback for unlabelled values.
pub fn description(value: Desc) -> Desc {
    let inner = value.type_label();
    let value_part = if inner.is_empty() { "value" } else { &inner };
    Desc::plain(format!("key_values[string → {}]", value_part))
}

// ---------------------------------------------------------------------------------------------- //
// Iterator implementations

impl<V> IntoIterator for KeyValues<V> {
    type Item = (String, V);
    type IntoIter = std::vec::IntoIter<(String, V)>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a, V> IntoIterator for &'a KeyValues<V> {
    type Item = &'a (String, V);
    type IntoIter = std::slice::Iter<'a, (String, V)>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl<V> FromIterator<(String, V)> for KeyValues<V> {
    fn from_iter<I: IntoIterator<Item = (String, V)>>(iter: I) -> Self {
        Self(iter.into_iter().collect())
    }
}

// ---------------------------------------------------------------------------------------------- //
// Scry trait implementations

impl<V: FromNode> FromNode for KeyValues<V> {
    fn from_node(node: &Node) -> Result<Self, NodeError> {
        read_with(node, Node::as_type)
    }
}

impl<V: Describe> Describe for KeyValues<V> {
    fn describe() -> Desc {
        description(V::describe())
    }
}

impl<V: ToNode> ToNode for KeyValues<V> {
    fn to_node(&self) -> Result<Node, NodeError> {
        write_with(self, ToNode::to_node)
    }
}
