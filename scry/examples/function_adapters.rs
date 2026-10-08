//! Demonstrates foreign values, nested shape helpers, and eager sequence conversion.
//!
//! Run with: `cargo run --example function_adapters`.

use std::net::IpAddr;

use scry::kit::seq_expr::int_sequence;
use scry::kit::KeyValues;
use scry::node::Format;
use scry::{Config, Describe, Node, NodeError, ToNode};

// ---------------------------------------------------------------------------------------------- //

#[derive(Debug, Config, ToNode)]
struct Settings {
    /// The local address to listen on.
    #[scry(with(address_text))]
    bind: IpAddr,
    /// Named groups of addresses, with null placeholders allowed in each group.
    #[scry(with(address_groups))]
    peers: KeyValues<Vec<Option<IpAddr>>>,
    /// Retry delays selected by an expression or an explicit array.
    #[scry(with(int_sequence::u16))]
    delays: Vec<u16>,
}

fn main() -> Result<(), NodeError> {
    let node = Node::parse_str(
        indoc::indoc! {r#"
            #{
                bind: "127.0.0.1",
                peers: #{ local: ["127.0.0.1", ()], remote: ["192.0.2.1"] },
                delays: "[1..5]:2",
            }
        "#},
        Format::Rhai,
    )?;
    let settings: Settings = node.as_type()?;
    node.ensure_no_unknown_keys()?;

    println!("{}", Settings::describe().display());
    println!("{}", settings.to_node()?.to_string_as(Format::Rhai)?);
    Ok(())
}

// ---------------------------------------------------------------------------------------------- //

mod address_text {
    use std::net::IpAddr;

    use scry::{Desc, Node, NodeError, ToNode};

    // ------------------------------------------------------------------------------------------ //

    pub fn from_node(node: &Node) -> Result<IpAddr, NodeError> {
        node.as_type::<String>()?.parse().map_err(|error| {
            NodeError::invalid_value_with_source(&node.path, "invalid IP address", error)
        })
    }

    pub fn to_node(value: &IpAddr) -> Result<Node, NodeError> {
        value.to_string().to_node()
    }

    pub fn describe() -> Desc {
        Desc::plain("IP address string")
    }
}

mod address_groups {
    use std::net::IpAddr;

    use scry::convert::{read, write};
    use scry::kit::{key_values, KeyValues};
    use scry::{Desc, Node, NodeError};

    use super::address_text;

    // ------------------------------------------------------------------------------------------ //

    pub fn from_node(node: &Node) -> Result<KeyValues<Vec<Option<IpAddr>>>, NodeError> {
        key_values::read_with(node, |entry| {
            read::vec(entry, |item| read::option(item, address_text::from_node))
        })
    }

    pub fn to_node(value: &KeyValues<Vec<Option<IpAddr>>>) -> Result<Node, NodeError> {
        key_values::write_with(value, |items| {
            write::list(items, |item| write::option(item, address_text::to_node))
        })
    }

    pub fn describe() -> Desc {
        key_values::description(Desc::list(address_text::describe().nullable()))
    }
}
