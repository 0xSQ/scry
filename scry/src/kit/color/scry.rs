//! Native Scry conversion for resolved byte colors.

use super::{Rgb8, Rgba8};
use crate::convert::read;
use crate::node::{Kind, Value};
use crate::{Desc, Describe, FromNode, Node, NodeError, ToNode};

// ---------------------------------------------------------------------------------------------- //

impl FromNode for Rgb8 {
    fn from_node(node: &Node) -> Result<Self, NodeError> {
        let color = Rgba8::from_node(node)?;
        Self::try_from(color).map_err(|error| {
            let path = match &node.kind {
                Kind::Vec(entries) if entries.len() == 4 => &entries[3].path,
                Kind::Map(entries) => entries.get("a").map_or(&node.path, |alpha| &alpha.path),
                _ => &node.path,
            };
            NodeError::invalid_value_with_source(path, "color must be opaque", error)
        })
    }
}

impl ToNode for Rgb8 {
    fn to_node(&self) -> Result<Node, NodeError> {
        self.to_string().to_node()
    }
}

impl Describe for Rgb8 {
    fn describe() -> Desc {
        description(true)
    }
}

impl FromNode for Rgba8 {
    fn from_node(node: &Node) -> Result<Self, NodeError> {
        match &node.kind {
            Kind::Leaf(leaf) if matches!(leaf.value, Value::String(_)) => {
                let Value::String(source) = node.read_leaf("color string")? else {
                    unreachable!("the leaf was checked to contain a string");
                };
                source.parse().map_err(|error| {
                    NodeError::invalid_value_with_source(&node.path, "invalid color string", error)
                })
            }
            Kind::Vec(entries) => match entries.len() {
                3 => {
                    let [r, g, b] = read::array(node, u8::from_node)?;
                    Ok(Self::new(r, g, b, 255))
                }
                4 => read::array::<_, 4>(node, u8::from_node).map(Self::from),
                _ => Err(NodeError::invalid_value(
                    &node.path,
                    "color arrays must contain three or four u8 channels",
                )),
            },
            Kind::Map(_) => {
                let r = node.req::<u8>("r")?;
                let g = node.req::<u8>("g")?;
                let b = node.req::<u8>("b")?;
                let a = node.opt::<u8>("a")?.unwrap_or(255);
                node.ensure_only_keys(&["r", "g", "b", "a"])?;
                Ok(Self::new(r, g, b, a))
            }
            _ => Err(NodeError::kind_mismatch(
                &node.path,
                "color string, RGB/RGBA array, or channel map",
                &node.kind,
            )),
        }
    }
}

impl ToNode for Rgba8 {
    fn to_node(&self) -> Result<Node, NodeError> {
        self.to_string().to_node()
    }
}

impl Describe for Rgba8 {
    fn describe() -> Desc {
        description(false)
    }
}

// ---------------------------------------------------------------------------------------------- //

fn description(opaque: bool) -> Desc {
    let (hint, alpha, output) = if opaque {
        (
            "opaque sRGB color",
            "Alpha must be 255, including when supplied explicitly.",
            "Output is lowercase #rrggbb.",
        )
    } else {
        (
            "sRGB color with optional alpha",
            "Alpha is separate opacity: 0 is transparent and 255 is opaque.",
            "Output is lowercase #rrggbb when opaque and #rrggbbaa otherwise.",
        )
    };
    Desc::plain(hint).with_doc(format!(
        "Accepts CSS named colors, transparent, #rgb, #rgba, #rrggbb, #rrggbbaa, \
         [r, g, b], [r, g, b, a], or {{ r, g, b, a? }}. \
         Channels use native u8 decoding in 0..=255, including decimal integer strings. \
         Missing alpha defaults to 255. Null and unknown map keys are rejected. \
         {alpha} {output} Authored notation is not retained."
    ))
}
