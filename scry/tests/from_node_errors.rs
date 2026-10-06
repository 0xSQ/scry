//! Covers original primitive causes and logical input locations.

use std::error::Error;
use std::num::{ParseFloatError, ParseIntError, TryFromIntError};
use std::str::ParseBoolError;

use scry::node::Format;
use scry::{FromNode, KeyPath, Node, NodeError};

// ---------------------------------------------------------------------------------------------- //

struct Custom;

impl FromNode for Custom {
    fn from_node(_node: &Node) -> Result<Self, NodeError> {
        Err(NodeError::with_context(
            "custom decoder failed",
            std::io::Error::other("original cause"),
        ))
    }
}

#[test]
fn string_number_and_boolean_conversions_keep_concrete_parser_causes() {
    let input = Node::parse_str(
        r#"#{ limit: "invalid", scale: "invalid", enabled: "invalid" }"#,
        Format::Rhai,
    )
    .unwrap();
    let integer = input.req::<u16>("limit").unwrap_err();
    assert!(matches!(integer, NodeError::InvalidConversion { .. }));
    assert!(integer.source().unwrap().is::<ParseIntError>());
    assert_eq!(integer.path(), Some(&KeyPath::from_keys(["limit"])));
    let float = input.req::<f64>("scale").unwrap_err();
    assert!(float.source().unwrap().is::<ParseFloatError>());
    let boolean = input.req::<bool>("enabled").unwrap_err();
    assert!(boolean.source().unwrap().is::<ParseBoolError>());
}

#[test]
fn integer_range_failures_retain_the_original_range_error() {
    let input = Node::parse_str("#{ limit: -1 }", Format::Rhai).unwrap();
    let error = input.req::<u8>("limit").unwrap_err();
    assert!(error.source().unwrap().is::<TryFromIntError>());
    assert_eq!(error.path(), Some(&KeyPath::from_keys(["limit"])));
}

#[test]
fn unsupported_conversion_shapes_still_have_no_fabricated_cause() {
    let input = Node::parse_str("#{ limit: true }", Format::Rhai).unwrap();
    let error = input.req::<u16>("limit").unwrap_err();
    assert!(matches!(error, NodeError::InvalidConversion { .. }));
    assert!(error.source().is_none());
}

#[test]
fn typed_input_reads_attach_the_child_location_to_locationless_errors() {
    let input = Node::parse_str("#{ jobs: [#{ limit: 1 }] }", Format::Rhai).unwrap();
    let error = input.req::<Custom>("jobs[0].limit").err().unwrap();
    let path = KeyPath::from_keys(["jobs"]).push_index(0).push_key("limit");
    assert_eq!(error.path(), Some(&path));
    let original = error.source().unwrap().downcast_ref::<NodeError>().unwrap();
    assert!(original.source().unwrap().is::<std::io::Error>());
}

#[test]
fn fixed_array_children_receive_their_complete_input_location() {
    let input = Node::parse_str("#{ jobs: [#{ limits: [1] }] }", Format::Rhai).unwrap();
    let error = input.req::<[Custom; 1]>("jobs[0].limits").err().unwrap();
    assert_eq!(error.path(), Some(&"jobs[0].limits[0]".parse::<KeyPath>().unwrap()));
    let original = error.source().unwrap().downcast_ref::<NodeError>().unwrap();
    assert!(original.source().unwrap().is::<std::io::Error>());
}

#[test]
fn direct_optional_decoding_attaches_the_input_location() {
    let input = Node::parse_str("#{ limit: 1 }", Format::Rhai).unwrap();
    let node = input.req_node("limit").unwrap();
    let error = Option::<Custom>::from_node(node).err().unwrap();
    assert_eq!(error.path(), Some(&KeyPath::from_keys(["limit"])));
    let original = error.source().unwrap().downcast_ref::<NodeError>().unwrap();
    assert!(original.source().unwrap().is::<std::io::Error>());
}
