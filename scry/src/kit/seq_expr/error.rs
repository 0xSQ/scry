use std::fmt::{self, Display, Formatter};
use std::sync::Arc;

use thiserror::Error;

use super::Span;

// ---------------------------------------------------------------------------------------------- //

/// Describes why parsing an expression failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseErrorKind {
    /// The authored input exceeds its effective UTF-8 byte limit.
    InputTooLong {
        /// The maximum permitted number of UTF-8 bytes.
        limit: usize,
        /// The number of UTF-8 bytes in the complete authored input.
        actual: usize,
    },
    /// The expression contains no terms.
    EmptyInput,
    /// A comma-separated term is missing.
    EmptyTerm,
    /// The input ends before a required token appears.
    UnexpectedEnd {
        /// The token or syntactic form expected at the end of input.
        expected: &'static str,
    },
    /// A token appears where another token or syntactic form was required.
    UnexpectedToken {
        /// The token or syntactic form expected at the failure location.
        expected: &'static str,
    },
    /// A bracketed range lacks a valid closing endpoint delimiter.
    MissingEndpointDelimiter,
    /// A numeric token is malformed or unsupported.
    InvalidNumber,
    /// An end-relative offset lacks an unsigned decimal magnitude.
    InvalidEndRelativeOffset,
    /// A number, subdivision count, or offset magnitude exceeds its decimal digit limit.
    LiteralDigitLimitExceeded {
        /// The maximum permitted number of decimal digits in the token.
        limit: usize,
    },
    /// The absolute written numeric exponent exceeds its permitted magnitude.
    ExponentLimitExceeded {
        /// The maximum permitted absolute exponent magnitude.
        limit: usize,
    },
    /// A fixed-step magnitude is not greater than zero.
    NonPositiveStep,
    /// A subdivision count is not greater than zero.
    NonPositiveSubdivisionCount,
}

impl Display for ParseErrorKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::InputTooLong { limit, actual } => {
                write!(formatter, "input has {actual} bytes. The limit is {limit}")
            }
            Self::EmptyInput => formatter.write_str("sequence expression is empty"),
            Self::EmptyTerm => formatter.write_str("sequence term is empty"),
            Self::UnexpectedEnd { expected } => {
                write!(formatter, "unexpected end of input. Expected {expected}")
            }
            Self::UnexpectedToken { expected } => {
                write!(formatter, "unexpected token. Expected {expected}")
            }
            Self::MissingEndpointDelimiter => {
                formatter.write_str("bracketed range is missing its closing endpoint delimiter")
            }
            Self::InvalidNumber => formatter.write_str("invalid numeric literal"),
            Self::InvalidEndRelativeOffset => {
                formatter.write_str("end-relative offsets require unsigned decimal digits")
            }
            Self::LiteralDigitLimitExceeded { limit } => {
                write!(formatter, "numeric literal exceeds the {limit}-digit limit")
            }
            Self::ExponentLimitExceeded { limit } => {
                write!(formatter, "exponent magnitude exceeds the limit of {limit}")
            }
            Self::NonPositiveStep => {
                formatter.write_str("step magnitude must be greater than zero")
            }
            Self::NonPositiveSubdivisionCount => {
                formatter.write_str("subdivision count must be greater than zero")
            }
        }
    }
}

/// Reports a syntax or lexical-limit error.
#[derive(Debug, Clone, Error)]
#[error("{}", self.render())]
pub struct ParseError {
    source_text: Arc<str>,
    span: Span,
    kind: ParseErrorKind,
}

impl ParseError {
    /// Creates a parser diagnostic with its source and original byte span.
    pub(super) fn new(source: Arc<str>, span: Span, kind: ParseErrorKind) -> Self {
        Self {
            source_text: source,
            span,
            kind,
        }
    }

    /// Returns the authored expression, or a bounded prefix when the input exceeded its limit.
    ///
    /// For [`ParseErrorKind::InputTooLong`], the retained source is a UTF-8-safe prefix of at most
    /// 160 bytes. Its span still refers to the original input and may extend beyond this prefix.
    pub fn source_text(&self) -> &str {
        &self.source_text
    }

    /// Returns the half-open UTF-8 byte span in the original authored input.
    ///
    /// For [`ParseErrorKind::InputTooLong`], this span may lie outside the retained prefix returned
    /// by [`Self::source_text`]. It must not be used to slice that prefix without checking bounds.
    pub fn span(&self) -> Span {
        self.span
    }

    /// Returns the machine-readable error category.
    pub fn kind(&self) -> &ParseErrorKind {
        &self.kind
    }

    /// Renders a bounded source excerpt with a caret and explanation.
    ///
    /// Overlong input diagnostics render the retained prefix and clamp the caret to that prefix.
    pub fn render(&self) -> String {
        render_diagnostic(&self.source_text, self.span, &self.kind.to_string())
    }
}

/// Describes why a parsed expression is invalid for a numeric profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileErrorKind {
    /// An integer-profile literal uses a decimal point or exponent spelling.
    RealLiteralUnsupportedForInteger,
    /// A real-profile range omits an endpoint.
    OpenEndpointUnsupportedForReal,
    /// A real-profile range omits its explicit step or subdivision sampler.
    MissingRealSampler,
    /// A real-profile singleton or endpoint uses `N` or an end-relative offset.
    EndRelativeUnsupportedForReal,
}

impl Display for ProfileErrorKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::RealLiteralUnsupportedForInteger => formatter.write_str(
                "integer sequences require integer-spelled literals without decimals or exponents",
            ),
            Self::OpenEndpointUnsupportedForReal => {
                formatter.write_str("omitted endpoints are not supported for real sequences")
            }
            Self::MissingRealSampler => {
                formatter.write_str("real ranges require an explicit :step or /count sampler")
            }
            Self::EndRelativeUnsupportedForReal => {
                formatter.write_str("end-relative values are not supported for real sequences")
            }
        }
    }
}

/// Reports an expression that is syntactically valid but invalid for a numeric profile.
#[derive(Debug, Clone, Error)]
#[error("{}", self.render())]
pub struct ProfileError {
    source_text: Arc<str>,
    term_index: usize,
    span: Span,
    kind: ProfileErrorKind,
}

impl ProfileError {
    /// Creates a profile diagnostic associated with an authored term and byte span.
    pub(super) fn new(
        source: Arc<str>,
        term_index: usize,
        span: Span,
        kind: ProfileErrorKind,
    ) -> Self {
        Self {
            source_text: source,
            term_index,
            span,
            kind,
        }
    }

    /// Returns the complete authored expression.
    pub fn source_text(&self) -> &str {
        &self.source_text
    }

    /// Returns the zero-based term containing the failure.
    pub fn term_index(&self) -> usize {
        self.term_index
    }

    /// Returns the half-open UTF-8 byte span associated with the failure.
    ///
    /// The span refers to the complete authored expression returned by [`Self::source_text`].
    pub fn span(&self) -> Span {
        self.span
    }

    /// Returns the machine-readable error category.
    pub fn kind(&self) -> &ProfileErrorKind {
        &self.kind
    }

    /// Renders a bounded source excerpt with a caret and explanation.
    pub fn render(&self) -> String {
        render_diagnostic(&self.source_text, self.span, &self.kind.to_string())
    }
}

/// Combines parsing and numeric-profile validation errors.
#[derive(Debug, Clone, Error)]
pub enum ExprBuildError {
    /// The expression failed syntax parsing or a parser resource limit.
    #[error(transparent)]
    Parse(#[from] ParseError),
    /// The parsed expression failed numeric-profile validation.
    #[error(transparent)]
    Profile(#[from] ProfileError),
}

impl ExprBuildError {
    /// Returns the retained source from the underlying parse or profile error.
    ///
    /// The source normally contains the complete authored expression. An overlong-input parse
    /// error retains only the bounded prefix described by [`ParseError::source_text`].
    pub fn source_text(&self) -> &str {
        match self {
            Self::Parse(error) => error.source_text(),
            Self::Profile(error) => error.source_text(),
        }
    }

    /// Returns the underlying failure's half-open UTF-8 byte span in the original input.
    ///
    /// For an overlong-input parse error, the span may extend beyond [`Self::source_text`].
    pub fn span(&self) -> Span {
        match self {
            Self::Parse(error) => error.span(),
            Self::Profile(error) => error.span(),
        }
    }

    /// Returns the machine-readable parse or profile error category.
    pub fn kind(&self) -> ExprBuildErrorKind<'_> {
        match self {
            Self::Parse(error) => ExprBuildErrorKind::Parse(error.kind()),
            Self::Profile(error) => ExprBuildErrorKind::Profile(error.kind()),
        }
    }

    /// Renders the underlying error's bounded source excerpt, caret, and explanation.
    pub fn render(&self) -> String {
        match self {
            Self::Parse(error) => error.render(),
            Self::Profile(error) => error.render(),
        }
    }
}

/// Borrows the concrete category contained in an expression-building error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExprBuildErrorKind<'error> {
    /// Borrows the category of an underlying parse failure.
    Parse(&'error ParseErrorKind),
    /// Borrows the category of an underlying numeric-profile failure.
    Profile(&'error ProfileErrorKind),
}

/// Describes why evaluating a validated expression failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvalErrorKind {
    /// An omitted start has no supplied integer evaluation context.
    MissingOpenStartContext,
    /// An omitted stop has no supplied integer evaluation context.
    MissingOpenStopContext,
    /// An end-relative value has no explicit finite-source context.
    MissingFiniteContext,
    /// A supplied finite source length cannot be represented as an `i64` coordinate.
    InvalidFiniteExtent {
        /// The supplied source length that failed context validation.
        length: usize,
    },
    /// The exactly resolved end-relative coordinate is outside the `i64` range.
    EndRelativeOutOfRange,
    /// An ordinary integer literal is outside the `i64` range.
    IntegerOutOfRange,
    /// Checked arithmetic for an integer sequence calculation overflowed.
    IntegerOverflow,
    /// A retained-value cardinality cannot be represented by the platform index type.
    CardinalityOverflow,
    /// A retained exact real value cannot be converted to a finite `f64`.
    RealValueNotFinite,
    /// The cumulative retained-value count exceeds the effective evaluation limit.
    OutputLimitExceeded {
        /// The effective maximum number of retained values in the evaluation.
        limit: usize,
    },
}

impl Display for EvalErrorKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingOpenStartContext => {
                formatter.write_str("the open start has no supplied integer context")
            }
            Self::MissingOpenStopContext => {
                formatter.write_str("the open stop has no supplied integer context")
            }
            Self::MissingFiniteContext => {
                formatter.write_str("end-relative values require a finite source context")
            }
            Self::InvalidFiniteExtent { length } => {
                write!(
                    formatter,
                    "source length {length} cannot be represented as an i64 coordinate"
                )
            }
            Self::EndRelativeOutOfRange => {
                formatter.write_str("resolved end-relative coordinate is outside the i64 range")
            }
            Self::IntegerOutOfRange => {
                formatter.write_str("integer literal is outside the i64 range")
            }
            Self::IntegerOverflow => formatter.write_str("integer sequence calculation overflowed"),
            Self::CardinalityOverflow => formatter.write_str("sequence cardinality overflowed"),
            Self::RealValueNotFinite => {
                formatter.write_str("exact value cannot be represented as a finite f64")
            }
            Self::OutputLimitExceeded { limit } => {
                write!(formatter, "sequence exceeds the output limit of {limit} values")
            }
        }
    }
}

/// Reports a failure while evaluating a validated expression.
#[derive(Debug, Clone, Error)]
#[error("{}", self.render())]
pub struct EvalError {
    source_text: Arc<str>,
    term_index: Option<usize>,
    span: Span,
    kind: EvalErrorKind,
}

impl EvalError {
    /// Creates an evaluation diagnostic with its source, optional term, and byte span.
    pub(super) fn new(
        source: Arc<str>,
        term_index: Option<usize>,
        span: Span,
        kind: EvalErrorKind,
    ) -> Self {
        Self {
            source_text: source,
            term_index,
            span,
            kind,
        }
    }

    /// Returns the complete authored expression.
    pub fn source_text(&self) -> &str {
        &self.source_text
    }

    /// Returns the zero-based originating term, or none for a supplied-context preflight failure.
    pub fn term_index(&self) -> Option<usize> {
        self.term_index
    }

    /// Returns the half-open UTF-8 byte span associated with the failure.
    ///
    /// Term-local failures retain the relevant value or term span. A context preflight failure
    /// spans the complete authored expression and has no originating term.
    pub fn span(&self) -> Span {
        self.span
    }

    /// Returns the machine-readable error category.
    pub fn kind(&self) -> &EvalErrorKind {
        &self.kind
    }

    /// Renders a bounded source excerpt with a caret and explanation.
    pub fn render(&self) -> String {
        render_diagnostic(&self.source_text, self.span, &self.kind.to_string())
    }
}

/// Renders a bounded UTF-8-safe excerpt and caret for an authored byte span.
pub(super) fn render_diagnostic(source: &str, span: Span, message: &str) -> String {
    let mut safe_start = span.start.min(source.len());
    while !source.is_char_boundary(safe_start) {
        safe_start -= 1;
    }
    let line_start = source[..safe_start].rfind(['\n', '\r']).map_or(0, |position| position + 1);
    let line_end = source[safe_start..]
        .find(['\n', '\r'])
        .map_or(source.len(), |position| safe_start + position);
    let left_bytes: usize =
        source[line_start..safe_start].chars().rev().take(80).map(char::len_utf8).sum();
    let right_bytes: usize =
        source[safe_start..line_end].chars().take(80).map(char::len_utf8).sum();
    let visible_start = safe_start - left_bytes;
    let visible_end = safe_start + right_bytes;
    let prefix = if visible_start > line_start {
        "..."
    } else {
        ""
    };
    let suffix = if visible_end < line_end { "..." } else { "" };
    let line = &source[visible_start..visible_end];
    let caret_offset = prefix.len() + source[visible_start..safe_start].chars().count();

    format!("{prefix}{line}{suffix}\n{}^ {message}", " ".repeat(caret_offset))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_points_at_a_utf8_byte_span_by_character_column() {
        let error = ParseError::new(
            Arc::from("\u{2003}1, nope"),
            Span { start: 6, end: 7 },
            ParseErrorKind::InvalidNumber,
        );

        assert_eq!(error.render(), "\u{2003}1, nope\n    ^ invalid numeric literal");
    }

    #[test]
    fn build_error_delegates_diagnostics() {
        let error = ExprBuildError::Profile(ProfileError::new(
            Arc::from("1.0"),
            0,
            Span { start: 0, end: 3 },
            ProfileErrorKind::RealLiteralUnsupportedForInteger,
        ));

        assert_eq!(error.source_text(), "1.0");
        assert_eq!(error.span(), Span { start: 0, end: 3 });
        assert!(matches!(
            error.kind(),
            ExprBuildErrorKind::Profile(ProfileErrorKind::RealLiteralUnsupportedForInteger)
        ));
        assert!(error.render().contains("integer sequences require"));
    }

    #[test]
    fn long_unicode_lines_have_bounded_excerpts_and_caret_padding() {
        let source = format!("{}?{}", "界".repeat(10_000), "界".repeat(10_000));
        let rendered = render_diagnostic(
            &source,
            Span {
                start: 30_000,
                end: 30_001,
            },
            "bad token",
        );
        assert!(rendered.len() < 1_000);
        assert!(rendered.starts_with("..."));
        assert!(rendered.lines().nth(1).unwrap().find('^').unwrap() <= 83);
    }
}
