use num_bigint::BigInt;
use num_traits::ToPrimitive;

use super::error::{EvalError, EvalErrorKind, ProfileError, ProfileErrorKind};
use super::{
    Endpoint, Inclusion, IntContext, IntEvalOptions, IntSeqExpr, NumberForm, NumberLiteral,
    Sampler, SeqExpr, Span, Subdivision, TermKind, ValueExpr, MAX_VALUES,
};

// ---------------------------------------------------------------------------------------------- //

/// Evaluates integer expressions with finite contextual bounds and checked arithmetic.
#[derive(Debug, Clone)]
pub struct IntEvaluator {
    options: IntEvalOptions,
}

impl IntEvaluator {
    /// Creates an evaluator with the supplied context and bounded output limit.
    pub fn new(mut options: IntEvalOptions) -> Self {
        options.max_values = options.max_values.min(MAX_VALUES);
        Self { options }
    }

    /// Evaluates an expression into an ordered vector, preserving duplicate values.
    ///
    /// Returns no vector on failure. Omitted endpoints and end-relative values require the
    /// appropriate explicit context, and retained values must fit `i64`.
    pub fn evaluate(&self, expression: &IntSeqExpr) -> Result<Vec<i64>, EvalError> {
        let mut values = Vec::new();
        self.evaluate_each(expression, |value, _, _| {
            values.push(value);
            Ok::<_, EvalError>(())
        })?;
        Ok(values)
    }

    /// Visits bounded integer candidates while retaining their diagnostic source association.
    ///
    /// The callback receives the value, its zero-based term index, and its authored byte span.
    /// Evaluation preserves order and duplicates and stops on the first evaluation or callback
    /// error. Earlier callbacks may already have run when a later term fails. Collect into a
    /// temporary value or use [`Self::evaluate`] when results must be published atomically.
    pub fn evaluate_each<E: From<EvalError>>(
        &self,
        expression: &IntSeqExpr,
        mut emit: impl FnMut(i64, usize, Span) -> Result<(), E>,
    ) -> Result<(), E> {
        let finite_length = match self.options.context {
            Some(IntContext::FiniteSource { length }) => {
                Some(i64::try_from(length).map_err(|_| {
                    EvalError::new(
                        expression.0.source.clone(),
                        None,
                        Span {
                            start: 0,
                            end: expression.source().len(),
                        },
                        EvalErrorKind::InvalidFiniteExtent { length },
                    )
                })?)
            }
            _ => None,
        };
        let mut emitted = 0;
        for (term_index, term) in expression.0.terms.iter().enumerate() {
            let error = |kind| {
                EvalError::new(expression.0.source.clone(), Some(term_index), term.span, kind)
            };
            match &term.kind {
                TermKind::Singleton(value_expr) => {
                    let value =
                        self.resolve_value(expression, term_index, value_expr, finite_length)?;
                    self.check_count(emitted, 1, &error)?;
                    emit(value, term_index, value_expr.span())?;
                    emitted += 1;
                }
                TermKind::Range(range) => {
                    let start = self.resolve_endpoint(
                        expression,
                        term_index,
                        &range.start,
                        true,
                        finite_length,
                    )?;
                    let stop = self.resolve_endpoint(
                        expression,
                        term_index,
                        &range.stop,
                        false,
                        finite_length,
                    )?;
                    match &range.sampler {
                        Sampler::Unit | Sampler::Step(_) => {
                            let step = match &range.sampler {
                                Sampler::Step(literal) => {
                                    integer_value(expression, term_index, literal)?
                                }
                                _ => 1,
                            };
                            let lattice = IntegerLattice::new(
                                start,
                                stop,
                                step,
                                range.start.inclusion,
                                range.stop.inclusion,
                            );
                            let count = self.check_count(emitted, lattice.count, &error)?;
                            for offset in 0..count {
                                let k = lattice.first_k + offset as i128;
                                let value = k
                                    .checked_mul(i128::from(step))
                                    .and_then(|value| value.checked_mul(lattice.direction))
                                    .and_then(|value| i128::from(start).checked_add(value))
                                    .and_then(|value| i64::try_from(value).ok())
                                    .ok_or_else(|| error(EvalErrorKind::IntegerOverflow))?;
                                emit(value, term_index, term.span)?;
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
                            let denominator = subdivision.count as i128;
                            let difference = i128::from(stop) - i128::from(start);
                            let base = i128::from(start)
                                .checked_mul(denominator)
                                .ok_or_else(|| error(EvalErrorKind::IntegerOverflow))?;
                            for k in subdivision.first_k..subdivision.first_k + subdivision.retained
                            {
                                let numerator = difference
                                    .checked_mul(k as i128)
                                    .and_then(|value| base.checked_add(value))
                                    .ok_or_else(|| error(EvalErrorKind::IntegerOverflow))?;
                                let value = round_anchor(numerator, denominator)
                                    .and_then(|value| i64::try_from(value).ok())
                                    .ok_or_else(|| error(EvalErrorKind::IntegerOverflow))?;
                                emit(value, term_index, term.span)?;
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
        additional: i128,
        error: &impl Fn(EvalErrorKind) -> EvalError,
    ) -> Result<usize, EvalError> {
        if additional > (self.options.max_values - current) as i128 {
            return Err(error(EvalErrorKind::OutputLimitExceeded {
                limit: self.options.max_values,
            }));
        }
        usize::try_from(additional).map_err(|_| error(EvalErrorKind::CardinalityOverflow))
    }

    fn resolve_endpoint(
        &self,
        expression: &IntSeqExpr,
        term_index: usize,
        endpoint: &Endpoint,
        is_start: bool,
        finite_length: Option<i64>,
    ) -> Result<i64, EvalError> {
        if let Some(value) = &endpoint.value {
            return self.resolve_value(expression, term_index, value, finite_length);
        }
        if let Some(length) = finite_length {
            return Ok(if is_start { 0 } else { length });
        }
        match (self.options.context, is_start) {
            (Some(IntContext::Bounds { open_start, .. }), true) => Ok(open_start),
            (Some(IntContext::Bounds { open_stop, .. }), false) => Ok(open_stop),
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
        finite_length: Option<i64>,
    ) -> Result<i64, EvalError> {
        match value {
            ValueExpr::Literal(literal) => integer_value(expression, term_index, literal),
            ValueExpr::EndRelative { offset, span } => {
                let error = |kind| {
                    EvalError::new(expression.0.source.clone(), Some(term_index), *span, kind)
                };
                let length =
                    finite_length.ok_or_else(|| error(EvalErrorKind::MissingFiniteContext))?;
                (BigInt::from(length) + offset)
                    .to_i64()
                    .ok_or_else(|| error(EvalErrorKind::EndRelativeOutOfRange))
            }
        }
    }
}

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
    first_k: i128,
    count: i128,
    direction: i128,
}

impl IntegerLattice {
    fn new(start: i64, stop: i64, step: i64, left: Inclusion, right: Inclusion) -> Self {
        debug_assert!(step > 0);
        let distance = (i128::from(stop) - i128::from(start)).abs();
        let step = i128::from(step);
        let first_k = i128::from(left == Inclusion::Excluded);
        let mut last_k = distance / step;
        if right == Inclusion::Excluded && distance % step == 0 {
            last_k -= 1;
        }
        Self {
            first_k,
            count: (last_k - first_k + 1).max(0),
            direction: if start <= stop { 1 } else { -1 },
        }
    }
}

fn integer_value(
    expression: &IntSeqExpr,
    term_index: usize,
    literal: &NumberLiteral,
) -> Result<i64, EvalError> {
    literal.value.to_integer().to_i64().ok_or_else(|| {
        EvalError::new(
            expression.0.source.clone(),
            Some(term_index),
            literal.span,
            EvalErrorKind::IntegerOutOfRange,
        )
    })
}

fn round_anchor(numerator: i128, denominator: i128) -> Option<i128> {
    let quotient = numerator.checked_div(denominator)?;
    let remainder = numerator.checked_rem(denominator)?;
    if remainder.checked_abs()?.checked_mul(2)? < denominator {
        Some(quotient)
    } else {
        quotient.checked_add(numerator.signum())
    }
}

#[cfg(test)]
mod tests;
