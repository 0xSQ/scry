//! Checks logical error locations and source retention in generated conversions.
#![cfg(feature = "format-json")]

use std::error::Error;
use std::io;
use std::num::ParseIntError;

use scry::node::Format;
use scry::{FromNode, KeyPath, Node, NodeError, ToNode};

// ---------------------------------------------------------------------------------------------- //

#[derive(Debug, Clone, Copy)]
enum FailedValue {
    Message,
    MessageWithSource,
    Located,
    LocatedWithSource,
}

impl ToNode for FailedValue {
    fn to_node(&self) -> Result<Node, NodeError> {
        Err(match self {
            Self::Message => NodeError::new("value rejected"),
            Self::MessageWithSource => {
                NodeError::with_context("value rejected", io::Error::other("original cause"))
            }
            Self::Located => NodeError::invalid_value(&relative_path(), "value rejected"),
            Self::LocatedWithSource => NodeError::invalid_value_with_source(
                &relative_path(),
                "value rejected",
                io::Error::other("original cause"),
            ),
        })
    }
}

#[derive(ToNode)]
struct NamedOutput {
    #[scry(rename = "wire.key")]
    value: FailedValue,
}

#[derive(ToNode)]
struct HookOutput {
    #[scry(rename = "hook.key", to_node_with(write_hook))]
    value: FailedValue,
}

#[derive(ToNode)]
struct TupleOutput(u8, FailedValue);

#[derive(ToNode)]
struct TransparentOutput(FailedValue);

#[derive(ToNode)]
enum PayloadOutput {
    #[scry(rename = "idle.key")]
    Unit,
    #[scry(rename = "single.key")]
    Single(FailedValue),
    #[scry(rename = "pair.key")]
    Pair(u8, FailedValue),
    #[scry(rename = "named.key")]
    Named {
        #[scry(rename = "field.key")]
        value: FailedValue,
    },
    #[scry(rename = "hook.variant")]
    Hook {
        #[scry(rename = "hook.field", to_node_with(write_hook))]
        value: FailedValue,
    },
}

#[derive(ToNode)]
struct NestedOutput {
    #[scry(rename = "outer.key")]
    value: NestedTuple,
}

#[derive(ToNode)]
struct NestedTuple(u8, PayloadOutput);

#[derive(Debug, FromNode)]
struct NamedInput {
    #[scry(rename = "wire.key")]
    value: u16,
}

#[derive(Debug, FromNode)]
struct TupleInput(u8, u16);

#[derive(Debug, FromNode)]
struct TransparentInput(u16);

#[derive(Debug, FromNode)]
enum PayloadInput {
    #[scry(rename = "single.key")]
    Single(u16),
    #[scry(rename = "pair.key")]
    Pair(u8, u16),
    #[scry(rename = "named.key")]
    Named {
        #[scry(rename = "field.key")]
        value: u16,
    },
    Items(Vec<u16>),
}

#[derive(Debug, FromNode)]
struct HookInput {
    #[scry(rename = "hook.key", from_node_with(read_hook))]
    value: u16,
}

#[derive(Debug, FromNode)]
enum HookPayloadInput {
    #[scry(rename = "hook.variant")]
    Named {
        #[scry(rename = "hook.field", from_node_with(read_hook))]
        value: u16,
    },
}

#[derive(Debug)]
struct CustomInput;

impl FromNode for CustomInput {
    fn from_node(node: &Node) -> Result<Self, NodeError> {
        if node.as_type::<u16>()? == 0 {
            Err(NodeError::with_context(
                "custom input rejected",
                io::Error::other("original cause"),
            ))
        } else {
            Ok(Self)
        }
    }
}

#[derive(Debug, FromNode)]
enum CustomPayloadInput {
    Single(CustomInput),
    Pair(u8, CustomInput),
}

// ---------------------------------------------------------------------------------------------- //

#[test]
fn named_serializers_prefix_wire_keys_for_children_and_hooks() {
    for failure in failures() {
        assert_output_error(
            NamedOutput { value: failure }.to_node().unwrap_err(),
            failure,
            KeyPath::from_keys(["wire.key"]),
        );
        assert_output_error(
            HookOutput { value: failure }.to_node().unwrap_err(),
            failure,
            KeyPath::from_keys(["hook.key"]),
        );
    }
}

#[test]
fn tuple_structs_prefix_indices_and_transparent_structs_preserve_errors() {
    for failure in failures() {
        assert_output_error(
            TupleOutput(7, failure).to_node().unwrap_err(),
            failure,
            KeyPath::from_index(1),
        );

        let direct = failure.to_node().unwrap_err();
        let transparent = TransparentOutput(failure).to_node().unwrap_err();
        assert_eq!(transparent.path(), direct.path());
        assert_eq!(transparent.to_string(), direct.to_string());
        assert_eq!(find_source::<io::Error>(&transparent).is_some(), has_source(failure));
    }
}

#[test]
fn enum_serializers_prefix_each_payload_shape_and_hook() {
    for failure in failures() {
        let cases = [
            (PayloadOutput::Single(failure), KeyPath::from_keys(["single.key"])),
            (PayloadOutput::Pair(7, failure), KeyPath::from_keys(["pair.key"]).push_index(1)),
            (
                PayloadOutput::Named { value: failure },
                KeyPath::from_keys(["named.key", "field.key"]),
            ),
            (
                PayloadOutput::Hook { value: failure },
                KeyPath::from_keys(["hook.variant", "hook.field"]),
            ),
        ];
        for (value, prefix) in cases {
            assert_output_error(value.to_node().unwrap_err(), failure, prefix);
        }
    }

    assert_eq!(PayloadOutput::Unit.to_node().unwrap().as_type::<String>().unwrap(), "idle.key");
}

#[test]
fn nested_derive_serializers_compose_paths_once_and_retain_sources() {
    for failure in failures() {
        let value = NestedOutput {
            value: NestedTuple(3, PayloadOutput::Pair(7, failure)),
        };
        let prefix =
            KeyPath::from_keys(["outer.key"]).push_index(1).push_key("pair.key").push_index(1);
        assert_output_error(value.to_node().unwrap_err(), failure, prefix);
    }
}

#[test]
fn derived_input_preserves_absolute_paths_for_each_shape() {
    let outer = KeyPath::from_keys(["outer.key"]);
    let cases = [
        (
            subtree(r#"{ "outer.key": { "wire.key": "bad" } }"#)
                .as_type::<NamedInput>()
                .unwrap_err(),
            outer.push_key("wire.key"),
        ),
        (
            subtree(r#"{ "outer.key": [7, "bad"] }"#).as_type::<TupleInput>().unwrap_err(),
            outer.push_index(1),
        ),
        (
            subtree(r#"{ "outer.key": "bad" }"#).as_type::<TransparentInput>().unwrap_err(),
            outer.clone(),
        ),
        (
            subtree(r#"{ "outer.key": { "single.key": "bad" } }"#)
                .as_type::<PayloadInput>()
                .unwrap_err(),
            outer.push_key("single.key"),
        ),
        (
            subtree(r#"{ "outer.key": { "pair.key": [7, "bad"] } }"#)
                .as_type::<PayloadInput>()
                .unwrap_err(),
            outer.push_key("pair.key").push_index(1),
        ),
        (
            subtree(r#"{ "outer.key": { "named.key": { "field.key": "bad" } } }"#)
                .as_type::<PayloadInput>()
                .unwrap_err(),
            outer.push_key("named.key").push_key("field.key"),
        ),
    ];
    for (error, expected) in cases {
        assert_eq!(error.path(), Some(&expected));
        assert!(find_source::<ParseIntError>(&error).is_some());
    }

    assert_eq!(node(r#"{ "wire.key": 42 }"#).as_type::<NamedInput>().unwrap().value, 42);
    let TupleInput(first, second) = node("[7, 42]").as_type().unwrap();
    assert_eq!((first, second), (7, 42));
    assert_eq!(node("42").as_type::<TransparentInput>().unwrap().0, 42);
    let PayloadInput::Single(value) = node(r#"{ "single.key": 42 }"#).as_type().unwrap() else {
        panic!("expected a single payload");
    };
    assert_eq!(value, 42);
    let PayloadInput::Pair(first, second) = node(r#"{ "pair.key": [7, 42] }"#).as_type().unwrap()
    else {
        panic!("expected a tuple payload");
    };
    assert_eq!((first, second), (7, 42));
    let PayloadInput::Named { value } =
        node(r#"{ "named.key": { "field.key": 42 } }"#).as_type().unwrap()
    else {
        panic!("expected a named payload");
    };
    assert_eq!(value, 42);
}

#[test]
fn input_hooks_locate_generic_errors_and_preserve_native_paths() {
    for (input, sourced) in [("plain", false), ("sourced", true)] {
        let source = format!(r#"{{ "outer.key": {{ "hook.key": "{input}" }} }}"#);
        let error = subtree(&source).as_type::<HookInput>().unwrap_err();
        assert_eq!(error.path(), Some(&KeyPath::from_keys(["outer.key", "hook.key"])));
        assert_eq!(find_source::<io::Error>(&error).is_some(), sourced);

        let source =
            format!(r#"{{ "outer.key": {{ "hook.variant": {{ "hook.field": "{input}" }} }} }}"#,);
        let error = subtree(&source).as_type::<HookPayloadInput>().unwrap_err();
        assert_eq!(
            error.path(),
            Some(&KeyPath::from_keys(["outer.key", "hook.variant", "hook.field"])),
        );
        assert_eq!(find_source::<io::Error>(&error).is_some(), sourced);
    }

    let error =
        subtree(r#"{ "outer.key": { "hook.key": "bad" } }"#).as_type::<HookInput>().unwrap_err();
    assert_eq!(error.path(), Some(&KeyPath::from_keys(["outer.key", "hook.key"])));
    assert!(find_source::<ParseIntError>(&error).is_some());

    assert_eq!(node(r#"{ "hook.key": "42" }"#).as_type::<HookInput>().unwrap().value, 42);
    let HookPayloadInput::Named { value } =
        node(r#"{ "hook.variant": { "hook.field": "42" } }"#).as_type().unwrap();
    assert_eq!(value, 42);
}

#[test]
fn native_enum_payload_decoding_locates_generic_child_errors() {
    let outer = KeyPath::from_keys(["outer.key"]);
    let error =
        subtree(r#"{ "outer.key": { "single": 0 } }"#).as_type::<CustomPayloadInput>().unwrap_err();
    assert_eq!(error.path(), Some(&outer.push_key("single")));
    assert!(find_source::<io::Error>(&error).is_some());

    let error = subtree(r#"{ "outer.key": { "pair": [7, 0] } }"#)
        .as_type::<CustomPayloadInput>()
        .unwrap_err();
    assert_eq!(error.path(), Some(&outer.push_key("pair").push_index(1)));
    assert!(find_source::<io::Error>(&error).is_some());

    let CustomPayloadInput::Single(CustomInput) = node(r#"{ "single": 1 }"#).as_type().unwrap()
    else {
        panic!("expected a single custom payload");
    };
    let CustomPayloadInput::Pair(first, CustomInput) =
        node(r#"{ "pair": [7, 1] }"#).as_type().unwrap()
    else {
        panic!("expected a tuple custom payload");
    };
    assert_eq!(first, 7);
}

#[test]
fn single_element_array_hint_retains_the_original_error_and_its_source() {
    let error = subtree(r#"{ "outer.key": { "single.key": ["bad"] } }"#)
        .as_type::<PayloadInput>()
        .unwrap_err();
    assert!(error.to_string().contains("hint: payload is a 1-element array"));
    let original = error.source().unwrap().downcast_ref::<NodeError>().unwrap();
    let expected = KeyPath::from_keys(["outer.key", "single.key"]);
    assert_eq!(error.path(), Some(&expected));
    assert_eq!(original.path(), Some(&expected));
    assert!(matches!(original, NodeError::TypeMismatch { .. }));

    let error =
        subtree(r#"{ "outer.key": { "items": ["bad"] } }"#).as_type::<PayloadInput>().unwrap_err();
    assert!(error.to_string().contains("hint: payload is a 1-element array"));
    let original = error.source().unwrap().downcast_ref::<NodeError>().unwrap();
    let expected = KeyPath::from_keys(["outer.key", "items"]);
    assert_eq!(error.path(), Some(&expected));
    assert_eq!(original.path(), Some(&expected.push_index(0)));
    assert!(find_source::<ParseIntError>(&error).is_some());

    let PayloadInput::Items(values) = node(r#"{ "items": [42] }"#).as_type().unwrap() else {
        panic!("expected an array payload");
    };
    assert_eq!(values, [42]);
}

// ---------------------------------------------------------------------------------------------- //

fn write_hook(value: &FailedValue) -> Result<Node, NodeError> {
    value.to_node()
}

fn read_hook(node: &Node) -> Result<u16, NodeError> {
    match node.as_type::<String>()?.as_str() {
        "plain" => Err(NodeError::new("hook rejected input")),
        "sourced" => {
            Err(NodeError::with_context("hook rejected input", io::Error::other("original cause")))
        }
        _ => node.as_type(),
    }
}

fn failures() -> [FailedValue; 4] {
    [
        FailedValue::Message,
        FailedValue::MessageWithSource,
        FailedValue::Located,
        FailedValue::LocatedWithSource,
    ]
}

fn relative_path() -> KeyPath {
    KeyPath::from_keys(["child.key"]).push_index(2)
}

fn has_source(value: FailedValue) -> bool {
    matches!(value, FailedValue::MessageWithSource | FailedValue::LocatedWithSource)
}

fn assert_output_error(error: NodeError, value: FailedValue, prefix: KeyPath) {
    let expected = if matches!(value, FailedValue::Located | FailedValue::LocatedWithSource) {
        prefix.join(&relative_path())
    } else {
        prefix
    };
    assert_eq!(error.path(), Some(&expected));
    assert!(error.to_string().contains(&expected.to_string()));
    assert_eq!(find_source::<io::Error>(&error).is_some(), has_source(value));
    if let Some(source) = find_source::<io::Error>(&error) {
        assert_eq!(source.to_string(), "original cause");
    }
}

fn find_source<'a, T: Error + 'static>(error: &'a (dyn Error + 'static)) -> Option<&'a T> {
    let mut current = Some(error);
    while let Some(error) = current {
        if let Some(source) = error.downcast_ref::<T>() {
            return Some(source);
        }
        current = error.source();
    }
    None
}

fn node(source: &str) -> Node {
    Node::parse_str(source, Format::Json).unwrap()
}

fn subtree(source: &str) -> Node {
    node(source).req_node(KeyPath::from_keys(["outer.key"])).unwrap().clone()
}
