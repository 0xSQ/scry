use std::error::Error;

use scry::desc::{DescKind, FieldDesc};
use scry::kit::seq_expr::{
    EvalErrorKind, ExprBuildError, ExprBuildErrorKind, IndexEvaluator, IntEvalOptions,
    IntEvaluator, IntSeqExpr, ParseError, ParseErrorKind, ParseLimits, ProfileErrorKind,
    RealEvalOptions, RealEvaluator, RealSeqExpr, SeqExpr,
};
use scry::node::{Format, Value};
use scry::{Config, Desc, Describe, FromDefaults, FromNode, KeyPath, Node, NodeError, ToNode};

// ---------------------------------------------------------------------------------------------- //

#[test]
fn native_traits_preserve_source_and_consume_directly_decoded_leaves() {
    let source = "\u{2003}[0..1]/2 , 3\n";
    assert_native_roundtrip::<SeqExpr>(source);
    assert_native_roundtrip::<IntSeqExpr>(source);
    assert_native_roundtrip::<RealSeqExpr>(source);
}

#[test]
fn retained_expressions_accept_only_string_leaves() {
    for source in ["()", "3", "3.0", "true", "[]", "#{ expression: 3 }"] {
        let node = node(source);
        assert!(SeqExpr::from_node(&node).is_err(), "accepted neutral {source}");
        assert!(IntSeqExpr::from_node(&node).is_err(), "accepted integer {source}");
        assert!(RealSeqExpr::from_node(&node).is_err(), "accepted real {source}");
    }
}

#[test]
fn native_decoding_validates_profiles_without_evaluating_values() {
    let integer = IntSeqExpr::from_node(&"N-2..N".to_node().unwrap()).unwrap();
    assert_eq!(integer.source(), "N-2..N");
    let error = IntEvaluator::new(IntEvalOptions::default()).evaluate(&integer).unwrap_err();
    assert_eq!(error.kind(), &EvalErrorKind::MissingFiniteContext);
    assert_eq!(IndexEvaluator::default().evaluate(&integer, 10).unwrap(), [8, 9]);

    let integer = IntSeqExpr::from_node(&"9223372036854775808".to_node().unwrap()).unwrap();
    assert_eq!(
        IntEvaluator::new(IntEvalOptions::default()).evaluate(&integer).unwrap_err().kind(),
        &EvalErrorKind::IntegerOutOfRange { target_type: "i64" },
    );

    let real = RealSeqExpr::from_node(&"1e4000".to_node().unwrap()).unwrap();
    let evaluator = RealEvaluator::new(RealEvalOptions::default());
    assert_eq!(
        evaluator.evaluate(&real).unwrap_err().kind(),
        &EvalErrorKind::RealValueNotFinite { target_type: "f64" }
    );
    let real = RealSeqExpr::from_node(&"(-1e4000..1e4000)/2".to_node().unwrap()).unwrap();
    assert_eq!(evaluator.evaluate(&real).unwrap(), [0.0]);

    let neutral = SeqExpr::from_node(&"0..1".to_node().unwrap()).unwrap();
    let error = RealSeqExpr::try_from(neutral).unwrap_err();
    assert_eq!(error.kind(), &ProfileErrorKind::MissingRealSampler);
}

#[test]
fn domain_errors_keep_concrete_causes_and_full_literal_field_paths() {
    let expected = KeyPath::from_keys(["scope", "expression.text"]);
    let input = node(r#"#{ scope: #{ "expression.text": "1,,2" } }"#);
    let error =
        ExpressionField::<SeqExpr>::from_node(input.req_node("scope").unwrap()).unwrap_err();
    assert_eq!(error.path(), Some(&expected));
    let parse = cause::<ParseError>(&error);
    assert_eq!(parse.kind(), &ParseErrorKind::EmptyTerm);
    assert_eq!(parse.source_text(), "1,,2");
    assert!(parse.render().contains('^'));

    let error =
        ExpressionField::<IntSeqExpr>::from_node(input.req_node("scope").unwrap()).unwrap_err();
    assert_eq!(error.path(), Some(&expected));
    assert_eq!(
        cause::<ExprBuildError>(&error).kind(),
        ExprBuildErrorKind::Parse(&ParseErrorKind::EmptyTerm),
    );

    let input = node(r#"#{ scope: #{ "expression.text": "2.5" } }"#);
    let error =
        ExpressionField::<IntSeqExpr>::from_node(input.req_node("scope").unwrap()).unwrap_err();
    assert_eq!(error.path(), Some(&expected));
    let build = cause::<ExprBuildError>(&error);
    assert_eq!(
        build.kind(),
        ExprBuildErrorKind::Profile(&ProfileErrorKind::RealLiteralUnsupportedForInteger),
    );
    assert_eq!(build.source_text(), "2.5");

    let input = node(r#"#{ scope: #{ "expression.text": "N-1" } }"#);
    let error =
        ExpressionField::<RealSeqExpr>::from_node(input.req_node("scope").unwrap()).unwrap_err();
    assert_eq!(error.path(), Some(&expected));
    let build = cause::<ExprBuildError>(&error);
    assert_eq!(
        build.kind(),
        ExprBuildErrorKind::Profile(&ProfileErrorKind::EndRelativeUnsupportedForReal),
    );
    assert_eq!(build.source_text(), "N-1");
}

#[test]
fn config_fields_keep_required_optional_and_missing_only_default_behavior() {
    let input = node(r#"#{ "required.expr": "N-1" }"#);
    let settings = Settings::from_node(&input).unwrap();
    assert_eq!(settings.required.source(), "N-1");
    assert!(settings.optional.is_none());
    assert_eq!(settings.defaulted.source(), "..");
    input.ensure_no_unknown_keys().unwrap();

    let scope = KeyPath::from_keys(["scope"]);
    let missing = Settings::from_node(&Node::empty_map_at(scope.clone())).unwrap_err();
    assert!(
        matches!(missing, NodeError::MissingRequired { path } if path == scope.push_key("required.expr"))
    );
    let missing = Settings::from_defaults_at(&scope).unwrap_err();
    assert!(
        matches!(missing, NodeError::MissingRequired { path } if path == scope.push_key("required.expr"))
    );

    let input = node(r#"#{ "required.expr": "N-1", optional: (), defaulted: "[2..4]" }"#);
    let settings = Settings::from_node(&input).unwrap();
    assert!(settings.optional.is_none());
    assert_eq!(settings.defaulted.source(), "[2..4]");
    input.ensure_no_unknown_keys().unwrap();
    let output = settings.to_node().unwrap();
    assert_eq!(output.req::<Option<String>>("optional").unwrap(), None);
    assert_eq!(Settings::from_node(&output).unwrap().defaulted.source(), "[2..4]");

    for (source, key) in [
        (r#"#{ "required.expr": () }"#, "required.expr"),
        (r#"#{ "required.expr": "N-1", defaulted: () }"#, "defaulted"),
        (r#"#{ "required.expr": "N-1", defaulted: "" }"#, "defaulted"),
        (r#"#{ "required.expr": "N-1", optional: "N-1" }"#, "optional"),
    ] {
        let error = Settings::from_node(&node(source)).unwrap_err();
        assert_eq!(error.path(), Some(&KeyPath::from_keys([key])), "{source}");
    }
}

#[test]
fn native_containers_preserve_strings_nulls_and_indexed_errors() {
    let input = node(r#"#{ "experiments.values": [(), "[0..1]/2", "1e4000"] }"#);
    let config = Expressions::from_node(&input).unwrap();
    assert!(config.values[0].is_none());
    assert_eq!(config.values[1].as_ref().unwrap().source(), "[0..1]/2");
    assert_eq!(config.values[2].as_ref().unwrap().source(), "1e4000");
    input.ensure_no_unknown_keys().unwrap();

    let output = config.to_node().unwrap();
    let expected = vec![
        None,
        Some("[0..1]/2".to_string()),
        Some("1e4000".to_string()),
    ];
    assert_eq!(
        output.req::<Vec<Option<String>>>(KeyPath::from_keys(["experiments.values"])).unwrap(),
        expected
    );
    let decoded = Expressions::from_node(&output).unwrap();
    assert_eq!(decoded.values[2].as_ref().unwrap().source(), "1e4000");

    let input = node(r#"#{ scope: #{ "experiments.values": [(), "[0..1]/2", "N-1"] } }"#);
    let error = Expressions::from_node(input.req_node("scope").unwrap()).unwrap_err();
    let expected = KeyPath::from_keys(["scope", "experiments.values"]).push_index(2);
    assert_eq!(error.path(), Some(&expected));
    assert_eq!(
        cause::<ExprBuildError>(&error).kind(),
        ExprBuildErrorKind::Profile(&ProfileErrorKind::EndRelativeUnsupportedForReal),
    );
}

#[test]
fn descriptions_keep_plain_expression_shapes_and_container_metadata() {
    for (description, hint) in [
        (SeqExpr::describe(), "sequence expression string"),
        (IntSeqExpr::describe(), "integer sequence expression string"),
        (RealSeqExpr::describe(), "real sequence expression string"),
    ] {
        assert!(!description.nullable);
        assert!(matches!(&description.kind, DescKind::Plain { .. }));
        assert_eq!(description.type_label(), hint);
        assert!(!description.doc.is_empty());
    }

    let settings = Settings::describe();
    let required = field(&settings, "required.expr");
    assert!(!required.optional);
    assert!(!required.value.nullable);
    let optional = field(&settings, "optional");
    assert!(optional.optional);
    assert!(optional.value.nullable);
    let defaulted = field(&settings, "defaulted");
    assert!(defaulted.optional);
    assert!(!defaulted.value.nullable);
    settings.validate_path(r#"["required.expr"]"#).unwrap();
    assert!(settings.validate_path(r#"["required.expr"][0]"#).is_err());

    let expressions = Expressions::describe();
    let values = field(&expressions, "experiments.values");
    assert!(!values.optional);
    assert!(!values.value.nullable);
    let DescKind::List { item } = &values.value.kind else {
        panic!("native expression vectors must retain their list shape");
    };
    assert!(item.nullable);
    assert_eq!(item.type_label(), "real sequence expression string");
    expressions.validate_path(r#"["experiments.values"][2]"#).unwrap();
    assert!(expressions.validate_path(r#"["experiments.values"][2][0]"#).is_err());
}

#[test]
fn oversized_native_input_preserves_bounded_diagnostics_and_full_spans() {
    let limit = ParseLimits::default().max_input_bytes;
    let source = "\u{2003}".repeat(limit / 3 + 1);
    let original_bytes = source.len();
    let path = KeyPath::from_keys(["experiment", "expression.text"]);
    let input = Node::new_leaf(path.clone(), Value::String(source));
    let error = SeqExpr::from_node(&input).unwrap_err();
    assert_eq!(error.path(), Some(&path));
    let parse = cause::<ParseError>(&error);
    assert_eq!(
        parse.kind(),
        &ParseErrorKind::InputTooLong {
            limit,
            actual: original_bytes
        }
    );
    assert!(parse.source_text().len() <= 160);
    assert!(parse.span().start > parse.source_text().len());
    assert_eq!(parse.span().end, original_bytes);
    assert!(format!("{error:?}").len() < 2_000);
    input.ensure_no_unknown_keys().unwrap();

    let error = IntSeqExpr::from_node(&input).unwrap_err();
    assert_eq!(error.path(), Some(&path));
    assert_eq!(
        cause::<ExprBuildError>(&error).kind(),
        ExprBuildErrorKind::Parse(&ParseErrorKind::InputTooLong {
            limit,
            actual: original_bytes
        }),
    );
}

// ---------------------------------------------------------------------------------------------- //

#[derive(Debug, Config, ToNode)]
struct Settings {
    #[scry(rename = "required.expr")]
    required: IntSeqExpr,
    optional: Option<RealSeqExpr>,
    #[scry(default = "..".parse().expect("valid integer expression default"))]
    defaulted: IntSeqExpr,
}

#[derive(Debug, Config, ToNode)]
struct Expressions {
    #[scry(rename = "experiments.values")]
    values: Vec<Option<RealSeqExpr>>,
}

#[derive(Debug, Config, ToNode)]
struct ExpressionField<T> {
    #[scry(rename = "expression.text")]
    expression: T,
}

fn assert_native_roundtrip<T: FromNode + ToNode>(source: &str) {
    let input = source.to_node().unwrap();
    let value = T::from_node(&input).unwrap();
    input.ensure_no_unknown_keys().unwrap();
    let output = value.to_node().unwrap();
    assert_eq!(output.as_type::<String>().unwrap(), source);
    let decoded = T::from_node(&output).unwrap();
    assert_eq!(decoded.to_node().unwrap().as_type::<String>().unwrap(), source);
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
