//! Checks ordered unique entries, checked construction, and source-aware decoding failures.

use std::cell::Cell;
use std::error::Error;
use std::iter::FusedIterator;
use std::num::ParseIntError;

use scry::kit::key_values::{self, DuplicateKeyError};
use scry::kit::KeyValues;
use scry::node::Format;
use scry::{Config, KeyPath, Node, NodeError, ToNode};

// ---------------------------------------------------------------------------------------------- //

#[test]
fn empty_construction_and_borrowed_iteration_need_no_value_traits() {
    struct Value;

    const EMPTY: KeyValues<Value> = KeyValues::new();
    assert!(EMPTY.is_empty());
    assert!(KeyValues::<Value>::default().is_empty());

    let values = KeyValues::try_from(vec![("primary".to_owned(), Value)]).unwrap();
    let mut iter = values.iter();
    assert_borrowed_iterator(&iter);
    assert_eq!(iter.len(), 1);
    let mut cloned = iter.clone();
    let (key, value) = iter.next().unwrap();
    assert_eq!(key, "primary");
    assert!(std::ptr::eq(value, values.get("primary").unwrap()));
    assert_eq!(cloned.next_back().unwrap().0, "primary");
    assert_eq!(iter.len(), 0);
    assert!(iter.next().is_none());
    assert!(iter.next_back().is_none());

    let borrowed = (&values).into_iter();
    assert_borrowed_iterator(&borrowed);
    assert_eq!(borrowed.collect::<Vec<_>>().len(), 1);
}

#[test]
fn checked_construction_preserves_supplied_order_and_owned_values() {
    let input = entries([("zeta", 7), ("alpha", 9), ("middle", 11)]);
    let values = KeyValues::try_from(input.clone()).unwrap();
    assert_eq!(values.entries(), input);
    assert_eq!(values.len(), 3);
    assert_eq!(values.keys().collect::<Vec<_>>(), ["zeta", "alpha", "middle"]);
    assert_eq!(values.get("alpha"), Some(&9));
    assert!(values.contains_key("alpha"));
    assert_eq!(values.get("missing"), None);
    assert!(!values.contains_key("missing"));

    let collected = KeyValues::try_from_iter(input.clone().into_iter().filter(|_| true)).unwrap();
    assert_eq!(collected, values);
    assert_eq!(values.clone().into_entries(), input);
    assert_eq!(values.into_iter().collect::<Vec<_>>(), input);
}

#[test]
fn borrowed_iteration_is_consistent_and_supports_both_ends() {
    let values = KeyValues::try_from(entries([("first", 1), ("middle", 2), ("last", 3)])).unwrap();
    assert_eq!(values.iter().collect::<Vec<_>>(), (&values).into_iter().collect::<Vec<_>>());

    let mut iter = values.iter();
    assert_eq!(iter.size_hint(), (3, Some(3)));
    assert_eq!(iter.next_back(), Some(("last", &3)));
    assert_eq!(iter.next(), Some(("first", &1)));
    assert_eq!(iter.len(), 1);
    assert_eq!(iter.next_back(), Some(("middle", &2)));
    assert_eq!(iter.next(), None);
    assert_eq!(iter.next_back(), None);
}

#[test]
fn all_checked_construction_paths_report_the_first_and_duplicate_positions() {
    for input in [
        entries([("same", 1), ("same", 1)]),
        entries([("same", 1), ("other", 2), ("same", 3)]),
        entries([("", 1), ("other", 2), ("", 3)]),
    ] {
        let expected_key = input[0].0.clone();
        let expected_index = input.len() - 1;
        let error = KeyValues::try_from(input.clone()).unwrap_err();
        assert_duplicate(&error, &expected_key, 0, expected_index);
        let iter_error = KeyValues::try_from_iter(input).unwrap_err();
        assert_duplicate(&iter_error, &expected_key, 0, expected_index);
    }

    let error = KeyValues::try_from(entries([("other", 1), ("same", 2), ("same", 3)])).unwrap_err();
    assert_duplicate(&error, "same", 1, 2);
}

#[test]
fn iterator_construction_stops_requesting_entries_at_the_first_duplicate() {
    let requests = Cell::new(0);
    let input = entries([("same", 1), ("same", 2), ("later", 3)])
        .into_iter()
        .inspect(|_| requests.set(requests.get() + 1));

    let error = KeyValues::try_from_iter(input).unwrap_err();

    assert_duplicate(&error, "same", 0, 1);
    assert_eq!(requests.get(), 2);
}

#[test]
fn iterator_construction_rejects_duplicates_from_an_unbounded_iterator() {
    let error = KeyValues::try_from_iter(std::iter::repeat(("same".to_owned(), 1))).unwrap_err();

    assert_duplicate(&error, "same", 0, 1);
}

#[test]
fn uniqueness_uses_exact_strings_and_equality_keeps_order_significant() {
    let values = KeyValues::try_from(entries([
        ("", 0),
        ("name", 1),
        ("Name", 2),
        (" name", 3),
        ("name ", 4),
        ("\u{e9}", 5),
        ("e\u{301}", 6),
    ]))
    .unwrap();
    assert_eq!(values.len(), 7);
    assert_eq!(values.get(""), Some(&0));
    assert_eq!(values.get("Name"), Some(&2));

    let reordered = KeyValues::try_from_iter(values.clone().into_iter().rev()).unwrap();
    assert_ne!(values, reordered);
}

#[test]
fn native_and_callback_decoders_reject_duplicate_pair_keys_with_the_same_cause() {
    for source in [
        r#"[["same", 1], ["same", 1]]"#,
        r#"[["same", 1], ["other", 2], ["same", 3]]"#,
        r#"[["", 1], ["other", 2], ["", 3]]"#,
    ] {
        let input = node(source);
        let count = input.as_vec().unwrap().len();
        let key = input.req::<String>("[0][0]").unwrap();
        let native = input.as_type::<KeyValues<u8>>().unwrap_err();
        let callback = key_values::read_with(&input, Node::as_type::<u8>).unwrap_err();
        for error in [&native, &callback] {
            assert_eq!(error.path(), Some(&KeyPath::from_index(count - 1).push_index(0)));
            assert_duplicate(duplicate_cause(error), &key, 0, count - 1);
        }
    }
}

#[test]
fn duplicate_errors_use_the_actual_nested_key_path_and_stop_before_its_value() {
    let input = node(
        r#"#{ settings: #{ "channels.main": [["primary", 1], ["other", 2], ["primary", "bad"], ["later", 4]] } }"#,
    );
    let path = KeyPath::from_keys(["settings", "channels.main"]);
    let channels = input.req_node(&path).unwrap();
    let mut calls = Vec::new();
    let error = key_values::read_with(channels, |value| {
        calls.push(value.path.clone());
        value.as_type::<u8>()
    })
    .unwrap_err();

    assert_eq!(error.path(), Some(&path.push_index(2).push_index(0)));
    assert_duplicate(duplicate_cause(&error), "primary", 0, 2);
    assert_eq!(
        calls,
        [
            path.push_index(0).push_index(1),
            path.push_index(1).push_index(1)
        ]
    );
    assert!(channels.req_node("[2][0]").unwrap().as_leaf().unwrap().is_visited());
    assert!(!channels.req_node("[2][1]").unwrap().as_leaf().unwrap().is_visited());
    assert!(!channels.req_node("[3][0]").unwrap().as_leaf().unwrap().is_visited());
    assert!(!channels.req_node("[3][1]").unwrap().as_leaf().unwrap().is_visited());
}

#[test]
fn earlier_value_failures_keep_precedence_over_later_duplicates() {
    let input = node(r#"#{ entries: [["same", "bad"], ["same", 2]] }"#);
    let entries = input.req_node("entries").unwrap();
    let mut calls = 0;
    let error = key_values::read_with(entries, |value| {
        calls += 1;
        value.as_type::<u8>()
    })
    .unwrap_err();

    assert_eq!(calls, 1);
    assert_eq!(error.path(), Some(&KeyPath::from_keys(["entries"]).push_index(0).push_index(1)));
    assert!(error.source().unwrap().is::<ParseIntError>());
    assert!(!entries.req_node("[1][0]").unwrap().as_leaf().unwrap().is_visited());
    assert!(!entries.req_node("[1][1]").unwrap().as_leaf().unwrap().is_visited());
}

#[test]
fn pair_shape_and_key_validation_happen_before_duplicate_checks_and_value_callbacks() {
    for (source, expected_path, expected_calls) in [
        (r#"[["same", 1], ["same"]]"#, KeyPath::from_index(1), 1),
        (r#"[["same", 1], ["same", 2, 3]]"#, KeyPath::from_index(1), 1),
        (r#"[["same", 1], false]"#, KeyPath::from_index(1), 1),
        (r#"[[#{ invalid: true }, 1]]"#, KeyPath::from_index(0).push_index(0), 0),
        (r#"[[1, 1], ["1", 2]]"#, KeyPath::from_index(0).push_index(0), 0),
    ] {
        let input = node(source);
        let mut calls = 0;
        let error = key_values::read_with(&input, |value| {
            calls += 1;
            value.as_type::<u8>()
        })
        .unwrap_err();
        assert_eq!(error.path(), Some(&expected_path));
        assert!(error.source().is_none());
        assert_eq!(calls, expected_calls);
    }
}

#[test]
fn successful_decoding_preserves_node_order_and_canonical_pair_output() {
    let mut object = Node::empty_map();
    object.set_value("zeta", 7u8).unwrap();
    object.set_value("alpha", 9u8).unwrap();
    object.set_value("middle", 11u8).unwrap();

    for input in [
        object,
        node(r#"[["zeta", 7], ["alpha", 9], ["middle", 11]]"#),
    ] {
        let values: KeyValues<u8> = input.as_type().unwrap();
        assert_eq!(values.keys().collect::<Vec<_>>(), ["zeta", "alpha", "middle"]);
        input.ensure_no_unknown_keys().unwrap();

        let output = values.to_node().unwrap();
        assert_eq!(output.as_vec().unwrap().len(), 3);
        assert_eq!(output.req::<String>("[0][0]").unwrap(), "zeta");
        assert_eq!(output.req::<u8>("[1][1]").unwrap(), 9);
        assert_eq!(output.as_type::<KeyValues<u8>>().unwrap(), values);
    }
}

#[test]
fn successful_callbacks_do_not_consume_values_they_did_not_read() {
    let input = node(r#"[["first", 1], ["second", 2]]"#);
    let values = key_values::read_with(&input, |_| Ok(())).unwrap();
    assert_eq!(values.len(), 2);
    assert!(input.req_node("[0][0]").unwrap().as_leaf().unwrap().is_visited());
    assert!(input.req_node("[1][0]").unwrap().as_leaf().unwrap().is_visited());
    assert!(!input.req_node("[0][1]").unwrap().as_leaf().unwrap().is_visited());
    assert!(!input.req_node("[1][1]").unwrap().as_leaf().unwrap().is_visited());
    assert!(matches!(input.ensure_no_unknown_keys(), Err(NodeError::UnknownKeys { .. })));
}

#[test]
fn empty_objects_and_pairs_remain_valid_and_write_empty_pairs() {
    for input in [node("#{}"), node("[]")] {
        let values = key_values::read_with::<()>(&input, |_| panic!("empty collection")).unwrap();
        assert!(values.is_empty());
        let output = key_values::write_with(&values, |_| panic!("empty collection")).unwrap();
        assert!(output.as_vec().unwrap().is_empty());
    }
}

#[test]
fn empty_default_does_not_make_a_configuration_field_optional() {
    #[derive(Debug, Config)]
    struct Required {
        entries: KeyValues<u8>,
    }

    #[derive(Debug, Config)]
    struct Defaulted {
        #[scry(default = KeyValues::new())]
        entries: KeyValues<u8>,
    }

    let missing = Node::empty_map();
    let error = missing.as_type::<Required>().unwrap_err();
    assert!(matches!(error, NodeError::MissingRequired { .. }));
    assert_eq!(error.path(), Some(&KeyPath::from_keys(["entries"])));
    assert!(missing.as_type::<Defaulted>().unwrap().entries.is_empty());
    assert!(node("#{ entries: [] }").as_type::<Required>().unwrap().entries.is_empty());
}

fn entries<V, const N: usize>(entries: [(&str, V); N]) -> Vec<(String, V)> {
    entries.into_iter().map(|(key, value)| (key.to_owned(), value)).collect()
}

fn assert_borrowed_iterator<'a, V: 'a>(
    _: &(impl ExactSizeIterator<Item = (&'a str, &'a V)> + DoubleEndedIterator + FusedIterator + Clone),
) {
}

fn assert_duplicate(error: &DuplicateKeyError, key: &str, first: usize, duplicate: usize) {
    assert_eq!(error.key(), key);
    assert_eq!(error.first_index(), first);
    assert_eq!(error.duplicate_index(), duplicate);
    assert!(error.to_string().contains("duplicate"));
    assert!(error.to_string().contains(key));
    assert!(error.source().is_none());
}

fn duplicate_cause(error: &NodeError) -> &DuplicateKeyError {
    error.source().unwrap().downcast_ref::<DuplicateKeyError>().unwrap()
}

fn node(source: &str) -> Node {
    Node::parse_str(source, Format::Rhai).unwrap()
}
