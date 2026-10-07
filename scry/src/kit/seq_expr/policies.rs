//! Via policies for immediately expanded numeric sequences.

use super::MAX_VALUES;
use super::{
    IntEvalOptions, IntEvaluator, IntSeqExpr, RealEvalOptions, RealEvaluator, RealSeqExpr,
};
use crate::node::{Kind, Value};
use crate::traits::{parse_vec, serialize_vec};
use crate::{
    Desc, DescribeVia, FromNode, FromNodeVia, KeyPath, Node, NodeError, ToNode, ToNodeVia,
};

// ---------------------------------------------------------------------------------------------- //

/// Expands integer expression strings or decodes integer arrays into a native integer vector.
///
/// This Via policy supports `i8`, `i16`, `i32`, `i64`, `isize`, `u8`, `u16`, `u32`, `u64`, and
/// `usize`. Expressions are evaluated exactly before retained values are checked against the
/// destination type. Evaluation has no context, so `N` and omitted endpoints require a retained
/// [`IntSeqExpr`] instead. Arrays use ordinary Scry integer decoding. Input and output are limited
/// to [`MAX_VALUES`] values, and output is always a numeric array.
pub struct IntSequence;

macro_rules! impl_integer_sequence {
    ($($target:ty),+ $(,)?) => {
        $(
            impl FromNodeVia<Vec<$target>> for IntSequence {
                fn from_node(node: &Node) -> Result<Vec<$target>, NodeError> {
                    match &node.kind {
                        Kind::Leaf(leaf) if matches!(leaf.value, Value::String(_)) => {
                            let expression = IntSeqExpr::from_node(node)?;
                            IntEvaluator::new(IntEvalOptions::default())
                                .evaluate_as::<$target>(&expression)
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
            }

            impl ToNodeVia<Vec<$target>> for IntSequence {
                fn to_node(value: &Vec<$target>) -> Result<Node, NodeError> {
                    serialize_array(value, |_| Ok(()))
                }
            }

            impl DescribeVia<Vec<$target>> for IntSequence {
                fn describe() -> Desc {
                    Desc::plain("integer sequence expression string or integer array").with_doc(
                        format!(
                            "Expression strings expand during decoding without context. Arrays \
                             contain scalar integers. Retained values must fit {} ({}..={}). \
                             Input and output allow at most 1,000,000 values. Output is a numeric array.",
                            stringify!($target),
                            <$target>::MIN,
                            <$target>::MAX,
                        ),
                    )
                }
            }
        )+
    };
}

impl_integer_sequence!(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);

/// Expands real expression strings or decodes real arrays into `Vec<f32>` or `Vec<f64>`.
///
/// Expression evaluation rounds exact retained values directly into the destination type, using
/// nearest rounding with ties to even. Arrays use ordinary Scry floating-point decoding and preserve
/// signed zero. Expression evaluation retains its positive-zero normalization. This policy requires
/// finite values and limits input and output to [`MAX_VALUES`] values. Output is a numeric array.
pub struct RealSequence;

macro_rules! impl_real_sequence {
    ($($target:ty => $evaluate:ident),+ $(,)?) => {
        $(
            impl FromNodeVia<Vec<$target>> for RealSequence {
                fn from_node(node: &Node) -> Result<Vec<$target>, NodeError> {
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
                        Kind::Vec(_) => parse_array(node, |path, value: &$target| {
                            check_finite(path, value.is_finite(), stringify!($target))
                        }),
                        _ => Err(NodeError::kind_mismatch(
                            &node.path,
                            "real sequence expression string or real array",
                            &node.kind,
                        )),
                    }
                }
            }

            impl ToNodeVia<Vec<$target>> for RealSequence {
                fn to_node(value: &Vec<$target>) -> Result<Node, NodeError> {
                    serialize_array(value, |item| {
                        check_finite(&KeyPath::new(), item.is_finite(), stringify!($target))
                    })
                }
            }

            impl DescribeVia<Vec<$target>> for RealSequence {
                fn describe() -> Desc {
                    Desc::plain("real sequence expression string or real array").with_doc(format!(
                        "Expression strings expand during decoding, rounding exact retained values \
                         directly to {} with nearest rounding and ties to even. Arrays contain scalar \
                         real values decoded as {}. Values must be finite in that type. Input and \
                         output allow at most 1,000,000 values. Output is a numeric array.",
                        stringify!($target),
                        stringify!($target),
                    ))
                }
            }
        )+
    };
}

impl_real_sequence!(f32 => evaluate_f32, f64 => evaluate);

// ---------------------------------------------------------------------------------------------- //

fn parse_array<T: FromNode>(
    node: &Node,
    validate: impl Fn(&KeyPath, &T) -> Result<(), NodeError>,
) -> Result<Vec<T>, NodeError> {
    check_count(&node.path, node.as_vec()?.len())?;
    parse_vec(node, |entry| {
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
    serialize_vec(values, |item| {
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
