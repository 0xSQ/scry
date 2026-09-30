//! Covers missing-only defaults across decoding, serialization, CLI edits, and formats.

use std::sync::atomic::{AtomicUsize, Ordering};

use clap::{ArgMatches, Command};
use scry::cli::setup::{ExposeMap, OverrideArgs, QueryArgs};
use scry::node::{Format, Value};
use scry::{Config, Describe, FromNode, Node, NodeError, ToNode};

// ---------------------------------------------------------------------------------------------- //

#[derive(Debug, PartialEq, Config, ToNode)]
struct Timeout {
    #[scry(default = Some(30))]
    timeout: Option<u64>,
}

#[derive(Debug, PartialEq, Config, ToNode)]
enum TimeoutMode {
    Named {
        #[scry(default = Some(30))]
        timeout: Option<u64>,
    },
}

#[test]
fn optional_defaults_round_trip_none_and_some_in_structs_and_named_variants() {
    assert_eq!(decode::<Timeout>("#{}").timeout, Some(30));
    assert_eq!(decode::<Timeout>("#{ timeout: () }").timeout, None);
    assert_eq!(decode::<TimeoutMode>("#{ named: #{} }"), TimeoutMode::Named { timeout: Some(30) });
    assert_eq!(
        decode::<TimeoutMode>("#{ named: #{ timeout: () } }"),
        TimeoutMode::Named { timeout: None },
    );

    for timeout in [None, Some(10), Some(30)] {
        let value = Timeout { timeout };
        let node = value.to_node().unwrap();
        assert_eq!(node.req::<Option<u64>>("timeout").unwrap(), timeout);
        assert_eq!(node.as_type::<Timeout>().unwrap(), value);

        let value = TimeoutMode::Named { timeout };
        let node = value.to_node().unwrap();
        assert_eq!(node.req::<Option<u64>>("named.timeout").unwrap(), timeout);
        assert_eq!(node.as_type::<TimeoutMode>().unwrap(), value);
    }
}

#[test]
fn defaults_are_lazy_and_only_missing_input_evaluates_them() {
    #[derive(Debug, Config)]
    struct Counted {
        #[scry(default = fallback())]
        timeout: Option<u64>,
    }

    static CALLS: AtomicUsize = AtomicUsize::new(0);

    fn fallback() -> Option<u64> {
        CALLS.fetch_add(1, Ordering::SeqCst);
        Some(30)
    }

    assert_eq!(decode::<Counted>("#{ timeout: () }").timeout, None);
    assert_eq!(decode::<Counted>("#{ timeout: 10 }").timeout, Some(10));
    assert!(parse("#{ timeout: \"invalid\" }").as_type::<Counted>().is_err());
    assert_eq!(CALLS.load(Ordering::SeqCst), 0);
    assert_eq!(decode::<Counted>("#{}").timeout, Some(30));
    assert_eq!(CALLS.load(Ordering::SeqCst), 1);
}

#[derive(Debug, PartialEq, Config, ToNode)]
struct Hooked {
    #[scry(from_node_with(read_optional), to_node_with(write_optional))]
    ordinary: Option<u32>,
    #[scry(default = Some(30), from_node_with(read_optional), to_node_with(write_optional))]
    defaulted: Option<u32>,
}

#[derive(Debug, PartialEq, Config, ToNode)]
enum HookedMode {
    Named {
        #[scry(from_node_with(read_optional), to_node_with(write_optional))]
        ordinary: Option<u32>,
        #[scry(default = Some(30), from_node_with(read_optional), to_node_with(write_optional))]
        defaulted: Option<u32>,
    },
}

#[test]
fn struct_hooks_receive_complete_optional_fields_and_skip_missing_input() {
    assert_eq!(
        decode::<Hooked>("#{}"),
        Hooked {
            ordinary: None,
            defaulted: Some(30)
        }
    );
    assert_eq!(
        decode::<Hooked>("#{ ordinary: (), defaulted: () }"),
        Hooked {
            ordinary: Some(99),
            defaulted: Some(99)
        },
    );
    for value in [
        Hooked {
            ordinary: None,
            defaulted: None,
        },
        Hooked {
            ordinary: Some(10),
            defaulted: Some(30),
        },
    ] {
        let node = value.to_node().unwrap();
        if value.ordinary.is_none() {
            assert_eq!(node.req::<String>("ordinary").unwrap(), "disabled");
            assert_eq!(node.req::<String>("defaulted").unwrap(), "disabled");
        }
        assert_eq!(node.as_type::<Hooked>().unwrap(), value);
    }
}

#[test]
fn named_variant_hooks_use_the_same_complete_field_contract() {
    assert_eq!(
        decode::<HookedMode>("#{ named: #{} }"),
        HookedMode::Named {
            ordinary: None,
            defaulted: Some(30)
        },
    );
    assert_eq!(
        decode::<HookedMode>("#{ named: #{ ordinary: (), defaulted: () } }"),
        HookedMode::Named {
            ordinary: Some(99),
            defaulted: Some(99)
        },
    );
    let value = HookedMode::Named {
        ordinary: None,
        defaulted: None,
    };
    let node = value.to_node().unwrap();
    assert_eq!(node.req::<String>("named.ordinary").unwrap(), "disabled");
    assert_eq!(node.req::<String>("named.defaulted").unwrap(), "disabled");
    assert_eq!(node.as_type::<HookedMode>().unwrap(), value);
}

fn read_optional(node: &Node) -> Result<Option<u32>, NodeError> {
    match node.read_leaf("optional count")? {
        Value::Null => Ok(Some(99)),
        Value::String(value) if value == "disabled" => Ok(None),
        _ => u32::from_node(node).map(Some),
    }
}

fn write_optional(value: &Option<u32>) -> Result<Node, NodeError> {
    match value {
        Some(value) => value.to_node(),
        None => "disabled".to_node(),
    }
}

#[test]
fn missing_null_and_empty_child_maps_select_different_policies() {
    #[derive(Debug, Config)]
    struct Parent {
        #[scry(default = Child { port: 9000 })]
        child: Child,
        #[scry(from_defaults)]
        recursive: Child,
        optional: Option<Child>,
    }

    #[derive(Debug, Config)]
    struct Child {
        #[scry(default = 8080)]
        port: u16,
    }

    let missing: Parent = decode("#{}");
    assert_eq!(missing.child.port, 9000);
    assert_eq!(missing.recursive.port, 8080);
    assert!(missing.optional.is_none());

    let empty: Parent = decode("#{ child: #{}, recursive: #{}, optional: #{} }");
    assert_eq!(empty.child.port, 8080);
    assert_eq!(empty.recursive.port, 8080);
    assert_eq!(empty.optional.unwrap().port, 8080);
    assert!(decode::<Parent>("#{ optional: () }").optional.is_none());

    for source in ["()", "#{ child: () }", "#{ recursive: () }"] {
        assert!(parse(source).as_type::<Parent>().is_err(), "accepted {source}");
    }
}

#[test]
fn empty_named_structures_require_maps_even_when_unknown_keys_are_allowed() {
    #[derive(Debug, Config)]
    #[scry(allow_unknown_keys)]
    struct Empty {}

    #[derive(Debug, Config)]
    enum EmptyMode {
        Named {},
    }

    decode::<Empty>("#{}");
    decode::<EmptyMode>("#{ named: #{} }");
    for source in ["()", "[]", "42"] {
        assert!(parse(source).as_type::<Empty>().is_err(), "accepted {source}");
        let payload = format!("#{{ named: {source} }}");
        assert!(parse(&payload).as_type::<EmptyMode>().is_err(), "accepted {payload}");
    }
}

#[test]
fn required_raw_nodes_preserve_null_without_making_the_key_optional() {
    #[derive(Debug, Config)]
    struct Raw {
        value: Node,
    }

    let raw: Raw = decode("#{ value: () }");
    assert!(matches!(raw.value.read_leaf("raw value").unwrap(), Value::Null));
    assert!(parse("#{}").as_type::<Raw>().is_err());
    let root: Node = decode("()");
    assert!(matches!(root.read_leaf("raw root").unwrap(), Value::Null));
}

#[test]
fn direct_decoders_consume_nulls_and_unit_enum_leaves_in_all_container_shapes() {
    #[derive(Debug, PartialEq, Config)]
    struct Wrapped(Option<u32>);

    #[derive(Debug, PartialEq, Config)]
    struct Pair(Option<u32>, Option<String>);

    #[derive(Debug, PartialEq, Config)]
    enum Mode {
        Auto,
        Value(Option<u32>),
    }

    assert_eq!(decode::<Option<u32>>("()"), None);
    assert_eq!(decode::<Vec<Option<u32>>>("[(), 2]"), [None, Some(2)]);
    assert_eq!(decode::<[Option<u32>; 2]>("[(), 2]"), [None, Some(2)]);
    assert_eq!(decode::<(Option<u32>, Option<String>)>("[(), ()]"), (None, None));
    assert_eq!(decode::<Wrapped>("()"), Wrapped(None));
    assert_eq!(decode::<Pair>("[(), ()]"), Pair(None, None));
    assert_eq!(decode::<Mode>("#{ value: () }"), Mode::Value(None));
    assert_eq!(decode::<Mode>("\"auto\""), Mode::Auto);
    assert_eq!(decode::<[Option<Mode>; 2]>("[(), \"auto\"]"), [None, Some(Mode::Auto)]);
    assert!(parse("#{ auto: () }").as_type::<Mode>().is_err());
    assert!(parse("#{ timeout: (), unknown: () }").as_type::<Timeout>().is_err());
}

#[test]
fn aliases_can_decode_null_but_need_an_explicit_omission_policy() {
    type OptionalTimeout = Option<u64>;

    #[derive(Debug, Config)]
    struct Aliased {
        required: OptionalTimeout,
        #[scry(default = None)]
        omittable: OptionalTimeout,
    }

    let value: Aliased = decode("#{ required: () }");
    assert_eq!(value.required, None);
    assert_eq!(value.omittable, None);
    assert!(parse("#{}").as_type::<Aliased>().is_err());
}

#[test]
fn null_flags_disable_defaults_while_removal_restores_them_in_cli_order() {
    for (args, expected) in [
        (vec!["test", "--no-timeout"], None),
        (vec!["test", "--no-timeout", "--remove", "timeout"], Some(30)),
        (
            vec![
                "test",
                "--timeout",
                "10",
                "--remove",
                "timeout",
                "--no-timeout",
            ],
            None,
        ),
    ] {
        let (node, _) = apply_cli(&args);
        assert_eq!(node.as_type::<Timeout>().unwrap().timeout, expected);
    }
}

#[test]
fn queries_distinguish_explicit_null_from_missing_without_typed_conversion() {
    #[derive(Debug, Config)]
    struct Concrete {
        timeout: u64,
    }

    let queries = QueryArgs::standard();
    let (node, matches) = apply_cli(&["test", "--no-timeout", "--get-as", "rhai", "timeout"]);
    let output = queries.check_get::<Concrete>(&node, &matches).unwrap().unwrap();
    assert!(matches!(parse(&output).read_leaf("queried value").unwrap(), Value::Null));
    assert!(node.ensure_no_unknown_keys().is_err(), "queries must not consume null");
    assert!(node.as_type::<Concrete>().is_err());

    let (node, matches) = apply_cli(&["test", "--get-as", "rhai", "timeout"]);
    let error = queries.check_get::<Timeout>(&node, &matches).unwrap_err().to_string();
    assert!(error.contains("does not exist"), "{error}");
    assert_eq!(node.as_type::<Timeout>().unwrap().timeout, Some(30));

    // Reading the field keeps the type's non-null case exercised too.
    assert_eq!(decode::<Concrete>("#{ timeout: 10 }").timeout, 10);
}

#[test]
fn cli_values_that_spell_null_are_still_literal_strings() {
    for args in [
        vec!["test", "--set", "timeout", "null"],
        vec!["test", "--timeout", "()"],
    ] {
        let (node, _) = apply_cli(&args);
        assert_eq!(node.req::<String>("timeout").unwrap(), *args.last().unwrap());
        assert!(node.as_type::<Timeout>().is_err());
    }
}

#[test]
fn null_capable_formats_preserve_defaulted_none_through_typed_output() {
    let formats = [
        (Format::Rhai, Format::Rhai, "#{ timeout: () }"),
        #[cfg(feature = "format-json")]
        (Format::Json, Format::Json, r#"{ "timeout": null }"#),
        #[cfg(feature = "format-json5")]
        (Format::Json5, Format::Json, "{ timeout: null }"),
        #[cfg(feature = "format-yaml")]
        (Format::Yaml, Format::Yaml, "timeout: null"),
    ];

    for (input, output, source) in formats {
        let value: Timeout = Node::parse_str(source, input.clone()).unwrap().as_type().unwrap();
        assert_eq!(value.timeout, None);
        let serialized = value.to_node().unwrap().to_string_as(output).unwrap();
        let restored: Timeout = Node::parse_str(&serialized, input).unwrap().as_type().unwrap();
        assert_eq!(restored, value);
    }
}

#[cfg(feature = "format-toml")]
#[test]
fn toml_rejects_typed_none_and_retains_null_free_raw_documents() {
    let value = Timeout { timeout: None };
    let error = value.to_node().unwrap().to_string_as(Format::Toml).unwrap_err().to_string();
    assert!(error.contains("null"), "{error}");
    assert!(error.contains("timeout"), "{error}");

    #[derive(ToNode)]
    struct Nested {
        children: Vec<Timeout>,
    }

    let nested = Nested {
        children: vec![value],
    }
    .to_node()
    .unwrap();
    let error = nested.to_string_as(Format::Toml).unwrap_err().to_string();
    assert!(error.contains("children[0].timeout"), "{error}");

    let error = parse("#{ timeout: () }").to_string_as(Format::Toml).unwrap_err().to_string();
    assert!(error.contains("timeout"), "{error}");

    let anchored = parse("#{ outer: #{ timeout: () } }");
    let error =
        anchored.req_node("outer").unwrap().to_string_as(Format::Toml).unwrap_err().to_string();
    assert!(error.contains("outer.timeout"), "{error}");

    let raw = Node::parse_str("timeout = 10", Format::Toml).unwrap();
    let output = raw.to_string_as(Format::Toml).unwrap();
    assert_eq!(
        Node::parse_str(&output, Format::Toml).unwrap().as_type::<Timeout>().unwrap(),
        Timeout { timeout: Some(10) },
    );
}

// ---------------------------------------------------------------------------------------------- //

fn parse(source: &str) -> Node {
    Node::parse_str(source, Format::Rhai).unwrap()
}

fn decode<T: FromNode>(source: &str) -> T {
    let node = parse(source);
    let value = T::from_node(&node).unwrap();
    node.ensure_no_unknown_keys().unwrap();
    value
}

fn apply_cli(args: &[&str]) -> (Node, ArgMatches) {
    let mut expose = ExposeMap::new();
    expose.option("timeout");
    expose.flag("timeout", None::<u64>).long("no-timeout");
    let overrides = OverrideArgs::standard();
    let command = QueryArgs::standard()
        .augment(overrides.augment(expose.augment(Command::new("test"), &Timeout::describe())));
    let matches = command.try_get_matches_from(args).unwrap();
    let mut node = Node::empty_map();
    overrides.apply::<Timeout>(&mut node, &expose, &matches).unwrap();
    (node, matches)
}
