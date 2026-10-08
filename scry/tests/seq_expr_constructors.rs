use std::error::Error;

use scry::kit::seq_expr::{
    int_sequence, EvalError, EvalErrorKind, IndexEvaluator, IntContext, IntEvalOptions,
    IntEvaluator, IntSeqExpr, Span,
};
use scry::node::Format;
use scry::{Config, FromNode, KeyPath, Node, NodeError, ToNode};

// ---------------------------------------------------------------------------------------------- //

#[test]
fn open_range_matches_parsing_and_defers_context_until_evaluation() {
    let expression = IntSeqExpr::open_range();
    assert_eq!(expression.source(), "..");
    assert_eq!(expression.to_string(), "..");
    let output = expression.to_node().unwrap();
    assert_eq!(output.as_type::<String>().unwrap(), "..");
    let roundtrip = IntSeqExpr::from_node(&output).unwrap();
    assert_eq!(roundtrip.source(), expression.source());
    output.ensure_no_unknown_keys().unwrap();

    let error = IntEvaluator::new(IntEvalOptions::default()).evaluate(&expression).unwrap_err();
    assert_eq!(error.kind(), &EvalErrorKind::MissingOpenStartContext);
    assert_eq!(error.source_text(), "..");
    assert_eq!(error.span(), Span { start: 0, end: 2 });
    assert_eq!(error.term_index(), Some(0));

    let parsed: IntSeqExpr = "..".parse().unwrap();
    for length in [0, 4] {
        let evaluator = IntEvaluator::new(IntEvalOptions {
            context: Some(IntContext::FiniteSource { length }),
            ..IntEvalOptions::default()
        });
        let expected: Vec<i64> = (0..length).map(|index| index as i64).collect();
        assert_eq!(evaluator.evaluate(&expression).unwrap(), expected);
        assert_eq!(evaluator.evaluate(&parsed).unwrap(), expected);
        assert_eq!(
            IndexEvaluator::default().evaluate(&expression, length).unwrap(),
            (0..length).collect::<Vec<_>>()
        );
    }
    let evaluator = IntEvaluator::new(IntEvalOptions {
        context: Some(IntContext::Bounds {
            open_start: -2,
            open_stop: 3,
        }),
        ..IntEvalOptions::default()
    });
    assert_eq!(evaluator.evaluate(&expression).unwrap(), [-2, -1, 0, 1, 2]);
}

#[test]
fn single_accepts_unsuffixed_literals_and_matches_parsed_native_output() {
    let expression = IntSeqExpr::single(7);
    assert_eq!(expression.source(), "7");
    assert_eq!(expression.to_string(), "7");
    let parsed: IntSeqExpr = "7".parse().unwrap();
    let evaluator = IntEvaluator::new(IntEvalOptions::default());
    assert_eq!(evaluator.evaluate(&expression).unwrap(), [7]);
    assert_eq!(evaluator.evaluate(&expression).unwrap(), evaluator.evaluate(&parsed).unwrap());

    let output = expression.to_node().unwrap();
    assert_eq!(output.as_type::<String>().unwrap(), "7");
    assert_eq!(IntSeqExpr::from_node(&output).unwrap().source(), parsed.source());
    output.ensure_no_unknown_keys().unwrap();
}

#[test]
fn single_accepts_every_native_integer_type_and_preserves_its_boundaries() {
    macro_rules! check_target {
        ($($target:ident),+ $(,)?) => {
            $(
                for value in [<$target>::MIN, 0, <$target>::MAX] {
                    let expression: IntSeqExpr = IntSeqExpr::single(value);
                    let source = value.to_string();
                    assert_eq!(expression.source(), source);
                    assert_eq!(expression.to_string(), source);
                    let parsed: IntSeqExpr = source.parse().unwrap();
                    assert_eq!(parsed.source(), expression.source());
                    let output = expression.to_node().unwrap();
                    assert_eq!(output.as_type::<String>().unwrap(), source);
                    assert_eq!(
                        int_sequence::$target::from_node(&output).unwrap(),
                        [value],
                    );
                    assert_eq!(
                        int_sequence::$target::from_node(
                            &parsed.to_node().unwrap(),
                        ).unwrap(),
                        [value],
                    );
                }
            )+
        };
    }
    check_target!(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);
}

#[test]
fn single_defers_target_narrowing_and_retains_the_canonical_diagnostic_span() {
    let expression = IntSeqExpr::single(u64::MAX);
    let error = IntEvaluator::new(IntEvalOptions::default()).evaluate(&expression).unwrap_err();
    assert_eq!(error.kind(), &EvalErrorKind::IntegerOutOfRange { target_type: "i64" });
    assert_eq!(error.source_text(), "18446744073709551615");
    assert_eq!(error.span(), Span { start: 0, end: 20 });
    assert_eq!(error.term_index(), Some(0));

    for expression in [IntSeqExpr::single(256_u16), IntSeqExpr::single(-1_i8)] {
        let mut input = Node::empty_map();
        input.set_node("value", expression.to_node().unwrap()).unwrap();
        let error = int_sequence::u8::from_node(input.req_node("value").unwrap()).unwrap_err();
        assert_eq!(error.path(), Some(&KeyPath::from_keys(["value"])));
        let evaluation = error.source().unwrap().downcast_ref::<EvalError>().unwrap();
        assert_eq!(evaluation.kind(), &EvalErrorKind::IntegerOutOfRange { target_type: "u8" });
        assert_eq!(evaluation.source_text(), expression.source());
        assert_eq!(
            evaluation.span(),
            Span {
                start: 0,
                end: expression.source().len()
            }
        );
        assert_eq!(evaluation.term_index(), Some(0));
    }
}

#[test]
fn constructors_work_as_explicit_missing_only_config_defaults() {
    let config = ConstructorDefaults::from_node(&Node::empty_map()).unwrap();
    assert_eq!(config.selection.source(), "..");
    assert_eq!(config.value.source(), "7");
    assert_eq!(IndexEvaluator::default().evaluate(&config.selection, 3).unwrap(), [0, 1, 2]);
    let output = config.to_node().unwrap();
    assert_eq!(output.req::<String>("selection").unwrap(), "..");
    assert_eq!(output.req::<String>("value").unwrap(), "7");

    let input = Node::parse_str(r#"#{ selection: "1..3", value: "-4" }"#, Format::Rhai).unwrap();
    let config = ConstructorDefaults::from_node(&input).unwrap();
    assert_eq!(config.selection.source(), "1..3");
    assert_eq!(config.value.source(), "-4");
    input.ensure_no_unknown_keys().unwrap();

    let input = Node::parse_str("#{ selection: () }", Format::Rhai).unwrap();
    let error = ConstructorDefaults::from_node(&input).unwrap_err();
    assert!(matches!(error, NodeError::TypeMismatch { .. }));
    assert_eq!(error.path(), Some(&KeyPath::from_keys(["selection"])));
}

// ---------------------------------------------------------------------------------------------- //

#[derive(Debug, Config, ToNode)]
struct ConstructorDefaults {
    #[scry(default = IntSeqExpr::open_range())]
    selection: IntSeqExpr,
    #[scry(default = IntSeqExpr::single(7))]
    value: IntSeqExpr,
}
