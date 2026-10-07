//! Explicit policies for converting and describing complete field values.
//!
//! A field selects a policy with `#[scry(via(Policy))]`. Each derive requests only its own
//! capability. Policies do not need instances, native target traits, or a shared representation.
//!
//! ```
//! use std::net::IpAddr;
//! use scry::{Config, Desc, DescribeVia, FromNodeVia, Node, NodeError};
//!
//! struct AddressText;
//!
//! impl FromNodeVia<IpAddr> for AddressText {
//!     fn from_node(node: &Node) -> Result<IpAddr, NodeError> {
//!         node.as_type::<String>()?.parse().map_err(|error| {
//!             NodeError::invalid_value_with_source(&node.path, "invalid IP address", error)
//!         })
//!     }
//! }
//!
//! impl DescribeVia<IpAddr> for AddressText {
//!     fn describe() -> Desc {
//!         Desc::plain("IP address string")
//!     }
//! }
//!
//! #[derive(Config)]
//! struct Server {
//!     #[scry(via(AddressText))]
//!     address: IpAddr,
//! }
//! ```
//!
//! Missing keys retain their ordinary field fallback. Present values, including null, reach the
//! policy as the complete field type. Via cannot be combined with operation hooks on that field.
//! Input and description suffice for `Config`. Output remains independently selectable.
//!
//! Container policies compose explicitly. `Option<A>` adapts an `Option<T>` and uses null for
//! `None`. `Vec<A>` adapts each element of a `Vec<T>` from an array. A direct policy can instead
//! adapt an entire vector, such as expanding one expression into many values. [`Native`] delegates
//! to the target's ordinary capability and is available under `scry::via`.
//!
//! ```
//! use scry::{Config, ToNode};
//! use scry::via::Native;
//!
//! #[derive(Config, ToNode)]
//! struct Levels {
//!     #[scry(via(Vec<Option<Native>>))]
//!     values: Vec<Option<u16>>,
//! }
//! ```
//!
//! Each container requires only the corresponding inner capability. Missing support remains a
//! compile-time error through nested composition:
//!
//! ```compile_fail,E0277
//! use scry::FromNode;
//! struct Missing;
//! #[derive(FromNode)]
//! struct Values(#[scry(via(Vec<Option<Missing>>))] Vec<Option<u16>>);
//! ```
//!
//! ```compile_fail,E0277
//! use scry::ToNode;
//! struct Missing;
//! #[derive(ToNode)]
//! struct Values(#[scry(via(Vec<Option<Missing>>))] Vec<Option<u16>>);
//! ```
//!
//! ```compile_fail,E0277
//! use scry::Describe;
//! struct Missing;
//! #[derive(Describe)]
//! struct Values(#[scry(via(Vec<Option<Missing>>))] Vec<Option<u16>>);
//! ```

use std::marker::PhantomData;

use crate::traits::{parse_vec, serialize_vec};
use crate::{Desc, Describe, FromNode, Node, NodeError, ToNode};

// ---------------------------------------------------------------------------------------------- //

/// Parses a complete target value using an explicitly selected policy.
///
/// Implementations use the Node's full logical location. Derived calls attach that location to
/// errors without a logical path and preserve existing locations and original causes. Accepted
/// leaves are consumed through typed reads or [`Node::read_leaf`] for explicit unread-input audits.
/// A policy owning a map shape validates its immediate keys with [`Node::ensure_only_keys`] or
/// delegates to a decoder that validates that shape.
///
/// The target is returned by value and must be sized. No output or description support is implied.
/// A policy does not fall back to the target's native input capability:
///
/// ```compile_fail,E0277
/// use scry::FromNode;
/// struct Policy;
/// #[derive(FromNode)]
/// struct Value(#[scry(via(Policy))] u16);
/// ```
pub trait FromNodeVia<T> {
    /// Parses the complete target value using this policy.
    fn from_node(node: &Node) -> Result<T, NodeError>;
}

/// Serializes a borrowed complete target value using an explicitly selected policy.
///
/// Error locations are relative to this value and describe its emitted representation. Enclosing
/// serializers prepend their field or index with [`NodeError::prepend_path`]. Successful Nodes
/// may be unanchored, and root null is permitted. Output needs neither cloned targets nor an owned
/// intermediate representation. Unsized targets are supported.
///
/// A policy does not fall back to the target's native output capability:
///
/// ```compile_fail,E0277
/// use scry::ToNode;
/// struct Policy;
/// #[derive(ToNode)]
/// struct Value(#[scry(via(Policy))] u16);
/// ```
pub trait ToNodeVia<T: ?Sized> {
    /// Serializes the complete target value using this policy.
    fn to_node(value: &T) -> Result<Node, NodeError>;
}

/// Describes the complete configuration value represented by an explicitly selected policy.
///
/// The description includes the policy's value shape and nullability. Field omission and prose
/// remain separate metadata. Adapted named fields suppress target-derived fallback displays and
/// directly described enum default markers, while preserving defaults on nested described fields.
/// No input or output support is implied. Unsized targets are supported.
///
/// A policy does not fall back to the target's native description capability:
///
/// ```compile_fail,E0277
/// use scry::Describe;
/// struct Policy;
/// #[derive(Describe)]
/// struct Value(#[scry(via(Policy))] u16);
/// ```
pub trait DescribeVia<T: ?Sized> {
    /// Describes the configuration value represented by this policy.
    fn describe() -> Desc;
}

/// Delegates each requested capability to the target's ordinary Scry implementation.
///
/// This explicit policy composes with `Option<Native>` and `Vec<Native>`. Input requires native
/// [`FromNode`], output requires native [`ToNode`], and description requires native [`Describe`].
/// Each requirement is independent. Output and description also support unsized targets.
pub struct Native;

impl<T: FromNode> FromNodeVia<T> for Native {
    fn from_node(node: &Node) -> Result<T, NodeError> {
        node.as_type()
    }
}

impl<T: ToNode + ?Sized> ToNodeVia<T> for Native {
    fn to_node(value: &T) -> Result<Node, NodeError> {
        value.to_node()
    }
}

impl<T: Describe + ?Sized> DescribeVia<T> for Native {
    fn describe() -> Desc {
        T::describe()
    }
}

// ---------------------------------------------------------------------------------------------- //
// Container Policies

impl<T, A: FromNodeVia<T>> FromNodeVia<Option<T>> for Option<A> {
    fn from_node(node: &Node) -> Result<Option<T>, NodeError> {
        <Option<Adapted<T, A>>>::from_node(node).map(|value| value.map(|adapted| adapted.0))
    }
}

impl<T, A: ToNodeVia<T>> ToNodeVia<Option<T>> for Option<A> {
    fn to_node(value: &Option<T>) -> Result<Node, NodeError> {
        value.as_ref().map(|value| AdaptedRef::<T, A>(value, PhantomData)).to_node()
    }
}

impl<T, A: DescribeVia<T>> DescribeVia<Option<T>> for Option<A> {
    fn describe() -> Desc {
        <Option<Adapted<T, A>>>::describe()
    }
}

impl<T, A: FromNodeVia<T>> FromNodeVia<Vec<T>> for Vec<A> {
    fn from_node(node: &Node) -> Result<Vec<T>, NodeError> {
        parse_vec(node, A::from_node)
    }
}

impl<T, A: ToNodeVia<T>> ToNodeVia<Vec<T>> for Vec<A> {
    fn to_node(value: &Vec<T>) -> Result<Node, NodeError> {
        serialize_vec(value, A::to_node)
    }
}

impl<T, A: DescribeVia<T>> DescribeVia<Vec<T>> for Vec<A> {
    fn describe() -> Desc {
        <Vec<Adapted<T, A>>>::describe()
    }
}

// ---------------------------------------------------------------------------------------------- //
// Native Operation Bridges

/// Reuses native input and description containers with an adapted inner value.
struct Adapted<T, A>(T, PhantomData<A>);

impl<T, A: FromNodeVia<T>> FromNode for Adapted<T, A> {
    fn from_node(node: &Node) -> Result<Self, NodeError> {
        A::from_node(node).map(|value| Self(value, PhantomData))
    }
}

impl<T, A: DescribeVia<T>> Describe for Adapted<T, A> {
    fn describe() -> Desc {
        A::describe()
    }
}

/// Reuses native output containers while borrowing the adapted inner value.
struct AdaptedRef<'a, T: ?Sized, A>(&'a T, PhantomData<A>);

impl<T: ?Sized, A: ToNodeVia<T>> ToNode for AdaptedRef<'_, T, A> {
    fn to_node(&self) -> Result<Node, NodeError> {
        A::to_node(self.0)
    }
}
