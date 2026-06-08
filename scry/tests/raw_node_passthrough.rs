//! Tests for raw Node passthrough fields.
#![cfg(feature = "format-json")]

use scry::node::Format;
use scry::{FromNode, Node, ToNode};

// ---------------------------------------------------------------------------------------------- //
// Test Types

#[derive(Debug, Clone, FromNode, ToNode)]
struct RawPayloadConfig {
    name: String,
    raw: Node,
}

#[derive(Debug, Clone, FromNode, ToNode)]
struct OptionalRawPayloadConfig {
    name: String,
    raw: Option<Node>,
}

#[derive(Debug, Clone, FromNode)]
#[allow(dead_code)]
struct StrictConfig {
    name: String,
}

fn node(json: &str) -> Node {
    Node::parse_str(json, Format::Json).unwrap()
}

// ---------------------------------------------------------------------------------------------- //
// Tests

#[test]
fn raw_node_field_accepts_arbitrary_nested_payload() {
    let n = node(
        r#"{
            "name": "demo",
            "raw": {
                "anything": {
                    "nested": [1, true, "x"]
                }
            }
        }"#,
    );

    let cfg: RawPayloadConfig = n.as_type().unwrap();

    assert_eq!(cfg.name, "demo");
    assert_eq!(cfg.raw.req::<String>("anything.nested[2]").unwrap(), "x");
    n.ensure_no_unknown_keys().unwrap();
}

#[test]
fn raw_node_field_serializes_as_the_original_subtree() {
    let cfg = RawPayloadConfig {
        name: "demo".to_string(),
        raw: node(
            r#"{
                "anything": {
                    "nested": [1, true, "x"]
                }
            }"#,
        ),
    };

    let out = cfg.to_node().unwrap();

    assert_eq!(out.req::<String>("name").unwrap(), "demo");
    assert_eq!(out.req::<u64>("raw.anything.nested[0]").unwrap(), 1);
    assert!(out.req::<bool>("raw.anything.nested[1]").unwrap());
    assert_eq!(out.req::<String>("raw.anything.nested[2]").unwrap(), "x");
}

#[test]
fn optional_raw_node_field_can_be_absent() {
    let n = node(r#"{ "name": "demo" }"#);

    let cfg: OptionalRawPayloadConfig = n.as_type().unwrap();

    assert_eq!(cfg.name, "demo");
    assert!(cfg.raw.is_none());
    n.ensure_no_unknown_keys().unwrap();
}

#[test]
fn optional_raw_node_field_accepts_arbitrary_nested_payload_when_present() {
    let n = node(
        r#"{
            "name": "demo",
            "raw": {
                "plugin": {
                    "payload": {
                        "enabled": true
                    }
                }
            }
        }"#,
    );

    let cfg: OptionalRawPayloadConfig = n.as_type().unwrap();
    let raw = cfg.raw.expect("expected raw payload");

    assert!(raw.req::<bool>("plugin.payload.enabled").unwrap());
    n.ensure_no_unknown_keys().unwrap();
}

#[test]
fn unknown_keys_still_fail_without_raw_node_field() {
    let n = node(
        r#"{
            "name": "demo",
            "raw": {
                "anything": true
            }
        }"#,
    );

    let err = n.as_type::<StrictConfig>().unwrap_err();

    assert!(err.to_string().contains("raw.anything"));
}
