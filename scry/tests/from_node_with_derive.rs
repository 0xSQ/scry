//! Tests for from_node_with derive support.
#![cfg(feature = "format-json")]

use scry::node::Format;
use scry::{Node, NodeError};

// ---------------------------------------------------------------------------------------------- //
// Test Types

/// Simple wrapper type.
#[derive(Debug, Clone, PartialEq)]
struct Port(u16);

/// Custom parser for Port that validates the port number.
fn parse_port(node: &Node) -> Result<Port, NodeError> {
    let val: u16 = node.as_type()?;
    if val == 0 {
        return Err(NodeError::invalid_value(&node.path, "port cannot be 0"));
    }
    Ok(Port(val))
}

/// Parses a comma-separated list of strings.
fn parse_csv(node: &Node) -> Result<Vec<String>, NodeError> {
    let s: String = node.as_type()?;
    Ok(s.split(',').map(|s| s.trim().to_string()).collect())
}

// ---------------------------------------------------------------------------------------------- //
// Struct with parse_with

#[derive(Debug, Clone, PartialEq, scry::FromNode)]
struct RequiredConfig {
    #[scry(from_node_with(parse_port))]
    port: Port,
}

#[derive(Debug, Clone, PartialEq, scry::FromNode)]
struct DefaultedConfig {
    #[scry(from_node_with(parse_port), default = Port(8080))]
    port: Port,
}

#[derive(Debug, Clone, PartialEq, scry::FromNode)]
struct DefaultConfig {
    #[scry(from_node_with(parse_csv), default = Vec::new())]
    tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, scry::FromNode)]
struct OptionalConfig {
    #[scry(from_node_with(parse_port))]
    port: Option<Port>,
}

fn node(json: &str) -> Node {
    Node::parse_str(json, Format::Json).unwrap()
}

// ---------------------------------------------------------------------------------------------- //
// Required parse_with Tests

#[test]
fn required_parse_with_parses_value() {
    let n = node(r#"{"port": 3000}"#);
    let cfg: RequiredConfig = n.as_type().unwrap();
    assert_eq!(cfg.port, Port(3000));
}

#[test]
fn required_parse_with_missing_key_errors() {
    let n = node(r#"{}"#);
    let err = n.as_type::<RequiredConfig>().unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("port"), "error should mention 'port': {}", msg);
}

#[test]
fn required_parse_with_validation_error_propagates() {
    let n = node(r#"{"port": 0}"#);
    let err = n.as_type::<RequiredConfig>().unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("cannot be 0"), "error should mention validation: {}", msg);
}

// ---------------------------------------------------------------------------------------------- //
// Defaulted parse_with Tests

#[test]
fn defaulted_parse_with_uses_default_when_missing() {
    let n = node(r#"{}"#);
    let cfg: DefaultedConfig = n.as_type().unwrap();
    assert_eq!(cfg.port, Port(8080));
}

#[test]
fn defaulted_parse_with_uses_provided_value() {
    let n = node(r#"{"port": 3000}"#);
    let cfg: DefaultedConfig = n.as_type().unwrap();
    assert_eq!(cfg.port, Port(3000));
}

#[test]
fn defaulted_parse_with_validation_error_when_invalid() {
    let n = node(r#"{"port": 0}"#);
    let err = n.as_type::<DefaultedConfig>().unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("cannot be 0"), "error should mention validation: {}", msg);
}

// ---------------------------------------------------------------------------------------------- //
// Default (Default::default()) parse_with Tests

#[test]
fn default_parse_with_uses_default_when_missing() {
    let n = node(r#"{}"#);
    let cfg: DefaultConfig = n.as_type().unwrap();
    assert_eq!(cfg.tags, Vec::<String>::new());
}

#[test]
fn default_parse_with_uses_provided_value() {
    let n = node(r#"{"tags": "a, b, c"}"#);
    let cfg: DefaultConfig = n.as_type().unwrap();
    assert_eq!(cfg.tags, vec!["a".to_string(), "b".to_string(), "c".to_string()]);
}

// ---------------------------------------------------------------------------------------------- //
// Optional parse_with Tests

#[test]
fn optional_parse_with_none_when_missing() {
    let n = node(r#"{}"#);
    let cfg: OptionalConfig = n.as_type().unwrap();
    assert_eq!(cfg.port, None);
}

#[test]
fn optional_parse_with_some_when_present() {
    let n = node(r#"{"port": 3000}"#);
    let cfg: OptionalConfig = n.as_type().unwrap();
    assert_eq!(cfg.port, Some(Port(3000)));
}

#[test]
fn optional_parse_with_validation_error_when_invalid() {
    let n = node(r#"{"port": 0}"#);
    let err = n.as_type::<OptionalConfig>().unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("cannot be 0"), "error should mention validation: {}", msg);
}
