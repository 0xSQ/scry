//! Checks uniform field hooks, locations, and documentation across positional shapes.

use std::error::Error;
use std::io;

use scry::desc::{DescKind, VariantRepr};
use scry::node::Format;
use scry::{Desc, Describe, FromNode, KeyPath, Node, NodeError, ToNode};

// ---------------------------------------------------------------------------------------------- //

#[derive(Debug, PartialEq, FromNode, ToNode, Describe)]
struct Transparent(
    #[scry(
        from_node_with(read_value),
        to_node_with(write_value),
        describe_with(value_description)
    )]
    DomainValue,
);

#[derive(Debug, PartialEq, FromNode, ToNode, Describe)]
struct Pair(
    u8,
    #[scry(
        from_node_with(read_value),
        to_node_with(write_value),
        describe_with(value_description)
    )]
    DomainValue,
);

#[derive(Debug, PartialEq, FromNode, ToNode, Describe)]
struct OptionalTransparent(
    #[scry(
        from_node_with(read_optional),
        to_node_with(write_optional),
        describe_with(optional_description)
    )]
    Option<DomainValue>,
);

#[derive(Debug, PartialEq, FromNode, ToNode, Describe)]
enum Payload {
    #[scry(rename = "single.key")]
    Single(
        #[scry(
            from_node_with(read_value),
            to_node_with(write_value),
            describe_with(value_description)
        )]
        DomainValue,
    ),
    #[scry(rename = "pair.key")]
    Pair(
        u8,
        #[scry(
            from_node_with(read_value),
            to_node_with(write_value),
            describe_with(value_description)
        )]
        DomainValue,
    ),
}

#[derive(Debug, FromNode, ToNode)]
struct Nested {
    #[scry(rename = "items.key")]
    items: Vec<Pair>,
}

/// An owning envelope.
///
/// Extended owner details are omitted from the summary.
#[derive(Describe)]
#[allow(dead_code)]
struct DocumentedTransparent(
    /// A positional field.
    #[scry(describe_with(value_description))]
    DomainValue,
);

#[derive(Describe)]
#[allow(dead_code)]
struct FieldDocumentedTransparent(
    /// A positional field.
    ///
    /// Extended field details are omitted from the summary.
    #[scry(describe_with(value_description))]
    DomainValue,
);

/// A documented pair.
#[derive(Describe)]
#[allow(dead_code)]
struct DocumentedPair(
    /// The position counter.
    u8,
    /// An encoded payload.
    #[scry(describe_with(value_description))]
    DomainValue,
);

/// A documented selection.
#[derive(Describe)]
#[allow(dead_code)]
enum DocumentedPayload {
    /// One encoded value.
    Single(
        /// The payload value.
        #[scry(describe_with(value_description))]
        DomainValue,
    ),
    /// A counter and encoded value.
    Pair(
        /// The position counter.
        u8,
        #[scry(describe_with(value_description))] DomainValue,
    ),
}

#[derive(Debug, PartialEq)]
struct DomainValue(u16);

// ---------------------------------------------------------------------------------------------- //

#[test]
fn transparent_positional_hooks_replace_native_traits_and_preserve_the_wire_value() {
    let value: Transparent = parse("17");
    assert_eq!(value.0, DomainValue(17));
    assert_eq!(value.to_node().unwrap().as_type::<u16>().unwrap(), 17);
    let description = Transparent::describe();
    assert_eq!(description.type_label(), "encoded integer");
    assert_eq!(description.doc, "Domain encoding.");
}

#[test]
fn tuple_struct_hooks_preserve_native_siblings_and_exact_array_arity() {
    let value: Pair = parse("[3, 17]");
    assert_eq!(value, Pair(3, DomainValue(17)));
    let output = value.to_node().unwrap();
    let items = output.as_vec().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].as_type::<u8>().unwrap(), 3);
    assert_eq!(items[1].as_type::<u16>().unwrap(), 17);
    let description = Pair::describe();
    let DescKind::Tuple { items } = description.kind else {
        panic!("expected tuple");
    };
    assert_eq!(items[0].type_label(), "u8");
    assert_eq!(items[1].type_label(), "encoded integer");

    for source in ["[3]", "[3, 17, 19]"] {
        let error = node(source).as_type::<Pair>().unwrap_err();
        assert!(matches!(error, NodeError::ArrayLength { expected: 2, .. }));
    }
}

#[test]
fn positional_hooks_receive_the_complete_optional_type() {
    let value: OptionalTransparent = parse("()");
    assert_eq!(value.0, Some(DomainValue(23)));
    assert_eq!(OptionalTransparent(None).to_node().unwrap().as_type::<String>().unwrap(), "none");
    assert_eq!(value.to_node().unwrap().as_type::<u16>().unwrap(), 23);
    assert_eq!(OptionalTransparent::describe().type_label(), "encoded integer | null");
}

#[test]
fn enum_positional_hooks_use_literal_variant_names_and_payload_indices() {
    let single: Payload = parse("#{ \"single.key\": 17 }");
    assert_eq!(single, Payload::Single(DomainValue(17)));
    assert_eq!(
        single.to_node().unwrap().req::<u16>(KeyPath::from_keys(["single.key"])).unwrap(),
        17
    );

    let pair: Payload = parse("#{ \"pair.key\": [3, 17] }");
    assert_eq!(pair, Payload::Pair(3, DomainValue(17)));
    assert_eq!(pair.to_node().unwrap().as_type::<Payload>().unwrap(), pair);
    let description = Payload::describe();
    description.validate_path(r#"["single.key"]"#).unwrap();
    description.validate_path(r#"["pair.key"][1]"#).unwrap();
    assert!(description.validate_path("single.key").is_err());
}

#[test]
fn positional_input_hooks_attach_existing_absolute_locations_once() {
    let cases = [
        (
            "#{ scope: 0 }",
            input_error::<Transparent> as fn(&str) -> NodeError,
            KeyPath::from_keys(["scope"]),
        ),
        ("#{ scope: [3, 0] }", input_error::<Pair>, KeyPath::from_keys(["scope"]).push_index(1)),
        (
            "#{ scope: #{ \"single.key\": 0 } }",
            input_error::<Payload>,
            KeyPath::from_keys(["scope", "single.key"]),
        ),
        (
            "#{ scope: #{ \"pair.key\": [3, 0] } }",
            input_error::<Payload>,
            KeyPath::from_keys(["scope", "pair.key"]).push_index(1),
        ),
        (
            "#{ scope: #{ \"items.key\": [[3, 0]] } }",
            input_error::<Nested>,
            KeyPath::from_keys(["scope", "items.key"]).push_index(0).push_index(1),
        ),
    ];
    for (source, parse_error, expected) in cases {
        let error = parse_error(source);
        assert_eq!(error.path(), Some(&expected));
        assert!(find_io_source(&error).is_some());
    }
}

#[test]
fn positional_output_hooks_compose_relative_locations_once() {
    let cases = [
        (Transparent(DomainValue(0)).to_node().unwrap_err(), KeyPath::new()),
        (Pair(3, DomainValue(0)).to_node().unwrap_err(), KeyPath::from_index(1)),
        (
            Payload::Single(DomainValue(0)).to_node().unwrap_err(),
            KeyPath::from_keys(["single.key"]),
        ),
        (
            Payload::Pair(3, DomainValue(0)).to_node().unwrap_err(),
            KeyPath::from_keys(["pair.key"]).push_index(1),
        ),
        (
            Nested {
                items: vec![Pair(3, DomainValue(0))],
            }
            .to_node()
            .unwrap_err(),
            KeyPath::from_keys(["items.key"]).push_index(0).push_index(1),
        ),
    ];
    for (error, expected) in cases {
        assert_eq!(error.path(), Some(&expected));
        assert!(find_io_source(&error).is_some());
    }
}

#[test]
fn positional_descriptions_preserve_owner_field_and_delegated_documentation() {
    assert_eq!(DocumentedTransparent::describe().doc, "An owning envelope.");
    assert_eq!(FieldDocumentedTransparent::describe().doc, "A positional field.");
    let description = DocumentedPair::describe();
    assert_eq!(description.doc, "A documented pair.");
    let DescKind::Tuple { items } = description.kind else {
        panic!("expected tuple");
    };
    assert_eq!(items[0].doc, "The position counter.");
    assert_eq!(items[1].doc, "An encoded payload.");

    let description = DocumentedPayload::describe();
    assert_eq!(description.doc, "A documented selection.");
    let DescKind::Enum { variants } = description.kind else {
        panic!("expected variants");
    };
    assert_eq!(variants[0].doc, "One encoded value.");
    let VariantRepr::Payload { payload, .. } = &variants[0].repr else {
        panic!("expected payload");
    };
    assert_eq!(payload.doc, "The payload value.");
    assert_eq!(variants[1].doc, "A counter and encoded value.");
    let VariantRepr::Payload { payload, .. } = &variants[1].repr else {
        panic!("expected payload");
    };
    let DescKind::Tuple { items } = &payload.kind else {
        panic!("expected tuple");
    };
    assert_eq!(items[0].doc, "The position counter.");
    assert_eq!(items[1].doc, "Domain encoding.");
}

// ---------------------------------------------------------------------------------------------- //

fn read_value(node: &Node) -> Result<DomainValue, NodeError> {
    let value: u16 = node.as_type()?;
    if value == 0 {
        Err(NodeError::with_context("input rejected", io::Error::other("domain cause")))
    } else {
        Ok(DomainValue(value))
    }
}

fn write_value(value: &DomainValue) -> Result<Node, NodeError> {
    if value.0 == 0 {
        Err(NodeError::invalid_value_with_source(
            &KeyPath::new(),
            "output rejected",
            io::Error::other("domain cause"),
        ))
    } else {
        value.0.to_node()
    }
}

fn value_description() -> Desc {
    Desc::plain("encoded integer").with_doc("Domain encoding.")
}

fn read_optional(node: &Node) -> Result<Option<DomainValue>, NodeError> {
    if node.as_type::<Option<u16>>()?.is_none() {
        Ok(Some(DomainValue(23)))
    } else {
        read_value(node).map(Some)
    }
}

fn write_optional(value: &Option<DomainValue>) -> Result<Node, NodeError> {
    match value {
        Some(value) => write_value(value),
        None => "none".to_node(),
    }
}

fn optional_description() -> Desc {
    value_description().nullable()
}

fn input_error<T: FromNode>(source: &str) -> NodeError {
    node(source).req_node("scope").unwrap().as_type::<T>().err().unwrap()
}

fn find_io_source<'a>(error: &'a (dyn Error + 'static)) -> Option<&'a io::Error> {
    let mut current = Some(error);
    while let Some(error) = current {
        if let Some(source) = error.downcast_ref::<io::Error>() {
            return Some(source);
        }
        current = error.source();
    }
    None
}

fn parse<T: FromNode>(source: &str) -> T {
    node(source).as_type().unwrap()
}

fn node(source: &str) -> Node {
    Node::parse_str(source, Format::Rhai).unwrap()
}
