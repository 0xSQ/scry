//! Flat key=value format serialization.
//!
//! Provides [`FlatWriter`] for serializing Node trees to a simple flat format
//! where each leaf value is output as `path.to.key = "value"`.

use crate::key_path::KeyPath;
use crate::node::{Kind, Node};

use super::format_value;

// ---------------------------------------------------------------------------------------------- //
// FlatWriter

/// Writer for serializing Node trees to flat key=value format.
///
/// Each leaf value is output as a single line: `path.to.key = "value"`
pub struct FlatWriter {
    output: String,
    config: FlatConfig,
}

impl FlatWriter {
    /// Creates a new FlatWriter with default configuration.
    pub fn new() -> Self {
        Self::with_config(FlatConfig::default())
    }

    /// Creates a new FlatWriter with the given configuration.
    pub fn with_config(config: FlatConfig) -> Self {
        Self {
            output: String::new(),
            config,
        }
    }

    /// Writes a Node tree to flat format.
    pub fn write(&mut self, node: &Node) {
        self.write_node(node, &node.path);
    }

    /// Consumes the writer and returns the output string.
    pub fn into_string(self) -> String {
        self.output
    }

    fn write_node(&mut self, node: &Node, path: &KeyPath) {
        match &node.kind {
            Kind::Leaf(leaf) => {
                if !self.output.is_empty() {
                    self.output.push('\n');
                }
                self.output.push_str(&path.to_string());
                self.output.push_str(self.config.separator);
                self.output.push_str(&format_value(&leaf.value));
            }
            Kind::Vec(vec) => {
                for (i, child) in vec.iter().enumerate() {
                    let child_path = path.push_index(i);
                    self.write_node(child, &child_path);
                }
            }
            Kind::Map(map) => {
                for (key, child) in map.iter() {
                    let child_path = path.push_key(key);
                    self.write_node(child, &child_path);
                }
            }
        }
    }
}

impl Default for FlatWriter {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------------------------- //
// FlatConfig

/// Configuration for flat output formatting.
#[derive(Debug, Clone)]
pub struct FlatConfig {
    /// The separator between key and value (default: " = ").
    pub separator: &'static str,
}

impl Default for FlatConfig {
    fn default() -> Self {
        Self { separator: " = " }
    }
}
