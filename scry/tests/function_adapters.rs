//! Checks complete-value adaptation through application-owned adapter modules.

use std::error::Error;
use std::io;
use std::net::{AddrParseError, IpAddr, Ipv4Addr};

use scry::desc::{DescKind, FieldDesc, VariantRepr};
use scry::node::Format;
use scry::{Config, Desc, Describe, FromDefaults, FromNode, KeyPath, Node, NodeError, ToNode};

// ---------------------------------------------------------------------------------------------- //

#[derive(Debug, PartialEq, Config, ToNode)]
struct AddressConfig {
    /// The listening address.
    #[scry(rename = "server.address", with(address_text))]
    address: IpAddr,
}

#[derive(Debug, PartialEq, FromNode, ToNode, Describe)]
struct Transparent(#[scry(with(address_text))] IpAddr);

#[derive(Debug, PartialEq, FromNode, ToNode, Describe)]
struct Pair(u8, #[scry(with(address_text))] IpAddr);

#[derive(Debug, PartialEq, FromNode, ToNode, Describe)]
enum AddressChoice {
    #[scry(rename = "single.key")]
    Single(#[scry(with(address_text))] IpAddr),
    #[scry(rename = "pair.key")]
    Pair(u8, #[scry(with(address_text))] IpAddr),
    #[scry(rename = "named.key")]
    Named {
        #[scry(rename = "address.key", with(address_text))]
        address: IpAddr,
    },
}

#[derive(Config)]
struct ReadConfig {
    #[scry(with(read_address))]
    address: IpAddr,
}

#[derive(ToNode)]
struct WriteOnly(#[scry(with(write_address))] IpAddr);

#[derive(Describe)]
#[allow(dead_code)]
struct DescribeOnly(#[scry(with(describe_address))] IpAddr);

#[derive(Debug, PartialEq, Config, ToNode)]
struct OptionalAddress {
    #[scry(with(whole_optional))]
    address: Option<IpAddr>,
}

#[derive(Debug, PartialEq, Config, ToNode)]
struct ExplicitOptionalAddress {
    #[scry(default = Some(IpAddr::V4(Ipv4Addr::UNSPECIFIED)), with(whole_optional))]
    address: Option<IpAddr>,
}

type MaybeAddress = Option<IpAddr>;

#[derive(Debug, PartialEq, FromNode)]
struct AliasOptional {
    #[scry(default = None, with(whole_optional))]
    address: MaybeAddress,
}

#[derive(Debug, PartialEq, Config)]
struct AdaptedDefaults {
    #[scry(default = true, with(switch_word))]
    toggle: bool,
    #[scry(from_defaults, with(mode_policy))]
    mode: TargetMode,
}

#[derive(Debug, PartialEq, FromDefaults)]
enum TargetMode {
    #[scry(default)]
    Idle,
    Nested {
        count: u8,
    },
}

#[derive(Config)]
enum ModeRepresentation {
    #[scry(default)]
    Automatic,
    Nested {
        #[scry(default = 7)]
        count: u8,
    },
}

#[derive(Describe)]
#[allow(dead_code)]
struct UnevaluatedDefault {
    #[scry(default = panic!("description evaluated a fallback"), with(integer_label))]
    value: u16,
}

/// An address envelope.
#[derive(Describe)]
#[allow(dead_code)]
struct DocumentedTransparent(
    /// A positional address.
    #[scry(with(address_text))]
    IpAddr,
);

#[derive(Describe)]
#[allow(dead_code)]
struct FieldDocumentedTransparent(
    /// A positional address.
    #[scry(with(address_text))]
    IpAddr,
);

#[derive(Describe)]
#[allow(dead_code)]
enum DocumentedChoice {
    /// One listening address.
    Single(
        /// The variant's address.
        #[scry(with(address_text))]
        IpAddr,
    ),
}

#[derive(ToNode)]
struct RelativeOutput {
    #[scry(rename = "address.key", with(address_pair))]
    address: IpAddr,
}

#[derive(ToNode)]
struct RelativeTuple(u8, #[scry(with(address_pair))] IpAddr);

#[derive(ToNode)]
enum RelativeChoice {
    #[scry(rename = "single.key")]
    Single(#[scry(with(address_pair))] IpAddr),
    #[scry(rename = "pair.key")]
    Pair(u8, #[scry(with(address_pair))] IpAddr),
    #[scry(rename = "named.key")]
    Named {
        #[scry(rename = "address.key", with(address_pair))]
        address: IpAddr,
    },
}

#[derive(ToNode)]
struct NestedOutput {
    #[scry(rename = "items.key")]
    items: Vec<RelativeChoice>,
}

#[derive(Debug, FromNode)]
struct StrictMapAddress {
    #[scry(with(strict_address_map))]
    address: IpAddr,
}

#[derive(Debug, FromNode)]
struct PermissiveMapAddress {
    #[scry(with(permissive_address_map))]
    address: IpAddr,
}

#[derive(FromNode)]
struct StrictRepresentation {
    text: String,
}

#[derive(FromNode)]
#[scry(allow_unknown_keys)]
struct PermissiveRepresentation {
    text: String,
}

// ---------------------------------------------------------------------------------------------- //

#[test]
fn a_local_module_adapts_a_foreign_target_without_changing_the_stored_type() {
    let value: AddressConfig = parse(r#"#{ "server.address": "127.0.0.1" }"#);
    assert_eq!(value.address, IpAddr::V4(Ipv4Addr::LOCALHOST));
    let output = value.to_node().unwrap();
    assert_eq!(output.as_map().unwrap().len(), 1);
    assert_eq!(output.req::<String>(KeyPath::from_keys(["server.address"])).unwrap(), "127.0.0.1");
    assert_eq!(output.as_type::<AddressConfig>().unwrap(), value);

    let desc = AddressConfig::describe();
    let address = field(&desc, "server.address");
    assert_eq!(address.doc, "The listening address.");
    assert_eq!(address.value.doc, "A textual IP address.");
    assert_eq!(address.value.type_label(), "IP address");
    desc.validate_path(r#"["server.address"]"#).unwrap();
    assert!(desc.validate_path("server.address").is_err());
}

#[test]
fn modules_preserve_transparent_tuple_and_enum_payload_shapes() {
    let address = IpAddr::V4(Ipv4Addr::LOCALHOST);
    let transparent: Transparent = parse(r#""127.0.0.1""#);
    assert_eq!(transparent, Transparent(address));
    assert_eq!(transparent.to_node().unwrap().as_type::<String>().unwrap(), "127.0.0.1");

    let pair: Pair = parse(r#"[3, "127.0.0.1"]"#);
    assert_eq!(pair, Pair(3, address));
    assert_eq!(pair.to_node().unwrap().as_type::<Pair>().unwrap(), pair);
    for source in [r#"[3]"#, r#"[3, "127.0.0.1", 4]"#] {
        assert!(matches!(
            node(source).as_type::<Pair>(),
            Err(NodeError::ArrayLength { expected: 2, .. })
        ));
    }

    let cases = [
        (r#"#{ "single.key": "127.0.0.1" }"#, AddressChoice::Single(address)),
        (r#"#{ "pair.key": [3, "127.0.0.1"] }"#, AddressChoice::Pair(3, address)),
        (r#"#{ "named.key": #{ "address.key": "127.0.0.1" } }"#, AddressChoice::Named { address }),
    ];
    for (source, expected) in cases {
        let value: AddressChoice = parse(source);
        assert_eq!(value, expected);
        assert_eq!(value.to_node().unwrap().as_type::<AddressChoice>().unwrap(), value);
    }
    AddressChoice::describe().validate_path(r#"["named.key"]["address.key"]"#).unwrap();
    AddressChoice::describe().validate_path(r#"["pair.key"][1]"#).unwrap();
}

#[test]
fn requested_capabilities_do_not_require_other_adapter_or_native_capabilities() {
    let value: ReadConfig = parse(r#"#{ address: "127.0.0.1" }"#);
    assert_eq!(value.address, IpAddr::V4(Ipv4Addr::LOCALHOST));
    assert_eq!(field(&ReadConfig::describe(), "address").value.type_label(), "IP address");

    let output = WriteOnly(value.address).to_node().unwrap();
    assert_eq!(output.as_type::<String>().unwrap(), "127.0.0.1");
    assert_eq!(DescribeOnly::describe().type_label(), "IP address");
}

#[test]
fn optional_adapters_receive_present_null_and_missing_keys_use_target_fallbacks() {
    assert_eq!(parse::<OptionalAddress>("#{}").address, None);
    assert_eq!(
        parse::<OptionalAddress>("#{ address: () }").address,
        Some(IpAddr::V4(Ipv4Addr::LOCALHOST))
    );
    assert_eq!(
        parse::<ExplicitOptionalAddress>("#{}").address,
        Some(IpAddr::V4(Ipv4Addr::UNSPECIFIED)),
    );
    assert_eq!(
        parse::<ExplicitOptionalAddress>("#{ address: () }").address,
        Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
    );
    assert_eq!(parse::<AliasOptional>("#{}").address, None);
    assert_eq!(
        parse::<AliasOptional>("#{ address: () }").address,
        Some(IpAddr::V4(Ipv4Addr::LOCALHOST))
    );
    assert!(node("#{ address: 5 }").as_type::<ExplicitOptionalAddress>().is_err());

    let output = OptionalAddress { address: None }.to_node().unwrap();
    assert_eq!(output.as_map().unwrap().len(), 1);
    assert_eq!(output.req::<Option<String>>("address").unwrap(), None);
    let root = whole_optional::to_node(&None).unwrap();
    assert_eq!(root.as_type::<Option<String>>().unwrap(), None);

    let desc = OptionalAddress::describe();
    let address = field(&desc, "address");
    assert!(address.optional);
    assert!(address.value.nullable);
    assert_eq!(address.value.type_label(), "IP address | null");
}

#[test]
fn target_defaults_are_missing_only_and_description_does_not_evaluate_them() {
    let fallback = scry::from_defaults::<AdaptedDefaults>().unwrap();
    assert_eq!(
        fallback,
        AdaptedDefaults {
            toggle: true,
            mode: TargetMode::Idle
        }
    );
    let supplied: AdaptedDefaults = parse(r#"#{ toggle: "off", mode: #{ nested: #{} } }"#);
    assert_eq!(
        supplied,
        AdaptedDefaults {
            toggle: false,
            mode: TargetMode::Nested { count: 7 }
        }
    );
    assert!(node("#{ toggle: true }").as_type::<AdaptedDefaults>().is_err());

    let desc = UnevaluatedDefault::describe();
    let value = field(&desc, "value");
    assert!(value.optional);
    assert_eq!(value.default_display, None);
    assert_eq!(value.value.type_label(), "level");
}

#[test]
fn adapted_defaults_hide_target_literals_and_direct_enum_markers_but_keep_nested_defaults() {
    let desc = AdaptedDefaults::describe();
    let toggle = field(&desc, "toggle");
    assert!(toggle.optional);
    assert_eq!(toggle.default_display, None);
    let mode = field(&desc, "mode");
    assert!(mode.optional);
    let DescKind::Enum { variants } = &mode.value.kind else {
        panic!("expected adapted enum description");
    };
    assert!(variants.iter().all(|variant| !variant.is_default()));
    assert!(ModeRepresentation::describe().unit_enum_variants().is_none());
    let DescKind::Enum { variants: native } = ModeRepresentation::describe().kind else {
        panic!("expected representation enum");
    };
    assert!(native[0].is_default());
    let VariantRepr::Payload { payload, .. } = &variants[1].repr else {
        panic!("expected nested representation payload");
    };
    assert_eq!(field(payload, "count").default_display.as_deref(), Some("7"));
}

#[test]
fn adapted_descriptions_keep_existing_documentation_precedence() {
    assert_eq!(Transparent::describe().doc, "A textual IP address.");
    assert_eq!(DocumentedTransparent::describe().doc, "An address envelope.");
    assert_eq!(FieldDocumentedTransparent::describe().doc, "A positional address.");
    let DescKind::Enum { variants } = DocumentedChoice::describe().kind else {
        panic!("expected enum");
    };
    assert_eq!(variants[0].doc, "One listening address.");
    let VariantRepr::Payload { payload, .. } = &variants[0].repr else {
        panic!("expected adapted payload");
    };
    assert_eq!(payload.doc, "The variant's address.");
}

#[test]
fn adapter_input_errors_keep_absolute_locations_and_original_causes() {
    let cases = [
        (
            r#"#{ scope: #{ "server.address": "invalid" } }"#,
            input_error::<AddressConfig> as fn(&str) -> NodeError,
            KeyPath::from_keys(["scope", "server.address"]),
        ),
        (
            r#"#{ scope: [3, "invalid"] }"#,
            input_error::<Pair>,
            KeyPath::from_keys(["scope"]).push_index(1),
        ),
        (
            r#"#{ scope: #{ "pair.key": [3, "invalid"] } }"#,
            input_error::<AddressChoice>,
            KeyPath::from_keys(["scope", "pair.key"]).push_index(1),
        ),
        (
            r#"#{ scope: #{ "named.key": #{ "address.key": "invalid" } } }"#,
            input_error::<AddressChoice>,
            KeyPath::from_keys(["scope", "named.key", "address.key"]),
        ),
    ];
    for (source, parse_error, expected) in cases {
        let error = parse_error(source);
        assert_eq!(error.path(), Some(&expected));
        assert!(find_source::<AddrParseError>(&error).is_some());
    }

    let unlocated = input_error::<Transparent>(r#"#{ scope: "unlocated" }"#);
    assert_eq!(unlocated.path(), Some(&KeyPath::from_keys(["scope"])));
    assert!(find_source::<io::Error>(&unlocated).is_some());
    let root = input_error::<Transparent>(r#"#{ scope: "root-located" }"#);
    assert_eq!(root.path(), Some(&KeyPath::new()));
    assert!(find_source::<io::Error>(&root).is_some());
}

#[test]
fn adapter_output_errors_compose_actual_representation_paths_once() {
    let address = IpAddr::V4(Ipv4Addr::UNSPECIFIED);
    let cases = [
        (
            RelativeOutput { address }.to_node().unwrap_err(),
            KeyPath::from_keys(["address.key"]).push_index(1),
        ),
        (RelativeTuple(3, address).to_node().unwrap_err(), KeyPath::from_index(1).push_index(1)),
        (
            RelativeChoice::Single(address).to_node().unwrap_err(),
            KeyPath::from_keys(["single.key"]).push_index(1),
        ),
        (
            RelativeChoice::Pair(3, address).to_node().unwrap_err(),
            KeyPath::from_keys(["pair.key"]).push_index(1).push_index(1),
        ),
        (
            RelativeChoice::Named { address }.to_node().unwrap_err(),
            KeyPath::from_keys(["named.key", "address.key"]).push_index(1),
        ),
        (
            NestedOutput {
                items: vec![RelativeChoice::Pair(3, address)],
            }
            .to_node()
            .unwrap_err(),
            KeyPath::from_keys(["items.key"])
                .push_index(0)
                .push_key("pair.key")
                .push_index(1)
                .push_index(1),
        ),
    ];
    for (error, expected) in cases {
        assert_eq!(error.path(), Some(&expected));
        assert!(find_source::<io::Error>(&error).is_some());
    }
}

#[test]
fn parent_and_adapter_validate_their_own_map_shapes_and_leaf_audits_remain_explicit() {
    let source = node(r#"#{ address: #{ text: "127.0.0.1" } }"#);
    let parsed = source.as_type::<StrictMapAddress>().unwrap();
    assert_eq!(parsed.address, IpAddr::V4(Ipv4Addr::LOCALHOST));
    source.ensure_no_unknown_keys().unwrap();

    let child_error = node(r#"#{ address: #{ text: "127.0.0.1", extra: #{} } }"#)
        .as_type::<StrictMapAddress>()
        .unwrap_err();
    assert!(matches!(child_error, NodeError::UnknownKeys { paths }
        if paths == [KeyPath::from_keys(["address", "extra"])]));

    let source = node(r#"#{ address: #{ text: "127.0.0.1", extra: 5 } }"#);
    let parsed = source.as_type::<PermissiveMapAddress>().unwrap();
    assert_eq!(parsed.address, IpAddr::V4(Ipv4Addr::LOCALHOST));
    assert!(matches!(source.ensure_no_unknown_keys(), Err(NodeError::UnknownKeys { paths })
        if paths == [KeyPath::from_keys(["address", "extra"])]));

    let parent_error = node(r#"#{ address: #{ text: "127.0.0.1", extra: 5 }, extra: #{} }"#)
        .as_type::<PermissiveMapAddress>()
        .unwrap_err();
    assert!(matches!(parent_error, NodeError::UnknownKeys { paths }
        if paths == [KeyPath::from_keys(["extra"])]));
}

// ---------------------------------------------------------------------------------------------- //

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
            _ => text.parse().map_err(|error| {
                NodeError::invalid_value_with_source(&node.path, "invalid IP address", error)
            }),
        }
    }

    pub fn to_node(value: &IpAddr) -> Result<Node, NodeError> {
        value.to_string().to_node()
    }

    pub fn describe() -> Desc {
        Desc::plain("IP address").with_doc("A textual IP address.")
    }
}

mod read_address {
    use super::*;

    pub fn from_node(node: &Node) -> Result<IpAddr, NodeError> {
        address_text::from_node(node)
    }

    pub fn describe() -> Desc {
        address_text::describe()
    }
}

mod write_address {
    use super::*;

    pub fn to_node(value: &IpAddr) -> Result<Node, NodeError> {
        address_text::to_node(value)
    }
}

mod describe_address {
    use super::*;

    pub fn describe() -> Desc {
        address_text::describe()
    }
}

mod whole_optional {
    use super::*;

    pub fn from_node(node: &Node) -> Result<Option<IpAddr>, NodeError> {
        if node.as_type::<Option<String>>()?.is_none() {
            Ok(Some(IpAddr::V4(Ipv4Addr::LOCALHOST)))
        } else {
            address_text::from_node(node).map(Some)
        }
    }

    pub fn to_node(value: &Option<IpAddr>) -> Result<Node, NodeError> {
        value.map(|value| value.to_string()).to_node()
    }

    pub fn describe() -> Desc {
        address_text::describe().nullable()
    }
}

mod switch_word {
    use super::*;

    pub fn from_node(node: &Node) -> Result<bool, NodeError> {
        match node.as_type::<String>()?.as_str() {
            "on" => Ok(true),
            "off" => Ok(false),
            _ => Err(NodeError::invalid_value(&node.path, "expected on or off")),
        }
    }

    pub fn describe() -> Desc {
        Desc::plain("on or off")
    }
}

mod mode_policy {
    use super::*;

    pub fn from_node(node: &Node) -> Result<TargetMode, NodeError> {
        match node.as_type::<ModeRepresentation>()? {
            ModeRepresentation::Automatic => Ok(TargetMode::Idle),
            ModeRepresentation::Nested { count } => Ok(TargetMode::Nested { count }),
        }
    }

    pub fn describe() -> Desc {
        ModeRepresentation::describe()
    }
}

mod integer_label {
    use super::*;

    pub fn describe() -> Desc {
        Desc::plain("level")
    }
}

mod address_pair {
    use super::*;

    pub fn to_node(value: &IpAddr) -> Result<Node, NodeError> {
        if *value == IpAddr::V4(Ipv4Addr::UNSPECIFIED) {
            Err(NodeError::invalid_value_with_source(
                &KeyPath::from_index(1),
                "address rejected",
                io::Error::other("domain cause"),
            ))
        } else {
            (4u8, value.to_string()).to_node()
        }
    }
}

mod strict_address_map {
    use super::*;

    pub fn from_node(node: &Node) -> Result<IpAddr, NodeError> {
        let representation: StrictRepresentation = node.as_type()?;
        representation.text.parse().map_err(|error| {
            NodeError::invalid_value_with_source(
                &node.path.clone().push_key("text"),
                "invalid IP address",
                error,
            )
        })
    }
}

mod permissive_address_map {
    use super::*;

    pub fn from_node(node: &Node) -> Result<IpAddr, NodeError> {
        let representation: PermissiveRepresentation = node.as_type()?;
        representation.text.parse().map_err(|error| {
            NodeError::invalid_value_with_source(
                &node.path.clone().push_key("text"),
                "invalid IP address",
                error,
            )
        })
    }
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
