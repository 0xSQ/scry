//! Regression tests for literal derived field names.
#![cfg(feature = "format-json")]

use scry::cli::setup::Setup;
use scry::desc::EntryRef;
use scry::node::Format;
use scry::{Config, Describe, FromDefaults, FromNode, KeyPath, Node, NodeError, ToNode};

// ---------------------------------------------------------------------------------------------- //

#[derive(Debug, PartialEq, Config, ToNode)]
struct LiteralKeys {
    #[scry(rename = "server.port")]
    dotted: u32,
    #[scry(rename = "limits[0]")]
    bracketed: u32,
    #[scry(rename = "say\"hello")]
    quoted: u32,
    #[scry(rename = "max retries")]
    spaced: u32,
    #[scry(rename = "worker-count")]
    hyphenated: u32,
    #[scry(rename = "")]
    empty: u32,
    #[scry(rename = "größer")]
    unicode: u32,
}

#[derive(Debug, PartialEq, Config, ToNode)]
enum Command {
    Run {
        #[scry(rename = "server.port")]
        port: u16,
        #[scry(rename = "worker-count", default = 2)]
        workers: u32,
    },
}

#[derive(Debug, PartialEq, FromNode, ToNode)]
enum DistinctForms {
    #[scry(rename = "FAST")]
    Upper(u32),
    #[scry(rename = "fast")]
    Lower(u32),
    #[scry(rename = "auto-mode")]
    Unit,
    #[scry(rename = "auto_mode")]
    Payload(u32),
}

#[derive(Debug, Config)]
struct ExposedPort {
    /// The server port.
    #[scry(rename = "server.port")]
    port: u16,
}

#[derive(Debug, FromNode)]
#[allow(dead_code)]
struct RequiredPort {
    #[scry(rename = "server.port")]
    port: u16,
}

#[derive(Debug, FromNode, FromDefaults)]
#[allow(dead_code)]
struct RequiredHost {
    #[scry(rename = "host.name")]
    host: String,
}

#[derive(Debug, FromNode)]
#[allow(dead_code)]
struct DefaultedChild {
    #[scry(rename = "database.settings", from_defaults)]
    child: RequiredHost,
}

#[derive(Debug, FromNode)]
#[allow(dead_code)]
struct Parent {
    #[scry(rename = "outer.config")]
    child: DefaultedChild,
}

#[derive(Debug, FromNode)]
#[allow(dead_code)]
enum DefaultedCommand {
    Run {
        #[scry(rename = "database.settings", from_defaults)]
        child: RequiredHost,
    },
}

fn node(source: &str) -> Node {
    Node::parse_str(source, Format::Json).unwrap()
}

// ---------------------------------------------------------------------------------------------- //

#[test]
fn literal_names_decode_and_round_trip() {
    let input = node(
        r#"{ "server.port": 1, "limits[0]": 2, "say\"hello": 3, "max retries": 4,
              "worker-count": 5, "": 6, "größer": 7 }"#,
    );
    let expected = LiteralKeys {
        dotted: 1,
        bracketed: 2,
        quoted: 3,
        spaced: 4,
        hyphenated: 5,
        empty: 6,
        unicode: 7,
    };

    assert_eq!(input.as_type::<LiteralKeys>().unwrap(), expected);
    let serialized = expected.to_node().unwrap();
    assert_eq!(serialized.as_type::<LiteralKeys>().unwrap(), expected);

    let reparsed = node(&serialized.to_string_as(Format::Json).unwrap());
    assert_eq!(reparsed.as_type::<LiteralKeys>().unwrap(), expected);
}

#[test]
fn named_variant_fields_use_literal_names_and_missing_defaults() {
    let input = node(r#"{ "run": { "server.port": 9000 } }"#);
    let expected = Command::Run {
        port: 9000,
        workers: 2,
    };

    assert_eq!(input.as_type::<Command>().unwrap(), expected);
    assert_eq!(expected.to_node().unwrap().as_type::<Command>().unwrap(), expected);
}

#[test]
fn dotted_rename_does_not_select_a_nested_path() {
    let input = node(r#"{ "server": { "port": 9000 } }"#);
    let error = input.as_type::<RequiredPort>().unwrap_err();

    assert!(matches!(error, NodeError::MissingRequired { ref path }
        if *path == KeyPath::from_keys(["server.port"])));
    assert_eq!(error.to_string(), "missing value for '[\"server.port\"]'");
}

#[test]
fn recursive_defaults_keep_literal_segments_under_a_parent() {
    let input = node(r#"{ "outer.config": {} }"#);
    let error = input.as_type::<Parent>().unwrap_err();

    assert!(matches!(error, NodeError::MissingRequired { ref path }
        if *path == KeyPath::from_keys(["outer.config", "database.settings", "host.name"])));
}

#[test]
fn named_variant_recursive_defaults_keep_literal_segments() {
    let input = node(r#"{ "run": {} }"#);
    let error = input.as_type::<DefaultedCommand>().unwrap_err();

    assert!(matches!(error, NodeError::MissingRequired { ref path }
        if *path == KeyPath::from_keys(["run", "database.settings", "host.name"])));
}

#[test]
fn descriptions_select_the_same_literal_keys() {
    let description = LiteralKeys::describe();
    for key in [
        "server.port",
        "limits[0]",
        "say\"hello",
        "max retries",
        "worker-count",
        "",
        "größer",
    ] {
        let path = KeyPath::from_keys([key]);
        description.validate_path(&path).unwrap();
        let Some(EntryRef::Field(field)) = description.entry_at_path(&path) else {
            panic!("missing description for literal key {key:?}");
        };
        assert_eq!(field.name, key);
        assert_eq!(field.value.type_label(), "u32");
    }

    assert!(description.validate_path("server.port").is_err());
    description.validate_path(r#"["server.port"]"#).unwrap();
}

#[test]
fn node_queries_keep_their_explicit_path_semantics() {
    let input = node(r#"{ "server": { "port": 9000 }, "server.port": 8000 }"#);

    assert_eq!(input.req::<u16>("server.port").unwrap(), 9000);
    assert_eq!(input.req::<u16>(r#"["server.port"]"#).unwrap(), 8000);
}

#[test]
fn cli_exposure_targets_a_literal_renamed_key() {
    let config = Setup::new("test")
        .expose(|expose| {
            expose.option(r#"["server.port"]"#).long("port");
        })
        .into_bundle(|config: ExposedPort| config)
        .run_from(["test", "--port", "9000"])
        .unwrap()
        .unwrap();

    assert_eq!(config.port, 9000);
}

#[test]
fn distinct_enum_input_forms_remain_valid() {
    for expected in [
        DistinctForms::Upper(1),
        DistinctForms::Lower(2),
        DistinctForms::Unit,
        DistinctForms::Payload(3),
    ] {
        assert_eq!(expected.to_node().unwrap().as_type::<DistinctForms>().unwrap(), expected);
    }

    assert_eq!(node(r#""AUTO_MODE""#).as_type::<DistinctForms>().unwrap(), DistinctForms::Unit);
    assert_eq!(
        node(r#"{ "auto-mode": 3 }"#).as_type::<DistinctForms>().unwrap(),
        DistinctForms::Payload(3),
    );
    assert!(node(r#""fast""#)
        .as_type::<DistinctForms>()
        .unwrap_err()
        .to_string()
        .contains("requires a payload"));
}
