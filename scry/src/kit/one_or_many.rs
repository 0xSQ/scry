//! One-or-many collection type for config fields.
//!
//! Provides [`OneOrMany<T>`], a collection that accepts either a single value
//! or an array in config, allowing ergonomic shorthand for single-element lists.

use std::ops::Deref;

use crate::desc::Desc;
use crate::key_path::KeyPath;
use crate::node::{Kind, Node, NodeError};
use crate::traits::{Describe, FromNode, ToNode};

// ---------------------------------------------------------------------------------------------- //

/// A collection that accepts either a single value or an array in config.
///
/// Use this for config fields where users commonly specify just one item,
/// but may occasionally need multiple. The shorthand works for any element
/// type - scalars, maps, or objects.
///
/// # Config examples
///
/// Single scalar value (shorthand):
/// ```rhai
/// include: "*.txt"
/// ```
///
/// Single map value (shorthand):
/// ```rhai
/// include: #{ path: "specific/file.txt" }
/// ```
///
/// Multiple values (full form):
/// ```rhai
/// include: ["*.txt", "*.md"]
/// ```
///
/// # Canonical output
///
/// When serialized (e.g., via `--get`), `OneOrMany<T>` always emits an array,
/// even for single elements. This ensures stable round-trip behavior.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OneOrMany<T>(Vec<T>);

impl<T> Default for OneOrMany<T> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

impl<T> OneOrMany<T> {
    /// Creates a new `OneOrMany` from a vector.
    pub fn new(items: Vec<T>) -> Self {
        Self(items)
    }

    /// Creates a `OneOrMany` containing a single item.
    pub fn one(item: T) -> Self {
        Self(vec![item])
    }

    /// Consumes self and returns the inner vector.
    pub fn into_vec(self) -> Vec<T> {
        self.0
    }

    /// Returns a slice of all items.
    pub fn as_slice(&self) -> &[T] {
        &self.0
    }

    /// Returns an iterator over borrowed items.
    pub fn iter(&self) -> std::slice::Iter<'_, T> {
        self.0.iter()
    }

    /// Returns the number of items.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns true if there are no items.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

// ---------------------------------------------------------------------------------------------- //
// Deref to slice

impl<T> Deref for OneOrMany<T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> AsRef<[T]> for OneOrMany<T> {
    fn as_ref(&self) -> &[T] {
        &self.0
    }
}

// ---------------------------------------------------------------------------------------------- //
// Iterator implementations

impl<T> IntoIterator for OneOrMany<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a, T> IntoIterator for &'a OneOrMany<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl<T> FromIterator<T> for OneOrMany<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        Self(iter.into_iter().collect())
    }
}

// ---------------------------------------------------------------------------------------------- //
// Scry trait implementations

impl<T: FromNode> FromNode for OneOrMany<T> {
    fn from_node(node: &Node) -> Result<Self, NodeError> {
        if node.kind.is_vec() {
            // Array form: parse as Vec<T>.
            Ok(OneOrMany::new(Vec::<T>::from_node(node)?))
        } else {
            // Non-array (scalar, map, etc.): parse as single T.
            Ok(OneOrMany::one(T::from_node(node)?))
        }
    }
}

impl<T: Describe> Describe for OneOrMany<T> {
    fn describe() -> Desc {
        let inner = T::describe().type_label();
        let type_part = if inner.is_empty() { "value" } else { &inner };
        Desc::plain(format!("{}…", type_part))
    }
}

impl<T: ToNode> ToNode for OneOrMany<T> {
    fn to_node(&self) -> Result<Node, NodeError> {
        // Always emit as array for canonical output.
        let items: Vec<Node> =
            self.0.iter().map(|item| item.to_node()).collect::<Result<_, _>>()?;
        Ok(Node {
            path: KeyPath::new(),
            kind: Kind::Vec(items),
        })
    }
}

// ---------------------------------------------------------------------------------------------- //

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kit::key_values::KeyValues;
    use crate::node::Format;

    // ------------------------------------------------------------------------------------------ //
    // Parsing tests

    #[test]
    fn parse_single_string() {
        let node = Node::parse_str("\"hello\"", Format::Rhai).unwrap();
        let result: OneOrMany<String> = node.as_type().unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "hello");
    }

    #[test]
    fn parse_single_number() {
        let node = Node::parse_str("42", Format::Rhai).unwrap();
        let result: OneOrMany<i64> = node.as_type().unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], 42);
    }

    #[test]
    fn parse_single_map() {
        // Critical test: non-leaf single value should work.
        // KeyValues<i64> parses from a map: #{ key: 42 } -> [("key", 42)]
        let node = Node::parse_str("#{ key: 42 }", Format::Rhai).unwrap();
        let result: OneOrMany<KeyValues<i64>> = node.as_type().unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].len(), 1);
    }

    #[test]
    fn parse_array_of_strings() {
        let node = Node::parse_str("[\"a\", \"b\", \"c\"]", Format::Rhai).unwrap();
        let result: OneOrMany<String> = node.as_type().unwrap();
        assert_eq!(result.len(), 3);
        assert_eq!(result.as_slice(), &["a", "b", "c"]);
    }

    #[test]
    fn parse_empty_array() {
        let node = Node::parse_str("[]", Format::Rhai).unwrap();
        let result: OneOrMany<String> = node.as_type().unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn parse_array_of_maps() {
        // Array of maps - each map parses as KeyValues<i64>.
        let node = Node::parse_str("[#{ a: 1 }, #{ b: 2 }]", Format::Rhai).unwrap();
        let result: OneOrMany<KeyValues<i64>> = node.as_type().unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].len(), 1);
        assert_eq!(result[1].len(), 1);
    }

    // ------------------------------------------------------------------------------------------ //
    // Encoding tests

    #[test]
    fn encode_single_as_array() {
        let one: OneOrMany<String> = OneOrMany::one("hello".to_string());
        let node = one.to_node().unwrap();
        assert!(node.kind.is_vec());
        let vec = node.as_vec().unwrap();
        assert_eq!(vec.len(), 1);
    }

    #[test]
    fn encode_many_as_array() {
        let many: OneOrMany<i64> = OneOrMany::new(vec![1, 2, 3]);
        let node = many.to_node().unwrap();
        assert!(node.kind.is_vec());
        let vec = node.as_vec().unwrap();
        assert_eq!(vec.len(), 3);
    }

    // ------------------------------------------------------------------------------------------ //
    // API tests

    #[test]
    fn default_is_empty() {
        let empty: OneOrMany<String> = OneOrMany::default();
        assert!(empty.is_empty());
        assert_eq!(empty.len(), 0);
    }

    #[test]
    fn into_vec_consumes() {
        let items: OneOrMany<i64> = OneOrMany::new(vec![1, 2, 3]);
        let vec = items.into_vec();
        assert_eq!(vec, vec![1, 2, 3]);
    }

    #[test]
    fn deref_to_slice() {
        let items: OneOrMany<i64> = OneOrMany::new(vec![10, 20]);
        let slice: &[i64] = &items;
        assert_eq!(slice, &[10, 20]);
    }

    #[test]
    fn iter_borrows() {
        let items: OneOrMany<i64> = OneOrMany::new(vec![1, 2, 3]);
        let sum: i64 = items.iter().sum();
        assert_eq!(sum, 6);
        // items still usable
        assert_eq!(items.len(), 3);
    }

    #[test]
    fn into_iterator_owned() {
        let items: OneOrMany<i64> = OneOrMany::new(vec![1, 2, 3]);
        let collected: Vec<i64> = items.into_iter().collect();
        assert_eq!(collected, vec![1, 2, 3]);
    }

    #[test]
    fn from_iterator() {
        let items: OneOrMany<i64> = [1, 2, 3].into_iter().collect();
        assert_eq!(items.len(), 3);
    }
}
