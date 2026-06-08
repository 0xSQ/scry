//! "Mother of all Desc" example to exercise all description tree cases.
//!
//! This example demonstrates how different Rust types map to Desc tree nodes.
//! Run with: cargo run --example desc_mother

use scry::kit::KeyValues;
use scry::{Describe, FromNode, ToNode};
use std::path::PathBuf;

// ---------------------------------------------------------------------------------------------- //
// Newtype and Tuple Structs

/// A newtype wrapper around PathBuf.
#[derive(Debug, Clone, PartialEq, FromNode, ToNode, Describe)]
pub struct WrappedPath(pub PathBuf);

/// A tuple struct with two elements.
#[derive(Debug, Clone, PartialEq, FromNode, ToNode, Describe)]
pub struct Pair(pub String, pub i32);

// ---------------------------------------------------------------------------------------------- //
// Enum with All Variant Types

/// An enum demonstrating all variant types.
#[derive(Debug, Clone, PartialEq, Default, FromNode, ToNode, Describe)]
pub enum Mode {
    /// Unit variant - the default mode.
    #[default]
    Auto,

    /// Tuple variant with single value.
    Fixed(u32),

    /// Tuple variant with two values.
    Range(i32, i32),

    /// Struct variant with named fields.
    Named {
        /// The name for this mode.
        name: String,
        /// Whether this mode is enabled.
        enabled: bool,
    },
}

// ---------------------------------------------------------------------------------------------- //
// Nested Struct

/// Inner struct with various field types.
#[derive(Debug, Clone, PartialEq, FromNode, ToNode, Describe)]
pub struct Inner {
    /// Optional field (None if missing).
    pub maybe: Option<String>,

    /// Field with a default value.
    #[scry(default = 10)]
    pub retries: u32,

    /// A tuple field (rendered as array).
    pub window: (u32, u32),

    /// Fixed-size array.
    pub rgb: [u8; 3],
}

// ---------------------------------------------------------------------------------------------- //
// Root Config Struct

/// Root configuration demonstrating all container types.
#[derive(Debug, Clone, PartialEq, FromNode, ToNode, Describe)]
pub struct Root {
    // Scalars
    /// A boolean flag.
    pub ok: bool,

    /// An integer count.
    pub count: i32,

    /// A floating-point ratio.
    pub ratio: f64,

    /// A string name.
    pub name: String,

    // Path types
    /// A file path.
    pub path: PathBuf,

    /// A wrapped path (newtype).
    pub wrapped: WrappedPath,

    /// A pair tuple struct.
    pub pair: Pair,

    // Enum
    /// The operating mode.
    pub mode: Mode,

    // Collections
    /// A vector of strings.
    pub tags: Vec<String>,

    /// A vector of complex objects.
    pub items: Vec<Inner>,

    /// A key-value collection of integers.
    pub labels: KeyValues<i32>,

    /// A list of unique paths.
    pub paths: Vec<PathBuf>,
}

fn main() {
    println!("--- Root Desc ---\n");
    println!("{}", Root::describe().display());

    println!("\n--- Mode Enum Desc ---\n");
    println!("{}", Mode::describe().display());

    println!("\n--- Inner Struct Desc ---\n");
    println!("{}", Inner::describe().display());

    println!("\n--- Path Validation Example ---\n");
    let desc = Root::describe();

    // Valid paths (including tuple indices)
    let valid_paths = ["ok", "mode", "items", "labels", "pair[0]", "pair[1]"];
    for path in valid_paths {
        match desc.validate_path(path) {
            Ok(()) => println!("  '{}' - valid", path),
            Err(e) => println!("  '{}' - ERROR: {}", path, e),
        }
    }

    // Invalid tuple index
    println!("\n--- Invalid Tuple Index Example ---\n");
    if let Err(e) = desc.validate_path("pair[5]") {
        println!("{}", e);
    }

    // Invalid path
    println!("\n--- Invalid Path Example ---\n");
    if let Err(e) = desc.validate_path("nonexistent") {
        println!("{}", e);
    }

    // Tuple entry lookup
    println!("\n--- Tuple Entry Lookup ---\n");
    if let Some(entry) = desc.entry_at_path("pair[0]") {
        println!("pair[0] entry:\n{}", entry.display());
    }
}
