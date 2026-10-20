//! Example demonstrating manual implementation of FromNode, ToNode, and Describe.
//!
//! Shows how to support multiple input formats and smart serialization.
//! Run with: cargo run --example rectangle

use indexmap::IndexMap;
use scry::node::Format;
use scry::{Desc, Describe, FromNode, KeyPath, Node, NodeError, ToNode};

// ---------------------------------------------------------------------------------------------- //

#[derive(Debug, PartialEq, Eq)]
struct Rectangle {
    width: u32,
    height: u32,
    area: u64,
}

impl FromNode for Rectangle {
    /// Parses dimensions from an array, square shorthand, or explicit map.
    fn from_node(node: &Node) -> Result<Self, NodeError> {
        let (width, height) = if let Some(arr) = node.as_opt_vec() {
            if arr.len() != 2 {
                return Err(NodeError::array_length(&node.path, 2, arr.len()));
            }
            (arr[0].as_type()?, arr[1].as_type()?)
        } else if let Some(side) = node.opt::<u32>("side")? {
            node.ensure_only_keys(&["side"])?;
            (side, side)
        } else {
            let dimensions = (node.req("width")?, node.req("height")?);
            node.ensure_only_keys(&["width", "height"])?;
            dimensions
        };

        if width == 0 {
            return Err(NodeError::invalid_value(&node.path, "width must be positive"));
        }
        if height == 0 {
            return Err(NodeError::invalid_value(&node.path, "height must be positive"));
        }

        Ok(Self {
            width,
            height,
            area: u64::from(width) * u64::from(height),
        })
    }
}

impl ToNode for Rectangle {
    /// Serializes squares as a side map and other rectangles as dimension pairs.
    fn to_node(&self) -> Result<Node, NodeError> {
        if self.width == self.height {
            let side_node = self
                .width
                .to_node()
                .map_err(|error| error.prepend_path(&KeyPath::from_keys(["side"])))?;
            let mut map = IndexMap::new();
            map.insert("side".to_string(), side_node);
            Ok(Node::new_map(KeyPath::default(), map))
        } else {
            (self.width, self.height).to_node()
        }
    }
}

impl Describe for Rectangle {
    fn describe() -> Desc {
        Desc::plain("rectangle")
    }
}

// ---------------------------------------------------------------------------------------------- //

fn main() -> anyhow::Result<()> {
    // Test various input formats (in Rhai syntax).
    let inputs = [
        r#"#{ side: 512 }"#,
        r#"[800, 600]"#,
        r#"#{ width: 1920, height: 1080 }"#,
        r#"[100, 100]"#,
    ];

    for input in inputs {
        // Parse from Rhai.
        let node = Node::parse_str(input, Format::Rhai)?;
        let rect: Rectangle = node.as_type()?;
        println!("Parsed struct: {:?}", rect);

        // Round-trip back to Rhai.
        let output = rect.to_node()?.to_string_as(Format::Rhai)?;
        println!("As Rhai:\n{}", output);
        println!();
    }

    // Show the simple type hint label.
    let desc = Rectangle::describe();
    println!("Type:  {}", desc.type_label());

    Ok(())
}

// ---------------------------------------------------------------------------------------------- //

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dimensions_round_trip_without_overflow() -> Result<(), NodeError> {
        for (input, area) in [
            ("[800, 600]", 480_000),
            ("#{ side: 512 }", 262_144),
            ("#{ width: 1920, height: 1080 }", 2_073_600),
            ("[4294967295, 4294967295]", 18_446_744_065_119_617_025),
        ] {
            let rectangle: Rectangle = Node::parse_str(input, Format::Rhai)?.as_type()?;
            assert_eq!(rectangle.area, area);
            let output = rectangle.to_node()?.to_string_as(Format::Rhai)?;
            assert_eq!(Node::parse_str(&output, Format::Rhai)?.as_type::<Rectangle>()?, rectangle);
        }
        Ok(())
    }

    #[test]
    fn map_forms_reject_unknown_empty_values_even_after_prior_reads() -> Result<(), NodeError> {
        let node = Node::parse_str("#{ side: 5, extra: #{} }", Format::Rhai)?;
        let _: Node = node.req("extra")?;
        assert!(node.as_type::<Rectangle>().is_err());
        Ok(())
    }
}
