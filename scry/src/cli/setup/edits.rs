//! Collects command-line edits and applies them to configuration nodes.

use clap::parser::ValueSource;
use clap::ArgMatches;
use indexmap::IndexMap;

use super::error::SetupError;
use super::expose_map::{ExposeEntry, ExposeKind, ExposeMap, ValueShape};
use crate::desc::DescPathError;
use crate::node::{Node, RemoveOutcome, Value};
use crate::{Describe, KeyPath};

// ---------------------------------------------------------------------------------------------- //

/// One configuration edit in command-line order.
pub(super) enum Edit {
    Set { path: String, value: Node },
    Remove { path: String },
    Append { path: String, value: Node },
}

/// Collects input edits after the support switch in argument order.
pub(super) fn collect_support(matches: &ArgMatches) -> Result<Vec<Edit>, SetupError> {
    let mut edits = Vec::new();
    for occurrence in argument_occurrences(matches, "set")? {
        let [(_, key), (index, value)] = occurrence.values.as_slice() else {
            return Err(invalid_matches("set", "expected a key and value"));
        };
        edits.push((
            *index,
            Edit::Set {
                path: (*key).to_owned(),
                value: node_from_arg_str(value),
            },
        ));
    }
    for occurrence in argument_occurrences(matches, "remove")? {
        for (index, path) in occurrence.values {
            edits.push((
                index,
                Edit::Remove {
                    path: path.to_owned(),
                },
            ));
        }
    }
    Ok(in_argument_order(edits))
}

/// Collects exposed edits in application argument order.
pub(super) fn collect_exposed(
    expose: &ExposeMap,
    matches: &ArgMatches,
) -> Result<Vec<Edit>, SetupError> {
    let mut edits = Vec::new();
    for entry in &expose.entries {
        collect_exposed_ops(entry, matches, &mut edits)?;
    }
    Ok(in_argument_order(edits))
}

/// Applies configuration edits in their collected order.
pub(super) fn apply<T: Describe>(node: &mut Node, edits: Vec<Edit>) -> Result<(), SetupError> {
    for edit in edits {
        match edit {
            Edit::Set { path, value } => {
                let key_path: KeyPath =
                    path.parse().map_err(|source| SetupError::KeyPath { source })?;
                node.set_node(key_path, value)?;
            }
            Edit::Remove { path } => {
                if node.remove(&path)? == RemoveOutcome::NotFound {
                    let mut message = format!("cannot remove '{path}': path does not exist");
                    if let Err(DescPathError::UnknownPath(error)) =
                        T::describe().validate_path(&path)
                    {
                        message.push_str(&format!("\n\n{error}"));
                    }
                    return Err(SetupError::RemoveNotFound { message });
                }
            }
            Edit::Append { path, value } => {
                let key_path: KeyPath =
                    path.parse().map_err(|source| SetupError::KeyPath { source })?;
                match node.opt_node(&key_path)? {
                    None => {
                        node.set_node(key_path, Node::new_vec(KeyPath::new(), vec![value]))?;
                    }
                    Some(existing) if existing.kind.is_vec() => {
                        node.push_to(key_path, value)?;
                    }
                    Some(_) => return Err(SetupError::AppendToNonArray { path }),
                }
            }
        }
    }
    Ok(())
}

fn collect_exposed_ops(
    entry: &ExposeEntry,
    matches: &ArgMatches,
    ops: &mut Vec<(usize, Edit)>,
) -> Result<(), SetupError> {
    let name = entry.arg_name();
    if !is_command_line(matches, &name)? {
        return Ok(());
    }

    let path = match &entry.kind {
        ExposeKind::Fixed { assignments } => {
            let present = matches
                .try_get_one::<bool>(&name)
                .map_err(|error| invalid_matches(&name, error.to_string()))?
                .ok_or_else(|| invalid_matches(&name, "missing flag value"))?;
            if *present {
                let index = matches
                    .indices_of(&name)
                    .and_then(Iterator::last)
                    .ok_or_else(|| invalid_matches(&name, "missing flag position"))?;
                for assignment in assignments {
                    ops.push((
                        index,
                        Edit::Set {
                            path: assignment.path.clone(),
                            value: wrap_variant(entry, assignment.value.clone()),
                        },
                    ));
                }
            }
            return Ok(());
        }
        ExposeKind::Option { path } | ExposeKind::Positional { path } => path,
    };

    let occurrences = argument_occurrences(matches, &name)?;
    if matches!(entry.kind, ExposeKind::Positional { .. }) && entry.shape == ValueShape::Array {
        if let Some(last) = occurrences.last() {
            let index = last.completion_index();
            let children = occurrences
                .into_iter()
                .flat_map(|o| o.values)
                .map(|(_, value)| node_from_arg_str(value))
                .collect();
            let value = wrap_variant(entry, Node::new_vec(KeyPath::new(), children));
            ops.push((
                index,
                Edit::Set {
                    path: path.clone(),
                    value,
                },
            ));
        }
        return Ok(());
    }

    for occurrence in occurrences {
        if entry.shape == ValueShape::Array {
            let index = occurrence.completion_index();
            let children =
                occurrence.values.into_iter().map(|(_, value)| node_from_arg_str(value)).collect();
            let value = wrap_variant(entry, Node::new_vec(KeyPath::new(), children));
            let op = if entry.append {
                Edit::Append {
                    path: path.clone(),
                    value,
                }
            } else {
                Edit::Set {
                    path: path.clone(),
                    value,
                }
            };
            ops.push((index, op));
        } else {
            if !entry.append && occurrence.values.len() != 1 {
                return Err(invalid_matches(&name, "expected one scalar value"));
            }
            for (index, value) in occurrence.values {
                let value = wrap_variant(entry, node_from_arg_str(value));
                let op = if entry.append {
                    Edit::Append {
                        path: path.clone(),
                        value,
                    }
                } else {
                    Edit::Set {
                        path: path.clone(),
                        value,
                    }
                };
                ops.push((index, op));
            }
        }
    }
    Ok(())
}

struct Occurrence<'a> {
    values: Vec<(usize, &'a str)>,
}

impl Occurrence<'_> {
    fn completion_index(&self) -> usize {
        self.values.last().expect("collected occurrences are nonempty").0
    }
}

fn in_argument_order(mut edits: Vec<(usize, Edit)>) -> Vec<Edit> {
    edits.sort_by_key(|(index, _)| *index);
    edits.into_iter().map(|(_, edit)| edit).collect()
}

fn argument_occurrences<'a>(
    matches: &'a ArgMatches,
    name: &str,
) -> Result<Vec<Occurrence<'a>>, SetupError> {
    if !is_command_line(matches, name)? {
        return Ok(Vec::new());
    }
    let occurrences = matches
        .try_get_occurrences::<String>(name)
        .map_err(|error| invalid_matches(name, error.to_string()))?
        .ok_or_else(|| invalid_matches(name, "missing command-line values"))?;
    let mut indices =
        matches.indices_of(name).ok_or_else(|| invalid_matches(name, "missing value positions"))?;
    let mut result = Vec::new();
    for occurrence in occurrences {
        let mut values = Vec::new();
        for value in occurrence {
            let index = indices
                .next()
                .ok_or_else(|| invalid_matches(name, "fewer positions than values"))?;
            values.push((index, value.as_str()));
        }
        if values.is_empty() {
            return Err(invalid_matches(name, "empty value occurrences are unsupported"));
        }
        result.push(Occurrence { values });
    }
    if indices.next().is_some() {
        return Err(invalid_matches(name, "more positions than values"));
    }
    Ok(result)
}

fn is_command_line(matches: &ArgMatches, name: &str) -> Result<bool, SetupError> {
    matches.try_contains_id(name).map_err(|error| invalid_matches(name, error.to_string()))?;
    Ok(matches.value_source(name) == Some(ValueSource::CommandLine))
}

fn invalid_matches(name: &str, message: impl Into<String>) -> SetupError {
    SetupError::InvalidCliMatches {
        argument: name.to_owned(),
        message: message.into(),
    }
}

/// Converts a CLI string argument into a leaf Node.
fn node_from_arg_str(value_str: &str) -> Node {
    Node::new_leaf(KeyPath::new(), Value::String(value_str.to_string()))
}

/// Wraps an entry's computed value into its declared variant map, or returns it unchanged.
///
/// The wrapped form (`#{ key: value }`) is Scry's standard enum serialization, and the op that
/// carries it targets the enum field itself, so `set_node` replaces the previous node wholesale -
/// selecting one arm can never graft a second key into an arm the config already holds.
fn wrap_variant(entry: &ExposeEntry, value: Node) -> Node {
    match &entry.variant {
        Some(key) => {
            let mut map = IndexMap::new();
            map.insert(key.clone(), value);
            Node::new_map(KeyPath::new(), map)
        }
        None => value,
    }
}
