//! Integration tests for derived recursive Scry defaults.
#![cfg(feature = "format-json")]

use std::path::PathBuf;

use scry::desc::DescKind;
use scry::node::Format;
use scry::{Config, Describe, FromDefaults, FromNode, KeyPath, Node, NodeError};

// ---------------------------------------------------------------------------------------------- //

#[derive(Debug, PartialEq, Config)]
struct ChildConfig {
    #[scry(default = 3)]
    retries: u32,
    #[scry(default = false)]
    enabled: bool,
}

impl Default for ChildConfig {
    fn default() -> Self {
        Self {
            retries: 99,
            enabled: true,
        }
    }
}

#[derive(Debug, PartialEq, Config)]
struct ParentConfig {
    #[scry(from_defaults)]
    child: ChildConfig,
}

#[derive(Debug, Config)]
#[allow(dead_code)]
struct RequiredConfig {
    host: String,
}

#[derive(Debug, Config)]
#[allow(dead_code)]
struct MiddleConfig {
    #[scry(rename = "inner", from_defaults)]
    required: RequiredConfig,
}

#[derive(Debug, Config)]
#[allow(dead_code)]
struct RootConfig {
    #[scry(rename = "outer", from_defaults)]
    middle: MiddleConfig,
}

#[derive(Debug, PartialEq)]
struct Endpoint(u16);

impl FromDefaults for Endpoint {
    fn from_defaults_at(_path: &KeyPath) -> Result<Self, NodeError> {
        Ok(Self(8080))
    }
}

fn parse_endpoint(node: &Node) -> Result<Endpoint, NodeError> {
    Ok(Endpoint(node.as_type()?))
}

#[derive(Debug, PartialEq, FromNode)]
struct EndpointConfig {
    #[scry(from_node_with(parse_endpoint), from_defaults)]
    endpoint: Endpoint,
}

#[derive(Debug, PartialEq, FromNode)]
enum Command {
    Run {
        #[scry(from_defaults)]
        child: ChildConfig,
    },
}

#[derive(Debug, PartialEq, FromNode, FromDefaults)]
struct ComponentDerives {
    #[scry(default = 17)]
    value: u32,
}

#[derive(Debug, PartialEq, Default, Config)]
enum OutputMode {
    #[scry(default)]
    Summary,
    #[default]
    Full,
}

#[derive(Debug, PartialEq, Config)]
struct EnumDefaultConfig {
    #[scry(from_defaults)]
    mode: OutputMode,
}

#[derive(Debug, PartialEq, FromDefaults)]
#[allow(dead_code)]
enum StandaloneEnumDefault {
    Other,
    #[scry(default)]
    Selected,
}

#[derive(Describe)]
#[allow(dead_code)]
struct DescriptionDefaults {
    #[scry(default = true)]
    literal_bool: bool,
    #[scry(default = (-3))]
    literal_number: i32,
    #[scry(default = "cache")]
    literal_string: &'static str,
    #[scry(default = Vec::new())]
    constructed_list: Vec<String>,
    #[scry(default = PathBuf::from("cache"))]
    constructed_path: PathBuf,
    #[scry(default = OutputMode::Full)]
    enum_path: OutputMode,
    #[scry(default = DEFAULT_LIMIT)]
    constant: usize,
    #[scry(from_defaults)]
    nested: ChildConfig,
}

#[derive(Describe)]
#[allow(dead_code)]
struct EnumDescriptionContexts {
    #[scry(from_defaults)]
    from_defaults: OutputMode,
    required: OutputMode,
    #[scry(default = OutputMode::Full)]
    explicit: OutputMode,
}

#[derive(Default, Describe)]
#[allow(dead_code)]
enum RustDefaultEnum {
    #[default]
    First,
    Second,
}

fn parse<T: FromNode>(source: &str) -> T {
    Node::parse_str(source, Format::Json).unwrap().as_type().unwrap()
}

// ---------------------------------------------------------------------------------------------- //

#[test]
fn absent_null_and_empty_map_use_the_same_recursive_defaults() {
    let expected = ParentConfig {
        child: ChildConfig {
            retries: 3,
            enabled: false,
        },
    };

    assert_eq!(parse::<ParentConfig>("{}"), expected);
    assert_eq!(parse::<ParentConfig>(r#"{"child": null}"#), expected);
    assert_eq!(parse::<ParentConfig>(r#"{"child": {}}"#), expected);
}

#[test]
fn present_child_values_override_individual_defaults() {
    let config: ParentConfig = parse(r#"{"child": {"retries": 9}}"#);

    assert_eq!(
        config,
        ParentConfig {
            child: ChildConfig {
                retries: 9,
                enabled: false,
            },
        }
    );
}

#[test]
fn recursive_scry_defaults_do_not_consult_rust_default() {
    let config: ParentConfig = parse("{}");

    assert_eq!(
        config.child,
        ChildConfig {
            retries: 3,
            enabled: false,
        }
    );
    assert_ne!(config.child, ChildConfig::default());
}

#[test]
fn nested_and_renamed_fields_preserve_the_full_error_path() {
    let error = scry::from_defaults::<RootConfig>().unwrap_err();

    assert_eq!(error.to_string(), "missing value for 'outer.inner.host'");
}

#[test]
fn custom_present_parser_and_recursive_default_are_independent() {
    let absent: EndpointConfig = parse("{}");
    let present: EndpointConfig = parse(r#"{"endpoint": 9000}"#);

    assert_eq!(absent.endpoint, Endpoint(8080));
    assert_eq!(present.endpoint, Endpoint(9000));
}

#[test]
fn named_enum_variant_fields_support_recursive_defaults() {
    let command: Command = parse(r#"{"run": {}}"#);

    assert_eq!(
        command,
        Command::Run {
            child: ChildConfig {
                retries: 3,
                enabled: false,
            },
        }
    );
}

#[test]
fn standalone_component_derives_support_the_root_helper() {
    assert_eq!(scry::from_defaults::<ComponentDerives>().unwrap(), ComponentDerives { value: 17 });
}

#[test]
fn config_enums_use_the_scry_default_instead_of_rust_default() {
    assert_eq!(scry::from_defaults::<OutputMode>().unwrap(), OutputMode::Summary);
    assert_eq!(scry::from_defaults::<EnumDefaultConfig>().unwrap().mode, OutputMode::Summary);
    assert_eq!(OutputMode::default(), OutputMode::Full);

    let explicit: EnumDefaultConfig = parse(r#"{"mode": "full"}"#);
    assert_eq!(explicit.mode, OutputMode::Full);
}

#[test]
fn standalone_enum_derive_uses_its_marked_unit_variant() {
    assert_eq!(
        scry::from_defaults::<StandaloneEnumDefault>().unwrap(),
        StandaloneEnumDefault::Selected
    );
}

#[test]
fn descriptions_display_literals_but_not_rust_construction_syntax() {
    let desc = DescriptionDefaults::describe();
    let DescKind::Struct { fields } = desc.kind else {
        panic!("expected a struct description");
    };

    let field = |name: &str| fields.iter().find(|field| field.name == name).unwrap();

    assert_eq!(field("literal_bool").default_display.as_deref(), Some("true"));
    assert_eq!(field("literal_number").default_display.as_deref(), Some("-3"));
    assert_eq!(field("literal_string").default_display.as_deref(), Some("\"cache\""));
    assert_eq!(field("constructed_list").default_display, None);
    assert_eq!(field("constructed_path").default_display, None);
    assert_eq!(field("enum_path").default_display, None);
    assert_eq!(field("constant").default_display, None);
    assert_eq!(field("nested").default_display, None);

    assert!(fields.iter().all(|field| field.optional));
}

#[test]
fn rust_default_variant_is_not_scry_description_metadata() {
    let desc = RustDefaultEnum::describe();
    let DescKind::Enum { variants } = desc.kind else {
        panic!("expected an enum description");
    };

    assert!(variants.iter().all(|variant| !variant.is_default()));
}

#[test]
fn enum_default_marker_is_contextual_to_from_defaults_fields() {
    let root = OutputMode::describe();
    assert_eq!(default_variant_name(&root).as_deref(), Some("summary"));

    let desc = EnumDescriptionContexts::describe();
    let DescKind::Struct { fields } = desc.kind else {
        panic!("expected a struct description");
    };
    let field = |name: &str| fields.iter().find(|field| field.name == name).unwrap();

    assert_eq!(default_variant_name(&field("from_defaults").value).as_deref(), Some("summary"));
    assert_eq!(default_variant_name(&field("required").value), None);
    assert_eq!(default_variant_name(&field("explicit").value), None);

    let render_field = |name: &str| scry::Desc::structure(vec![field(name).clone()]).display();
    let from_defaults = render_field("from_defaults");
    let required = render_field("required");
    let explicit = render_field("explicit");

    assert!(from_defaults.contains("» summary"));
    assert!(required.contains("› summary"));
    assert!(!required.contains("» summary"));
    assert!(explicit.contains("› summary"));
    assert!(!explicit.contains("» summary"));
}

fn default_variant_name(desc: &scry::Desc) -> Option<String> {
    desc.unit_enum_variants()?
        .iter()
        .find(|variant| variant.is_default())
        .map(|variant| variant.name.clone())
}
