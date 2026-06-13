//! Default-override annotations for `--get` tree output.
//!
//! `--get` renders the user-supplied input tree. This module adds one thing on top: when a
//! displayed leaf value shadows a known default with a *different* value, the default is appended
//! in brackets, for example `auto_open true [false]`. A value equal to its default, or a default
//! the user did not set, carries no suffix - `--desc` is where every default is listed.

use crate::key_path::{KeyPath, Segment};
use crate::node::{Kind, Node};
use crate::writer::{format_value, TreeAnnotation, TreeConfig, TreeWriter};
use crate::{DefaultNode, NodeError, ToNode};

// ---------------------------------------------------------------------------------------------- //

/// Renders an input subtree as a tree, annotating leaves that override a default.
///
/// `node` is the subtree of the input tree to render (its stored path must be the absolute path
/// at which it sits, so descendant paths resolve correctly). `config` is the parsed config; its
/// `ToNode` form supplies the typed values used for comparison, and its [`DefaultNode`] baseline
/// supplies the defaults. A suffix is added only when both lookups land on leaves and the typed
/// value differs from the default.
pub fn render_with_default_annotations<T>(
    node: &Node,
    config: &T,
    tree_config: &TreeConfig,
) -> Result<String, NodeError>
where
    T: ToNode + DefaultNode,
{
    let typed = config.to_node()?;
    let baseline = T::default_node()?;

    let annotate = |path: &KeyPath, entry: &Node| -> TreeAnnotation {
        let suffix = default_override_suffix(path, entry, &typed, baseline.as_ref());
        TreeAnnotation { suffix }
    };

    let mut writer = TreeWriter::with_config(tree_config.clone());
    writer.write_annotated(node, &annotate);
    Ok(writer.into_string())
}

/// Computes the ` [<default>]` suffix for a rendered entry, when it overrides a leaf default.
///
/// Returns `None` (no annotation) unless the entry is a leaf, the baseline has a leaf at the
/// same path, and the typed value differs from that default. The comparison uses the typed value
/// (`config.to_node()`), not the raw input: a `--set` override enters the input as a string, so
/// comparing the input directly would misjudge a CLI value that equals its default.
fn default_override_suffix(
    path: &KeyPath,
    entry: &Node,
    typed: &Node,
    baseline: Option<&Node>,
) -> Option<String> {
    // Only leaves carry inline suffixes; container defaults don't render on one line.
    if !matches!(entry.kind, Kind::Leaf(_)) {
        return None;
    }

    let baseline = lookup(baseline?, path)?;
    let Kind::Leaf(default_leaf) = &baseline.kind else {
        return None; // The default at this path is a container; skip inline annotation.
    };

    let typed_at = lookup(typed, path)?;
    if typed_at.same_value(baseline) {
        return None; // The user supplied the default value; nothing to flag.
    }

    Some(format!(" [{}]", format_value(&default_leaf.value)))
}

/// Walks a node by path segments, without null-as-absence or visited marking.
///
/// Stepping by segment makes the lookup independent of the nodes' stored paths, so it works on
/// freshly serialized `ToNode`/`DefaultNode` trees whose child paths are not fixed up.
fn lookup<'a>(node: &'a Node, path: &KeyPath) -> Option<&'a Node> {
    let mut current = node;
    for seg in path.iter() {
        match (seg, &current.kind) {
            (Segment::Key(key), Kind::Map(map)) => current = map.get(key)?,
            (Segment::Index(idx), Kind::Vec(vec)) => current = vec.get(*idx)?,
            _ => return None,
        }
    }
    Some(current)
}
