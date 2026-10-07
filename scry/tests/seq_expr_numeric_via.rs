use std::error::Error;
use std::num::{ParseFloatError, TryFromIntError};

use scry::desc::DescKind;
use scry::kit::seq_expr::{EvalError, EvalErrorKind, IntSequence, RealSequence, MAX_VALUES};
use scry::node::{Format, Kind, Value};
use scry::{
    Config, DescribeVia, FromNode, FromNodeVia, KeyPath, Node, NodeError, ToNode, ToNodeVia,
};

// ---------------------------------------------------------------------------------------------- //

#[test]
fn every_integer_target_accepts_its_full_range_and_roundtrips_native_arrays() {
    macro_rules! check_target {
        ($($target:ty),+ $(,)?) => {
            $(
                let values = vec![<$target>::MIN, 0, <$target>::MAX];
                let expression = format!("{},0,{}", <$target>::MIN, <$target>::MAX);
                let input = expression.to_node().unwrap();
                let expanded = <IntSequence as FromNodeVia<Vec<$target>>>::from_node(&input)
                    .unwrap();
                assert_eq!(expanded, values, "{} expression", stringify!($target));
                input.ensure_no_unknown_keys().unwrap();

                let array = values.to_node().unwrap();
                let decoded = <IntSequence as FromNodeVia<Vec<$target>>>::from_node(&array)
                    .unwrap();
                assert_eq!(decoded, values, "{} array", stringify!($target));
                array.ensure_no_unknown_keys().unwrap();
                let output = <IntSequence as ToNodeVia<Vec<$target>>>::to_node(&decoded).unwrap();
                assert_eq!(output.as_type::<Vec<$target>>().unwrap(), values);
                assert_eq!(
                    <IntSequence as FromNodeVia<Vec<$target>>>::from_node(&output).unwrap(),
                    values,
                );

                let description = <IntSequence as DescribeVia<Vec<$target>>>::describe();
                assert_eq!(description.type_label(),
                    "integer sequence expression string or integer array");
                assert!(matches!(description.kind, DescKind::Plain { .. }));
                assert!(description.doc.contains(stringify!($target)));
                assert!(description.doc.contains(&<$target>::MIN.to_string()));
                assert!(description.doc.contains(&<$target>::MAX.to_string()));
            )+
        };
    }
    check_target!(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);
}

#[test]
fn each_integer_target_reports_its_own_overflow_and_preserves_native_array_causes() {
    macro_rules! check_target {
        ($($target:ty),+ $(,)?) => {
            $(
                let path = KeyPath::from_keys(["scope", "values"]);
                for outside in [<$target>::MIN as i128 - 1, <$target>::MAX as i128 + 1] {
                    let source = outside.to_string();
                    let input = Node::new_leaf(path.clone(), Value::String(source.clone()));
                    let error = <IntSequence as FromNodeVia<Vec<$target>>>::from_node(&input)
                        .unwrap_err();
                    assert_eq!(error.path(), Some(&path));
                    let evaluation = cause::<EvalError>(&error);
                    assert_eq!(evaluation.kind(), &EvalErrorKind::IntegerOutOfRange {
                        target_type: stringify!($target),
                    });
                    assert_eq!(evaluation.source_text(), source);
                    assert_eq!(evaluation.term_index(), Some(0));

                    let input = Node::new_vec(path.clone(), vec![Node::new_leaf(
                        path.push_index(0),
                        Value::String(source),
                    )]);
                    let error = <IntSequence as FromNodeVia<Vec<$target>>>::from_node(&input)
                        .unwrap_err();
                    assert_eq!(error.path(), Some(&path.push_index(0)));
                    assert!(error.source().unwrap().is::<std::num::ParseIntError>());
                }
            )+
        };
    }
    check_target!(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);
}

#[test]
fn unsigned_expressions_can_use_exclusive_bounds_beyond_the_target_range() {
    let input = "18446744073709551614..18446744073709551616".to_node().unwrap();
    let values = <IntSequence as FromNodeVia<Vec<u64>>>::from_node(&input).unwrap();
    assert_eq!(values, [u64::MAX - 1, u64::MAX]);
    let output = <IntSequence as ToNodeVia<Vec<u64>>>::to_node(&values).unwrap();
    assert_eq!(output.as_type::<Vec<u64>>().unwrap(), values);

    let input = "18446744073709551616..18446744073709551616".to_node().unwrap();
    assert!(<IntSequence as FromNodeVia<Vec<u64>>>::from_node(&input).unwrap().is_empty());
    let input = "[18446744073709551615..18446744073709551616]".to_node().unwrap();
    let error = <IntSequence as FromNodeVia<Vec<u64>>>::from_node(&input).unwrap_err();
    assert_eq!(
        cause::<EvalError>(&error).kind(),
        &EvalErrorKind::IntegerOutOfRange { target_type: "u64" }
    );
}

#[test]
fn f32_expressions_round_exact_values_directly_and_emit_f32_leaves() {
    // The first decimal lies just above the f32 midpoint, but rounds to that midpoint in f64.
    let source = "1.000000059604644775390626,1.000000059604644775390625,-0.0,1e-45";
    let input = source.to_node().unwrap();
    let values = <RealSequence as FromNodeVia<Vec<f32>>>::from_node(&input).unwrap();
    assert_eq!(
        values.iter().map(|value| value.to_bits()).collect::<Vec<_>>(),
        [
            1.0_f32.to_bits() + 1,
            1.0_f32.to_bits(),
            0.0_f32.to_bits(),
            1,
        ]
    );
    input.ensure_no_unknown_keys().unwrap();
    assert_eq!(source.split(',').next().unwrap().parse::<f64>().unwrap() as f32, 1.0);

    let output = <RealSequence as ToNodeVia<Vec<f32>>>::to_node(&values).unwrap();
    assert!(output.as_vec().unwrap().iter().all(|entry| {
        matches!(&entry.kind, Kind::Leaf(leaf) if matches!(leaf.value, Value::F32(_)))
    }));
    let roundtrip = <RealSequence as FromNodeVia<Vec<f32>>>::from_node(&output).unwrap();
    assert_eq!(roundtrip, values);

    let maximum = "340282346638528859811704183484516925440".to_node().unwrap();
    assert_eq!(<RealSequence as FromNodeVia<Vec<f32>>>::from_node(&maximum).unwrap(), [f32::MAX]);
    for description in [
        <RealSequence as DescribeVia<Vec<f32>>>::describe(),
        <RealSequence as DescribeVia<Vec<f64>>>::describe(),
    ] {
        assert_eq!(description.type_label(), "real sequence expression string or real array");
        assert!(matches!(description.kind, DescKind::Plain { .. }));
        assert!(description.doc.contains("ties to even"));
    }
    assert!(<RealSequence as DescribeVia<Vec<f32>>>::describe().doc.contains("f32"));
    assert!(<RealSequence as DescribeVia<Vec<f64>>>::describe().doc.contains("f64"));
}

#[test]
fn f32_overflow_is_checked_in_the_destination_type_and_arrays_keep_signed_zero() {
    let path = KeyPath::from_keys(["scope", "values"]);
    let input = Node::new_leaf(path.clone(), Value::String("3.4028236e38".to_owned()));
    let error = <RealSequence as FromNodeVia<Vec<f32>>>::from_node(&input).unwrap_err();
    assert_eq!(error.path(), Some(&path));
    assert_eq!(
        cause::<EvalError>(&error).kind(),
        &EvalErrorKind::RealValueNotFinite { target_type: "f32" }
    );
    assert!(<RealSequence as FromNodeVia<Vec<f64>>>::from_node(&input).unwrap()[0].is_finite());

    let input = vec![-0.0_f64, 0.0, f64::from(f32::MAX)].to_node().unwrap();
    let values = <RealSequence as FromNodeVia<Vec<f32>>>::from_node(&input).unwrap();
    assert_eq!(values[0].to_bits(), (-0.0_f32).to_bits());
    assert_eq!(values[1].to_bits(), 0.0_f32.to_bits());
    assert_eq!(values[2], f32::MAX);
    let output = <RealSequence as ToNodeVia<Vec<f32>>>::to_node(&values).unwrap();
    let roundtrip = <RealSequence as FromNodeVia<Vec<f32>>>::from_node(&output).unwrap();
    assert_eq!(roundtrip[0].to_bits(), (-0.0_f32).to_bits());

    for value in [f64::MAX, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let input = Node::new_vec(
            path.clone(),
            vec![
                Node::new_leaf(path.push_index(0), Value::F64(1.0)),
                Node::new_leaf(path.push_index(1), Value::F64(value)),
                Node::new_leaf(path.push_index(2), Value::F64(2.0)),
            ],
        );
        let error = <RealSequence as FromNodeVia<Vec<f32>>>::from_node(&input).unwrap_err();
        assert_eq!(error.path(), Some(&path.push_index(1)));
        assert!(matches!(error, NodeError::InvalidValue { .. }));
        assert!(error.source().is_none());
        assert!(!visited(&input.as_vec().unwrap()[2]));
    }
    let error =
        <RealSequence as FromNodeVia<Vec<f32>>>::from_node(&node(r#"["bad"]"#)).unwrap_err();
    assert_eq!(error.path(), Some(&KeyPath::from_index(0)));
    assert!(error.source().unwrap().is::<ParseFloatError>());
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let error = <RealSequence as ToNodeVia<Vec<f32>>>::to_node(&vec![0.0, value]).unwrap_err();
        assert_eq!(error.path(), Some(&KeyPath::from_index(1)));
        assert!(error.source().is_none());
    }
}

#[test]
fn every_target_shares_input_and_output_caps_before_element_conversion() {
    let path = KeyPath::from_keys(["scope", "values"]);
    let leaf = Node::new_leaf(KeyPath::new(), Value::Null);
    let input = Node::new_vec(path.clone(), vec![leaf.clone(); MAX_VALUES + 1]);
    macro_rules! check_target {
        ($policy:ty; $($target:ty),+ $(,)?) => {
            $(
                let error = <$policy as FromNodeVia<Vec<$target>>>::from_node(&input).unwrap_err();
                assert_eq!(error.path(), Some(&path));
                assert!(matches!(error, NodeError::InvalidValue { .. }));
                assert!(error.source().is_none());
                let error = <$policy as ToNodeVia<Vec<$target>>>::to_node(
                    &vec![0 as $target; MAX_VALUES + 1],
                ).unwrap_err();
                assert_eq!(error.path(), Some(&KeyPath::new()));
                assert!(matches!(error, NodeError::InvalidValue { .. }));
                assert!(error.source().is_none());
            )+
        };
    }
    check_target!(IntSequence; i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);
    check_target!(RealSequence; f32, f64);
    assert!(!visited(&leaf));

    for error in [
        <IntSequence as FromNodeVia<Vec<u64>>>::from_node(&"0..1000001".to_node().unwrap())
            .unwrap_err(),
        <RealSequence as FromNodeVia<Vec<f32>>>::from_node(&"[0..1000000]:1".to_node().unwrap())
            .unwrap_err(),
    ] {
        assert_eq!(
            cause::<EvalError>(&error).kind(),
            &EvalErrorKind::OutputLimitExceeded { limit: MAX_VALUES }
        );
    }
}

#[test]
fn u64_and_f32_accept_the_exact_array_and_output_count_boundary() {
    let leaf = Node::new_leaf(KeyPath::new(), Value::I64(1));
    let input = Node::new_vec(KeyPath::new(), vec![leaf; MAX_VALUES]);
    let values = <IntSequence as FromNodeVia<Vec<u64>>>::from_node(&input).unwrap();
    assert_eq!(values.len(), MAX_VALUES);
    assert_eq!(values.last(), Some(&1));
    let output = <IntSequence as ToNodeVia<Vec<u64>>>::to_node(&values).unwrap();
    assert_eq!(output.as_vec().unwrap().len(), MAX_VALUES);
    drop(output);
    drop(values);
    let values = <RealSequence as FromNodeVia<Vec<f32>>>::from_node(&input).unwrap();
    assert_eq!(values.len(), MAX_VALUES);
    assert_eq!(values.last(), Some(&1.0));
    let output = <RealSequence as ToNodeVia<Vec<f32>>>::to_node(&values).unwrap();
    assert_eq!(output.as_vec().unwrap().len(), MAX_VALUES);
}

#[test]
fn target_specific_policies_compose_through_optional_nested_sequences() {
    let input = node(r#"#{ counts: [(), "0..3", ["65535"]], weights: [(), "[0..1]/2", [-0.0]] }"#);
    let config = TargetGroups::from_node(&input).unwrap();
    assert_eq!(config.counts, [None, Some(vec![0, 1, 2]), Some(vec![u16::MAX])]);
    assert_eq!(config.weights[1], Some(vec![0.0, 0.5, 1.0]));
    assert_eq!(config.weights[2].as_ref().unwrap()[0].to_bits(), (-0.0_f32).to_bits());
    input.ensure_no_unknown_keys().unwrap();
    let output = config.to_node().unwrap();
    let roundtrip = TargetGroups::from_node(&output).unwrap();
    assert_eq!(roundtrip.counts, config.counts);
    assert_eq!(roundtrip.weights, config.weights);
    assert_eq!(roundtrip.weights[2].as_ref().unwrap()[0].to_bits(), (-0.0_f32).to_bits());

    let path = KeyPath::from_keys(["scope", "counts"]).push_index(1);
    let input = node(r#"#{ scope: #{ counts: [(), "-1"], weights: [] } }"#);
    let error = TargetGroups::from_node(input.req_node("scope").unwrap()).unwrap_err();
    assert_eq!(error.path(), Some(&path));
    assert_eq!(
        cause::<EvalError>(&error).kind(),
        &EvalErrorKind::IntegerOutOfRange { target_type: "u16" }
    );
    let input = node(r#"#{ scope: #{ counts: [(), [-1]], weights: [] } }"#);
    let error = TargetGroups::from_node(input.req_node("scope").unwrap()).unwrap_err();
    assert_eq!(error.path(), Some(&path.push_index(0)));
    assert!(error.source().unwrap().is::<TryFromIntError>());

    let input = node(r#"#{ scope: #{ counts: [], weights: [(), "3.4028236e38"] } }"#);
    let error = TargetGroups::from_node(input.req_node("scope").unwrap()).unwrap_err();
    assert_eq!(error.path(), Some(&KeyPath::from_keys(["scope", "weights"]).push_index(1)));
    assert_eq!(
        cause::<EvalError>(&error).kind(),
        &EvalErrorKind::RealValueNotFinite { target_type: "f32" }
    );

    let invalid = TargetGroups {
        counts: vec![],
        weights: vec![None, Some(vec![f32::NAN])],
    };
    let error = invalid.to_node().unwrap_err();
    assert_eq!(error.path(), Some(&KeyPath::from_keys(["weights"]).push_index(1).push_index(0)));
    assert!(error.source().is_none());
}

// ---------------------------------------------------------------------------------------------- //

#[derive(Debug, Config, ToNode)]
struct TargetGroups {
    #[scry(via(Vec<Option<IntSequence>>))]
    counts: Vec<Option<Vec<u16>>>,
    #[scry(via(Vec<Option<RealSequence>>))]
    weights: Vec<Option<Vec<f32>>>,
}

fn cause<E: Error + 'static>(error: &NodeError) -> &E {
    error
        .source()
        .and_then(|source| source.downcast_ref::<E>())
        .expect("expected concrete domain cause")
}

fn visited(node: &Node) -> bool {
    let Kind::Leaf(leaf) = &node.kind else {
        panic!("expected a leaf");
    };
    leaf.is_visited()
}

fn node(source: &str) -> Node {
    Node::parse_str(source, Format::Rhai).unwrap()
}
