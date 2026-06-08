//! Tests for large integer precision handling in format conversions.
#![cfg(all(feature = "format-json", feature = "format-json5"))]

use scry::node::Format;
use scry::{FromNode, Node};

// ---------------------------------------------------------------------------------------------- //
// Test Types

#[derive(Debug, Clone, PartialEq, FromNode)]
struct Sentinel {
    sentinel: u64,
}

#[derive(Debug, Clone, PartialEq, FromNode)]
struct SignedSentinel {
    sentinel: i64,
}

// ---------------------------------------------------------------------------------------------- //
// JSON Tests

#[test]
fn parse_u64_max_from_json() {
    let json = format!(r#"{{"sentinel": {}}}"#, u64::MAX);
    let node = Node::parse_str(&json, Format::Json).unwrap();
    let cfg: Sentinel = node.as_type().unwrap();
    assert_eq!(cfg.sentinel, u64::MAX);
}

#[test]
fn parse_i64_max_plus_one_from_json() {
    let value = i64::MAX as u64 + 1;
    let json = format!(r#"{{"sentinel": {}}}"#, value);
    let node = Node::parse_str(&json, Format::Json).unwrap();
    let cfg: Sentinel = node.as_type().unwrap();
    assert_eq!(cfg.sentinel, value);
}

#[test]
fn parse_i64_max_as_both_types_from_json() {
    let json = format!(r#"{{"sentinel": {}}}"#, i64::MAX);
    let node = Node::parse_str(&json, Format::Json).unwrap();

    // Works as u64
    let cfg: Sentinel = node.as_type().unwrap();
    assert_eq!(cfg.sentinel, i64::MAX as u64);

    // Works as i64
    let cfg: SignedSentinel = node.as_type().unwrap();
    assert_eq!(cfg.sentinel, i64::MAX);
}

// ---------------------------------------------------------------------------------------------- //
// JSON5 Tests
//
// Note: The json5 crate can't parse integers > i64::MAX directly.
// Large u64 values must be provided as strings in JSON5.

#[test]
fn parse_string_u64_max_from_json5() {
    // Large u64 values must be provided as strings in JSON5
    let json5 = format!(r#"{{ sentinel: "{}" }}"#, u64::MAX);
    let node = Node::parse_str(&json5, Format::Json5).unwrap();
    let cfg: Sentinel = node.as_type().unwrap();
    assert_eq!(cfg.sentinel, u64::MAX);
}

#[test]
fn parse_i64_max_from_json5() {
    // i64::MAX is the largest integer JSON5 can parse directly
    let json5 = format!(r#"{{ sentinel: {} }}"#, i64::MAX);
    let node = Node::parse_str(&json5, Format::Json5).unwrap();
    let cfg: Sentinel = node.as_type().unwrap();
    assert_eq!(cfg.sentinel, i64::MAX as u64);
}

// ---------------------------------------------------------------------------------------------- //
// Rhai Tests

#[test]
fn parse_string_u64_max_from_rhai() {
    // Large u64 values must be provided as strings in Rhai (since Rhai uses i64)
    let rhai = format!(r#"#{{ sentinel: "{}" }}"#, u64::MAX);
    let node = Node::parse_str(&rhai, Format::Rhai).unwrap();
    let cfg: Sentinel = node.as_type().unwrap();
    assert_eq!(cfg.sentinel, u64::MAX);
}

#[test]
fn parse_string_i64_max_plus_one_from_rhai() {
    let value = i64::MAX as u64 + 1;
    let rhai = format!(r#"#{{ sentinel: "{}" }}"#, value);
    let node = Node::parse_str(&rhai, Format::Rhai).unwrap();
    let cfg: Sentinel = node.as_type().unwrap();
    assert_eq!(cfg.sentinel, value);
}

#[test]
fn parse_small_u64_from_rhai_int() {
    // Small u64 values can be provided as regular integers
    let rhai = r#"#{ sentinel: 1000 }"#;
    let node = Node::parse_str(rhai, Format::Rhai).unwrap();
    let cfg: Sentinel = node.as_type().unwrap();
    assert_eq!(cfg.sentinel, 1000);
}

#[test]
fn parse_i64_max_from_rhai() {
    // i64::MAX fits in Rhai's native integer type
    let rhai = format!(r#"#{{ sentinel: {} }}"#, i64::MAX);
    let node = Node::parse_str(&rhai, Format::Rhai).unwrap();
    let cfg: Sentinel = node.as_type().unwrap();
    assert_eq!(cfg.sentinel, i64::MAX as u64);
}

// ---------------------------------------------------------------------------------------------- //
// Error Cases

#[test]
fn negative_value_fails_as_u64() {
    let json = r#"{"sentinel": -42}"#;
    let node = Node::parse_str(json, Format::Json).unwrap();
    let err = node.as_type::<Sentinel>().unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("sentinel") || msg.contains("-42") || msg.contains("negative"),
        "error should mention the issue: {msg}"
    );
}
