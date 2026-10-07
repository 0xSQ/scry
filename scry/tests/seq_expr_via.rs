use std::error::Error;
use std::num::{ParseFloatError, ParseIntError, TryFromIntError};

use scry::desc::{DescKind, FieldDesc};
use scry::kit::seq_expr::{
    EvalError, EvalErrorKind, ExprBuildError, ExprBuildErrorKind, IntSequence, ParseErrorKind,
    ProfileErrorKind, RealSequence, MAX_VALUES,
};
use scry::node::{Format, Kind, Value};
use scry::{
    Config, Desc, Describe, DescribeVia, FromNode, FromNodeVia, KeyPath, Node, NodeError, ToNode,
    ToNodeVia,
};

// ---------------------------------------------------------------------------------------------- //

#[test]
fn equivalent_expression_and_array_inputs_emit_canonical_numeric_arrays() {
    for source in [r#""2,1,2""#, r#"[2, "1", 2]"#] {
        let input = node(source);
        let values = IntSequence::from_node(&input).unwrap();
        assert_eq!(values, [2, 1, 2]);
        input.ensure_no_unknown_keys().unwrap();
        let output = IntSequence::to_node(&values).unwrap();
        assert_numeric_array(&output);
        assert_eq!(output.as_type::<Vec<i64>>().unwrap(), values);
        assert_eq!(IntSequence::from_node(&output).unwrap(), values);
    }

    for source in [r#""[0..0.3]:0.1,0.1""#, r#"[0.0, "0.1", 0.2, 0.3, 0.1]"#] {
        let input = node(source);
        let values = RealSequence::from_node(&input).unwrap();
        assert_eq!(values, [0.0, 0.1, 0.2, 0.3, 0.1]);
        input.ensure_no_unknown_keys().unwrap();
        let output = RealSequence::to_node(&values).unwrap();
        assert_numeric_array(&output);
        assert_eq!(output.as_type::<Vec<f64>>().unwrap(), values);
        assert_eq!(RealSequence::from_node(&output).unwrap(), values);
    }
}

#[test]
fn shape_dispatch_does_not_expand_array_entries_or_treat_missing_values_as_empty() {
    for source in ["()", "7", "1.5", "true", "#{}"] {
        let input = node(source);
        assert!(
            matches!(IntSequence::from_node(&input), Err(NodeError::TypeMismatch { .. })),
            "{source}"
        );
        assert!(
            matches!(RealSequence::from_node(&input), Err(NodeError::TypeMismatch { .. })),
            "{source}"
        );
    }
    assert_eq!(IntSequence::from_node(&node(r#""7""#)).unwrap(), [7]);
    assert_eq!(RealSequence::from_node(&node(r#""7""#)).unwrap(), [7.0]);
    assert!(IntSequence::from_node(&node(r#"["1..3"]"#)).is_err());
    assert!(RealSequence::from_node(&node(r#"["[0..1]/2"]"#)).is_err());
    assert!(IntSequence::from_node(&node("[1.0]")).is_err());
    assert_eq!(RealSequence::from_node(&node("[1]")).unwrap(), [1.0]);

    assert!(IntSequence::from_node(&node("[]")).unwrap().is_empty());
    assert!(RealSequence::from_node(&node("[]")).unwrap().is_empty());
    assert!(IntSequence::from_node(&node(r#""3..3""#)).unwrap().is_empty());
    assert!(RealSequence::from_node(&node(r#""1..1:1""#)).unwrap().is_empty());
    for error in [
        IntSequence::from_node(&node(r#""""#)).unwrap_err(),
        RealSequence::from_node(&node(r#""""#)).unwrap_err(),
    ] {
        assert_eq!(
            cause::<ExprBuildError>(&error).kind(),
            ExprBuildErrorKind::Parse(&ParseErrorKind::EmptyInput),
        );
    }
}

#[test]
fn string_failures_keep_build_or_evaluation_causes_and_full_nested_paths() {
    let expected = KeyPath::from_keys(["scope", "iterations.groups"]).push_index(1);
    for (source, kind) in [
        ("1,,2", ExprBuildErrorKind::Parse(&ParseErrorKind::EmptyTerm)),
        ("1.5", ExprBuildErrorKind::Profile(&ProfileErrorKind::RealLiteralUnsupportedForInteger)),
    ] {
        let input = integer_group_input(source);
        let error = IntegerGroups::from_node(input.req_node("scope").unwrap()).unwrap_err();
        assert_eq!(error.path(), Some(&expected));
        let build = cause::<ExprBuildError>(&error);
        assert_eq!(build.kind(), kind);
        assert_eq!(build.source_text(), source);
    }
    for (source, kind) in [
        ("N-1", EvalErrorKind::MissingFiniteContext),
        ("..", EvalErrorKind::MissingOpenStartContext),
        ("9223372036854775808", EvalErrorKind::IntegerOutOfRange),
    ] {
        let input = integer_group_input(source);
        let error = IntegerGroups::from_node(input.req_node("scope").unwrap()).unwrap_err();
        assert_eq!(error.path(), Some(&expected));
        let evaluation = cause::<EvalError>(&error);
        assert_eq!(evaluation.kind(), &kind);
        assert_eq!(evaluation.source_text(), source);
        assert_eq!(evaluation.term_index(), Some(0));
    }

    let path = KeyPath::from_keys(["scope", "real.sweep"]);
    let input = Node::new_leaf(path.clone(), Value::String("0..1".to_string()));
    let error = RealSequence::from_node(&input).unwrap_err();
    assert_eq!(error.path(), Some(&path));
    assert_eq!(
        cause::<ExprBuildError>(&error).kind(),
        ExprBuildErrorKind::Profile(&ProfileErrorKind::MissingRealSampler),
    );
    let input = Node::new_leaf(path.clone(), Value::String("1e4000".to_string()));
    let error = RealSequence::from_node(&input).unwrap_err();
    assert_eq!(error.path(), Some(&path));
    assert_eq!(cause::<EvalError>(&error).kind(), &EvalErrorKind::RealValueNotFinite);
}

#[test]
fn array_conversion_keeps_native_causes_and_stops_after_the_first_failure() {
    let input = node(r#"[1, "bad", 3]"#);
    let error = IntSequence::from_node(&input).unwrap_err();
    assert_eq!(error.path(), Some(&KeyPath::from_index(1)));
    assert!(error.source().unwrap().is::<ParseIntError>());
    assert!(!is_visited(&input.as_vec().unwrap()[2]));

    let input = node(r#"[1, "bad", 3]"#);
    let error = RealSequence::from_node(&input).unwrap_err();
    assert_eq!(error.path(), Some(&KeyPath::from_index(1)));
    assert!(error.source().unwrap().is::<ParseFloatError>());
    assert!(!is_visited(&input.as_vec().unwrap()[2]));

    let path = KeyPath::from_keys(["scope", "integers"]);
    let input = Node::new_vec(
        path.clone(),
        vec![
            Node::new_leaf(path.push_index(0), Value::I64(1)),
            Node::new_leaf(path.push_index(1), Value::U64(u64::MAX)),
        ],
    );
    let error = IntSequence::from_node(&input).unwrap_err();
    assert_eq!(error.path(), Some(&path.push_index(1)));
    assert!(error.source().unwrap().is::<TryFromIntError>());
}

#[test]
fn real_arrays_require_finite_values_and_preserve_native_signed_zero() {
    let values = vec![-0.0_f64, 0.0, f64::MAX];
    let input = values.to_node().unwrap();
    let decoded = RealSequence::from_node(&input).unwrap();
    assert_eq!(
        decoded.iter().map(|value| value.to_bits()).collect::<Vec<_>>(),
        values.iter().map(|value| value.to_bits()).collect::<Vec<_>>()
    );
    input.ensure_no_unknown_keys().unwrap();
    let output = RealSequence::to_node(&decoded).unwrap();
    let roundtrip = RealSequence::from_node(&output).unwrap();
    assert_eq!(roundtrip[0].to_bits(), (-0.0_f64).to_bits());
    assert_eq!(roundtrip[1].to_bits(), 0.0_f64.to_bits());
    assert_eq!(
        RealSequence::from_node(&node(r#""-0.0""#)).unwrap()[0].to_bits(),
        0.0_f64.to_bits()
    );

    for spelling in ["NaN", "inf", "-inf"] {
        let input = node(&format!(r#"[1, "{spelling}", 3]"#));
        let error = RealSequence::from_node(&input).unwrap_err();
        assert!(matches!(error, NodeError::InvalidValue { .. }));
        assert_eq!(error.path(), Some(&KeyPath::from_index(1)));
        assert!(error.source().is_none());
        assert!(!is_visited(&input.as_vec().unwrap()[2]));
    }
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let input = Node::new_vec(
            KeyPath::new(),
            vec![Node::new_leaf(KeyPath::from_index(0), Value::F64(value))],
        );
        let error = RealSequence::from_node(&input).unwrap_err();
        assert_eq!(error.path(), Some(&KeyPath::from_index(0)));
        assert!(error.source().is_none());

        let error = RealSequence::to_node(&vec![1.0, value]).unwrap_err();
        assert_eq!(error.path(), Some(&KeyPath::from_index(1)));
        assert!(error.source().is_none());
    }
}

#[test]
fn count_caps_cover_expansion_and_array_preflight_at_the_whole_value_path() {
    for error in [
        IntSequence::from_node(&node(r#""0..1000001""#)).unwrap_err(),
        RealSequence::from_node(&node(r#""[0..1000000]:1""#)).unwrap_err(),
    ] {
        assert_eq!(error.path(), Some(&KeyPath::new()));
        assert_eq!(
            cause::<EvalError>(&error).kind(),
            &EvalErrorKind::OutputLimitExceeded { limit: MAX_VALUES }
        );
    }

    let path = KeyPath::from_keys(["scope", "values"]);
    // Shared visit state keeps this large boundary fixture from allocating a separate leaf cell.
    let leaf = Node::new_leaf(KeyPath::new(), Value::I64(1));
    let mut input = Node::new_vec(path.clone(), vec![leaf.clone(); MAX_VALUES + 1]);
    for error in [
        IntSequence::from_node(&input).unwrap_err(),
        RealSequence::from_node(&input).unwrap_err(),
    ] {
        assert!(matches!(error, NodeError::InvalidValue { .. }));
        assert_eq!(error.path(), Some(&path));
        assert!(error.source().is_none());
    }
    assert!(!is_visited(&leaf));

    let Kind::Vec(children) = &mut input.kind else {
        unreachable!();
    };
    children.pop();
    let integers = IntSequence::from_node(&input).unwrap();
    assert_eq!(integers.len(), MAX_VALUES);
    assert_eq!(integers.last(), Some(&1));
    let reals = RealSequence::from_node(&input).unwrap();
    assert_eq!(reals.len(), MAX_VALUES);
    assert_eq!(reals.last(), Some(&1.0));
}

#[test]
fn output_count_boundaries_match_accepted_input_boundaries() {
    // Each emitted tree is dropped before creating the next large boundary fixture.
    assert_output_boundary::<i64, IntSequence>(1);
    assert_output_boundary::<f64, RealSequence>(1.0);
}

#[test]
fn missing_null_and_explicit_defaults_keep_the_standard_field_rules() {
    let error = Sweeps::from_node(&Node::empty_map()).unwrap_err();
    assert!(
        matches!(error, NodeError::MissingRequired { path } if path == KeyPath::from_keys(["iterations.values"]))
    );
    let input = node(r#"#{ "iterations.values": "3..5" }"#);
    let config = Sweeps::from_node(&input).unwrap();
    assert_eq!(config.values, [3, 4]);
    assert!(config.optional.is_none());
    assert_eq!(config.fallback, [7, 8]);
    input.ensure_no_unknown_keys().unwrap();
    let output = config.to_node().unwrap();
    assert_eq!(output.req::<Option<Vec<f64>>>("optional").unwrap(), None);

    let input = node(r#"#{ "iterations.values": [], optional: (), fallback: "2,1,2" }"#);
    let config = Sweeps::from_node(&input).unwrap();
    assert!(config.values.is_empty());
    assert!(config.optional.is_none());
    assert_eq!(config.fallback, [2, 1, 2]);
    input.ensure_no_unknown_keys().unwrap();

    for (source, key) in [
        (r#"#{ "iterations.values": () }"#, "iterations.values"),
        (r#"#{ "iterations.values": [], fallback: () }"#, "fallback"),
        (r#"#{ "iterations.values": [], fallback: "N-1" }"#, "fallback"),
        (r#"#{ "iterations.values": [], optional: "0..1" }"#, "optional"),
    ] {
        let error = Sweeps::from_node(&node(source)).unwrap_err();
        assert_eq!(error.path(), Some(&KeyPath::from_keys([key])), "{source}");
    }
}

#[test]
fn container_policies_adapt_each_complete_sequence_and_roundtrip_nulls() {
    let input = node(r#"#{ "iterations.groups": ["1..3", [7,8], "3..3"] }"#);
    let config = IntegerGroups::from_node(&input).unwrap();
    assert_eq!(config.groups, [vec![1, 2], vec![7, 8], vec![]]);
    input.ensure_no_unknown_keys().unwrap();
    let output = config.to_node().unwrap();
    assert_eq!(
        output.req::<Vec<Vec<i64>>>(KeyPath::from_keys(["iterations.groups"])).unwrap(),
        config.groups
    );

    let input = node(r#"#{ "sweeps.values": [(), "[0..1]/2", [2,"3"]] }"#);
    let config = RealGroups::from_node(&input).unwrap();
    assert_eq!(config.groups, [None, Some(vec![0.0, 0.5, 1.0]), Some(vec![2.0, 3.0])]);
    input.ensure_no_unknown_keys().unwrap();
    let output = config.to_node().unwrap();
    assert_eq!(RealGroups::from_node(&output).unwrap().groups, config.groups);
    assert_eq!(
        output
            .req::<Option<Vec<f64>>>(KeyPath::from_keys(["sweeps.values"]).push_index(0))
            .unwrap(),
        None
    );

    let optional = node("()");
    assert_eq!(
        <Option<RealSequence> as FromNodeVia<Option<Vec<f64>>>>::from_node(&optional).unwrap(),
        None
    );
    optional.ensure_no_unknown_keys().unwrap();
}

#[test]
fn nested_finiteness_and_count_failures_gain_each_output_prefix_once() {
    let input = node(r#"#{ scope: #{ "sweeps.values": [(), [1,"NaN",3]] } }"#);
    let error = RealGroups::from_node(input.req_node("scope").unwrap()).unwrap_err();
    assert_eq!(
        error.path(),
        Some(&KeyPath::from_keys(["scope", "sweeps.values"]).push_index(1).push_index(1))
    );
    assert!(error.source().is_none());

    let output = RealGroups {
        groups: vec![None, Some(vec![1.0, f64::NAN])],
    };
    let error = output.to_node().unwrap_err();
    assert_eq!(
        error.path(),
        Some(&KeyPath::from_keys(["sweeps.values"]).push_index(1).push_index(1))
    );
    assert!(error.source().is_none());

    let output = RealGroups {
        groups: vec![Some(vec![0.0; MAX_VALUES + 1])],
    };
    let error = output.to_node().unwrap_err();
    assert_eq!(error.path(), Some(&KeyPath::from_keys(["sweeps.values"]).push_index(0)));
    assert!(error.source().is_none());
}

#[test]
fn descriptions_expose_both_forms_without_claiming_an_array_shape() {
    for (description, hint) in [
        (IntSequence::describe(), "integer sequence expression string or integer array"),
        (RealSequence::describe(), "real sequence expression string or real array"),
    ] {
        assert!(!description.nullable);
        assert!(matches!(&description.kind, DescKind::Plain { .. }));
        assert_eq!(description.type_label(), hint);
        assert!(!description.doc.is_empty());
    }
    let description = Sweeps::describe();
    let values = field(&description, "iterations.values");
    assert!(!values.optional);
    assert!(!values.value.nullable);
    let optional = field(&description, "optional");
    assert!(optional.optional);
    assert!(optional.value.nullable);
    let fallback = field(&description, "fallback");
    assert!(fallback.optional);
    assert!(fallback.default_display.is_none());
    description.validate_path(r#"["iterations.values"]"#).unwrap();
    assert!(description.validate_path(r#"["iterations.values"][0]"#).is_err());

    let description = RealGroups::describe();
    let DescKind::List { item } = &field(&description, "sweeps.values").value.kind else {
        panic!("outer policy composition must retain its list shape");
    };
    assert!(item.nullable);
    assert!(matches!(&item.kind, DescKind::Plain { .. }));
    description.validate_path(r#"["sweeps.values"][0]"#).unwrap();
    assert!(description.validate_path(r#"["sweeps.values"][0][0]"#).is_err());
}

// ---------------------------------------------------------------------------------------------- //

#[derive(Debug, Config, ToNode)]
struct Sweeps {
    #[scry(rename = "iterations.values", via(IntSequence))]
    values: Vec<i64>,
    #[scry(via(Option<RealSequence>))]
    optional: Option<Vec<f64>>,
    #[scry(default = vec![7, 8], via(IntSequence))]
    fallback: Vec<i64>,
}

#[derive(Debug, Config, ToNode)]
struct IntegerGroups {
    #[scry(rename = "iterations.groups", via(Vec<IntSequence>))]
    groups: Vec<Vec<i64>>,
}

#[derive(Debug, Config, ToNode)]
struct RealGroups {
    #[scry(rename = "sweeps.values", via(Vec<Option<RealSequence>>))]
    groups: Vec<Option<Vec<f64>>>,
}

fn assert_output_boundary<T: Clone, A: ToNodeVia<Vec<T>>>(value: T) {
    let mut values = vec![value; MAX_VALUES];
    let output = A::to_node(&values).unwrap();
    assert_eq!(output.as_vec().unwrap().len(), MAX_VALUES);
    drop(output);
    values.push(values[0].clone());
    let error = A::to_node(&values).unwrap_err();
    assert!(matches!(error, NodeError::InvalidValue { .. }));
    assert_eq!(error.path(), Some(&KeyPath::new()));
    assert!(error.source().is_none());
}

fn integer_group_input(source: &str) -> Node {
    node(&format!(r#"#{{ scope: #{{ "iterations.groups": ["1..3", "{source}"] }} }}"#))
}

fn assert_numeric_array(node: &Node) {
    assert!(node.as_vec().unwrap().iter().all(|child| {
        matches!(&child.kind, Kind::Leaf(leaf) if !matches!(&leaf.value, Value::String(_)))
    }));
}

fn is_visited(node: &Node) -> bool {
    let Kind::Leaf(leaf) = &node.kind else {
        panic!("expected a leaf");
    };
    leaf.is_visited()
}

fn cause<E: Error + 'static>(error: &NodeError) -> &E {
    error
        .source()
        .and_then(|source| source.downcast_ref::<E>())
        .expect("expected concrete domain cause")
}

fn field<'a>(description: &'a Desc, name: &str) -> &'a FieldDesc {
    let DescKind::Struct { fields } = &description.kind else {
        panic!("expected a struct description");
    };
    fields.iter().find(|field| field.name == name).unwrap()
}

fn node(source: &str) -> Node {
    Node::parse_str(source, Format::Rhai).unwrap()
}
