//! Native Scry conversion for retained sequence expressions.

use super::{IntSeqExpr, RealSeqExpr, SeqExpr};
use crate::node::Value;
use crate::{Desc, Describe, FromNode, Node, NodeError, ToNode};

// ---------------------------------------------------------------------------------------------- //

impl FromNode for SeqExpr {
    fn from_node(node: &Node) -> Result<Self, NodeError> {
        read_source(node)?.parse().map_err(|error| {
            NodeError::invalid_value_with_source(&node.path, "invalid sequence expression", error)
        })
    }
}

impl ToNode for SeqExpr {
    fn to_node(&self) -> Result<Node, NodeError> {
        self.source().to_node()
    }
}

impl Describe for SeqExpr {
    fn describe() -> Desc {
        Desc::plain("sequence expression string").with_doc(
            "Comma-separated values and ranges with optional step or subdivision sampling. \
             The numeric profile is selected before evaluation.",
        )
    }
}

impl FromNode for IntSeqExpr {
    fn from_node(node: &Node) -> Result<Self, NodeError> {
        read_source(node)?.parse().map_err(|error| {
            NodeError::invalid_value_with_source(
                &node.path,
                "invalid integer sequence expression",
                error,
            )
        })
    }
}

impl ToNode for IntSeqExpr {
    fn to_node(&self) -> Result<Node, NodeError> {
        self.source().to_node()
    }
}

impl Describe for IntSeqExpr {
    fn describe() -> Desc {
        Desc::plain("integer sequence expression string").with_doc(
            "Comma-separated integer values and ranges with optional step or subdivision sampling. \
             Open ranges and N require evaluation context.",
        )
    }
}

impl FromNode for RealSeqExpr {
    fn from_node(node: &Node) -> Result<Self, NodeError> {
        read_source(node)?.parse().map_err(|error| {
            NodeError::invalid_value_with_source(
                &node.path,
                "invalid real sequence expression",
                error,
            )
        })
    }
}

impl ToNode for RealSeqExpr {
    fn to_node(&self) -> Result<Node, NodeError> {
        self.source().to_node()
    }
}

impl Describe for RealSeqExpr {
    fn describe() -> Desc {
        Desc::plain("real sequence expression string").with_doc(
            "Comma-separated real values and ranges. Ranges require explicit endpoints and a step \
             or subdivision sampler. Evaluation produces finite f64 values.",
        )
    }
}

// ---------------------------------------------------------------------------------------------- //

/// Reads a string leaf without copying input before the parser checks its limits.
pub(super) fn read_source(node: &Node) -> Result<&str, NodeError> {
    match node.read_leaf("sequence expression string")? {
        Value::String(source) => Ok(source),
        value => Err(NodeError::type_mismatch(
            &node.path,
            "sequence expression string",
            value.type_name(),
        )),
    }
}
