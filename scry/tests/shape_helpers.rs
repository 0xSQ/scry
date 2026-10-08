//! Covers shared shape operations with foreign values, stateful callbacks, and real Node errors.

use std::cell::Cell;
use std::error::Error;
use std::net::IpAddr;
use std::num::ParseIntError;

use scry::convert::{read, write};
use scry::kit::{key_values, one_or_many, KeyValues, OneOrMany};
use scry::node::{Format, Value};
use scry::{Desc, Describe, KeyPath, Node, NodeError, ToNode};

// ---------------------------------------------------------------------------------------------- //

#[test]
fn foreign_values_compose_through_key_values_vectors_and_options() {
    let input = node(r#"#{ "accent.color": ["127.0.0.1", ()], warning: ["::1"] }"#);
    let values = key_values::read_with(&input, |child| {
        read::vec(child, |entry| read::option(entry, read_address))
    })
    .unwrap();

    assert_eq!(values.entries()[0].0, "accent.color");
    assert_eq!(values.entries()[0].1, [Some("127.0.0.1".parse::<IpAddr>().unwrap()), None]);
    assert_eq!(values.entries()[1].1, [Some("::1".parse::<IpAddr>().unwrap())]);
    input.ensure_no_unknown_keys().unwrap();

    let output = key_values::write_with(&values, |items| {
        write::list(items, |entry| write::option(entry, write_address))
    })
    .unwrap();
    assert_eq!(output.req::<String>("[0][0]").unwrap(), "accent.color");
    assert_eq!(output.req::<Option<String>>("[0][1][1]").unwrap(), None);
    let round_trip = key_values::read_with(&output, |child| {
        read::vec(child, |entry| read::option(entry, read_address))
    })
    .unwrap();
    assert_eq!(round_trip, values);
}

#[test]
fn readers_preserve_order_duplicates_and_mutable_callback_state() {
    let input = node("[3, 3, 7]");
    let mut seen = Vec::new();
    let values = read::vec(&input, |child| {
        let value = child.as_type::<u8>()?;
        seen.push(child.path.clone());
        Ok((seen.len(), value))
    })
    .unwrap();

    assert_eq!(values, [(1, 3), (2, 3), (3, 7)]);
    assert_eq!(seen, (0..3).map(KeyPath::from_index).collect::<Vec<_>>());
    input.ensure_no_unknown_keys().unwrap();
}

#[test]
fn readers_do_not_mark_unread_leaves_or_later_children_consumed() {
    let unread = node("[1, 2]");
    assert_eq!(read::vec(&unread, |_| Ok(0)).unwrap(), [0, 0]);
    assert!(matches!(unread.ensure_no_unknown_keys(), Err(NodeError::UnknownKeys { .. })));

    let input = node(r#"[1, "bad", 3]"#);
    let mut calls = 0;
    let error = read::vec(&input, |child| {
        calls += 1;
        child.as_type::<u8>()
    })
    .unwrap_err();
    assert_eq!(calls, 2);
    assert_eq!(error.path(), Some(&KeyPath::from_index(1)));
    assert!(error.source().unwrap().is::<ParseIntError>());
    assert!(!input.req_node("[2]").unwrap().as_leaf().unwrap().is_visited());
}

#[test]
fn optional_helpers_consume_null_and_skip_inner_callbacks() {
    let input = Node::new_leaf(KeyPath::from_keys(["settings", "color"]), Value::Null);
    let reads = Cell::new(0);
    let value = read::option::<IpAddr>(&input, |_| {
        reads.set(reads.get() + 1);
        panic!("null must not call the inner reader");
    })
    .unwrap();
    assert_eq!(value, None);
    assert_eq!(reads.get(), 0);
    input.ensure_no_unknown_keys().unwrap();

    let writes = Cell::new(0);
    let output = write::option(&value, |_| {
        writes.set(writes.get() + 1);
        panic!("None must not call the inner writer");
    })
    .unwrap();
    assert_eq!(writes.get(), 0);
    assert_eq!(output.as_type::<Option<String>>().unwrap(), None);
    assert!(output.path.is_empty());
}

#[test]
fn fixed_shapes_check_arity_before_reading_any_child() {
    let input = node("[1]");
    let calls = Cell::new(0);
    let mut reader = |child: &Node| {
        calls.set(calls.get() + 1);
        child.as_type::<u8>()
    };

    let errors = [
        read::array::<_, 2>(&input, &mut reader).unwrap_err(),
        read::tuple2(&input, &mut reader, Node::as_type::<u8>).unwrap_err(),
        read::tuple3(&input, &mut reader, Node::as_type::<u8>, Node::as_type::<u8>).unwrap_err(),
        read::tuple4(
            &input,
            &mut reader,
            Node::as_type::<u8>,
            Node::as_type::<u8>,
            Node::as_type::<u8>,
        )
        .unwrap_err(),
    ];
    assert_eq!(calls.get(), 0);
    assert!(!input.req_node("[0]").unwrap().as_leaf().unwrap().is_visited());
    for error in errors {
        assert!(matches!(error, NodeError::ArrayLength { .. }));
        assert_eq!(error.path(), Some(&KeyPath::new()));
    }

    let empty = node("[]");
    let values: [BorrowedOnly; 0] =
        read::array(&empty, |_| panic!("empty array has no child")).unwrap();
    assert!(values.is_empty());
}

#[test]
fn array_and_heterogeneous_tuple_readers_need_no_native_foreign_capabilities() {
    let input = node(r#"["127.0.0.1", "::1"]"#);
    let addresses: [IpAddr; 2] = read::array(&input, read_address).unwrap();
    assert_eq!(addresses[0], "127.0.0.1".parse::<IpAddr>().unwrap());
    input.ensure_no_unknown_keys().unwrap();

    let pair =
        read::tuple2(&node(r#"["primary", "127.0.0.1"]"#), Node::as_type::<String>, read_address)
            .unwrap();
    assert_eq!(pair, ("primary".to_string(), addresses[0]));
    let triple = read::tuple3(
        &node(r#"[7, true, "::1"]"#),
        Node::as_type::<u8>,
        Node::as_type::<bool>,
        read_address,
    )
    .unwrap();
    assert_eq!(triple, (7, true, addresses[1]));
    let quadruple = read::tuple4(
        &node(r#"[7, "primary", true, "::1"]"#),
        Node::as_type::<u8>,
        Node::as_type::<String>,
        Node::as_type::<bool>,
        read_address,
    )
    .unwrap();
    assert_eq!(quadruple, (7, "primary".to_string(), true, addresses[1]));
}

#[test]
fn input_callbacks_gain_missing_locations_and_preserve_existing_paths_and_causes() {
    let input = node("#{ entries: [5] }");
    let entries = input.req_node("entries").unwrap();
    let error = read::vec::<()>(entries, |_| Err(unlocated_error())).unwrap_err();
    assert_eq!(error.path(), Some(&KeyPath::from_keys(["entries"]).push_index(0)));
    assert_original_cause(&error);

    let child = entries.req_node("[0]").unwrap();
    let error = read::option::<()>(child, |_| Err(unlocated_error())).unwrap_err();
    assert_eq!(error.path(), Some(&child.path));
    assert_original_cause(&error);

    for preserved in [KeyPath::new(), KeyPath::from_keys(["other", "specific"])] {
        let error = read::vec::<()>(entries, |_| {
            Err(NodeError::invalid_value_with_source(&preserved, "specific failure", DomainCause))
        })
        .unwrap_err();
        assert_eq!(error.path(), Some(&preserved));
        assert_original_cause(&error);
    }
}

#[test]
fn input_shape_mismatches_are_located_without_running_callbacks() {
    let input = Node::new_leaf(KeyPath::from_keys(["settings", "items"]), Value::Bool(true));
    let error = read::vec::<()>(&input, |_| panic!("wrong outer shape")).unwrap_err();
    assert_eq!(error.path(), Some(&input.path));
    assert!(!input.as_leaf().unwrap().is_visited());
    let error = key_values::read_with::<()>(&input, |_| panic!("wrong outer shape")).unwrap_err();
    assert_eq!(error.path(), Some(&input.path));
}

#[test]
fn key_values_preserve_duplicate_pairs_and_validate_pair_shape_before_values() {
    let input = node(r#"[["same", "127.0.0.1"], ["same", "::1"]]"#);
    let values = key_values::read_with(&input, read_address).unwrap();
    assert_eq!(values.keys().collect::<Vec<_>>(), ["same", "same"]);
    input.ensure_no_unknown_keys().unwrap();
    let output = key_values::write_with(&values, write_address).unwrap();
    assert_eq!(output.req::<String>("[1][0]").unwrap(), "same");
    assert_eq!(output.req::<String>("[1][1]").unwrap(), "::1");

    for source in [r#"["invalid pair"]"#, r#"[["key only"]]"#] {
        let invalid = node(source);
        let error =
            key_values::read_with::<IpAddr>(&invalid, |_| panic!("invalid pair")).unwrap_err();
        assert_eq!(error.path(), Some(&KeyPath::from_index(0)));
    }

    for source in ["#{ literal: 1 }", r#"[["literal", 1]]"#] {
        let input = node(source);
        let error = key_values::read_with::<()>(&input, |_| Err(unlocated_error())).unwrap_err();
        let expected = if source.starts_with("#{") {
            KeyPath::from_keys(["literal"])
        } else {
            KeyPath::from_index(0).push_index(1)
        };
        assert_eq!(error.path(), Some(&expected));
        assert_original_cause(&error);
    }
}

#[test]
fn one_or_many_preserves_array_valued_leaf_rules_and_singleton_error_locations() {
    let direct = node("[1, 2, 3]");
    let error = one_or_many::read_with(&direct, Node::as_type::<[u8; 3]>).unwrap_err();
    assert_eq!(error.path(), Some(&KeyPath::from_index(0)));

    let nested = node("[[1, 2, 3]]");
    let values = one_or_many::read_with(&nested, Node::as_type::<[u8; 3]>).unwrap();
    assert_eq!(values.as_slice(), [[1, 2, 3]]);
    nested.ensure_no_unknown_keys().unwrap();
    let output = one_or_many::write_with(&values, |item| write::list(item, u8::to_node)).unwrap();
    assert_eq!(output.as_type::<Vec<[u8; 3]>>().unwrap(), [[1, 2, 3]]);

    let single = Node::new_leaf(KeyPath::from_keys(["settings", "single"]), Value::U8(1));
    let error = one_or_many::read_with::<()>(&single, |_| Err(unlocated_error())).unwrap_err();
    assert_eq!(error.path(), Some(&single.path));
    assert_original_cause(&error);
    let empty = one_or_many::read_with::<IpAddr>(&node("[]"), |_| panic!("no items")).unwrap();
    assert!(empty.is_empty());
}

#[test]
fn writers_borrow_non_clone_values_and_preserve_state_without_reading_back() {
    let values = [BorrowedOnly(3), BorrowedOnly(7)];
    let mut seen = Vec::new();
    let output = write::list(&values, |value| {
        seen.push(value as *const BorrowedOnly);
        value.0.to_node()
    })
    .unwrap();
    assert_eq!(seen, values.iter().map(|value| value as *const BorrowedOnly).collect::<Vec<_>>());
    assert_eq!(output.as_type::<Vec<u8>>().unwrap(), [3, 7]);
}

#[test]
fn writer_failures_compose_actual_output_positions_once_and_stop_later_calls() {
    let values = KeyValues::new(vec![("same".to_string(), 1u8), ("same".to_string(), 2)]);
    let calls = Cell::new(0);
    let error = key_values::write_with(&values, |value| {
        calls.set(calls.get() + 1);
        if *value == 2 {
            Err(NodeError::invalid_value_with_source(
                &KeyPath::from_keys(["detail"]),
                "cannot write value",
                DomainCause,
            ))
        } else {
            value.to_node()
        }
    })
    .unwrap_err();
    assert_eq!(error.path(), Some(&KeyPath::from_index(1).push_index(1).push_key("detail")));
    assert_eq!(calls.get(), 2);
    assert_original_cause(&error);

    let calls = Cell::new(0);
    let error = write::list(&[1u8, 2, 3], |_| {
        calls.set(calls.get() + 1);
        Err(unlocated_error())
    })
    .unwrap_err();
    assert_eq!(error.path(), Some(&KeyPath::from_index(0)));
    assert_eq!(calls.get(), 1);
    assert_original_cause(&error);

    let relative = KeyPath::from_keys(["detail"]);
    let error = write::option(&Some(1u8), |_| {
        Err(NodeError::invalid_value(&relative, "transparent failure"))
    })
    .unwrap_err();
    assert_eq!(error.path(), Some(&relative));
}

#[test]
fn heterogeneous_writers_prefix_their_positions_and_accept_foreign_values() {
    let address: IpAddr = "::1".parse().unwrap();
    let pair =
        write::tuple2(&("primary".to_string(), address), String::to_node, write_address).unwrap();
    assert_eq!(pair.as_type::<(String, String)>().unwrap(), ("primary".into(), "::1".into()));

    let triple =
        write::tuple3(&(1u8, true, address), u8::to_node, bool::to_node, write_address).unwrap();
    assert_eq!(triple.as_type::<(u8, bool, String)>().unwrap(), (1, true, "::1".into()));
    let error = write::tuple4(
        &(1u8, true, address, 7u8),
        u8::to_node,
        bool::to_node,
        write_address,
        |_| Err(unlocated_error()),
    )
    .unwrap_err();
    assert_eq!(error.path(), Some(&KeyPath::from_index(3)));
    assert_original_cause(&error);
}

#[test]
fn helpers_keep_raw_output_paths_until_destination_anchoring() {
    let raw = Node::new_leaf(KeyPath::from_keys(["original"]), Value::U8(7));
    let output = write::list(&[raw], Node::to_node).unwrap();
    assert!(output.path.is_empty());
    assert_eq!(output.req_node("[0]").unwrap().path, KeyPath::from_keys(["original"]));
    let mut destination = Node::empty_map_at(KeyPath::from_keys(["settings"]));
    destination.set_node("values", output).unwrap();
    assert_eq!(
        destination.req_node("values[0]").unwrap().path,
        KeyPath::from_keys(["settings", "values"]).push_index(0)
    );
}

#[test]
fn kit_descriptions_keep_their_existing_labels_and_fallbacks() {
    let value = Desc::plain("foreign value").nullable();
    assert_eq!(
        key_values::description(value.clone()).type_label(),
        "key_values[string → foreign value | null]"
    );
    assert_eq!(one_or_many::description(value).type_label(), "foreign value | null…");
    let unlabelled = Desc::structure(Vec::new());
    assert_eq!(
        key_values::description(unlabelled.clone()).type_label(),
        "key_values[string → value]"
    );
    assert_eq!(one_or_many::description(unlabelled).type_label(), "value…");
    assert_eq!(KeyValues::<u8>::describe().type_label(), "key_values[string → u8]");
    assert_eq!(OneOrMany::<u8>::describe().type_label(), "u8…");
}

// ---------------------------------------------------------------------------------------------- //

#[derive(Debug)]
struct BorrowedOnly(u8);

#[derive(Debug)]
struct DomainCause;

impl std::fmt::Display for DomainCause {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("original domain cause")
    }
}

impl Error for DomainCause {}

fn unlocated_error() -> NodeError {
    NodeError::with_context("conversion failed", DomainCause)
}

fn assert_original_cause(error: &NodeError) {
    let mut current: &dyn Error = error;
    loop {
        if current.is::<DomainCause>() {
            return;
        }
        current = current.source().expect("the original typed domain cause must be retained");
    }
}

fn node(source: &str) -> Node {
    Node::parse_str(source, Format::Rhai).unwrap()
}

fn read_address(node: &Node) -> Result<IpAddr, NodeError> {
    node.as_type::<String>()?.parse().map_err(|error| {
        NodeError::invalid_value_with_source(&node.path, "invalid IP address", error)
    })
}

fn write_address(address: &IpAddr) -> Result<Node, NodeError> {
    address.to_string().to_node()
}
