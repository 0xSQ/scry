use num_bigint::BigInt;
use num_traits::{Signed, ToPrimitive, Zero};

use super::error::{EvalError, EvalErrorKind, ProfileError, ProfileErrorKind};
use super::{
    Endpoint, Inclusion, IntContext, IntEvalOptions, IntSeqExpr, NumberForm, NumberLiteral,
    Sampler, SeqExpr, Span, Subdivision, TermKind, ValueExpr, MAX_VALUES,
};

// ---------------------------------------------------------------------------------------------- //

/// Evaluates [`IntSeqExpr`] values as `i64` with explicit context.
///
/// Uses [`IntEvalOptions`] for open endpoints, `N`, and output limits. For strict source indices,
/// use [`IndexEvaluator`](super::IndexEvaluator). The [`int_sequence`](super::int_sequence)
/// adapters select other native integer outputs when decoding a Scry field.
#[derive(Debug, Clone)]
pub struct IntEvaluator {
    options: IntEvalOptions,
}

impl IntEvaluator {
    /// Creates an evaluator whose output limit is capped at [`MAX_VALUES`].
    pub fn new(mut options: IntEvalOptions) -> Self {
        options.max_values = options.max_values.min(MAX_VALUES);
        Self { options }
    }

    /// Evaluates an expression into an ordered `Vec<i64>`.
    ///
    /// Preserves duplicates and returns no vector on failure. Omitted endpoints and `N` require
    /// [`IntContext`]. Endpoints, steps, and sampling stay exact. Only retained values must fit `i64`.
    pub fn evaluate(&self, expression: &IntSeqExpr) -> Result<Vec<i64>, EvalError> {
        self.evaluate_as(expression)
    }

    /// Evaluates directly into the selected native integer type.
    pub(super) fn evaluate_as<T: IntegerTarget>(
        &self,
        expression: &IntSeqExpr,
    ) -> Result<Vec<T>, EvalError> {
        let mut values = Vec::new();
        self.evaluate_each_as(expression, |value, _, _| {
            values.push(value);
            Ok::<_, EvalError>(())
        })?;
        Ok(values)
    }

    /// Visits retained `i64` values in expression order, preserving duplicates.
    ///
    /// The callback receives each value, its zero-based term index, and its source [`Span`].
    /// Uses the context and narrowing rules of [`Self::evaluate`] and checks each term's count
    /// before emitting it. Stops on the first evaluation or callback error. Earlier callbacks may
    /// already have run when a later failure occurs, and their effects are not rolled back.
    /// Collect into a temporary value or use [`Self::evaluate`] for atomic publication.
    pub fn evaluate_each<E: From<EvalError>>(
        &self,
        expression: &IntSeqExpr,
        emit: impl FnMut(i64, usize, Span) -> Result<(), E>,
    ) -> Result<(), E> {
        self.evaluate_each_as(expression, emit)
    }

    fn evaluate_each_as<T: IntegerTarget, E: From<EvalError>>(
        &self,
        expression: &IntSeqExpr,
        mut emit: impl FnMut(T, usize, Span) -> Result<(), E>,
    ) -> Result<(), E> {
        self.evaluate_exact(expression, |value, term_index, span| {
            let value = T::from_bigint(value).ok_or_else(|| {
                EvalError::new(
                    expression.0.source.clone(),
                    Some(term_index),
                    span,
                    EvalErrorKind::IntegerOutOfRange {
                        target_type: T::NAME,
                    },
                )
            })?;
            emit(value, term_index, span)
        })
    }

    fn evaluate_exact<E: From<EvalError>>(
        &self,
        expression: &IntSeqExpr,
        mut emit: impl FnMut(&BigInt, usize, Span) -> Result<(), E>,
    ) -> Result<(), E> {
        let finite_length = match self.options.context {
            Some(IntContext::FiniteSource { length }) => Some(BigInt::from(length)),
            _ => None,
        };
        let mut emitted = 0;
        for (term_index, term) in expression.0.terms.iter().enumerate() {
            let error = |kind| {
                EvalError::new(expression.0.source.clone(), Some(term_index), term.span, kind)
            };
            match &term.kind {
                TermKind::Singleton(value_expr) => {
                    let value = self.resolve_value(
                        expression,
                        term_index,
                        value_expr,
                        finite_length.as_ref(),
                    )?;
                    self.check_count(emitted, &BigInt::from(1_u8), &error)?;
                    emit(&value, term_index, value_expr.span())?;
                    emitted += 1;
                }
                TermKind::Range(range) => {
                    let start = self.resolve_endpoint(
                        expression,
                        term_index,
                        &range.start,
                        true,
                        finite_length.as_ref(),
                    )?;
                    let stop = self.resolve_endpoint(
                        expression,
                        term_index,
                        &range.stop,
                        false,
                        finite_length.as_ref(),
                    )?;
                    match &range.sampler {
                        Sampler::Unit | Sampler::Step(_) => {
                            let step = match &range.sampler {
                                Sampler::Step(literal) => literal.value.to_integer(),
                                _ => BigInt::from(1_u8),
                            };
                            let lattice = IntegerLattice::new(
                                &start,
                                &stop,
                                &step,
                                range.start.inclusion,
                                range.stop.inclusion,
                            );
                            let count = self.check_count(emitted, &lattice.count, &error)?;
                            let directed_step = if start <= stop { step } else { -step };
                            let mut value = start + &directed_step * lattice.first_k;
                            for _ in 0..count {
                                emit(&value, term_index, term.span)?;
                                value += &directed_step;
                            }
                            emitted += count;
                        }
                        Sampler::Subdivide(count) => {
                            let subdivision = Subdivision::bounded(
                                &count.value,
                                range.start.inclusion,
                                range.stop.inclusion,
                                self.options.max_values - emitted,
                            )
                            .ok_or_else(|| {
                                error(EvalErrorKind::OutputLimitExceeded {
                                    limit: self.options.max_values,
                                })
                            })?;
                            let denominator = BigInt::from(subdivision.count);
                            let difference = stop - &start;
                            let mut numerator =
                                start * &denominator + &difference * subdivision.first_k;
                            for _ in 0..subdivision.retained {
                                let value = round_anchor(&numerator, &denominator);
                                emit(&value, term_index, term.span)?;
                                numerator += &difference;
                            }
                            emitted += subdivision.retained;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn check_count(
        &self,
        current: usize,
        additional: &BigInt,
        error: &impl Fn(EvalErrorKind) -> EvalError,
    ) -> Result<usize, EvalError> {
        if additional > &BigInt::from(self.options.max_values - current) {
            return Err(error(EvalErrorKind::OutputLimitExceeded {
                limit: self.options.max_values,
            }));
        }
        Ok(additional.to_usize().expect("bounded retained cardinality fits usize"))
    }

    fn resolve_endpoint(
        &self,
        expression: &IntSeqExpr,
        term_index: usize,
        endpoint: &Endpoint,
        is_start: bool,
        finite_length: Option<&BigInt>,
    ) -> Result<BigInt, EvalError> {
        if let Some(value) = &endpoint.value {
            return self.resolve_value(expression, term_index, value, finite_length);
        }
        if let Some(length) = finite_length {
            return Ok(if is_start {
                BigInt::zero()
            } else {
                length.clone()
            });
        }
        match (self.options.context, is_start) {
            (Some(IntContext::Bounds { open_start, .. }), true) => Ok(BigInt::from(open_start)),
            (Some(IntContext::Bounds { open_stop, .. }), false) => Ok(BigInt::from(open_stop)),
            _ => Err(EvalError::new(
                expression.0.source.clone(),
                Some(term_index),
                expression.0.terms[term_index].span,
                if is_start {
                    EvalErrorKind::MissingOpenStartContext
                } else {
                    EvalErrorKind::MissingOpenStopContext
                },
            )),
        }
    }

    fn resolve_value(
        &self,
        expression: &IntSeqExpr,
        term_index: usize,
        value: &ValueExpr,
        finite_length: Option<&BigInt>,
    ) -> Result<BigInt, EvalError> {
        match value {
            ValueExpr::Literal(literal) => Ok(literal.value.to_integer()),
            ValueExpr::EndRelative { offset, span } => {
                let error = |kind| {
                    EvalError::new(expression.0.source.clone(), Some(term_index), *span, kind)
                };
                let length =
                    finite_length.ok_or_else(|| error(EvalErrorKind::MissingFiniteContext))?;
                Ok(length + offset)
            }
        }
    }
}

/// Defines the closed set of native integer outputs supported by sequence adapters.
pub(super) trait IntegerTarget: Sized {
    const NAME: &'static str;

    fn from_bigint(value: &BigInt) -> Option<Self>;
}

macro_rules! integer_targets {
    ($($target:ty => $convert:ident),+ $(,)?) => {
        $(
            impl IntegerTarget for $target {
                const NAME: &'static str = stringify!($target);

                fn from_bigint(value: &BigInt) -> Option<Self> {
                    value.$convert()
                }
            }
        )+
    };
}

integer_targets!(
    i8 => to_i8, i16 => to_i16, i32 => to_i32, i64 => to_i64, isize => to_isize,
    u8 => to_u8, u16 => to_u16, u32 => to_u32, u64 => to_u64, usize => to_usize,
);

/// Validates that every integer-profile literal uses integer spelling.
pub(super) fn validate(expression: &SeqExpr) -> Result<(), ProfileError> {
    for (term_index, term) in expression.terms.iter().enumerate() {
        let validate = |literal: &NumberLiteral| {
            if literal.form == NumberForm::DecimalOrExponent {
                Err(ProfileError::new(
                    expression.source.clone(),
                    term_index,
                    literal.span,
                    ProfileErrorKind::RealLiteralUnsupportedForInteger,
                ))
            } else {
                Ok(())
            }
        };
        match &term.kind {
            TermKind::Singleton(value) => {
                if let ValueExpr::Literal(literal) = value {
                    validate(literal)?;
                }
            }
            TermKind::Range(range) => {
                if let Some(ValueExpr::Literal(literal)) = &range.start.value {
                    validate(literal)?;
                }
                if let Some(ValueExpr::Literal(literal)) = &range.stop.value {
                    validate(literal)?;
                }
                if let Sampler::Step(literal) = &range.sampler {
                    validate(literal)?;
                }
            }
        }
    }
    Ok(())
}

struct IntegerLattice {
    first_k: usize,
    count: BigInt,
}

impl IntegerLattice {
    fn new(
        start: &BigInt,
        stop: &BigInt,
        step: &BigInt,
        left: Inclusion,
        right: Inclusion,
    ) -> Self {
        debug_assert!(step.is_positive());
        let distance = (stop - start).abs();
        let first_k = usize::from(left == Inclusion::Excluded);
        let mut last_k = &distance / step;
        if right == Inclusion::Excluded && (&distance % step).is_zero() {
            last_k -= 1_u8;
        }
        Self {
            first_k,
            count: (last_k - first_k + 1_u8).max(BigInt::zero()),
        }
    }
}

fn round_anchor(numerator: &BigInt, denominator: &BigInt) -> BigInt {
    debug_assert!(denominator.is_positive());
    let quotient = numerator / denominator;
    let twice_remainder = (numerator % denominator).abs() * 2_u8;
    if &twice_remainder < denominator {
        quotient
    } else {
        quotient + numerator.signum()
    }
}

#[cfg(test)]
mod tests;
