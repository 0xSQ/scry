//! Serialization utilities.
//!
//! Provides:
//! - [`Document`] for constructing annotated configuration values.
//! - [`RhaiWriter`] for rendering values and documents as Rhai.
//! - `Json5Writer` for JSON5 rendering when enabled.
//! - [`FlatWriter`] for serializing Node trees to flat key-value format.
//! - [`TreeWriter`] for serializing Node trees to visual tree format.
//! - [`format_template`] for generating documented configuration scaffolds.

mod document;
mod flat;
mod literal;
mod template;
mod tree;

pub use document::{Document, DocumentBuilder, DocumentEntry, MapBuilder, SeqBuilder};
pub use flat::{FlatConfig, FlatWriter};
#[cfg(feature = "format-json5")]
pub use literal::{Json5Writer, Json5WriterConfig};
pub use literal::{LiteralWriterConfig, RhaiWriter, RhaiWriterConfig, StdoutRhaiWriter};
pub use template::{format_template, TemplateMode};
pub use tree::{Color, TreeConfig, TreeWriter};

use crate::key_path::quote_string;
use crate::node::Value;

// ---------------------------------------------------------------------------------------------- //
// Shared utilities

/// Formats a Value for display output (flat/tree formats).
///
/// - Strings are quoted with proper escaping
/// - Numbers, bools, and null are unquoted
pub fn format_value(value: &Value) -> String {
    match value {
        Value::String(s) => quote_string(s),
        _ => value.to_string(),
    }
}

// ---------------------------------------------------------------------------------------------- //
// Tests

#[cfg(all(test, feature = "format-json"))]
mod tests {
    use super::*;
    use crate::node::{Format, Node, NodeError};
    use indoc::indoc;

    // ------------------------------------------------------------------------------------------ //
    // Rhai writer tests

    #[test]
    fn test_write_primitives() -> Result<(), NodeError> {
        let mut w = RhaiWriter::to_string_writer();
        w.value(42i32)?;
        assert_eq!(w.as_str()?, "42");

        let mut w = RhaiWriter::to_string_writer();
        w.value("hello".to_string())?;
        assert_eq!(w.as_str()?, "\"hello\"");

        let mut w = RhaiWriter::to_string_writer();
        w.value(true)?;
        assert_eq!(w.as_str()?, "true");

        Ok(())
    }

    #[test]
    fn test_write_map_inline() -> Result<(), NodeError> {
        let mut w = RhaiWriter::to_string_writer();
        let document = Document::map(|m| {
            m.entry("a", 1i32)?;
            m.entry("b", 2i32)?;
            Ok(())
        })?
        .compact();
        w.write(&document)?;
        assert_eq!(w.as_str()?, "#{ a: 1, b: 2 }\n");
        Ok(())
    }

    #[test]
    fn rhai_writer_quotes_reserved_identifier_keys() -> Result<(), NodeError> {
        let mut w = RhaiWriter::to_string_writer();
        let document = Document::map(|m| {
            for key in literal::RHAI_RESERVED_IDENTIFIERS_FOR_TESTS {
                m.entry(*key, 1i32)?;
            }
            m.entry("normal", 1i32)?;
            Ok(())
        })?
        .compact();
        w.write(&document)?;
        let output = w.as_str()?;

        for key in literal::RHAI_RESERVED_IDENTIFIERS_FOR_TESTS {
            assert!(output.contains(&format!(r#""{key}": 1"#)), "key was not quoted: {key}");
        }
        assert!(output.contains("normal: 1"));
        Node::parse_str(output, Format::Rhai)?;

        Ok(())
    }

    #[test]
    fn test_write_seq_inline() -> Result<(), NodeError> {
        let mut w = RhaiWriter::to_string_writer();
        let document = Document::seq(|s| {
            s.elem(1i32)?;
            s.elem(2i32)?;
            s.elem(3i32)?;
            Ok(())
        })?
        .compact();
        w.write(&document)?;
        assert_eq!(w.as_str()?, "[1, 2, 3]\n");
        Ok(())
    }

    // ------------------------------------------------------------------------------------------ //
    // Flat writer tests

    /// Helper to parse JSON into a Node for testing.
    fn node_from_json(s: &str) -> Node {
        Node::parse_str(s, Format::Json).expect("valid JSON test input")
    }

    #[test]
    fn flat_simple_values() {
        let node = node_from_json(r#"{"name": "test", "count": 42, "enabled": true}"#);
        let output = node.to_flat_string();
        assert_eq!(
            output,
            indoc! {r#"
                name "test"
                count 42
                enabled true"#}
        );
    }

    #[test]
    fn flat_nested_map() {
        let node = node_from_json(r#"{"database": {"host": "localhost", "port": 5432}}"#);
        let output = node.to_flat_string();
        assert_eq!(
            output,
            indoc! {r#"
                database.host "localhost"
                database.port 5432"#}
        );
    }

    #[test]
    fn flat_array_indices() {
        let node = node_from_json(r#"{"tags": ["alpha", "beta", "gamma"]}"#);
        let output = node.to_flat_string();
        assert_eq!(
            output,
            indoc! {r#"
                tags[0] "alpha"
                tags[1] "beta"
                tags[2] "gamma""#}
        );
    }

    #[test]
    fn flat_array_of_maps() {
        let node = node_from_json(
            r#"{"servers": [{"host": "a.example.com", "port": 80}, {"host": "b.example.com", "port": 443}]}"#,
        );
        let output = node.to_flat_string();
        assert_eq!(
            output,
            indoc! {r#"
                servers[0].host "a.example.com"
                servers[0].port 80
                servers[1].host "b.example.com"
                servers[1].port 443"#}
        );
    }

    #[test]
    fn flat_nested_arrays() {
        let node = node_from_json(r#"{"matrix": [[1, 2], [3, 4]]}"#);
        let output = node.to_flat_string();
        assert_eq!(
            output,
            indoc! {"
                matrix[0][0] 1
                matrix[0][1] 2
                matrix[1][0] 3
                matrix[1][1] 4"}
        );
    }

    #[test]
    fn flat_empty_containers_produce_no_output() {
        let node = node_from_json(r#"{"empty_map": {}, "empty_vec": [], "name": "test"}"#);
        let output = node.to_flat_string();
        // Empty containers should produce no output
        assert_eq!(output, r#"name "test""#);
    }

    #[test]
    fn flat_null_value() {
        let node = node_from_json(r#"{"timeout": null}"#);
        let output = node.to_flat_string();
        assert_eq!(output, r#"timeout null"#);
    }

    #[test]
    fn flat_custom_separator() {
        let node = node_from_json(r#"{"host": "localhost"}"#);
        let config = FlatConfig { separator: ": " };
        let output = node.to_flat_string_with_config(config);
        assert_eq!(output, r#"host: "localhost""#);
    }

    // ------------------------------------------------------------------------------------------ //
    // Tree writer tests

    #[test]
    fn tree_simple_leaf_values() {
        let node = node_from_json(r#"{"name": "test", "count": 42, "enabled": true}"#);
        let output = node.to_tree_string_with_config(TreeConfig::no_color());
        assert_eq!(
            output,
            indoc! {r#"
                ▸ name "test"
                ▸ count 42
                ▸ enabled true"#}
        );
    }

    #[test]
    fn tree_nested_map() {
        let node = node_from_json(r#"{"database": {"host": "localhost", "port": 5432}}"#);
        let output = node.to_tree_string_with_config(TreeConfig::no_color());
        // database is the only/last entry, so children use spaces not ┊
        assert_eq!(
            output,
            indoc! {r#"
                ▾ database
                   ▸ host "localhost"
                   ▸ port 5432"#}
        );
    }

    #[test]
    fn tree_map_not_last_uses_tree_line() {
        let node = node_from_json(r#"{"database": {"host": "localhost"}, "name": "test"}"#);
        let output = node.to_tree_string_with_config(TreeConfig::no_color());
        // database is NOT last (name follows), so children use ┊
        assert_eq!(
            output,
            indoc! {r#"
                ▾ database
                ┊  ▸ host "localhost"
                ▸ name "test""#}
        );
    }

    #[test]
    fn tree_array() {
        let node = node_from_json(r#"{"tags": ["alpha", "beta"]}"#);
        let output = node.to_tree_string_with_config(TreeConfig::no_color());
        // Leaf indices are hidden by default
        assert_eq!(
            output,
            indoc! {r#"
                ▪ tags
                   ▸ "alpha"
                   ▸ "beta""#}
        );
    }

    #[test]
    fn tree_array_of_maps() {
        let node = node_from_json(
            r#"{"servers": [{"host": "a.example.com"}, {"host": "b.example.com"}]}"#,
        );
        let output = node.to_tree_string_with_config(TreeConfig::no_color());
        // servers is the only entry so uses spaces
        // first map [0] is not last, so its children use ┊
        // second map [1] is last, so its children use spaces
        assert_eq!(
            output,
            indoc! {r#"
                ▪ servers
                   ▾ [0]
                   ┊  ▸ host "a.example.com"
                   ▾ [1]
                      ▸ host "b.example.com""#}
        );
    }

    #[test]
    fn tree_nested_arrays() {
        let node = node_from_json(r#"{"matrix": [[1, 2], [3, 4]]}"#);
        let output = node.to_tree_string_with_config(TreeConfig::no_color());
        // Container indices shown, leaf indices hidden by default
        assert_eq!(
            output,
            indoc! {"
                ▪ matrix
                   ▪ [0]
                   ┊  ▸ 1
                   ┊  ▸ 2
                   ▪ [1]
                      ▸ 3
                      ▸ 4"}
        );
    }

    #[test]
    fn tree_empty_containers() {
        let node = node_from_json(r#"{"empty_map": {}, "empty_vec": [], "name": "test"}"#);
        let output = node.to_tree_string_with_config(TreeConfig::no_color());
        // Empty containers show just the marker with no children
        assert_eq!(
            output,
            indoc! {r#"
                ▾ empty_map
                ▪ empty_vec
                ▸ name "test""#}
        );
    }

    #[test]
    fn tree_deeply_nested_last_sibling_logic() {
        // Test the ┊ vs space decision at multiple depths
        let node = node_from_json(
            r#"{
                "database": {
                    "credentials": {
                        "username": "admin",
                        "password": "secret"
                    },
                    "replicas": ["r1", "r2"]
                },
                "name": "test"
            }"#,
        );
        let output = node.to_tree_string_with_config(TreeConfig::no_color());
        // database is NOT last (name follows) → ┊
        // credentials is NOT last (replicas follows) → ┊
        // replicas IS last in database → spaces
        // Leaf indices are hidden by default
        assert_eq!(
            output,
            indoc! {r#"
                ▾ database
                ┊  ▾ credentials
                ┊  ┊  ▸ username "admin"
                ┊  ┊  ▸ password "secret"
                ┊  ▪ replicas
                ┊     ▸ "r1"
                ┊     ▸ "r2"
                ▸ name "test""#}
        );
    }

    #[test]
    fn tree_custom_symbols() {
        let node = node_from_json(r#"{"database": {"host": "localhost"}}"#);
        let config = TreeConfig {
            leaf_marker: "->",
            map_marker: "{}",
            vec_marker: "[]",
            tree_line: "|",
            indent_width: 3,
            ..TreeConfig::no_color()
        };
        let output = node.to_tree_string_with_config(config);
        assert_eq!(
            output,
            indoc! {r#"
                {} database
                    -> host "localhost""#}
        );
    }

    #[test]
    fn tree_depth_marks_only_containers_with_omitted_children() {
        let node = node_from_json(
            r#"{
                "database": {
                    "credentials": { "username": "admin" },
                    "replicas": ["r1", "r2"]
                },
                "matrix": [[1, 2], []],
                "empty_map": {},
                "empty_vec": [],
                "enabled": true
            }"#,
        );
        let config = TreeConfig {
            max_depth: Some(1),
            ..TreeConfig::no_color()
        };
        assert_eq!(
            node.to_tree_string_with_config(config),
            indoc! {"
                ▾ database …
                ▪ matrix …
                ▾ empty_map
                ▪ empty_vec
                ▸ enabled true"}
        );

        let config = TreeConfig {
            max_depth: Some(2),
            ..TreeConfig::no_color()
        };
        assert_eq!(
            node.to_tree_string_with_config(config),
            indoc! {"
                ▾ database
                ┊  ▾ credentials …
                ┊  ▪ replicas …
                ▪ matrix
                ┊  ▪ [0] …
                ┊  ▪ [1]
                ▾ empty_map
                ▪ empty_vec
                ▸ enabled true"}
        );
    }

    #[test]
    fn tree_depth_is_relative_to_the_displayed_root() {
        let node = node_from_json(
            r#"{ "outer": { "inner": { "value": 42 }, "names": ["alpha", "beta"] } }"#,
        );
        let selected = node.req_node("outer").expect("test branch exists");
        let config = TreeConfig {
            max_depth: Some(1),
            ..TreeConfig::no_color()
        };
        assert_eq!(
            selected.to_tree_string_with_config(config),
            indoc! {"
                ▾ inner …
                ▪ names …"}
        );
    }

    #[test]
    fn tree_depth_preserves_root_values_and_array_indices() {
        let node = node_from_json(r#"["alpha", { "host": "localhost" }, []]"#);
        let config = TreeConfig {
            max_depth: Some(1),
            show_leaf_indices: true,
            ..TreeConfig::no_color()
        };
        assert_eq!(
            node.to_tree_string_with_config(config.clone()),
            indoc! {r#"
                ▸ [0] "alpha"
                ▾ [1] …
                ▪ [2]"#}
        );
        assert_eq!(node_from_json("42").to_tree_string_with_config(config.clone()), "42");
        assert_eq!(node_from_json("{}").to_tree_string_with_config(config.clone()), "");
        assert_eq!(node_from_json("[]").to_tree_string_with_config(config), "");
    }

    #[test]
    fn tree_root_leaf() {
        // When root is a leaf, just output the value
        let node = node_from_json(r#""hello""#);
        let output = node.to_tree_string_with_config(TreeConfig::no_color());
        assert_eq!(output, "\"hello\"");
    }

    #[test]
    fn tree_root_array() {
        // When root is an array, output elements directly
        // Leaf indices are hidden by default
        let node = node_from_json(r#"[1, 2, 3]"#);
        let output = node.to_tree_string_with_config(TreeConfig::no_color());
        assert_eq!(
            output,
            indoc! {"
                ▸ 1
                ▸ 2
                ▸ 3"}
        );
    }
}
