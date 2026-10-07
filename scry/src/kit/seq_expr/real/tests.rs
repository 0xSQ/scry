use super::*;

// ---------------------------------------------------------------------------------------------- //

fn evaluate(source: &str) -> Vec<f64> {
    RealEvaluator::new(RealEvalOptions::default()).evaluate(&source.parse().unwrap()).unwrap()
}

#[test]
fn subdivision_is_anchored_exactly_and_honors_all_delimiters() {
    assert_eq!(
        evaluate("[1.0..0.0)/6"),
        vec![1.0, 5.0 / 6.0, 4.0 / 6.0, 0.5, 2.0 / 6.0, 1.0 / 6.0]
    );
    assert_eq!(evaluate("[0..1]/2"), vec![0.0, 0.5, 1.0]);
    assert_eq!(evaluate("[0..1)/2"), vec![0.0, 0.5]);
    assert_eq!(evaluate("(0..1]/2"), vec![0.5, 1.0]);
    assert_eq!(evaluate("(0..1)/2"), vec![0.5]);
}

#[test]
fn subdivision_is_symmetric_when_descending() {
    let mut ascending = evaluate("[0.0..1.0]/6");
    ascending.reverse();
    assert_eq!(evaluate("[1.0..0.0]/6"), ascending);
}

#[test]
fn fixed_decimal_steps_use_an_exact_anchor() {
    assert_eq!(evaluate("[0.1..0.5]:0.1"), vec![0.1, 0.2, 0.3, 0.4, 0.5]);
    assert_eq!(evaluate("[0..1]:0.3"), vec![0.0, 0.3, 0.6, 0.9]);
    assert_eq!(evaluate("[1..0]:0.3"), vec![1.0, 0.7, 0.4, 0.1]);
}

#[test]
fn accepts_integer_and_exponent_spelling() {
    assert_eq!(evaluate("1,2e-1,3.0"), vec![1.0, 0.2, 3.0]);
}

#[test]
fn rejects_open_endpoints_during_profile_building() {
    let error = "1.0..".parse::<RealSeqExpr>().unwrap_err();
    assert!(matches!(error, super::super::ExprBuildError::Profile(_)));
}

#[test]
fn rejects_symbolic_singletons_and_endpoints_at_the_authored_span() {
    for (source, term_index, span) in [
        ("N", 0, Span { start: 0, end: 1 }),
        ("N-1", 0, Span { start: 0, end: 3 }),
        ("1,N + 0", 1, Span { start: 2, end: 7 }),
        ("0..N:1", 0, Span { start: 3, end: 4 }),
        ("[N-1..3]/2", 0, Span { start: 1, end: 4 }),
        ("N..N", 0, Span { start: 0, end: 1 }),
    ] {
        let expression = SeqExpr::parse(source).unwrap();
        let error = RealSeqExpr::try_from(expression).unwrap_err();
        assert_eq!(error.kind(), &ProfileErrorKind::EndRelativeUnsupportedForReal);
        assert_eq!(error.term_index(), term_index);
        assert_eq!(error.span(), span);
        assert_eq!(error.source_text(), source);
    }
}

#[test]
fn equal_endpoints_preserve_subdivision_cardinality() {
    assert_eq!(evaluate("[1.0..1.0]/2"), [1.0, 1.0, 1.0]);
    assert!(evaluate("(1.0..1.0)/1").is_empty());
}

#[test]
fn rejects_non_finite_conversion_and_large_output_before_materializing() {
    let finite_error = RealEvaluator::new(RealEvalOptions::default())
        .evaluate(&"1e4000".parse().unwrap())
        .unwrap_err();
    assert_eq!(finite_error.kind(), &EvalErrorKind::RealValueNotFinite);

    let limit_error = RealEvaluator::new(RealEvalOptions { max_values: 3 })
        .evaluate(&"[0..1]/4".parse().unwrap())
        .unwrap_err();
    assert_eq!(limit_error.kind(), &EvalErrorKind::OutputLimitExceeded { limit: 3 });
}

#[test]
fn preserves_composition_and_empty_ranges() {
    assert_eq!(evaluate("2.0,1..1:1,2.0"), vec![2.0, 2.0]);
    assert_eq!(evaluate("(0..1)/1"), Vec::<f64>::new());
}

#[test]
fn requires_explicit_sampling_without_converting_values_during_profile_validation() {
    for source in ["0..1", "[0.0..1.0]", "1..1"] {
        let error = source.parse::<RealSeqExpr>().unwrap_err();
        assert!(matches!(
            error.kind(),
            super::super::ExprBuildErrorKind::Profile(ProfileErrorKind::MissingRealSampler)
        ));
    }
    "1e4000".parse::<RealSeqExpr>().unwrap();
    assert_eq!(evaluate("[0..1e4000):1e4000"), [0.0]);
    assert_eq!(evaluate("(-1e4000..1e4000)/2"), [0.0]);
    assert!(evaluate("(1e4000..1e4000)/1").is_empty());
    assert_eq!(evaluate("[0..1]:1e4000"), [0.0]);
}

#[test]
fn subdivision_is_monotonic_and_reversible_for_all_delimiters() {
    for (start, stop) in [
        ("-5", "5"),
        ("1.0", "1.0000000000000002"),
        ("1e-1000", "2e-1000"),
        ("-1.7976931348623157e308", "1.7976931348623157e308"),
        ("1.7976931348623155e308", "1.7976931348623157e308"),
    ] {
        for count in [1, 2, 7, 100] {
            for (left, right, reverse_left, reverse_right) in [
                ('[', ']', '[', ']'),
                ('[', ')', '(', ']'),
                ('(', ']', '[', ')'),
                ('(', ')', '(', ')'),
            ] {
                let source = format!("{left}{start}..{stop}{right}/{count}");
                let values = evaluate(&source);
                assert!(values.windows(2).all(|pair| pair[0] <= pair[1]), "{source}");
                let reversed =
                    evaluate(&format!("{reverse_left}{stop}..{start}{reverse_right}/{count}"));
                assert!(
                    values
                        .iter()
                        .map(|value| value.to_bits())
                        .eq(reversed.iter().rev().map(|value| value.to_bits())),
                    "{source}"
                );
                let low = evaluate(start)[0];
                let high = evaluate(stop)[0];
                assert!(values
                    .iter()
                    .all(|&value| value.is_finite() && value >= low && value <= high));
                if left == '[' {
                    assert_eq!(values.first().unwrap().to_bits(), low.to_bits());
                }
                if right == ']' {
                    assert_eq!(values.last().unwrap().to_bits(), high.to_bits());
                }
            }
        }
    }
}

#[test]
fn converts_rationals_directly_with_ties_to_even_and_normalizes_zeros() {
    assert_eq!(evaluate("1.00000000000000011102230246251565404236316680908203125"), [1.0]);
    assert_eq!(
        evaluate("1.00000000000000033306690738754696212708950042724609375"),
        [f64::from_bits(1.0_f64.to_bits() + 2)]
    );
    assert_eq!(evaluate("5e-324"), [f64::from_bits(1)]);
    assert_eq!(evaluate("2.4703282292062327e-324"), [0.0]);
    assert_eq!(evaluate("2.4703282292062328e-324"), [f64::from_bits(1)]);
    assert_eq!(evaluate("1.7976931348623157e308"), [f64::MAX]);
    assert!(RealEvaluator::new(RealEvalOptions::default())
        .evaluate(&"1.7976931348623159e308".parse().unwrap())
        .is_err());
    let zeros = evaluate("-0.0,-1e-1000,[0..1e-1000]/2,[-1e-1000..1e-1000]/2");
    assert!(zeros.iter().all(|value| value.to_bits() == 0.0_f64.to_bits()));
    assert_eq!(evaluate("(1.0..1.0000000000000002)/2"), [1.0]);
    assert_eq!(
        evaluate("[-1.7976931348623157e308..1.7976931348623157e308]/2"),
        [-f64::MAX, 0.0, f64::MAX]
    );
}

#[test]
fn checks_exact_cardinality_before_materializing_each_term() {
    let evaluator = RealEvaluator::new(RealEvalOptions {
        max_values: usize::MAX,
    });
    for source in [
        "[0..1]:1e-1000",
        "[0..1]/1000000",
        "[0..1]/999999999999999999999999999999999",
    ] {
        let error = evaluator.evaluate(&source.parse().unwrap()).unwrap_err();
        assert_eq!(error.kind(), &EvalErrorKind::OutputLimitExceeded { limit: MAX_VALUES });
    }
    let limited = RealEvaluator::new(RealEvalOptions { max_values: 3 });
    assert!(limited.evaluate(&"0,1,[2..3]/1".parse().unwrap()).is_err());
    let empty = RealEvaluator::new(RealEvalOptions { max_values: 0 });
    assert!(empty.evaluate(&"(1e4000..1e4000)/1".parse().unwrap()).unwrap().is_empty());
    assert!(empty.evaluate(&"1".parse().unwrap()).is_err());
}
