//! Covers object-local unknown-key checks independently of the explicit leaf-read audit.
#![cfg(feature = "format-json")]

use indoc::indoc;
use scry::node::Format;
use scry::{FromNode, KeyPath, Node, NodeError};

// ---------------------------------------------------------------------------------------------- //

#[derive(Debug, FromNode)]
struct Strict {
    known: u8,
}

#[derive(Debug, FromNode)]
#[scry(allow_unknown_keys)]
struct Loose {
    known: u8,
}

#[derive(Debug, FromNode)]
struct StrictWithLoose {
    child: Loose,
}

#[derive(Debug, FromNode)]
#[scry(allow_unknown_keys)]
struct LooseWithStrict {
    child: Strict,
}

#[derive(Debug, FromNode)]
enum Named {
    Payload { child: Loose },
}

#[derive(Debug, FromNode)]
struct RawPayload {
    known: u8,
    raw: Node,
}

#[derive(Debug, FromNode)]
struct Renamed {
    #[scry(rename = "model.version")]
    known: u8,
}

#[test]
fn strict_objects_reject_unknown_values_of_every_shape() {
    for value in ["1", "null", "{}", "[]", r#"{"nested":1}"#, "[1]"] {
        let input = node(&format!(r#"{{"known":1,"extra":{value}}}"#));

        assert_unknown_paths(input.as_type::<Strict>().unwrap_err(), &["extra"]);
    }
}

#[test]
fn unknown_keys_are_aggregated_at_the_immediate_object_boundary() {
    let input = node(indoc! {r#"
        {
            "known": 1,
            "scalar": 2,
            "nothing": null,
            "empty_map": {},
            "empty_list": [],
            "nested_map": { "value": 3 },
            "nested_list": [4]
        }
    "#});

    assert_unknown_paths(
        input.as_type::<Strict>().unwrap_err(),
        &[
            "scalar",
            "nothing",
            "empty_map",
            "empty_list",
            "nested_map",
            "nested_list",
        ],
    );
}

#[test]
fn a_strict_parent_respects_a_loose_child_and_rejects_its_own_extras() {
    let input = node(r#"{"child":{"known":1,"ignored":{"nested":2}}}"#);
    assert_eq!(input.as_type::<StrictWithLoose>().unwrap().child.known, 1);

    let input = node(r#"{"child":{"known":1,"ignored":{"nested":2}},"extra":{}}"#);
    assert_unknown_paths(input.as_type::<StrictWithLoose>().unwrap_err(), &["extra"]);
}

#[test]
fn a_loose_parent_still_respects_a_strict_child() {
    let input = node(r#"{"child":{"known":1},"ignored":[]}"#);
    assert_eq!(input.as_type::<LooseWithStrict>().unwrap().child.known, 1);

    let input = node(r#"{"child":{"known":1,"extra":null},"ignored":[]}"#);
    assert_unknown_paths(input.as_type::<LooseWithStrict>().unwrap_err(), &["child.extra"]);
}

#[test]
fn named_enum_payloads_respect_loose_children_and_reject_their_own_extras() {
    let input = node(r#"{"payload":{"child":{"known":1,"ignored":[]}}}"#);
    let Named::Payload { child } = input.as_type::<Named>().unwrap();
    assert_eq!(child.known, 1);

    let input = node(r#"{"payload":{"child":{"known":1,"ignored":[]},"extra":{"nested":2}}}"#);
    assert_unknown_paths(input.as_type::<Named>().unwrap_err(), &["payload.extra"]);
}

#[test]
fn map_hooks_own_their_local_key_validation() {
    #[derive(Debug, FromNode)]
    struct Hooked {
        #[scry(from_node_with(parse_child))]
        child: u8,
    }

    fn parse_child(node: &Node) -> Result<u8, NodeError> {
        let known = node.req("known")?;
        node.ensure_only_keys(&["known"])?;
        Ok(known)
    }

    let input = node(r#"{ "child": { "known": 1 } }"#);
    assert_eq!(input.as_type::<Hooked>().unwrap().child, 1);

    let input = node(r#"{ "child": { "known": 1, "extra": {} } }"#);
    assert_unknown_paths(input.as_type::<Hooked>().unwrap_err(), &["child.extra"]);

    let input = node(r#"{ "child": { "known": 1 }, "extra": [] }"#);
    assert_unknown_paths(input.as_type::<Hooked>().unwrap_err(), &["extra"]);
}

#[test]
fn successful_reads_repeated_decodes_and_clones_do_not_hide_unknown_keys() {
    let input = node(r#"{"known":1,"extra":2}"#);
    let untouched_clone = input.clone();

    assert_eq!(input.as_type::<Loose>().unwrap().known, 1);
    assert_eq!(input.req::<u8>("extra").unwrap(), 2);
    input.ensure_no_unknown_keys().unwrap();

    let visited_clone = input.clone();
    for current in [&input, &untouched_clone, &visited_clone] {
        for _ in 0..2 {
            assert_unknown_paths(current.as_type::<Strict>().unwrap_err(), &["extra"]);
        }
        assert_eq!(current.as_type::<Loose>().unwrap().known, 1);
    }

    let valid = node(r#"{"known":1}"#);
    for _ in 0..2 {
        assert_eq!(valid.as_type::<Strict>().unwrap().known, 1);
    }
}

#[test]
fn failed_reads_and_decodes_do_not_hide_unknown_keys() {
    #[derive(Debug, FromNode)]
    struct ReadThenFail {
        extra: u8,
        known: bool,
    }

    let input = node(r#"{"known":1,"extra":2}"#);
    assert!(input.req::<bool>("extra").is_err());
    assert_unknown_paths(input.as_type::<Strict>().unwrap_err(), &["extra"]);

    let input = node(r#"{"known":1,"extra":2}"#);
    assert!(input.as_type::<ReadThenFail>().map(|value| (value.extra, value.known)).is_err());
    input.ensure_no_unknown_keys().unwrap();
    for current in [&input, &input.clone()] {
        assert_unknown_paths(current.as_type::<Strict>().unwrap_err(), &["extra"]);
    }
}

#[test]
fn raw_subtrees_still_consume_leaves_for_the_explicit_read_audit() {
    let input = node(r#"{"known":1,"raw":{"nested":[null,{"value":2}],"empty":{}}}"#);
    let value = input.as_type::<RawPayload>().unwrap();

    assert_eq!(value.known, 1);
    assert_eq!(value.raw.req::<u8>("nested[1].value").unwrap(), 2);
    input.ensure_no_unknown_keys().unwrap();

    let input = node(r#"{"known":1,"raw":{"nested":[null,{"value":2}]},"extra":[]}"#);
    assert_unknown_paths(input.as_type::<RawPayload>().unwrap_err(), &["extra"]);
}

#[test]
fn permissive_decoding_keeps_the_explicit_leaf_read_audit_available() {
    let input = node(r#"{"known":1,"ignored":{"nested":2}}"#);
    assert_eq!(input.as_type::<Loose>().unwrap().known, 1);
    assert_unknown_paths(input.ensure_no_unknown_keys().unwrap_err(), &["ignored.nested"]);

    assert_eq!(input.req::<u8>("ignored.nested").unwrap(), 2);
    input.ensure_no_unknown_keys().unwrap();
}

#[test]
fn literal_renames_are_the_allowed_keys_and_unknown_paths_keep_their_literal_identity() {
    let input = node(r#"{"model.version":1}"#);
    assert_eq!(input.as_type::<Renamed>().unwrap().known, 1);

    let input = node(r#"{"model.version":1,"other.key":{}}"#);
    assert_unknown_paths(input.as_type::<Renamed>().unwrap_err(), &[r#"["other.key"]"#]);
}

#[test]
fn the_key_membership_helper_requires_maps_and_does_not_consume_values() {
    for value in ["1", "null", "[]"] {
        assert!(matches!(
            node(value).ensure_only_keys(&[]),
            Err(NodeError::TypeMismatch { target_type, .. }) if target_type == "map"
        ));
    }

    let input = node(r#"{"known":1}"#);
    input.ensure_only_keys(&["known"]).unwrap();
    assert_unknown_paths(input.ensure_no_unknown_keys().unwrap_err(), &["known"]);
    assert_eq!(input.req::<u8>("known").unwrap(), 1);
    input.ensure_no_unknown_keys().unwrap();
}

fn node(json: &str) -> Node {
    Node::parse_str(json, Format::Json).unwrap()
}

fn assert_unknown_paths(error: NodeError, expected: &[&str]) {
    let NodeError::UnknownKeys { paths } = error else {
        panic!("expected unknown keys, got {error:?}");
    };
    let expected: Vec<KeyPath> = expected.iter().map(|path| path.parse().unwrap()).collect();
    assert_eq!(paths, expected);
}
