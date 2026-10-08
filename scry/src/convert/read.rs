//! Reads configuration shapes with explicitly supplied child readers.
//!
//! Callbacks receive Nodes with their complete logical input locations. Existing error paths and
//! causes are preserved. Otherwise, the helper attaches the failing child's location. Accepted
//! leaves must be consumed by the callback through typed decoding or [`Node::read_leaf`]. Helpers
//! stop at the first failure and do not read later children or mark whole containers consumed.

use crate::node::{Kind, Value};
use crate::{Node, NodeError};

// ---------------------------------------------------------------------------------------------- //

/// Reads an array into a vector using the supplied reader for each element.
///
/// Preserves input order and duplicates. The callback can maintain state across child reads.
pub fn vec<T>(
    node: &Node,
    mut read_element: impl FnMut(&Node) -> Result<T, NodeError>,
) -> Result<Vec<T>, NodeError> {
    let entries = node.as_vec()?;
    let mut values = Vec::with_capacity(entries.len());
    for entry in entries {
        values.push(read_element(entry).map_err(|error| error.at_path(&entry.path))?);
    }
    Ok(values)
}

/// Reads null as `None` or delegates the complete non-null value to the supplied reader.
///
/// Consumes null without invoking the callback. Missing keys are handled by the enclosing field
/// decoder, rather than this value operation. Non-null child errors retain their complete paths.
pub fn option<T>(
    node: &Node,
    mut read_value: impl FnMut(&Node) -> Result<T, NodeError>,
) -> Result<Option<T>, NodeError> {
    if let Kind::Leaf(leaf) = &node.kind {
        if matches!(leaf.value, Value::Null) {
            node.read_leaf("optional value")?;
            return Ok(None);
        }
    }
    read_value(node).map(Some).map_err(|error| error.at_path(&node.path))
}

/// Reads an exact-length array using the supplied reader for each element.
///
/// Checks the length before invoking any callback, including for zero-length arrays. Does not
/// require `Default`, `Clone`, or native Scry support for the element type.
pub fn array<T, const N: usize>(
    node: &Node,
    read_element: impl FnMut(&Node) -> Result<T, NodeError>,
) -> Result<[T; N], NodeError> {
    exact_entries(node, N)?;
    let values = vec(node, read_element)?;
    match values.try_into() {
        Ok(array) => Ok(array),
        Err(_) => unreachable!("the array length was checked before reading its elements"),
    }
}

/// Reads an exact pair using a separately typed reader for each position.
///
/// Checks the length before invoking either reader. Readers run from left to right.
pub fn tuple2<A, B>(
    node: &Node,
    mut read_a: impl FnMut(&Node) -> Result<A, NodeError>,
    mut read_b: impl FnMut(&Node) -> Result<B, NodeError>,
) -> Result<(A, B), NodeError> {
    let entries = exact_entries(node, 2)?;
    Ok((
        read_a(&entries[0]).map_err(|error| error.at_path(&entries[0].path))?,
        read_b(&entries[1]).map_err(|error| error.at_path(&entries[1].path))?,
    ))
}

/// Reads an exact triple using a separately typed reader for each position.
///
/// Checks the length before invoking any reader. Readers run from left to right.
pub fn tuple3<A, B, C>(
    node: &Node,
    mut read_a: impl FnMut(&Node) -> Result<A, NodeError>,
    mut read_b: impl FnMut(&Node) -> Result<B, NodeError>,
    mut read_c: impl FnMut(&Node) -> Result<C, NodeError>,
) -> Result<(A, B, C), NodeError> {
    let entries = exact_entries(node, 3)?;
    Ok((
        read_a(&entries[0]).map_err(|error| error.at_path(&entries[0].path))?,
        read_b(&entries[1]).map_err(|error| error.at_path(&entries[1].path))?,
        read_c(&entries[2]).map_err(|error| error.at_path(&entries[2].path))?,
    ))
}

/// Reads an exact quadruple using a separately typed reader for each position.
///
/// Checks the length before invoking any reader. Readers run from left to right.
pub fn tuple4<A, B, C, D>(
    node: &Node,
    mut read_a: impl FnMut(&Node) -> Result<A, NodeError>,
    mut read_b: impl FnMut(&Node) -> Result<B, NodeError>,
    mut read_c: impl FnMut(&Node) -> Result<C, NodeError>,
    mut read_d: impl FnMut(&Node) -> Result<D, NodeError>,
) -> Result<(A, B, C, D), NodeError> {
    let entries = exact_entries(node, 4)?;
    Ok((
        read_a(&entries[0]).map_err(|error| error.at_path(&entries[0].path))?,
        read_b(&entries[1]).map_err(|error| error.at_path(&entries[1].path))?,
        read_c(&entries[2]).map_err(|error| error.at_path(&entries[2].path))?,
        read_d(&entries[3]).map_err(|error| error.at_path(&entries[3].path))?,
    ))
}

/// Returns the input entries after validating their exact count.
fn exact_entries(node: &Node, expected: usize) -> Result<&[Node], NodeError> {
    let entries = node.as_vec()?;
    if entries.len() != expected {
        return Err(NodeError::array_length(&node.path, expected, entries.len()));
    }
    Ok(entries)
}
