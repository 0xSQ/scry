use super::super::ParseLimits;
use super::*;

// ---------------------------------------------------------------------------------------------- //

fn evaluate(source: &str) -> Vec<i64> {
    IntEvaluator::new(IntEvalOptions::default()).evaluate(&source.parse().unwrap()).unwrap()
}

#[test]
fn evaluates_lattices_delimiters_and_empty_ranges() {
    for (source, expected) in [
        ("3..10:2", vec![3, 5, 7, 9]),
        ("10..3:2", vec![10, 8, 6, 4]),
        ("(0..6]:2", vec![2, 4, 6]),
        ("[0..5]:2", vec![0, 2, 4]),
        ("[3..3]:2", vec![3]),
        ("3..3:2", vec![]),
        ("(3..3]:2", vec![]),
        ("(3..3):2", vec![]),
        ("2,1,2", vec![2, 1, 2]),
    ] {
        assert_eq!(evaluate(source), expected, "{source}");
    }
}

#[test]
fn rounds_complete_coordinates_and_preserves_duplicates() {
    for (source, expected) in [
        ("[0..10]/4", vec![0, 3, 5, 8, 10]),
        ("[10..0]/4", vec![10, 8, 5, 3, 0]),
        ("[-5..5]/4", vec![-5, -3, 0, 3, 5]),
        ("[0..2]/4", vec![0, 1, 1, 2, 2]),
        ("[0..1)/2", vec![0, 1]),
        ("[1..1]/2", vec![1, 1, 1]),
        ("(0..1)/1", vec![]),
        ("(1..1)/1", vec![]),
    ] {
        assert_eq!(evaluate(source), expected, "{source}");
    }
    for (numerator, expected) in [
        (149, 1),
        (150, 2),
        (151, 2),
        (-149, -1),
        (-150, -2),
        (-151, -2),
    ] {
        assert_eq!(round_anchor(numerator, 100), Some(expected));
    }
}

#[test]
fn subdivision_is_monotonic_reversible_and_symmetric_under_negation() {
    for start in -6_i64..=6 {
        for stop in start..=6 {
            for count in 1..=9 {
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
                    assert!(values.iter().eq(reversed.iter().rev()), "{source}");
                    let negated = evaluate(&format!("{left}{}..{}{right}/{count}", -start, -stop));
                    assert!(values.iter().map(|value| -value).eq(negated), "{source}");
                }
            }
        }
    }
}

#[test]
fn supports_extreme_i64_values_and_checks_conversion_during_evaluation() {
    assert_eq!(evaluate("[-9223372036854775808..9223372036854775807]/2"), [i64::MIN, -1, i64::MAX]);
    assert_eq!(
        evaluate("[-9223372036854775808..9223372036854775807]:9223372036854775807"),
        [i64::MIN, -1, i64::MAX - 1]
    );
    let expression = "9223372036854775808".parse::<IntSeqExpr>().unwrap();
    let error = IntEvaluator::new(IntEvalOptions::default()).evaluate(&expression).unwrap_err();
    assert_eq!(error.kind(), &EvalErrorKind::IntegerOutOfRange);
}

#[test]
fn supplies_context_and_reports_the_missing_bound() {
    let evaluator = IntEvaluator::new(IntEvalOptions {
        context: Some(IntContext::Bounds {
            open_start: 2,
            open_stop: 6,
        }),
        ..IntEvalOptions::default()
    });
    assert_eq!(evaluator.evaluate(&"..,[..]/2".parse().unwrap()).unwrap(), [2, 3, 4, 5, 2, 4, 6]);
    let error = IntEvaluator::new(IntEvalOptions::default())
        .evaluate(&"1,..3".parse().unwrap())
        .unwrap_err();
    assert_eq!(error.kind(), &EvalErrorKind::MissingOpenStartContext);
    assert_eq!(error.term_index(), Some(1));
}

#[test]
fn checks_each_term_before_emitting_and_enforces_the_hard_cap() {
    let evaluator = IntEvaluator::new(IntEvalOptions {
        max_values: usize::MAX,
        context: None,
    });
    let mut calls = 0;
    let error = evaluator
        .evaluate_each(&"[0..1]/1000000".parse().unwrap(), |_, _, _| {
            calls += 1;
            Ok::<_, EvalError>(())
        })
        .unwrap_err();
    assert_eq!(calls, 0);
    assert_eq!(error.kind(), &EvalErrorKind::OutputLimitExceeded { limit: MAX_VALUES });
    let values = evaluator
        .evaluate(&"(-9223372036854775808..9223372036854775807)/1000001".parse().unwrap())
        .unwrap();
    assert_eq!(values.len(), MAX_VALUES);
    assert!(values.windows(2).all(|pair| pair[0] <= pair[1]));
    let small = IntEvaluator::new(IntEvalOptions {
        max_values: 3,
        context: None,
    });
    assert!(small.evaluate(&"0..3,3".parse().unwrap()).is_err());
    assert!(small.evaluate(&"[0..1]/99999999999999999999999999999".parse().unwrap()).is_err());
}

#[test]
fn resolves_finite_source_values_without_imposing_index_bounds() {
    let evaluator = finite_evaluator(10);
    assert_eq!(
        evaluator.evaluate(&"N,N+1,N-12,..:3,[N-2..]/2".parse().unwrap()).unwrap(),
        [10, 11, -2, 0, 3, 6, 9, 8, 9, 10]
    );
    assert!(finite_evaluator(0).evaluate(&"N..N".parse().unwrap()).unwrap().is_empty());
}

#[test]
fn requires_finite_context_even_for_zero_offsets_and_empty_terms() {
    for context in [
        None,
        Some(IntContext::Bounds {
            open_start: 0,
            open_stop: 10,
        }),
    ] {
        let evaluator = IntEvaluator::new(IntEvalOptions {
            context,
            ..IntEvalOptions::default()
        });
        for (source, span) in [
            ("1,N", Span { start: 2, end: 3 }),
            ("1,N+0", Span { start: 2, end: 5 }),
            ("1,N..N", Span { start: 2, end: 3 }),
            ("1,(N..N)/1", Span { start: 3, end: 4 }),
        ] {
            let error = evaluator.evaluate(&source.parse().unwrap()).unwrap_err();
            assert_eq!(error.kind(), &EvalErrorKind::MissingFiniteContext, "{source}");
            assert_eq!(error.term_index(), Some(1));
            assert_eq!(error.span(), span);
            assert_eq!(error.source_text(), source);
        }
    }
    let error =
        IntEvaluator::new(IntEvalOptions::default()).evaluate(&"1..".parse().unwrap()).unwrap_err();
    assert_eq!(error.kind(), &EvalErrorKind::MissingOpenStopContext);
}

#[test]
fn narrows_the_resolved_coordinate_instead_of_the_written_offset() {
    for (source, length) in [("N-9223372036854775808", 0), ("N-9223372036854775809", 1)] {
        assert_eq!(
            finite_evaluator(length).evaluate(&source.parse().unwrap()).unwrap(),
            [i64::MIN]
        );
    }
    if let Ok(length) = usize::try_from(i64::MAX) {
        let evaluator = finite_evaluator(length);
        assert_eq!(evaluator.evaluate(&"N".parse().unwrap()).unwrap(), [i64::MAX]);
        assert_eq!(
            evaluator.evaluate(&"N-18446744073709551615".parse().unwrap()).unwrap(),
            [i64::MIN]
        );
        let error = evaluator.evaluate(&"1,N+1".parse().unwrap()).unwrap_err();
        assert_eq!(error.kind(), &EvalErrorKind::EndRelativeOutOfRange);
        assert_eq!(error.term_index(), Some(1));
        assert_eq!(error.span(), Span { start: 2, end: 5 });
    }
    let source = "(N-9223372036854775809..0)/1";
    let error = finite_evaluator(0).evaluate(&source.parse().unwrap()).unwrap_err();
    assert_eq!(error.kind(), &EvalErrorKind::EndRelativeOutOfRange);
    assert_eq!(
        error.span(),
        Span {
            start: 1,
            end: source.find("..").unwrap()
        }
    );
    let huge = format!("N+{}", "9".repeat(ParseLimits::default().max_literal_digits));
    let error = finite_evaluator(0).evaluate(&huge.parse().unwrap()).unwrap_err();
    assert_eq!(error.kind(), &EvalErrorKind::EndRelativeOutOfRange);
}

#[test]
fn rejects_invalid_finite_context_before_emitting_any_term() {
    let Some(length) = usize::try_from(i64::MAX).ok().and_then(|length| length.checked_add(1))
    else {
        return;
    };
    for source in ["0", "N..N", "(0..1)/1", ".."] {
        let mut calls = 0;
        let error = finite_evaluator(length)
            .evaluate_each(&source.parse().unwrap(), |_, _, _| {
                calls += 1;
                Ok::<_, EvalError>(())
            })
            .unwrap_err();
        assert_eq!(calls, 0);
        assert_eq!(error.kind(), &EvalErrorKind::InvalidFiniteExtent { length });
        assert_eq!(error.term_index(), None);
        assert_eq!(
            error.span(),
            Span {
                start: 0,
                end: source.len()
            }
        );
        assert_eq!(error.source_text(), source);
    }
}

#[test]
fn symbolic_sampling_agrees_with_resolved_numeric_coordinates() {
    for length in 0..=5 {
        let evaluator = finite_evaluator(length);
        for start_offset in [-7, -1, 0, 3] {
            for stop_offset in [-6, 0, 2] {
                for (left, right) in [('[', ']'), ('[', ')'), ('(', ']'), ('(', ')')] {
                    for sampler in ["", ":3", "/1", "/2", "/5"] {
                        let symbolic =
                            format!("{left}N{start_offset:+}..N{stop_offset:+}{right}{sampler}");
                        let numeric = format!(
                            "{left}{}..{}{right}{sampler}",
                            length as i64 + start_offset,
                            length as i64 + stop_offset
                        );
                        assert_eq!(
                            evaluator.evaluate(&symbolic.parse().unwrap()).unwrap(),
                            evaluate(&numeric),
                            "{symbolic}, length {length}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn symbolic_terms_preserve_cumulative_budgets_and_validate_empty_terms() {
    let evaluator = IntEvaluator::new(IntEvalOptions {
        context: Some(IntContext::FiniteSource { length: 10 }),
        max_values: 3,
    });
    let error = evaluator.evaluate(&"N-3..N,N-1".parse().unwrap()).unwrap_err();
    assert_eq!(error.kind(), &EvalErrorKind::OutputLimitExceeded { limit: 3 });
    assert_eq!(error.term_index(), Some(1));
    let evaluator = IntEvaluator::new(IntEvalOptions {
        context: Some(IntContext::FiniteSource { length: 10 }),
        max_values: 0,
    });
    assert!(evaluator.evaluate(&"N..N".parse().unwrap()).unwrap().is_empty());
    assert_eq!(
        evaluator.evaluate(&"N..N/2".parse().unwrap()).unwrap_err().kind(),
        &EvalErrorKind::OutputLimitExceeded { limit: 0 }
    );
}

fn finite_evaluator(length: usize) -> IntEvaluator {
    IntEvaluator::new(IntEvalOptions {
        context: Some(IntContext::FiniteSource { length }),
        ..IntEvalOptions::default()
    })
}
