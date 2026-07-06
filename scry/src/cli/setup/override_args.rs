//! Override arguments specification (--set, --remove).
//!
//! These arguments modify config values before the final config is processed.

use clap::{Arg, ArgAction, ArgMatches, Command};
use indexmap::IndexMap;

use super::error::SetupError;
use super::expose_map::{ExposeEntry, ExposeKind, ExposeMap};
use crate::desc::DescPathError;
use crate::node::Value;
use crate::node::{Node, RemoveOutcome};
use crate::{Describe, KeyPath};

// ---------------------------------------------------------------------------------------------- //

/// Configuration for a single override argument.
#[derive(Clone)]
pub struct ArgConfig {
    /// The long flag name (without --).
    pub long: String,
    /// Optional short flag character.
    pub short: Option<char>,
}

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

// ---------------------------------------------------------------------------------------------- //

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
    pub fn set(mut self, long: impl Into<String>, c: Option<char>) -> Self {
        self.set = Some(ArgConfig {
            long: long.into(),
            short: c,
        });
        self
    }

    /// Enables --remove KEY for removing config values.
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
                .help("Sets a config value (can be repeated)");

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
                .help("Removes a config value (can be repeated)");

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
    /// - Exposed options and flags
    ///
    /// All operations are sorted by their argv index and applied in that order,
    /// ensuring command-line arguments work as users expect (later args override earlier ones).
    pub fn apply<T: Describe>(
        &self,
        node: &mut Node,
        expose_map: &ExposeMap,
        matches: &ArgMatches,
    ) -> Result<(), SetupError> {
        // A config modification operation with its argv index for ordering.
        enum Op {
            Set { path: String, value: Node },
            Remove { path: String },
            Append { path: String, value: Node },
        }
        let mut ops: Vec<(usize, Op)> = Vec::new();

        // Collect --set operations.
        if let Some(set_arg) = &self.set {
            if let Some(occurrences) = matches.get_occurrences::<String>(&set_arg.long) {
                // Get indices for each occurrence.
                // For --set which takes 2 values, indices_of returns indices for each value.
                // We take every other index (the first value of each pair).
                let indices: Vec<usize> =
                    matches.indices_of(&set_arg.long).map(|i| i.collect()).unwrap_or_default();

                for (i, occurrence) in occurrences.enumerate() {
                    let values: Vec<_> = occurrence.collect();
                    let key_str = values[0];
                    let value_str = values[1];
                    let value_node = node_from_arg_str(value_str);

                    // For --set with 2 values per occurrence, take every other index (0, 2, 4, ...).
                    let idx = indices.get(i * 2).copied().unwrap_or(0);
                    ops.push((
                        idx,
                        Op::Set {
                            path: key_str.clone(),
                            value: value_node,
                        },
                    ));
                }
            }
        }

        // Collect --remove operations.
        if let Some(remove_arg) = &self.remove {
            if let Some(keys) = matches.get_many::<String>(&remove_arg.long) {
                let indices: Vec<usize> =
                    matches.indices_of(&remove_arg.long).map(|i| i.collect()).unwrap_or_default();

                for (i, key_str) in keys.enumerate() {
                    let idx = indices.get(i).copied().unwrap_or(0);
                    ops.push((
                        idx,
                        Op::Remove {
                            path: key_str.clone(),
                        },
                    ));
                }
            }
        }

        // Collect exposed field operations.
        for entry in &expose_map.entries {
            let arg_name = entry.arg_name();

            match &entry.kind {
                ExposeKind::Flag { value } => {
                    if matches.get_flag(&arg_name) {
                        if let Some(idx) = matches.indices_of(&arg_name).and_then(Iterator::last) {
                            ops.push((
                                idx,
                                Op::Set {
                                    path: entry.path.clone(),
                                    value: wrap_variant(entry, value.clone()),
                                },
                            ));
                        }
                    }
                }
                ExposeKind::Option | ExposeKind::Positional => {
                    if let Some(value_str) = matches.get_one::<String>(&arg_name) {
                        let value_node = node_from_arg_str(value_str);
                        if let Some(idx) = matches.indices_of(&arg_name).and_then(Iterator::last) {
                            ops.push((
                                idx,
                                Op::Set {
                                    path: entry.path.clone(),
                                    value: wrap_variant(entry, value_node),
                                },
                            ));
                        }
                    }
                }
                ExposeKind::List => {
                    if let Some(values) = matches.get_many::<String>(&arg_name) {
                        let indices: Vec<usize> =
                            matches.indices_of(&arg_name).map(|i| i.collect()).unwrap_or_default();
                        for (i, value_str) in values.enumerate() {
                            let idx = indices.get(i).copied().unwrap_or(0);
                            let value_node = node_from_arg_str(value_str);
                            ops.push((
                                idx,
                                Op::Append {
                                    path: entry.path.clone(),
                                    value: value_node,
                                },
                            ));
                        }
                    }
                }
            }
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
                        Some(_) => {
                            node.push_to(key_path, value)?;
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------- //

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
