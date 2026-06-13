//! Query arguments specification (--desc, --get, --get-flat, --get-as).
//!
//! These arguments provide read-only operations that output information and exit early.
//! The `--get` family renders the user-supplied input tree. Tree output additionally annotates
//! leaves whose value overrides a default (see [`render_with_default_annotations`]); flat and
//! format output render the input subtree as-is.

use clap::{Arg, ArgMatches, Command};

use super::error::SetupError;
use super::get_annotations::render_with_default_annotations;
use crate::desc::DescPathError;
use crate::node::Format;
use crate::writer::TreeConfig;
use crate::{DefaultNode, Describe, KeyPath, Node, ToNode};

// ---------------------------------------------------------------------------------------------- //

/// Configuration for a single query argument.
#[derive(Clone)]
pub struct ArgConfig {
    /// The long flag name (without --).
    pub long: String,
    /// Optional short flag character.
    pub short: Option<char>,
}

// ---------------------------------------------------------------------------------------------- //

/// Specifies which query options to enable for config inspection.
///
/// Each field controls whether a specific query argument is enabled:
/// - `desc`: Enable `--desc [KEY]` for printing config descriptions
/// - `get`: Enable `--get [KEY]` for printing config in tree format
/// - `get_flat`: Enable `--get-flat [KEY]` for printing as flat key=value lines
/// - `get_as`: Enable `--get-as FORMAT [KEY]` for writer-registry output
///
/// Use [`QueryArgs::standard()`] for the default set of options, or construct
/// directly with public fields.
#[derive(Default, Clone)]
pub struct QueryArgs {
    /// Configuration for --desc argument.
    pub desc: Option<ArgConfig>,
    /// Configuration for --get argument.
    pub get: Option<ArgConfig>,
    /// Configuration for --get-flat argument.
    pub get_flat: Option<ArgConfig>,
    /// Configuration for --get-as argument.
    pub get_as: Option<ArgConfig>,
}

// ---------------------------------------------------------------------------------------------- //

impl QueryArgs {
    /// Creates an empty specification.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the standard set of query option names.
    pub fn standard() -> Self {
        Self::new()
            .desc("desc", None)
            .get("get", None)
            .get_flat("get-flat", None)
            .get_as("get-as", None)
    }

    /// Enables `--desc [KEY]` for printing configuration description.
    pub fn desc(mut self, long: impl Into<String>, c: Option<char>) -> Self {
        self.desc = Some(ArgConfig {
            long: long.into(),
            short: c,
        });
        self
    }

    /// Enables `--get [KEY]` for printing config values in tree format.
    pub fn get(mut self, long: impl Into<String>, c: Option<char>) -> Self {
        self.get = Some(ArgConfig {
            long: long.into(),
            short: c,
        });
        self
    }

    /// Enables `--get-flat [KEY]` for printing config values as flat key-value lines.
    pub fn get_flat(mut self, long: impl Into<String>, c: Option<char>) -> Self {
        self.get_flat = Some(ArgConfig {
            long: long.into(),
            short: c,
        });
        self
    }

    /// Enables `--get-as <FORMAT> [KEY]` for printing config in a specific output format.
    pub fn get_as(mut self, long: impl Into<String>, c: Option<char>) -> Self {
        self.get_as = Some(ArgConfig {
            long: long.into(),
            short: c,
        });
        self
    }

    /// Returns all configured option names for collision detection.
    pub fn all_names(&self) -> impl Iterator<Item = &str> {
        [
            self.desc.as_ref().map(|a| a.long.as_str()),
            self.get.as_ref().map(|a| a.long.as_str()),
            self.get_flat.as_ref().map(|a| a.long.as_str()),
            self.get_as.as_ref().map(|a| a.long.as_str()),
        ]
        .into_iter()
        .flatten()
    }

    /// Returns all configured short flags for collision detection.
    pub fn all_shorts(&self) -> impl Iterator<Item = char> + '_ {
        [
            self.desc.as_ref().and_then(|a| a.short),
            self.get.as_ref().and_then(|a| a.short),
            self.get_flat.as_ref().and_then(|a| a.short),
            self.get_as.as_ref().and_then(|a| a.short),
        ]
        .into_iter()
        .flatten()
    }

    /// Adds query arguments to a clap Command (--desc, --get, etc.).
    ///
    /// These arguments provide config inspection capabilities.
    pub fn augment(&self, mut cmd: Command) -> Command {
        const SUPPORT_HEADING: &str = "Config options";

        // --desc [KEY] (first in help)
        if let Some(desc_arg) = &self.desc {
            let long = desc_arg.long.clone();
            let mut arg = Arg::new(long.clone())
                .long(long)
                .num_args(0..=1)
                .value_name("KEY")
                .default_missing_value("")
                .help_heading(SUPPORT_HEADING)
                .help("Prints config description and exits (no key = full description)");

            if let Some(short) = desc_arg.short {
                arg = arg.short(short);
            }

            cmd = cmd.arg(arg);
        }

        // --get [KEY] (tree format output)
        if let Some(get_arg) = &self.get {
            let long = get_arg.long.clone();
            let mut arg = Arg::new(long.clone())
                .long(long)
                .num_args(0..=1)
                .value_name("KEY")
                .default_missing_value("")
                .help_heading(SUPPORT_HEADING)
                .help("Prints config value and exits (no key = whole config)");

            if let Some(short) = get_arg.short {
                arg = arg.short(short);
            }

            cmd = cmd.arg(arg);
        }

        // --get-flat [KEY] (flat key=value format)
        if let Some(get_flat_arg) = &self.get_flat {
            let long = get_flat_arg.long.clone();
            let mut arg = Arg::new(long.clone())
                .long(long)
                .num_args(0..=1)
                .value_name("KEY")
                .default_missing_value("")
                .help_heading(SUPPORT_HEADING)
                .help("Prints config as flat key=value lines");

            if let Some(short) = get_flat_arg.short {
                arg = arg.short(short);
            }

            cmd = cmd.arg(arg);
        }

        // --get-as <FORMAT> [KEY] (format id output)
        if let Some(get_as_arg) = &self.get_as {
            let supported = crate::node::default_format_writer_registry()
                .supported_format_ids()
                .iter()
                .map(|id| id.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            let long = get_as_arg.long.clone();
            let mut arg = Arg::new(long.clone())
                .long(long)
                .num_args(1..=2)
                .value_names(["FORMAT", "KEY"])
                .help_heading(SUPPORT_HEADING)
                .help(format!(
                    "Prints config in specified format ({})",
                    if supported.is_empty() {
                        "<none>"
                    } else {
                        supported.as_str()
                    }
                ));

            if let Some(short) = get_as_arg.short {
                arg = arg.short(short);
            }

            cmd = cmd.arg(arg);
        }

        cmd
    }

    /// Handles parse-free `--get-flat` and `--get-as` requests against the raw input tree.
    ///
    /// These formats render the selected input subtree verbatim, so they need neither a typed
    /// parse nor `ToNode`/`DefaultNode` - they work even on a config that fails to evaluate.
    /// Returns `Ok(None)` for a tree `--get` request (deferred to [`check_get_annotated`] after
    /// parsing) or when no get request is present.
    pub fn check_get_raw<T>(
        &self,
        input_node: &Node,
        matches: &ArgMatches,
    ) -> Result<Option<String>, SetupError>
    where
        T: Describe,
    {
        let Some((path, format)) = self.requested_get(matches)? else {
            return Ok(None); // no get request found
        };
        if matches!(format, GetFormat::Tree) {
            return Ok(None); // deferred to the annotated path, after parsing
        }
        Ok(Some(format_raw_at_path::<T>(input_node, &path, format)?))
    }

    /// Handles the tree `--get` request, annotating leaves that override a default.
    ///
    /// Tree output draws the typed values and default baseline from the parsed `config`, so it
    /// only runs after a successful parse. Returns `Ok(None)` for `--get-flat` / `--get-as`
    /// (handled by [`check_get_raw`]) or when no get request is present.
    pub fn check_get_annotated<T>(
        &self,
        input_node: &Node,
        config: &T,
        matches: &ArgMatches,
    ) -> Result<Option<String>, SetupError>
    where
        T: Describe + ToNode + DefaultNode,
    {
        let Some((path, format)) = self.requested_get(matches)? else {
            return Ok(None); // no get request found
        };
        if !matches!(format, GetFormat::Tree) {
            return Ok(None); // handled before parsing by check_get_raw
        }
        Ok(Some(format_tree_at_path::<T>(input_node, config, &path)?))
    }

    /// Extracts the requested get operation (path and format) from the matches, if any.
    fn requested_get(
        &self,
        matches: &ArgMatches,
    ) -> Result<Option<(String, GetFormat)>, SetupError> {
        // Handle 'get'
        if let Some(arg) = &self.get {
            if let Some(path) = matches.get_one::<String>(&arg.long) {
                return Ok(Some((path.clone(), GetFormat::Tree)));
            }
        }

        // Handle 'get-flat'
        if let Some(arg) = &self.get_flat {
            if let Some(path) = matches.get_one::<String>(&arg.long) {
                return Ok(Some((path.clone(), GetFormat::Flat)));
            }
        }

        // Handle 'get-as'
        if let Some(arg) = &self.get_as {
            if let Some(values) = matches.get_many::<String>(&arg.long) {
                let values: Vec<_> = values.collect();
                let format_str = values[0].as_str();
                let format_id =
                    format_str.parse::<Format>().map_err(|_| SetupError::UnknownGetAsFormat {
                        format_str: format_str.to_string(),
                    })?;
                let writer_registry = crate::node::default_format_writer_registry();
                if writer_registry.writer_by_id(&format_id).is_none() {
                    return Err(SetupError::UnknownGetAsFormat {
                        format_str: format_str.to_string(),
                    });
                }
                let path = if values.len() > 1 {
                    values[1].to_string()
                } else {
                    "".to_string()
                };
                return Ok(Some((path, GetFormat::Format(format_id))));
            }
        }

        Ok(None)
    }

    /// Checks for --desc argument and returns formatted output.
    pub fn check_desc<T>(&self, matches: &ArgMatches) -> Result<Option<String>, SetupError>
    where
        T: Describe,
    {
        let Some(arg) = &self.desc else {
            return Ok(None); // 'desc' support not enabled
        };

        let Some(path) = matches.get_one::<String>(&arg.long) else {
            return Ok(None); // no 'desc' arg provided
        };

        let desc = T::describe();
        let output = if path.is_empty() {
            desc.display().to_string()
        } else {
            // Validate path first to get helpful error if invalid.
            desc.validate_path(path).map_err(|e| SetupError::DescPath { source: e })?;
            let entry =
                desc.entry_at_path(path).expect("entry_at_path should succeed after validate_path");
            entry.display().to_string()
        };
        Ok(Some(output))
    }
}

// ---------------------------------------------------------------------------------------------- //

/// Output format for --get operations.
#[derive(Clone)]
pub enum GetFormat {
    /// Tree-structured output.
    Tree,
    /// Flat key=value output.
    Flat,
    /// Format-specific serialized output.
    Format(Format),
}

// ---------------------------------------------------------------------------------------------- //
// Output Formatting

/// Selects an input subtree and renders it raw, for `--get-flat` / `--get-as`.
///
/// Renders the input verbatim with no annotations and no typed parse. Panics if called with
/// [`GetFormat::Tree`]; tree output goes through [`format_tree_at_path`].
pub fn format_raw_at_path<T>(
    input_node: &Node,
    path: &str,
    format: GetFormat,
) -> Result<String, SetupError>
where
    T: Describe,
{
    let (_, target) = select_path::<T>(input_node, path)?;

    match format {
        GetFormat::Flat => Ok(target.to_flat_string()),
        GetFormat::Format(format_id) => {
            let registry = crate::node::default_format_writer_registry();
            Ok(target.to_format_string(format_id, &registry)?)
        }
        GetFormat::Tree => unreachable!("tree output is handled by format_tree_at_path"),
    }
}

/// Selects an input subtree and renders it as a tree, annotating overridden defaults.
pub fn format_tree_at_path<T>(
    input_node: &Node,
    config: &T,
    path: &str,
) -> Result<String, SetupError>
where
    T: Describe + ToNode + DefaultNode,
{
    let (base, target) = select_path::<T>(input_node, path)?;
    // Render from a root whose path is the absolute base, so the annotator's path lookups into
    // the typed-config and baseline trees resolve correctly.
    let mut root = target.clone();
    root.path = base;
    Ok(render_with_default_annotations(&root, config, &TreeConfig::default())?)
}

/// Resolves a key path against the input tree, returning the absolute path and the subtree.
///
/// An empty path selects the whole tree. A missing path yields a [`SetupError::GetNotFound`]
/// carrying the `--desc` hint for known schema paths.
fn select_path<'a, T: Describe>(
    node: &'a Node,
    path: &str,
) -> Result<(KeyPath, &'a Node), SetupError> {
    if path.is_empty() {
        return Ok((KeyPath::new(), node));
    }

    let key_path: KeyPath = path.parse().map_err(|e| SetupError::KeyPath { source: e })?;
    match node.opt_node(key_path.clone())? {
        Some(target) => Ok((key_path, target)),
        None => {
            let mut message = format!("cannot get '{path}': path does not exist");
            let desc = T::describe();
            if let Err(DescPathError::UnknownPath(upe)) = desc.validate_path(path) {
                message.push_str(&format!("\n\n{upe}"));
            }
            Err(SetupError::GetNotFound { message })
        }
    }
}
