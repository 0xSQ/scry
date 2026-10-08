//! Composes custom child conversions through shared configuration shapes.
//!
//! Readers receive the actual input Nodes and attach missing error locations to those Nodes.
//! Child readers consume accepted leaves through typed decoding or [`crate::Node::read_leaf`].
//! Writers borrow stored values and prepend each emitted index to relative child errors.
//! Neither operation requires the child type to implement a Scry trait.
//!
//! ```
//! use std::net::IpAddr;
//! use scry::convert::{read, write};
//! use scry::node::Format;
//! use scry::{Node, NodeError, ToNode};
//!
//! let input = Node::parse_str(r#"["127.0.0.1", "::1"]"#, Format::Rhai)?;
//! let addresses: Vec<IpAddr> = read::vec(&input, |node| {
//!     node.as_type::<String>()?.parse().map_err(|error| {
//!         NodeError::invalid_value_with_source(&node.path, "invalid IP address", error)
//!     })
//! })?;
//! let output = write::list(&addresses, |address| address.to_string().to_node())?;
//! assert_eq!(output.as_type::<Vec<String>>()?, ["127.0.0.1", "::1"]);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! Kit containers expose their own operations beside their types. For example,
//! [`crate::kit::key_values::read_with`] accepts map or pair input, while
//! [`crate::kit::one_or_many::read_with`] owns the singleton-or-array decision.
//! Standard descriptions compose with [`crate::Desc::list`], [`crate::Desc::tuple`], and
//! [`crate::Desc::nullable`]. Missing keys and defaults remain at the field boundary.
//!
//! A field can select a module with `#[scry(with(adapter))]`. Each derive requests only its own
//! function: `from_node`, `to_node`, or `describe`. A missing requested function does not fall
//! back to the target's native support:
//!
//! ```compile_fail,E0425
//! use scry::ToNode;
//! mod adapter {}
//! #[derive(ToNode)]
//! struct Settings {
//!     #[scry(with(adapter))]
//!     port: u16,
//! }
//! ```
//!
//! A module selection cannot be combined with an individual operation hook:
//!
//! ```compile_fail
//! use scry::{FromNode, Node, NodeError};
//! mod adapter {
//!     pub fn from_node(node: &scry::Node) -> Result<u16, scry::NodeError> {
//!         node.as_type()
//!     }
//! }
//! fn read_port(node: &Node) -> Result<u16, NodeError> {
//!     node.as_type()
//! }
//! #[derive(FromNode)]
//! struct Settings {
//!     #[scry(with(adapter), from_node_with(read_port))]
//!     port: u16,
//! }
//! ```
//!
//! A foreign target still needs explicit conversion when it lacks native support:
//!
//! ```compile_fail,E0277
//! use scry::Config;
//! #[derive(Config)]
//! struct Settings {
//!     address: std::net::IpAddr,
//! }
//! ```

pub mod read;
pub mod write;
