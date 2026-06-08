//! Visual tree format serialization.
//!
//! Provides [`TreeWriter`] for serializing Node trees to a human-readable
//! visual tree format with customizable markers and colors.

pub use colored::Color;
use colored::Colorize;

use crate::node::{Kind, Node, Value};

use super::format_value;

// ---------------------------------------------------------------------------------------------- //
// TreeWriter

/// Writer for serializing Node trees to visual tree format.
///
/// Produces output like:
/// ```text
/// ▸ auto_open true
/// ▸ output_dir "/path/to/output"
/// ▾ setup
///    ▾ base
///    ┊  ▸ cfg_scale 1.5
///    ┊  ▸ model "model.safetensors"
///    ▪ tags
///       ▸ "alpha"
///       ▸ "beta"
/// ```
pub struct TreeWriter {
    output: String,
    config: TreeConfig,
    /// Tracks whether each ancestor is the last sibling at its depth.
    /// Used to decide between `┊` (not last) and spaces (last).
    is_last_at_depth: Vec<bool>,
}

impl TreeWriter {
    /// Creates a new TreeWriter with default configuration.
    pub fn new() -> Self {
        Self::with_config(TreeConfig::default())
    }

    /// Creates a new TreeWriter with the given configuration.
    pub fn with_config(config: TreeConfig) -> Self {
        Self {
            output: String::new(),
            config,
            is_last_at_depth: Vec::new(),
        }
    }

    /// Writes a Node tree to tree format.
    pub fn write(&mut self, node: &Node) {
        match &node.kind {
            Kind::Leaf(leaf) => {
                // Single leaf at root - just output the value
                self.output.push_str(&self.colorize_value(&leaf.value));
            }
            Kind::Vec(vec) => {
                // Root is a vec - output each element
                for (i, child) in vec.iter().enumerate() {
                    let is_last = i == vec.len() - 1;
                    self.write_entry(child, EntryKind::VecElement(i), is_last);
                }
            }
            Kind::Map(map) => {
                // Root is a map - output each entry
                for (i, (key, child)) in map.iter().enumerate() {
                    let is_last = i == map.len() - 1;
                    self.write_entry(child, EntryKind::MapEntry(key), is_last);
                }
            }
        }
    }

    /// Consumes the writer and returns the output string.
    pub fn into_string(self) -> String {
        self.output
    }

    /// Colorizes a value based on its type.
    fn colorize_value(&self, value: &Value) -> String {
        let formatted = format_value(value);
        if !self.config.color {
            return formatted;
        }
        match value {
            Value::String(_) => formatted.color(self.config.string_color).to_string(),
            Value::Bool(_) => formatted.color(self.config.bool_color).to_string(),
            Value::Null => {
                if self.config.dim_scaffolding {
                    formatted.dimmed().to_string()
                } else {
                    formatted
                }
            }
            // All numeric types
            _ => formatted.color(self.config.number_color).to_string(),
        }
    }

    /// Dims text if scaffolding dimming is enabled.
    fn dim(&self, text: &str) -> String {
        if self.config.color && self.config.dim_scaffolding {
            text.dimmed().to_string()
        } else {
            text.to_string()
        }
    }

    /// Writes a single entry (map entry or vec element).
    fn write_entry(&mut self, node: &Node, entry_kind: EntryKind, is_last: bool) {
        // Build the prefix from ancestor last-sibling state
        let prefix = self.build_prefix();

        match &node.kind {
            Kind::Leaf(leaf) => {
                if !self.output.is_empty() {
                    self.output.push('\n');
                }
                self.output.push_str(&prefix);
                self.output.push_str(&self.dim(self.config.leaf_marker));
                self.output.push(' ');
                match entry_kind {
                    EntryKind::MapEntry(key) => {
                        self.output.push_str(key);
                        self.output.push(' ');
                        self.output.push_str(&self.colorize_value(&leaf.value));
                    }
                    EntryKind::VecElement(idx) => {
                        if self.config.show_leaf_indices {
                            self.output.push_str(&self.dim(&format!("[{}] ", idx)));
                        }
                        self.output.push_str(&self.colorize_value(&leaf.value));
                    }
                }
            }
            Kind::Map(map) => {
                if !self.output.is_empty() {
                    self.output.push('\n');
                }
                self.output.push_str(&prefix);
                self.output.push_str(&self.dim(self.config.map_marker));
                self.output.push(' ');
                match entry_kind {
                    EntryKind::MapEntry(key) => {
                        self.output.push_str(key);
                    }
                    EntryKind::VecElement(idx) => {
                        self.output.push_str(&self.dim(&format!("[{}]", idx)));
                    }
                }
                // Recurse into children
                self.is_last_at_depth.push(is_last);
                for (i, (child_key, child)) in map.iter().enumerate() {
                    let child_is_last = i == map.len() - 1;
                    self.write_entry(child, EntryKind::MapEntry(child_key), child_is_last);
                }
                self.is_last_at_depth.pop();
            }
            Kind::Vec(vec) => {
                if !self.output.is_empty() {
                    self.output.push('\n');
                }
                self.output.push_str(&prefix);
                self.output.push_str(&self.dim(self.config.vec_marker));
                self.output.push(' ');
                match entry_kind {
                    EntryKind::MapEntry(key) => {
                        self.output.push_str(key);
                    }
                    EntryKind::VecElement(idx) => {
                        self.output.push_str(&self.dim(&format!("[{}]", idx)));
                    }
                }
                // Recurse into children
                self.is_last_at_depth.push(is_last);
                for (i, child) in vec.iter().enumerate() {
                    let child_is_last = i == vec.len() - 1;
                    self.write_entry(child, EntryKind::VecElement(i), child_is_last);
                }
                self.is_last_at_depth.pop();
            }
        }
    }

    /// Builds the prefix string based on ancestor last-sibling state.
    fn build_prefix(&self) -> String {
        let mut prefix = String::new();
        for &is_last in &self.is_last_at_depth {
            if is_last {
                // Use spaces (tree line width + indent)
                let width = self.config.tree_line.chars().count() + self.config.indent_width;
                for _ in 0..width {
                    prefix.push(' ');
                }
            } else {
                // Use tree line + spaces (dimmed if scaffolding dimming enabled)
                prefix.push_str(&self.dim(self.config.tree_line));
                for _ in 0..self.config.indent_width {
                    prefix.push(' ');
                }
            }
        }
        prefix
    }
}

impl Default for TreeWriter {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------------------------- //
// TreeConfig

/// Configuration for tree output formatting.
#[derive(Debug, Clone)]
pub struct TreeConfig {
    /// Marker for leaf values (default: "▸").
    pub leaf_marker: &'static str,
    /// Marker for map containers (default: "▾").
    pub map_marker: &'static str,
    /// Marker for vec containers (default: "▪").
    pub vec_marker: &'static str,
    /// Tree continuation line (default: "┊").
    pub tree_line: &'static str,
    /// Number of spaces after tree_line or for blank continuation (default: 2).
    pub indent_width: usize,
    /// Whether to show array indices for leaf values (default: false).
    /// Indices are always shown for container elements regardless of this setting.
    pub show_leaf_indices: bool,
    /// Whether to colorize output (default: true).
    pub color: bool,
    /// Color for string values (default: Green).
    pub string_color: Color,
    /// Color for number values (default: Yellow).
    pub number_color: Color,
    /// Color for boolean values (default: Magenta).
    pub bool_color: Color,
    /// Whether to dim scaffolding elements like markers, tree lines, and array indices (default: true).
    pub dim_scaffolding: bool,
}

impl Default for TreeConfig {
    fn default() -> Self {
        Self {
            leaf_marker: "▸",
            map_marker: "▾",
            vec_marker: "▪",
            tree_line: "┊",
            indent_width: 2,
            show_leaf_indices: false,
            color: true,
            string_color: Color::BrightGreen,
            number_color: Color::BrightYellow,
            bool_color: Color::BrightMagenta,
            dim_scaffolding: true,
        }
    }
}

impl TreeConfig {
    /// Creates a config with color disabled.
    pub fn no_color() -> Self {
        Self {
            color: false,
            dim_scaffolding: false,
            ..Self::default()
        }
    }
}

// ---------------------------------------------------------------------------------------------- //
// Internal helpers

/// The kind of entry being written (for formatting decisions).
enum EntryKind<'a> {
    MapEntry(&'a str),
    VecElement(usize),
}
