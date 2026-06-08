//! Key-value collection type for interface boundaries.
//!
//! Provides [`KeyValues<V>`], an ordered collection of string-keyed entries
//! that accepts config input as either a map or a list of pairs.

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

use crate::desc::Desc;
use crate::key_path::KeyPath;
use crate::node::{Kind, Node, NodeError};
use crate::traits::{Describe, FromNode, ToNode};

// Accepts both map and list-of-pairs forms
impl<V: FromNode> FromNode for KeyValues<V> {
    fn from_node(node: &Node) -> Result<Self, NodeError> {
        match &node.kind {
            Kind::Map(map) => {
                let mut entries = Vec::with_capacity(map.len());
                for (key, child) in map {
                    let value = V::from_node(child)?;
                    entries.push((key.clone(), value));
                }
                Ok(KeyValues::new(entries))
            }
            Kind::Vec(vec) => {
                let mut entries = Vec::with_capacity(vec.len());
                for (i, elem) in vec.iter().enumerate() {
                    let elem_path = node.path.push_index(i);
                    let pair = elem.as_vec().map_err(|_| {
                        NodeError::invalid_value(&elem_path, "expected [key, value] pair")
                    })?;
                    if pair.len() != 2 {
                        return Err(NodeError::array_length(&elem_path, 2, pair.len()));
                    }
                    let key = String::from_node(&pair[0])?;
                    let value = V::from_node(&pair[1])?;
                    entries.push((key, value));
                }
                Ok(KeyValues::new(entries))
            }
            Kind::Leaf(_) => Err(NodeError::invalid_value(
                &node.path,
                "expected map {k: v} or list of pairs [[k, v], ...]",
            )),
        }
    }
}

impl<V: Describe> Describe for KeyValues<V> {
    fn describe() -> Desc {
        let inner = V::describe().type_label();
        let value_part = if inner.is_empty() { "value" } else { &inner };
        Desc::plain(format!("key_values[string → {}]", value_part))
    }
}

// Emits as list-of-pairs (preserves duplicates and ordering)
impl<V: ToNode> ToNode for KeyValues<V> {
    fn to_node(&self) -> Result<Node, NodeError> {
        let mut pairs = Vec::with_capacity(self.len());
        for (key, value) in self.iter() {
            let pair = Node {
                path: KeyPath::new(),
                kind: Kind::Vec(vec![key.to_node()?, value.to_node()?]),
            };
            pairs.push(pair);
        }
        Ok(Node {
            path: KeyPath::new(),
            kind: Kind::Vec(pairs),
        })
    }
}
