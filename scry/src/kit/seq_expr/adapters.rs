//! Expands expression strings or decodes arrays into native numeric vectors.

use super::MAX_VALUES;
use super::{
    IntEvalOptions, IntEvaluator, IntSeqExpr, RealEvalOptions, RealEvaluator, RealSeqExpr,
};
use crate::convert::{read, write};
use crate::node::{Kind, Value};
use crate::{Desc, FromNode, KeyPath, Node, NodeError, ToNode};

// ---------------------------------------------------------------------------------------------- //

/// Provides whole-vector integer adapters for every native integer width.
///
/// Selects a concrete module, such as [`u16`](mod@int_sequence::u16), with
/// `#[scry(with(int_sequence::u16))]`.
/// Expression strings use exact evaluation without context. Only retained values must fit the
/// destination type. Keep an [`IntSeqExpr`] when `N` or omitted endpoints need context supplied
/// later. Arrays use ordinary [`FromNode`] integer decoding.
///
/// Input and output allow at most [`MAX_VALUES`] values. Output is always a numeric array and
/// does not retain the expression source.
pub mod int_sequence {
    use super::*;

    macro_rules! integer_modules {
        ($($target:ident),+ $(,)?) => {
            $(
                #[doc = concat!("Expands integer sequences into `Vec<", stringify!($target), ">`.")]
                pub mod $target {
                    use super::*;

                    /// Expands an expression string or decodes an integer array.
                    pub fn from_node(node: &Node) -> Result<Vec<core::primitive::$target>, NodeError> {
                        match &node.kind {
                            Kind::Leaf(leaf) if matches!(leaf.value, Value::String(_)) => {
                                let expression = IntSeqExpr::from_node(node)?;
                                IntEvaluator::new(IntEvalOptions::default())
                                    .evaluate_as::<core::primitive::$target>(&expression)
                                    .map_err(|error| {
                                        NodeError::invalid_value_with_source(
                                            &node.path,
                                            "could not evaluate integer sequence expression",
                                            error,
                                        )
                                    })
                            }
                            Kind::Vec(_) => parse_array(node, |_, _| Ok(())),
                            _ => Err(NodeError::kind_mismatch(
                                &node.path,
                                "integer sequence expression string or integer array",
                                &node.kind,
                            )),
                        }
                    }

                    /// Serializes borrowed values as a numeric array within the value limit.
                    pub fn to_node(values: &[core::primitive::$target]) -> Result<Node, NodeError> {
                        serialize_array(values, |_| Ok(()))
                    }

                    /// Describes both input forms and the destination integer range.
                    pub fn describe() -> Desc {
                        Desc::plain("integer sequence expression string or integer array").with_doc(
                            format!(
                                "Expression strings expand without context. Arrays use ordinary integer \
                                 decoding. Retained values must fit {} ({}..={}). \
                                 Input and output allow at most 1,000,000 values. Output is a numeric array.",
                                stringify!($target),
                                core::primitive::$target::MIN,
                                core::primitive::$target::MAX,
                            ),
                        )
                    }
                }
            )+
        };
    }

    integer_modules!(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);
}

/// Provides whole-vector real adapters for `f32` and `f64`.
///
/// Selects a concrete module, such as [`f32`](mod@real_sequence::f32), with
/// `#[scry(with(real_sequence::f32))]`.
/// Expression evaluation rounds exact retained values directly into the destination type, using
/// nearest rounding with ties to even. Arrays use ordinary [`FromNode`] floating-point decoding
/// and preserve signed zero. Expression zeros are normalized to positive zero.
///
/// Input and output require finite values and allow at most [`MAX_VALUES`] values. Output is
/// always a numeric array and does not retain the expression source. Keep a [`RealSeqExpr`] to
/// evaluate later with [`RealEvaluator`].
pub mod real_sequence {
    use super::*;

    macro_rules! real_modules {
        ($($target:ident => $evaluate:ident),+ $(,)?) => {
            $(
                #[doc = concat!("Expands real sequences into `Vec<", stringify!($target), ">`.")]
                pub mod $target {
                    use super::*;

                    /// Expands an expression string or decodes a finite real array.
                    pub fn from_node(node: &Node) -> Result<Vec<core::primitive::$target>, NodeError> {
                        match &node.kind {
                            Kind::Leaf(leaf) if matches!(leaf.value, Value::String(_)) => {
                                let expression = RealSeqExpr::from_node(node)?;
                                RealEvaluator::new(RealEvalOptions::default())
                                    .$evaluate(&expression)
                                    .map_err(|error| {
                                        NodeError::invalid_value_with_source(
                                            &node.path,
                                            "could not evaluate real sequence expression",
                                            error,
                                        )
                                    })
                            }
                            Kind::Vec(_) => parse_array(node, |path, value: &core::primitive::$target| {
                                check_finite(path, value.is_finite(), stringify!($target))
                            }),
                            _ => Err(NodeError::kind_mismatch(
                                &node.path,
                                "real sequence expression string or real array",
                                &node.kind,
                            )),
                        }
                    }

                    /// Serializes finite borrowed values as a numeric array within the value limit.
                    pub fn to_node(values: &[core::primitive::$target]) -> Result<Node, NodeError> {
                        serialize_array(values, |item| {
                            check_finite(&KeyPath::new(), item.is_finite(), stringify!($target))
                        })
                    }

                    /// Describes both input forms and destination float requirements.
                    pub fn describe() -> Desc {
                        Desc::plain("real sequence expression string or real array").with_doc(format!(
                            "Expression strings round exact retained values directly to {} with nearest \
                             rounding and ties to even. Arrays use ordinary floating-point decoding and \
                             preserve signed zero. Expression zeros become positive. Values must be \
                             finite in the destination type. Input and output allow at most 1,000,000 \
                             values. Output is a numeric array.",
                            stringify!($target),
                        ))
                    }
                }
            )+
        };
    }

    real_modules!(f32 => evaluate_f32, f64 => evaluate);
}

// ---------------------------------------------------------------------------------------------- //

fn parse_array<T: FromNode>(
    node: &Node,
    validate: impl Fn(&KeyPath, &T) -> Result<(), NodeError>,
) -> Result<Vec<T>, NodeError> {
    check_count(&node.path, node.as_vec()?.len())?;
    read::vec(node, |entry| {
        let value = entry.as_type::<T>()?;
        validate(&entry.path, &value)?;
        Ok(value)
    })
}

fn serialize_array<T: ToNode>(
    values: &[T],
    validate: impl Fn(&T) -> Result<(), NodeError>,
) -> Result<Node, NodeError> {
    check_count(&KeyPath::new(), values.len())?;
    write::list(values, |item| {
        validate(item)?;
        item.to_node()
    })
}

fn check_count(path: &KeyPath, count: usize) -> Result<(), NodeError> {
    if count > MAX_VALUES {
        return Err(NodeError::invalid_value(
            path,
            format!("sequence exceeds the limit of {MAX_VALUES} values (received {count})"),
        ));
    }
    Ok(())
}

fn check_finite(path: &KeyPath, finite: bool, target_type: &str) -> Result<(), NodeError> {
    if !finite {
        return Err(NodeError::invalid_value(
            path,
            format!("real sequence values must be finite in {target_type}"),
        ));
    }
    Ok(())
}
