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
    assert_eq!(finite_error.kind(), &EvalErrorKind::RealValueNotFinite { target_type: "f64" });

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

#[test]
fn f32_rounds_exact_values_directly_with_both_midpoint_parities() {
    let one = 1.0_f32.to_bits();
    for (source, expected_bits) in [
        ("1.000000059604644775390625", one),
        ("1.000000059604644775390624", one),
        ("1.000000059604644775390626", one + 1),
        ("1.000000178813934326171875", one + 2),
        ("1.999999940395355224609375", 2.0_f32.to_bits()),
        ("-1.000000059604644775390625", one | (1 << 31)),
        ("-1.000000059604644775390626", (one + 1) | (1 << 31)),
        ("-1.000000178813934326171875", (one + 2) | (1 << 31)),
    ] {
        assert_eq!(evaluate_f32(source)[0].to_bits(), expected_bits, "{source}");
    }

    // An intermediate f64 loses the difference above this exact binary32 midpoint.
    let source = "1.000000059604644775390626";
    assert_eq!((evaluate(source)[0] as f32).to_bits(), one);
    assert_eq!(evaluate_f32(source)[0].to_bits(), one + 1);
}

#[test]
fn f32_handles_subnormals_underflow_ties_and_the_minimum_normal_crossover() {
    for (numerator, denominator_power, expected_bits) in [
        (1, 149, 1),
        (-1, 149, (1 << 31) | 1),
        (1, 150, 0),
        (-1, 150, 0),
        (1, 151, 0),
        (3, 151, 1),
        (-3, 151, (1 << 31) | 1),
        (3, 150, 2),
        (5, 150, 2),
        (1, 126, 1 << 23),
        ((1 << 23) - 1, 149, (1 << 23) - 1),
        ((1 << 24) - 1, 150, 1 << 23),
        ((1 << 25) - 3, 151, (1 << 23) - 1),
        ((1 << 25) - 1, 151, 1 << 23),
    ] {
        let source = dyadic_source(numerator, denominator_power);
        assert_eq!(evaluate_f32(&source)[0].to_bits(), expected_bits, "{source}");
    }
    let zeros = evaluate_f32("-0.0,-1e-1000,[0..1e-1000]/2,[-1e-1000..1e-1000]/2");
    assert!(zeros.iter().all(|value| value.to_bits() == 0));
}

#[test]
fn f32_rejects_the_exact_overflow_tie_but_accepts_values_just_below_it() {
    let maximum = ((BigInt::one() << 24_usize) - 1_u8) << 104_usize;
    let overflow_tie = &maximum + (BigInt::one() << 103_usize);
    for (value, expected_bits) in [
        (maximum.clone(), f32::MAX.to_bits()),
        (-maximum, (-f32::MAX).to_bits()),
        (&overflow_tie - 1_u8, f32::MAX.to_bits()),
    ] {
        assert_eq!(evaluate_f32(&value.to_string())[0].to_bits(), expected_bits);
    }
    let evaluator = RealEvaluator::new(RealEvalOptions::default());
    for value in [
        &overflow_tie,
        &(&overflow_tie + 1_u8),
        &(-overflow_tie.clone()),
    ] {
        let expression = value.to_string().parse().unwrap();
        let error = evaluator.evaluate_f32(&expression).unwrap_err();
        assert_eq!(error.kind(), &EvalErrorKind::RealValueNotFinite { target_type: "f32" });
        assert!(evaluator.evaluate(&expression).unwrap()[0].is_finite());
    }
}

#[test]
fn f32_samplers_round_each_exact_anchor_and_skip_unretained_huge_values() {
    let one = 1.0_f32.to_bits();
    for source in [
        "[1..1.000000119209289550781252]/2",
        "[1..1.000000119209289550781252]:0.000000059604644775390626",
    ] {
        let bits: Vec<_> = evaluate_f32(source).into_iter().map(f32::to_bits).collect();
        assert_eq!(bits, [one, one + 1, one + 1], "{source}");
    }
    let bits: Vec<_> =
        evaluate_f32("[1.000000119209289550781252..1]/2").into_iter().map(f32::to_bits).collect();
    assert_eq!(bits, [one + 1, one + 1, one]);
    assert_eq!(evaluate_f32("[0..1e4000):1e4000"), [0.0]);
    assert_eq!(evaluate_f32("(-1e4000..1e4000)/2"), [0.0]);
    assert_eq!(evaluate_f32("[0..1]:1e4000"), [0.0]);
    assert!(evaluate_f32("(1e4000..1e4000)/1").is_empty());
}

#[test]
fn f32_keeps_error_associations_and_preflights_output_limits() {
    let evaluator = RealEvaluator::new(RealEvalOptions::default());
    let source = "0,1e4000";
    let error = evaluator.evaluate_f32(&source.parse().unwrap()).unwrap_err();
    assert_eq!(error.kind(), &EvalErrorKind::RealValueNotFinite { target_type: "f32" });
    assert_eq!(error.source_text(), source);
    assert_eq!(error.term_index(), Some(1));
    assert_eq!(
        error.span(),
        Span {
            start: 2,
            end: source.len()
        }
    );

    let limited = RealEvaluator::new(RealEvalOptions { max_values: 2 });
    let source = "0,[1..2]/1";
    let error = limited.evaluate_f32(&source.parse().unwrap()).unwrap_err();
    assert_eq!(error.kind(), &EvalErrorKind::OutputLimitExceeded { limit: 2 });
    assert_eq!(error.term_index(), Some(1));
    assert_eq!(
        error.span(),
        Span {
            start: 2,
            end: source.len()
        }
    );

    let bounded = RealEvaluator::new(RealEvalOptions {
        max_values: usize::MAX,
    });
    for source in ["[0..1]:1e-1000", "[0..1]/1000000"] {
        let error = bounded.evaluate_f32(&source.parse().unwrap()).unwrap_err();
        assert_eq!(error.kind(), &EvalErrorKind::OutputLimitExceeded { limit: MAX_VALUES });
    }
    let empty = RealEvaluator::new(RealEvalOptions { max_values: 0 });
    assert!(empty.evaluate_f32(&"(1e4000..1e4000)/1".parse().unwrap()).unwrap().is_empty());
    assert!(empty.evaluate_f32(&"1".parse().unwrap()).is_err());
}

#[test]
fn f32_decimal_conversion_agrees_with_direct_rust_parsing_across_magnitudes() {
    let evaluator = RealEvaluator::new(RealEvalOptions::default());
    for mantissa in [1, 3, 7, 9, 1_000_001, 16_777_215, 16_777_217, 99_999_999] {
        for exponent in -60..=45 {
            for sign in ["", "-"] {
                let source = format!("{sign}{mantissa}e{exponent}");
                let expected: f32 = source.parse().unwrap();
                let result = evaluator.evaluate_f32(&source.parse().unwrap());
                if expected.is_finite() {
                    let expected_bits = if expected == 0.0 {
                        0
                    } else {
                        expected.to_bits()
                    };
                    assert_eq!(result.unwrap()[0].to_bits(), expected_bits, "{source}");
                } else {
                    assert_eq!(
                        result.unwrap_err().kind(),
                        &EvalErrorKind::RealValueNotFinite { target_type: "f32" },
                        "{source}"
                    );
                }
            }
        }
    }
}

fn evaluate_f32(source: &str) -> Vec<f32> {
    RealEvaluator::new(RealEvalOptions::default()).evaluate_f32(&source.parse().unwrap()).unwrap()
}

fn dyadic_source(numerator: i64, denominator_power: u32) -> String {
    let decimal_significand = BigInt::from(numerator) * BigInt::from(5_u8).pow(denominator_power);
    format!("{decimal_significand}e-{denominator_power}")
}
