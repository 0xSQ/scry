//! Checks that generated bindings do not hide field hook functions.

use scry::desc::DescKind;
use scry::node::Format;
use scry::{Config, Describe, FromNode, KeyPath, Node, NodeError, ToNode};

// ---------------------------------------------------------------------------------------------- //

#[derive(Debug, PartialEq, FromNode)]
struct NamedInput {
    #[scry(from_node_with(node))]
    value: Opaque,
}

#[derive(Debug, PartialEq, FromNode)]
struct TransparentInput(#[scry(from_node_with(node))] Opaque);

#[derive(Debug, PartialEq, FromNode)]
struct TupleInput(#[scry(from_node_with(arr))] Opaque, u16);

#[derive(Debug, PartialEq, FromNode)]
enum PayloadInput {
    Single(#[scry(from_node_with(payload))] Opaque),
    Pair(#[scry(from_node_with(arr))] Opaque, u16),
    Named {
        #[scry(from_node_with(payload))]
        value: Opaque,
    },
}

#[derive(ToNode)]
struct NamedOutput {
    #[scry(to_node_with(map))]
    value: Opaque,
}

#[derive(ToNode)]
enum PayloadOutput {
    Single(#[scry(to_node_with(inner))] Opaque),
    Pair(#[scry(to_node_with(f0))] Opaque, u16),
    Named {
        #[scry(to_node_with(map))]
        map: Opaque,
        #[scry(to_node_with(inner_map))]
        inner_map: Opaque,
    },
}

#[derive(Debug, PartialEq, Config, ToNode)]
enum GenericNames<Kind, Result, Vec> {
    #[scry(default)]
    Empty,
    Pair(Kind, Result, Vec),
}

#[derive(Debug, FromNode, ToNode, Describe)]
struct RequiredOption<Option> {
    value: Option,
}

#[derive(Debug, PartialEq)]
struct Opaque(u16);

// ---------------------------------------------------------------------------------------------- //

#[test]
fn input_hooks_can_share_names_with_generated_decoder_bindings() {
    assert_eq!(parse::<NamedInput>("#{ value: 7 }"), NamedInput { value: Opaque(7) });
    assert_eq!(parse::<TransparentInput>("9"), TransparentInput(Opaque(9)));
    assert_eq!(parse::<TupleInput>("[11, 13]"), TupleInput(Opaque(11), 13));

    assert_eq!(parse::<PayloadInput>("#{ single: 17 }"), PayloadInput::Single(Opaque(17)));
    assert_eq!(parse::<PayloadInput>("#{ pair: [19, 23] }"), PayloadInput::Pair(Opaque(19), 23));
    assert_eq!(
        parse::<PayloadInput>("#{ named: #{ value: 29 } }"),
        PayloadInput::Named { value: Opaque(29) }
    );
}

#[test]
fn output_hooks_can_share_names_with_generated_serializer_bindings() {
    let output = NamedOutput { value: Opaque(7) }.to_node().unwrap();
    assert_eq!(output.req::<u16>("value").unwrap(), 7);

    let output = PayloadOutput::Single(Opaque(11)).to_node().unwrap();
    assert_eq!(output.req::<u16>("single").unwrap(), 11);

    let output = PayloadOutput::Pair(Opaque(13), 17).to_node().unwrap();
    assert_eq!(output.req::<Vec<u16>>("pair").unwrap(), [13, 17]);

    let output = PayloadOutput::Named {
        map: Opaque(19),
        inner_map: Opaque(23),
    }
    .to_node()
    .unwrap();
    assert_eq!(output.req::<u16>("named.map").unwrap(), 19);
    assert_eq!(output.req::<u16>("named.inner_map").unwrap(), 23);
}

#[test]
fn generic_parameters_can_share_names_with_generated_runtime_types() {
    let value: GenericNames<u16, String, bool> = parse("#{ pair: [7, \"label\", true] }");
    assert_eq!(value, GenericNames::Pair(7, "label".to_owned(), true));
    assert_eq!(
        value.to_node().unwrap().as_type::<GenericNames<u16, String, bool>>().unwrap(),
        value
    );
    assert_eq!(
        scry::from_defaults::<GenericNames<u16, String, bool>>().unwrap(),
        GenericNames::Empty
    );
    GenericNames::<u16, String, bool>::describe().validate_path("pair[2]").unwrap();
}

#[test]
fn a_generic_parameter_named_option_remains_a_required_field() {
    let value: RequiredOption<u16> = parse("#{ value: 7 }");
    assert_eq!(value.value, 7);
    assert_eq!(value.to_node().unwrap().req::<u16>("value").unwrap(), 7);

    let error =
        Node::parse_str("#{}", Format::Rhai).unwrap().as_type::<RequiredOption<u16>>().unwrap_err();
    assert_eq!(error.path(), Some(&KeyPath::from_keys(["value"])));

    let DescKind::Struct { fields } = RequiredOption::<u16>::describe().kind else {
        panic!("expected fields");
    };
    assert!(!fields[0].optional);
    assert!(!fields[0].value.nullable);
}

mod input_map_bindings {
    use super::{parse, read_value, FromNode, Node, NodeError, Opaque};

    // ------------------------------------------------------------------------------------------ //

    #[derive(Debug, PartialEq, FromNode)]
    enum Input {
        Single(#[scry(from_node_with(map))] Opaque),
        Named {
            #[scry(from_node_with(variant_key))]
            value: Opaque,
        },
    }

    #[test]
    fn input_hooks_can_share_names_with_variant_map_bindings() {
        assert_eq!(parse::<Input>("#{ single: 7 }"), Input::Single(Opaque(7)));
        assert_eq!(
            parse::<Input>("#{ named: #{ value: 11 } }"),
            Input::Named { value: Opaque(11) }
        );
    }

    fn map(input: &Node) -> Result<Opaque, NodeError> {
        read_value(input)
    }

    fn variant_key(input: &Node) -> Result<Opaque, NodeError> {
        read_value(input)
    }
}

// ---------------------------------------------------------------------------------------------- //

fn parse<T: FromNode>(source: &str) -> T {
    Node::parse_str(source, Format::Rhai).unwrap().as_type().unwrap()
}

fn node(input: &Node) -> Result<Opaque, NodeError> {
    read_value(input)
}

fn payload(input: &Node) -> Result<Opaque, NodeError> {
    read_value(input)
}

fn arr(input: &Node) -> Result<Opaque, NodeError> {
    read_value(input)
}

fn read_value(input: &Node) -> Result<Opaque, NodeError> {
    input.as_type().map(Opaque)
}

fn inner(value: &Opaque) -> Result<Node, NodeError> {
    write_value(value)
}

fn f0(value: &Opaque) -> Result<Node, NodeError> {
    write_value(value)
}

fn map(value: &Opaque) -> Result<Node, NodeError> {
    write_value(value)
}

fn inner_map(value: &Opaque) -> Result<Node, NodeError> {
    write_value(value)
}

fn write_value(value: &Opaque) -> Result<Node, NodeError> {
    value.0.to_node()
}
