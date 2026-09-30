//! Override arguments specification (--set, --remove).
//!
//! These arguments modify config values before the final config is processed.

use clap::parser::ValueSource;
use clap::{Arg, ArgAction, ArgMatches, Command};
use indexmap::IndexMap;

use super::error::SetupError;
use super::expose_map::{ExposeEntry, ExposeKind, ExposeMap, ValueShape};
use crate::desc::DescPathError;
use crate::node::Value;
use crate::node::{Node, RemoveOutcome};
use crate::{Describe, KeyPath};

// ---------------------------------------------------------------------------------------------- //

/// Specifies which override options to enable for config modification.
///
/// Each field controls whether a specific override argument is enabled:
/// - `set`: Enable `--set KEY VALUE` for overriding config values
/// - `remove`: Enable `--remove KEY` for removing config values
///
/// Use [`OverrideArgs::standard()`] for the default set of options, or construct
/// directly with public fields.
#[derive(Default, Clone)]
pub struct OverrideArgs {
    /// Configuration for --set argument.
    pub set: Option<ArgConfig>,
    /// Configuration for --remove argument.
    pub remove: Option<ArgConfig>,
}

impl OverrideArgs {
    /// Creates an empty specification.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the standard set of override option names.
    pub fn standard() -> Self {
        Self::new().set("set", None).remove("remove", None)
    }

    /// Enables --set KEY VALUE for overriding config values.
    ///
    /// `VALUE` is a literal string, including `null`, rather than a config expression.
    pub fn set(mut self, long: impl Into<String>, c: Option<char>) -> Self {
        self.set = Some(ArgConfig {
            long: long.into(),
            short: c,
        });
        self
    }

    /// Enables --remove KEY for removing config values.
    ///
    /// Removal makes the field missing, allowing typed conversion to apply its fallback.
    /// A missing target is an error. Removing a present null succeeds.
    pub fn remove(mut self, long: impl Into<String>, c: Option<char>) -> Self {
        self.remove = Some(ArgConfig {
            long: long.into(),
            short: c,
        });
        self
    }

    /// Returns all configured option names for collision detection.
    pub fn all_names(&self) -> impl Iterator<Item = &str> {
        [
            self.set.as_ref().map(|a| a.long.as_str()),
            self.remove.as_ref().map(|a| a.long.as_str()),
        ]
        .into_iter()
        .flatten()
    }

    /// Returns all configured short flags for collision detection.
    pub fn all_shorts(&self) -> impl Iterator<Item = char> + '_ {
        [
            self.set.as_ref().and_then(|a| a.short),
            self.remove.as_ref().and_then(|a| a.short),
        ]
        .into_iter()
        .flatten()
    }

    /// Adds override arguments to a clap Command (--set, --remove).
    ///
    /// These arguments provide config modification capabilities.
    pub fn augment(&self, mut cmd: Command) -> Command {
        const SUPPORT_HEADING: &str = "Config options";

        // --set KEY VALUE
        if let Some(set_arg) = &self.set {
            let long = set_arg.long.clone();
            let mut arg = Arg::new(long.clone())
                .long(long)
                .num_args(2)
                .value_names(["KEY", "VALUE"])
                .action(ArgAction::Append)
                .help_heading(SUPPORT_HEADING)
                .help("Sets a config value (can be repeated).");

            if let Some(short) = set_arg.short {
                arg = arg.short(short);
            }

            cmd = cmd.arg(arg);
        }

        // --remove KEY
        if let Some(remove_arg) = &self.remove {
            let long = remove_arg.long.clone();
            let mut arg = Arg::new(long.clone())
                .long(long)
                .num_args(1)
                .value_name("KEY")
                .action(ArgAction::Append)
                .help_heading(SUPPORT_HEADING)
                .help("Removes a config value (can be repeated).");

            if let Some(short) = remove_arg.short {
                arg = arg.short(short);
            }

            cmd = cmd.arg(arg);
        }

        cmd
    }

    /// Collects and applies all config overrides in argv order.
    ///
    /// This includes:
    /// - `--set KEY VALUE` occurrences
    /// - `--remove KEY` occurrences
    /// - Exposed options, flags, and presets
    ///
    /// Scalar values use their individual parser indices. Grouped values are assigned when their
    /// last value is consumed. All runs of a positional array form one assignment at its final
    /// value, even when other options appear between those runs.
    pub fn apply<T: Describe>(
        &self,
        node: &mut Node,
        expose_map: &ExposeMap,
        matches: &ArgMatches,
    ) -> Result<(), SetupError> {
        let mut ops: Vec<(usize, Op)> = Vec::new();

        if let Some(set_arg) = &self.set {
            for occurrence in argument_occurrences(matches, &set_arg.long)? {
                let [(_, key), (index, value)] = occurrence.values.as_slice() else {
                    return Err(invalid_matches(&set_arg.long, "expected a key and value"));
                };
                ops.push((
                    *index,
                    Op::Set {
                        path: (*key).to_owned(),
                        value: node_from_arg_str(value),
                    },
                ));
            }
        }

        if let Some(remove_arg) = &self.remove {
            for occurrence in argument_occurrences(matches, &remove_arg.long)? {
                for (index, path) in occurrence.values {
                    ops.push((
                        index,
                        Op::Remove {
                            path: path.to_owned(),
                        },
                    ));
                }
            }
        }

        for entry in &expose_map.entries {
            collect_exposed_ops(entry, matches, &mut ops)?;
        }

        // Sort by argv index.
        ops.sort_by_key(|(idx, _)| *idx);

        // Apply in order.
        for (_, op) in ops {
            match op {
                Op::Set { path, value } => {
                    let key_path: KeyPath =
                        path.parse().map_err(|e| SetupError::KeyPath { source: e })?;
                    node.set_node(key_path, value)?;
                }
                Op::Remove { path } => {
                    let outcome = node.remove(&path)?;
                    if outcome == RemoveOutcome::NotFound {
                        let mut message = format!("cannot remove '{path}': path does not exist");
                        let desc = T::describe();
                        if let Err(DescPathError::UnknownPath(upe)) = desc.validate_path(&path) {
                            message.push_str(&format!("\n\n{upe}"));
                        }
                        return Err(SetupError::RemoveNotFound { message });
                    }
                }
                Op::Append { path, value } => {
                    let key_path: KeyPath =
                        path.parse().map_err(|e| SetupError::KeyPath { source: e })?;
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
}

// ---------------------------------------------------------------------------------------------- //

/// Configuration for a single override argument.
#[derive(Clone)]
pub struct ArgConfig {
    /// The long flag name (without --).
    pub long: String,
    /// Optional short flag character.
    pub short: Option<char>,
}

enum Op {
    Set { path: String, value: Node },
    Remove { path: String },
    Append { path: String, value: Node },
}

fn collect_exposed_ops(
    entry: &ExposeEntry,
    matches: &ArgMatches,
    ops: &mut Vec<(usize, Op)>,
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
                        Op::Set {
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
                Op::Set {
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
                Op::Append {
                    path: path.clone(),
                    value,
                }
            } else {
                Op::Set {
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
                    Op::Append {
                        path: path.clone(),
                        value,
                    }
                } else {
                    Op::Set {
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
