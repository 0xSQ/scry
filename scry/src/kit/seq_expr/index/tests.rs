use super::*;

// ---------------------------------------------------------------------------------------------- //

fn evaluate(source: &str, length: usize) -> Result<Vec<usize>, IndexError> {
    IndexEvaluator::default().evaluate(&source.parse().unwrap(), length)
}

#[test]
fn follows_the_complete_integer_language_without_clipping() {
    for (source, expected) in [
        ("..", (0..12).collect()),
        ("..5", vec![0, 1, 2, 3, 4]),
        ("5..", vec![5, 6, 7, 8, 9, 10, 11]),
        ("..:3", vec![0, 3, 6, 9]),
        ("[10..3]:2", vec![10, 8, 6, 4]),
        ("7,2,0", vec![7, 2, 0]),
        ("7,2,7", vec![7, 2, 7]),
        ("0..12", (0..12).collect()),
        ("0..100:100", vec![0]),
        ("3..3", vec![]),
    ] {
        assert_eq!(evaluate(source, 12).unwrap(), expected, "{source}");
    }
    for (source, index) in [("..100", 12), ("100..", 100), ("[0..]", 12), ("-1", -1)] {
        assert_eq!(
            evaluate(source, 12).unwrap_err().kind(),
            &IndexErrorKind::OutOfBounds { index, length: 12 }
        );
    }
}

#[test]
fn validates_rounded_indices_and_preserves_repeated_anchors() {
    assert_eq!(evaluate("[0..1]/4", 2).unwrap(), [0, 0, 1, 1, 1]);
    assert_eq!(evaluate("[1..1]/2", 2).unwrap(), [1, 1, 1]);
    assert_eq!(
        evaluate("../4", 2).unwrap_err().kind(),
        &IndexErrorKind::OutOfBounds {
            index: 2,
            length: 2
        }
    );
    assert!(evaluate("..", 0).unwrap().is_empty());
    assert!(evaluate("(99..99)/1", 0).unwrap().is_empty());
    assert!(evaluate("0", 0).is_err());
}

#[test]
fn retains_the_offending_term_and_span() {
    let source = "1, 2, 9..15";
    let error = evaluate(source, 12).unwrap_err();
    assert_eq!(error.term_index(), Some(2));
    assert_eq!(error.span(), Span { start: 6, end: 11 });
    assert_eq!(error.source_text(), source);
    assert!(error.to_string().contains("index 12 is outside 0..12"));
    let numeric = evaluate("1,9223372036854775808", 12).unwrap_err();
    assert_eq!(numeric.term_index(), Some(1));
    assert_eq!(numeric.kind(), &IndexErrorKind::Evaluation(EvalErrorKind::IntegerOutOfRange));
}

#[test]
fn enforces_limits_even_for_identity_selection() {
    let error = evaluate("..", MAX_VALUES + 1).unwrap_err();
    assert_eq!(
        error.kind(),
        &IndexErrorKind::Evaluation(EvalErrorKind::OutputLimitExceeded { limit: MAX_VALUES })
    );
    assert!(IndexEvaluator::new(2).evaluate(&"0,1,2".parse().unwrap(), 12).is_err());
    if usize::BITS > 63 {
        let error = evaluate("..", usize::MAX);
        assert!(error.is_err());
    }
}

#[test]
fn selects_end_relative_windows_repeats_and_reverse_ranges() {
    for (source, expected) in [
        ("N-1", vec![9]),
        ("N-6..N", vec![4, 5, 6, 7, 8, 9]),
        ("N-6..", vec![4, 5, 6, 7, 8, 9]),
        ("[N-6..N)", vec![4, 5, 6, 7, 8, 9]),
        ("N-6..N-3", vec![4, 5, 6]),
        ("0,N-1,N-1", vec![0, 9, 9]),
        ("[N-1..N-6]", vec![9, 8, 7, 6, 5, 4]),
        ("(N..0]", (0..10).rev().collect()),
        ("N-6..N:2", vec![4, 6, 8]),
        ("(N..0]:2", vec![8, 6, 4, 2, 0]),
        ("N-1..N+1:2", vec![9]),
        ("[N-1..N-1]/3", vec![9, 9, 9, 9]),
        ("[0..N-1]/4", vec![0, 2, 5, 7, 9]),
    ] {
        assert_eq!(evaluate(source, 10).unwrap(), expected, "{source}");
    }
    assert_eq!(evaluate("N-6..", 6).unwrap(), [0, 1, 2, 3, 4, 5]);
    assert_eq!(evaluate("0,N-1", 1).unwrap(), [0, 0]);
    assert_eq!(evaluate("[N-1..N-1]/3", 1).unwrap(), [0, 0, 0, 0]);
}

#[test]
fn keeps_short_sources_and_rounded_end_relative_indices_strict() {
    for length in [0, 1, 4, 5] {
        let error = evaluate("N-6..", length).unwrap_err();
        assert_eq!(
            error.kind(),
            &IndexErrorKind::OutOfBounds {
                index: length as i64 - 6,
                length
            }
        );
        assert_eq!(error.term_index(), Some(0));
    }
    for source in ["N", "N..0", "[N-6..]", "[N-1..N)/2"] {
        let error = evaluate(source, 10).unwrap_err();
        assert_eq!(
            error.kind(),
            &IndexErrorKind::OutOfBounds {
                index: 10,
                length: 10
            },
            "{source}"
        );
        assert_eq!(error.term_index(), Some(0));
        assert_eq!(
            error.span(),
            Span {
                start: 0,
                end: source.len()
            }
        );
    }
}

#[test]
fn empty_end_relative_ranges_follow_the_same_sampling_rules() {
    for source in ["N..N", "[N..N)", "(N..0]", "(N..N)/1"] {
        assert!(evaluate(source, 0).unwrap().is_empty(), "{source}");
    }
    for (source, index) in [("N", 0), ("N-1", -1), ("[N..N]", 0), ("N..N/2", 0)] {
        assert_eq!(
            evaluate(source, 0).unwrap_err().kind(),
            &IndexErrorKind::OutOfBounds { index, length: 0 },
            "{source}"
        );
    }
}

#[test]
fn retains_symbolic_value_spans_and_distinguishes_numeric_failures() {
    let source = "1, \u{2003}N - 12";
    let error = evaluate(source, 10).unwrap_err();
    assert_eq!(
        error.kind(),
        &IndexErrorKind::OutOfBounds {
            index: -2,
            length: 10
        }
    );
    assert_eq!(error.term_index(), Some(1));
    assert_eq!(
        error.span(),
        Span {
            start: source.find('N').unwrap(),
            end: source.len()
        }
    );
    assert_eq!(error.source_text(), source);
    let source = "1,N+9223372036854775807";
    let error = evaluate(source, 10).unwrap_err();
    assert_eq!(error.kind(), &IndexErrorKind::Evaluation(EvalErrorKind::EndRelativeOutOfRange));
    assert_eq!(error.term_index(), Some(1));
    assert_eq!(
        error.span(),
        Span {
            start: 2,
            end: source.len()
        }
    );
    if let Some(length) = usize::try_from(i64::MAX).ok().and_then(|length| length.checked_add(1)) {
        for source in ["0", "N..N"] {
            let error = evaluate(source, length).unwrap_err();
            assert_eq!(error.kind(), &IndexErrorKind::InvalidExtent { length });
            assert_eq!(error.term_index(), None);
        }
    }
}
