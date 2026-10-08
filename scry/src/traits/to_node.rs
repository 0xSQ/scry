//! The [`ToNode`] trait and implementations for primitive types.

use std::path::PathBuf;

use crate::convert::write;
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
        write::option(self, ToNode::to_node)
    }
}

impl<T: ToNode> ToNode for Vec<T> {
    fn to_node(&self) -> Result<Node, NodeError> {
        write::list(self, ToNode::to_node)
    }
}

impl<T: ToNode, const N: usize> ToNode for [T; N] {
    fn to_node(&self) -> Result<Node, NodeError> {
        write::list(self, ToNode::to_node)
    }
}

// ---------------------------------------------------------------------------------------------- //
// Tuples

impl<A: ToNode, B: ToNode> ToNode for (A, B) {
    fn to_node(&self) -> Result<Node, NodeError> {
        write::tuple2(self, ToNode::to_node, ToNode::to_node)
    }
}

impl<A: ToNode, B: ToNode, C: ToNode> ToNode for (A, B, C) {
    fn to_node(&self) -> Result<Node, NodeError> {
        write::tuple3(self, ToNode::to_node, ToNode::to_node, ToNode::to_node)
    }
}

impl<A: ToNode, B: ToNode, C: ToNode, D: ToNode> ToNode for (A, B, C, D) {
    fn to_node(&self) -> Result<Node, NodeError> {
        write::tuple4(self, ToNode::to_node, ToNode::to_node, ToNode::to_node, ToNode::to_node)
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
