//! Converts color notation to standard Rust arrays and tuples.

use super::{Rgb8, Rgba8};
use crate::{Desc, Describe, FromNode, Node, NodeError, ToNode};

// ---------------------------------------------------------------------------------------------- //

macro_rules! color_adapter {
    ($module:ident, $color:ident, $target:ty, $doc:literal) => {
        #[doc = $doc]
        pub mod $module {
            use super::*;

            // ---------------------------------------------------------------------------------- //

            /// Reads color notation into the destination channel representation.
            pub fn from_node(node: &Node) -> Result<$target, NodeError> {
                $color::from_node(node).map(Into::into)
            }

            /// Writes the channels as the native color's canonical hex string.
            pub fn to_node(value: &$target) -> Result<Node, NodeError> {
                $color::from(*value).to_node()
            }

            /// Describes the native color's accepted notation and channel semantics.
            pub fn describe() -> Desc {
                $color::describe()
            }
        }
    };
}

color_adapter!(
    rgb8_array,
    Rgb8,
    [u8; 3],
    "Adapts opaque color notation to `[u8; 3]` with `#[scry(with(rgb8_array))]`."
);
color_adapter!(
    rgba8_array,
    Rgba8,
    [u8; 4],
    "Adapts color notation to `[u8; 4]` with `#[scry(with(rgba8_array))]`."
);
color_adapter!(
    rgb8_tuple,
    Rgb8,
    (u8, u8, u8),
    "Adapts opaque color notation to `(u8, u8, u8)` with `#[scry(with(rgb8_tuple))]`."
);
color_adapter!(
    rgba8_tuple,
    Rgba8,
    (u8, u8, u8, u8),
    "Adapts color notation to `(u8, u8, u8, u8)` with `#[scry(with(rgba8_tuple))]`."
);
