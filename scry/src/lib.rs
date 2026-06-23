//! A Rust library for building config-file-driven interfaces and CLI tools.
//!
//! Scry handles the scaffolding of config-driven applications: parsing files into typed structs,
//! generating format documentation, validating user input with helpful errors, and letting users
//! inspect and override values at runtime. It also supports [Rhai](https://rhai.rs) scripts as
//! config files, enabling variables, imports, and computed values.
//!
//! # Quick Example
//!
//! ```rust,ignore
//! use scry::{Config, Node};
//!
//! #[derive(Debug, Config)]
//! struct Deploy {
//!     target: String,
//!     #[scry(default = 3)]
//!     retries: u32,
//! }
//!
//! let node = Node::parse_file("deploy.json")?;
//! let config: Deploy = node.as_type()?;
//! ```
//!
//! # Modules
//!
//! - [`node`] - The [`Node`] tree, an intermediate representation for configuration data.
//! - [`desc`] - Description types for documenting config shapes (powers `--desc`).
//! - [`key_path`] - Path navigation for addressing locations in a config tree.
//! - [`cli`] - CLI utilities, including the [`Setup`](cli::setup::Setup) builder for assembling
//!   clap commands with config loading, overrides, and inspection options.
//! - [`writer`] - Serialization writers (Rhai, JSON tree, flat key-value).
//! - [`rhai`] - Rhai script evaluation environment.
//! - [`kit`] - Reusable config building blocks (file lists, one-or-many collections).
//! - [`util`] - General utilities (path extensions, etc.).
//!
//! # Further Reading
//!
//! See the [README](https://github.com/0xSQ/scry) for a guided walkthrough of building a CLI
//! with Scry, and [Core Concepts](https://github.com/0xSQ/scry/blob/main/docs/concepts.md) for
//! details on the Node tree, traits, and manual implementations.

// Self-alias so derive macros can use `::scry::` paths from within this crate.
extern crate self as scry;

pub mod cli;
pub mod desc;
pub mod key_path;
pub mod kit;
pub mod node;
pub mod rhai;
pub mod string_enum;
mod traits;
pub mod util;
pub mod writer;

// ---------------------------------------------------------------------------------------------- //

// Re-export clap because CLI bundle extension points use clap types.
pub use clap;

// Re-export core types at the root for convenience.
pub use desc::{Desc, DescPathError};
pub use key_path::{KeyPath, KeyPathError};
pub use node::{Node, NodeError};
pub use string_enum::StringEnumError;
pub use traits::{Describe, FromNode, ToNode};

// Re-export derive macros.
pub use scry_derive::{Config, Describe, FromNode, StringEnum, ToNode};

// Re-export probe machinery for derive macro (hidden from docs).
#[doc(hidden)]
pub use desc::{make_desc_probe, DescFallback};

// Re-export third-party types needed by derive macro expansions (hidden from docs).
#[doc(hidden)]
pub mod _private {
    pub use indexmap::IndexMap;
}

// ---------------------------------------------------------------------------------------------- //

// Simple boxed error type
pub type BoxedError = Box<dyn std::error::Error + Send + Sync + 'static>;
