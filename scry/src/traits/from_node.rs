//! The [`FromNode`] trait and implementations for primitive types.

use std::path::PathBuf;

use crate::node::{Kind, Node, NodeError, Value};

// ---------------------------------------------------------------------------------------------- //

/// Parses a value from a Node tree.
///
/// Use `#[derive(scry::FromNode)]` or `#[derive(scry::Config)]` to generate implementations.
///
/// Implementations must consume accepted leaves through [`Node::read_leaf`] or typed decoding,
/// including on direct trait calls, so [`Node::ensure_no_unknown_keys`] can detect unread input.
/// A strict map decoder validates its immediate keys with [`Node::ensure_only_keys`]. Parent
/// decoders validate their own keys and do not inspect the interiors of accepted child values.
/// Errors with a logical configuration path use the Node's full path, including inside containers.
/// [`Node::as_type`] attaches that path to errors that have no logical location.
pub trait FromNode: Sized {
    /// Parses a Node into this type.
    fn from_node(node: &Node) -> Result<Self, NodeError>;
}

// ---------------------------------------------------------------------------------------------- //
// Node

impl FromNode for Node {
    fn from_node(node: &Node) -> Result<Self, NodeError> {
        // Raw Node fields accept their whole subtree. Mark its leaves for explicit input audits.
        mark_node_visited(node)?;
        Ok(node.clone())
    }
}

fn mark_node_visited(node: &Node) -> Result<(), NodeError> {
    match &node.kind {
        Kind::Leaf(_) => {
            node.read_leaf("raw node")?;
        }
        Kind::Vec(children) => {
            for child in children {
                mark_node_visited(child)?;
            }
        }
        Kind::Map(children) => {
            for child in children.values() {
                mark_node_visited(child)?;
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------- //
// Floating Point

macro_rules! impl_from_node_float {
    ($($t:ty),+ $(,)?) => {
        $(
            impl FromNode for $t {
                fn from_node(node: &Node) -> Result<Self, NodeError> {
                    let value = node.read_leaf(stringify!($t))?;
                    let result = match value {
                        Value::String(s) => s.parse::<$t>()
                            .map_err(|error| conversion_error_with_source(node, stringify!($t), value, error))?,
                        Value::I8(v) => *v as $t,
                        Value::I16(v) => *v as $t,
                        Value::I32(v) => *v as $t,
                        Value::I64(v) => *v as $t,
                        Value::U8(v) => *v as $t,
                        Value::U16(v) => *v as $t,
                        Value::U32(v) => *v as $t,
                        Value::U64(v) => *v as $t,
                        Value::F32(v) => *v as $t,
                        Value::F64(v) => *v as $t,
                        _ => return Err(conversion_error(node, stringify!($t), value)),
                    };
                    Ok(result)
                }
            }
        )+
    };
}

impl_from_node_float!(f32, f64);

// ---------------------------------------------------------------------------------------------- //
// Integers

macro_rules! impl_from_node_int {
    ($($t:ty),+ $(,)?) => {
        $(
            impl FromNode for $t {
                fn from_node(node: &Node) -> Result<Self, NodeError> {
                    let value = node.read_leaf(stringify!($t))?;
                    let result = match value {
                        Value::String(s) => s.parse::<$t>()
                            .map_err(|error| conversion_error_with_source(node, stringify!($t), value, error))?,
                        Value::I8(v) => <$t>::try_from(*v)
                            .map_err(|error| conversion_error_with_source(node, stringify!($t), value, error))?,
                        Value::I16(v) => <$t>::try_from(*v)
                            .map_err(|error| conversion_error_with_source(node, stringify!($t), value, error))?,
                        Value::I32(v) => <$t>::try_from(*v)
                            .map_err(|error| conversion_error_with_source(node, stringify!($t), value, error))?,
                        Value::I64(v) => <$t>::try_from(*v)
                            .map_err(|error| conversion_error_with_source(node, stringify!($t), value, error))?,
                        Value::U8(v) => <$t>::try_from(*v)
                            .map_err(|error| conversion_error_with_source(node, stringify!($t), value, error))?,
                        Value::U16(v) => <$t>::try_from(*v)
                            .map_err(|error| conversion_error_with_source(node, stringify!($t), value, error))?,
                        Value::U32(v) => <$t>::try_from(*v)
                            .map_err(|error| conversion_error_with_source(node, stringify!($t), value, error))?,
                        Value::U64(v) => <$t>::try_from(*v)
                            .map_err(|error| conversion_error_with_source(node, stringify!($t), value, error))?,
                        _ => return Err(conversion_error(node, stringify!($t), value)),
                    };
                    Ok(result)
                }
            }
        )+
    };
}

impl_from_node_int!(i8, i16, i32, i64, isize);
impl_from_node_int!(u8, u16, u32, u64, usize);

// ---------------------------------------------------------------------------------------------- //
// Bool, String, PathBuf

impl FromNode for bool {
    fn from_node(node: &Node) -> Result<Self, NodeError> {
        let value = node.read_leaf("bool")?;
        match value {
            Value::Bool(v) => Ok(*v),
            Value::String(s) => s
                .parse::<bool>()
                .map_err(|error| conversion_error_with_source(node, "bool", value, error)),
            _ => Err(conversion_error(node, "bool", value)),
        }
    }
}

impl FromNode for String {
    fn from_node(node: &Node) -> Result<Self, NodeError> {
        let value = node.read_leaf("string")?;
        match value {
            Value::String(s) => Ok(s.clone()),
            _ => Err(conversion_error(node, "string", value)),
        }
    }
}

impl FromNode for PathBuf {
    fn from_node(node: &Node) -> Result<Self, NodeError> {
        String::from_node(node).map(PathBuf::from)
    }
}

// ---------------------------------------------------------------------------------------------- //
// Option, Vec, Arrays

impl<T: FromNode> FromNode for Option<T> {
    fn from_node(node: &Node) -> Result<Self, NodeError> {
        // Null represents None and must be consumed even on direct trait calls.
        if let Kind::Leaf(leaf) = &node.kind {
            if matches!(leaf.value, Value::Null) {
                node.read_leaf("optional value")?;
                return Ok(None);
            }
        }
        // Otherwise parse as T.
        node.as_type().map(Some)
    }
}

impl<T: FromNode> FromNode for Vec<T> {
    fn from_node(node: &Node) -> Result<Self, NodeError> {
        parse_vec(node, Node::as_type)
    }
}

impl<T: FromNode, const N: usize> FromNode for [T; N] {
    fn from_node(node: &Node) -> Result<Self, NodeError> {
        let entries = node.as_vec()?;
        if entries.len() != N {
            return Err(NodeError::array_length(&node.path, N, entries.len()));
        }
        let mut values = Vec::with_capacity(N);
        for entry in entries {
            values.push(entry.as_type()?);
        }
        // This cannot fail since we checked the length above
        Ok(values.try_into().ok().unwrap())
    }
}

// ---------------------------------------------------------------------------------------------- //
// Tuples

impl<A: FromNode, B: FromNode> FromNode for (A, B) {
    fn from_node(node: &Node) -> Result<Self, NodeError> {
        let vec = node.as_vec()?;
        if vec.len() != 2 {
            return Err(NodeError::array_length(&node.path, 2, vec.len()));
        }
        Ok((vec[0].as_type()?, vec[1].as_type()?))
    }
}

impl<A: FromNode, B: FromNode, C: FromNode> FromNode for (A, B, C) {
    fn from_node(node: &Node) -> Result<Self, NodeError> {
        let vec = node.as_vec()?;
        if vec.len() != 3 {
            return Err(NodeError::array_length(&node.path, 3, vec.len()));
        }
        Ok((vec[0].as_type()?, vec[1].as_type()?, vec[2].as_type()?))
    }
}

impl<A: FromNode, B: FromNode, C: FromNode, D: FromNode> FromNode for (A, B, C, D) {
    fn from_node(node: &Node) -> Result<Self, NodeError> {
        let vec = node.as_vec()?;
        if vec.len() != 4 {
            return Err(NodeError::array_length(&node.path, 4, vec.len()));
        }
        Ok((vec[0].as_type()?, vec[1].as_type()?, vec[2].as_type()?, vec[3].as_type()?))
    }
}

// ---------------------------------------------------------------------------------------------- //
// Sequence Traversal

/// Parses each array element directly into the final target vector.
pub(crate) fn parse_vec<T>(
    node: &Node,
    mut parse_element: impl FnMut(&Node) -> Result<T, NodeError>,
) -> Result<Vec<T>, NodeError> {
    let entries = node.as_vec()?;
    let mut result = Vec::with_capacity(entries.len());
    for entry in entries {
        result.push(parse_element(entry).map_err(|error| error.at_path(&entry.path))?);
    }
    Ok(result)
}

// ---------------------------------------------------------------------------------------------- //
// Conversion Failures

/// Creates a conversion error for invalid values.
fn conversion_error(node: &Node, target_type: &str, source_value: &Value) -> NodeError {
    NodeError::invalid_conversion(
        &node.path,
        target_type,
        source_value.type_name(),
        &source_value.to_string(),
    )
}

/// Creates a conversion error while preserving the underlying parser or range failure.
fn conversion_error_with_source(
    node: &Node,
    target_type: &str,
    source_value: &Value,
    source: impl Into<crate::BoxedError>,
) -> NodeError {
    NodeError::invalid_conversion_with_source(
        &node.path,
        target_type,
        source_value.type_name(),
        &source_value.to_string(),
        source,
    )
}
