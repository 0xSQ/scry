use super::super::{IntSeqExpr, RealSeqExpr};
use super::*;

// ---------------------------------------------------------------------------------------------- //

#[test]
fn parses_the_supported_term_shapes_and_preserves_source() {
    let source = " 7, 3..7, [3..7], (3..7], [4..28]:2, [1.0..0.0)/6, ..:2 ";
    let expression = SeqExpr::parse(source).unwrap();

    assert_eq!(expression.source(), source);
    assert_eq!(expression.terms.len(), 7);
}

#[test]
fn accepts_all_open_endpoint_combinations() {
    for source in ["3..", "..7", "..", "[3..]", "[..7]", "[..]"] {
        SeqExpr::parse(source).unwrap_or_else(|error| panic!("{source}: {error}"));
    }
}

#[test]
fn parses_end_relative_singletons_and_range_endpoints() {
    let source = "N, N-6.., [N-6..N), [N-1..N-6], (N..0], 0..N:2, [0..N-1]/4";
    let expression = SeqExpr::parse(source).unwrap();

    assert_eq!(expression.source(), source);
    assert_eq!(expression.terms.len(), 7);
    let TermKind::Singleton(ValueExpr::EndRelative { offset, span }) = &expression.terms[0].kind
    else {
        panic!("bare N must remain a symbolic value");
    };
    assert!(offset.is_zero());
    assert_eq!(*span, Span { start: 0, end: 1 });

    let TermKind::Range(range) = &expression.terms[2].kind else {
        panic!("an end-relative window must remain a range");
    };
    let Some(ValueExpr::EndRelative { offset, span }) = &range.start.value else {
        panic!("the window start must remain symbolic");
    };
    assert_eq!(*offset, BigInt::from(-6));
    assert_eq!(&source[span.start..span.end], "N-6");
    assert!(matches!(range.stop.value, Some(ValueExpr::EndRelative { .. })));
}

#[test]
fn end_relative_offsets_accept_whitespace_zero_and_exact_large_magnitudes() {
    let source = "N - 0006, N +\n0, N-0, N+18446744073709551615, N-18446744073709551615";
    let expression = SeqExpr::parse(source).unwrap();
    let expected = [
        ("N - 0006", "-6"),
        ("N +\n0", "0"),
        ("N-0", "0"),
        ("N+18446744073709551615", "18446744073709551615"),
        ("N-18446744073709551615", "-18446744073709551615"),
    ];

    for (term, (authored, offset_text)) in expression.terms.iter().zip(expected) {
        let TermKind::Singleton(ValueExpr::EndRelative { offset, span }) = &term.kind else {
            panic!("{authored} must remain symbolic");
        };
        assert_eq!(&source[span.start..span.end], authored);
        assert_eq!(offset.to_string(), offset_text);
    }
}

#[test]
fn rejects_foreign_and_arithmetic_end_relative_forms() {
    for source in [
        "n",
        "+N",
        "-N",
        "N-",
        "N+",
        "N--1",
        "N+-1",
        "N-+1",
        "N++1",
        "N-1.0",
        "N-1e0",
        "N-1_000",
        "N-1-2",
        "N+1+2",
        "N-1 2",
        "N1",
        "N-\u{ff11}",
        "2*N",
        "(N-1)",
        "0..N:N-1",
        "0..N/N",
        "N...",
    ] {
        assert!(SeqExpr::parse(source).is_err(), "{source:?}");
    }
}

#[test]
fn malformed_offset_errors_point_to_the_missing_or_signed_magnitude() {
    for (source, expected_span) in [
        ("N-", Span { start: 1, end: 2 }),
        ("N + ", Span { start: 2, end: 4 }),
        ("N--1", Span { start: 2, end: 3 }),
        ("N+-1", Span { start: 2, end: 3 }),
        ("N- ,0", Span { start: 3, end: 4 }),
    ] {
        let error = SeqExpr::parse(source).unwrap_err();
        assert_eq!(error.kind(), &ParseErrorKind::InvalidEndRelativeOffset);
        assert_eq!(error.span(), expected_span, "{source:?}");
    }
}

#[test]
fn rejects_shorthand_infinity_identifiers_and_negative_steps() {
    for source in [":2", "42:3", "[4..inf]", "[4..-inf]", "[10..0]:-2"] {
        assert!(SeqExpr::parse(source).is_err(), "{source} unexpectedly parsed");
    }
}

#[test]
fn rejects_malformed_terms_and_numbers() {
    for source in [
        "", " ", ",1", "1,", "1,,2", "[3]", ".5", "1.", "1e", "1..2:0", "1..2/0", "[1..2",
        "1..2:3:4",
    ] {
        assert!(SeqExpr::parse(source).is_err(), "{source:?} unexpectedly parsed");
    }
}

#[test]
fn enforces_each_parse_limit() {
    let defaults = ParseLimits::default();
    let error = SeqExpr::parse_with_limits(
        "123",
        ParseLimits {
            max_input_bytes: 2,
            ..defaults
        },
    )
    .unwrap_err();
    assert!(matches!(error.kind(), ParseErrorKind::InputTooLong { .. }));

    let error = SeqExpr::parse_with_limits(
        "123",
        ParseLimits {
            max_literal_digits: 2,
            ..defaults
        },
    )
    .unwrap_err();
    assert!(matches!(error.kind(), ParseErrorKind::LiteralDigitLimitExceeded { .. }));

    let error = SeqExpr::parse_with_limits(
        "1e11",
        ParseLimits {
            max_abs_exponent: 10,
            ..defaults
        },
    )
    .unwrap_err();
    assert!(matches!(error.kind(), ParseErrorKind::ExponentLimitExceeded { .. }));
}

#[test]
fn profile_parsing_retains_lexical_distinctions() {
    assert!("3".parse::<IntSeqExpr>().is_ok());
    assert!("3.0".parse::<IntSeqExpr>().is_err());
    assert!("3e0".parse::<IntSeqExpr>().is_err());
    assert!("3e0".parse::<RealSeqExpr>().is_ok());
    assert!("N-1".parse::<IntSeqExpr>().is_ok());
    assert!("N-1".parse::<RealSeqExpr>().is_err());
}

#[test]
fn spans_are_utf8_byte_offsets() {
    let error = SeqExpr::parse("\u{2003}1, n").unwrap_err();
    assert_eq!(error.span(), Span { start: 6, end: 7 });
}

#[test]
fn symbolic_spans_include_interior_utf8_whitespace_and_exclude_trailing_whitespace() {
    let source = "\u{2003}N\u{2003}-\u{2003}6\u{2003},N\u{2003}";
    let expression = SeqExpr::parse(source).unwrap();
    let TermKind::Singleton(value) = &expression.terms[0].kind else {
        panic!("the first term must be a symbolic singleton");
    };
    assert_eq!(value.span(), Span { start: 3, end: 12 });
    assert_eq!(&source[value.span().start..value.span().end], "N\u{2003}-\u{2003}6");
    let TermKind::Singleton(value) = &expression.terms[1].kind else {
        panic!("the second term must be a symbolic singleton");
    };
    assert_eq!(value.span(), Span { start: 16, end: 17 });

    let error = SeqExpr::parse("\u{2003}N-\u{2003}\u{ff11}").unwrap_err();
    assert_eq!(error.span(), Span { start: 8, end: 11 });
    assert_eq!(error.kind(), &ParseErrorKind::InvalidEndRelativeOffset);
}

#[test]
fn input_limit_diagnostics_do_not_split_utf8_characters() {
    let error = SeqExpr::parse_with_limits(
        "\u{2003}x",
        ParseLimits {
            max_input_bytes: 1,
            ..ParseLimits::default()
        },
    )
    .unwrap_err();

    assert_eq!(error.span().start, 0);
    assert!(error.render().contains("input has 4 bytes"));
}

#[test]
fn input_byte_limit_accepts_its_boundary_and_bounds_retained_errors() {
    let max_input_bytes = 4 * 1024 * 1024;
    let mut source = " ".repeat(max_input_bytes - 1);
    source.push('0');
    let expression = SeqExpr::parse(&source).unwrap();
    assert_eq!(expression.source(), source);
    assert_eq!(expression.terms.len(), 1);

    source.push(' ');
    let unbounded = ParseLimits {
        max_input_bytes: usize::MAX,
        max_literal_digits: usize::MAX,
        max_abs_exponent: usize::MAX,
    };
    for limits in [ParseLimits::default(), unbounded] {
        let error = SeqExpr::parse_with_limits(&source, limits).unwrap_err();
        assert_eq!(error.source_text(), &source[..160]);
        assert!(matches!(
            error.kind(),
            ParseErrorKind::InputTooLong { limit, actual }
                if *limit == max_input_bytes && *actual == max_input_bytes + 1
        ));
    }
}

#[test]
fn digit_limits_count_exponent_digits_and_leading_zeros() {
    let accepted = format!("1e{}1", "0".repeat(1022));
    SeqExpr::parse(&accepted).unwrap();
    let excessive = format!("1e{}1", "0".repeat(1023));
    assert!(matches!(
        SeqExpr::parse(&excessive).unwrap_err().kind(),
        ParseErrorKind::LiteralDigitLimitExceeded { .. }
    ));
    SeqExpr::parse(&"0".repeat(1024)).unwrap();
    assert!(SeqExpr::parse(&"0".repeat(1025)).is_err());
    SeqExpr::parse("1e4096").unwrap();
    assert!(matches!(
        SeqExpr::parse("1e4097").unwrap_err().kind(),
        ParseErrorKind::ExponentLimitExceeded { .. }
    ));
}

#[test]
fn strict_grammar_rejects_foreign_literal_and_sampler_forms() {
    for source in [
        ".5",
        "1.",
        "1_000",
        "0xff",
        "1.0f32",
        "NaN",
        "inf",
        "1 2",
        "1 . 2",
        "1. .2",
        "[1]",
        "(1)",
        "1/2",
        "1:2",
        "[0..1]/2:1",
        "[0..1]:1/2",
        "0..1/-1",
        "0..1/+1",
        "0..1/1.0",
        "0..1/1e2",
        "0..1:0e4000",
    ] {
        assert!(SeqExpr::parse(source).is_err(), "{source}");
    }
    SeqExpr::parse("\n +1 , [-2 .. +3) : +2 , (0.5..1E+1]/003 \n").unwrap();
}

#[test]
fn accepts_comma_lists_without_a_separate_term_limit() {
    let source = vec!["0"; 10_000].join(",");
    let expression = SeqExpr::parse(&source).unwrap();
    assert_eq!(expression.source(), source);
    assert_eq!(expression.terms.len(), 10_000);
}

#[test]
fn subdivision_digit_limit_includes_its_boundary() {
    SeqExpr::parse(&format!("[0..1]/{}1", "0".repeat(1023))).unwrap();
    assert!(matches!(
        SeqExpr::parse(&format!("[0..1]/{}1", "0".repeat(1024))).unwrap_err().kind(),
        ParseErrorKind::LiteralDigitLimitExceeded { .. }
    ));
}

#[test]
fn offset_digit_limit_counts_leading_zeros_and_cannot_exceed_the_hard_ceiling() {
    SeqExpr::parse(&format!("N-{}1", "0".repeat(1023))).unwrap();
    let excessive = format!("N+{}1", "0".repeat(1024));
    let unbounded = ParseLimits {
        max_literal_digits: usize::MAX,
        ..ParseLimits::default()
    };
    for limits in [ParseLimits::default(), unbounded] {
        let error = SeqExpr::parse_with_limits(&excessive, limits).unwrap_err();
        assert_eq!(
            error.span(),
            Span {
                start: 2,
                end: 1027
            }
        );
        assert_eq!(error.kind(), &ParseErrorKind::LiteralDigitLimitExceeded { limit: 1024 });
    }

    let tightened = ParseLimits {
        max_literal_digits: 2,
        ..ParseLimits::default()
    };
    SeqExpr::parse_with_limits("N - 06", tightened).unwrap();
    let error = SeqExpr::parse_with_limits("N - 006", tightened).unwrap_err();
    assert_eq!(error.span(), Span { start: 4, end: 7 });
    assert_eq!(error.kind(), &ParseErrorKind::LiteralDigitLimitExceeded { limit: 2 });
}
