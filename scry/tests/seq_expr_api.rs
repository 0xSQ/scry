use scry::kit::seq_expr::{
    EvalError, EvalErrorKind, ExprBuildError, ExprBuildErrorKind, IndexError, IndexErrorKind,
    IndexEvaluator, IntContext, IntEvalOptions, IntEvaluator, IntSeqExpr, ParseError,
    ParseErrorKind, ParseLimits, ProfileError, ProfileErrorKind, RealEvalOptions, RealEvaluator,
    RealSeqExpr, SeqExpr, Span, MAX_VALUES,
};

// ---------------------------------------------------------------------------------------------- //

#[test]
fn profiles_preserve_authored_source_through_conversion_and_display() {
    let source = "\u{2003}[0..1]/2 , 3\n";
    let neutral: SeqExpr = source.parse().unwrap();
    assert_eq!(neutral.source(), source);
    assert_eq!(neutral.to_string(), source);

    let integer = IntSeqExpr::try_from(neutral.clone()).unwrap();
    assert_eq!(integer.source(), source);
    assert_eq!(integer.as_expr().source(), source);
    assert_eq!(integer.to_string(), source);
    assert_eq!(source.parse::<IntSeqExpr>().unwrap().source(), source);

    let real = RealSeqExpr::try_from(neutral).unwrap();
    assert_eq!(real.source(), source);
    assert_eq!(real.as_expr().source(), source);
    assert_eq!(real.to_string(), source);
    assert_eq!(source.parse::<RealSeqExpr>().unwrap().source(), source);
}

#[test]
fn profile_building_exposes_parse_and_profile_diagnostics() {
    let error: ExprBuildError = "1,,2".parse::<IntSeqExpr>().unwrap_err();
    assert_eq!(error.kind(), ExprBuildErrorKind::Parse(&ParseErrorKind::EmptyTerm));
    assert_eq!(error.source_text(), "1,,2");
    assert!(error.render().contains('^'));

    let source = "2.5";
    let error = source.parse::<IntSeqExpr>().unwrap_err();
    assert_eq!(
        error.kind(),
        ExprBuildErrorKind::Profile(&ProfileErrorKind::RealLiteralUnsupportedForInteger),
    );
    assert_eq!(error.source_text(), source);
    assert_eq!(error.span(), Span { start: 0, end: 3 });

    let neutral: SeqExpr = "N-1".parse().unwrap();
    let error: ProfileError = RealSeqExpr::try_from(neutral).unwrap_err();
    assert_eq!(error.kind(), &ProfileErrorKind::EndRelativeUnsupportedForReal);
    assert_eq!(error.term_index(), 0);
    assert_eq!(error.source_text(), "N-1");
    assert_eq!(error.span(), Span { start: 0, end: 3 });
}

#[test]
fn evaluators_accept_explicit_context_and_keep_indices_strict() {
    let symbolic: IntSeqExpr = "N-2..N".parse().unwrap();
    let error = IntEvaluator::new(IntEvalOptions::default()).evaluate(&symbolic).unwrap_err();
    assert_eq!(error.kind(), &EvalErrorKind::MissingFiniteContext);

    let numeric = IntEvaluator::new(IntEvalOptions {
        context: Some(IntContext::FiniteSource { length: 10 }),
        ..IntEvalOptions::default()
    });
    assert_eq!(numeric.evaluate(&symbolic).unwrap(), [8, 9]);
    assert_eq!(numeric.evaluate(&"N".parse().unwrap()).unwrap(), [10]);

    let bounded = IntEvaluator::new(IntEvalOptions {
        context: Some(IntContext::Bounds {
            open_start: 2,
            open_stop: 8,
        }),
        ..IntEvalOptions::default()
    });
    assert_eq!(bounded.evaluate(&"..:2".parse().unwrap()).unwrap(), [2, 4, 6]);
    assert_eq!(
        bounded.evaluate(&symbolic).unwrap_err().kind(),
        &EvalErrorKind::MissingFiniteContext
    );

    let indices = IndexEvaluator::default();
    assert_eq!(indices.evaluate(&"N-1,N-1".parse().unwrap(), 10).unwrap(), [9, 9]);
    let error: IndexError = indices.evaluate(&"N".parse().unwrap(), 10).unwrap_err();
    assert_eq!(
        error.kind(),
        &IndexErrorKind::OutOfBounds {
            index: 10,
            length: 10
        }
    );
    assert_eq!(error.source_text(), "N");
    assert_eq!(error.term_index(), Some(0));
    assert_eq!(error.span(), Span { start: 0, end: 1 });

    let real = RealEvaluator::new(RealEvalOptions::default());
    assert_eq!(real.evaluate(&"[0..0.3]:0.1".parse().unwrap()).unwrap(), [0.0, 0.1, 0.2, 0.3]);
}

#[test]
fn output_representability_is_checked_after_profile_building() {
    let integer: IntSeqExpr = "9223372036854775808".parse().unwrap();
    let error = IntEvaluator::new(IntEvalOptions::default()).evaluate(&integer).unwrap_err();
    assert_eq!(error.kind(), &EvalErrorKind::IntegerOutOfRange { target_type: "i64" });
    assert_eq!(error.source_text(), integer.source());

    // Only retained values must fit the output type, including after integer rounding.
    let integer: IntSeqExpr = "(-9223372036854775809..9223372036854775809)/2".parse().unwrap();
    assert_eq!(IntEvaluator::new(IntEvalOptions::default()).evaluate(&integer).unwrap(), [0]);

    let real: RealSeqExpr = "1e4000".parse().unwrap();
    let evaluator = RealEvaluator::new(RealEvalOptions::default());
    let error = evaluator.evaluate(&real).unwrap_err();
    assert_eq!(error.kind(), &EvalErrorKind::RealValueNotFinite { target_type: "f64" });

    // Exact endpoints outside f64 remain valid when the retained anchor is representable.
    let real: RealSeqExpr = "(-1e4000..1e4000)/2".parse().unwrap();
    assert_eq!(evaluator.evaluate(&real).unwrap(), [0.0]);

    // Single-precision evaluation rounds the exact decimal directly to its target precision.
    let real: RealSeqExpr = "1.000000059604644775390626".parse().unwrap();
    assert_eq!(evaluator.evaluate_f32(&real).unwrap()[0].to_bits(), 1.0_f32.to_bits() + 1);
    assert_eq!((evaluator.evaluate(&real).unwrap()[0] as f32).to_bits(), 1.0_f32.to_bits());
}

#[test]
fn custom_limits_report_public_typed_errors() {
    let limits = ParseLimits {
        max_literal_digits: 2,
        max_abs_exponent: 3,
        ..ParseLimits::default()
    };
    let error: ParseError = SeqExpr::parse_with_limits("123", limits).unwrap_err();
    assert_eq!(error.kind(), &ParseErrorKind::LiteralDigitLimitExceeded { limit: 2 });
    let error = SeqExpr::parse_with_limits("1e4", limits).unwrap_err();
    assert_eq!(error.kind(), &ParseErrorKind::ExponentLimitExceeded { limit: 3 });

    assert_eq!(IntEvalOptions::default().max_values, MAX_VALUES);
    assert_eq!(RealEvalOptions::default().max_values, MAX_VALUES);
    let integer = IntEvaluator::new(IntEvalOptions {
        max_values: 2,
        ..IntEvalOptions::default()
    });
    let error = integer.evaluate(&"1,2,3".parse().unwrap()).unwrap_err();
    assert_eq!(error.kind(), &EvalErrorKind::OutputLimitExceeded { limit: 2 });
    assert_eq!(error.term_index(), Some(2));

    let real = RealEvaluator::new(RealEvalOptions { max_values: 2 });
    let error = real.evaluate(&"[0..1]/2".parse().unwrap()).unwrap_err();
    assert_eq!(error.kind(), &EvalErrorKind::OutputLimitExceeded { limit: 2 });
}

#[test]
fn oversized_utf8_input_retains_a_bounded_diagnostic_prefix() {
    let source = format!("{}1", "\u{2003}".repeat(100));
    let limit = 257;
    let error = SeqExpr::parse_with_limits(
        &source,
        ParseLimits {
            max_input_bytes: limit,
            ..ParseLimits::default()
        },
    )
    .unwrap_err();
    assert_eq!(
        error.kind(),
        &ParseErrorKind::InputTooLong {
            limit,
            actual: source.len()
        }
    );
    assert!(error.source_text().len() <= 160);
    assert!(source.starts_with(error.source_text()));
    assert!(source.is_char_boundary(error.source_text().len()));
    assert!(source.is_char_boundary(error.span().start));
    assert!(error.span().start > error.source_text().len());
    assert_eq!(error.span().end, source.len());
    assert!(error.render().len() < 1_000);
    assert!(error.render().contains('^'));
}

#[test]
fn streaming_evaluation_can_deliver_values_before_a_later_failure() {
    let expression: IntSeqExpr = "1,2,9223372036854775808".parse().unwrap();
    let evaluator = IntEvaluator::new(IntEvalOptions::default());
    let mut delivered = Vec::new();
    let error = evaluator
        .evaluate_each(&expression, |value, term, span| {
            delivered.push((value, term, span));
            Ok::<_, EvalError>(())
        })
        .unwrap_err();
    assert_eq!(
        delivered,
        [
            (1, 0, Span { start: 0, end: 1 }),
            (2, 1, Span { start: 2, end: 3 })
        ],
    );
    assert_eq!(error.kind(), &EvalErrorKind::IntegerOutOfRange { target_type: "i64" });
    assert_eq!(error.term_index(), Some(2));
}

#[test]
fn streaming_evaluation_preserves_caller_errors_and_stops_callbacks() {
    let evaluator = IntEvaluator::new(IntEvalOptions::default());
    let expression: IntSeqExpr = "4,5,6".parse().unwrap();
    let mut delivered = Vec::new();
    let error = evaluator
        .evaluate_each(&expression, |value, _, _| {
            delivered.push(value);
            if value == 5 {
                Err(CallbackError::Rejected(value))
            } else {
                Ok(())
            }
        })
        .unwrap_err();
    assert!(matches!(error, CallbackError::Rejected(5)));
    assert_eq!(delivered, [4, 5]);

    let expression: IntSeqExpr = "9223372036854775808".parse().unwrap();
    let error =
        evaluator.evaluate_each(&expression, |_, _, _| Ok::<_, CallbackError>(())).unwrap_err();
    let CallbackError::Evaluation(error) = error else {
        panic!("evaluation failures must use the caller's From<EvalError> conversion");
    };
    assert_eq!(error.kind(), &EvalErrorKind::IntegerOutOfRange { target_type: "i64" });
}

// ---------------------------------------------------------------------------------------------- //

#[derive(Debug)]
enum CallbackError {
    Evaluation(EvalError),
    Rejected(i64),
}

impl From<EvalError> for CallbackError {
    fn from(error: EvalError) -> Self {
        Self::Evaluation(error)
    }
}
