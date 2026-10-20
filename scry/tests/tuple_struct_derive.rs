//! Checks transparent and positional wire forms for derived tuple structs.

use scry::node::Format;
use scry::{FromNode, Node, NodeError, ToNode};

// ---------------------------------------------------------------------------------------------- //

#[derive(Debug, PartialEq, FromNode, ToNode)]
struct Port(u16);

#[derive(Debug, PartialEq, FromNode, ToNode)]
struct Pair(String, i32);

#[derive(Debug, PartialEq, FromNode, ToNode)]
struct Triple(u8, u8, u8);

#[derive(Debug, PartialEq, FromNode, ToNode)]
struct NewtypeVec(Vec<i32>);

#[test]
fn newtypes_preserve_their_complete_inner_wire_form() {
    let port: Port = node("8080").as_type().unwrap();
    assert_eq!(port, Port(8080));
    assert_eq!(port.to_node().unwrap().as_type::<u16>().unwrap(), 8080);

    let values: NewtypeVec = node("[1, 2, 3]").as_type().unwrap();
    assert_eq!(values, NewtypeVec(vec![1, 2, 3]));
    assert_eq!(values.to_node().unwrap().as_type::<Vec<i32>>().unwrap(), [1, 2, 3]);
}

#[test]
fn tuples_preserve_field_order_and_values_in_each_direction() {
    let pair: Pair = node(r#"["hello", 42]"#).as_type().unwrap();
    assert_eq!(pair, Pair("hello".into(), 42));
    assert_eq!(pair.to_node().unwrap().as_type::<(String, i32)>().unwrap(), ("hello".into(), 42));

    let triple: Triple = node("[255, 128, 0]").as_type().unwrap();
    assert_eq!(triple, Triple(255, 128, 0));
    assert_eq!(triple.to_node().unwrap().as_type::<[u8; 3]>().unwrap(), [255, 128, 0]);
}

#[test]
fn tuple_arity_errors_report_expected_and_actual_lengths() {
    for (source, found) in [(r#"["hello"]"#, 1), (r#"["hello", 42, true]"#, 3)] {
        let error = node(source).as_type::<Pair>().unwrap_err();
        assert!(matches!(error, NodeError::ArrayLength { path, expected: 2, found: actual }
            if path.is_empty() && actual == found));
    }
}

fn node(source: &str) -> Node {
    Node::parse_str(source, Format::Rhai).unwrap()
}
