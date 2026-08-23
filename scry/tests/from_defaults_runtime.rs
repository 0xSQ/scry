use scry::{FromDefaults, FromNode, KeyPath, Node, NodeError};

// ---------------------------------------------------------------------------------------------- //

#[derive(Debug)]
struct NeedsHost;

impl FromNode for NeedsHost {
    fn from_node(node: &Node) -> Result<Self, NodeError> {
        node.req::<String>("host")?;
        Ok(Self)
    }
}

impl FromDefaults for NeedsHost {
    fn from_defaults_at(path: &KeyPath) -> Result<Self, NodeError> {
        Self::from_node(&Node::empty_map_at(path.clone()))
    }
}

#[test]
fn root_helper_uses_the_root_path() {
    let error = scry::from_defaults::<NeedsHost>().unwrap_err();
    assert_eq!(error.to_string(), "missing value for 'host'");
}

#[test]
fn default_construction_preserves_a_nested_diagnostic_path() {
    let path: KeyPath = "outer.inner".parse().unwrap();
    let error = NeedsHost::from_defaults_at(&path).unwrap_err();
    assert_eq!(error.to_string(), "missing value for 'outer.inner.host'");
}

#[test]
fn full_path_uses_key_path_syntax() {
    let node = Node::empty_map_at("outer".parse().unwrap());
    let path = node.full_path(r#"inner[1]["display.name"]"#).unwrap();
    assert_eq!(path.to_string(), r#"outer.inner[1]["display.name"]"#);
}
