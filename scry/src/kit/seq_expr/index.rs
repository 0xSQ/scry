use std::sync::Arc;

use thiserror::Error;

use super::error::{render_diagnostic, EvalError, EvalErrorKind};
use super::{IntContext, IntEvalOptions, IntEvaluator, IntSeqExpr, Span, MAX_VALUES};

// ---------------------------------------------------------------------------------------------- //

/// Evaluates [`IntSeqExpr`] values as strict indices into a finite source.
#[derive(Debug, Clone, Copy)]
pub struct IndexEvaluator {
    max_values: usize,
}

impl IndexEvaluator {
    /// Creates an index evaluator whose output limit is capped at [`MAX_VALUES`].
    pub fn new(max_values: usize) -> Self {
        Self {
            max_values: max_values.min(MAX_VALUES),
        }
    }

    /// Returns every selected occurrence or fails on the first invalid emitted index.
    ///
    /// Supplies [`IntContext::FiniteSource`]. Defines `N` as `length` and fills omitted endpoints
    /// with zero and `length`. The length must fit `i64`, including for empty results. Every emitted
    /// or rounded index must satisfy `0 <= index < length`. Selection preserves order and duplicates
    /// and never clips indices.
    pub fn evaluate(
        &self,
        expression: &IntSeqExpr,
        length: usize,
    ) -> Result<Vec<usize>, IndexError> {
        i64::try_from(length).map_err(|_| IndexError {
            source_text: expression.0.source.clone(),
            term_index: None,
            span: Span {
                start: 0,
                end: expression.source().len(),
            },
            kind: IndexErrorKind::InvalidExtent { length },
        })?;
        let evaluator = IntEvaluator::new(IntEvalOptions {
            context: Some(IntContext::FiniteSource { length }),
            max_values: self.max_values,
        });
        let mut indices = Vec::new();
        evaluator.evaluate_each(expression, |value, term_index, span| {
            let index =
                usize::try_from(value).ok().filter(|&index| index < length).ok_or_else(|| {
                    IndexError {
                        source_text: expression.0.source.clone(),
                        term_index: Some(term_index),
                        span,
                        kind: IndexErrorKind::OutOfBounds {
                            index: value,
                            length,
                        },
                    }
                })?;
            indices.push(index);
            Ok::<_, IndexError>(())
        })?;
        Ok(indices)
    }
}

impl Default for IndexEvaluator {
    fn default() -> Self {
        Self::new(MAX_VALUES)
    }
}

/// Reports why an integer expression cannot select indices from its supplied extent.
///
/// Underlying evaluation failures retain their kind, expression text, span, and term association.
/// They do not retain the original [`EvalError`] as an [`std::error::Error::source`] cause.
#[derive(Debug, Clone, Error)]
#[error("{}", self.render())]
pub struct IndexError {
    source_text: Arc<str>,
    term_index: Option<usize>,
    span: Span,
    kind: IndexErrorKind,
}

impl IndexError {
    /// Returns the authored expression.
    pub fn source_text(&self) -> &str {
        &self.source_text
    }

    /// Returns the originating term, or none when the extent itself is invalid.
    pub fn term_index(&self) -> Option<usize> {
        self.term_index
    }

    /// Returns the half-open UTF-8 byte [`Span`] associated with the failure.
    pub fn span(&self) -> Span {
        self.span
    }

    /// Returns the machine-readable error category.
    pub fn kind(&self) -> &IndexErrorKind {
        &self.kind
    }

    /// Renders a bounded excerpt with a caret and explanation.
    pub fn render(&self) -> String {
        render_diagnostic(&self.source_text, self.span, &self.kind.to_string())
    }
}

impl From<EvalError> for IndexError {
    fn from(error: EvalError) -> Self {
        Self {
            source_text: Arc::from(error.source_text()),
            term_index: error.term_index(),
            span: error.span(),
            kind: IndexErrorKind::Evaluation(error.kind().clone()),
        }
    }
}

/// Describes an invalid extent, emitted index, or underlying numeric evaluation.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum IndexErrorKind {
    /// The source length exceeds `i64::MAX`.
    #[error("source length {length} cannot be represented as an i64 coordinate")]
    InvalidExtent {
        /// The supplied source length.
        length: usize,
    },
    /// An emitted or rounded index is outside the supplied source.
    #[error("index {index} is outside 0..{length}")]
    OutOfBounds {
        /// The invalid emitted index.
        index: i64,
        /// The supplied source length.
        length: usize,
    },
    /// Numeric evaluation failed before a valid index could be produced.
    #[error("{0}")]
    Evaluation(EvalErrorKind),
}

#[cfg(test)]
mod tests;
