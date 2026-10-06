//! Regression tests for output paths and collection conversion causes.

use std::cell::Cell;
use std::error::Error;

use scry::kit::{KeyValues, OneOrMany};
use scry::node::Value;
use scry::{BoxedError, FromNode, KeyPath, Node, NodeError, ToNode};

// ---------------------------------------------------------------------------------------------- //

#[test]
fn collections_prefix_the_failing_element_index() {
    let values = vec![None, Some(FailingValue::new(KeyPath::new()))];
    assert_path(values.to_node(), KeyPath::from_index(1));

    let values = [
        None,
        None,
        Some(FailingValue::new(KeyPath::from_keys(["payload"]))),
    ];
    assert_path(values.to_node(), KeyPath::from_index(2).push_key("payload"));
}

#[test]
fn tuples_prefix_each_position_for_every_supported_arity() {
    assert_path((failure(), ()).to_node(), KeyPath::from_index(0));
    assert_path(((), failure()).to_node(), KeyPath::from_index(1));

    assert_path((failure(), (), ()).to_node(), KeyPath::from_index(0));
    assert_path(((), failure(), ()).to_node(), KeyPath::from_index(1));
    assert_path(((), (), failure()).to_node(), KeyPath::from_index(2));

    assert_path((failure(), (), (), ()).to_node(), KeyPath::from_index(0));
    assert_path(((), failure(), (), ()).to_node(), KeyPath::from_index(1));
    assert_path(((), (), failure(), ()).to_node(), KeyPath::from_index(2));
    assert_path(((), (), (), failure()).to_node(), KeyPath::from_index(3));
}

#[test]
fn nested_output_composes_root_and_nonempty_relative_paths_once() {
    for relative in [
        KeyPath::new(),
        KeyPath::from_keys(["payload.part"]).push_index(2),
    ] {
        let values = vec![Some((
            true,
            OneOrMany::one(FailingValue::new(relative.clone())),
        ))];

        assert_path(
            values.to_node(),
            KeyPath::from_index(0).push_index(1).push_index(0).join(&relative),
        );
    }
}

#[test]
fn transparent_wrappers_preserve_child_paths() {
    let relative = KeyPath::from_keys(["payload"]).push_index(2);
    assert_path(Some(FailingValue::new(relative.clone())).to_node(), relative.clone());

    let mut value = FailingValue::new(relative.clone());
    let shared = &value;
    assert_path(<&FailingValue as ToNode>::to_node(&shared), relative.clone());

    let exclusive = &mut value;
    assert_path(<&mut FailingValue as ToNode>::to_node(&exclusive), relative);
}

#[test]
fn key_values_uses_pair_positions_even_with_duplicate_keys() {
    let relative = KeyPath::from_keys(["detail"]);
    let values = KeyValues::new(vec![
        ("same.key".to_string(), None),
        ("same.key".to_string(), Some(FailingValue::new(relative.clone()))),
    ]);

    assert_path(values.to_node(), KeyPath::from_index(1).push_index(1).join(&relative));
}

#[test]
fn nested_output_retains_the_original_boxed_cause() {
    let failure = FailingValue::new(KeyPath::from_keys(["payload"]));
    let values = vec![(true, OneOrMany::one(Some(&failure)))];

    let error = values.to_node().unwrap_err();

    assert_eq!(
        error.path(),
        Some(&KeyPath::from_index(0).push_index(1).push_index(0).push_key("payload"))
    );
    let cause = error.source().unwrap().downcast_ref::<OutputCause>().unwrap();
    assert_eq!(cause.code, 42);
    assert!(std::ptr::eq(cause, failure.source_address.get()));
}

#[test]
fn a_locationless_failure_gains_a_path_and_retains_its_error_chain() {
    let values = vec![(true, UnlocatedFailure)];

    let error = values.to_node().unwrap_err();

    assert_eq!(error.path(), Some(&KeyPath::from_index(0).push_index(1)));
    let original = error.source().unwrap();
    assert_eq!(original.to_string(), "writer failed");
    assert_eq!(original.source().unwrap().downcast_ref::<OutputCause>().unwrap().code, 17);
}

#[test]
fn a_sourcefree_failure_gains_its_nested_output_location() {
    let values = [vec![((), SourcefreeFailure)]];

    let error = values.to_node().unwrap_err();

    assert_eq!(error.path(), Some(&KeyPath::from_index(0).push_index(0).push_index(1)));
    let original = error.source().unwrap();
    assert_eq!(original.to_string(), "cannot serialize value");
    assert!(original.source().is_none());
}

#[test]
fn successful_kit_wrappers_keep_their_canonical_array_shapes() {
    let single = OneOrMany::one(7u8).to_node().unwrap();
    assert_eq!(single.as_vec().unwrap().len(), 1);
    assert_eq!(single.req::<u8>("[0]").unwrap(), 7);
    assert!(OneOrMany::<u8>::default().to_node().unwrap().as_vec().unwrap().is_empty());

    let pairs = KeyValues::new(vec![("same".to_string(), 3u8), ("same".to_string(), 5u8)])
        .to_node()
        .unwrap();
    assert_eq!(pairs.as_vec().unwrap().len(), 2);
    for (index, expected) in [3u8, 5].into_iter().enumerate() {
        assert_eq!(pairs.req_node(KeyPath::from_index(index)).unwrap().as_vec().unwrap().len(), 2);
        assert_eq!(pairs.req::<String>(KeyPath::from_index(index).push_index(0)).unwrap(), "same");
        assert_eq!(pairs.req::<u8>(KeyPath::from_index(index).push_index(1)).unwrap(), expected);
    }
}

#[test]
fn successful_output_preserves_raw_nodes_until_destination_anchoring() {
    let original = KeyPath::from_keys(["original"]);
    let raw = Node::new_leaf(original.clone(), Value::String("kept".to_string()));
    let values = vec![Some((raw, [1u8, 2]))];

    let output = values.to_node().unwrap();

    assert!(output.path.is_empty());
    assert!(output.req_node("[0]").unwrap().path.is_empty());
    assert_eq!(output.req_node("[0][0]").unwrap().path, original);
    assert!(output.req_node("[0][1][1]").unwrap().path.is_empty());

    let mut destination = Node::empty_map_at(KeyPath::from_keys(["config"]));
    destination.set_node("output", output).unwrap();

    assert_eq!(
        destination.req_node("output[0][0]").unwrap().path,
        KeyPath::from_keys(["config", "output"]).push_index(0).push_index(0)
    );
    assert_eq!(
        destination.req_node("output[0][1][1]").unwrap().path,
        KeyPath::from_keys(["config", "output"]).push_index(0).push_index(1).push_index(1)
    );
    assert_eq!(destination.req::<String>("output[0][0]").unwrap(), "kept");
    assert_eq!(destination.req::<u8>("output[0][1][1]").unwrap(), 2);
}

#[test]
fn key_values_map_input_attaches_the_absolute_value_path() {
    let mut input = Node::empty_map_at(KeyPath::from_keys(["settings"]));
    input.set_node(KeyPath::from_keys(["literal.key"]), 1u8.to_node().unwrap()).unwrap();

    let error = KeyValues::<UnlocatedInputFailure>::from_node(&input).unwrap_err();

    assert_eq!(error.path(), Some(&KeyPath::from_keys(["settings", "literal.key"])));
    assert_input_cause(&error);
}

#[test]
fn key_values_pair_input_attaches_the_absolute_value_index() {
    let pairs = vec![("first".to_string(), 0u8), ("second".to_string(), 1u8)];
    let mut input = Node::empty_map_at(KeyPath::from_keys(["settings"]));
    input.set_node("entries", pairs.to_node().unwrap()).unwrap();

    let error = KeyValues::<UnlocatedInputFailure>::from_node(input.req_node("entries").unwrap())
        .unwrap_err();

    assert_eq!(
        error.path(),
        Some(&KeyPath::from_keys(["settings", "entries"]).push_index(1).push_index(1))
    );
    assert_input_cause(&error);
}

#[test]
fn one_or_many_single_input_preserves_the_absolute_value_path() {
    let input = Node::new_leaf(KeyPath::from_keys(["settings", "single"]), Value::U8(1));

    let error = OneOrMany::<UnlocatedInputFailure>::from_node(&input).unwrap_err();

    assert_eq!(error.path(), Some(&KeyPath::from_keys(["settings", "single"])));
    assert_input_cause(&error);
}

// ---------------------------------------------------------------------------------------------- //

struct FailingValue {
    path: KeyPath,
    source_address: Cell<*const OutputCause>,
}

impl FailingValue {
    fn new(path: KeyPath) -> Self {
        Self {
            path,
            source_address: Cell::new(std::ptr::null()),
        }
    }
}

impl ToNode for FailingValue {
    fn to_node(&self) -> Result<Node, NodeError> {
        let source: BoxedError = Box::new(OutputCause { code: 42 });
        self.source_address.set(source.downcast_ref::<OutputCause>().unwrap());
        Err(NodeError::invalid_value_with_source(&self.path, "cannot serialize value", source))
    }
}

struct UnlocatedFailure;

impl ToNode for UnlocatedFailure {
    fn to_node(&self) -> Result<Node, NodeError> {
        Err(NodeError::with_context("writer failed", OutputCause { code: 17 }))
    }
}

struct SourcefreeFailure;

impl ToNode for SourcefreeFailure {
    fn to_node(&self) -> Result<Node, NodeError> {
        Err(NodeError::new("cannot serialize value"))
    }
}

#[derive(Debug, thiserror::Error)]
#[error("original output failure {code}")]
struct OutputCause {
    code: u8,
}

#[derive(Debug)]
struct UnlocatedInputFailure;

impl FromNode for UnlocatedInputFailure {
    fn from_node(node: &Node) -> Result<Self, NodeError> {
        if node.as_type::<u8>()? == 0 {
            Ok(Self)
        } else {
            Err(NodeError::with_context("cannot parse value", InputCause { code: 23 }))
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("original input failure {code}")]
struct InputCause {
    code: u8,
}

fn failure() -> FailingValue {
    FailingValue::new(KeyPath::new())
}

fn assert_path(result: Result<Node, NodeError>, expected: KeyPath) {
    let error = result.unwrap_err();
    assert_eq!(error.path(), Some(&expected), "unexpected output error: {error:?}");
}

fn assert_input_cause(error: &NodeError) {
    let original = error.source().unwrap();
    assert_eq!(original.to_string(), "cannot parse value");
    assert_eq!(original.source().unwrap().downcast_ref::<InputCause>().unwrap().code, 23);
}
