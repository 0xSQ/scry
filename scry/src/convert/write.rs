//! Writes configuration shapes with explicitly supplied borrowed child writers.
//!
//! Callbacks return errors relative to their emitted values. Lists and tuples prepend their
//! actual output positions once, while optional values delegate transparently. Successful Nodes
//! keep their supplied paths until insertion through [`Node::set_node`] anchors the subtree.
//! Helpers stop at the first failure and never clone stored values or read them back from Nodes.

use crate::node::Value;
use crate::{KeyPath, Node, NodeError};

// ---------------------------------------------------------------------------------------------- //

/// Writes borrowed elements as an array using the supplied writer for each element.
///
/// Accepts vectors, fixed arrays, and slices through their borrowed slice view. Preserves order
/// and duplicates and prepends the failing element's index to a child error.
pub fn list<T>(
    values: &[T],
    mut write_element: impl FnMut(&T) -> Result<Node, NodeError>,
) -> Result<Node, NodeError> {
    let mut children = Vec::with_capacity(values.len());
    for (index, item) in values.iter().enumerate() {
        children.push(
            write_element(item).map_err(|error| error.prepend_path(&KeyPath::from_index(index)))?,
        );
    }
    Ok(Node::new_vec(KeyPath::new(), children))
}

/// Writes `None` as null or delegates the complete borrowed `Some` value to its writer.
///
/// Does not invoke the callback for `None`. Delegation preserves child error paths without adding
/// an artificial position. Root null is permitted.
pub fn option<T>(
    value: &Option<T>,
    mut write_value: impl FnMut(&T) -> Result<Node, NodeError>,
) -> Result<Node, NodeError> {
    match value {
        Some(value) => write_value(value),
        None => Ok(Node::new_leaf(KeyPath::new(), Value::Null)),
    }
}

/// Writes a borrowed pair using a separately typed writer for each position.
///
/// Emits an array and prepends each child's position to its relative error path.
pub fn tuple2<A, B>(
    value: &(A, B),
    mut write_a: impl FnMut(&A) -> Result<Node, NodeError>,
    mut write_b: impl FnMut(&B) -> Result<Node, NodeError>,
) -> Result<Node, NodeError> {
    Ok(Node::new_vec(
        KeyPath::new(),
        vec![
            write_a(&value.0).map_err(|error| error.prepend_path(&KeyPath::from_index(0)))?,
            write_b(&value.1).map_err(|error| error.prepend_path(&KeyPath::from_index(1)))?,
        ],
    ))
}

/// Writes a borrowed triple using a separately typed writer for each position.
///
/// Emits an array and prepends each child's position to its relative error path.
pub fn tuple3<A, B, C>(
    value: &(A, B, C),
    mut write_a: impl FnMut(&A) -> Result<Node, NodeError>,
    mut write_b: impl FnMut(&B) -> Result<Node, NodeError>,
    mut write_c: impl FnMut(&C) -> Result<Node, NodeError>,
) -> Result<Node, NodeError> {
    Ok(Node::new_vec(
        KeyPath::new(),
        vec![
            write_a(&value.0).map_err(|error| error.prepend_path(&KeyPath::from_index(0)))?,
            write_b(&value.1).map_err(|error| error.prepend_path(&KeyPath::from_index(1)))?,
            write_c(&value.2).map_err(|error| error.prepend_path(&KeyPath::from_index(2)))?,
        ],
    ))
}

/// Writes a borrowed quadruple using a separately typed writer for each position.
///
/// Emits an array and prepends each child's position to its relative error path.
pub fn tuple4<A, B, C, D>(
    value: &(A, B, C, D),
    mut write_a: impl FnMut(&A) -> Result<Node, NodeError>,
    mut write_b: impl FnMut(&B) -> Result<Node, NodeError>,
    mut write_c: impl FnMut(&C) -> Result<Node, NodeError>,
    mut write_d: impl FnMut(&D) -> Result<Node, NodeError>,
) -> Result<Node, NodeError> {
    Ok(Node::new_vec(
        KeyPath::new(),
        vec![
            write_a(&value.0).map_err(|error| error.prepend_path(&KeyPath::from_index(0)))?,
            write_b(&value.1).map_err(|error| error.prepend_path(&KeyPath::from_index(1)))?,
            write_c(&value.2).map_err(|error| error.prepend_path(&KeyPath::from_index(2)))?,
            write_d(&value.3).map_err(|error| error.prepend_path(&KeyPath::from_index(3)))?,
        ],
    ))
}
