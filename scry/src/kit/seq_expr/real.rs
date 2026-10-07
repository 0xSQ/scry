use num_bigint::{BigInt, BigUint, Sign};
use num_rational::BigRational;
use num_traits::{One, ToPrimitive, Zero};

use super::error::{EvalError, EvalErrorKind, ProfileError, ProfileErrorKind};
use super::{
    Inclusion, RealEvalOptions, RealSeqExpr, Sampler, SeqExpr, Span, Subdivision, TermKind,
    ValueExpr, MAX_VALUES,
};

// ---------------------------------------------------------------------------------------------- //

/// Evaluates validated real-number sequence expressions.
#[derive(Debug, Clone)]
pub struct RealEvaluator {
    options: RealEvalOptions,
}

impl RealEvaluator {
    /// Creates an evaluator with the supplied output limit capped at [`MAX_VALUES`].
    pub fn new(options: RealEvalOptions) -> Self {
        Self {
            options: RealEvalOptions {
                max_values: options.max_values.min(MAX_VALUES),
            },
        }
    }

    /// Evaluates an expression exactly before converting each retained anchor once to `f64`.
    ///
    /// Uses nearest rounding with ties to even, normalizes floating zero to positive zero, and
    /// rejects non-finite retained values. Order and duplicates are preserved. Returns no vector
    /// on failure.
    pub fn evaluate(&self, expression: &RealSeqExpr) -> Result<Vec<f64>, EvalError> {
        self.evaluate_as::<f64>(expression)
    }

    /// Evaluates an expression exactly before converting each retained anchor once to `f32`.
    ///
    /// Rounds directly from the exact anchor with ties to even, without an intermediate `f64`.
    /// Normalizes floating zero to positive zero and rejects non-finite retained values.
    /// Order and duplicates are preserved. Returns no vector on failure.
    pub fn evaluate_f32(&self, expression: &RealSeqExpr) -> Result<Vec<f32>, EvalError> {
        self.evaluate_as::<f32>(expression)
    }

    fn evaluate_as<T: RealTarget>(&self, expression: &RealSeqExpr) -> Result<Vec<T>, EvalError> {
        let mut values = Vec::new();
        for (term_index, term) in expression.0.terms.iter().enumerate() {
            match &term.kind {
                TermKind::Singleton(value) => {
                    let literal = value.as_literal().expect("real profile rejects symbolic values");
                    self.ensure_output_budget(expression, term_index, term.span, values.len(), 1)?;
                    values.push(real_value(expression, term_index, literal.span, &literal.value)?);
                }
                TermKind::Range(range) => {
                    let start = &range
                        .start
                        .value
                        .as_ref()
                        .expect("real profile rejects open starts")
                        .as_literal()
                        .expect("real profile rejects symbolic values")
                        .value;
                    let stop = &range
                        .stop
                        .value
                        .as_ref()
                        .expect("real profile rejects open stops")
                        .as_literal()
                        .expect("real profile rejects symbolic values")
                        .value;
                    match &range.sampler {
                        Sampler::Unit => unreachable!("real profile rejects omitted samplers"),
                        Sampler::Step(step) => self.append_fixed_step(
                            expression,
                            term_index,
                            term.span,
                            start,
                            stop,
                            &step.value,
                            range.start.inclusion,
                            range.stop.inclusion,
                            &mut values,
                        )?,
                        Sampler::Subdivide(count) => self.append_subdivision(
                            expression,
                            term_index,
                            term.span,
                            start,
                            stop,
                            &count.value,
                            range.start.inclusion,
                            range.stop.inclusion,
                            &mut values,
                        )?,
                    }
                }
            }
        }
        Ok(values)
    }

    #[allow(clippy::too_many_arguments)]
    fn append_fixed_step<T: RealTarget>(
        &self,
        expression: &RealSeqExpr,
        term_index: usize,
        span: Span,
        start: &BigRational,
        stop: &BigRational,
        step: &BigRational,
        start_inclusion: Inclusion,
        stop_inclusion: Inclusion,
        values: &mut Vec<T>,
    ) -> Result<(), EvalError> {
        if start == stop {
            let count = usize::from(
                start_inclusion == Inclusion::Included && stop_inclusion == Inclusion::Included,
            );
            self.ensure_output_budget(expression, term_index, span, values.len(), count)?;
            if count == 1 {
                values.push(real_value(expression, term_index, span, start)?);
            }
            return Ok(());
        }

        let ascending = start < stop;
        let distance = if ascending {
            stop - start
        } else {
            start - stop
        };
        let ratio = distance / step;
        let first_k = if start_inclusion == Inclusion::Included {
            BigInt::zero()
        } else {
            BigInt::one()
        };
        let mut last_k = ratio.to_integer();
        if stop_inclusion == Inclusion::Excluded && ratio.is_integer() {
            last_k -= 1;
        }
        if last_k < first_k {
            return Ok(());
        }

        let exact_count = &last_k - &first_k + BigInt::one();
        if exact_count > BigInt::from(self.options.max_values - values.len()) {
            return Err(self.output_limit_error(expression, term_index, span));
        }
        let count = exact_count
            .to_usize()
            .ok_or_else(|| self.output_limit_error(expression, term_index, span))?;
        self.ensure_output_budget(expression, term_index, span, values.len(), count)?;
        values.reserve(count);

        for offset in 0..count {
            let k = &first_k + BigInt::from(offset);
            let displacement = step * BigRational::from_integer(k);
            let candidate = if ascending {
                start + displacement
            } else {
                start - displacement
            };
            values.push(real_value(expression, term_index, span, &candidate)?);
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn append_subdivision<T: RealTarget>(
        &self,
        expression: &RealSeqExpr,
        term_index: usize,
        span: Span,
        start: &BigRational,
        stop: &BigRational,
        count: &BigUint,
        start_inclusion: Inclusion,
        stop_inclusion: Inclusion,
        values: &mut Vec<T>,
    ) -> Result<(), EvalError> {
        let subdivision = Subdivision::bounded(
            count,
            start_inclusion,
            stop_inclusion,
            self.options.max_values - values.len(),
        )
        .ok_or_else(|| self.output_limit_error(expression, term_index, span))?;
        if subdivision.retained == 0 {
            return Ok(());
        }
        let delta = stop - start;
        values.reserve(subdivision.retained);
        for k in subdivision.first_k..subdivision.first_k + subdivision.retained {
            let fraction = BigRational::new(BigInt::from(k), BigInt::from(subdivision.count));
            let candidate = start + &delta * fraction;
            values.push(real_value(expression, term_index, span, &candidate)?);
        }
        Ok(())
    }

    fn ensure_output_budget(
        &self,
        expression: &RealSeqExpr,
        term_index: usize,
        span: Span,
        current: usize,
        additional: usize,
    ) -> Result<(), EvalError> {
        let total = current
            .checked_add(additional)
            .ok_or_else(|| self.output_limit_error(expression, term_index, span))?;
        if total > self.options.max_values {
            return Err(self.output_limit_error(expression, term_index, span));
        }
        Ok(())
    }

    fn output_limit_error(
        &self,
        expression: &RealSeqExpr,
        term_index: usize,
        span: Span,
    ) -> EvalError {
        eval_error(
            expression,
            term_index,
            span,
            EvalErrorKind::OutputLimitExceeded {
                limit: self.options.max_values,
            },
        )
    }
}

pub(super) fn validate(expression: &SeqExpr) -> Result<(), ProfileError> {
    for (term_index, term) in expression.terms.iter().enumerate() {
        let validate_value = |value: &ValueExpr| {
            if matches!(value, ValueExpr::EndRelative { .. }) {
                Err(ProfileError::new(
                    expression.source.clone(),
                    term_index,
                    value.span(),
                    ProfileErrorKind::EndRelativeUnsupportedForReal,
                ))
            } else {
                Ok(())
            }
        };
        if let TermKind::Singleton(value) = &term.kind {
            validate_value(value)?;
        }
        let TermKind::Range(range) = &term.kind else {
            continue;
        };
        for value in [&range.start.value, &range.stop.value].into_iter().flatten() {
            validate_value(value)?;
        }
        if range.start.value.is_none() {
            return Err(ProfileError::new(
                expression.source.clone(),
                term_index,
                term.span,
                ProfileErrorKind::OpenEndpointUnsupportedForReal,
            ));
        }
        if range.stop.value.is_none() {
            return Err(ProfileError::new(
                expression.source.clone(),
                term_index,
                term.span,
                ProfileErrorKind::OpenEndpointUnsupportedForReal,
            ));
        }
        if matches!(range.sampler, Sampler::Unit) {
            return Err(ProfileError::new(
                expression.source.clone(),
                term_index,
                term.span,
                ProfileErrorKind::MissingRealSampler,
            ));
        }
    }
    Ok(())
}

fn real_value<T: RealTarget>(
    expression: &RealSeqExpr,
    term_index: usize,
    span: Span,
    value: &BigRational,
) -> Result<T, EvalError> {
    T::from_rational(value).ok_or_else(|| {
        eval_error(
            expression,
            term_index,
            span,
            EvalErrorKind::RealValueNotFinite {
                target_type: T::TYPE_NAME,
            },
        )
    })
}

trait RealTarget: Sized {
    const TYPE_NAME: &'static str;

    fn from_rational(value: &BigRational) -> Option<Self>;
}

impl RealTarget for f64 {
    const TYPE_NAME: &'static str = "f64";

    fn from_rational(value: &BigRational) -> Option<Self> {
        value.to_f64().filter(|converted| converted.is_finite()).map(|converted| {
            if converted == 0.0 {
                0.0
            } else {
                converted
            }
        })
    }
}

impl RealTarget for f32 {
    const TYPE_NAME: &'static str = "f32";

    fn from_rational(value: &BigRational) -> Option<Self> {
        rational_to_f32(value)
    }
}

fn rational_to_f32(value: &BigRational) -> Option<f32> {
    let numerator = value.numer().magnitude();
    if numerator.is_zero() {
        return Some(0.0);
    }
    let denominator = value.denom().magnitude();
    let mut exponent =
        i64::try_from(numerator.bits()).ok()? - i64::try_from(denominator.bits()).ok()?;

    // The exact exponent is this bit-length difference or one less. Avoid scaling huge values.
    if exponent > 128 {
        return None;
    }
    if exponent < -150 {
        return Some(0.0);
    }
    let below_power = if exponent >= 0 {
        numerator < &(denominator << usize::try_from(exponent).ok()?)
    } else {
        &(numerator << usize::try_from(-exponent).ok()?) < denominator
    };
    if below_power {
        exponent -= 1;
    }
    if exponent > 127 {
        return None;
    }

    // Normal values retain 24 significand bits. Subnormals use the fixed quantum 2^-149.
    let shift = 23 - exponent.max(-126);
    let (scaled_numerator, scaled_denominator) = if shift >= 0 {
        (numerator << usize::try_from(shift).ok()?, denominator.clone())
    } else {
        (numerator.clone(), denominator << usize::try_from(-shift).ok()?)
    };
    let mut significand = &scaled_numerator / &scaled_denominator;
    let twice_remainder = (&scaled_numerator % &scaled_denominator) << 1_usize;
    if twice_remainder > scaled_denominator
        || (twice_remainder == scaled_denominator && significand.bit(0))
    {
        significand += 1_u8;
    }
    let mut significand = significand.to_u32()?;
    if significand == 0 {
        return Some(0.0);
    }

    let magnitude = if exponent < -126 {
        // Rounding can reach the smallest normal value. Its raw bits equal this significand too.
        significand
    } else {
        if significand == 1 << 24 {
            significand >>= 1;
            exponent += 1;
        }
        if exponent > 127 {
            return None;
        }
        let biased_exponent = u32::try_from(exponent + 127).ok()?;
        (biased_exponent << 23) | (significand - (1 << 23))
    };
    let sign = if value.numer().sign() == Sign::Minus {
        1 << 31
    } else {
        0
    };
    Some(f32::from_bits(sign | magnitude))
}

fn eval_error(
    expression: &RealSeqExpr,
    term_index: usize,
    span: Span,
    kind: EvalErrorKind,
) -> EvalError {
    EvalError::new(expression.0.source.clone(), Some(term_index), span, kind)
}

#[cfg(test)]
mod tests;
