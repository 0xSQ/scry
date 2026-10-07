//! The [`ToNode`] trait and implementations for primitive types.

use std::path::PathBuf;

use crate::key_path::KeyPath;
use crate::node::{Kind, Leaf, Node, NodeError, Value};

// ---------------------------------------------------------------------------------------------- //

/// Serializes a value to a Node tree.
///
/// Use `#[derive(scry::ToNode)]` to generate implementations.
///
/// Implementations can construct nodes with empty paths. Inserting the result through
/// [`Node::set_node`] anchors the subtree at its destination.
///
/// Error paths are relative to the value being serialized. An implementation that nests a child
/// beneath a key or index uses [`NodeError::prepend_path`] to add that location to the child's
/// error path. Transparent wrappers return child errors unchanged. Successful nodes follow the
/// anchoring contract above.
pub trait ToNode {
    /// Converts this value to a Node.
    fn to_node(&self) -> Result<Node, NodeError>;
}

// ---------------------------------------------------------------------------------------------- //

/// Creates a leaf node with an empty path.
fn leaf(value: Value) -> Node {
    Node {
        path: KeyPath::new(),
        kind: Kind::Leaf(Leaf::new(value)),
    }
}

// ---------------------------------------------------------------------------------------------- //
// Node

impl ToNode for Node {
    fn to_node(&self) -> Result<Node, NodeError> {
        // Raw Node values serialize as identity-style passthrough subtrees.
        Ok(self.clone())
    }
}

// ---------------------------------------------------------------------------------------------- //
// Primitives

impl ToNode for () {
    fn to_node(&self) -> Result<Node, NodeError> {
        Ok(leaf(Value::Null))
    }
}

impl ToNode for bool {
    fn to_node(&self) -> Result<Node, NodeError> {
        Ok(leaf(Value::Bool(*self)))
    }
}

impl ToNode for String {
    fn to_node(&self) -> Result<Node, NodeError> {
        Ok(leaf(Value::String(self.clone())))
    }
}

impl ToNode for str {
    fn to_node(&self) -> Result<Node, NodeError> {
        Ok(leaf(Value::String(self.to_string())))
    }
}

impl ToNode for PathBuf {
    fn to_node(&self) -> Result<Node, NodeError> {
        Ok(leaf(Value::String(self.to_string_lossy().into_owned())))
    }
}

// ---------------------------------------------------------------------------------------------- //
// Floating Point

macro_rules! impl_to_node_float {
    ($($t:ty => $variant:ident),+ $(,)?) => {
        $(
            impl ToNode for $t {
                fn to_node(&self) -> Result<Node, NodeError> {
                    Ok(leaf(Value::$variant(*self)))
                }
            }
        )+
    };
}

impl_to_node_float!(f32 => F32, f64 => F64);

// ---------------------------------------------------------------------------------------------- //
// Integers

macro_rules! impl_to_node_int {
    ($($t:ty => $variant:ident),+ $(,)?) => {
        $(
            impl ToNode for $t {
                fn to_node(&self) -> Result<Node, NodeError> {
                    Ok(leaf(Value::$variant(*self)))
                }
            }
        )+
    };
}

impl_to_node_int!(
    i8 => I8, i16 => I16, i32 => I32, i64 => I64,
    u8 => U8, u16 => U16, u32 => U32, u64 => U64,
);

// usize/isize need conversion since Value doesn't have those variants
impl ToNode for usize {
    fn to_node(&self) -> Result<Node, NodeError> {
        Ok(leaf(Value::U64(*self as u64)))
    }
}

impl ToNode for isize {
    fn to_node(&self) -> Result<Node, NodeError> {
        Ok(leaf(Value::I64(*self as i64)))
    }
}

// ---------------------------------------------------------------------------------------------- //
// Option, Vec, Arrays

impl<T: ToNode> ToNode for Option<T> {
    fn to_node(&self) -> Result<Node, NodeError> {
        match self {
            Some(v) => v.to_node(),
            None => Ok(leaf(Value::Null)),
        }
    }
}

impl<T: ToNode> ToNode for Vec<T> {
    fn to_node(&self) -> Result<Node, NodeError> {
        serialize_vec(self, ToNode::to_node)
    }
}

impl<T: ToNode, const N: usize> ToNode for [T; N] {
    fn to_node(&self) -> Result<Node, NodeError> {
        let mut children = Vec::with_capacity(N);
        for (index, item) in self.iter().enumerate() {
            children.push(
                item.to_node().map_err(|error| error.prepend_path(&KeyPath::from_index(index)))?,
            );
        }
        Ok(Node {
            path: KeyPath::new(),
            kind: Kind::Vec(children),
        })
    }
}

// ---------------------------------------------------------------------------------------------- //
// Tuples

impl<A: ToNode, B: ToNode> ToNode for (A, B) {
    fn to_node(&self) -> Result<Node, NodeError> {
        Ok(Node {
            path: KeyPath::new(),
            kind: Kind::Vec(vec![
                self.0.to_node().map_err(|error| error.prepend_path(&KeyPath::from_index(0)))?,
                self.1.to_node().map_err(|error| error.prepend_path(&KeyPath::from_index(1)))?,
            ]),
        })
    }
}

impl<A: ToNode, B: ToNode, C: ToNode> ToNode for (A, B, C) {
    fn to_node(&self) -> Result<Node, NodeError> {
        Ok(Node {
            path: KeyPath::new(),
            kind: Kind::Vec(vec![
                self.0.to_node().map_err(|error| error.prepend_path(&KeyPath::from_index(0)))?,
                self.1.to_node().map_err(|error| error.prepend_path(&KeyPath::from_index(1)))?,
                self.2.to_node().map_err(|error| error.prepend_path(&KeyPath::from_index(2)))?,
            ]),
        })
    }
}

impl<A: ToNode, B: ToNode, C: ToNode, D: ToNode> ToNode for (A, B, C, D) {
    fn to_node(&self) -> Result<Node, NodeError> {
        Ok(Node {
            path: KeyPath::new(),
            kind: Kind::Vec(vec![
                self.0.to_node().map_err(|error| error.prepend_path(&KeyPath::from_index(0)))?,
                self.1.to_node().map_err(|error| error.prepend_path(&KeyPath::from_index(1)))?,
                self.2.to_node().map_err(|error| error.prepend_path(&KeyPath::from_index(2)))?,
                self.3.to_node().map_err(|error| error.prepend_path(&KeyPath::from_index(3)))?,
            ]),
        })
    }
}

// ---------------------------------------------------------------------------------------------- //
// References

impl<T: ToNode + ?Sized> ToNode for &T {
    fn to_node(&self) -> Result<Node, NodeError> {
        (*self).to_node()
    }
}

impl<T: ToNode + ?Sized> ToNode for &mut T {
    fn to_node(&self) -> Result<Node, NodeError> {
        (**self).to_node()
    }
}

// ---------------------------------------------------------------------------------------------- //
// Sequence Traversal

/// Serializes borrowed elements into the final child vector with relative index locations.
pub(crate) fn serialize_vec<T>(
    values: &[T],
    mut serialize_element: impl FnMut(&T) -> Result<Node, NodeError>,
) -> Result<Node, NodeError> {
    let mut children = Vec::with_capacity(values.len());
    for (index, item) in values.iter().enumerate() {
        children.push(
            serialize_element(item)
                .map_err(|error| error.prepend_path(&KeyPath::from_index(index)))?,
        );
    }
    Ok(Node::new_vec(KeyPath::new(), children))
}
