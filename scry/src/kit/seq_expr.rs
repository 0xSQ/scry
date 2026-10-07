//! Parses and evaluates concise integer and real-number sequence expressions.
//!
//! Expressions retain their authored text. Parsing checks syntax, and converting to [`IntSeqExpr`]
//! or [`RealSeqExpr`] checks the numeric profile. Evaluation supplies runtime context and checks
//! output representability and limits. Integer sequences produce `i64`, real sequences produce
//! finite `f64`, and index selection checks a caller-supplied source length.

#![doc = include_str!("../../docs/sequence-expressions.md")]
#![warn(missing_docs)]

mod error;
mod exact;
mod index;
mod integer;
mod parser;
mod real;
mod scry;

use std::fmt::{self, Display, Formatter};
use std::str::FromStr;
use std::sync::Arc;

use num_bigint::{BigInt, BigUint};
use num_rational::BigRational;
use num_traits::ToPrimitive;

pub use error::{
    EvalError, EvalErrorKind, ExprBuildError, ExprBuildErrorKind, ParseError, ParseErrorKind,
    ProfileError, ProfileErrorKind,
};
pub use index::{IndexError, IndexErrorKind, IndexEvaluator};
pub use integer::IntEvaluator;
pub use real::RealEvaluator;

// ---------------------------------------------------------------------------------------------- //

/// Stores a parsed sequence expression whose numeric profile has not yet been selected.
#[derive(Debug, Clone)]
pub struct SeqExpr {
    source: Arc<str>,
    terms: Vec<Term>,
}

impl SeqExpr {
    /// Parses an expression with the default resource limits.
    pub fn parse(source: &str) -> Result<Self, ParseError> {
        Self::parse_with_limits(source, ParseLimits::default())
    }

    /// Parses an expression with explicit limits bounded by the hard parser ceilings.
    pub fn parse_with_limits(source: &str, limits: ParseLimits) -> Result<Self, ParseError> {
        parser::parse(source, limits)
    }

    /// Returns the expression exactly as authored.
    pub fn source(&self) -> &str {
        &self.source
    }
}

impl FromStr for SeqExpr {
    type Err = ParseError;

    fn from_str(source: &str) -> Result<Self, Self::Err> {
        Self::parse(source)
    }
}

impl Display for SeqExpr {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.source())
    }
}

/// Stores a sequence expression validated for integer evaluation.
///
/// Validation checks integer spelling and sampler rules. Contextual values such as `N` remain
/// unresolved, and output representability is checked during evaluation.
#[derive(Debug, Clone)]
pub struct IntSeqExpr(SeqExpr);

impl IntSeqExpr {
    /// Returns the expression exactly as authored.
    pub fn source(&self) -> &str {
        self.0.source()
    }

    /// Returns the underlying profile-neutral expression.
    pub fn as_expr(&self) -> &SeqExpr {
        &self.0
    }
}

impl FromStr for IntSeqExpr {
    type Err = ExprBuildError;

    fn from_str(source: &str) -> Result<Self, Self::Err> {
        SeqExpr::parse(source)?.try_into().map_err(Into::into)
    }
}

impl TryFrom<SeqExpr> for IntSeqExpr {
    type Error = ProfileError;

    fn try_from(expression: SeqExpr) -> Result<Self, Self::Error> {
        integer::validate(&expression)?;
        Ok(Self(expression))
    }
}

impl Display for IntSeqExpr {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.source())
    }
}

/// Stores a sequence expression validated for real-number evaluation.
///
/// Validation requires explicit range endpoints and samplers and rejects end-relative values.
/// Exact literals can exceed the finite `f64` range until evaluation needs a retained output value.
#[derive(Debug, Clone)]
pub struct RealSeqExpr(SeqExpr);

impl RealSeqExpr {
    /// Returns the expression exactly as authored.
    pub fn source(&self) -> &str {
        self.0.source()
    }

    /// Returns the underlying profile-neutral expression.
    pub fn as_expr(&self) -> &SeqExpr {
        &self.0
    }
}

impl FromStr for RealSeqExpr {
    type Err = ExprBuildError;

    fn from_str(source: &str) -> Result<Self, Self::Err> {
        SeqExpr::parse(source)?.try_into().map_err(Into::into)
    }
}

impl TryFrom<SeqExpr> for RealSeqExpr {
    type Error = ProfileError;

    fn try_from(expression: SeqExpr) -> Result<Self, Self::Error> {
        real::validate(&expression)?;
        Ok(Self(expression))
    }
}

impl Display for RealSeqExpr {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.source())
    }
}

/// Identifies a half-open UTF-8 byte span in the authored expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    /// The inclusive starting byte offset.
    pub start: usize,
    /// The exclusive ending byte offset.
    pub end: usize,
}

/// Limits the work performed while parsing an expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseLimits {
    /// The input byte limit, capped at 4 MiB.
    pub max_input_bytes: usize,
    /// The digit limit for each numeric literal, end-relative offset, or count, capped at 1,024.
    pub max_literal_digits: usize,
    /// The absolute decimal exponent limit, capped at 4,096.
    pub max_abs_exponent: usize,
}

impl ParseLimits {
    /// Restricts caller-provided limits to the core ceilings.
    fn bounded(self) -> Self {
        let ceiling = Self::default();
        Self {
            max_input_bytes: self.max_input_bytes.min(ceiling.max_input_bytes),
            max_literal_digits: self.max_literal_digits.min(ceiling.max_literal_digits),
            max_abs_exponent: self.max_abs_exponent.min(ceiling.max_abs_exponent),
        }
    }
}

impl Default for ParseLimits {
    fn default() -> Self {
        Self {
            max_input_bytes: 4 * 1024 * 1024,
            max_literal_digits: 1_024,
            max_abs_exponent: 4_096,
        }
    }
}

/// Supplies numeric open bounds or a finite source for contextual integer values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntContext {
    /// Supplies omitted bounds without defining the source length `N`.
    Bounds {
        /// The numeric coordinate substituted for an omitted start.
        open_start: i64,
        /// The numeric coordinate substituted for an omitted stop.
        open_stop: i64,
    },
    /// Supplies omitted bounds as zero and length, and defines `N` as length.
    FiniteSource {
        /// The source length, which must fit `i64` even when evaluation emits no values.
        length: usize,
    },
}

/// Configures integer sequence evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IntEvalOptions {
    /// The explicit context for omitted endpoints and end-relative values, if available.
    pub context: Option<IntContext>,
    /// The cumulative retained-value limit, capped at [`MAX_VALUES`].
    pub max_values: usize,
}

impl Default for IntEvalOptions {
    fn default() -> Self {
        Self {
            context: None,
            max_values: MAX_VALUES,
        }
    }
}

/// Configures real-number sequence evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RealEvalOptions {
    /// The cumulative retained-value limit, capped at [`MAX_VALUES`].
    pub max_values: usize,
}

impl Default for RealEvalOptions {
    fn default() -> Self {
        Self {
            max_values: MAX_VALUES,
        }
    }
}

/// The maximum number of retained values for any core evaluation.
pub const MAX_VALUES: usize = 1_000_000;

struct Subdivision {
    count: usize,
    first_k: usize,
    retained: usize,
}

impl Subdivision {
    fn bounded(
        count: &BigUint,
        start: Inclusion,
        stop: Inclusion,
        remaining: usize,
    ) -> Option<Self> {
        let mut retained = count + BigUint::from(1_u8);
        if start == Inclusion::Excluded {
            retained -= 1_u8;
        }
        if stop == Inclusion::Excluded {
            retained -= 1_u8;
        }
        if retained > BigUint::from(remaining) {
            return None;
        }
        Some(Self {
            count: count.to_usize()?,
            first_k: usize::from(start == Inclusion::Excluded),
            retained: retained.to_usize()?,
        })
    }
}

// ---------------------------------------------------------------------------------------------- //
// Private Syntax Tree

#[derive(Debug, Clone)]
struct Term {
    span: Span,
    kind: TermKind,
}

#[derive(Debug, Clone)]
enum TermKind {
    Singleton(ValueExpr),
    Range(RangeExpr),
}

#[derive(Debug, Clone)]
enum ValueExpr {
    Literal(NumberLiteral),
    EndRelative { offset: BigInt, span: Span },
}

impl ValueExpr {
    fn span(&self) -> Span {
        match self {
            Self::Literal(literal) => literal.span,
            Self::EndRelative { span, .. } => *span,
        }
    }

    fn as_literal(&self) -> Option<&NumberLiteral> {
        match self {
            Self::Literal(literal) => Some(literal),
            Self::EndRelative { .. } => None,
        }
    }
}

#[derive(Debug, Clone)]
struct RangeExpr {
    start: Endpoint,
    stop: Endpoint,
    sampler: Sampler,
}

#[derive(Debug, Clone)]
struct Endpoint {
    value: Option<ValueExpr>,
    inclusion: Inclusion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Inclusion {
    Included,
    Excluded,
}

#[derive(Debug, Clone)]
enum Sampler {
    Unit,
    Step(NumberLiteral),
    Subdivide(CountLiteral),
}

#[derive(Debug, Clone)]
struct NumberLiteral {
    value: BigRational,
    form: NumberForm,
    span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NumberForm {
    Integer,
    DecimalOrExponent,
}

#[derive(Debug, Clone)]
struct CountLiteral {
    value: BigUint,
    span: Span,
}
