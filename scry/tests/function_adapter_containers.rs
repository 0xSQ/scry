//! Checks structural composition of adapters without changing ordinary container semantics.

use std::error::Error;
use std::io;
use std::net::{AddrParseError, IpAddr, Ipv4Addr, Ipv6Addr};

use scry::cli::setup::{ExposeMap, Setup};
use scry::convert::{read, write};
use scry::desc::{DescKind, FieldDesc};
use scry::node::Format;
use scry::{Config, Desc, Describe, FromNode, KeyPath, Node, NodeError, ToNode};

// ---------------------------------------------------------------------------------------------- //

#[derive(Debug, PartialEq, Config, ToNode)]
struct Addresses {
    #[scry(rename = "server.addresses", with(address_options))]
    addresses: Vec<Option<IpAddr>>,
    #[scry(with(optional_addresses))]
    fallback: Option<Vec<IpAddr>>,
}

#[derive(Debug, PartialEq, Config, ToNode)]
struct AddressPair(u8, #[scry(with(address_options))] Vec<Option<IpAddr>>);

#[derive(Debug, PartialEq, Config)]
struct WholePairs(
    #[scry(from_node_with(|node| read::vec(node, pair_text::from_node)), describe_with(|| Desc::list(pair_text::describe())))]
     Vec<Vec<u16>>,
);

#[derive(Debug, PartialEq, FromNode)]
struct AddressGroups {
    #[scry(rename = "address.groups", from_node_with(|node| read::vec(node, |node| read::option(node, |node| read::vec(node, address_text::from_node)))))]
    groups: Vec<Option<Vec<IpAddr>>>,
}

#[derive(ToNode)]
struct OutputGroups {
    #[scry(rename = "address.groups", to_node_with(|values| write::list(values, |value| write::option(value, |values| write::list(values, pair_representation::to_node)))))]
    groups: Vec<Option<Vec<IpAddr>>>,
}

#[derive(Debug, Config)]
struct MapAddresses {
    #[scry(from_node_with(|node| read::vec(node, |node| read::option(node, strict_map_address::from_node))), describe_with(|| Desc::list(strict_map_address::describe().nullable())))]
    addresses: Vec<Option<IpAddr>>,
}

#[derive(Debug, FromNode)]
struct PermissiveMapAddresses {
    #[scry(from_node_with(|node| read::vec(node, |node| read::option(node, permissive_map_address::from_node))))]
    addresses: Vec<Option<IpAddr>>,
}

#[derive(Debug, FromNode)]
struct RawValues {
    #[scry(from_node_with(|node| read::vec(node, |node| read::option(node, Node::as_type::<Node>))))]
    values: Vec<Option<Node>>,
}

#[derive(Config)]
struct AddressRepresentation {
    text: String,
    #[scry(default = 3)]
    retries: u8,
    #[scry(default = Transport::Tcp)]
    transport: Transport,
}

#[derive(Config)]
enum Transport {
    /// Uses a reliable connection.
    Tcp,
    /// Uses connectionless packets.
    Udp,
}

#[derive(FromNode)]
#[scry(allow_unknown_keys)]
struct PermissiveRepresentation {
    text: String,
}

// ---------------------------------------------------------------------------------------------- //

#[test]
fn foreign_values_round_trip_through_named_and_positional_containers() {
    let value: Addresses =
        parse(r#"#{ "server.addresses": ["127.0.0.1", (), "::1"], fallback: ["192.0.2.1"] }"#);
    assert_eq!(value.addresses, vec![Some(local_v4()), None, Some(local_v6())]);
    assert_eq!(value.fallback, Some(vec!["192.0.2.1".parse().unwrap()]));
    let output = value.to_node().unwrap();
    assert_eq!(output.as_type::<Addresses>().unwrap(), value);
    assert_eq!(output.as_map().unwrap().len(), 2);
    output.ensure_no_unknown_keys().unwrap();

    let value: AddressPair = parse(r#"[7, ["127.0.0.1", (), "::1"]]"#);
    assert_eq!(value, AddressPair(7, vec![Some(local_v4()), None, Some(local_v6())]));
    assert_eq!(value.to_node().unwrap().as_type::<AddressPair>().unwrap(), value);

    let value: AddressGroups = parse(r#"#{ "address.groups": [(), ["127.0.0.1", "::1"], []] }"#);
    assert_eq!(value.groups, vec![None, Some(vec![local_v4(), local_v6()]), Some(vec![])]);
}

#[test]
fn each_element_adapter_receives_its_complete_inner_vector_target() {
    let value: WholePairs = parse(r#"["2,4", "6,8"]"#);
    assert_eq!(value, WholePairs(vec![vec![2, 4], vec![6, 8]]));
    let DescKind::List { item } = WholePairs::describe().kind else {
        panic!("expected list");
    };
    assert_eq!(item.type_label(), "integer pair text");
    assert!(matches!(item.kind, DescKind::Plain { .. }));

    // The target has an inner vector, but this adapter accepts one string for the complete vector.
    let error = node("[[2, 4]]").as_type::<WholePairs>().unwrap_err();
    assert_eq!(error.path(), Some(&KeyPath::from_index(0)));
    assert!(
        matches!(error, NodeError::TypeMismatch { target_type, .. } if target_type == "string")
    );
}

#[test]
fn null_skips_inner_adapters_is_consumed_and_is_retained_in_output() {
    // The address reader rejects null. Successful decoding therefore also checks that it was bypassed.
    let source = node(r#"#{ "server.addresses": [(), "127.0.0.1"], fallback: () }"#);
    let value = source.as_type::<Addresses>().unwrap();
    assert_eq!(value.addresses, vec![None, Some(local_v4())]);
    assert_eq!(value.fallback, None);
    source.ensure_no_unknown_keys().unwrap();
    assert_eq!(parse::<Addresses>(r#"#{ "server.addresses": [] }"#).fallback, None);

    let output = value.to_node().unwrap();
    assert_eq!(output.as_map().unwrap().len(), 2);
    assert_eq!(output.req::<Option<String>>(r#"["server.addresses"][0]"#).unwrap(), None);
    assert_eq!(output.req::<Option<Vec<String>>>("fallback").unwrap(), None);

    let root = node("()");
    assert_eq!(optional_addresses::from_node(&root).unwrap(), None);
    root.ensure_no_unknown_keys().unwrap();
    let output = optional_addresses::to_node(&None).unwrap();
    assert_eq!(output.as_type::<Option<Vec<String>>>().unwrap(), None);
}

#[test]
fn vectors_reject_wrong_shapes_at_the_container_that_owns_them() {
    for source in [
        r#"#{ scope: #{ "address.groups": 5 } }"#,
        r#"#{ scope: #{ "address.groups": #{} } }"#,
        r#"#{ scope: #{ "address.groups": () } }"#,
    ] {
        let error = input_error::<AddressGroups>(source);
        assert!(
            matches!(&error, NodeError::TypeMismatch { target_type, .. } if target_type == "array")
        );
        assert_eq!(error.path(), Some(&KeyPath::from_keys(["scope", "address.groups"])));
    }
    let error = input_error::<AddressGroups>(r#"#{ scope: #{ "address.groups": [(), 5] } }"#);
    assert!(
        matches!(&error, NodeError::TypeMismatch { target_type, .. } if target_type == "array")
    );
    assert_eq!(error.path(), Some(&KeyPath::from_keys(["scope", "address.groups"]).push_index(1)));
}

#[test]
fn nested_input_errors_keep_full_paths_and_original_causes() {
    let error = input_error::<AddressGroups>(
        r#"#{ scope: #{ "address.groups": [(), ["127.0.0.1", "invalid"]] } }"#,
    );
    let path = KeyPath::from_keys(["scope", "address.groups"]).push_index(1).push_index(1);
    assert_eq!(error.path(), Some(&path));
    assert!(find_source::<AddrParseError>(&error).is_some());

    let error =
        input_error::<AddressGroups>(r#"#{ scope: #{ "address.groups": [["unlocated"]] } }"#);
    let path = KeyPath::from_keys(["scope", "address.groups"]).push_index(0).push_index(0);
    assert_eq!(error.path(), Some(&path));
    assert_eq!(find_source::<io::Error>(&error).unwrap().to_string(), "domain cause");

    let error =
        input_error::<AddressGroups>(r#"#{ scope: #{ "address.groups": [["root-located"]] } }"#);
    assert_eq!(error.path(), Some(&KeyPath::new()));
    assert_eq!(find_source::<io::Error>(&error).unwrap().to_string(), "domain cause");
}

#[test]
fn nested_output_errors_prepend_indices_and_literal_fields_once() {
    let error = OutputGroups {
        groups: vec![
            None,
            Some(vec![local_v4(), IpAddr::V4(Ipv4Addr::UNSPECIFIED)]),
        ],
    }
    .to_node()
    .unwrap_err();
    let path = KeyPath::from_keys(["address.groups"]).push_index(1).push_index(1).push_index(1);
    assert_eq!(error.path(), Some(&path));
    assert_eq!(find_source::<io::Error>(&error).unwrap().to_string(), "unrepresentable address");

    let root = Some(vec![IpAddr::V4(Ipv4Addr::UNSPECIFIED)]);
    let error = write::option(&root, |values| write::list(values, pair_representation::to_node))
        .unwrap_err();
    assert_eq!(error.path(), Some(&KeyPath::from_index(0).push_index(1)));
    assert!(find_source::<io::Error>(&error).is_some());
}

#[test]
fn composition_keeps_structural_descriptions_child_defaults_and_cli_choices() {
    let desc = Addresses::describe();
    let addresses = field(&desc, "server.addresses");
    assert!(!addresses.optional);
    assert!(!addresses.value.nullable);
    let DescKind::List { item } = &addresses.value.kind else {
        panic!("expected list");
    };
    assert!(item.nullable);
    assert_eq!(item.type_label(), "IP address");
    let fallback = field(&desc, "fallback");
    assert!(fallback.optional);
    assert!(fallback.value.nullable);
    let DescKind::List { item } = &fallback.value.kind else {
        panic!("expected optional list");
    };
    assert!(!item.nullable);
    desc.validate_path(r#"["server.addresses"][2]"#).unwrap();
    assert!(desc.validate_path(r#"["server.addresses"][2].text"#).is_err());

    let desc = MapAddresses::describe();
    let DescKind::List { item } = &field(&desc, "addresses").value.kind else {
        panic!("expected list");
    };
    assert!(item.nullable);
    assert_eq!(field(item, "retries").default_display.as_deref(), Some("3"));
    assert!(field(item, "retries").optional);
    let choices = field(item, "transport").value.unit_enum_variants().unwrap();
    assert_eq!(
        choices.iter().map(|variant| variant.name.as_str()).collect::<Vec<_>>(),
        ["tcp", "udp"]
    );
    desc.validate_path("addresses[0].transport").unwrap();
    assert!(desc.validate_path("addresses[0].missing").is_err());

    let bundle = Setup::new("test")
        .expose(|expose: &mut ExposeMap| {
            expose.option("addresses[0].transport").long("transport");
        })
        .into_bundle(|_value: MapAddresses| {});
    let help = bundle.command().clone().render_long_help().to_string();
    assert!(help.contains("Possible values:"));
    assert!(help.contains("tcp"));
    assert!(help.contains("udp"));
    let error = bundle
        .command()
        .clone()
        .try_get_matches_from(["test", "--transport", "invalid"])
        .unwrap_err();
    assert!(error.to_string().contains("invalid value"));
}

#[test]
fn composed_map_adapters_keep_local_strictness_and_explicit_leaf_audits() {
    let source = node(r#"#{ addresses: [(), #{ text: "127.0.0.1" }] }"#);
    let value = source.as_type::<MapAddresses>().unwrap();
    assert_eq!(value.addresses, vec![None, Some(local_v4())]);
    source.ensure_no_unknown_keys().unwrap();

    let error = node(r#"#{ addresses: [#{ text: "127.0.0.1", extra: #{} }] }"#)
        .as_type::<MapAddresses>()
        .unwrap_err();
    assert_unknown_paths(
        error,
        vec![KeyPath::from_keys(["addresses"]).push_index(0).push_key("extra")],
    );

    let source = node(r#"#{ addresses: [#{ text: "127.0.0.1", extra: 5 }] }"#);
    let value = source.as_type::<PermissiveMapAddresses>().unwrap();
    assert_eq!(value.addresses, vec![Some(local_v4())]);
    assert_unknown_paths(
        source.ensure_no_unknown_keys().unwrap_err(),
        vec![KeyPath::from_keys(["addresses"]).push_index(0).push_key("extra")],
    );

    let error = node(r#"#{ addresses: [#{ text: "127.0.0.1", extra: 5 }], extra: #{} }"#)
        .as_type::<PermissiveMapAddresses>()
        .unwrap_err();
    assert_unknown_paths(error, vec![KeyPath::from_keys(["extra"])]);
}

#[test]
fn native_raw_subtrees_inside_adapted_containers_consume_their_leaves() {
    let source = node(r#"#{ values: [(), #{ nested: [1, #{ arbitrary: true }] }, []] }"#);
    let value = source.as_type::<RawValues>().unwrap();
    assert_eq!(value.values.len(), 3);
    assert!(value.values[0].is_none());
    assert!(value.values[1].as_ref().unwrap().req::<bool>("nested[1].arbitrary").unwrap());
    assert!(value.values[2].as_ref().unwrap().as_vec().unwrap().is_empty());
    source.ensure_no_unknown_keys().unwrap();
}

#[test]
fn a_failed_element_prevents_conversion_and_reads_of_later_elements() {
    let source = node(r#"["127.0.0.1", "invalid", "::1"]"#);
    let error = read::vec(&source, address_text::from_node).unwrap_err();
    assert_eq!(error.path(), Some(&KeyPath::from_index(1)));
    assert!(find_source::<AddrParseError>(&error).is_some());
    assert_unknown_paths(
        source.ensure_no_unknown_keys().unwrap_err(),
        vec![KeyPath::from_index(2)],
    );
}

// ---------------------------------------------------------------------------------------------- //

mod address_options {
    use super::*;

    pub fn from_node(node: &Node) -> Result<Vec<Option<IpAddr>>, NodeError> {
        read::vec(node, |node| read::option(node, address_text::from_node))
    }

    pub fn to_node(values: &[Option<IpAddr>]) -> Result<Node, NodeError> {
        write::list(values, |value| write::option(value, address_text::to_node))
    }

    pub fn describe() -> Desc {
        Desc::list(address_text::describe().nullable())
    }
}

mod optional_addresses {
    use super::*;

    pub fn from_node(node: &Node) -> Result<Option<Vec<IpAddr>>, NodeError> {
        read::option(node, |node| read::vec(node, address_text::from_node))
    }

    pub fn to_node(value: &Option<Vec<IpAddr>>) -> Result<Node, NodeError> {
        write::option(value, |values| write::list(values, address_text::to_node))
    }

    pub fn describe() -> Desc {
        Desc::list(address_text::describe()).nullable()
    }
}

mod address_text {
    use super::*;

    pub fn from_node(node: &Node) -> Result<IpAddr, NodeError> {
        let text: String = node.as_type()?;
        match text.as_str() {
            "unlocated" => {
                Err(NodeError::with_context("address rejected", io::Error::other("domain cause")))
            }
            "root-located" => Err(NodeError::invalid_value_with_source(
                &KeyPath::new(),
                "address rejected",
                io::Error::other("domain cause"),
            )),
            _ => parse_address(node, &text),
        }
    }

    pub fn to_node(value: &IpAddr) -> Result<Node, NodeError> {
        value.to_string().to_node()
    }

    pub fn describe() -> Desc {
        Desc::plain("IP address")
    }
}

mod pair_representation {
    use super::*;

    pub fn to_node(value: &IpAddr) -> Result<Node, NodeError> {
        if *value == IpAddr::V4(Ipv4Addr::UNSPECIFIED) {
            return Err(NodeError::invalid_value_with_source(
                &KeyPath::from_index(1),
                "address rejected",
                io::Error::other("unrepresentable address"),
            ));
        }
        (4u8, value.to_string()).to_node()
    }
}

mod pair_text {
    use super::*;

    pub fn from_node(node: &Node) -> Result<Vec<u16>, NodeError> {
        let text: String = node.as_type()?;
        let (first, second) = text.split_once(',').ok_or_else(|| {
            NodeError::invalid_value(&node.path, "expected two comma-separated integers")
        })?;
        [first, second]
            .into_iter()
            .map(|part| {
                part.parse().map_err(|error| {
                    NodeError::invalid_value_with_source(&node.path, "invalid integer", error)
                })
            })
            .collect()
    }

    pub fn describe() -> Desc {
        Desc::plain("integer pair text")
    }
}

mod strict_map_address {
    use super::*;

    pub fn from_node(node: &Node) -> Result<IpAddr, NodeError> {
        let representation: AddressRepresentation = node.as_type()?;
        let _ = (representation.retries, representation.transport);
        parse_address(node.req_node("text")?, &representation.text)
    }

    pub fn describe() -> Desc {
        AddressRepresentation::describe()
    }
}

mod permissive_map_address {
    use super::*;

    pub fn from_node(node: &Node) -> Result<IpAddr, NodeError> {
        let representation: PermissiveRepresentation = node.as_type()?;
        parse_address(node.req_node("text")?, &representation.text)
    }
}

fn parse_address(node: &Node, text: &str) -> Result<IpAddr, NodeError> {
    text.parse().map_err(|error| {
        NodeError::invalid_value_with_source(&node.path, "invalid IP address", error)
    })
}

fn local_v4() -> IpAddr {
    IpAddr::V4(Ipv4Addr::LOCALHOST)
}

fn local_v6() -> IpAddr {
    IpAddr::V6(Ipv6Addr::LOCALHOST)
}

fn field<'a>(desc: &'a Desc, name: &str) -> &'a FieldDesc {
    let DescKind::Struct { fields } = &desc.kind else {
        panic!("expected struct description");
    };
    fields.iter().find(|field| field.name == name).unwrap()
}

fn input_error<T: FromNode>(source: &str) -> NodeError {
    node(source).req_node("scope").unwrap().as_type::<T>().err().unwrap()
}

fn assert_unknown_paths(error: NodeError, expected: Vec<KeyPath>) {
    assert!(matches!(error, NodeError::UnknownKeys { paths } if paths == expected));
}

fn find_source<'a, E: Error + 'static>(error: &'a (dyn Error + 'static)) -> Option<&'a E> {
    let mut current = Some(error);
    while let Some(error) = current {
        if let Some(source) = error.downcast_ref::<E>() {
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
