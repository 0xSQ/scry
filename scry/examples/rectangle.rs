//! Example demonstrating manual implementation of FromNode, ToNode, and Describe.
//!
//! Shows how to support multiple input formats and smart serialization.
//! Run with: cargo run --example rectangle

use indexmap::IndexMap;
use scry::node::Format;
use scry::{Desc, Describe, FromNode, KeyPath, Node, NodeError, ToNode};

// ---------------------------------------------------------------------------------------------- //

#[derive(Debug, Clone, PartialEq, Eq)]
struct Rectangle {
    width: u32,
    height: u32,
    area: u32, // Computed from width * height
}

// ---------------------------------------------------------------------------------------------- //

impl FromNode for Rectangle {
    /// Parses a Rectangle from one of three formats:
    /// - `[800, 600]`: width and height array
    /// - `{ side: 512 }`: square
    /// - `{ width: 800, height: 600 }`: explicit form
    fn from_node(node: &Node) -> Result<Self, NodeError> {
        let (width, height) = if let Some(arr) = node.as_opt_vec() {
            // Array [width, height]
            if arr.len() != 2 {
                return Err(NodeError::array_length(&node.path, 2, arr.len()));
            }
            (arr[0].as_type()?, arr[1].as_type()?)
        } else if let Some(side) = node.opt::<u32>("side")? {
            // Square shorthand { side: n }
            (side, side)
        } else {
            // Explicit map { width, height }
            (node.req("width")?, node.req("height")?)
        };

        // Treat leftover keys as errors
        node.ensure_no_unknown_keys()?;

        if width == 0 {
            return Err(NodeError::invalid_value(&node.path, "width must be positive"));
        }
        if height == 0 {
            return Err(NodeError::invalid_value(&node.path, "height must be positive"));
        }

        Ok(Self {
            width,
            height,
            area: width * height,
        })
    }
}

impl ToNode for Rectangle {
    /// Serializes to the most compact form:
    /// - Squares become `{ side: n }`
    /// - Non-squares become `[width, height]`
    fn to_node(&self) -> Result<Node, NodeError> {
        if self.width == self.height {
            // Build { side: n }
            let side_node = self.width.to_node()?;
            let mut map = IndexMap::new();
            map.insert("side".to_string(), side_node);
            Ok(Node::new_map(KeyPath::default(), map))
        } else {
            // Build [width, height]
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
