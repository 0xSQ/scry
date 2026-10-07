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

/// Expands integer expression strings or decodes integer arrays into `Vec<i64>`.
///
/// This Via policy evaluates expressions without context, so `N` and omitted endpoints require
/// a retained [`IntSeqExpr`] instead. Arrays use ordinary Scry integer decoding. Input and output
/// are limited to [`MAX_VALUES`] values, and output is always a numeric array.
pub struct IntSequence;

impl FromNodeVia<Vec<i64>> for IntSequence {
    fn from_node(node: &Node) -> Result<Vec<i64>, NodeError> {
        match &node.kind {
            Kind::Leaf(leaf) if matches!(leaf.value, Value::String(_)) => {
                let expression = IntSeqExpr::from_node(node)?;
                IntEvaluator::new(IntEvalOptions::default()).evaluate(&expression).map_err(
                    |error| {
                        NodeError::invalid_value_with_source(
                            &node.path,
                            "could not evaluate integer sequence expression",
                            error,
                        )
                    },
                )
            }
            Kind::Vec(entries) => {
                check_count(&node.path, entries.len())?;
                Vec::<i64>::from_node(node)
            }
            _ => Err(NodeError::kind_mismatch(
                &node.path,
                "integer sequence expression string or integer array",
                &node.kind,
            )),
        }
    }
}

impl ToNodeVia<Vec<i64>> for IntSequence {
    fn to_node(value: &Vec<i64>) -> Result<Node, NodeError> {
        check_count(&KeyPath::new(), value.len())?;
        value.to_node()
    }
}

impl DescribeVia<Vec<i64>> for IntSequence {
    fn describe() -> Desc {
        Desc::plain("integer sequence expression string or integer array").with_doc(
            "Expression strings expand during decoding without context. Arrays contain scalar \
             integers. Input and output allow at most 1,000,000 values. Output is a numeric array.",
        )
    }
}

/// Expands real expression strings or decodes real arrays into `Vec<f64>`.
///
/// This Via policy requires finite values and limits input and output to [`MAX_VALUES`] values.
/// Arrays use ordinary Scry floating-point decoding and preserve signed zero. Expression evaluation
/// retains its positive-zero normalization. Output is always a numeric array.
pub struct RealSequence;

impl FromNodeVia<Vec<f64>> for RealSequence {
    fn from_node(node: &Node) -> Result<Vec<f64>, NodeError> {
        match &node.kind {
            Kind::Leaf(leaf) if matches!(leaf.value, Value::String(_)) => {
                let expression = RealSeqExpr::from_node(node)?;
                RealEvaluator::new(RealEvalOptions::default()).evaluate(&expression).map_err(
                    |error| {
                        NodeError::invalid_value_with_source(
                            &node.path,
                            "could not evaluate real sequence expression",
                            error,
                        )
                    },
                )
            }
            Kind::Vec(entries) => {
                check_count(&node.path, entries.len())?;
                parse_vec(node, |entry| {
                    let value = entry.as_type::<f64>()?;
                    check_finite(&entry.path, value)?;
                    Ok(value)
                })
            }
            _ => Err(NodeError::kind_mismatch(
                &node.path,
                "real sequence expression string or real array",
                &node.kind,
            )),
        }
    }
}

impl ToNodeVia<Vec<f64>> for RealSequence {
    fn to_node(value: &Vec<f64>) -> Result<Node, NodeError> {
        check_count(&KeyPath::new(), value.len())?;
        serialize_vec(value, |item| {
            check_finite(&KeyPath::new(), *item)?;
            item.to_node()
        })
    }
}

impl DescribeVia<Vec<f64>> for RealSequence {
    fn describe() -> Desc {
        Desc::plain("real sequence expression string or real array").with_doc(
            "Expression strings expand during decoding. Arrays contain scalar real values. \
             Values must be finite. Input and output allow at most 1,000,000 values. \
             Output is a numeric array.",
        )
    }
}

// ---------------------------------------------------------------------------------------------- //

fn check_count(path: &KeyPath, count: usize) -> Result<(), NodeError> {
    if count > MAX_VALUES {
        return Err(NodeError::invalid_value(
            path,
            format!("sequence exceeds the limit of {MAX_VALUES} values (received {count})"),
        ));
    }
    Ok(())
}

fn check_finite(path: &KeyPath, value: f64) -> Result<(), NodeError> {
    if !value.is_finite() {
        return Err(NodeError::invalid_value(path, "real sequence values must be finite"));
    }
    Ok(())
}
