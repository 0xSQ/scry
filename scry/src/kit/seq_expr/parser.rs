use std::sync::Arc;

use num_bigint::{BigInt, BigUint};
use num_traits::Zero;

use super::error::{ParseError, ParseErrorKind};
use super::exact::parse_rational;
use super::{
    CountLiteral, Endpoint, Inclusion, NumberForm, NumberLiteral, ParseLimits, RangeExpr, Sampler,
    SeqExpr, Span, Term, TermKind, ValueExpr,
};

// ---------------------------------------------------------------------------------------------- //

pub(super) fn parse(source: &str, limits: ParseLimits) -> Result<SeqExpr, ParseError> {
    let limits = limits.bounded();
    if source.len() > limits.max_input_bytes {
        let mut limit_boundary = limits.max_input_bytes.min(source.len());
        while !source.is_char_boundary(limit_boundary) {
            limit_boundary -= 1;
        }
        let mut excerpt_end = source.len().min(160);
        while !source.is_char_boundary(excerpt_end) {
            excerpt_end -= 1;
        }
        return Err(ParseError::new(
            Arc::from(&source[..excerpt_end]),
            Span {
                start: limit_boundary,
                end: source.len(),
            },
            ParseErrorKind::InputTooLong {
                limit: limits.max_input_bytes,
                actual: source.len(),
            },
        ));
    }
    let shared: Arc<str> = Arc::from(source);
    Parser::new(source, shared, limits).parse()
}

struct Parser<'source> {
    source: &'source str,
    shared: Arc<str>,
    position: usize,
    limits: ParseLimits,
}

impl<'source> Parser<'source> {
    fn new(source: &'source str, shared: Arc<str>, limits: ParseLimits) -> Self {
        Self {
            source,
            shared,
            position: 0,
            limits,
        }
    }

    fn parse(mut self) -> Result<SeqExpr, ParseError> {
        self.skip_whitespace();
        if self.is_end() {
            return Err(self.error(
                Span {
                    start: 0,
                    end: self.source.len(),
                },
                ParseErrorKind::EmptyInput,
            ));
        }

        let mut terms = Vec::new();
        loop {
            terms.push(self.parse_term()?);
            self.skip_whitespace();

            if self.is_end() {
                break;
            }
            if self.peek() != Some(',') {
                return Err(self.error(
                    self.current_span(),
                    ParseErrorKind::UnexpectedToken {
                        expected: "a comma or the end of the expression",
                    },
                ));
            }

            self.bump();
            self.skip_whitespace();
            if self.is_end() || self.peek() == Some(',') {
                return Err(self.error(self.current_span(), ParseErrorKind::EmptyTerm));
            }
        }

        Ok(SeqExpr {
            source: self.shared,
            terms,
        })
    }

    fn parse_term(&mut self) -> Result<Term, ParseError> {
        let start = self.position;
        let kind = match self.peek() {
            Some('[' | '(') => TermKind::Range(self.parse_bracketed_range()?),
            _ => self.parse_bare_term()?,
        };
        let end = self.position;

        Ok(Term {
            span: Span { start, end },
            kind,
        })
    }

    fn parse_bracketed_range(&mut self) -> Result<RangeExpr, ParseError> {
        let start_inclusion = match self.bump() {
            Some('[') => Inclusion::Included,
            Some('(') => Inclusion::Excluded,
            _ => unreachable!("caller verifies the opening delimiter"),
        };
        self.skip_whitespace();

        let start = if self.starts_with("..") {
            None
        } else {
            Some(self.parse_value()?)
        };
        self.skip_whitespace();
        self.expect_range_operator()?;
        self.skip_whitespace();

        let stop = if matches!(self.peek(), Some(']' | ')')) {
            None
        } else {
            Some(self.parse_value()?)
        };
        self.skip_whitespace();
        let stop_inclusion = match self.bump() {
            Some(']') => Inclusion::Included,
            Some(')') => Inclusion::Excluded,
            _ => {
                return Err(
                    self.error(self.current_span(), ParseErrorKind::MissingEndpointDelimiter)
                );
            }
        };

        self.skip_whitespace();
        let sampler = self.parse_sampler()?;
        Ok(RangeExpr {
            start: Endpoint {
                value: start,
                inclusion: start_inclusion,
            },
            stop: Endpoint {
                value: stop,
                inclusion: stop_inclusion,
            },
            sampler,
        })
    }

    fn parse_bare_term(&mut self) -> Result<TermKind, ParseError> {
        let start = if self.starts_with("..") {
            None
        } else {
            Some(self.parse_value()?)
        };
        self.skip_whitespace();

        if !self.starts_with("..") {
            return Ok(TermKind::Singleton(start.expect("a bare singleton always parsed a value")));
        }

        self.expect_range_operator()?;
        self.skip_whitespace();
        let stop = if self.is_end() || matches!(self.peek(), Some(',' | ':' | '/')) {
            None
        } else {
            Some(self.parse_value()?)
        };
        self.skip_whitespace();
        let sampler = self.parse_sampler()?;

        Ok(TermKind::Range(RangeExpr {
            start: Endpoint {
                value: start,
                inclusion: Inclusion::Included,
            },
            stop: Endpoint {
                value: stop,
                inclusion: Inclusion::Excluded,
            },
            sampler,
        }))
    }

    fn parse_sampler(&mut self) -> Result<Sampler, ParseError> {
        match self.peek() {
            Some(':') => {
                self.bump();
                self.skip_whitespace();
                let step = self.parse_number()?;
                if step.value <= num_rational::BigRational::zero() {
                    return Err(self.error(step.span, ParseErrorKind::NonPositiveStep));
                }
                Ok(Sampler::Step(step))
            }
            Some('/') => {
                self.bump();
                self.skip_whitespace();
                let count = self.parse_count()?;
                if count.value.is_zero() {
                    return Err(self.error(count.span, ParseErrorKind::NonPositiveSubdivisionCount));
                }
                Ok(Sampler::Subdivide(count))
            }
            _ => Ok(Sampler::Unit),
        }
    }

    fn parse_value(&mut self) -> Result<ValueExpr, ParseError> {
        if self.peek() == Some('N') {
            self.parse_end_relative()
        } else {
            self.parse_number().map(ValueExpr::Literal)
        }
    }

    fn parse_end_relative(&mut self) -> Result<ValueExpr, ParseError> {
        let start = self.position;
        self.bump();
        let bare_end = self.position;
        self.skip_whitespace();

        let Some(operator @ ('+' | '-')) = self.peek() else {
            return Ok(ValueExpr::EndRelative {
                offset: BigInt::zero(),
                span: Span {
                    start,
                    end: bare_end,
                },
            });
        };

        let operator_start = self.position;
        self.bump();
        self.skip_whitespace();
        let magnitude_start = self.position;
        let digits = self.scan_digits();
        if digits == 0 {
            let span = if self.is_end() {
                Span {
                    start: operator_start,
                    end: self.position,
                }
            } else {
                self.current_span()
            };
            return Err(self.error(span, ParseErrorKind::InvalidEndRelativeOffset));
        }
        if digits > self.limits.max_literal_digits {
            return Err(self.error(
                self.span_from(magnitude_start),
                ParseErrorKind::LiteralDigitLimitExceeded {
                    limit: self.limits.max_literal_digits,
                },
            ));
        }

        let magnitude =
            BigInt::parse_bytes(&self.source.as_bytes()[magnitude_start..self.position], 10)
                .expect("a bounded ASCII digit token is a valid integer");
        Ok(ValueExpr::EndRelative {
            offset: if operator == '-' {
                -magnitude
            } else {
                magnitude
            },
            span: self.span_from(start),
        })
    }

    fn parse_number(&mut self) -> Result<NumberLiteral, ParseError> {
        let start = self.position;
        if matches!(self.peek(), Some('+' | '-')) {
            self.bump();
        }

        let whole_digits = self.scan_digits();
        if whole_digits == 0 {
            return Err(self.error(self.invalid_number_span(start), ParseErrorKind::InvalidNumber));
        }

        let mut decimal = false;
        let mut fractional_digits = 0;
        if self.peek() == Some('.') && !self.starts_with("..") {
            decimal = true;
            self.bump();
            fractional_digits = self.scan_digits();
            if fractional_digits == 0 {
                return Err(self.error(self.span_from(start), ParseErrorKind::InvalidNumber));
            }
        }

        let significand_digits = whole_digits.saturating_add(fractional_digits);
        if significand_digits > self.limits.max_literal_digits {
            return Err(self.error(
                self.span_from(start),
                ParseErrorKind::LiteralDigitLimitExceeded {
                    limit: self.limits.max_literal_digits,
                },
            ));
        }

        let mut exponent = false;
        if matches!(self.peek(), Some('e' | 'E')) {
            exponent = true;
            self.bump();
            if matches!(self.peek(), Some('+' | '-')) {
                self.bump();
            }
            let exponent_start = self.position;
            let exponent_digits = self.scan_digits();
            if exponent_digits == 0 {
                return Err(self.error(self.span_from(start), ParseErrorKind::InvalidNumber));
            }
            if significand_digits.saturating_add(exponent_digits) > self.limits.max_literal_digits {
                return Err(self.error(
                    self.span_from(start),
                    ParseErrorKind::LiteralDigitLimitExceeded {
                        limit: self.limits.max_literal_digits,
                    },
                ));
            }
            if self.exponent_exceeds_limit(exponent_start, self.position) {
                return Err(self.error(
                    Span {
                        start: exponent_start,
                        end: self.position,
                    },
                    ParseErrorKind::ExponentLimitExceeded {
                        limit: self.limits.max_abs_exponent,
                    },
                ));
            }
        }

        let span = self.span_from(start);
        let value = parse_rational(&self.source[span.start..span.end])
            .ok_or_else(|| self.error(span, ParseErrorKind::InvalidNumber))?;

        Ok(NumberLiteral {
            value,
            form: if decimal || exponent {
                NumberForm::DecimalOrExponent
            } else {
                NumberForm::Integer
            },
            span,
        })
    }

    fn parse_count(&mut self) -> Result<CountLiteral, ParseError> {
        let start = self.position;
        let digits = self.scan_digits();
        if digits == 0 {
            return Err(self.error(self.span_from(start), ParseErrorKind::InvalidNumber));
        }
        if digits > self.limits.max_literal_digits {
            return Err(self.error(
                self.span_from(start),
                ParseErrorKind::LiteralDigitLimitExceeded {
                    limit: self.limits.max_literal_digits,
                },
            ));
        }

        let span = self.span_from(start);
        let value = BigUint::parse_bytes(&self.source.as_bytes()[span.start..span.end], 10)
            .ok_or_else(|| self.error(span, ParseErrorKind::InvalidNumber))?;
        Ok(CountLiteral { value, span })
    }

    fn expect_range_operator(&mut self) -> Result<(), ParseError> {
        if self.starts_with("..") {
            self.position += 2;
            Ok(())
        } else if self.is_end() {
            Err(self.error(self.current_span(), ParseErrorKind::UnexpectedEnd { expected: "`..`" }))
        } else {
            Err(self
                .error(self.current_span(), ParseErrorKind::UnexpectedToken { expected: "`..`" }))
        }
    }

    fn exponent_exceeds_limit(&self, start: usize, end: usize) -> bool {
        let mut value = 0_usize;
        for byte in self.source.as_bytes()[start..end].iter().copied() {
            let digit = usize::from(byte - b'0');
            match value.checked_mul(10).and_then(|value| value.checked_add(digit)) {
                Some(next) if next <= self.limits.max_abs_exponent => value = next,
                _ => return true,
            }
        }
        false
    }

    fn scan_digits(&mut self) -> usize {
        let start = self.position;
        while matches!(self.peek(), Some('0'..='9')) {
            self.bump();
        }
        self.position - start
    }

    fn skip_whitespace(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.bump();
        }
    }

    fn starts_with(&self, pattern: &str) -> bool {
        self.source[self.position..].starts_with(pattern)
    }

    fn peek(&self) -> Option<char> {
        self.source[self.position..].chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let character = self.peek()?;
        self.position += character.len_utf8();
        Some(character)
    }

    fn is_end(&self) -> bool {
        self.position == self.source.len()
    }

    fn span_from(&self, start: usize) -> Span {
        Span {
            start,
            end: self.position.max(start),
        }
    }

    fn current_span(&self) -> Span {
        let end =
            self.peek().map_or(self.position, |character| self.position + character.len_utf8());
        Span {
            start: self.position,
            end,
        }
    }

    fn invalid_number_span(&self, start: usize) -> Span {
        let end =
            self.peek().map_or(self.position, |character| self.position + character.len_utf8());
        Span {
            start,
            end: end.max(self.position),
        }
    }

    fn error(&self, span: Span, kind: ParseErrorKind) -> ParseError {
        ParseError::new(Arc::clone(&self.shared), span, kind)
    }
}

#[cfg(test)]
mod tests;
