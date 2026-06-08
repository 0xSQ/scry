//! Tests for tuple struct derive support.
#![cfg(feature = "format-json")]

use scry::node::{Format, Kind};
use scry::{Node, ToNode};

// ---------------------------------------------------------------------------------------------- //
// Test Types

#[derive(Debug, Clone, PartialEq, scry::FromNode, scry::ToNode, scry::Describe)]
struct Port(u16);

#[derive(Debug, Clone, PartialEq, scry::FromNode, scry::ToNode, scry::Describe)]
struct Pair(String, i32);

#[derive(Debug, Clone, PartialEq, scry::FromNode, scry::ToNode, scry::Describe)]
struct Triple(u8, u8, u8);

#[derive(Debug, Clone, PartialEq, scry::FromNode, scry::ToNode, scry::Describe)]
struct NewtypeVec(Vec<i32>);

fn node(json: &str) -> Node {
    Node::parse_str(json, Format::Json).unwrap()
}

// ---------------------------------------------------------------------------------------------- //
// Newtype Tests

#[test]
fn newtype_parses_from_scalar() {
    let n = node("8080");
    let port: Port = n.as_type().unwrap();
    assert_eq!(port, Port(8080));
}

#[test]
fn newtype_roundtrip() {
    let original = Port(443);
    let node = original.to_node().unwrap();
    let parsed: Port = node.as_type().unwrap();
    assert_eq!(parsed, original);
}

#[test]
fn newtype_vec_parses_from_array() {
    let n = node("[1, 2, 3]");
    let nv: NewtypeVec = n.as_type().unwrap();
    assert_eq!(nv, NewtypeVec(vec![1, 2, 3]));
}

#[test]
fn newtype_vec_roundtrip() {
    let original = NewtypeVec(vec![10, 20, 30]);
    let node = original.to_node().unwrap();
    let parsed: NewtypeVec = node.as_type().unwrap();
    assert_eq!(parsed, original);
}

// ---------------------------------------------------------------------------------------------- //
// Pair Tests

#[test]
fn pair_parses_from_array() {
    let n = node(r#"["hello", 42]"#);
    let pair: Pair = n.as_type().unwrap();
    assert_eq!(pair, Pair("hello".into(), 42));
}

#[test]
fn pair_roundtrip() {
    let original = Pair("test".into(), 99);
    let node = original.to_node().unwrap();
    let parsed: Pair = node.as_type().unwrap();
    assert_eq!(parsed, original);
}

#[test]
fn pair_wrong_length_errors() {
    let n = node(r#"["hello"]"#);
    let err = n.as_type::<Pair>().unwrap_err();
    assert!(err.to_string().contains("2"));
}

#[test]
fn pair_too_many_elements_errors() {
    let n = node(r#"["hello", 42, true]"#);
    let err = n.as_type::<Pair>().unwrap_err();
    assert!(err.to_string().contains("2"));
}

// ---------------------------------------------------------------------------------------------- //
// Triple Tests

#[test]
fn triple_parses_from_array() {
    let n = node("[255, 128, 0]");
    let rgb: Triple = n.as_type().unwrap();
    assert_eq!(rgb, Triple(255, 128, 0));
}

#[test]
fn triple_roundtrip() {
    let original = Triple(10, 20, 30);
    let node = original.to_node().unwrap();
    let parsed: Triple = node.as_type().unwrap();
    assert_eq!(parsed, original);
}

// ---------------------------------------------------------------------------------------------- //
// ToScry Output Structure Tests

#[test]
fn newtype_to_scry_is_scalar() {
    let port = Port(8080);
    let node = port.to_node().unwrap();
    assert!(matches!(&node.kind, Kind::Leaf(_)));
}

#[test]
fn pair_to_scry_is_array() {
    let pair = Pair("a".into(), 1);
    let node = pair.to_node().unwrap();
    let Kind::Vec(v) = &node.kind else {
        panic!("expected Vec");
    };
    assert_eq!(v.len(), 2);
}
