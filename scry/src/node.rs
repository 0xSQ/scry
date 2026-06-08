//! The Node tree - intermediate representation for configuration data.
//!
//! Provides a tree structure that can be loaded from various sources (Rhai, JSON, etc.)
//! and then parsed into typed Rust structures via the [`FromNode`](crate::FromNode) trait.

pub mod errors;
mod format;
mod value;

pub use errors::NodeError;
pub use format::{
    default_format_parser_registry, default_format_parser_registry_builder,
    default_format_writer_registry, ConfigFormatParser, ConfigFormatWriter, Format, FormatError,
    FormatParserRegistry, FormatParserRegistryBuilder, FormatUsage, FormatWriterRegistry,
    FormatWriterRegistryBuilder,
};
pub use value::{IntoValue, Value};

use crate::key_path::{KeyPath, Segment, TryIntoKeyPath};
use crate::util::{PathError, PathExt};
use crate::writer::{FlatConfig, FlatWriter, TreeConfig, TreeWriter};
use indexmap::IndexMap;
use std::{cell::RefCell, path::Path, rc::Rc};

// ---------------------------------------------------------------------------------------------- //
// Node

/// A node in the configuration tree.
///
/// Each node has a path (its location in the tree) and a kind (leaf, array, or map).
/// Nodes are typically created by parsing configuration files, then traversed
/// to extract typed values.
#[derive(Debug, Clone)]
pub struct Node {
    pub path: KeyPath,
    pub kind: Kind,
}

impl Node {
    // ------------------------------------------------------------------------------------------ //
    // Constructors

    /// Create an empty node.
    pub fn new() -> Self {
        Self {
            path: KeyPath::new(),
            kind: Kind::Leaf(Leaf::new(Value::Null)),
        }
    }

    /// Creates a new leaf node.
    pub fn new_leaf(path: KeyPath, value: Value) -> Self {
        Self {
            path,
            kind: Kind::Leaf(Leaf::new(value)),
        }
    }

    /// Creates a new array node.
    pub fn new_vec(path: KeyPath, children: Vec<Node>) -> Self {
        Self {
            path,
            kind: Kind::Vec(children),
        }
    }

    /// Creates a new map node.
    pub fn new_map(path: KeyPath, children: IndexMap<String, Node>) -> Self {
        Self {
            path,
            kind: Kind::Map(children),
        }
    }

    // ------------------------------------------------------------------------------------------ //
    // Parsing

    /// Creates a Node tree from a Rhai Dynamic value.
    ///
    /// Use this with custom Rhai engines. For simple cases, use
    /// [`parse_str_as`](Self::parse_str_as) or [`parse_file`](Self::parse_file).
    pub fn from_rhai_dynamic(dynamic: rhai::Dynamic) -> Result<Self, NodeError> {
        format::from_rhai_dynamic(dynamic)
    }

    /// Parses a Node tree from a string using the default format registry.
    pub fn parse_str(source: &str, format_id: impl Into<Format>) -> Result<Self, NodeError> {
        let registry = default_format_parser_registry();
        Self::parse_str_as(source, format_id, &registry)
    }

    /// Parses a Node tree from a string in the format resolved through the given registry.
    pub fn parse_str_as(
        source: &str,
        format_id: impl Into<Format>,
        registry: &FormatParserRegistry,
    ) -> Result<Self, NodeError> {
        let format_id = format_id.into();
        let parser = registry.parser_by_id(&format_id).ok_or_else(|| {
            let supported =
                registry.supported_format_ids().iter().map(|id| id.as_str()).collect::<Vec<_>>();
            FormatError::unknown_format_id(FormatUsage::Input, format_id.as_str(), &supported)
        })?;
        parser.parse_str(source)
    }

    /// Parses a Node tree from a file, detecting format from the extension.
    ///
    /// For `.rhai` files, this uses relative module resolution, so imports like
    /// `import "util"` resolve relative to the script's directory.
    pub fn parse_file(path: impl AsRef<Path>) -> Result<Self, NodeError> {
        let registry = default_format_parser_registry();
        Self::parse_file_with_registry(path, &registry)
    }

    /// Parses a Node tree from a file with parser dispatch from the given registry.
    pub fn parse_file_with_registry(
        path: impl AsRef<Path>,
        registry: &FormatParserRegistry,
    ) -> Result<Self, NodeError> {
        let path = path.as_ref();
        let ext = path.ext_str().map_err(|err| match err {
            PathError::MissingComponent {
                component: "file extension",
                ..
            } => FormatError::missing_file_extension(path).into(),
            other => NodeError::read_file_extension(path, other),
        })?;

        let parser = registry.parser_for_extension(ext).ok_or_else(|| {
            let supported = registry.supported_extensions();
            NodeError::from(FormatError::unknown_file_extension(path, ext, &supported))
        })?;
        parser.parse_file(path)
    }

    // ------------------------------------------------------------------------------------------ //
    // Traversal

    /// Internal traversal helper with strict error semantics.
    ///
    /// Returns:
    /// - `Ok(None)` if a key/index is missing, or if the resolved node is Null
    /// - `Err(...)` for type mismatches (e.g., indexing into a map, keying into a vec)
    /// - `Ok(Some(&Node))` when fully resolved to a non-Null node
    fn walk(&self, path: &KeyPath) -> Result<Option<&Node>, NodeError> {
        let mut node = self;

        for (i, seg) in path.iter().enumerate() {
            match (seg, &node.kind) {
                (Segment::Key(key), Kind::Map(map)) => match map.get(key) {
                    Some(child) => node = child,
                    None => return Ok(None),
                },
                (Segment::Index(idx), Kind::Vec(vec)) => match vec.get(*idx) {
                    Some(child) => node = child,
                    None => return Ok(None),
                },
                (Segment::Key(key), Kind::Vec(_)) => {
                    return Err(NodeError::key_on_array(&node.path, key));
                }
                (Segment::Index(idx), Kind::Map(_)) => {
                    return Err(NodeError::index_on_map(&node.path, *idx));
                }
                (_, Kind::Leaf(leaf)) => {
                    // Null is treated as absence - mark visited so it's not flagged as unknown
                    if matches!(leaf.value, Value::Null) {
                        leaf.mark_visited();
                        return Ok(None);
                    }
                    // Trying to descend into a non-null leaf is an error
                    let traversed = KeyPath {
                        segs: path.segs[..i].to_vec(),
                    };
                    return Err(NodeError::descend_into_leaf(&traversed, leaf.value.type_name()));
                }
            }
        }

        // Check final node for Null-as-absence - mark visited so it's not flagged as unknown
        if let Kind::Leaf(leaf) = &node.kind {
            if matches!(leaf.value, Value::Null) {
                leaf.mark_visited();
                return Ok(None);
            }
        }

        Ok(Some(node))
    }

    /// Returns a required child node by path, or an error if not found.
    ///
    /// String paths are parsed as dotted paths (e.g., `"server.port"` navigates to `server` then `port`).
    /// For keys containing dots, use bracket notation: `["server.port"]`.
    pub fn req_node(&self, path: impl TryIntoKeyPath) -> Result<&Node, NodeError> {
        let kp = path.try_into_key_path()?;
        match self.walk(&kp)? {
            Some(node) => Ok(node),
            None => {
                let full_path = self.path.join(&kp);
                Err(NodeError::missing_required(&full_path))
            }
        }
    }

    /// Returns an optional child node by path, or None if not found or null.
    ///
    /// String paths are parsed as dotted paths (e.g., `"server.port"` navigates to `server` then `port`).
    /// For keys containing dots, use bracket notation: `["server.port"]`.
    ///
    /// Returns `Err` for path syntax errors or type mismatches during traversal.
    /// Returns `Ok(None)` only when the path is absent or null.
    pub fn opt_node(&self, path: impl TryIntoKeyPath) -> Result<Option<&Node>, NodeError> {
        let kp = path.try_into_key_path()?;
        self.walk(&kp)
    }

    // ------------------------------------------------------------------------------------------ //
    // Type Conversion

    /// Parses this node as type T, marking the leaf as visited.
    pub fn as_type<T: crate::FromNode>(&self) -> Result<T, NodeError> {
        if let Kind::Leaf(leaf) = &self.kind {
            leaf.mark_visited();
        }
        T::from_node(self)
    }

    /// Returns a required value by path.
    ///
    /// String paths are parsed as dotted paths (e.g., `"server.port"` navigates to `server` then `port`).
    pub fn req<T: crate::FromNode>(&self, path: impl TryIntoKeyPath) -> Result<T, NodeError> {
        self.req_node(path)?.as_type()
    }

    /// Returns an optional value by path.
    ///
    /// String paths are parsed as dotted paths (e.g., `"server.port"` navigates to `server` then `port`).
    /// Returns `Err` for path syntax errors or type mismatches during traversal.
    /// Returns `Ok(None)` only when the path is absent or null.
    pub fn opt<T: crate::FromNode>(
        &self,
        path: impl TryIntoKeyPath,
    ) -> Result<Option<T>, NodeError> {
        match self.opt_node(path)? {
            None => Ok(None),
            Some(node) => node.as_type().map(Some),
        }
    }

    // ------------------------------------------------------------------------------------------ //
    // Kind Accessors

    /// Returns the inner vec if this is a Vec node.
    pub fn as_vec(&self) -> Result<&Vec<Node>, NodeError> {
        match &self.kind {
            Kind::Vec(vec) => Ok(vec),
            _ => Err(NodeError::kind_mismatch(&self.path, "array", &self.kind)),
        }
    }

    /// Returns the inner vec if this is a Vec node, or None otherwise.
    pub fn as_opt_vec(&self) -> Option<&Vec<Node>> {
        self.as_vec().ok()
    }

    /// Returns the inner map if this is a Map node.
    pub fn as_map(&self) -> Result<&IndexMap<String, Node>, NodeError> {
        match &self.kind {
            Kind::Map(map) => Ok(map),
            _ => Err(NodeError::kind_mismatch(&self.path, "map", &self.kind)),
        }
    }

    /// Returns the inner map if this is a Map node, or None otherwise.
    pub fn as_opt_map(&self) -> Option<&IndexMap<String, Node>> {
        self.as_map().ok()
    }

    /// Returns the leaf, or an error if not a leaf.
    pub fn as_leaf(&self) -> Result<&Leaf, NodeError> {
        match &self.kind {
            Kind::Leaf(leaf) => Ok(leaf),
            _ => Err(NodeError::kind_mismatch(&self.path, "leaf", &self.kind)),
        }
    }

    /// Reads the leaf value and marks it as visited.
    ///
    /// The `target_type` parameter is used in error messages when the node is not a leaf.
    pub fn read_leaf(&self, target_type: &'static str) -> Result<&Value, NodeError> {
        match &self.kind {
            Kind::Leaf(leaf) => {
                leaf.mark_visited();
                Ok(&leaf.value)
            }
            Kind::Vec(_) => Err(NodeError::type_mismatch(&self.path, target_type, "array")),
            Kind::Map(_) => Err(NodeError::type_mismatch(&self.path, target_type, "map")),
        }
    }

    // ------------------------------------------------------------------------------------------ //
    // Mutation

    /// Sets a value at the given path, creating intermediate nodes as needed.
    ///
    /// String paths are parsed as dotted paths (e.g., `"server.port"` navigates to `server` then `port`).
    pub fn set_value(
        &mut self,
        path: impl TryIntoKeyPath,
        value: impl IntoValue,
    ) -> Result<(), NodeError> {
        let kp = path.try_into_key_path()?;
        self.set_value_recursive(&kp.segs, value.into_value())
    }

    fn set_value_recursive(&mut self, segs: &[Segment], value: Value) -> Result<(), NodeError> {
        if segs.is_empty() {
            self.kind = Kind::Leaf(Leaf::new(value));
            return Ok(());
        }

        let (first, rest) = segs.split_first().unwrap();

        match first {
            Segment::Key(key) => {
                // Ensure we have a map
                if !self.kind.is_map() {
                    self.kind = Kind::Map(IndexMap::new());
                }

                if let Kind::Map(map) = &mut self.kind {
                    let child = map.entry(key.clone()).or_insert_with(|| {
                        Node::new_map(self.path.clone().push_key(key), IndexMap::new())
                    });
                    child.set_value_recursive(rest, value)?;
                }
            }
            Segment::Index(idx) => {
                if let Kind::Vec(vec) = &mut self.kind {
                    if *idx >= vec.len() {
                        return Err(NodeError::index_out_of_bounds(&self.path, *idx, vec.len()));
                    }
                    vec[*idx].set_value_recursive(rest, value)?;
                } else {
                    return Err(NodeError::kind_mismatch(&self.path, "array", &self.kind));
                }
            }
        }

        Ok(())
    }

    /// Sets a node at the given path, creating intermediate nodes as needed.
    ///
    /// String paths are parsed as dotted paths (e.g., `"server.port"` navigates to `server` then `port`).
    pub fn set_node(
        &mut self,
        path: impl TryIntoKeyPath,
        value_node: Node,
    ) -> Result<(), NodeError> {
        let kp = path.try_into_key_path().map_err(NodeError::invalid_path)?;
        if kp.segs.is_empty() {
            self.kind = value_node.kind;
            return Ok(());
        }
        self.set_node_recursive(&kp.segs, value_node)
    }

    fn set_node_recursive(&mut self, segs: &[Segment], value_node: Node) -> Result<(), NodeError> {
        let (first, rest) = segs.split_first().unwrap();

        match first {
            Segment::Key(key) => {
                // Ensure we have a map
                if !self.kind.is_map() {
                    self.kind = Kind::Map(IndexMap::new());
                }

                if let Kind::Map(map) = &mut self.kind {
                    if rest.is_empty() {
                        // Final segment - insert the value node with correct path
                        let mut new_node = value_node;
                        new_node.path = self.path.push_key(key);
                        // Recursively fix paths in children
                        new_node.fix_paths();
                        map.insert(key.clone(), new_node);
                    } else {
                        // Intermediate segment - recurse
                        let child = map.entry(key.clone()).or_insert_with(|| {
                            Node::new_map(self.path.push_key(key), IndexMap::new())
                        });
                        child.set_node_recursive(rest, value_node)?;
                    }
                }
            }
            Segment::Index(idx) => {
                if let Kind::Vec(vec) = &mut self.kind {
                    if *idx >= vec.len() {
                        return Err(NodeError::index_out_of_bounds(&self.path, *idx, vec.len()));
                    }

                    if rest.is_empty() {
                        let mut new_node = value_node;
                        new_node.path = self.path.push_index(*idx);
                        new_node.fix_paths();
                        vec[*idx] = new_node;
                    } else {
                        vec[*idx].set_node_recursive(rest, value_node)?;
                    }
                } else {
                    return Err(NodeError::kind_mismatch(&self.path, "array", &self.kind));
                }
            }
        }

        Ok(())
    }

    /// Appends an element to the array node at the given path.
    ///
    /// Navigates to the node at `path` and pushes `element` onto it. The target node
    /// must be an array; returns a type mismatch error otherwise. The path must exist -
    /// missing intermediate nodes are not created.
    pub fn push_to(&mut self, path: impl TryIntoKeyPath, element: Node) -> Result<(), NodeError> {
        let kp = path.try_into_key_path().map_err(NodeError::invalid_path)?;
        if kp.segs.is_empty() {
            self.push_element(element)
        } else {
            self.push_to_recursive(&kp.segs, element)
        }
    }

    fn push_to_recursive(&mut self, segs: &[Segment], element: Node) -> Result<(), NodeError> {
        let (first, rest) = segs.split_first().unwrap();

        match first {
            Segment::Key(key) => {
                if let Kind::Map(map) = &mut self.kind {
                    let child = map
                        .get_mut(key)
                        .ok_or_else(|| NodeError::missing_required(&self.path.push_key(key)))?;
                    if rest.is_empty() {
                        child.push_element(element)
                    } else {
                        child.push_to_recursive(rest, element)
                    }
                } else {
                    Err(NodeError::kind_mismatch(&self.path, "map", &self.kind))
                }
            }
            Segment::Index(idx) => {
                if let Kind::Vec(vec) = &mut self.kind {
                    let len = vec.len();
                    let child = vec
                        .get_mut(*idx)
                        .ok_or_else(|| NodeError::index_out_of_bounds(&self.path, *idx, len))?;
                    if rest.is_empty() {
                        child.push_element(element)
                    } else {
                        child.push_to_recursive(rest, element)
                    }
                } else {
                    Err(NodeError::kind_mismatch(&self.path, "array", &self.kind))
                }
            }
        }
    }

    /// Pushes an element onto this node, which must be a vec.
    fn push_element(&mut self, element: Node) -> Result<(), NodeError> {
        if let Kind::Vec(vec) = &mut self.kind {
            let mut el = element;
            el.path = self.path.push_index(vec.len());
            el.fix_paths();
            vec.push(el);
            Ok(())
        } else {
            Err(NodeError::kind_mismatch(&self.path, "array", &self.kind))
        }
    }

    /// Recursively updates paths in children after the node is moved.
    fn fix_paths(&mut self) {
        match &mut self.kind {
            Kind::Leaf(_) => {}
            Kind::Vec(vec) => {
                for (i, child) in vec.iter_mut().enumerate() {
                    child.path = self.path.push_index(i);
                    child.fix_paths();
                }
            }
            Kind::Map(map) => {
                for (key, child) in map.iter_mut() {
                    child.path = self.path.push_key(key);
                    child.fix_paths();
                }
            }
        }
    }

    // ------------------------------------------------------------------------------------------ //
    // Removal

    /// Removes a node at the given path.
    ///
    /// String paths are parsed as dotted paths (e.g., `"server.port"` navigates to `server` then `port`).
    ///
    /// Returns `Removed` if the node was deleted, `NotFound` if the path didn't exist.
    /// Returns `Err` for structural mismatches or attempting to remove the root.
    ///
    /// **Note:** Empty containers are NOT pruned. Removing the last element from an array
    /// leaves an empty array; removing the last key from a map leaves an empty map.
    pub fn remove(&mut self, path: impl TryIntoKeyPath) -> Result<RemoveOutcome, NodeError> {
        let kp = path.try_into_key_path().map_err(NodeError::invalid_path)?;
        if kp.is_empty() {
            return Err(NodeError::cannot_remove_root());
        }
        self.remove_recursive(&kp.segs)
    }

    fn remove_recursive(&mut self, segs: &[Segment]) -> Result<RemoveOutcome, NodeError> {
        let (first, rest) = segs.split_first().unwrap();

        if rest.is_empty() {
            // Final segment - perform the removal
            match (first, &mut self.kind) {
                (Segment::Key(key), Kind::Map(map)) => {
                    if map.shift_remove(key).is_some() {
                        Ok(RemoveOutcome::Removed)
                    } else {
                        Ok(RemoveOutcome::NotFound)
                    }
                }
                (Segment::Index(idx), Kind::Vec(vec)) => {
                    if *idx < vec.len() {
                        vec.remove(*idx);
                        // Fix paths for remaining elements after the removed index
                        for (i, child) in vec.iter_mut().enumerate().skip(*idx) {
                            child.path = self.path.push_index(i);
                            child.fix_paths();
                        }
                        Ok(RemoveOutcome::Removed)
                    } else {
                        Ok(RemoveOutcome::NotFound)
                    }
                }
                (Segment::Key(key), Kind::Vec(_)) => Err(NodeError::key_on_array(&self.path, key)),
                (Segment::Index(idx), Kind::Map(_)) => {
                    Err(NodeError::index_on_map(&self.path, *idx))
                }
                (_, Kind::Leaf(leaf)) => {
                    Err(NodeError::descend_into_leaf(&self.path, leaf.value.type_name()))
                }
            }
        } else {
            // Recurse to the child
            match (first, &mut self.kind) {
                (Segment::Key(key), Kind::Map(map)) => match map.get_mut(key) {
                    Some(child) => child.remove_recursive(rest),
                    None => Ok(RemoveOutcome::NotFound),
                },
                (Segment::Index(idx), Kind::Vec(vec)) => match vec.get_mut(*idx) {
                    Some(child) => child.remove_recursive(rest),
                    None => Ok(RemoveOutcome::NotFound),
                },
                (Segment::Key(key), Kind::Vec(_)) => Err(NodeError::key_on_array(&self.path, key)),
                (Segment::Index(idx), Kind::Map(_)) => {
                    Err(NodeError::index_on_map(&self.path, *idx))
                }
                (_, Kind::Leaf(leaf)) => {
                    // Null is treated as absence
                    if matches!(leaf.value, Value::Null) {
                        Ok(RemoveOutcome::NotFound)
                    } else {
                        Err(NodeError::descend_into_leaf(&self.path, leaf.value.type_name()))
                    }
                }
            }
        }
    }

    // ------------------------------------------------------------------------------------------ //
    // Validation

    /// Fails if the node contains unknown config keys.
    ///
    /// "Unknown" means leaf values present in the parsed config that were never read (visited)
    /// while converting the node into a typed config (e.g., via `FromNode` / `Config` derive).
    /// This catches typos and stale keys early.
    pub fn ensure_no_unknown_keys(&self) -> Result<(), NodeError> {
        let mut paths = Vec::new();
        self.collect_unknown_keys(&mut paths);

        if paths.is_empty() {
            Ok(())
        } else {
            Err(NodeError::unknown_keys(&paths))
        }
    }

    fn collect_unknown_keys(&self, out: &mut Vec<KeyPath>) {
        match &self.kind {
            Kind::Leaf(leaf) => {
                if !leaf.is_visited() {
                    out.push(self.path.clone());
                }
            }
            Kind::Vec(vec) => {
                for child in vec {
                    child.collect_unknown_keys(out);
                }
            }
            Kind::Map(map) => {
                for child in map.values() {
                    child.collect_unknown_keys(out);
                }
            }
        }
    }

    // ------------------------------------------------------------------------------------------ //
    // Serialization

    /// Serializes this Node tree using a writer resolved through the given registry.
    pub fn to_format_string(
        &self,
        format_id: impl Into<Format>,
        registry: &FormatWriterRegistry,
    ) -> Result<String, NodeError> {
        let format_id = format_id.into();
        let writer = registry.writer_by_id(&format_id).ok_or_else(|| {
            let supported =
                registry.supported_format_ids().iter().map(|id| id.as_str()).collect::<Vec<_>>();
            NodeError::from(FormatError::unknown_format_id(
                FormatUsage::Output,
                format_id.as_str(),
                &supported,
            ))
        })?;
        writer.write_node(self)
    }

    /// Serializes this Node tree using a writer from the default writer registry.
    pub fn to_string_as(&self, format_id: impl Into<Format>) -> Result<String, NodeError> {
        let registry = default_format_writer_registry();
        self.to_format_string(format_id, &registry)
    }

    /// Converts this Node tree to flat key=value format.
    ///
    /// Each leaf value is output on its own line as `path.to.key = "value"`.
    /// Empty containers produce no output.
    pub fn to_flat_string(&self) -> String {
        self.to_flat_string_with_config(FlatConfig::default())
    }

    /// Converts this Node tree to flat format with custom configuration.
    pub fn to_flat_string_with_config(&self, config: FlatConfig) -> String {
        let mut w = FlatWriter::with_config(config);
        w.write(self);
        w.into_string()
    }

    /// Converts this Node tree to visual tree format.
    ///
    /// Produces a human-readable tree with visual markers.
    pub fn to_tree_string(&self) -> String {
        self.to_tree_string_with_config(TreeConfig::default())
    }

    /// Converts this Node tree to visual tree format with custom configuration.
    pub fn to_tree_string_with_config(&self, config: TreeConfig) -> String {
        let mut w = TreeWriter::with_config(config);
        w.write(self);
        w.into_string()
    }

    /// Converts this Node tree to a Rhai Dynamic value.
    pub fn to_rhai_dynamic(&self) -> rhai::Dynamic {
        format::to_rhai_dynamic(self)
    }
}

// ---------------------------------------------------------------------------------------------- //
// Default

impl Default for Node {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------------------------- //
// Kind

/// The structural variant of a node: leaf value, array, or map.
#[derive(Debug, Clone)]
pub enum Kind {
    Leaf(Leaf),
    Vec(Vec<Node>),
    Map(IndexMap<String, Node>),
}

impl Kind {
    /// Returns true if this is a leaf node.
    pub fn is_leaf(&self) -> bool {
        matches!(self, Kind::Leaf(_))
    }

    /// Returns true if this is an array node.
    pub fn is_vec(&self) -> bool {
        matches!(self, Kind::Vec(_))
    }

    /// Returns true if this is a map node.
    pub fn is_map(&self) -> bool {
        matches!(self, Kind::Map(_))
    }
}

// ---------------------------------------------------------------------------------------------- //
// Leaf

/// A leaf node containing a value and visit-tracking state.
///
/// The visited flag is used by [`Node::ensure_no_unknown_keys`] to detect
/// config keys that were never read during parsing.
#[derive(Debug, Clone)]
pub struct Leaf {
    pub value: Value,
    visited: Rc<RefCell<bool>>,
}

impl Leaf {
    /// Creates a new leaf with the given value, initially unvisited.
    pub fn new(value: Value) -> Self {
        Self {
            value,
            visited: Rc::new(RefCell::new(false)),
        }
    }

    /// Returns true if this leaf has been visited (read).
    pub fn is_visited(&self) -> bool {
        *self.visited.borrow()
    }

    /// Marks this leaf as visited.
    pub fn mark_visited(&self) {
        *self.visited.borrow_mut() = true;
    }
}

// ---------------------------------------------------------------------------------------------- //
// RemoveOutcome

/// The outcome of a [`Node::remove`] operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoveOutcome {
    /// The node was successfully removed.
    Removed,
    /// The path did not exist (no change made).
    NotFound,
}

// ---------------------------------------------------------------------------------------------- //
// Tests

#[cfg(all(test, feature = "format-json"))]
mod tests {
    use super::*;

    /// Parses a JSON string into a Node for testing.
    fn node_from_json(s: &str) -> Node {
        Node::parse_str(s, Format::Json).expect("valid JSON test input")
    }

    // ------------------------------------------------------------------------------------------ //
    // Traversal

    mod traversal {
        use super::*;

        const TEST_JSON: &str = r#"{
            "server": {
                "port": 8080,
                "tls": null
            },
            "items": [{"name": "a"}, {"name": "b"}]
        }"#;

        #[test]
        fn missing_path_opt_returns_none() {
            let node = node_from_json(TEST_JSON);
            assert!(node.opt::<u16>("server.missing").unwrap().is_none());
        }

        #[test]
        fn missing_path_req_returns_err() {
            let node = node_from_json(TEST_JSON);
            assert!(node.req::<u16>("server.missing").is_err());
        }

        #[test]
        fn null_path_opt_returns_none() {
            let node = node_from_json(TEST_JSON);
            assert!(node.opt_node("server.tls").unwrap().is_none());
        }

        #[test]
        fn null_path_req_returns_err() {
            let node = node_from_json(TEST_JSON);
            assert!(node.req_node("server.tls").is_err());
        }

        #[test]
        fn key_lookup_into_array_errors() {
            let node = node_from_json(TEST_JSON);
            assert!(node.req_node("items.name").is_err());
        }

        #[test]
        fn index_lookup_into_map_errors() {
            let node = node_from_json(TEST_JSON);
            assert!(node.req_node("server[0]").is_err());
        }

        #[test]
        fn descend_into_leaf_errors() {
            let node = node_from_json(TEST_JSON);
            assert!(node.req_node("server.port.value").is_err());
        }
    }

    // ------------------------------------------------------------------------------------------ //
    // Unknown Keys

    mod unknown_keys {
        use super::*;

        #[test]
        fn null_accessed_via_opt_does_not_cause_unknown_key_error() {
            // If a key exists with value null, and code reads it via opt_*,
            // it must not be reported as an unknown key.
            let node = node_from_json(r#"{"tls": null, "port": 8080}"#);

            // Access both keys - tls via opt (returns None), port via req
            assert!(node.opt_node("tls").unwrap().is_none());
            let _port: u16 = node.req("port").unwrap();

            // Should pass because both keys were accessed
            assert!(node.ensure_no_unknown_keys().is_ok());
        }
    }

    // ------------------------------------------------------------------------------------------ //
    // Remove

    mod remove {
        use super::*;

        #[test]
        fn remove_map_key() {
            let mut node = node_from_json(r#"{"a": {"b": 1, "c": 2}}"#);
            assert_eq!(node.remove("a.b").unwrap(), RemoveOutcome::Removed);
            assert!(node.req::<i32>("a.b").is_err());
            assert_eq!(node.req::<i32>("a.c").unwrap(), 2);
        }

        #[test]
        fn remove_missing_key_returns_not_found() {
            let mut node = node_from_json(r#"{"a": {"b": 1}}"#);
            assert_eq!(node.remove("a.nope").unwrap(), RemoveOutcome::NotFound);
        }

        #[test]
        fn remove_last_key_leaves_empty_map() {
            let mut node = node_from_json(r#"{"a": {"b": 1}}"#);
            assert_eq!(node.remove("a.b").unwrap(), RemoveOutcome::Removed);
            // Map should still exist but be empty
            let a = node.req_node("a").unwrap();
            assert_eq!(a.as_map().unwrap().len(), 0);
        }

        #[test]
        fn remove_vec_index_shifts_elements() {
            let mut node =
                node_from_json(r#"{"items": [{"name": "a"}, {"name": "b"}, {"name": "c"}]}"#);
            assert_eq!(node.remove("items[1]").unwrap(), RemoveOutcome::Removed);
            // Elements should have shifted
            assert_eq!(node.req::<String>("items[0].name").unwrap(), "a");
            assert_eq!(node.req::<String>("items[1].name").unwrap(), "c"); // was [2], now [1]
        }

        #[test]
        fn remove_last_vec_element_leaves_empty_vec() {
            let mut node = node_from_json(r#"{"items": [{"name": "a"}]}"#);
            assert_eq!(node.remove("items[0]").unwrap(), RemoveOutcome::Removed);
            // Vec should still exist but be empty
            let items = node.req_node("items").unwrap();
            assert_eq!(items.as_vec().unwrap().len(), 0);
        }
    }

    // ------------------------------------------------------------------------------------------ //
    // Set Value

    mod set_value {
        use super::*;

        #[test]
        fn set_existing_element_succeeds() {
            let mut node = node_from_json(r#"{"items": [10, 20]}"#);
            node.set_value("items[1]", 99).unwrap();
            assert_eq!(node.req::<i32>("items[1]").unwrap(), 99);
        }

        #[test]
        fn set_out_of_bounds_on_empty_array_errors() {
            let mut node = node_from_json(r#"{"items": []}"#);
            let err = node.set_value("items[0]", 1).unwrap_err();
            assert!(err.to_string().contains("out of bounds"));
        }

        #[test]
        fn set_out_of_bounds_at_len_errors() {
            let mut node = node_from_json(r#"{"items": [10]}"#);
            let err = node.set_value("items[1]", 20).unwrap_err();
            assert!(err.to_string().contains("out of bounds"));
        }

        #[test]
        fn set_nested_out_of_bounds_errors() {
            let mut node = node_from_json(r#"{"items": [{"x": 1}]}"#);
            let err = node.set_value("items[1].x", 2).unwrap_err();
            assert!(err.to_string().contains("out of bounds"));
        }
    }

    // ------------------------------------------------------------------------------------------ //
    // Push To

    mod push_to {
        use super::*;

        #[test]
        fn push_to_existing_vec() {
            let mut node = node_from_json(r#"{"items": ["a", "b"]}"#);
            let element = Node::new_leaf(KeyPath::new(), Value::String("c".to_string()));
            node.push_to("items", element).unwrap();
            assert_eq!(node.req::<String>("items[2]").unwrap(), "c");
        }

        #[test]
        fn push_to_empty_vec() {
            let mut node = node_from_json(r#"{"items": []}"#);
            let element = Node::new_leaf(KeyPath::new(), Value::String("a".to_string()));
            node.push_to("items", element).unwrap();
            assert_eq!(node.req::<String>("items[0]").unwrap(), "a");
        }

        #[test]
        fn push_to_non_vec_errors() {
            let mut node = node_from_json(r#"{"items": "not_a_vec"}"#);
            let element = Node::new_leaf(KeyPath::new(), Value::String("a".to_string()));
            let err = node.push_to("items", element).unwrap_err();
            assert!(
                err.to_string().contains("expected array"),
                "expected 'expected array' in: {err}"
            );
        }

        #[test]
        fn push_to_missing_path_errors() {
            let mut node = node_from_json(r#"{}"#);
            let element = Node::new_leaf(KeyPath::new(), Value::String("a".to_string()));
            let err = node.push_to("items", element).unwrap_err();
            assert!(err.to_string().contains("missing"), "expected 'missing' in: {err}");
        }

        #[test]
        fn push_to_fixes_element_path() {
            let mut node = node_from_json(r#"{"items": ["a"]}"#);
            let element = Node::new_leaf(KeyPath::new(), Value::String("b".to_string()));
            node.push_to("items", element).unwrap();

            // Verify the pushed element has the correct path.
            let items = node.req_node("items").unwrap();
            let children = items.as_vec().unwrap();
            assert_eq!(children[1].path.to_string(), "items[1]");
        }
    }

    // ------------------------------------------------------------------------------------------ //
    // Rhai Relative Imports

    mod rhai_imports {
        use super::*;
        use std::fs;
        use tempfile::TempDir;

        #[test]
        fn parse_file_supports_relative_import() {
            let temp = TempDir::new().unwrap();

            // Create a utility module
            let util = temp.path().join("util.rhai");
            fs::write(&util, r#"fn get_port() { 8080 }"#).unwrap();

            // Create main config that imports the utility
            let config = temp.path().join("config.rhai");
            fs::write(&config, r#"import "util.rhai" as util; #{ port: util::get_port() }"#)
                .unwrap();

            // Parse should succeed with relative import resolution
            let node = Node::parse_file(&config).unwrap();
            assert_eq!(node.req::<i64>("port").unwrap(), 8080);
        }
    }
}
