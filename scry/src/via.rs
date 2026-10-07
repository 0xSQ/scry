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

use crate::{Desc, Node, NodeError};

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
