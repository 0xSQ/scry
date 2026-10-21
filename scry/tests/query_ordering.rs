//! Covers declaration-ordered inspection without changing the loaded input.
#![cfg(feature = "format-json")]
#![allow(dead_code)]

use std::convert::Infallible;
use std::path::Path;

use scry::cli::setup::{format_node_at_path, ConfigSource, GetFormat, Setup};
use scry::desc::FieldDesc;
use scry::node::{Format, Kind, Value};
use scry::writer::TreeConfig;
use scry::{Config, Desc, Describe, Node};

// ---------------------------------------------------------------------------------------------- //

#[derive(Debug, Config)]
struct Input {
    zeta: u32,
    alpha: String,
    #[scry(rename = "literal.key")]
    literal: Pair,
    nested: Pair,
    rows: Vec<Pair>,
    tuple: (Pair, Pair),
    mode: Mode,
    #[scry(default = 99)]
    missing: u32,
}

#[derive(Debug, Config)]
struct Pair {
    second: u32,
    first: String,
}

#[derive(Debug, Config)]
enum Mode {
    Payload(Pair),
    Unit,
}

struct Conservative;

impl Describe for Conservative {
    fn describe() -> Desc {
        Desc::structure(vec![
            FieldDesc::new("opaque", Desc::plain("custom input")),
            FieldDesc::new("list", Desc::list(Pair::describe())),
            FieldDesc::new("tuple", Desc::tuple(vec![Pair::describe()])),
            FieldDesc::new("choice", Mode::describe()),
            FieldDesc::new("unit", Mode::describe()),
            FieldDesc::new("unknown_variant", Mode::describe()),
        ])
    }
}

struct Duplicates;

impl Describe for Duplicates {
    fn describe() -> Desc {
        Desc::structure(vec![
            FieldDesc::new("same", Pair::describe()),
            FieldDesc::new("last", Desc::plain("integer")),
            FieldDesc::new(
                "same",
                Desc::structure(vec![
                    FieldDesc::new("first", Desc::plain("string")),
                    FieldDesc::new("second", Desc::plain("integer")),
                ]),
            ),
        ])
    }
}

// ---------------------------------------------------------------------------------------------- //

#[test]
fn rhai_engine_order_is_replaced_consistently_in_every_inspection_view() {
    // Authored order, Rhai's lexical order, and declaration order all differ.
    let node = Node::parse_str(
        r#"#{ zeta: 1, nested: #{ first: "n", second: 2 }, alpha: "a" }"#,
        Format::Rhai,
    )
    .unwrap();
    assert_eq!(keys(&node), ["alpha", "nested", "zeta"]);

    assert_eq!(
        inspect::<Input>(&node, "", GetFormat::Flat),
        "zeta 1\nalpha \"a\"\nnested.second 2\nnested.first \"n\""
    );
    assert_eq!(
        inspect::<Input>(&node, "", GetFormat::Tree(TreeConfig::no_color())),
        "▸ zeta 1\n▸ alpha \"a\"\n▾ nested\n   ▸ second 2\n   ▸ first \"n\""
    );
    let json = inspect::<Input>(&node, "", GetFormat::Format(Format::Json));
    let json = Node::parse_str(&json, Format::Json).unwrap();
    assert_eq!(keys(&json), ["zeta", "alpha", "nested"]);
    assert_eq!(keys(json.req_node("nested").unwrap()), ["second", "first"]);

    let rhai = inspect::<Input>(&node, "", GetFormat::Format(Format::Rhai));
    assert!(rhai.find("zeta:").unwrap() < rhai.find("alpha:").unwrap(), "{rhai}");
    assert!(rhai.find("alpha:").unwrap() < rhai.find("nested:").unwrap(), "{rhai}");
    assert!(rhai.find("second:").unwrap() < rhai.find("first:").unwrap(), "{rhai}");
    assert_eq!(keys(&node), ["alpha", "nested", "zeta"]);
}

#[test]
fn json_and_custom_loader_input_use_the_same_policy_without_changing_stored_order() {
    let source = r#"{ "unknown_z": 8, "alpha": "a", "unknown_a": 9, "zeta": 1 }"#;
    let node = Node::parse_str(source, Format::Json).unwrap();
    let before = node.to_flat_string();
    let expected = "zeta 1\nalpha \"a\"\nunknown_z 8\nunknown_a 9";
    assert_eq!(inspect::<Input>(&node, "", GetFormat::Flat), expected);
    assert_eq!(node.to_flat_string(), before);

    // The loader returns an ordinary Node, without source-format metadata.
    let loaded = Setup::new("test")
        .config_source(|_| {
            ConfigSource::new().option("config", Some('C'), "Loads input.").loader(
                move |_: &Path| -> Result<Node, Infallible> {
                    Ok(Node::parse_str(source, Format::Json).unwrap())
                },
            )
        })
        .into_bundle(|node: Node| node)
        .run_from(["test", "-C", "input.custom"])
        .unwrap()
        .unwrap();
    assert_eq!(loaded.to_flat_string(), before);
    assert_eq!(inspect::<Input>(&loaded, "", GetFormat::Flat), expected);
}

#[test]
fn nested_structs_lists_tuples_and_tagged_payloads_follow_their_value_descriptions() {
    let node = json(
        r#"{
            "mode": { "payload": { "first": "p", "second": 6 } },
            "tuple": [
                { "first": "t0", "second": 4 },
                { "first": "t1", "second": 5 },
                { "first": "extra", "second": 7 }
            ],
            "rows": [{ "first": "r0", "second": 2 }, { "first": "r1", "second": 3 }],
            "nested": { "extra_z": 8, "first": "n", "extra_a": 9, "second": 1 }
        }"#,
    );
    assert_eq!(
        inspect::<Input>(&node, "", GetFormat::Flat),
        concat!(
            "nested.second 1\nnested.first \"n\"\nnested.extra_z 8\nnested.extra_a 9\n",
            "rows[0].second 2\nrows[0].first \"r0\"\nrows[1].second 3\nrows[1].first \"r1\"\n",
            "tuple[0].second 4\ntuple[0].first \"t0\"\ntuple[1].second 5\ntuple[1].first \"t1\"\n",
            "tuple[2].first \"extra\"\ntuple[2].second 7\nmode.payload.second 6\nmode.payload.first \"p\""
        )
    );
    for (path, expected) in [
        ("nested", "nested.second 1\nnested.first \"n\"\nnested.extra_z 8\nnested.extra_a 9"),
        ("rows[1]", "rows[1].second 3\nrows[1].first \"r1\""),
        ("tuple[0]", "tuple[0].second 4\ntuple[0].first \"t0\""),
        ("tuple[2]", "tuple[2].first \"extra\"\ntuple[2].second 7"),
        ("mode.payload", "mode.payload.second 6\nmode.payload.first \"p\""),
    ] {
        assert_eq!(inspect::<Input>(&node, path, GetFormat::Flat), expected, "{path}");
    }
}

#[test]
fn renamed_literal_keys_missing_fields_and_invalid_raw_values_stay_inspectable() {
    let node = json(
        r#"{
            "literal": { "key": { "first": "unknown", "second": 7 } },
            "literal.key": { "first": null, "second": "not a number" },
            "nested": null,
            "alpha": false,
            "zeta": "23",
            "empty-map": {},
            "empty-list": []
        }"#,
    );
    let ordered = inspect::<Input>(&node, "", GetFormat::Format(Format::Json));
    let ordered = json(&ordered);
    assert_eq!(
        keys(&ordered),
        [
            "zeta",
            "alpha",
            "literal.key",
            "nested",
            "literal",
            "empty-map",
            "empty-list"
        ]
    );
    assert!(ordered.opt_node("missing").unwrap().is_none());
    assert!(ordered.req_node(r#"["empty-map"]"#).unwrap().as_map().unwrap().is_empty());
    assert!(ordered.req_node(r#"["empty-list"]"#).unwrap().as_vec().unwrap().is_empty());
    assert_eq!(
        inspect::<Input>(&node, r#"["literal.key"]"#, GetFormat::Flat),
        "[\"literal.key\"].second \"not a number\"\n[\"literal.key\"].first null"
    );
    assert_eq!(
        inspect::<Input>(&node, "literal.key", GetFormat::Flat),
        "literal.key.first \"unknown\"\nliteral.key.second 7"
    );
    assert!(matches!(node.req_node("zeta").unwrap().as_leaf().unwrap().value, Value::String(_)));
    assert!(matches!(ordered.req_node("zeta").unwrap().as_leaf().unwrap().value, Value::String(_)));
}

#[test]
fn opaque_mismatched_and_invalid_enum_boundaries_never_regain_ordering_hints() {
    let node = json(
        r#"{
            "opaque": { "rows": [{ "first": "o", "second": 1 }] },
            "list": { "0": { "first": "l", "second": 2 } },
            "tuple": [[{ "first": "t", "second": 3 }]],
            "choice": { "payload": { "first": "p", "second": 4 }, "other": 0 },
            "unit": { "unit": { "first": "u", "second": 5 } },
            "unknown_variant": { "other": { "first": "v", "second": 6 } },
            "unknown": { "nested": { "first": "x", "second": 7 } }
        }"#,
    );
    for path in [
        "opaque",
        "opaque.rows[0]",
        r#"list["0"]"#,
        "tuple[0][0]",
        "choice",
        "choice.payload",
        "unit.unit",
        "unknown_variant.other",
        "unknown.nested",
    ] {
        assert_eq!(
            inspect::<Conservative>(&node, path, GetFormat::Flat),
            node.req_node(path).unwrap().to_flat_string(),
            "{path}"
        );
    }
}

#[test]
fn repeated_description_names_emit_one_field_using_the_first_description() {
    let node = json(r#"{ "last": 3, "same": { "first": "a", "second": 2 } }"#);
    assert_eq!(
        inspect::<Duplicates>(&node, "", GetFormat::Flat),
        "same.second 2\nsame.first \"a\"\nlast 3"
    );
    assert_eq!(
        inspect::<Duplicates>(&node, "same", GetFormat::Flat),
        "same.second 2\nsame.first \"a\""
    );
}

#[test]
fn tree_depth_and_indices_render_the_same_declaration_order_relative_to_the_selection() {
    let node = json(r#"{ "rows": [{ "first": "r", "second": 2 }], "alpha": "a", "zeta": 1 }"#);
    let shallow = TreeConfig {
        max_depth: Some(1),
        show_leaf_indices: true,
        ..TreeConfig::no_color()
    };
    assert_eq!(
        inspect::<Input>(&node, "", GetFormat::Tree(shallow.clone())),
        "▸ zeta 1\n▸ alpha \"a\"\n▪ rows …"
    );
    assert_eq!(inspect::<Input>(&node, "rows", GetFormat::Tree(shallow.clone())), "▾ [0] …");
    assert_eq!(
        inspect::<Input>(&node, "rows[0]", GetFormat::Tree(shallow)),
        "▸ second 2\n▸ first \"r\""
    );
    let values = json(r#"{ "rows": [2, "raw", null] }"#);
    assert_eq!(
        inspect::<Input>(
            &values,
            "rows",
            GetFormat::Tree(TreeConfig {
                show_leaf_indices: true,
                ..TreeConfig::no_color()
            })
        ),
        "▸ [0] 2\n▸ [1] \"raw\"\n▸ [2] null"
    );
}

#[test]
fn inspection_preserves_original_order_paths_raw_types_values_and_visit_state() {
    let mut node = json(
        r#"{ "rows": [{ "first": "r", "second": 2 }], "alpha": "a", "zeta": "raw", "unknown": null }"#,
    );
    node.set_value("rows[0].second", 7_u16).unwrap();
    node.set_value("unknown", 1.25_f32).unwrap();
    node.req_node("alpha").unwrap().as_leaf().unwrap().mark_visited();
    let before = snapshot(&node);

    for path in ["", "rows", "rows[0]", "alpha", "unknown"] {
        for format in [
            GetFormat::Tree(TreeConfig::no_color()),
            GetFormat::Flat,
            GetFormat::Format(Format::Json),
            GetFormat::Format(Format::Rhai),
        ] {
            let output = inspect::<Input>(&node, path, format);
            assert!(!output.is_empty());
            assert_eq!(snapshot(&node), before, "{path}");
        }
    }
    assert!(node.req_node("alpha").unwrap().as_leaf().unwrap().is_visited());
    assert!(!node.req_node("zeta").unwrap().as_leaf().unwrap().is_visited());
    assert!(!node.req_node("rows[0].first").unwrap().as_leaf().unwrap().is_visited());
}

// ---------------------------------------------------------------------------------------------- //

fn json(source: &str) -> Node {
    Node::parse_str(source, Format::Json).unwrap()
}

fn keys(node: &Node) -> Vec<&str> {
    node.as_map().unwrap().keys().map(String::as_str).collect()
}

fn inspect<T: Describe>(node: &Node, path: &str, format: GetFormat) -> String {
    format_node_at_path::<T>(node, path, format).unwrap()
}

fn snapshot(node: &Node) -> Vec<String> {
    let mut entries = Vec::new();
    fn collect(node: &Node, entries: &mut Vec<String>) {
        match &node.kind {
            Kind::Map(map) => {
                entries.push(format!("{}: map {:?}", node.path, map.keys().collect::<Vec<_>>()));
                for child in map.values() {
                    collect(child, entries);
                }
            }
            Kind::Vec(items) => {
                entries.push(format!("{}: array {}", node.path, items.len()));
                for child in items {
                    collect(child, entries);
                }
            }
            Kind::Leaf(leaf) => {
                entries.push(format!(
                    "{}: {:?}, visited={}",
                    node.path,
                    leaf.value,
                    leaf.is_visited()
                ));
            }
        }
    }
    collect(node, &mut entries);
    entries
}
