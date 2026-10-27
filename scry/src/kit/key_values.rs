//! Key-value collection type for interface boundaries.
//!
//! Provides [`KeyValues<V>`], an ordered collection of values with unique string keys
//! that accepts config input as either a map or a list of pairs.

use std::collections::HashMap;
use std::iter::FusedIterator;

use crate::convert::write;
use crate::desc::Desc;
use crate::node::{Kind, Node, NodeError};
use crate::traits::{Describe, FromNode, ToNode};

// ---------------------------------------------------------------------------------------------- //

/// An ordered collection of uniquely named values for configuration interfaces.
///
/// Accepts config input as either:
/// - a map/object (ergonomic): `{ "a": 1, "b": 2 }`
/// - or a list of pairs (explicit): `[["a", 1], ["b", 2]]`
///
/// Keys use exact string equality, without trimming, case folding, or Unicode normalization.
/// Empty keys are allowed. Pair input preserves its supplied order. Object input preserves the
/// order encountered in the decoded Node, which need not match source-text order. A format parser
/// may already have collapsed duplicate object keys before decoding this collection.
///
/// Equality includes entry order. Writing always produces an ordered list of pairs, including
/// `[]` for an empty collection.
///
/// # Construction
///
/// Empty construction is infallible. Bulk construction rejects duplicate keys:
///
/// ```
/// use scry::kit::KeyValues;
/// use scry::kit::key_values::DuplicateKeyError;
///
/// let empty = KeyValues::<u32>::new();
/// assert!(empty.is_empty());
///
/// let weights = KeyValues::try_from(vec![("preview".to_owned(), 640)])?;
/// assert_eq!(weights.get("preview"), Some(&640));
///
/// let more = KeyValues::try_from_iter([("full".to_owned(), 1920)])?;
/// let entries: Vec<_> = more.into_iter().collect();
/// assert_eq!(entries, [("full".to_owned(), 1920)]);
/// # Ok::<(), DuplicateKeyError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyValues<V>(Vec<(String, V)>);

impl<V> KeyValues<V> {
    /// Creates an empty collection.
    pub const fn new() -> Self {
        Self(Vec::new())
    }

    /// Collects uniquely named entries in iteration order.
    ///
    /// Stops at the first duplicate without requesting any later entries from the iterator.
    ///
    /// # Errors
    ///
    /// Returns the repeated key and its first and duplicate zero-based entry indices.
    pub fn try_from_iter<I>(iter: I) -> Result<Self, DuplicateKeyError>
    where
        I: IntoIterator<Item = (String, V)>,
    {
        let mut entries = Vec::new();
        let mut first_indices = HashMap::new();
        for (index, (key, value)) in iter.into_iter().enumerate() {
            check_key(&mut first_indices, &key, index)?;
            entries.push((key, value));
        }
        Ok(Self(entries))
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
    pub fn iter(&self) -> Iter<'_, V> {
        Iter(self.0.iter())
    }

    /// Returns an iterator over just the keys.
    pub fn keys(
        &self,
    ) -> impl DoubleEndedIterator<Item = &str> + ExactSizeIterator + FusedIterator {
        self.0.iter().map(|(k, _)| k.as_str())
    }

    /// Returns the value associated with the exact key.
    ///
    /// Lookup scans entries in order and takes linear time in the collection's length.
    pub fn get(&self, key: &str) -> Option<&V> {
        self.0.iter().find(|(name, _)| name == key).map(|(_, value)| value)
    }

    /// Returns whether the collection contains the exact key.
    ///
    /// Like [`Self::get`], this performs a linear scan.
    pub fn contains_key(&self, key: &str) -> bool {
        self.get(key).is_some()
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

impl<V> Default for KeyValues<V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<V> TryFrom<Vec<(String, V)>> for KeyValues<V> {
    type Error = DuplicateKeyError;

    fn try_from(entries: Vec<(String, V)>) -> Result<Self, Self::Error> {
        let mut first_indices = HashMap::with_capacity(entries.len());
        for (index, (key, _)) in entries.iter().enumerate() {
            if let Some(first_index) = first_indices.insert(key.as_str(), index) {
                return Err(DuplicateKeyError {
                    key: key.clone(),
                    first_index,
                    duplicate_index: index,
                });
            }
        }
        Ok(Self(entries))
    }
}

/// Reports a repeated key during checked construction or configuration decoding.
///
/// Entry indices are zero-based positions in the supplied sequence.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("duplicate key {key:?} at entry {duplicate_index} (first seen at entry {first_index})")]
pub struct DuplicateKeyError {
    key: String,
    first_index: usize,
    duplicate_index: usize,
}

impl DuplicateKeyError {
    /// Returns the repeated key.
    pub fn key(&self) -> &str {
        &self.key
    }

    /// Returns the zero-based index of the first entry with this key.
    pub fn first_index(&self) -> usize {
        self.first_index
    }

    /// Returns the zero-based index of the rejected duplicate entry.
    pub fn duplicate_index(&self) -> usize {
        self.duplicate_index
    }
}

/// Iterates over borrowed names and values in entry order.
#[derive(Debug)]
pub struct Iter<'a, V>(std::slice::Iter<'a, (String, V)>);

impl<V> Clone for Iter<'_, V> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<'a, V> Iterator for Iter<'a, V> {
    type Item = (&'a str, &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(|(key, value)| (key.as_str(), value))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}

impl<V> DoubleEndedIterator for Iter<'_, V> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.0.next_back().map(|(key, value)| (key.as_str(), value))
    }
}

impl<V> ExactSizeIterator for Iter<'_, V> {}

impl<V> FusedIterator for Iter<'_, V> {}

// ---------------------------------------------------------------------------------------------- //
// Shared shape operations

/// Reads a map or list of exact pairs using the supplied reader for each value.
///
/// Map keys become entry keys. Pair keys use native string decoding and must be unique. A duplicate
/// fails at its second key before its value reader runs, retaining [`DuplicateKeyError`] as the
/// cause. Earlier value failures take precedence over later duplicates.
///
/// Preserves pair order and encountered map order. Missing callback error locations gain the actual
/// value Node's full path. The callback consumes accepted leaves through typed decoding or
/// [`Node::read_leaf`].
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
            // Node maps already have unique keys.
            Ok(KeyValues(entries))
        }
        Kind::Vec(pairs) => {
            let mut entries = Vec::with_capacity(pairs.len());
            let mut first_indices = HashMap::new();
            for (index, entry) in pairs.iter().enumerate() {
                let pair = entry.as_vec().map_err(|_| {
                    NodeError::invalid_value(&entry.path, "expected [key, value] pair")
                })?;
                if pair.len() != 2 {
                    return Err(NodeError::array_length(&entry.path, 2, pair.len()));
                }
                let key = pair[0].as_type::<String>()?;
                check_key(&mut first_indices, &key, index).map_err(|error| {
                    NodeError::invalid_value_with_source(&pair[0].path, error.to_string(), error)
                })?;
                let value = read_value(&pair[1]).map_err(|error| error.at_path(&pair[1].path))?;
                entries.push((key, value));
            }
            Ok(KeyValues(entries))
        }
        Kind::Leaf(_) => Err(NodeError::invalid_value(
            &node.path,
            "expected map {k: v} or list of pairs [[k, v], ...]",
        )),
    }
}

/// Writes borrowed entries as pairs using the supplied writer for each value.
///
/// Preserves entry order without constructing an owned representation collection.
/// A value error receives its emitted `[entry][1]` position before any enclosing field location.
pub fn write_with<V>(
    values: &KeyValues<V>,
    mut write_value: impl FnMut(&V) -> Result<Node, NodeError>,
) -> Result<Node, NodeError> {
    write::list(values.entries(), |entry| write::tuple2(entry, String::to_node, &mut write_value))
}

/// Builds the key-value label from the supplied value description.
///
/// Uses `value` when the supplied description has no type label.
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
    type Item = (&'a str, &'a V);
    type IntoIter = Iter<'a, V>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
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

// ---------------------------------------------------------------------------------------------- //

/// Records a key's first position or reports its duplicate.
fn check_key(
    first_indices: &mut HashMap<String, usize>,
    key: &str,
    index: usize,
) -> Result<(), DuplicateKeyError> {
    if let Some(&first_index) = first_indices.get(key) {
        return Err(DuplicateKeyError {
            key: key.to_owned(),
            first_index,
            duplicate_index: index,
        });
    }
    first_indices.insert(key.to_owned(), index);
    Ok(())
}
